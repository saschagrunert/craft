use std::path::Path;

use ops::{self, CompileFilter};
use util::{self, CraftResult, ProcessError};
use workspace::Workspace;

pub fn run(ws: &Workspace, options: &ops::CompileOptions, args: &[String]) -> CraftResult<Option<ProcessError>> {
    let config = ws.config();
    let root = try!(ws.current());

    let mut bins = root.manifest().targets().iter().filter(|a| {
        !a.is_lib() && !a.is_custom_build() &&
        match options.filter {
            CompileFilter::Everything => a.is_bin(),
            CompileFilter::Only { .. } => options.filter.matches(a),
        }
    });
    if bins.next().is_none() {
        match options.filter {
            CompileFilter::Everything => bail!("a bin target must be available for `craft run`"),
            CompileFilter::Only { .. } => {
                // this will be verified in craft_compile
            }
        }
    }
    if bins.next().is_some() {
        match options.filter {
            CompileFilter::Everything => {
                bail!("`craft run` requires that a project only have one \
                       executable; use the `--bin` option to specify which one \
                       to run")
            }
            CompileFilter::Only { .. } => {
                bail!("`craft run` can run at most one executable, but \
                       multiple were specified")
            }
        }
    }

    let compile = try!(ops::compile(ws, options));
    let exe = &compile.binaries[0];
    let exe = match util::without_prefix(&exe, config.cwd()) {
        Some(path) if path.file_name() == Some(path.as_os_str()) => Path::new(".").join(path).to_path_buf(),
        Some(path) => path.to_path_buf(),
        None => exe.to_path_buf(),
    };
    let mut process = try!(compile.target_process(exe, &root));
    process.args(args).cwd(config.cwd());

    try!(config.shell().status("Running", process.to_string()));
    Ok(process.exec_replace().err())
}
