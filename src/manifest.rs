use std::fmt;
use std::path::{PathBuf, Path};

use semver::Version;
use rustc_serialize::{Encoder, Encodable};

use core::{Dependency, PackageId, SourceId};
use core::WorkspaceConfig;
use package_id::Metadata;
use package_id_spec::PackageIdSpec;
use summary::Summary;

pub enum EitherManifest {
    Real(Manifest),
    Virtual(VirtualManifest),
}

/// Contains all the information about a package, as loaded from a Craft.toml.
#[derive(Clone, Debug)]
pub struct Manifest {
    summary: Summary,
    targets: Vec<Target>,
    links: Option<String>,
    warnings: Vec<String>,
    exclude: Vec<String>,
    include: Vec<String>,
    metadata: ManifestMetadata,
    profiles: Profiles,
    publish: bool,
    replace: Vec<(PackageIdSpec, Dependency)>,
    workspace: WorkspaceConfig,
}

#[derive(Clone, Debug)]
pub struct VirtualManifest {
    replace: Vec<(PackageIdSpec, Dependency)>,
    workspace: WorkspaceConfig,
    profiles: Profiles,
}

/// General metadata about a package which is just blindly uploaded to the
/// registry.
///
/// Note that many of these fields can contain invalid values such as the
/// homepage, repository, documentation, or license. These fields are not
/// validated by craft itself, but rather it is up to the registry when uploaded
/// to validate these fields. Craft will itself accept any valid TOML
/// specification for these values.
#[derive(PartialEq, Clone, Debug)]
pub struct ManifestMetadata {
    pub authors: Vec<String>,
    pub keywords: Vec<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub description: Option<String>, // not markdown
    pub readme: Option<String>, // file, not contents
    pub homepage: Option<String>, // url
    pub repository: Option<String>, // url
    pub documentation: Option<String>, // url
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LibKind {
    Lib,
    Rlib,
    Dylib,
    ProcMacro,
    Other(String),
}

impl LibKind {
    pub fn from_str(string: &str) -> LibKind {
        match string {
            "lib" => LibKind::Lib,
            "rlib" => LibKind::Rlib,
            "dylib" => LibKind::Dylib,
            "procc-macro" => LibKind::ProcMacro,
            s => LibKind::Other(s.to_string()),
        }
    }

    /// Returns the argument suitable for `--crate-type` to pass to rustc.
    pub fn crate_type(&self) -> &str {
        match *self {
            LibKind::Lib => "lib",
            LibKind::Rlib => "rlib",
            LibKind::Dylib => "dylib",
            LibKind::ProcMacro => "proc-macro",
            LibKind::Other(ref s) => s,
        }
    }

    pub fn linkable(&self) -> bool {
        match *self {
            LibKind::Lib | LibKind::Rlib | LibKind::Dylib | LibKind::ProcMacro => true,
            LibKind::Other(..) => false,
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum TargetKind {
    Lib(Vec<LibKind>),
    Bin,
    Test,
    Bench,
    Example,
    CustomBuild,
}

impl Encodable for TargetKind {
    fn encode<S: Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        match *self {
                TargetKind::Lib(ref kinds) => kinds.iter().map(|k| k.crate_type()).collect(),
                TargetKind::Bin => vec!["bin"],
                TargetKind::Example => vec!["example"],
                TargetKind::Test => vec!["test"],
                TargetKind::CustomBuild => vec!["custom-build"],
                TargetKind::Bench => vec!["bench"],
            }
            .encode(s)
    }
}

#[derive(RustcEncodable, RustcDecodable, Clone, PartialEq, Eq, Debug, Hash)]
pub struct Profile {
    pub opt_level: String,
    pub lto: bool,
    pub codegen_units: Option<u32>, // None = use rustc default
    pub rustc_args: Option<Vec<String>>,
    pub rustdoc_args: Option<Vec<String>>,
    pub debuginfo: bool,
    pub debug_assertions: bool,
    pub rpath: bool,
    pub test: bool,
    pub doc: bool,
    pub run_custom_build: bool,
    pub panic: Option<String>,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Profiles {
    pub release: Profile,
    pub dev: Profile,
    pub test: Profile,
    pub test_deps: Profile,
    pub bench: Profile,
    pub bench_deps: Profile,
    pub doc: Profile,
    pub custom_build: Profile,
}

/// Information about a binary, a library, an example, etc. that is part of the
/// package.
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Target {
    kind: TargetKind,
    name: String,
    src_path: PathBuf,
    metadata: Option<Metadata>,
    tested: bool,
    benched: bool,
    doc: bool,
    doctest: bool,
    harness: bool, // whether to use the test harness (--test)
    for_host: bool,
}

#[derive(RustcEncodable)]
struct SerializedTarget<'a> {
    kind: &'a TargetKind,
    name: &'a str,
    src_path: &'a str,
}

impl Encodable for Target {
    fn encode<S: Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        SerializedTarget {
                kind: &self.kind,
                name: &self.name,
                src_path: &self.src_path.display().to_string(),
            }
            .encode(s)
    }
}

impl Manifest {
    pub fn new(summary: Summary,
               targets: Vec<Target>,
               exclude: Vec<String>,
               include: Vec<String>,
               links: Option<String>,
               metadata: ManifestMetadata,
               profiles: Profiles,
               publish: bool,
               replace: Vec<(PackageIdSpec, Dependency)>,
               workspace: WorkspaceConfig)
               -> Manifest {
        Manifest {
            summary: summary,
            targets: targets,
            warnings: Vec::new(),
            exclude: exclude,
            include: include,
            links: links,
            metadata: metadata,
            profiles: profiles,
            publish: publish,
            replace: replace,
            workspace: workspace,
        }
    }

