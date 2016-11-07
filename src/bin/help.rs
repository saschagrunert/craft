use craft::util::{CliResult, CliError, Config, human};

#[derive(RustcDecodable)]
pub struct Options;

pub const USAGE: &'static str = "
Get some help with a craft command.

Usage:
    craft help <command>
    craft help -h | --help

Options:
    -h, --help          Print this message
";

pub fn execute(_: Options, _: &Config) -> CliResult<Option<()>> {
    // This is a dummy command just so that `craft help help` works.
    // The actual delegation of help flag to subcommands is handled by the
    // craft command.
    Err(CliError::new(human("help command should not be executed directly"), 101))
}
