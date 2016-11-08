use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{self, PathBuf};
use std::sync::Arc;
use std::{env, fs};

use rustc_serialize::json;
use shell::ColorConfig;

use manifest::{Profile, Profiles, Target};
use package::{Package, PackageSet};
use package_id::PackageId;
use resolver::Resolve;
use util::{self, CraftResult, ProcessBuilder, human, machine_message, Config, internal, ChainError, profile,
           join_paths, short_hash};
use workspace::Workspace;

use self::job::{Job, Work};
use self::job_queue::JobQueue;

pub use self::compilation::Compilation;
pub use self::context::{Context, Unit};
pub use self::layout::{Layout, LayoutProxy};
pub use self::custom_build::{BuildOutput, BuildMap, BuildScripts};

mod compilation;
mod context;
mod custom_build;
mod fingerprint;
mod job;
mod job_queue;
mod layout;
mod links;

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy, PartialOrd, Ord)]
pub enum Kind {
    Host,
    Target,
}

#[derive(Default, Clone)]
pub struct BuildConfig {
    pub host_triple: String,
    pub host: TargetConfig,
    pub requested_target: Option<String>,
    pub target: TargetConfig,
    pub jobs: u32,
    pub release: bool,
    pub test: bool,
    pub doc_all: bool,
    pub json_errors: bool,
}

#[derive(Clone, Default)]
pub struct TargetConfig {
    pub ar: Option<PathBuf>,
    pub linker: Option<PathBuf>,
    pub overrides: HashMap<String, BuildOutput>,
}

pub type PackagesToBuild<'a> = [(&'a Package, Vec<(&'a Target, &'a Profile)>)];

// Returns a mapping of the root package plus its immediate dependencies to
// where the compiled libraries are all located.
pub fn compile_targets<'a, 'cfg: 'a>(ws: &Workspace<'cfg>,
                                     pkg_targets: &'a PackagesToBuild<'a>,
                                     packages: &'a PackageSet<'cfg>,
                                     resolve: &'a Resolve,
                                     config: &'cfg Config,
                                     build_config: BuildConfig,
                                     profiles: &'a Profiles)
                                     -> CraftResult<Compilation<'cfg>> {
    let units = pkg_targets.iter()
        .flat_map(|&(pkg, ref targets)| {
            let default_kind = if build_config.requested_target.is_some() {
                Kind::Target
            } else {
                Kind::Host
            };
            targets.iter().map(move |&(target, profile)| {
                Unit {
                    pkg: pkg,
                    target: target,
                    profile: profile,
                    kind: if target.for_host() {
                        Kind::Host
                    } else {
                        default_kind
                    },
                }
            })
        })
        .collect::<Vec<_>>();

    let mut cx = try!(Context::new(ws, resolve, packages, config, build_config, profiles));

    let mut queue = JobQueue::new(&cx);

    try!(cx.prepare());
    try!(cx.probe_target_info(&units));
    try!(cx.build_used_in_plugin_map(&units));
    try!(custom_build::build_map(&mut cx, &units));

    for unit in units.iter() {
        // Build up a list of pending jobs, each of which represent
        // compiling a particular package. No actual work is executed as
        // part of this, that's all done next as part of the `execute`
        // function which will run everything in order with proper
        // parallelism.
        try!(compile(&mut cx, &mut queue, unit));
    }

    // Now that we've figured out everything that we're going to do, do it!
    try!(queue.execute(&mut cx));

    for unit in units.iter() {
        let out_dir = cx.layout(unit)
            .build_out(unit.pkg)
            .display()
            .to_string();
        cx.compilation
            .extra_env
            .entry(unit.pkg.package_id().clone())
            .or_insert(Vec::new())
            .push(("OUT_DIR".to_string(), out_dir));

        for (filename, _linkable) in try!(cx.target_filenames(unit)) {
            let dst = cx.out_dir(unit).join(filename);
            if unit.profile.test {
                cx.compilation.tests.push((unit.pkg.clone(), unit.target.name().to_string(), dst));
            } else if unit.target.is_bin() || unit.target.is_example() {
                cx.compilation.binaries.push(dst);
            } else if unit.target.is_lib() {
                let pkgid = unit.pkg.package_id().clone();
                cx.compilation
                    .libraries
                    .entry(pkgid)
                    .or_insert(Vec::new())
                    .push((unit.target.clone(), dst));
            }
            if !unit.target.is_lib() {
                continue;
            }

            // Include immediate lib deps as well
            for unit in try!(cx.dep_targets(unit)).iter() {
                let pkgid = unit.pkg.package_id();
                if !unit.target.is_lib() {
                    continue;
                }
                if unit.profile.doc {
                    continue;
                }
                if cx.compilation.libraries.contains_key(&pkgid) {
                    continue;
                }

                let v = try!(cx.target_filenames(unit));
                let v = v.into_iter()
                    .map(|(f, _)| (unit.target.clone(), cx.out_dir(unit).join(f)))
                    .collect::<Vec<_>>();
                cx.compilation.libraries.insert(pkgid.clone(), v);
            }
        }

        if let Some(feats) = cx.resolve.features(&unit.pkg.package_id()) {
            cx.compilation
                .cfgs
                .entry(unit.pkg.package_id().clone())
                .or_insert(HashSet::new())
                .extend(feats.iter().map(|feat| format!("feature=\"{}\"", feat)));
        }
    }

    for (&(ref pkg, _), output) in cx.build_state.outputs.lock().unwrap().iter() {
        cx.compilation
            .cfgs
            .entry(pkg.clone())
            .or_insert(HashSet::new())
            .extend(output.cfgs.iter().cloned());

        for dir in output.library_paths.iter() {
            cx.compilation.native_dirs.insert(dir.clone());
        }
    }
    cx.compilation.target = cx.target_triple().to_string();
    Ok(cx.compilation)
}

