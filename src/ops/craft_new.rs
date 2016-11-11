use std::{env, fs};
use std::path::Path;
use std::collections::BTreeMap;

use rustc_serialize::{Decodable, Decoder};
use git2::Config as GitConfig;

use util::{GitRepo, HgRepo, CraftResult, human, ChainError, internal, Config, paths};
use workspace::Workspace;

use toml;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VersionControl {
    Git,
    Hg,
    NoVcs,
}

pub struct NewOptions<'a> {
    pub version_control: Option<VersionControl>,
    pub bin: bool,
    pub lib: bool,
    pub path: &'a str,
    pub name: Option<&'a str>,
}

struct SourceFileInformation {
    relative_path: String,
    target_name: String,
    bin: bool,
}

struct MkOptions<'a> {
    version_control: Option<VersionControl>,
    path: &'a Path,
    name: &'a str,
    source_files: Vec<SourceFileInformation>,
    bin: bool,
}

impl Decodable for VersionControl {
    fn decode<D: Decoder>(d: &mut D) -> Result<VersionControl, D::Error> {
        Ok(match &d.read_str()?[..] {
            "git" => VersionControl::Git,
            "hg" => VersionControl::Hg,
            "none" => VersionControl::NoVcs,
            n => {
                let err = format!("could not decode '{}' as version control", n);
                return Err(d.error(&err));
            }
        })
    }
}

impl<'a> NewOptions<'a> {
    pub fn new(version_control: Option<VersionControl>,
               bin: bool,
               lib: bool,
               path: &'a str,
               name: Option<&'a str>)
               -> NewOptions<'a> {

        // default to lib
        let is_lib = if !bin { true } else { lib };

        NewOptions {
            version_control: version_control,
            bin: bin,
            lib: is_lib,
            path: path,
            name: name,
        }
    }
}

struct CraftNewConfig {
    name: Option<String>,
    email: Option<String>,
    version_control: Option<VersionControl>,
}

fn get_name<'a>(path: &'a Path, opts: &'a NewOptions) -> CraftResult<&'a str> {
    if let Some(name) = opts.name {
        return Ok(name);
    }

    if path.file_name().is_none() {
        bail!("cannot auto-detect project name from path {:?} ; use --name to override",
              path.as_os_str());
    }

    let dir_name = path.file_name()
        .and_then(|s| s.to_str())
        .chain_error(|| {
            human(&format!("cannot create a project with a non-unicode name: {:?}",
                           path.file_name().unwrap()))
        })?;

    Ok(dir_name)
}

fn check_name(name: &str) -> CraftResult<()> {

    // Ban keywords
    let blacklist = ["chest", "test", "true", "auto", "break", "case", "char", "const", "continue", "default", "do",
                     "double", "else", "enum", "extern", "float", "for", "goto", "if", "int", "long", "register",
                     "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef", "union",
                     "unsigned", "void", "volatile", "while"];
    if blacklist.contains(&name) {
        bail!("The name `{}` cannot be used as a chest name\n\
               use --name to override chest name",
              name)
    }

    for c in name.chars() {
        if c.is_alphanumeric() {
            continue;
        }
        if c == '_' || c == '-' {
            continue;
        }
        bail!("Invalid character `{}` in chest name: `{}`\n\
               use --name to override chest name",
              c,
              name)
    }
    Ok(())
}

fn detect_source_paths_and_types(project_path: &Path,
                                 project_name: &str,
                                 detected_files: &mut Vec<SourceFileInformation>)
                                 -> CraftResult<()> {
    let path = project_path;
    let name = project_name;

    enum H {
        Bin,
        Lib,
        Detect,
    }

    struct Test {
        proposed_path: String,
        handling: H,
    }

    let tests = vec![
        Test { proposed_path: format!("src/main.c"),     handling: H::Bin },
        Test { proposed_path: format!("main.c"),         handling: H::Bin },
        Test { proposed_path: format!("src/{}.c", name), handling: H::Detect },
        Test { proposed_path: format!("{}.c", name),     handling: H::Detect },
        Test { proposed_path: format!("src/lib.c"),      handling: H::Lib },
        Test { proposed_path: format!("lib.c"),          handling: H::Lib },
    ];

    for i in tests {
        let pp = i.proposed_path;

        // path/pp does not exist or is not a file
        if !fs::metadata(&path.join(&pp)).map(|x| x.is_file()).unwrap_or(false) {
            continue;
        }

        let sfi = match i.handling {
            H::Bin => {
                SourceFileInformation {
                    relative_path: pp,
                    target_name: project_name.to_string(),
                    bin: true,
                }
            }
            H::Lib => {
                SourceFileInformation {
                    relative_path: pp,
                    target_name: project_name.to_string(),
                    bin: false,
                }
            }
            H::Detect => {
                let content = paths::read(&path.join(pp.clone()))?;
                let isbin = content.contains("int main");
                SourceFileInformation {
                    relative_path: pp,
                    target_name: project_name.to_string(),
                    bin: isbin,
                }
            }
        };
        detected_files.push(sfi);
    }

    // Check for duplicate lib attempt
    let mut previous_lib_relpath: Option<&str> = None;
    let mut duplicates_checker: BTreeMap<&str, &SourceFileInformation> = BTreeMap::new();

    for i in detected_files {
        if i.bin {
            if let Some(x) = BTreeMap::get::<str>(&duplicates_checker, i.target_name.as_ref()) {
                bail!("\
multiple possible binary sources found:
  {}
  {}
cannot automatically generate Craft.toml as the main target would be ambiguous",
                      &x.relative_path,
                      &i.relative_path);
            }
            duplicates_checker.insert(i.target_name.as_ref(), i);
        } else {
            if let Some(plp) = previous_lib_relpath {
                return Err(human(format!("cannot have a project with multiple libraries, found both `{}` and `{}`",
                                         plp,
                                         i.relative_path)));
            }
            previous_lib_relpath = Some(&i.relative_path);
        }
    }

    Ok(())
}

