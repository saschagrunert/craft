//! A `Source` for registry-based packages.
//!
//! # What's a Registry?
//!
//! Registries are central locations where packages can be uploaded to,
//! discovered, and searched for. The purpose of a registry is to have a
//! location that serves as permanent storage for versions of a crate over time.
//!
//! Compared to git sources, a registry provides many packages as well as many
//! versions simultaneously. Git sources can also have commits deleted through
//! rebasings where registries cannot have their versions deleted.
//!
//! # The Index of a Registry
//!
//! One of the major difficulties with a registry is that hosting so many
//! packages may quickly run into performance problems when dealing with
//! dependency graphs. It's infeasible for craft to download the entire contents
//! of the registry just to resolve one package's dependencies, for example. As
//! a result, craft needs some efficient method of querying what packages are
//! available on a registry, what versions are available, and what the
//! dependencies for each version is.
//!
//! One method of doing so would be having the registry expose an HTTP endpoint
//! which can be queried with a list of packages and a response of their
//! dependencies and versions is returned. This is somewhat inefficient however
//! as we may have to hit the endpoint many times and we may have already
//! queried for much of the data locally already (for other packages, for
//! example). This also involves inventing a transport format between the
//! registry and Craft itself, so this route was not taken.
//!
//! Instead, Craft communicates with registries through a git repository
//! referred to as the Index. The Index of a registry is essentially an easily
//! query-able version of the registry's database for a list of versions of a
//! package as well as a list of dependencies for each version.
//!
//! Using git to host this index provides a number of benefits:
//!
//! * The entire index can be stored efficiently locally on disk. This means
//!   that all queries of a registry can happen locally and don't need to touch
//!   the network.
//!
//! * Updates of the index are quite efficient. Using git buys incremental
//!   updates, compressed transmission, etc for free. The index must be updated
//!   each time we need fresh information from a registry, but this is one
//!   update of a git repository that probably hasn't changed a whole lot so
//!   it shouldn't be too expensive.
//!
//!   Additionally, each modification to the index is just appending a line at
//!   the end of a file (the exact format is described later). This means that
//!   the commits for an index are quite small and easily applied/compressable.
//!
//! ## The format of the Index
//!
//! The index is a store for the list of versions for all packages known, so its
//! format on disk is optimized slightly to ensure that `ls registry` doesn't
//! produce a list of all packages ever known. The index also wants to ensure
//! that there's not a million files which may actually end up hitting
//! filesystem limits at some point. To this end, a few decisions were made
//! about the format of the registry:
//!
//! 1. Each crate will have one file corresponding to it. Each version for a
//!    crate will just be a line in this file.
//! 2. There will be two tiers of directories for crate names, under which
//!    crates corresponding to those tiers will be located.
//!
//! As an example, this is an example hierarchy of an index:
//!
//! ```notrust
//! .
//! ├── 3
//! │   └── u
//! │       └── url
//! ├── bz
//! │   └── ip
//! │       └── bzip2
//! ├── config.json
//! ├── en
//! │   └── co
//! │       └── encoding
//! └── li
//!     ├── bg
//!     │   └── libgit2
//!     └── nk
//!         └── link-config
//! ```
//!
//! The root of the index contains a `config.json` file with a few entries
//! corresponding to the registry (see `RegistryConfig` below).
//!
//! Otherwise, there are three numbered directories (1, 2, 3) for crates with
//! names 1, 2, and 3 characters in length. The 1/2 directories simply have the
//! crate files underneath them, while the 3 directory is sharded by the first
//! letter of the crate name.
//!
//! Otherwise the top-level directory contains many two-letter directory names,
//! each of which has many sub-folders with two letters. At the end of all these
//! are the actual crate files themselves.
//!
//! The purpose of this layout is to hopefully cut down on `ls` sizes as well as
//! efficient lookup based on the crate name itself.
//!
//! ## Crate files
//!
//! Each file in the index is the history of one crate over time. Each line in
//! the file corresponds to one version of a crate, stored in JSON format (see
//! the `RegistryPackage` structure below).
//!
//! As new versions are published, new lines are appended to this file. The only
//! modifications to this file that should happen over time are yanks of a
//! particular version.
//!
//! # Downloading Packages
//!
//! The purpose of the Index was to provide an efficient method to resolve the
//! dependency graph for a package. So far we only required one network
//! interaction to update the registry's repository (yay!). After resolution has
//! been performed, however we need to download the contents of packages so we
//! can read the full manifest and build the source code.
//!
//! To accomplish this, this source's `download` method will make an HTTP
//! request per-package requested to download tarballs into a local cache. These
//! tarballs will then be unpacked into a destination folder.
//!
//! Note that because versions uploaded to the registry are frozen forever that
//! the HTTP download and unpacking can all be skipped if the version has
//! already been downloaded and unpacked. This caching allows us to only
//! download a package when absolutely necessary.
//!
//! # Filesystem Hierarchy
//!
//! Overall, the `$HOME/.craft` looks like this when talking about the registry:
//!
//! ```notrust
//! # A folder under which all registry metadata is hosted (similar to
//! # $HOME/.craft/git)
//! $HOME/.craft/registry/
//!
//!     # For each registry that craft knows about (keyed by hostname + hash)
//!     # there is a folder which is the checked out version of the index for
//!     # the registry in this location. Note that this is done so craft can
//!     # support multiple registries simultaneously
//!     index/
//!         registry1-<hash>/
//!         registry2-<hash>/
//!         ...
//!
//!     # This folder is a cache for all downloaded tarballs from a registry.
//!     # Once downloaded and verified, a tarball never changes.
//!     cache/
//!         registry1-<hash>/<pkg>-<version>.crate
//!         ...
//!
//!     # Location in which all tarballs are unpacked. Each tarball is known to
//!     # be frozen after downloading, so transitively this folder is also
//!     # frozen once its unpacked (it's never unpacked again)
//!     src/
//!         registry1-<hash>/<pkg>-<version>/...
//!         ...
//! ```