fn compile<'a, 'cfg: 'a>(cx: &mut Context<'a, 'cfg>, jobs: &mut JobQueue<'a>, unit: &Unit<'a>) -> CraftResult<()> {
    if !cx.compiled.insert(*unit) {
        return Ok(());
    }

    // Build up the work to be done to compile this unit, enqueuing it once
    // we've got everything constructed.
    let p = profile::start(format!("preparing: {}/{}", unit.pkg, unit.target.name()));
    try!(fingerprint::prepare_init(cx, unit));
    try!(cx.links.validate(unit));

    let (dirty, fresh, freshness) = if unit.profile.run_custom_build {
        try!(custom_build::prepare(cx, unit))
    } else {
        let (freshness, dirty, fresh) = try!(fingerprint::prepare_target(cx, unit));
        let work = if unit.profile.doc {
            try!(rustdoc(cx, unit))
        } else {
            try!(rustc(cx, unit))
        };
        let dirty = work.then(dirty);
        (dirty, fresh, freshness)
    };
    try!(jobs.enqueue(cx, unit, Job::new(dirty, fresh), freshness));
    drop(p);

    // Be sure to compile all dependencies of this target as well.
    for unit in try!(cx.dep_targets(unit)).iter() {
        try!(compile(cx, jobs, unit));
    }
    Ok(())
}

