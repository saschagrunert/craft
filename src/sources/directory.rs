//! Source description for directories
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use rustc_serialize::hex::ToHex;
use rustc_serialize::json;

use dependency::Dependency;
use package::Package;
use package_id::PackageId;
use registry::Registry;
use source::{SourceId, Source};
use sources::PathSource;
use summary::Summary;
use util::paths;
use util::{CraftResult, human, ChainError, Config, Sha256};

pub struct DirectorySource<'cfg> {
    id: SourceId,
    root: PathBuf,
    packages: HashMap<PackageId, (Package, Checksum)>,
    config: &'cfg Config,
}

#[derive(RustcDecodable)]
struct Checksum {
    package: String,
    files: HashMap<String, String>,
}

impl<'cfg> DirectorySource<'cfg> {
    pub fn new(path: &Path, id: &SourceId, config: &'cfg Config) -> DirectorySource<'cfg> {
        DirectorySource {
            id: id.clone(),
            root: path.to_path_buf(),
            config: config,
            packages: HashMap::new(),
        }
    }
}

impl<'cfg> Debug for DirectorySource<'cfg> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "DirectorySource {{ root: {:?} }}", self.root)
    }
}

impl<'cfg> Registry for DirectorySource<'cfg> {
    fn query(&mut self, dep: &Dependency) -> CraftResult<Vec<Summary>> {
        let packages = self.packages.values().map(|p| &p.0);
        let matches = packages.filter(|pkg| dep.matches(pkg.summary()));
        let summaries = matches.map(|pkg| pkg.summary().clone());
        Ok(summaries.collect())
    }

    fn supports_checksums(&self) -> bool {
        true
    }
}

impl<'cfg> Source for DirectorySource<'cfg> {
    fn update(&mut self) -> CraftResult<()> {
        self.packages.clear();
        let entries = self.root
            .read_dir()
            .chain_error(|| {
                human(format!("failed to read root of directory source: {}",
                              self.root.display()))
            })?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let mut src = PathSource::new(&path, &self.id, self.config);
            src.update()?;
            let pkg = src.root_package()?;

            let cksum_file = path.join(".craft-checksum.json");
            let cksum = paths::read(&path.join(cksum_file)).chain_error(|| {
                    human(format!("failed to load checksum `.craft-checksum.json` of {} v{}",
                                  pkg.package_id().name(),
                                  pkg.package_id().version()))

                })?;
            let cksum: Checksum = json::decode(&cksum).chain_error(|| {
                    human(format!("failed to decode `.craft-checksum.json` of {} v{}",
                                  pkg.package_id().name(),
                                  pkg.package_id().version()))
                })?;

            let mut manifest = pkg.manifest().clone();
            let summary = manifest.summary().clone();
            manifest.set_summary(summary.set_checksum(cksum.package.clone()));
            let pkg = Package::new(manifest, pkg.manifest_path());
            self.packages.insert(pkg.package_id().clone(), (pkg, cksum));
        }

        Ok(())
    }

    fn download(&mut self, id: &PackageId) -> CraftResult<Package> {
        self.packages
            .get(id)
            .map(|p| &p.0)
            .cloned()
            .chain_error(|| human(format!("failed to find package with id: {}", id)))
    }

    fn fingerprint(&self, pkg: &Package) -> CraftResult<String> {
        Ok(pkg.package_id().version().to_string())
    }

    fn verify(&self, id: &PackageId) -> CraftResult<()> {
        let (pkg, cksum) = match self.packages.get(id) {
            Some(&(ref pkg, ref cksum)) => (pkg, cksum),
            None => bail!("failed to find entry for `{}` in directory source", id),
        };

        let mut buf = [0; 16 * 1024];
        for (file, cksum) in cksum.files.iter() {
            let mut h = Sha256::new();
            let file = pkg.root().join(file);

            (|| -> CraftResult<()> {
                    let mut f = File::open(&file)?;
                    loop {
                        match f.read(&mut buf)? {
                            0 => return Ok(()),
                            n => h.update(&buf[..n]),
                        }
                    }
                }).chain_error(|| human(format!("failed to calculate checksum of: {}", file.display())))?;

            let actual = h.finish().to_hex();
            if &*actual != cksum {
                bail!("\
                    the listed checksum of `{}` has changed:\n\
                    expected: {}\n\
                    actual:   {}\n\
                    \n\
                    directory sources are not intended to be edited, if \
                    modifications are required then it is recommended \
                    that [replace] is used with a forked copy of the \
                    source\
                ",
                      file.display(),
                      cksum,
                      actual);
            }
        }

        Ok(())
    }
}
