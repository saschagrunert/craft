use std::collections::{HashSet, HashMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::str::{self, FromStr};
use std::sync::Arc;

use dependency::Dependency;
use dependency::Kind as DepKind;
use manifest::{Target, Profile, TargetKind, Profiles};
use package::{Package, PackageSet};
use package_id::{PackageId, Metadata};
use resolver::Resolve;
use util::{CraftResult, ChainError, internal, Config, profile, Cfg, human};
use workspace::Workspace;

use super::TargetConfig;
use super::custom_build::{BuildState, BuildScripts};
use super::fingerprint::Fingerprint;
use super::layout::{Layout, LayoutProxy};
use super::links::Links;
use super::{Kind, Compilation, BuildConfig};

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct Unit<'a> {
    pub pkg: &'a Package,
    pub target: &'a Target,
    pub profile: &'a Profile,
    pub kind: Kind,
}

pub struct Context<'a, 'cfg: 'a> {
    pub config: &'cfg Config,
    pub resolve: &'a Resolve,
    pub current_package: PackageId,
    pub compilation: Compilation<'cfg>,
    pub packages: &'a PackageSet<'cfg>,
    pub build_state: Arc<BuildState>,
    pub build_explicit_deps: HashMap<Unit<'a>, (PathBuf, Vec<String>)>,
    pub fingerprints: HashMap<Unit<'a>, Arc<Fingerprint>>,
    pub compiled: HashSet<Unit<'a>>,
    pub build_config: BuildConfig,
    pub build_scripts: HashMap<Unit<'a>, Arc<BuildScripts>>,
    pub links: Links<'a>,
    pub used_in_plugin: HashSet<Unit<'a>>,

    host: Layout,
    target: Option<Layout>,
    target_info: TargetInfo,
    host_info: TargetInfo,
    profiles: &'a Profiles,
}

#[derive(Clone, Default)]
struct TargetInfo {
    chest_types: HashMap<String, Option<(String, String)>>,
    cfg: Option<Vec<Cfg>>,
}