fn rustc(cx: &mut Context, unit: &Unit) -> CraftResult<Work> {
    let crate_types = unit.target.rustc_crate_types();
    let mut rustc = try!(prepare_rustc(cx, crate_types, unit));

    let name = unit.pkg.name().to_string();
    if !cx.show_warnings(unit.pkg.package_id()) {
        if try!(cx.config.rustc()).cap_lints {
            rustc.arg("--cap-lints").arg("allow");
        } else {
            rustc.arg("-Awarnings");
        }
    }
    let has_custom_args = unit.profile.rustc_args.is_some();

    let filenames = try!(cx.target_filenames(unit));
    let root = cx.out_dir(unit);

    // Prepare the native lib state (extra -L and -l flags)
    let build_state = cx.build_state.clone();
    let current_id = unit.pkg.package_id().clone();
    let build_deps = load_build_deps(cx, unit);

    // If we are a binary and the package also contains a library, then we
    // don't pass the `-l` flags.
    let pass_l_flag = unit.target.is_lib() || !unit.pkg.targets().iter().any(|t| t.is_lib());
    let do_rename = unit.target.allows_underscores() && !unit.profile.test;
    let real_name = unit.target.name().to_string();
    let crate_name = unit.target.crate_name();
    let move_outputs_up = unit.pkg.package_id() == &cx.current_package;

    let rustc_dep_info_loc = if do_rename {
            root.join(&crate_name)
        } else {
            root.join(&cx.file_stem(unit))
        }
        .with_extension("d");
    let dep_info_loc = fingerprint::dep_info_loc(cx, unit);
    let cwd = cx.config.cwd().to_path_buf();

    rustc.args(&try!(cx.rustflags_args(unit)));
    let json_errors = cx.build_config.json_errors;
    let package_id = unit.pkg.package_id().clone();
    let target = unit.target.clone();
    return Ok(Work::new(move |state| {
        // Only at runtime have we discovered what the extra -L and -l
        // arguments are for native libraries, so we process those here. We
        // also need to be sure to add any -L paths for our plugins to the
        // dynamic library load path as a plugin's dynamic library may be
        // located somewhere in there.
        if let Some(build_deps) = build_deps {
            let build_state = build_state.outputs.lock().unwrap();
            try!(add_native_deps(&mut rustc,
                                 &build_state,
                                 &build_deps,
                                 pass_l_flag,
                                 &current_id));
            try!(add_plugin_deps(&mut rustc, &build_state, &build_deps));
        }

        // FIXME(rust-lang/rust#18913): we probably shouldn't have to do
        //                              this manually
        for &(ref filename, _linkable) in filenames.iter() {
            let dst = root.join(filename);
            if fs::metadata(&dst).is_ok() {
                try!(fs::remove_file(&dst).chain_error(|| human(format!("Could not remove file: {}.", dst.display()))));
            }
        }

        state.running(&rustc);
        try!(if json_errors {
                rustc.exec_with_streaming(&mut |line| if !line.is_empty() {
                                             Err(internal(&format!("compiler stdout is not empty: `{}`", line)))
                                         } else {
                                             Ok(())
                                         },
                                         &mut |line| {
                        let compiler_message = try!(json::Json::from_str(line)
                            .map_err(|_| internal(&format!("compiler produced invalid json: `{}`", line))));

                        machine_message::FromCompiler::new(&package_id, &target, compiler_message).emit();

                        Ok(())
                    })
                    .map(|_| ())
            } else {
                rustc.exec()
            }
            .chain_error(|| human(format!("Could not compile `{}`.", name))));

        if do_rename && real_name != crate_name {
            let dst = root.join(&filenames[0].0);
            let src = dst.with_file_name(dst.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .replace(&real_name, &crate_name));
            if !has_custom_args || src.exists() {
                try!(fs::rename(&src, &dst).chain_error(|| internal(format!("could not rename crate {:?}", src))));
            }
        }

        if !has_custom_args || fs::metadata(&rustc_dep_info_loc).is_ok() {
            try!(fs::rename(&rustc_dep_info_loc, &dep_info_loc)
                .chain_error(|| internal(format!("could not rename dep info: {:?}", rustc_dep_info_loc))));
            try!(fingerprint::append_current_dir(&dep_info_loc, &cwd));
        }

        // If we're a "root crate", e.g. the target of this compilation, then we
        // hard link our outputs out of the `deps` directory into the directory
        // above. This means that `craft build` will produce binaries in
        // `target/debug` which one probably expects.
        if move_outputs_up {
            for &(ref filename, _linkable) in filenames.iter() {
                let src = root.join(filename);
                // This may have been a `craft rustc` command which changes the
                // output, so the source may not actually exist.
                if !src.exists() {
                    continue;
                }

                // We currently only lift files up from the `deps` directory. If
                // it was compiled into something like `example/` or `doc/` then
                // we don't want to link it up.
                let src_dir = src.parent().unwrap();
                if !src_dir.ends_with("deps") {
                    continue;
                }
                let dst = src_dir.parent()
                    .unwrap()
                    .join(src.file_name().unwrap());
                if dst.exists() {
                    try!(fs::remove_file(&dst).chain_error(|| human(format!("failed to remove: {}", dst.display()))));
                }
                try!(fs::hard_link(&src, &dst)
                    .or_else(|_| fs::copy(&src, &dst).map(|_| ()))
                    .chain_error(|| {
                        human(format!("failed to link or copy `{}` to `{}`",
                                      src.display(),
                                      dst.display()))
                    }));
            }
        }

        Ok(())
    }));

    // Add all relevant -L and -l flags from dependencies (now calculated and
    // present in `state`) to the command provided
    fn add_native_deps(rustc: &mut ProcessBuilder,
                       build_state: &BuildMap,
                       build_scripts: &BuildScripts,
                       pass_l_flag: bool,
                       current_id: &PackageId)
                       -> CraftResult<()> {
        for key in build_scripts.to_link.iter() {
            let output = try!(build_state.get(key)
                .chain_error(|| internal(format!("couldn't find build state for {}/{:?}", key.0, key.1))));
            for path in output.library_paths.iter() {
                rustc.arg("-L").arg(path);
            }
            if key.0 == *current_id {
                for cfg in &output.cfgs {
                    rustc.arg("--cfg").arg(cfg);
                }
                if pass_l_flag {
                    for name in output.library_links.iter() {
                        rustc.arg("-l").arg(name);
                    }
                }
            }
        }
        Ok(())
    }
}

