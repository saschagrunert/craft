//! The top level executable
extern crate craft;
extern crate url;
extern crate env_logger;
extern crate git2_curl;
extern crate rustc_serialize;
extern crate toml;

#[macro_use]
extern crate log;

use std::{env, fs};
use std::path::{Path, PathBuf};
use std::collections::{BTreeSet, HashMap};

use craft::core::shell::Verbosity;
use craft::execute_main_without_stdin;
use craft::util::{self, CliResult, lev_distance, Config, human, CraftResult};
use craft::util::CliError;

#[derive(RustcDecodable)]
pub struct Flags {
    flag_list: bool,
    flag_version: bool,
    flag_verbose: u32,
    flag_quiet: Option<bool>,
    flag_color: Option<String>,
    arg_command: String,
    arg_args: Vec<String>,
    flag_locked: bool,
    flag_frozen: bool,
}

const USAGE: &'static str = "
Cargo inspired build system for C based projects

Usage:
    craft <command> [<args>...]
    craft [options]

Options:
    -h, --help          Display this message
    -V, --version       Print version info and exit
    --list              List installed commands
    -v, --verbose ...   Use verbose output
    -q, --quiet         No output printed to stdout
    --color WHEN        Coloring: auto, always, never
    --frozen            Require Craft.lock and cache are up to date
    --locked            Require Craft.lock is up to date

Some common craft commands are (see all commands with --list):
    build       Compile the current project
    clean       Remove the target directory
    doc         Build this project's and its dependencies' documentation
    new         Create a new craft project
    init        Create a new craft project in an existing directory
    run         Build and execute src/main.c
    test        Run the tests
    bench       Run the benchmarks
    update      Update dependencies listed in Craft.lock

See 'craft help <command>' for more information on a specific command.
";

fn main() {
    env_logger::init().unwrap();
    execute_main_without_stdin(execute, true, USAGE)
}

macro_rules! each_subcommand{
    ($mac:ident) => {
        $mac!(bench);
        $mac!(build);
        $mac!(clean);
        $mac!(doc);
        $mac!(fetch);
        $mac!(generate_lockfile);
        $mac!(git_checkout);
        $mac!(help);
        $mac!(init);
        $mac!(locate_project);
        $mac!(metadata);
        $mac!(new);
        $mac!(package);
        $mac!(pkgid);
        $mac!(run);
        $mac!(rustc);
        $mac!(rustdoc);
        $mac!(test);
        $mac!(update);
        $mac!(verify_project);
    }
}

macro_rules! declare_mod {
    ($name:ident) => ( pub mod $name; )
}
each_subcommand!(declare_mod);

fn execute(flags: Flags, config: &Config) -> CliResult<Option<()>> {
    try!(config.configure(flags.flag_verbose,
                          flags.flag_quiet,
                          &flags.flag_color,
                          flags.flag_frozen,
                          flags.flag_locked));

    init_git_transports(config);
    let _token = craft::util::job::setup();

    if flags.flag_version {
        println!("{}", craft::version());
        return Ok(None);
    }

    if flags.flag_list {
        println!("Installed Commands:");
        for command in list_commands(config) {
            println!("    {}", command);
        }
        return Ok(None);
    }

    let args = match &flags.arg_command[..] {
        // For the commands `craft` and `craft help`, re-execute ourselves as
        // `craft -h` so we can go through the normal process of printing the
        // help message.
        "" | "help" if flags.arg_args.is_empty() => {
            config.shell().set_verbosity(Verbosity::Verbose);
            let args = &["craft".to_string(), "-h".to_string()];
            let r = craft::call_main_without_stdin(execute, config, USAGE, args, false);
            craft::process_executed(r, &mut config.shell());
            return Ok(None);
        }

        // For `craft help -h` and `craft help --help`, print out the help
        // message for `craft help`
        "help" if flags.arg_args[0] == "-h" || flags.arg_args[0] == "--help" => {
            vec!["craft".to_string(), "help".to_string(), "-h".to_string()]
        }

        // For `craft help foo`, print out the usage message for the specified
        // subcommand by executing the command with the `-h` flag.
        "help" => vec!["craft".to_string(), flags.arg_args[0].clone(), "-h".to_string()],

        // For all other invocations, we're of the form `craft foo args...`. We
        // use the exact environment arguments to preserve tokens like `--` for
        // example.
        _ => {
            let mut default_alias = HashMap::new();
            default_alias.insert("b", "build".to_string());
            default_alias.insert("t", "test".to_string());
            default_alias.insert("r", "run".to_string());
            let mut args: Vec<String> = env::args().collect();
            if let Some(new_command) = default_alias.get(&args[1][..]) {
                args[1] = new_command.clone();
            }
            args
        }
    };

    if try_execute(&config, &args) {
        return Ok(None);
    }

    let alias_list = try!(aliased_command(&config, &args[1]));
    let args = match alias_list {
        Some(alias_command) => {
            let chain = args.iter()
                .take(1)
                .chain(alias_command.iter())
                .chain(args.iter().skip(2))
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            if try_execute(&config, &chain) {
                return Ok(None);
            } else {
                chain
            }
        }
        None => args,
    };
    try!(execute_subcommand(config, &args[1], &args));
    Ok(None)
}

