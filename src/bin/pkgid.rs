use craft::core::Workspace;
use craft::ops;
use craft::util::{CliResult, Config};
use craft::util::important_paths::{find_root_manifest_for_wd};

#[derive(RustcDecodable)]
pub struct Options {
    flag_verbose: u32,
    flag_quiet: Option<bool>,
    flag_color: Option<String>,
    flag_manifest_path: Option<String>,
    flag_frozen: bool,
    flag_locked: bool,
    flag_package: Option<String>,
    arg_spec: Option<String>,
}

pub const USAGE: &'static str = "
Print a fully qualified package specification

Usage:
    craft pkgid [options] [<spec>]

Options:
    -h, --help               Print this message
    -p SPEC, --package SPEC  Argument to get the package id specifier for
    --manifest-path PATH     Path to the manifest to the package to clean
    -v, --verbose ...        Use verbose output
    -q, --quiet              No output printed to stdout
    --color WHEN             Coloring: auto, always, never
    --frozen                 Require Craft.lock and cache are up to date
    --locked                 Require Craft.lock is up to date

Given a <spec> argument, print out the fully qualified package id specifier.
This command will generate an error if <spec> is ambiguous as to which package
it refers to in the dependency graph. If no <spec> is given, then the pkgid for
the local package is printed.

This command requires that a lockfile is available and dependencies have been
fetched.

Example Package IDs

           pkgid                  |  name  |  version  |          url
    |-----------------------------|--------|-----------|---------------------|
     foo                          | foo    | *         | *
     foo:1.2.3                    | foo    | 1.2.3     | *
     crates.io/foo                | foo    | *         | *://crates.io/foo
     crates.io/foo#1.2.3          | foo    | 1.2.3     | *://crates.io/foo
     crates.io/bar#foo:1.2.3      | foo    | 1.2.3     | *://crates.io/bar
     http://crates.io/foo#1.2.3   | foo    | 1.2.3     | http://crates.io/foo

";

pub fn execute(options: Options,
               config: &Config) -> CliResult<Option<()>> {
    try!(config.configure(options.flag_verbose,
                          options.flag_quiet,
                          &options.flag_color,
                          options.flag_frozen,
                          options.flag_locked));
    let root = try!(find_root_manifest_for_wd(options.flag_manifest_path.clone(), config.cwd()));
    let ws = try!(Workspace::new(&root, config));

    let spec = if options.arg_spec.is_some() {
        options.arg_spec
    } else if options.flag_package.is_some() {
        options.flag_package
    } else {
        None
    };
    let spec = spec.as_ref().map(|s| &s[..]);
    let spec = try!(ops::pkgid(&ws, spec));
    println!("{}", spec);
    Ok(None)
}

