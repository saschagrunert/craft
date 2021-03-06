use std::io::SeekFrom;
use std::io::prelude::*;
use std::path::Path;

use rustc_serialize::hex::ToHex;

use package_id::PackageId;
use sources::registry::{RegistryData, RegistryConfig};
use util::{Config, CraftResult, ChainError, human, Sha256, Filesystem, FileLock};

pub struct LocalRegistry<'cfg> {
    index_path: Filesystem,
    root: Filesystem,
    src_path: Filesystem,
    config: &'cfg Config,
}

impl<'cfg> LocalRegistry<'cfg> {
    pub fn new(root: &Path, config: &'cfg Config, name: &str) -> LocalRegistry<'cfg> {
        LocalRegistry {
            src_path: config.registry_source_path().join(name),
            index_path: Filesystem::new(root.join("index")),
            root: Filesystem::new(root.to_path_buf()),
            config: config,
        }
    }
}

impl<'cfg> RegistryData for LocalRegistry<'cfg> {
    fn index_path(&self) -> &Filesystem {
        &self.index_path
    }

    fn config(&self) -> CraftResult<Option<RegistryConfig>> {
        // Local registries don't have configuration for remote APIs or anything
        // like that
        Ok(None)
    }

    fn update_index(&mut self) -> CraftResult<()> {
        // Nothing to update, we just use what's on disk. Verify it actually
        // exists though. We don't use any locks as we're just checking whether
        // these directories exist.
        let root = self.root.clone().into_path_unlocked();
        if !root.is_dir() {
            bail!("local registry path is not a directory: {}", root.display())
        }
        let index_path = self.index_path.clone().into_path_unlocked();
        if !index_path.is_dir() {
            bail!("local registry index path is not a directory: {}",
                  index_path.display())
        }
        Ok(())
    }

    fn download(&mut self, pkg: &PackageId, checksum: &str) -> CraftResult<FileLock> {
        let chest_file = format!("{}-{}.chest", pkg.name(), pkg.version());
        let mut chest_file = self.root.open_ro(&chest_file, self.config, "chest file")?;

        // If we've already got an unpacked version of this chest, then skip the
        // checksum below as it is in theory already verified.
        let dst = format!("{}-{}", pkg.name(), pkg.version());
        if self.src_path.join(dst).into_path_unlocked().exists() {
            return Ok(chest_file);
        }

        self.config.shell().status("Unpacking", pkg)?;

        // We don't actually need to download anything per-se, we just need to
        // verify the checksum matches the .chest file itself.
        let mut state = Sha256::new();
        let mut buf = [0; 64 * 1024];
        loop {
            let n = chest_file.read(&mut buf)
                .chain_error(|| human(format!("failed to read `{}`", chest_file.path().display())))?;
            if n == 0 {
                break;
            }
            state.update(&buf[..n]);
        }
        if state.finish().to_hex() != checksum {
            bail!("failed to verify the checksum of `{}`", pkg)
        }

        chest_file.seek(SeekFrom::Start(0))?;

        Ok(chest_file)
    }
}