impl<'a, 'cfg> Context<'a, 'cfg> {
    pub fn new(ws: &Workspace<'cfg>,
               resolve: &'a Resolve,
               packages: &'a PackageSet<'cfg>,
               config: &'cfg Config,
               build_config: BuildConfig,
               profiles: &'a Profiles)
               -> CraftResult<Context<'a, 'cfg>> {

        let dest = if build_config.release {
            "release"
        } else {
            "debug"
        };
        let host_layout = Layout::new(ws, None, &dest)?;
        let target_layout = match build_config.requested_target.as_ref() {
            Some(target) => Some(Layout::new(ws, Some(&target), &dest)?),
            None => None,
        };

        let current_package = ws.current()?.package_id().clone();
        Ok(Context {
            host: host_layout,
            target: target_layout,
            resolve: resolve,
            current_package: current_package,
            packages: packages,
            config: config,
            target_info: TargetInfo::default(),
            host_info: TargetInfo::default(),
            compilation: Compilation::new(config),
            build_state: Arc::new(BuildState::new(&build_config)),
            build_config: build_config,
            fingerprints: HashMap::new(),
            profiles: profiles,
            compiled: HashSet::new(),
            build_scripts: HashMap::new(),
            build_explicit_deps: HashMap::new(),
            links: Links::new(),
            used_in_plugin: HashSet::new(),
        })
    }

    /// Prepare this context, ensuring that all filesystem directories are in
    /// place.
    pub fn prepare(&mut self) -> CraftResult<()> {
        let _p = profile::start("preparing layout");

        self.host.prepare().chain_error(|| internal(format!("couldn't prepare build directories")))?;
        match self.target {
            Some(ref mut target) => {
                target.prepare().chain_error(|| internal(format!("couldn't prepare build directories")))?;
            }
            None => {}
        }

        self.compilation.plugins_dylib_path = self.host.deps().to_path_buf();

        let layout = self.target.as_ref().unwrap_or(&self.host);
        self.compilation.root_output = layout.dest().to_path_buf();
        self.compilation.deps_output = layout.deps().to_path_buf();
        Ok(())
    }

    /// Ensure that we've collected all target-specific information to compile
    /// all the units mentioned in `units`.
    pub fn probe_target_info(&mut self, units: &[Unit<'a>]) -> CraftResult<()> {
        let mut chest_types = BTreeSet::new();
        // pre-fill with `bin` for learning about tests (nothing may be
        // explicitly `bin`) as well as `rlib` as it's the coalesced version of
        // `lib` in the compiler and we're not sure which we'll see.
        chest_types.insert("bin".to_string());
        chest_types.insert("rlib".to_string());
        for unit in units {
            self.visit_chest_type(unit, &mut chest_types)?;
        }
        self.probe_target_info_kind(&chest_types, Kind::Target)?;
        if self.requested_target().is_none() {
            self.host_info = self.target_info.clone();
        } else {
            self.probe_target_info_kind(&chest_types, Kind::Host)?;
        }
        Ok(())
    }

    fn visit_chest_type(&self, unit: &Unit<'a>, chest_types: &mut BTreeSet<String>) -> CraftResult<()> {
        for target in unit.pkg.manifest().targets() {
            chest_types.extend(target.cc_chest_types().iter().map(|s| {
                if *s == "lib" {
                    "rlib".to_string()
                } else {
                    s.to_string()
                }
            }));
        }
        for dep in self.dep_targets(&unit)? {
            self.visit_chest_type(&dep, chest_types)?;
        }
        Ok(())
    }

    fn probe_target_info_kind(&mut self, chest_types: &BTreeSet<String>, kind: Kind) -> CraftResult<()> {
        let cflags = env_args(self.config, &self.build_config, kind, "CFLAGS")?;
        let mut process = self.config.cc()?.process();
        process.arg("-")
            .arg("--crate-name")
            .arg("_")
            .arg("--print=file-names")
            .args(&cflags)
            .env_remove("RUST_LOG");

        for chest_type in chest_types {
            process.arg("--crate-type").arg(chest_type);
        }
        if kind == Kind::Target {
            process.arg("--target").arg(&self.target_triple());
        }

        let mut with_cfg = process.clone();
        with_cfg.arg("--print=cfg");

        let mut has_cfg = true;
        let output = with_cfg.exec_with_output()
            .or_else(|_| {
                has_cfg = false;
                process.exec_with_output()
            })
            .chain_error(|| human(format!("failed to run `cc` to learn about target-specific information")))?;

        let error = str::from_utf8(&output.stderr).unwrap();
        let output = str::from_utf8(&output.stdout).unwrap();
        let mut lines = output.lines();
        let mut map = HashMap::new();
        for chest_type in chest_types {
            let not_supported = error.lines()
                .any(|line| line.contains("unsupported chest type") && line.contains(chest_type));
            if not_supported {
                map.insert(chest_type.to_string(), None);
                continue;
            }
            let line = match lines.next() {
                Some(line) => line,
                None => bail!("malformed output when learning about target-specific information from cc"),
            };
            let mut parts = line.trim().split('_');
            let prefix = parts.next().unwrap();
            let suffix = match parts.next() {
                Some(part) => part,
                None => bail!("output of --print=file-names has changed in the compiler, cannot parse"),
            };
            map.insert(chest_type.to_string(),
                       Some((prefix.to_string(), suffix.to_string())));
        }

        let cfg = if has_cfg {
            Some(try!(lines.map(Cfg::from_str).collect()))
        } else {
            None
        };

        let info = match kind {
            Kind::Target => &mut self.target_info,
            Kind::Host => &mut self.host_info,
        };
        info.chest_types = map;
        info.cfg = cfg;
        Ok(())
    }

    /// Builds up the `used_in_plugin` internal to this context from the list of
    /// top-level units.
    ///
    /// This will recursively walk `units` and all of their dependencies to
    /// determine which chest are going to be used in plugins or not.
    pub fn build_used_in_plugin_map(&mut self, units: &[Unit<'a>]) -> CraftResult<()> {
        let mut visited = HashSet::new();
        for unit in units {
            self.walk_used_in_plugin_map(unit, unit.target.for_host(), &mut visited)?;
        }
        Ok(())
    }

    fn walk_used_in_plugin_map(&mut self,
                               unit: &Unit<'a>,
                               is_plugin: bool,
                               visited: &mut HashSet<(Unit<'a>, bool)>)
                               -> CraftResult<()> {
        if !visited.insert((*unit, is_plugin)) {
            return Ok(());
        }
        if is_plugin {
            self.used_in_plugin.insert(*unit);
        }
        for unit in self.dep_targets(unit)? {
            self.walk_used_in_plugin_map(&unit, is_plugin || unit.target.for_host(), visited)?;
        }
        Ok(())
    }

    /// Returns the appropriate directory layout for either a plugin or not.
    pub fn layout(&self, unit: &Unit) -> LayoutProxy {
        let primary = unit.pkg.package_id() == &self.current_package;
        match unit.kind {
            Kind::Host => LayoutProxy::new(&self.host, primary),
            Kind::Target => {
                LayoutProxy::new(self.target
                                     .as_ref()
                                     .unwrap_or(&self.host),
                                 primary)
            }
        }
    }

    /// Returns the appropriate output directory for the specified package and
    /// target.
    pub fn out_dir(&self, unit: &Unit) -> PathBuf {
        if unit.profile.doc {
            self.layout(unit).doc_root()
        } else {
            self.layout(unit).out_dir(unit)
        }
    }

    /// Return the host triple for this context
    pub fn host_triple(&self) -> &str {
        &self.build_config.host_triple
    }

    /// Return the target triple which this context is targeting.
    pub fn target_triple(&self) -> &str {
        self.requested_target().unwrap_or(self.host_triple())
    }

    /// Requested (not actual) target for the build
    pub fn requested_target(&self) -> Option<&str> {
        self.build_config.requested_target.as_ref().map(|s| &s[..])
    }

    /// Get the metadata for a target in a specific profile
    pub fn target_metadata(&self, unit: &Unit) -> Option<Metadata> {
        let metadata = unit.target.metadata();
        if unit.target.is_lib() && unit.profile.test {
            // Libs and their tests are built in parallel, so we need to make
            // sure that their metadata is different.
            metadata.cloned().map(|mut m| {
                m.mix(&"test");
                m
            })
        } else if unit.target.is_bin() && unit.profile.test {
            // Make sure that the name of this test executable doesn't
            // conflict with a library that has the same name and is
            // being tested
            let mut metadata = unit.pkg.generate_metadata();
            metadata.mix(&format!("bin-{}", unit.target.name()));
            Some(metadata)
        } else if unit.pkg.package_id().source_id().is_path() && !unit.profile.test {
            // If we're not building a unit test but we're building a path
            // dependency, then we're likely compiling the "current package" or
            // some package in a workspace. In this situation we pass no
            // metadata by default so we'll have predictable
            // file names like `target/debug/libfoo.{a,so,rlib}` and such.
            //
            // Note, though, that the compiler's build system at least wants
            // path dependencies to have hashes in filenames. To account for
            // that we have an extra hack here which reads the
            // `__CRAFT_DEFAULT_METADATA` environment variable and creates a
            // hash in the filename if that's present.
            //
            // This environment variable should not be relied on! It's basically
            // just here for rustbuild. We need a more principled method of
            // doing this eventually.
            if unit.target.is_lib() {
                env::var("__CRAFT_DEFAULT_LIB_METADATA").ok().map(|meta| {
                    let mut metadata = unit.pkg.generate_metadata();
                    metadata.mix(&meta);
                    metadata
                })
            } else {
                None
            }
        } else {
            metadata.cloned()
        }
    }

    /// Returns the file stem for a given target/profile combo
    pub fn file_stem(&self, unit: &Unit) -> String {
        match self.target_metadata(unit) {
            Some(ref metadata) => format!("{}{}", unit.target.chest_name(), metadata.extra_filename),
            None if unit.target.allows_underscores() => unit.target.name().to_string(),
            None => unit.target.chest_name(),
        }
    }

    /// Return the filenames that the given target for the given profile will
    /// generate, along with whether you can link against that file (e.g. it's a
    /// library).
    pub fn target_filenames(&self, unit: &Unit) -> CraftResult<Vec<(String, bool)>> {
        let stem = self.file_stem(unit);
        let info = if unit.target.for_host() {
            &self.host_info
        } else {
            &self.target_info
        };

        let mut ret = Vec::new();
        let mut unsupported = Vec::new();
        {
            let mut add = |chest_type: &str, linkable: bool| -> CraftResult<()> {
                let chest_type = if chest_type == "lib" {
                    "rlib"
                } else {
                    chest_type
                };
                match info.chest_types.get(chest_type) {
                    Some(&Some((ref prefix, ref suffix))) => {
                        ret.push((format!("{}{}{}", prefix, stem, suffix), linkable));
                        Ok(())
                    }
                    // not supported, don't worry about it
                    Some(&None) => {
                        unsupported.push(chest_type.to_string());
                        Ok(())
                    }
                    None => bail!("failed to learn about chest-type `{}` early on", chest_type),
                }
            };
            match *unit.target.kind() {
                TargetKind::Example | TargetKind::Bin | TargetKind::CustomBuild | TargetKind::Bench |
                TargetKind::Test => {
                    add("bin", false)?;
                }
                TargetKind::Lib(..) if unit.profile.test => {
                    add("bin", false)?;
                }
                TargetKind::Lib(ref libs) => {
                    for lib in libs {
                        add(lib.chest_type(), lib.linkable())?;
                    }
                }
            }
        }
        if ret.is_empty() {
            if unsupported.len() > 0 {
                bail!("cannot produce {} for `{}` as the target `{}` \
                       does not support these chest types",
                      unsupported.join(", "),
                      unit.pkg,
                      self.target_triple())
            }
            bail!("cannot compile `{}` as the target `{}` does not \
                   support any of the output chest types",
                  unit.pkg,
                  self.target_triple());
        }
        Ok(ret)
    }

    /// For a package, return all targets which are registered as dependencies
    /// for that package.
    pub fn dep_targets(&self, unit: &Unit<'a>) -> CraftResult<Vec<Unit<'a>>> {
        if unit.profile.run_custom_build {
            return self.dep_run_custom_build(unit);
        } else if unit.profile.doc {
            return self.doc_deps(unit);
        }

        let id = unit.pkg.package_id();
        let deps = self.resolve.deps(id);
        let mut ret = deps.filter(|dep| {
                unit.pkg
                    .dependencies()
                    .iter()
                    .filter(|d| d.name() == dep.name() && d.version_req().matches(dep.version()))
                    .any(|d| {
                        // If this target is a build command, then we only want build
                        // dependencies, otherwise we want everything *other than* build
                        // dependencies.
                        if unit.target.is_custom_build() != d.is_build() {
                            return false;
                        }

                        // If this dependency is *not* a transitive dependency, then it
                        // only applies to test/example targets
                        if !d.is_transitive() && !unit.target.is_test() && !unit.target.is_example() &&
                           !unit.profile.test {
                            return false;
                        }

                        // If this dependency is only available for certain platforms,
                        // make sure we're only enabling it for that platform.
                        if !self.dep_platform_activated(d, unit.kind) {
                            return false;
                        }

                        // If the dependency is optional, then we're only activating it
                        // if the corresponding feature was activated
                        if d.is_optional() {
                            match self.resolve.features(id) {
                                Some(f) if f.contains(d.name()) => {}
                                _ => return false,
                            }
                        }

                        // If we've gotten past all that, then this dependency is
                        // actually used!
                        true
                    })
            })
            .filter_map(|id| {
                match self.get_package(id) {
                    Ok(pkg) => {
                        pkg.targets().iter().find(|t| t.is_lib()).map(|t| {
                            Ok(Unit {
                                pkg: pkg,
                                target: t,
                                profile: self.lib_profile(id),
                                kind: unit.kind.for_target(t),
                            })
                        })
                    }
                    Err(e) => Some(Err(e)),
                }
            })
            .collect::<CraftResult<Vec<_>>>()?;

        // If this target is a build script, then what we've collected so far is
        // all we need. If this isn't a build script, then it depends on the
        // build script if there is one.
        if unit.target.is_custom_build() {
            return Ok(ret);
        }
        ret.extend(self.dep_build_script(unit));

        // If this target is a binary, test, example, etc, then it depends on
        // the library of the same package. The call to `resolve.deps` above
        // didn't include `pkg` in the return values, so we need to special case
        // it here and see if we need to push `(pkg, pkg_lib_target)`.
        if unit.target.is_lib() {
            return Ok(ret);
        }
        ret.extend(self.maybe_lib(unit));

        // Integration tests/benchmarks require binaries to be built
        if unit.profile.test && (unit.target.is_test() || unit.target.is_bench()) {
            ret.extend(unit.pkg.targets().iter().filter(|t| t.is_bin()).map(|t| {
                Unit {
                    pkg: unit.pkg,
                    target: t,
                    profile: self.lib_profile(id),
                    kind: unit.kind.for_target(t),
                }
            }));
        }
        Ok(ret)
    }

    /// Returns the dependencies needed to run a build script.
    ///
    /// The `unit` provided must represent an execution of a build script, and
    /// the returned set of units must all be run before `unit` is run.
    pub fn dep_run_custom_build(&self, unit: &Unit<'a>) -> CraftResult<Vec<Unit<'a>>> {
        // If this build script's execution has been overridden then we don't
        // actually depend on anything, we've reached the end of the dependency
        // chain as we've got all the info we're gonna get.
        let key = (unit.pkg.package_id().clone(), unit.kind);
        if self.build_state.outputs.lock().unwrap().contains_key(&key) {
            return Ok(Vec::new());
        }

        // When not overridden, then the dependencies to run a build script are:
        //
        // 1. Compiling the build script itself
        // 2. For each immediate dependency of our package which has a `links`
        //    key, the execution of that build script.
        let not_custom_build = unit.pkg
            .targets()
            .iter()
            .find(|t| !t.is_custom_build())
            .unwrap();
        let tmp = Unit {
            target: not_custom_build,
            profile: &self.profiles.dev,
            ..*unit
        };
        let deps = self.dep_targets(&tmp)?;
        Ok(deps.iter()
            .filter_map(|unit| {
                if !unit.target.linkable() || unit.pkg.manifest().links().is_none() {
                    return None;
                }
                self.dep_build_script(unit)
            })
            .chain(Some(Unit {
                profile: self.build_script_profile(unit.pkg.package_id()),
                kind: Kind::Host, // build scripts always compiled for the host
                ..*unit
            }))
            .collect())
    }

    /// Returns the dependencies necessary to document a package
    fn doc_deps(&self, unit: &Unit<'a>) -> CraftResult<Vec<Unit<'a>>> {
        let deps = self.resolve
            .deps(unit.pkg.package_id())
            .filter(|dep| {
                unit.pkg
                    .dependencies()
                    .iter()
                    .filter(|d| d.name() == dep.name())
                    .any(|dep| {
                        match dep.kind() {
                            DepKind::Normal => self.dep_platform_activated(dep, unit.kind),
                            _ => false,
                        }
                    })
            })
            .map(|dep| self.get_package(dep));

        // To document a library, we depend on dependencies actually being
        // built. If we're documenting *all* libraries, then we also depend on
        // the documentation of the library being built.
        let mut ret = Vec::new();
        for dep in deps {
            let dep = dep?;
            let lib = match dep.targets().iter().find(|t| t.is_lib()) {
                Some(lib) => lib,
                None => continue,
            };
            ret.push(Unit {
                pkg: dep,
                target: lib,
                profile: self.lib_profile(dep.package_id()),
                kind: unit.kind.for_target(lib),
            });
            if self.build_config.doc_all {
                ret.push(Unit {
                    pkg: dep,
                    target: lib,
                    profile: &self.profiles.doc,
                    kind: unit.kind.for_target(lib),
                });
            }
        }

        // Be sure to build/run the build script for documented libraries as
        ret.extend(self.dep_build_script(unit));

        // If we document a binary, we need the library available
        if unit.target.is_bin() {
            ret.extend(self.maybe_lib(unit));
        }
        Ok(ret)
    }

    /// If a build script is scheduled to be run for the package specified by
    /// `unit`, this function will return the unit to run that build script.
    ///
    /// Overriding a build script simply means that the running of the build
    /// script itself doesn't have any dependencies, so even in that case a unit
    /// of work is still returned. `None` is only returned if the package has no
    /// build script.
    fn dep_build_script(&self, unit: &Unit<'a>) -> Option<Unit<'a>> {
        unit.pkg.targets().iter().find(|t| t.is_custom_build()).map(|t| {
            Unit {
                pkg: unit.pkg,
                target: t,
                profile: &self.profiles.custom_build,
                kind: unit.kind,
            }
        })
    }

    fn maybe_lib(&self, unit: &Unit<'a>) -> Option<Unit<'a>> {
        unit.pkg.targets().iter().find(|t| t.linkable()).map(|t| {
            Unit {
                pkg: unit.pkg,
                target: t,
                profile: self.lib_profile(unit.pkg.package_id()),
                kind: unit.kind.for_target(t),
            }
        })
    }

    fn dep_platform_activated(&self, dep: &Dependency, kind: Kind) -> bool {
        // If this dependency is only available for certain platforms,
        // make sure we're only enabling it for that platform.
        let platform = match dep.platform() {
            Some(p) => p,
            None => return true,
        };
        let (name, info) = match kind {
            Kind::Host => (self.host_triple(), &self.host_info),
            Kind::Target => (self.target_triple(), &self.target_info),
        };
        platform.matches(name, info.cfg.as_ref().map(|cfg| &cfg[..]))
    }

    /// Gets a package for the given package id.
    pub fn get_package(&self, id: &PackageId) -> CraftResult<&'a Package> {
        self.packages.get(id)
    }

    /// Get the user-specified linker for a particular host or target
    pub fn linker(&self, kind: Kind) -> Option<&Path> {
        self.target_config(kind).linker.as_ref().map(|s| s.as_ref())
    }

    /// Get the user-specified `ar` program for a particular host or target
    pub fn ar(&self, kind: Kind) -> Option<&Path> {
        self.target_config(kind).ar.as_ref().map(|s| s.as_ref())
    }

    /// Get the list of cfg printed out from the compiler for the specified kind
    pub fn cfg(&self, kind: Kind) -> &[Cfg] {
        let info = match kind {
            Kind::Host => &self.host_info,
            Kind::Target => &self.target_info,
        };
        info.cfg.as_ref().map(|s| &s[..]).unwrap_or(&[])
    }

    /// Get the target configuration for a particular host or target
    fn target_config(&self, kind: Kind) -> &TargetConfig {
        match kind {
            Kind::Host => &self.build_config.host,
            Kind::Target => &self.build_config.target,
        }
    }

    /// Number of jobs specified for this build
    pub fn jobs(&self) -> u32 {
        self.build_config.jobs
    }

    pub fn lib_profile(&self, _pkg: &PackageId) -> &'a Profile {
        let (normal, test) = if self.build_config.release {
            (&self.profiles.release, &self.profiles.bench_deps)
        } else {
            (&self.profiles.dev, &self.profiles.test_deps)
        };
        if self.build_config.test { test } else { normal }
    }

    pub fn build_script_profile(&self, pkg: &PackageId) -> &'a Profile {
        // TODO: should build scripts always be built with the same library
        //       profile? How is this controlled at the CLI layer?
        self.lib_profile(pkg)
    }

    pub fn cflags_args(&self, unit: &Unit) -> CraftResult<Vec<String>> {
        env_args(self.config, &self.build_config, unit.kind, "CFLAGS")
    }

    pub fn docflags_args(&self, unit: &Unit) -> CraftResult<Vec<String>> {
        env_args(self.config, &self.build_config, unit.kind, "DOCFLAGS")
    }

    pub fn show_warnings(&self, pkg: &PackageId) -> bool {
        pkg == &self.current_package || pkg.source_id().is_path() || self.config.extra_verbose()
    }
}

// Acquire extra flags to pass to the compiler from the
// CFLAGS environment variable and similar config values
fn env_args(config: &Config, build_config: &BuildConfig, kind: Kind, name: &str) -> CraftResult<Vec<String>> {
    // We *want* to apply CFLAGS only to builds for the
    // requested target architecture, and not to things like build
    // scripts and plugins, which may be for an entirely different
    // architecture. Craft's present architecture makes it quite
    // hard to only apply flags to things that are not build
    // scripts and plugins though, so we do something more hacky
    // instead to avoid applying the same CFLAGS to multiple targets
    // arches:
    //
    // 1) If --target is not specified we just apply CFLAGS to
    // all builds; they are all going to have the same target.
    //
    // 2) If --target *is* specified then we only apply CFLAGS
    // to compilation units with the Target kind, which indicates
    // it was chosen by the --target flag.
    //
    // This means that, e.g. even if the specified --target is the
    // same as the host, build scripts in plugins won't get
    // CFLAGS.
    let compiling_with_target = build_config.requested_target.is_some();
    let is_target_kind = kind == Kind::Target;

    if compiling_with_target && !is_target_kind {
        // This is probably a build script or plugin and we're
        // compiling with --target. In this scenario there are
        // no cflags we can apply.
        return Ok(Vec::new());
    }

    // First try CFLAGS from the environment
    if let Some(a) = env::var(name).ok() {
        let args = a.split(" ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        return Ok(args.collect());
    }

    let name = name.chars().flat_map(|c| c.to_lowercase()).collect::<String>();
    // Then the target.*.cflags value
    let target = build_config.requested_target.as_ref().unwrap_or(&build_config.host_triple);
    let key = format!("target.{}.{}", target, name);
    if let Some(args) = config.get_list(&key)? {
        let args = args.val.into_iter().map(|a| a.0);
        return Ok(args.collect());
    }

    // Then the build.cflags value
    let key = format!("build.{}", name);
    if let Some(args) = config.get_list(&key)? {
        let args = args.val.into_iter().map(|a| a.0);
        return Ok(args.collect());
    }

    Ok(Vec::new())
}
