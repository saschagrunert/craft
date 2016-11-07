use std::fmt::{self, Debug, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use filetime::FileTime;
use git2;
use glob::Pattern;

use core::{Package, PackageId, Summary, SourceId, Source, Dependency, Registry};
use ops;
use util::{self, CraftResult, internal, internal_error, human, ChainError};
use util::Config;

pub struct PathSource<'cfg> {
    id: SourceId,
    path: PathBuf,
    updated: bool,
    packages: Vec<Package>,
    config: &'cfg Config,
    recursive: bool,
}

impl<'cfg> PathSource<'cfg> {
    /// Invoked with an absolute path to a directory that contains a Craft.toml.
    ///
    /// This source will only return the package at precisely the `path`
    /// specified, and it will be an error if there's not a package at `path`.
    pub fn new(path: &Path, id: &SourceId, config: &'cfg Config) -> PathSource<'cfg> {
        PathSource {
            id: id.clone(),
            path: path.to_path_buf(),
            updated: false,
            packages: Vec::new(),
            config: config,
            recursive: false,
        }
    }

    /// Creates a new source which is walked recursively to discover packages.
    ///
    /// This is similar to the `new` method except that instead of requiring a
    /// valid package to be present at `root` the folder is walked entirely to
    /// crawl for packages.
    ///
    /// Note that this should be used with care and likely shouldn't be chosen
    /// by default!
    pub fn new_recursive(root: &Path, id: &SourceId, config: &'cfg Config) -> PathSource<'cfg> {
        PathSource { recursive: true, ..PathSource::new(root, id, config) }
    }

    pub fn root_package(&mut self) -> CraftResult<Package> {
        trace!("root_package; source={:?}", self);

        try!(self.update());

        match self.packages.iter().find(|p| p.root() == &*self.path) {
            Some(pkg) => Ok(pkg.clone()),
            None => Err(internal("no package found in source")),
        }
    }

    pub fn read_packages(&self) -> CraftResult<Vec<Package>> {
        if self.updated {
            Ok(self.packages.clone())
        } else if self.recursive {
            ops::read_packages(&self.path, &self.id, self.config)
        } else {
            let path = self.path.join("Craft.toml");
            let (pkg, _) = try!(ops::read_package(&path, &self.id, self.config));
            Ok(vec![pkg])
        }
    }

    /// List all files relevant to building this package inside this source.
    ///
    /// This function will use the appropriate methods to determine the
    /// set of files underneath this source's directory which are relevant for
    /// building `pkg`.
    ///
    /// The basic assumption of this method is that all files in the directory
    /// are relevant for building this package, but it also contains logic to
    /// use other methods like .gitignore to filter the list of files.
    pub fn list_files(&self, pkg: &Package) -> CraftResult<Vec<PathBuf>> {
        let root = pkg.root();

        let parse = |p: &String| Pattern::new(p).map_err(|e| human(format!("could not parse pattern `{}`: {}", p, e)));
        let exclude = try!(pkg.manifest()
            .exclude()
            .iter()
            .map(|p| parse(p))
            .collect::<Result<Vec<_>, _>>());
        let include = try!(pkg.manifest()
            .include()
            .iter()
            .map(|p| parse(p))
            .collect::<Result<Vec<_>, _>>());

        let mut filter = |p: &Path| {
            let relative_path = util::without_prefix(p, &root).unwrap();
            include.iter().any(|p| p.matches_path(&relative_path)) ||
            {
                include.is_empty() && !exclude.iter().any(|p| p.matches_path(&relative_path))
            }
        };

        // If this package is in a git repository, then we really do want to
        // query the git repository as it takes into account items such as
        // .gitignore. We're not quite sure where the git repository is,
        // however, so we do a bit of a probe.
        //
        // We walk this package's path upwards and look for a sibling
        // Craft.toml and .git folder. If we find one then we assume that we're
        // part of that repository.
        let mut cur = root;
        loop {
            if cur.join("Craft.toml").is_file() {
                // If we find a git repository next to this Craft.toml, we still
                // check to see if we are indeed part of the index. If not, then
                // this is likely an unrelated git repo, so keep going.
                if let Ok(repo) = git2::Repository::open(cur) {
                    let index = try!(repo.index());
                    let path = util::without_prefix(root, cur)
                        .unwrap()
                        .join("Craft.toml");
                    if index.get_path(&path, 0).is_some() {
                        return self.list_files_git(pkg, repo, &mut filter);
                    }
                }
            }
            // don't cross submodule boundaries
            if cur.join(".git").is_dir() {
                break;
            }
            match cur.parent() {
                Some(parent) => cur = parent,
                None => break,
            }
        }
        self.list_files_walk(pkg, &mut filter)
    }

    fn list_files_git(&self,
                      pkg: &Package,
                      repo: git2::Repository,
                      filter: &mut FnMut(&Path) -> bool)
                      -> CraftResult<Vec<PathBuf>> {
        warn!("list_files_git {}", pkg.package_id());
        let index = try!(repo.index());
        let root = try!(repo.workdir().chain_error(|| internal_error("Can't list files on a bare repository.", "")));
        let pkg_path = pkg.root();

        let mut ret = Vec::<PathBuf>::new();

        // We use information from the git repository to guide us in traversing
        // its tree. The primary purpose of this is to take advantage of the
        // .gitignore and auto-ignore files that don't matter.
        //
        // Here we're also careful to look at both tracked and untracked files as
        // the untracked files are often part of a build and may become relevant
        // as part of a future commit.
        let index_files = index.iter().map(|entry| {
            use libgit2_sys::GIT_FILEMODE_COMMIT;
            let is_dir = entry.mode == GIT_FILEMODE_COMMIT as u32;
            (join(&root, &entry.path), Some(is_dir))
        });
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);
        if let Some(suffix) = util::without_prefix(pkg_path, &root) {
            opts.pathspec(suffix);
        }
        let statuses = try!(repo.statuses(Some(&mut opts)));
        let untracked = statuses.iter().filter_map(|entry| {
            match entry.status() {
                git2::STATUS_WT_NEW => Some((join(&root, entry.path_bytes()), None)),
                _ => None,
            }
        });

        let mut subpackages_found = Vec::new();

        'outer: for (file_path, is_dir) in index_files.chain(untracked) {
            let file_path = try!(file_path);

            // Filter out files blatantly outside this package. This is helped a
            // bit obove via the `pathspec` function call, but we need to filter
            // the entries in the index as well.
            if !file_path.starts_with(pkg_path) {
                continue;
            }

            match file_path.file_name().and_then(|s| s.to_str()) {
                // Filter out Craft.lock and target always, we don't want to
                // package a lock file no one will ever read and we also avoid
                // build artifacts
                Some("Craft.lock") |
                Some("target") => continue,

                // Keep track of all sub-packages found and also strip out all
                // matches we've found so far. Note, though, that if we find
                // our own `Craft.toml` we keep going.
                Some("Craft.toml") => {
                    let path = file_path.parent().unwrap();
                    if path != pkg_path {
                        warn!("subpackage found: {}", path.display());
                        ret.retain(|p| !p.starts_with(path));
                        subpackages_found.push(path.to_path_buf());
                        continue;
                    }
                }

                _ => {}
            }

            // If this file is part of any other sub-package we've found so far,
            // skip it.
            if subpackages_found.iter().any(|p| file_path.starts_with(p)) {
                continue;
            }

            if is_dir.unwrap_or_else(|| file_path.is_dir()) {
                warn!("  found submodule {}", file_path.display());
                let rel = util::without_prefix(&file_path, &root).unwrap();
                let rel = try!(rel.to_str()
                    .chain_error(|| human(format!("invalid utf-8 filename: {}", rel.display()))));
                // Git submodules are currently only named through `/` path
                // separators, explicitly not `\` which windows uses. Who knew?
                let rel = rel.replace(r"\", "/");
                match repo.find_submodule(&rel).and_then(|s| s.open()) {
                    Ok(repo) => {
                        let files = try!(self.list_files_git(pkg, repo, filter));
                        ret.extend(files.into_iter());
                    }
                    Err(..) => {
                        try!(PathSource::walk(&file_path, &mut ret, false, filter));
                    }
                }
            } else if (*filter)(&file_path) {
                // We found a file!
                warn!("  found {}", file_path.display());
                ret.push(file_path);
            }
        }
        return Ok(ret);

        #[cfg(unix)]
        fn join(path: &Path, data: &[u8]) -> CraftResult<PathBuf> {
            use std::os::unix::prelude::*;
            use std::ffi::OsStr;
            Ok(path.join(<OsStr as OsStrExt>::from_bytes(data)))
        }
        #[cfg(windows)]
        fn join(path: &Path, data: &[u8]) -> CraftResult<PathBuf> {
            use std::str;
            match str::from_utf8(data) {
                Ok(s) => Ok(path.join(s)),
                Err(..) => Err(internal("cannot process path in git with a non unicode filename")),
            }
        }
    }

    fn list_files_walk(&self, pkg: &Package, filter: &mut FnMut(&Path) -> bool) -> CraftResult<Vec<PathBuf>> {
        let mut ret = Vec::new();
        try!(PathSource::walk(pkg.root(), &mut ret, true, filter));
        Ok(ret)
    }

    fn walk(path: &Path, ret: &mut Vec<PathBuf>, is_root: bool, filter: &mut FnMut(&Path) -> bool) -> CraftResult<()> {
        if !fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false) {
            if (*filter)(path) {
                ret.push(path.to_path_buf());
            }
            return Ok(());
        }
        // Don't recurse into any sub-packages that we have
        if !is_root && fs::metadata(&path.join("Craft.toml")).is_ok() {
            return Ok(());
        }
        for dir in try!(fs::read_dir(path)) {
            let dir = try!(dir).path();
            let name = dir.file_name().and_then(|s| s.to_str());
            // Skip dotfile directories
            if name.map(|s| s.starts_with('.')) == Some(true) {
                continue;
            } else if is_root {
                // Skip craft artifacts
                match name {
                    Some("target") |
                    Some("Craft.lock") => continue,
                    _ => {}
                }
            }
            try!(PathSource::walk(&dir, ret, false, filter));
        }
        Ok(())
    }
}