use std::collections::HashMap;
use std::fs::File;
use std::path::{PathBuf, Path};

use flate2::read::GzDecoder;
use tar::Archive;

use core::{Source, SourceId, PackageId, Package};
use dependency::Dependency;
use registry::Registry;
use summary::Summary;
use sources::PathSource;
use util::{CraftResult, Config, internal, ChainError, FileLock, Filesystem};
use util::hex;

const INDEX_LOCK: &'static str = ".craft-index-lock";
pub static CRATES_IO: &'static str = "https://github.com/rust-lang/crates.io-index";

pub struct RegistrySource<'cfg> {
    source_id: SourceId,
    src_path: Filesystem,
    config: &'cfg Config,
    updated: bool,
    ops: Box<RegistryData + 'cfg>,
    index: index::RegistryIndex<'cfg>,
    index_locked: bool,
}

#[derive(RustcDecodable)]
pub struct RegistryConfig {
    /// Download endpoint for all crates. This will be appended with
    /// `/<crate>/<version>/download` and then will be hit with an HTTP GET
    /// request to download the tarball for a crate.
    pub dl: String,

    /// API endpoint for the registry. This is what's actually hit to perform
    /// operations like yanks, owner modifications, publish new crates, etc.
    pub api: String,
}

#[derive(RustcDecodable)]
struct RegistryPackage {
    name: String,
    vers: String,
    deps: Vec<RegistryDependency>,
    features: HashMap<String, Vec<String>>,
    cksum: String,
    yanked: Option<bool>,
}

#[derive(RustcDecodable)]
struct RegistryDependency {
    name: String,
    req: String,
    features: Vec<String>,
    optional: bool,
    default_features: bool,
    target: Option<String>,
    kind: Option<String>,
}

pub trait RegistryData {
    fn index_path(&self) -> &Filesystem;
    fn config(&self) -> CraftResult<Option<RegistryConfig>>;
    fn update_index(&mut self) -> CraftResult<()>;
    fn download(&mut self, pkg: &PackageId, checksum: &str) -> CraftResult<FileLock>;
}

mod index;
pub mod remote;
mod local;

fn short_name(id: &SourceId) -> String {
    let hash = hex::short_hash(id);
    let ident = id.url().host_str().unwrap_or("").to_string();
    format!("{}-{}", ident, hash)
}

