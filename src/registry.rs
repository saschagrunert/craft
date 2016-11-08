use std::collections::HashMap;

use dependency::Dependency;
use package::{Package, PackageSet};
use package_id::PackageId;
use source::{Source, SourceMap, SourceId};
use sources::config::SourceConfigMap;
use summary::Summary;
use util::{CraftResult, ChainError, Config, human, profile};

/// Source of information about a group of packages.
///
/// See also `source::Source`.
pub trait Registry {
    /// Attempt to find the packages that match a dependency request.
    fn query(&mut self, name: &Dependency) -> CraftResult<Vec<Summary>>;

    /// Returns whether or not this registry will return summaries with
    /// checksums listed.
    ///
    /// By default, registries do not support checksums.
    fn supports_checksums(&self) -> bool {
        false
    }
}

impl Registry for Vec<Summary> {
    fn query(&mut self, dep: &Dependency) -> CraftResult<Vec<Summary>> {
        Ok(self.iter()
            .filter(|summary| dep.matches(*summary))
            .cloned()
            .collect())
    }
}

impl Registry for Vec<Package> {
    fn query(&mut self, dep: &Dependency) -> CraftResult<Vec<Summary>> {
        Ok(self.iter()
            .filter(|pkg| dep.matches(pkg.summary()))
            .map(|pkg| pkg.summary().clone())
            .collect())
    }
}

impl<'a, T: ?Sized + Registry + 'a> Registry for Box<T> {
    fn query(&mut self, name: &Dependency) -> CraftResult<Vec<Summary>> {
        (**self).query(name)
    }
}

/// This structure represents a registry of known packages. It internally
/// contains a number of `Box<Source>` instances which are used to load a
/// `Package` from.
///
/// The resolution phase of Craft uses this to drive knowledge about new
/// packages as well as querying for lists of new packages. It is here that
/// sources are updated (e.g. network operations) and overrides are
/// handled.
///
/// The general idea behind this registry is that it is centered around the
/// `SourceMap` structure, contained within which is a mapping of a `SourceId` to
/// a `Source`. Each `Source` in the map has been updated (using network
/// operations if necessary) and is ready to be queried for packages.
pub struct PackageRegistry<'cfg> {
    sources: SourceMap<'cfg>,

    // A list of sources which are considered "overrides" which take precedent
    // when querying for packages.
    overrides: Vec<SourceId>,

    // Note that each SourceId does not take into account its `precise` field
    // when hashing or testing for equality. When adding a new `SourceId`, we
    // want to avoid duplicates in the `SourceMap` (to prevent re-updating the
    // same git repo twice for example), but we also want to ensure that the
    // loaded source is always updated.
    //
    // Sources with a `precise` field normally don't need to be updated because
    // their contents are already on disk, but sources without a `precise` field
    // almost always need to be updated. If we have a cached `Source` for a
    // precise `SourceId`, then when we add a new `SourceId` that is not precise
    // we want to ensure that the underlying source is updated.
    //
    // This is basically a long-winded way of saying that we want to know
    // precisely what the keys of `sources` are, so this is a mapping of key to
    // what exactly the key is.
    source_ids: HashMap<SourceId, (SourceId, Kind)>,

    locked: HashMap<SourceId, HashMap<String, Vec<(PackageId, Vec<PackageId>)>>>,
    source_config: SourceConfigMap<'cfg>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Kind {
    Override,
    Locked,
    Normal,
}

impl<'cfg> PackageRegistry<'cfg> {
    pub fn new(config: &'cfg Config) -> CraftResult<PackageRegistry<'cfg>> {
        let source_config = try!(SourceConfigMap::new(config));
        Ok(PackageRegistry {
            sources: SourceMap::new(),
            source_ids: HashMap::new(),
            overrides: Vec::new(),
            source_config: source_config,
            locked: HashMap::new(),
        })
    }

    pub fn get(self, package_ids: &[PackageId]) -> PackageSet<'cfg> {
        trace!("getting packages; sources={}", self.sources.len());
        PackageSet::new(package_ids, self.sources)
    }

