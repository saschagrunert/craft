//! Implementation of configuration for various sources
//!
//! This module will parse the various `source.*` TOML configuration keys into a
//! structure usable by Craft itself. Currently this is primarily used to map
//! sources to one another via the `replace-with` key in `.craft/config`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use url::Url;

use source::{Source, SourceId};
use sources::ReplacedSource;
use util::{CraftResult, Config, ChainError, human, ToUrl};
use util::config::ConfigValue;

pub struct SourceConfigMap<'cfg> {
    cfgs: HashMap<String, SourceConfig>,
    id2name: HashMap<SourceId, String>,
    config: &'cfg Config,
}

/// Configuration for a particular source, found in TOML looking like:
///
/// ```toml
/// [source.crates-io]
/// registry = 'https://github.com/rust-lang/crates.io-index'
/// replace-with = 'foo'    # optional
/// ```
struct SourceConfig {
    // id this source corresponds to, inferred from the various defined keys in
    // the configuration
    id: SourceId,

    // Name of the source that this source should be replaced with. This field
    // is a tuple of (name, path) where path is where this configuration key was
    // defined (the literal `.craft/config` file).
    replace_with: Option<(String, PathBuf)>,
}

impl<'cfg> SourceConfigMap<'cfg> {
    pub fn new(config: &'cfg Config) -> CraftResult<SourceConfigMap<'cfg>> {
        let mut base = try!(SourceConfigMap::empty(config));
        if let Some(table) = try!(config.get_table("source")) {
            for (key, value) in table.val.iter() {
                try!(base.add_config(key, value));
            }
        }
        Ok(base)
    }

    pub fn empty(config: &'cfg Config) -> CraftResult<SourceConfigMap<'cfg>> {
        let base = SourceConfigMap {
            cfgs: HashMap::new(),
            id2name: HashMap::new(),
            config: config,
        };
        Ok(base)
    }

    pub fn config(&self) -> &'cfg Config {
        self.config
    }

    pub fn load(&self, id: &SourceId) -> CraftResult<Box<Source + 'cfg>> {
        debug!("loading: {}", id);
        let mut name = match self.id2name.get(id) {
            Some(name) => name,
            None => return Ok(id.load(self.config)),
        };
        let mut path = Path::new("/");
        let orig_name = name;
        let new_id;
        loop {
            let cfg = match self.cfgs.get(name) {
                Some(cfg) => cfg,
                None => {
                    bail!("could not find a configured source with the \
                               name `{}` when attempting to lookup `{}` \
                               (configuration in `{}`)",
                          name,
                          orig_name,
                          path.display())
                }
            };
            match cfg.replace_with {
                Some((ref s, ref p)) => {
                    name = s;
                    path = p;
                }
                None if *id == cfg.id => return Ok(id.load(self.config)),
                None => {
                    new_id = cfg.id.with_precise(id.precise()
                        .map(|s| s.to_string()));
                    break;
                }
            }
            debug!("following pointer to {}", name);
            if name == orig_name {
                bail!("detected a cycle of `replace-with` sources, the source \
                       `{}` is eventually replaced with itself \
                       (configuration in `{}`)",
                      name,
                      path.display())
            }
        }
        let new_src = new_id.load(self.config);
        let old_src = id.load(self.config);
        if new_src.supports_checksums() != old_src.supports_checksums() {
            let (supports, no_support) = if new_src.supports_checksums() {
                (name, orig_name)
            } else {
                (orig_name, name)
            };
            bail!("\
cannot replace `{orig}` with `{name}`, the source `{supports}` supports \
checksums, but `{no_support}` does not

a lock file compatible with `{orig}` cannot be generated in this situation
",
                  orig = orig_name,
                  name = name,
                  supports = supports,
                  no_support = no_support);
        }
        Ok(Box::new(ReplacedSource::new(id, &new_id, new_src)))
    }

    fn add(&mut self, name: &str, cfg: SourceConfig) {
        self.id2name.insert(cfg.id.clone(), name.to_string());
        self.cfgs.insert(name.to_string(), cfg);
    }

    fn add_config(&mut self, name: &str, cfg: &ConfigValue) -> CraftResult<()> {
        let (table, _path) = try!(cfg.table(&format!("source.{}", name)));
        let mut srcs = Vec::new();
        if let Some(val) = table.get("registry") {
            let url = try!(url(val, &format!("source.{}.registry", name)));
            srcs.push(SourceId::for_registry(&url));
        }
        if let Some(val) = table.get("local-registry") {
            let (s, path) = try!(val.string(&format!("source.{}.local-registry", name)));
            let mut path = path.to_path_buf();
            path.pop();
            path.pop();
            path.push(s);
            srcs.push(try!(SourceId::for_local_registry(&path)));
        }
        if let Some(val) = table.get("directory") {
            let (s, path) = try!(val.string(&format!("source.{}.directory", name)));
            let mut path = path.to_path_buf();
            path.pop();
            path.pop();
            path.push(s);
            srcs.push(try!(SourceId::for_directory(&path)));
        }

        let mut srcs = srcs.into_iter();
        let src = try!(srcs.next().chain_error(|| {
            human(format!("no source URL specified for `source.{}`, need \
                           either `registry` or `local-registry` defined",
                          name))
        }));
        if srcs.next().is_some() {
            return Err(human(format!("more than one source URL specified for \
                                      `source.{}`",
                                     name)));
        }

        let mut replace_with = None;
        if let Some(val) = table.get("replace-with") {
            let (s, path) = try!(val.string(&format!("source.{}.replace-with", name)));
            replace_with = Some((s.to_string(), path.to_path_buf()));
        }

        self.add(name,
                 SourceConfig {
                     id: src,
                     replace_with: replace_with,
                 });

        return Ok(());

        fn url(cfg: &ConfigValue, key: &str) -> CraftResult<Url> {
            let (url, path) = try!(cfg.string(key));
            url.to_url().chain_error(|| {
                human(format!("configuration key `{}` specified an invalid \
                               URL (in {})",
                              key,
                              path.display()))

            })
        }
    }
}