fn load_build_deps(cx: &Context, unit: &Unit) -> Option<Arc<BuildScripts>> {
    cx.build_scripts.get(unit).cloned()
}

// For all plugin dependencies, add their -L paths (now calculated and
// present in `state`) to the dynamic library load path for the command to
// execute.
fn add_plugin_deps(rustc: &mut ProcessBuilder,
                   build_state: &BuildMap,
                   build_scripts: &BuildScripts)
                   -> CraftResult<()> {
    let var = util::dylib_path_envvar();
    let search_path = rustc.get_env(var).unwrap_or(OsString::new());
    let mut search_path = env::split_paths(&search_path).collect::<Vec<_>>();
    for id in build_scripts.plugins.iter() {
        let key = (id.clone(), Kind::Host);
        let output = try!(build_state.get(&key)
            .chain_error(|| internal(format!("couldn't find libs for plugin dep {}", id))));
        for path in output.library_paths.iter() {
            search_path.push(path.clone());
        }
    }
    let search_path = try!(join_paths(&search_path, var));
    rustc.env(var, &search_path);
    Ok(())
}

fn prepare_rustc(cx: &Context, crate_types: Vec<&str>, unit: &Unit) -> CraftResult<ProcessBuilder> {
    let mut base = try!(cx.compilation.rustc_process(unit.pkg));
    build_base_args(cx, &mut base, unit, &crate_types);
    build_plugin_args(&mut base, cx, unit);
    try!(build_deps_args(&mut base, cx, unit));
    Ok(base)
}


fn rustdoc(cx: &mut Context, unit: &Unit) -> CraftResult<Work> {
    let mut rustdoc = try!(cx.compilation.rustdoc_process(unit.pkg));
    rustdoc.arg(&root_path(cx, unit))
        .cwd(cx.config.cwd())
        .arg("--crate-name")
        .arg(&unit.target.crate_name());

    if unit.kind != Kind::Host {
        if let Some(target) = cx.requested_target() {
            rustdoc.arg("--target").arg(target);
        }
    }

    let doc_dir = cx.out_dir(unit);

    // Create the documentation directory ahead of time as rustdoc currently has
    // a bug where concurrent invocations will race to create this directory if
    // it doesn't already exist.
    try!(fs::create_dir_all(&doc_dir));

    rustdoc.arg("-o").arg(doc_dir);

    if let Some(features) = cx.resolve.features(unit.pkg.package_id()) {
        for feat in features {
            rustdoc.arg("--cfg").arg(&format!("feature=\"{}\"", feat));
        }
    }

    if let Some(ref args) = unit.profile.rustdoc_args {
        rustdoc.args(args);
    }

    try!(build_deps_args(&mut rustdoc, cx, unit));

    if unit.pkg.has_custom_build() {
        rustdoc.env("OUT_DIR", &cx.layout(unit).build_out(unit.pkg));
    }

    rustdoc.args(&try!(cx.rustdocflags_args(unit)));

    let name = unit.pkg.name().to_string();
    let build_state = cx.build_state.clone();
    let key = (unit.pkg.package_id().clone(), unit.kind);

    Ok(Work::new(move |state| {
        if let Some(output) = build_state.outputs.lock().unwrap().get(&key) {
            for cfg in output.cfgs.iter() {
                rustdoc.arg("--cfg").arg(cfg);
            }
        }
        state.running(&rustdoc);
        rustdoc.exec().chain_error(|| human(format!("Could not document `{}`.", name)))
    }))
}