fn plan_new_source_file(bin: bool, project_name: String) -> SourceFileInformation {
    if bin {
        SourceFileInformation {
            relative_path: "src/main.c".to_string(),
            target_name: project_name,
            bin: true,
        }
    } else {
        SourceFileInformation {
            relative_path: "src/lib.c".to_string(),
            target_name: project_name,
            bin: false,
        }
    }
}

pub fn new(opts: NewOptions, config: &Config) -> CraftResult<()> {
    let path = config.cwd().join(opts.path);
    if fs::metadata(&path).is_ok() {
        bail!("destination `{}` already exists", path.display())
    }

    if opts.lib && opts.bin {
        bail!("can't specify both lib and binary outputs");
    }

    let name = get_name(&path, &opts)?;
    check_name(name)?;

    let mkopts = MkOptions {
        version_control: opts.version_control,
        path: &path,
        name: name,
        source_files: vec![plan_new_source_file(opts.bin, name.to_string())],
        bin: opts.bin,
    };

    mk(config, &mkopts).chain_error(|| {
        human(format!("Failed to create project `{}` at `{}`",
                      name,
                      path.display()))
    })
}

pub fn init(opts: NewOptions, config: &Config) -> CraftResult<()> {
    let path = config.cwd().join(opts.path);

    let crafttoml_path = path.join("Craft.toml");
    if fs::metadata(&crafttoml_path).is_ok() {
        bail!("`craft init` cannot be run on existing Craft projects")
    }

    if opts.lib && opts.bin {
        bail!("can't specify both lib and binary outputs");
    }

    let name = get_name(&path, &opts)?;
    check_name(name)?;

    let mut src_paths_types = vec![];

    detect_source_paths_and_types(&path, name, &mut src_paths_types)?;

    if src_paths_types.len() == 0 {
        src_paths_types.push(plan_new_source_file(opts.bin, name.to_string()));
    } else {
        // --bin option may be ignored if lib.c or src/lib.c present
        // Maybe when doing `craft init --bin` inside a library project stub,
        // user may mean "initialize for library, but also add binary target"
    }

    let mut version_control = opts.version_control;

    if version_control == None {
        let mut num_detected_vsces = 0;

        if fs::metadata(&path.join(".git")).is_ok() {
            version_control = Some(VersionControl::Git);
            num_detected_vsces += 1;
        }

        if fs::metadata(&path.join(".hg")).is_ok() {
            version_control = Some(VersionControl::Hg);
            num_detected_vsces += 1;
        }

        // if none exists, maybe create git, like in `craft new`

        if num_detected_vsces > 1 {
            bail!("both .git and .hg directories found and the ignore file can't be filled in as a result, specify \
                   --vcs to override detection");
        }
    }

    let mkopts = MkOptions {
        version_control: version_control,
        path: &path,
        name: name,
        bin: src_paths_types.iter().any(|x| x.bin),
        source_files: src_paths_types,
    };

    mk(config, &mkopts).chain_error(|| {
        human(format!("Failed to create project `{}` at `{}`",
                      name,
                      path.display()))
    })
}

fn existing_vcs_repo(path: &Path, cwd: &Path) -> bool {
    GitRepo::discover(path, cwd).is_ok() || HgRepo::discover(path, cwd).is_ok()
}