    fn ensure_loaded(&mut self, namespace: &SourceId, kind: Kind) -> CraftResult<()> {
        match self.source_ids.get(namespace) {
            // We've previously loaded this source, and we've already locked it,
            // so we're not allowed to change it even if `namespace` has a
            // slightly different precise version listed.
            Some(&(_, Kind::Locked)) => {
                debug!("load/locked   {}", namespace);
                return Ok(());
            }

            // If the previous source was not a precise source, then we can be
            // sure that it's already been updated if we've already loaded it.
            Some(&(ref previous, _)) if previous.precise().is_none() => {
                debug!("load/precise  {}", namespace);
                return Ok(());
            }

            // If the previous source has the same precise version as we do,
            // then we're done, otherwise we need to need to move forward
            // updating this source.
            Some(&(ref previous, _)) => {
                if previous.precise() == namespace.precise() {
                    debug!("load/match    {}", namespace);
                    return Ok(());
                }
                debug!("load/mismatch {}", namespace);
            }
            None => {
                debug!("load/missing  {}", namespace);
            }
        }

        try!(self.load(namespace, kind));
        Ok(())
    }

    pub fn add_sources(&mut self, ids: &[SourceId]) -> CraftResult<()> {
        for id in ids.iter() {
            try!(self.ensure_loaded(id, Kind::Locked));
        }
        Ok(())
    }

    pub fn add_preloaded(&mut self, id: &SourceId, source: Box<Source + 'cfg>) {
        self.add_source(id, source, Kind::Locked);
    }

    fn add_source(&mut self, id: &SourceId, source: Box<Source + 'cfg>, kind: Kind) {
        self.sources.insert(id, source);
        self.source_ids.insert(id.clone(), (id.clone(), kind));
    }

    pub fn add_override(&mut self, id: &SourceId, source: Box<Source + 'cfg>) {
        self.add_source(id, source, Kind::Override);
        self.overrides.push(id.clone());
    }

    pub fn register_lock(&mut self, id: PackageId, deps: Vec<PackageId>) {
        trace!("register_lock: {}", id);
        for dep in deps.iter() {
            trace!("\t-> {}", dep);
        }
        let sub_map = self.locked
            .entry(id.source_id().clone())
            .or_insert(HashMap::new());
        let sub_vec = sub_map.entry(id.name().to_string())
            .or_insert(Vec::new());
        sub_vec.push((id, deps));
    }

    fn load(&mut self, source_id: &SourceId, kind: Kind) -> CraftResult<()> {
        (|| {
                let source = try!(self.source_config.load(source_id));

                if kind == Kind::Override {
                    self.overrides.push(source_id.clone());
                }
                self.add_source(source_id, source, kind);

                // Ensure the source has fetched all necessary remote data.
                let _p = profile::start(format!("updating: {}", source_id));
                self.sources.get_mut(source_id).unwrap().update()
            })
            .chain_error(|| human(format!("Unable to update {}", source_id)))
    }

    fn query_overrides(&mut self, dep: &Dependency) -> CraftResult<Option<Summary>> {
        for s in self.overrides.iter() {
            let src = self.sources.get_mut(s).unwrap();
            let dep = Dependency::new_override(dep.name(), s);
            let mut results = try!(src.query(&dep));
            if results.len() > 0 {
                return Ok(Some(results.remove(0)));
            }
        }
        Ok(None)
    }

    /// This function is used to transform a summary to another locked summary
    /// if possible. This is where the concept of a lockfile comes into play.
    ///
    /// If a summary points at a package id which was previously locked, then we
    /// override the summary's id itself, as well as all dependencies, to be
    /// rewritten to the locked versions. This will transform the summary's
    /// source to a precise source (listed in the locked version) as well as
    /// transforming all of the dependencies from range requirements on
    /// imprecise sources to exact requirements on precise sources.
    ///
    /// If a summary does not point at a package id which was previously locked,
    /// or if any dependencies were added and don't have a previously listed
    /// version, we still want to avoid updating as many dependencies as
    /// possible to keep the graph stable. In this case we map all of the
    /// summary's dependencies to be rewritten to a locked version wherever
    /// possible. If we're unable to map a dependency though, we just pass it on
    /// through.
    pub fn lock(&self, summary: Summary) -> Summary {
        let pair = self.locked
            .get(summary.source_id())
            .and_then(|map| map.get(summary.name()))
            .and_then(|vec| vec.iter().find(|&&(ref id, _)| id == summary.package_id()));

        trace!("locking summary of {}", summary.package_id());

        // Lock the summary's id if possible
        let summary = match pair {
            Some(&(ref precise, _)) => summary.override_id(precise.clone()),
            None => summary,
        };
        summary.map_dependencies(|dep| {
            trace!("\t{}/{}/{}", dep.name(), dep.version_req(), dep.source_id());

            // If we've got a known set of overrides for this summary, then
            // one of a few cases can arise:
            //
            // 1. We have a lock entry for this dependency from the same
            //    source as it's listed as coming from. In this case we make
            //    sure to lock to precisely the given package id.
            //
            // 2. We have a lock entry for this dependency, but it's from a
            //    different source than what's listed, or the version
            //    requirement has changed. In this case we must discard the
            //    locked version because the dependency needs to be
            //    re-resolved.
            //
            // 3. We don't have a lock entry for this dependency, in which
            //    case it was likely an optional dependency which wasn't
            //    included previously so we just pass it through anyway.
            //
            // Cases 1/2 are handled by `matches_id` and case 3 is handled by
            // falling through to the logic below.
            if let Some(&(_, ref locked_deps)) = pair {
                let locked = locked_deps.iter().find(|id| dep.matches_id(id));
                if let Some(locked) = locked {
                    trace!("\tfirst hit on {}", locked);
                    return dep.lock_to(locked);
                }
            }

            // If this dependency did not have a locked version, then we query
            // all known locked packages to see if they match this dependency.
            // If anything does then we lock it to that and move on.
            let v = self.locked
                .get(dep.source_id())
                .and_then(|map| map.get(dep.name()))
                .and_then(|vec| vec.iter().find(|&&(ref id, _)| dep.matches_id(id)));
            match v {
                Some(&(ref id, _)) => {
                    trace!("\tsecond hit on {}", id);
                    dep.lock_to(id)
                }
                None => {
                    trace!("\tremaining unlocked");
                    dep
                }
            }
        })
    }

