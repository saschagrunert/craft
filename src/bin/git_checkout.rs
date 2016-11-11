use craft::source::{Source, SourceId, GitReference};
use craft::sources::git::GitSource;
use craft::util::{Config, CliResult, ToUrl};

#[derive(RustcDecodable)]
pub struct Options {
    flag_url: String,
    flag_reference: String,
    flag_verbose: u32,
    flag_quiet: Option<bool>,
    flag_color: Option<String>,
    flag_frozen: bool,
    flag_locked: bool,
}

pub const USAGE: &'static str = "
Checkout a copy of a Git repository

Usage:
    craft git-checkout [options] --url=URL --reference=REF
    craft git-checkout -h | --help

Options:
    -h, --help               Print this message
    -v, --verbose ...        Use verbose output
    -q, --quiet              No output printed to stdout
    --color WHEN             Coloring: auto, always, never
    --frozen                 Require Craft.lock and cache are up to date
    --locked                 Require Craft.lock is up to date
";

pub fn execute(options: Options, config: &Config) -> CliResult<Option<()>> {
    config.configure(options.flag_verbose,
                          options.flag_quiet,
                          &options.flag_color,
                          options.flag_frozen,
                          options.flag_locked)?;
    let Options { flag_url: url, flag_reference: reference, .. } = options;

    let url = url.to_url()?;

    let reference = GitReference::Branch(reference.clone());
    let source_id = SourceId::for_git(&url, reference);

    let mut source = GitSource::new(&source_id, config);

    source.update()?;

    Ok(None)
}