fn try_execute(config: &Config, args: &[String]) -> bool {
    macro_rules! cmd {
        ($name:ident) => (if args[1] == stringify!($name).replace("_", "-") {
            config.shell().set_verbosity(Verbosity::Verbose);
            let r = craft::call_main_without_stdin($name::execute, config,
                                                   $name::USAGE,
                                                   &args,
                                                   false);
            craft::process_executed(r, &mut config.shell());
            return true
        })
    }
    each_subcommand!(cmd);

    return false;
}

fn aliased_command(config: &Config, command: &String) -> CraftResult<Option<Vec<String>>> {
    let alias_name = format!("alias.{}", command);
    let mut result = Ok(None);
    match config.get_string(&alias_name) {
        Ok(value) => {
            if let Some(record) = value {
                let alias_commands = record.val
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                result = Ok(Some(alias_commands));
            }
        }
        Err(_) => {
            let value = try!(config.get_list(&alias_name));
            if let Some(record) = value {
                let alias_commands: Vec<String> = record.val
                    .iter()
                    .map(|s| s.0.to_string())
                    .collect();
                result = Ok(Some(alias_commands));
            }
        }
    }
    result
}

fn find_closest(config: &Config, cmd: &str) -> Option<String> {
    let cmds = list_commands(config);
    // Only consider candidates with a lev_distance of 3 or less so we don't
    // suggest out-of-the-blue options.
    let mut filtered = cmds.iter()
        .map(|c| (lev_distance(&c, cmd), c))
        .filter(|&(d, _)| d < 4)
        .collect::<Vec<_>>();
    filtered.sort_by(|a, b| a.0.cmp(&b.0));
    filtered.get(0).map(|slot| slot.1.clone())
}

fn execute_subcommand(config: &Config, cmd: &str, args: &[String]) -> CliResult<()> {
    let command_exe = format!("craft-{}{}", cmd, env::consts::EXE_SUFFIX);
    let path = search_directories(config)
        .iter()
        .map(|dir| dir.join(&command_exe))
        .find(|file| is_executable(file));
    let command = match path {
        Some(command) => command,
        None => {
            return Err(human(match find_closest(config, cmd) {
                    Some(closest) => {
                        format!("no such subcommand: `{}`\n\n\tDid you mean `{}`?\n",
                                cmd,
                                closest)
                    }
                    None => format!("no such subcommand: `{}`", cmd),
                })
                .into())
        }
    };
    let err = match util::process(&command).args(&args[1..]).exec() {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };

    if let Some(code) = err.exit.as_ref().and_then(|c| c.code()) {
        Err(CliError::code(code))
    } else {
        Err(CliError::new(Box::new(err), 101))
    }
}

/// List all runnable commands. find_command should always succeed
/// if given one of returned command.
fn list_commands(config: &Config) -> BTreeSet<String> {
    let prefix = "craft-";
    let suffix = env::consts::EXE_SUFFIX;
    let mut commands = BTreeSet::new();
    for dir in search_directories(config) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            _ => continue,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let filename = match path.file_name().and_then(|s| s.to_str()) {
                Some(filename) => filename,
                _ => continue,
            };
            if !filename.starts_with(prefix) || !filename.ends_with(suffix) {
                continue;
            }
            if is_executable(entry.path()) {
                let end = filename.len() - suffix.len();
                commands.insert(filename[prefix.len()..end].to_string());
            }
        }
    }

    macro_rules! add_cmd {
        ($cmd:ident) => ({ commands.insert(stringify!($cmd).replace("_", "-")); })
    }
    each_subcommand!(add_cmd);
    commands
}

#[cfg(unix)]
fn is_executable<P: AsRef<Path>>(path: P) -> bool {
    use std::os::unix::prelude::*;
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable<P: AsRef<Path>>(path: P) -> bool {
    fs::metadata(path).map(|metadata| metadata.is_file()).unwrap_or(false)
}

fn search_directories(config: &Config) -> Vec<PathBuf> {
    let mut dirs = vec![config.home().clone().into_path_unlocked().join("bin")];
    if let Some(val) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&val));
    }
    dirs
}

fn init_git_transports(config: &Config) {
    use craft::sources::registry::remote::http_handle;
    let handle = match http_handle(config) {
        Ok(handle) => handle,
        Err(..) => return,
    };
    unsafe {
        git2_curl::register(handle);
    }
}
