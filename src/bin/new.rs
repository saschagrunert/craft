use std::env;

use craft::ops;
use craft::util::{CliResult, Config};

#[derive(RustcDecodable)]
pub struct Options {
    flag_verbose: u32,
    flag_quiet: Option<bool>,
    flag_color: Option<String>,
    flag_bin: bool,
    flag_lib: bool,
    arg_path: String,
    flag_name: Option<String>,
    flag_vcs: Option<ops::VersionControl>,
    flag_frozen: bool,
    flag_locked: bool,
}

pub const USAGE: &'static str = "
Create a new craft package at <path>

Usage:
    craft new [options] <path>
    craft new -h | --help

Options:
    -h, --help          Print this message
    --vcs VCS           Initialize a new repository for the given version
                        control system (git or hg) or do not initialize any version
                        control at all (none) overriding a global configuration.
    --bin               Use a binary (application) template
    --lib               Use a library template
    --name NAME         Set the resulting package name
    -v, --verbose ...   Use verbose output
    -q, --quiet         No output printed to stdout
    --color WHEN        Coloring: auto, always, never
    --frozen            Require Craft.lock and cache are up to date
    --locked            Require Craft.lock is up to date
";

pub fn execute(options: Options, config: &Config) -> CliResult<Option<()>> {
    debug!("executing; cmd=craft-new; args={:?}",
           env::args().collect::<Vec<_>>());
    config.configure(options.flag_verbose,
                          options.flag_quiet,
                          &options.flag_color,
                          options.flag_frozen,
                          options.flag_locked)?;

    let Options { flag_bin, flag_lib, arg_path, flag_name, flag_vcs, .. } = options;

    let opts = ops::NewOptions::new(flag_vcs,
                                    flag_bin,
                                    flag_lib,
                                    &arg_path,
                                    flag_name.as_ref().map(|s| s.as_ref()));

    let opts_lib = opts.lib;
    ops::new(opts, config)?;

    config.shell().status("Created",
                               format!("{} `{}` project",
                                       if opts_lib {
                                           "library"
                                       } else {
                                           "binary (application)"
                                       },
                                       arg_path))?;

    Ok(None)
}