    fn warn_bad_override(&self, override_summary: &Summary, real_summary: &Summary) -> CraftResult<()> {
        let real = real_summary.package_id();
        let map = try!(self.locked
            .get(real.source_id())
            .chain_error(|| human(format!("failed to find lock source of {}", real))));
        let list = try!(map.get(real.name()).chain_error(|| human(format!("failed to find lock name of {}", real))));
        let &(_, ref real_deps) = try!(list.iter()
            .find(|&&(ref id, _)| real == id)
            .chain_error(|| human(format!("failed to find lock version of {}", real))));
        let mut real_deps = real_deps.clone();

        let boilerplate = "\
This is currently allowed but is known to produce buggy behavior with spurious
recompiles and changes to the crate graph. Path overrides unfortunately were
never intended to support this feature, so for now this message is just a
warning. In the future, however, this message will become a hard error.

To change the dependency graph via an override it's recommended to use the
`[replace]` feature of Craft instead of the path override feature. This is
documented online at the url below for more information.
";

        for dep in override_summary.dependencies() {
            if let Some(i) = real_deps.iter().position(|id| dep.matches_id(id)) {
                real_deps.remove(i);
                continue;
            }
            let msg = format!("\
                path override for crate `{}` has altered the original list of\n\
                dependencies; the dependency on `{}` was either added or\n\
                modified to not match the previously resolved version\n\n\
                {}",
                              override_summary.package_id().name(),
                              dep.name(),
                              boilerplate);
            try!(self.source_config.config().shell().warn(&msg));
            return Ok(());
        }

        for id in real_deps {
            let msg = format!("\
                path override for crate `{}` has altered the original list of
                dependencies; the dependency on `{}` was removed\n\n
                {}",
                              override_summary.package_id().name(),
                              id.name(),
                              boilerplate);
            try!(self.source_config.config().shell().warn(&msg));
            return Ok(());
        }

        Ok(())
    }
}

impl<'cfg> Registry for PackageRegistry<'cfg> {
    fn query(&mut self, dep: &Dependency) -> CraftResult<Vec<Summary>> {
        // Ensure the requested source_id is loaded
        try!(self.ensure_loaded(dep.source_id(), Kind::Normal)
            .chain_error(|| human(format!("failed to load source for a dependency on `{}`", dep.name()))));

        let override_summary = try!(self.query_overrides(&dep));
        let real_summaries = match self.sources.get_mut(dep.source_id()) {
            Some(src) => Some(try!(src.query(&dep))),
            None => None,
        };

        let ret = match (override_summary, real_summaries) {
            (Some(candidate), Some(summaries)) => {
                if summaries.len() != 1 {
                    bail!("found an override with a non-locked list");
                }
                try!(self.warn_bad_override(&candidate, &summaries[0]));
                vec![candidate]
            }
            (Some(_), None) => bail!("override found but no real ones"),
            (None, Some(summaries)) => summaries,
            (None, None) => Vec::new(),
        };

        // post-process all returned summaries to ensure that we lock all relevant summaries to the
        // right versions and sources
        Ok(ret.into_iter().map(|summary| self.lock(summary)).collect())
    }
}
