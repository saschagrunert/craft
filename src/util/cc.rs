use std::path::PathBuf;

use util::{self, CraftResult, internal, ChainError, ProcessBuilder};

pub struct Cc {
    pub path: PathBuf,
    pub verbose_version: String,
    pub host: String,
}

impl Cc {
    /// Run the compiler at `path` to learn various pieces of information about it.
    ///
    /// If successful this function returns a description of the compiler along with a list of its
    /// capabilities.
    pub fn new(path: PathBuf) -> CraftResult<Cc> {
        let mut cmd = util::process(&path);
        cmd.arg("-v");

        let output = cmd.exec_with_output()?;

        let verbose_version =
            String::from_utf8(output.stderr).map_err(|_| internal("cc -v didn't return utf8 output"))?;

        let host = {
            let triple = verbose_version.lines()
                .find(|l| l.starts_with("Target: "))
                .map(|l| &l[8..]);
            let triple = triple.chain_error(|| internal("cc -v didn't have a line for `Target:`"))?;
            triple.to_string()
        };

        Ok(Cc {
            path: path,
            verbose_version: verbose_version,
            host: host,
        })
    }

    pub fn process(&self) -> ProcessBuilder {
        util::process(&self.path)
    }
}