fn mk(config: &Config, opts: &MkOptions) -> CraftResult<()> {
    let path = opts.path;
    let name = opts.name;
    let cfg = global_config(config)?;
    let mut ignore = "target\n".to_string();
    let in_existing_vcs_repo = existing_vcs_repo(path.parent().unwrap(), config.cwd());
    if !opts.bin {
        ignore.push_str("Craft.lock\n");
    }

    let vcs = match (opts.version_control, cfg.version_control, in_existing_vcs_repo) {
        (None, None, false) => VersionControl::Git,
        (None, Some(option), false) => option,
        (Some(option), _, _) => option,
        (_, _, true) => VersionControl::NoVcs,
    };

    match vcs {
        VersionControl::Git => {
            if !fs::metadata(&path.join(".git")).is_ok() {
                GitRepo::init(path, config.cwd())?;
            }
            paths::append(&path.join(".gitignore"), ignore.as_bytes())?;
        }
        VersionControl::Hg => {
            if !fs::metadata(&path.join(".hg")).is_ok() {
                HgRepo::init(path, config.cwd())?;
            }
            paths::append(&path.join(".hgignore"), ignore.as_bytes())?;
        }
        VersionControl::NoVcs => {
            fs::create_dir_all(path)?;
        }
    };

    let (author_name, email) = discover_author()?;
    // Hoo boy, sure glad we've got exhaustivenes checking behind us.
    let author = match (cfg.name, cfg.email, author_name, email) {
        (Some(name), Some(email), _, _) |
        (Some(name), None, _, Some(email)) |
        (None, Some(email), name, _) |
        (None, None, name, Some(email)) => format!("{} <{}>", name, email),
        (Some(name), None, _, None) |
        (None, None, name, None) => name,
    };

    let mut crafttoml_path_specifier = String::new();

    // Calculare what [lib] and [[bin]]s do we need to append to Craft.toml
    for i in &opts.source_files {
        if i.bin {
            if i.relative_path != "src/main.c" {
                crafttoml_path_specifier.push_str(&format!(r#"
[[bin]]
name = "{}"
path = {}
"#,
                                                           i.target_name,
                                                           toml::Value::String(i.relative_path.clone())));
            }
        } else if i.relative_path != "src/lib.c" {
            crafttoml_path_specifier.push_str(&format!(r#"
[lib]
name = "{}"
path = {}
"#,
                                                       i.target_name,
                                                       toml::Value::String(i.relative_path.clone())));
        }
    }

    // Create Craft.toml file with necessary [lib] and [[bin]] sections, if needed

    paths::write(&path.join("Craft.toml"),
                 format!(r#"[package]
name = "{}"
version = "0.1.0"
authors = [{}]

[dependencies]
{}"#,
                         name,
                         toml::Value::String(author),
                         crafttoml_path_specifier)
                     .as_bytes())?;


    // Create all specified source files
    // (with respective parent directories)
    // if they are don't exist

    for i in &opts.source_files {
        let path_of_source_file = path.join(i.relative_path.clone());

        if let Some(src_dir) = path_of_source_file.parent() {
            fs::create_dir_all(src_dir)?;
        }

        let default_file_content: &[u8] = if i.bin {
            b"\
#include <stdio.h>

int main(void) {
    printf(\"Hello, world!\");
}
"
        } else {
            b""
        };

        if !fs::metadata(&path_of_source_file).map(|x| x.is_file()).unwrap_or(false) {
            paths::write(&path_of_source_file, default_file_content)?;
        }
    }

    if let Err(e) = Workspace::new(&path.join("Craft.toml"), config) {
        let msg = format!("compiling this new chest may not work due to invalid workspace configuration\n\n{}",
                          e);
        config.shell().warn(msg)?;
    }

    Ok(())
}

fn get_environment_variable(variables: &[&str]) -> Option<String> {
    variables.iter()
        .filter_map(|var| env::var(var).ok())
        .next()
}

fn discover_author() -> CraftResult<(String, Option<String>)> {
    let git_config = GitConfig::open_default().ok();
    let git_config = git_config.as_ref();
    let name_variables = ["CRAFT_NAME", "GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME", "USER", "USERNAME", "NAME"];
    let name = get_environment_variable(&name_variables[0..3])
        .or_else(|| git_config.and_then(|g| g.get_string("user.name").ok()))
        .or_else(|| get_environment_variable(&name_variables[3..]));

    let name = match name {
        Some(name) => name,
        None => {
            let username_var = if cfg!(windows) { "USERNAME" } else { "USER" };
            bail!("could not determine the current user, please set ${}",
                  username_var)
        }
    };
    let email_variables = ["CRAFT_EMAIL", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL", "EMAIL"];
    let email = get_environment_variable(&email_variables[0..3])
        .or_else(|| git_config.and_then(|g| g.get_string("user.email").ok()))
        .or_else(|| get_environment_variable(&email_variables[3..]));

    let name = name.trim().to_string();
    let email = email.map(|s| s.trim().to_string());

    Ok((name, email))
}

fn global_config(config: &Config) -> CraftResult<CraftNewConfig> {
    let name = config.get_string("craft-new.name")?.map(|s| s.val);
    let email = config.get_string("craft-new.email")?.map(|s| s.val);
    let vcs = config.get_string("craft-new.vcs")?;

    let vcs = match vcs.as_ref().map(|p| (&p.val[..], &p.definition)) {
        Some(("git", _)) => Some(VersionControl::Git),
        Some(("hg", _)) => Some(VersionControl::Hg),
        Some(("none", _)) => Some(VersionControl::NoVcs),
        Some((s, p)) => {
            return Err(internal(format!("invalid configuration for key `craft-new.vcs`, unknown vcs `{}` (found in \
                                         {})",
                                        s,
                                        p)))
        }
        None => None,
    };
    Ok(CraftNewConfig {
        name: name,
        email: email,
        version_control: vcs,
    })
}