impl<'cfg> RegistrySource<'cfg> {
    pub fn remote(source_id: &SourceId, config: &'cfg Config) -> RegistrySource<'cfg> {
        let name = short_name(source_id);
        let ops = remote::RemoteRegistry::new(source_id, config, &name);
        RegistrySource::new(source_id, config, &name, Box::new(ops), true)
    }

    pub fn local(source_id: &SourceId, path: &Path, config: &'cfg Config) -> RegistrySource<'cfg> {
        let name = short_name(source_id);
        let ops = local::LocalRegistry::new(path, config, &name);
        RegistrySource::new(source_id, config, &name, Box::new(ops), false)
    }

    fn new(source_id: &SourceId,
           config: &'cfg Config,
           name: &str,
           ops: Box<RegistryData + 'cfg>,
           index_locked: bool)
           -> RegistrySource<'cfg> {
        RegistrySource {
            src_path: config.registry_source_path().join(name),
            config: config,
            source_id: source_id.clone(),
            updated: false,
            index: index::RegistryIndex::new(source_id, ops.index_path(), config, index_locked),
            index_locked: index_locked,
            ops: ops,
        }
    }

    /// Decode the configuration stored within the registry.
    ///
    /// This requires that the index has been at least checked out.
    pub fn config(&self) -> CraftResult<Option<RegistryConfig>> {
        self.ops.config()
    }

    /// Unpacks a downloaded package into a location where it's ready to be
    /// compiled.
    ///
    /// No action is taken if the source looks like it's already unpacked.
    fn unpack_package(&self, pkg: &PackageId, tarball: &FileLock) -> CraftResult<PathBuf> {
        let dst = self.src_path.join(&format!("{}-{}", pkg.name(), pkg.version()));
        try!(dst.create_dir());
        // Note that we've already got the `tarball` locked above, and that
        // implies a lock on the unpacked destination as well, so this access
        // via `into_path_unlocked` should be ok.
        let dst = dst.into_path_unlocked();
        let ok = dst.join(".craft-ok");
        if ok.exists() {
            return Ok(dst);
        }

        let gz = try!(GzDecoder::new(tarball.file()));
        let mut tar = Archive::new(gz);
        try!(tar.unpack(dst.parent().unwrap()));
        try!(File::create(&ok));
        Ok(dst)
    }

    fn do_update(&mut self) -> CraftResult<()> {
        try!(self.ops.update_index());
        let path = self.ops.index_path();
        self.index = index::RegistryIndex::new(&self.source_id, path, self.config, self.index_locked);
        Ok(())
    }
}

impl<'cfg> Registry for RegistrySource<'cfg> {
    fn query(&mut self, dep: &Dependency) -> CraftResult<Vec<Summary>> {
        // If this is a precise dependency, then it came from a lockfile and in
        // theory the registry is known to contain this version. If, however, we
        // come back with no summaries, then our registry may need to be
        // updated, so we fall back to performing a lazy update.
        if dep.source_id().precise().is_some() && !self.updated {
            if try!(self.index.query(dep)).is_empty() {
                try!(self.do_update());
            }
        }

        self.index.query(dep)
    }

    fn supports_checksums(&self) -> bool {
        true
    }
}

impl<'cfg> Source for RegistrySource<'cfg> {
    fn update(&mut self) -> CraftResult<()> {
        // If we have an imprecise version then we don't know what we're going
        // to look for, so we always attempt to perform an update here.
        //
        // If we have a precise version, then we'll update lazily during the
        // querying phase. Note that precise in this case is only
        // `Some("locked")` as other `Some` values indicate a `craft update
        // --precise` request
        if self.source_id.precise() != Some("locked") {
            try!(self.do_update());
        }
        Ok(())
    }

    fn download(&mut self, package: &PackageId) -> CraftResult<Package> {
        let hash = try!(self.index.hash(package));
        let path = try!(self.ops.download(package, &hash));
        let path = try!(self.unpack_package(package, &path)
            .chain_error(|| internal(format!("failed to unpack package `{}`", package))));
        let mut src = PathSource::new(&path, &self.source_id, self.config);
        try!(src.update());
        let pkg = try!(src.download(package));

        // Unfortunately the index and the actual Craft.toml in the index can
        // differ due to historical Craft bugs. To paper over these we trash the
        // *summary* loaded from the Craft.toml we just downloaded with the one
        // we loaded from the index.
        let summaries = try!(self.index.summaries(package.name()));
        let summary = summaries.iter()
            .map(|s| &s.0)
            .find(|s| s.package_id() == package)
            .expect("summary not found");
        let mut manifest = pkg.manifest().clone();
        manifest.set_summary(summary.clone());
        Ok(Package::new(manifest, pkg.manifest_path()))
    }

    fn fingerprint(&self, pkg: &Package) -> CraftResult<String> {
        Ok(pkg.package_id().version().to_string())
    }
}
