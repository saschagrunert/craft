use std::fs;
use std::path::{Path, PathBuf};
use util::{CraftResult, human};

/// Iteratively search for `file` in `pwd` and its parents, returning
/// the path of the directory.
pub fn find_project(pwd: &Path, file: &str) -> CraftResult<PathBuf> {
    find_project_manifest(pwd, file).map(|mut p| {
        // remove the file, leaving just the directory
        p.pop();
        p
    })
}

/// Iteratively search for `file` in `pwd` and its parents, returning
/// the path to the file.
pub fn find_project_manifest(pwd: &Path, file: &str) -> CraftResult<PathBuf> {
    let mut current = pwd;

    loop {
        let manifest = current.join(file);
        if fs::metadata(&manifest).is_ok() {
            return Ok(manifest);
        }

        match current.parent() {
            Some(p) => current = p,
            None => break,
        }
    }

    bail!("could not find `{}` in `{}` or any parent directory",
          file,
          pwd.display())
}

/// Find the root Craft.toml
pub fn find_root_manifest_for_wd(manifest_path: Option<String>, cwd: &Path) -> CraftResult<PathBuf> {
    match manifest_path {
        Some(path) => {
            let absolute_path = cwd.join(&path);
            if !absolute_path.ends_with("Craft.toml") {
                bail!("the manifest-path must be a path to a Craft.toml file")
            }
            if !fs::metadata(&absolute_path).is_ok() {
                bail!("manifest path `{}` does not exist", path)
            }
            Ok(absolute_path)
        }
        None => find_project_manifest(&cwd, "Craft.toml"),
    }
}

/// Return the path to the `file` in `pwd`, if it exists.
pub fn find_project_manifest_exact(pwd: &Path, file: &str) -> CraftResult<PathBuf> {
    let manifest = pwd.join(file);

    if fs::metadata(&manifest).is_ok() {
        Ok(manifest)
    } else {
        Err(human(format!("Could not find `{}` in `{}`", file, pwd.display())))
    }
}