// The path that we pass to rustc is actually fairly important because it will
// show up in error messages and the like. For this reason we take a few moments
// to ensure that something shows up pretty reasonably.
//
// The heuristic here is fairly simple, but the key idea is that the path is
// always "relative" to the current directory in order to be found easily. The
// path is only actually relative if the current directory is an ancestor if it.
// This means that non-path dependencies (git/registry) will likely be shown as
// absolute paths instead of relative paths.
fn root_path(cx: &Context, unit: &Unit) -> PathBuf {
    let absolute = unit.pkg.root().join(unit.target.src_path());
    let cwd = cx.config.cwd();
    if absolute.starts_with(cwd) {
        util::without_prefix(&absolute, cwd)
            .map(|s| s.to_path_buf())
            .unwrap_or(absolute)
    } else {
        absolute
    }
}

fn build_base_args(cx: &Context, cmd: &mut ProcessBuilder, unit: &Unit, crate_types: &[&str]) {
    let Profile { ref opt_level,
                  lto,
                  codegen_units,
                  ref rustc_args,
                  debuginfo,
                  debug_assertions,
                  rpath,
                  test,
                  doc: _doc,
                  run_custom_build,
                  ref panic,
                  rustdoc_args: _ } = *unit.profile;
    assert!(!run_custom_build);

    // Move to cwd so the root_path() passed below is actually correct
    cmd.cwd(cx.config.cwd());

    cmd.arg(&root_path(cx, unit));

    let color_config = cx.config.shell().color_config();
    if color_config != ColorConfig::Auto {
        cmd.arg("--color").arg(&color_config.to_string());
    }

    if cx.build_config.json_errors {
        cmd.arg("--error-format").arg("json");
    }

    cmd.arg("--crate-name").arg(&unit.target.crate_name());

    if !test {
        for crate_type in crate_types.iter() {
            cmd.arg("--crate-type").arg(crate_type);
        }
    }

    let prefer_dynamic = (unit.target.for_host() && !unit.target.is_custom_build()) ||
                         (crate_types.contains(&"dylib") && unit.pkg.package_id() != &cx.current_package);
    if prefer_dynamic {
        cmd.arg("-C").arg("prefer-dynamic");
    }

    if opt_level != "0" {
        cmd.arg("-C").arg(&format!("opt-level={}", opt_level));
    }

    // If a panic mode was configured *and* we're not ever going to be used in a
    // plugin, then we can compile with that panic mode.
    //
    // If we're used in a plugin then we'll eventually be linked to libsyntax
    // most likely which isn't compiled with a custom panic mode, so we'll just
    // get an error if we actually compile with that. This fixes `panic=abort`
    // crates which have plugin dependencies, but unfortunately means that
    // dependencies shared between the main application and plugins must be
    // compiled without `panic=abort`. This isn't so bad, though, as the main
    // application will still be compiled with `panic=abort`.
    if let Some(panic) = panic.as_ref() {
        if !cx.used_in_plugin.contains(unit) {
            cmd.arg("-C").arg(format!("panic={}", panic));
        }
    }

    // Disable LTO for host builds as prefer_dynamic and it are mutually
    // exclusive.
    if unit.target.can_lto() && lto && !unit.target.for_host() {
        cmd.args(&["-C", "lto"]);
    } else {
        // There are some restrictions with LTO and codegen-units, so we
        // only add codegen units when LTO is not used.
        if let Some(n) = codegen_units {
            cmd.arg("-C").arg(&format!("codegen-units={}", n));
        }
    }

    if debuginfo {
        cmd.arg("-g");
    }

    if let Some(ref args) = *rustc_args {
        cmd.args(args);
    }

    if debug_assertions && opt_level != "0" {
        cmd.args(&["-C", "debug-assertions=on"]);
    } else if !debug_assertions && opt_level == "0" {
        cmd.args(&["-C", "debug-assertions=off"]);
    }

    if test && unit.target.harness() {
        cmd.arg("--test");
    } else if test {
        cmd.arg("--cfg").arg("test");
    }

    if let Some(features) = cx.resolve.features(unit.pkg.package_id()) {
        for feat in features.iter() {
            cmd.arg("--cfg").arg(&format!("feature=\"{}\"", feat));
        }
    }

    match cx.target_metadata(unit) {
        Some(m) => {
            cmd.arg("-C").arg(&format!("metadata={}", m.metadata));
            cmd.arg("-C").arg(&format!("extra-filename={}", m.extra_filename));
        }
        None => {
            cmd.arg("-C").arg(&format!("metadata={}", short_hash(unit.pkg)));
        }
    }

    if rpath {
        cmd.arg("-C").arg("rpath");
    }
}


