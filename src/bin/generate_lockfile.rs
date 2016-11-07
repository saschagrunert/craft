use std::env;

use craft::core::Workspace;
use craft::ops;
use craft::util::{CliResult, Config};
use craft::util::important_paths::find_root_manifest_for_wd;

#[derive(RustcDecodable)]
pub struct Options {
    flag_manifest_path: Option<String>,
    flag_verbose: u32,
    flag_quiet: Option<bool>,
    flag_color: Option<String>,
    flag_frozen: bool,
    flag_locked: bool,
}

pub const USAGE: &'static str = "
Generate the lockfile for a project

Usage:
    craft generate-lockfile [options]

Options:
    -h, --help               Print this message
    --manifest-path PATH     Path to the manifest to generate a lockfile for
    -v, --verbose ...        Use verbose output
    -q, --quiet              No output printed to stdout
    --color WHEN             Coloring: auto, always, never
    --frozen                 Require Craft.lock and cache are up to date
    --locked                 Require Craft.lock is up to date
";

pub fn execute(options: Options, config: &Config) -> CliResult<Option<()>> {
    debug!("executing; cmd=craft-generate-lockfile; args={:?}", env::args().collect::<Vec<_>>());
    try!(config.configure(options.flag_verbose,
                          options.flag_quiet,
                          &options.flag_color,
                          options.flag_frozen,
                          options.flag_locked));
    let root = try!(find_root_manifest_for_wd(options.flag_manifest_path, config.cwd()));

    let ws = try!(Workspace::new(&root, config));
    try!(ops::generate_lockfile(&ws));
    Ok(None)
}
