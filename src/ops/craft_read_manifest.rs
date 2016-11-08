use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::{fs, io};

use manifest::EitherManifest;
use package::Package;
use package_id::PackageId;
use source::SourceId;
use util::important_paths::find_project_manifest_exact;
use util::toml::Layout;
use util::{self, paths, CraftResult, human, Config, ChainError};

pub fn read_manifest(path: &Path,
                     source_id: &SourceId,
                     config: &Config)
                     -> CraftResult<(EitherManifest, Vec<PathBuf>)> {
    trace!("read_package; path={}; source-id={}",
           path.display(),
           source_id);
    let contents = try!(paths::read(path));

    let layout = Layout::from_project_path(path.parent().unwrap());
    let root = layout.root.clone();
    util::toml::to_manifest(&contents, source_id, layout, config).chain_error(|| {
        human(format!("failed to parse manifest at `{}`",
                      root.join("Craft.toml").display()))
    })
}

pub fn read_package(path: &Path, source_id: &SourceId, config: &Config) -> CraftResult<(Package, Vec<PathBuf>)> {
    trace!("read_package; path={}; source-id={}",
           path.display(),
           source_id);
    let (manifest, nested) = try!(read_manifest(path, source_id, config));
    let manifest = match manifest {
        EitherManifest::Real(manifest) => manifest,
        EitherManifest::Virtual(..) => {
            bail!("found a virtual manifest at `{}` instead of a package \
                   manifest",
                  path.display())
        }
    };

    Ok((Package::new(manifest, path), nested))
}

pub fn read_packages(path: &Path, source_id: &SourceId, config: &Config) -> CraftResult<Vec<Package>> {
    let mut all_packages = HashMap::new();
    let mut visited = HashSet::<PathBuf>::new();

    trace!("looking for root package: {}, source_id={}",
           path.display(),
           source_id);

    try!(walk(path,
              &mut |dir| {
        trace!("looking for child package: {}", dir.display());

        // Don't recurse into hidden/dot directories unless we're at the toplevel
        if dir != path {
            let name = dir.file_name().and_then(|s| s.to_str());
            if name.map(|s| s.starts_with(".")) == Some(true) {
                return Ok(false);
            }

            // Don't automatically discover packages across git submodules
            if fs::metadata(&dir.join(".git")).is_ok() {
                return Ok(false);
            }
        }

        // Don't ever look at target directories
        if dir.file_name().and_then(|s| s.to_str()) == Some("target") && has_manifest(dir.parent().unwrap()) {
            return Ok(false);
        }

        if has_manifest(dir) {
            try!(read_nested_packages(dir, &mut all_packages, source_id, config, &mut visited));
        }
        Ok(true)
    }));

    if all_packages.is_empty() {
        Err(human(format!("Could not find Craft.toml in `{}`", path.display())))
    } else {
        Ok(all_packages.into_iter().map(|(_, v)| v).collect())
    }
}

fn walk(path: &Path, callback: &mut FnMut(&Path) -> CraftResult<bool>) -> CraftResult<()> {
    if !try!(callback(path)) {
        trace!("not processing {}", path.display());
        return Ok(());
    }

    // Ignore any permission denied errors because temporary directories
    // can often have some weird permissions on them.
    let dirs = match fs::read_dir(path) {
        Ok(dirs) => dirs,
        Err(ref e) if e.kind() == io::ErrorKind::PermissionDenied => return Ok(()),
        Err(e) => return Err(human(e)).chain_error(|| human(format!("failed to read directory `{}`", path.display()))),
    };
    for dir in dirs {
        let dir = try!(dir);
        if try!(dir.file_type()).is_dir() {
            try!(walk(&dir.path(), callback));
        }
    }
    Ok(())
}

fn has_manifest(path: &Path) -> bool {
    find_project_manifest_exact(path, "Craft.toml").is_ok()
}

fn read_nested_packages(path: &Path,
                        all_packages: &mut HashMap<PackageId, Package>,
                        source_id: &SourceId,
                        config: &Config,
                        visited: &mut HashSet<PathBuf>)
                        -> CraftResult<()> {
    if !visited.insert(path.to_path_buf()) {
        return Ok(());
    }

    let manifest_path = try!(find_project_manifest_exact(path, "Craft.toml"));

    let (manifest, nested) = try!(read_manifest(&manifest_path, source_id, config));
    let manifest = match manifest {
        EitherManifest::Real(manifest) => manifest,
        EitherManifest::Virtual(..) => return Ok(()),
    };
    let pkg = Package::new(manifest, &manifest_path);

    let pkg_id = pkg.package_id().clone();
    if !all_packages.contains_key(&pkg_id) {
        all_packages.insert(pkg_id, pkg);
    } else {
        info!("skipping nested package `{}` found at `{}`",
              pkg.name(),
              path.to_string_lossy());
    }

    // Registry sources are not allowed to have `path=` dependencies because
    // they're all translated to actual registry dependencies.
    //
    // We normalize the path here ensure that we don't infinitely walk around
    // looking for crates. By normalizing we ensure that we visit this crate at
    // most once.
    //
    // TODO: filesystem/symlink implications?
    if !source_id.is_registry() {
        for p in nested.iter() {
            let path = util::normalize_path(&path.join(p));
            try!(read_nested_packages(&path, all_packages, source_id, config, visited));
        }
    }

    Ok(())
}
