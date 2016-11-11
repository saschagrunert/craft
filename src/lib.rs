//! Cargo inspired build system for C based projects
#![doc(html_root_url = "https://saschagrunert.github.io/craft/")]
// #![deny(missing_docs)]
#![deny(unused)]
#![cfg_attr(test, deny(warnings))]

#[cfg(test)]
extern crate hamcrest;

#[macro_use]
extern crate log;

extern crate crossbeam;
extern crate curl;
extern crate docopt;
extern crate filetime;
extern crate flate2;
extern crate fs2;
extern crate git2;
extern crate glob;
extern crate libc;
extern crate libgit2_sys;
extern crate num_cpus;
extern crate regex;
extern crate rustc_serialize;
extern crate semver;
extern crate tar;
extern crate tempdir;
extern crate term;
extern crate toml;
extern crate url;

use std::{env, io};
use rustc_serialize::{Decodable, Encodable, json};
use docopt::Docopt;

use shell::{Shell, MultiShell, ShellConfig, Verbosity, ColorConfig};
use shell::Verbosity::Verbose;
use shell::ColorConfig::Auto;
use term::color::BLACK;

pub use util::{CraftError, CraftResult, CliError, CliResult, human, Config, ChainError};

macro_rules! bail {
    ($($fmt:tt)*) => (
        return Err(::util::human(&format_args!($($fmt)*)))
    )
}

pub mod dependency;
pub mod manifest;
pub mod ops;
pub mod package;
pub mod package_id;
pub mod package_id_spec;
pub mod registry;
pub mod resolver;
pub mod shell;
pub mod source;
pub mod sources;
pub mod summary;
pub mod util;
pub mod workspace;

pub fn execute_main_without_stdin<T, V>(exec: fn(T, &Config) -> CliResult<Option<V>>, options_first: bool, usage: &str)
    where V: Encodable,
          T: Decodable
{
    process::<V, _>(|rest, config| call_main_without_stdin(exec, config, usage, rest, options_first));
}

pub fn call_main_without_stdin<T, V>(exec: fn(T, &Config) -> CliResult<Option<V>>,
                                     config: &Config,
                                     usage: &str,
                                     args: &[String],
                                     options_first: bool)
                                     -> CliResult<Option<V>>
    where V: Encodable,
          T: Decodable
{
    let flags = flags_from_args::<T>(usage, args, options_first)?;
    exec(flags, config)
}

fn process<V, F>(mut callback: F)
    where F: FnMut(&[String], &Config) -> CliResult<Option<V>>,
          V: Encodable
{
    let mut config = None;
    let result = (|| {
        config = Some(Config::default()?);
        let args: Vec<_> =
            try!(env::args_os().map(|s| s.into_string().map_err(|s| human(format!("invalid unicode in argument: {:?}", s))))
                .collect());
        callback(&args, config.as_ref().unwrap())
    })();
    let mut verbose_shell = shell(Verbose, Auto);
    let mut shell = config.as_ref().map(|s| s.shell());
    let shell = shell.as_mut().map(|s| &mut **s).unwrap_or(&mut verbose_shell);
    process_executed(result, shell)
}

pub fn process_executed<T>(result: CliResult<Option<T>>, shell: &mut MultiShell)
    where T: Encodable
{
    match result {
        Err(e) => handle_error(e, shell),
        Ok(Some(encodable)) => {
            let encoded = json::encode(&encodable).unwrap();
            println!("{}", encoded);
        }
        Ok(None) => {}
    }
}

pub fn shell(verbosity: Verbosity, color_config: ColorConfig) -> MultiShell {
    enum Output {
        Stdout,
        Stderr,
    }

    let tty = isatty(Output::Stderr);

    let config = ShellConfig {
        color_config: color_config,
        tty: tty,
    };

    let err = Shell::create(|| Box::new(io::stderr()), config);

    let tty = isatty(Output::Stdout);

    let config = ShellConfig {
        color_config: color_config,
        tty: tty,
    };

    let out = Shell::create(|| Box::new(io::stdout()), config);

    return MultiShell::new(out, err, verbosity);

    #[cfg(unix)]
    fn isatty(output: Output) -> bool {
        let fd = match output {
            Output::Stdout => libc::STDOUT_FILENO,
            Output::Stderr => libc::STDERR_FILENO,
        };

        unsafe { libc::isatty(fd) != 0 }
    }

    #[cfg(windows)]
    fn isatty(output: Output) -> bool {
        extern crate kernel32;
        extern crate winapi;

        let handle = match output {
            Output::Stdout => winapi::winbase::STD_OUTPUT_HANDLE,
            Output::Stderr => winapi::winbase::STD_ERROR_HANDLE,
        };

        unsafe {
            let handle = kernel32::GetStdHandle(handle);
            let mut out = 0;
            kernel32::GetConsoleMode(handle, &mut out) != 0
        }
    }
}

pub fn handle_error(err: CliError, shell: &mut MultiShell) {
    debug!("handle_error; err={:?}", err);

    let CliError { error, exit_code, unknown } = err;
    // exit_code == 0 is non-fatal error, e.g. docopt version info
    let fatal = exit_code != 0;

    let hide = unknown && shell.get_verbose() != Verbose;

    if let Some(error) = error {
        let _ignored_result = if hide {
            shell.error("An unknown error occurred")
        } else if fatal {
            shell.error(&error)
        } else {
            shell.say(&error, BLACK)
        };

        if !handle_cause(&error, shell) || hide {
            let _ = shell.err().say("\nTo learn more, run the command again with --verbose.".to_string(),
                                    BLACK);
        }
    }

    std::process::exit(exit_code);
}

fn handle_cause(mut craft_err: &CraftError, shell: &mut MultiShell) -> bool {
    let verbose = shell.get_verbose();
    let mut err;
    loop {
        craft_err = match craft_err.craft_cause() {
            Some(cause) => cause,
            None => {
                err = craft_err.cause();
                break;
            }
        };
        if verbose != Verbose && !craft_err.is_human() {
            return false;
        }
        print(craft_err.to_string(), shell);
    }
    loop {
        let cause = match err {
            Some(err) => err,
            None => return true,
        };
        if verbose != Verbose {
            return false;
        }
        print(cause.to_string(), shell);
        err = cause.cause();
    }

    fn print(error: String, shell: &mut MultiShell) {
        let _ = shell.err().say("\nCaused by:", BLACK);
        let _ = shell.err().say(format!("  {}", error), BLACK);
    }
}

pub fn version() -> String {
    format!("craft {}",
            match option_env!("CFG_VERSION") {
                Some(s) => s.to_string(),
                None => {
                    format!("{}.{}.{}{}",
                            env!("CARGO_PKG_VERSION_MAJOR"),
                            env!("CARGO_PKG_VERSION_MINOR"),
                            env!("CARGO_PKG_VERSION_PATCH"),
                            option_env!("CARGO_PKG_VERSION_PRE").unwrap_or(""))
                }
            })
}

fn flags_from_args<T>(usage: &str, args: &[String], options_first: bool) -> CliResult<T>
    where T: Decodable
{
    let docopt = Docopt::new(usage)
        .unwrap()
        .options_first(options_first)
        .argv(args.iter().map(|s| &s[..]))
        .help(true);
    docopt.decode().map_err(|e| {
        let code = if e.fatal() { 1 } else { 0 };
        CliError::new(human(e.to_string()), code)
    })
}