impl<'cfg> Debug for PathSource<'cfg> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "the paths source")
    }
}

impl<'cfg> Registry for PathSource<'cfg> {
    fn query(&mut self, dep: &Dependency) -> CraftResult<Vec<Summary>> {
        self.packages.query(dep)
    }
}

impl<'cfg> Source for PathSource<'cfg> {
    fn update(&mut self) -> CraftResult<()> {
        if !self.updated {
            let packages = try!(self.read_packages());
            self.packages.extend(packages.into_iter());
            self.updated = true;
        }

        Ok(())
    }

    fn download(&mut self, id: &PackageId) -> CraftResult<Package> {
        trace!("getting packages; id={}", id);

        let pkg = self.packages.iter().find(|pkg| pkg.package_id() == id);
        pkg.cloned().ok_or_else(|| internal(format!("failed to find {} in path source", id)))
    }

    fn fingerprint(&self, pkg: &Package) -> CraftResult<String> {
        if !self.updated {
            return Err(internal_error("BUG: source was not updated", ""));
        }

        let mut max = FileTime::zero();
        let mut max_path = PathBuf::from("");
        for file in try!(self.list_files(pkg)) {
            // An fs::stat error here is either because path is a
            // broken symlink, a permissions error, or a race
            // condition where this path was rm'ed - either way,
            // we can ignore the error and treat the path's mtime
            // as 0.
            let mtime = fs::metadata(&file)
                .map(|meta| FileTime::from_last_modification_time(&meta))
                .unwrap_or(FileTime::zero());
            warn!("{} {}", mtime, file.display());
            if mtime > max {
                max = mtime;
                max_path = file;
            }
        }
        trace!("fingerprint {}: {}", self.path.display(), max);
        Ok(format!("{} ({})", max, max_path.display()))
    }
}