    pub fn dependencies(&self) -> &[Dependency] {
        self.summary.dependencies()
    }

    pub fn exclude(&self) -> &[String] {
        &self.exclude
    }

    pub fn include(&self) -> &[String] {
        &self.include
    }

    pub fn metadata(&self) -> &ManifestMetadata {
        &self.metadata
    }

    pub fn name(&self) -> &str {
        self.package_id().name()
    }

    pub fn package_id(&self) -> &PackageId {
        self.summary.package_id()
    }

    pub fn summary(&self) -> &Summary {
        &self.summary
    }

    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    pub fn version(&self) -> &Version {
        self.package_id().version()
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn profiles(&self) -> &Profiles {
        &self.profiles
    }

    pub fn publish(&self) -> bool {
        self.publish
    }

    pub fn replace(&self) -> &[(PackageIdSpec, Dependency)] {
        &self.replace
    }

    pub fn links(&self) -> Option<&str> {
        self.links.as_ref().map(|s| &s[..])
    }

    pub fn workspace_config(&self) -> &WorkspaceConfig {
        &self.workspace
    }

    pub fn add_warning(&mut self, s: String) {
        self.warnings.push(s)
    }

    pub fn set_summary(&mut self, summary: Summary) {
        self.summary = summary;
    }

    pub fn map_source(self, to_replace: &SourceId, replace_with: &SourceId) -> Manifest {
        Manifest { summary: self.summary.map_source(to_replace, replace_with), ..self }
    }
}

impl VirtualManifest {
    pub fn new(replace: Vec<(PackageIdSpec, Dependency)>,
               workspace: WorkspaceConfig,
               profiles: Profiles)
               -> VirtualManifest {
        VirtualManifest {
            replace: replace,
            workspace: workspace,
            profiles: profiles,
        }
    }

    pub fn replace(&self) -> &[(PackageIdSpec, Dependency)] {
        &self.replace
    }

    pub fn workspace_config(&self) -> &WorkspaceConfig {
        &self.workspace
    }

    pub fn profiles(&self) -> &Profiles {
        &self.profiles
    }
}

impl Target {
    fn blank() -> Target {
        Target {
            kind: TargetKind::Bin,
            name: String::new(),
            src_path: PathBuf::new(),
            metadata: None,
            doc: false,
            doctest: false,
            harness: true,
            for_host: false,
            tested: true,
            benched: true,
        }
    }

    pub fn lib_target(name: &str, crate_targets: Vec<LibKind>, src_path: &Path, metadata: Metadata) -> Target {
        Target {
            kind: TargetKind::Lib(crate_targets),
            name: name.to_string(),
            src_path: src_path.to_path_buf(),
            metadata: Some(metadata),
            doctest: true,
            doc: true,
            ..Target::blank()
        }
    }

    pub fn bin_target(name: &str, src_path: &Path, metadata: Option<Metadata>) -> Target {
        Target {
            kind: TargetKind::Bin,
            name: name.to_string(),
            src_path: src_path.to_path_buf(),
            metadata: metadata,
            doc: true,
            ..Target::blank()
        }
    }

    /// Builds a `Target` corresponding to the `build = "build.rs"` entry.
    pub fn custom_build_target(name: &str, src_path: &Path, metadata: Option<Metadata>) -> Target {
        Target {
            kind: TargetKind::CustomBuild,
            name: name.to_string(),
            src_path: src_path.to_path_buf(),
            metadata: metadata,
            for_host: true,
            benched: false,
            tested: false,
            ..Target::blank()
        }
    }

    pub fn example_target(name: &str, src_path: &Path) -> Target {
        Target {
            kind: TargetKind::Example,
            name: name.to_string(),
            src_path: src_path.to_path_buf(),
            benched: false,
            ..Target::blank()
        }
    }

    pub fn test_target(name: &str, src_path: &Path, metadata: Metadata) -> Target {
        Target {
            kind: TargetKind::Test,
            name: name.to_string(),
            src_path: src_path.to_path_buf(),
            metadata: Some(metadata),
            benched: false,
            ..Target::blank()
        }
    }