fn build_plugin_args(cmd: &mut ProcessBuilder, cx: &Context, unit: &Unit) {
    fn opt(cmd: &mut ProcessBuilder, key: &str, prefix: &str, val: Option<&OsStr>) {
        if let Some(val) = val {
            let mut joined = OsString::from(prefix);
            joined.push(val);
            cmd.arg(key).arg(joined);
        }
    }

    cmd.arg("--out-dir").arg(&cx.out_dir(unit));
    cmd.arg("--emit=dep-info,link");

    if unit.kind == Kind::Target {
        opt(cmd,
            "--target",
            "",
            cx.requested_target().map(|s| s.as_ref()));
    }

    opt(cmd, "-C", "ar=", cx.ar(unit.kind).map(|s| s.as_ref()));
    opt(cmd,
        "-C",
        "linker=",
        cx.linker(unit.kind).map(|s| s.as_ref()));
}

fn build_deps_args(cmd: &mut ProcessBuilder, cx: &Context, unit: &Unit) -> CraftResult<()> {
    let layout = cx.layout(unit);
    cmd.arg("-L").arg(&{
        let mut deps = OsString::from("dependency=");
        deps.push(layout.deps());
        deps
    });

    if unit.pkg.has_custom_build() {
        cmd.env("OUT_DIR", &layout.build_out(unit.pkg));
    }

    for unit in try!(cx.dep_targets(unit)).iter() {
        if unit.target.linkable() && !unit.profile.doc {
            try!(link_to(cmd, cx, unit));
        }
    }

    return Ok(());

    fn link_to(cmd: &mut ProcessBuilder, cx: &Context, unit: &Unit) -> CraftResult<()> {
        for (filename, linkable) in try!(cx.target_filenames(unit)) {
            if !linkable {
                continue;
            }
            let mut v = OsString::new();
            v.push(&unit.target.crate_name());
            v.push("=");
            v.push(cx.out_dir(unit));
            v.push(&path::MAIN_SEPARATOR.to_string());
            v.push(&filename);
            cmd.arg("--extern").arg(&v);
        }
        Ok(())
    }
}

fn envify(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_uppercase())
        .map(|c| if c == '-' { '_' } else { c })
        .collect()
}

impl Kind {
    fn for_target(&self, target: &Target) -> Kind {
        // Once we start compiling for the `Host` kind we continue doing so, but
        // if we are a `Target` kind and then we start compiling for a target
        // that needs to be on the host we lift ourselves up to `Host`
        match *self {
            Kind::Host => Kind::Host,
            Kind::Target if target.for_host() => Kind::Host,
            Kind::Target => Kind::Target,
        }
    }
}