    pub fn bench_target(name: &str, src_path: &Path, metadata: Metadata) -> Target {
        Target {
            kind: TargetKind::Bench,
            name: name.to_string(),
            src_path: src_path.to_path_buf(),
            metadata: Some(metadata),
            tested: false,
            ..Target::blank()
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn crate_name(&self) -> String {
        self.name.replace("-", "_")
    }

    pub fn src_path(&self) -> &Path {
        &self.src_path
    }

    pub fn metadata(&self) -> Option<&Metadata> {
        self.metadata.as_ref()
    }

    pub fn kind(&self) -> &TargetKind {
        &self.kind
    }

    pub fn tested(&self) -> bool {
        self.tested
    }

    pub fn harness(&self) -> bool {
        self.harness
    }

    pub fn documented(&self) -> bool {
        self.doc
    }

    pub fn for_host(&self) -> bool {
        self.for_host
    }

    pub fn benched(&self) -> bool {
        self.benched
    }

    pub fn doctested(&self) -> bool {
        self.doctest &&
        match self.kind {
            TargetKind::Lib(ref kinds) => kinds.contains(&LibKind::Rlib) || kinds.contains(&LibKind::Lib),
            _ => false,
        }
    }

    pub fn allows_underscores(&self) -> bool {
        self.is_bin() || self.is_example() || self.is_custom_build()
    }

    pub fn is_lib(&self) -> bool {
        match self.kind {
            TargetKind::Lib(_) => true,
            _ => false,
        }
    }

    pub fn linkable(&self) -> bool {
        match self.kind {
            TargetKind::Lib(ref kinds) => kinds.iter().any(|k| k.linkable()),
            _ => false,
        }
    }

    pub fn is_bin(&self) -> bool {
        self.kind == TargetKind::Bin
    }
    pub fn is_example(&self) -> bool {
        self.kind == TargetKind::Example
    }
    pub fn is_test(&self) -> bool {
        self.kind == TargetKind::Test
    }
    pub fn is_bench(&self) -> bool {
        self.kind == TargetKind::Bench
    }
    pub fn is_custom_build(&self) -> bool {
        self.kind == TargetKind::CustomBuild
    }

    /// Returns the arguments suitable for `--crate-type` to pass to rustc.
    pub fn rustc_crate_types(&self) -> Vec<&str> {
        match self.kind {
            TargetKind::Lib(ref kinds) => kinds.iter().map(|kind| kind.crate_type()).collect(),
            TargetKind::CustomBuild | TargetKind::Bench | TargetKind::Test | TargetKind::Example | TargetKind::Bin => {
                vec!["bin"]
            }
        }
    }

    pub fn can_lto(&self) -> bool {
        match self.kind {
            TargetKind::Lib(ref v) => {
                !v.contains(&LibKind::Rlib) && !v.contains(&LibKind::Dylib) && !v.contains(&LibKind::Lib)
            }
            _ => true,
        }
    }

    pub fn set_tested(&mut self, tested: bool) -> &mut Target {
        self.tested = tested;
        self
    }

    pub fn set_benched(&mut self, benched: bool) -> &mut Target {
        self.benched = benched;
        self
    }

    pub fn set_doctest(&mut self, doctest: bool) -> &mut Target {
        self.doctest = doctest;
        self
    }

    pub fn set_for_host(&mut self, for_host: bool) -> &mut Target {
        self.for_host = for_host;
        self
    }

    pub fn set_harness(&mut self, harness: bool) -> &mut Target {
        self.harness = harness;
        self
    }

    pub fn set_doc(&mut self, doc: bool) -> &mut Target {
        self.doc = doc;
        self
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            TargetKind::Lib(..) => write!(f, "Target(lib)"),
            TargetKind::Bin => write!(f, "Target(bin: {})", self.name),
            TargetKind::Test => write!(f, "Target(test: {})", self.name),
            TargetKind::Bench => write!(f, "Target(bench: {})", self.name),
            TargetKind::Example => write!(f, "Target(example: {})", self.name),
            TargetKind::CustomBuild => write!(f, "Target(script)"),
        }
    }
}

impl Profile {
    pub fn default_dev() -> Profile {
        Profile {
            debuginfo: true,
            debug_assertions: true,
            ..Profile::default()
        }
    }

    pub fn default_release() -> Profile {
        Profile {
            opt_level: "3".to_string(),
            debuginfo: false,
            ..Profile::default()
        }
    }

    pub fn default_test() -> Profile {
        Profile { test: true, ..Profile::default_dev() }
    }

    pub fn default_bench() -> Profile {
        Profile { test: true, ..Profile::default_release() }
    }

    pub fn default_doc() -> Profile {
        Profile { doc: true, ..Profile::default_dev() }
    }

    pub fn default_custom_build() -> Profile {
        Profile { run_custom_build: true, ..Profile::default_dev() }
    }
}

impl Default for Profile {
    fn default() -> Profile {
        Profile {
            opt_level: "0".to_string(),
            lto: false,
            codegen_units: None,
            rustc_args: None,
            rustdoc_args: None,
            debuginfo: false,
            debug_assertions: false,
            rpath: false,
            test: false,
            doc: false,
            run_custom_build: false,
            panic: None,
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.test {
            write!(f, "Profile(test)")
        } else if self.doc {
            write!(f, "Profile(doc)")
        } else if self.run_custom_build {
            write!(f, "Profile(run)")
        } else {
            write!(f, "Profile(build)")
        }

    }
}
