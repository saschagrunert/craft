pub use dependency::{Dependency, DependencyInner};
pub use manifest::{EitherManifest, VirtualManifest, Manifest, Target, TargetKind, Profile, LibKind, Profiles};
pub use package::{Package, PackageSet};
pub use package_id::{PackageId, Metadata};
pub use shell::{Shell, MultiShell, ShellConfig, Verbosity, ColorConfig};
pub use source::{Source, SourceId, SourceMap, GitReference};
pub use summary::Summary;
pub use workspace::{Workspace, WorkspaceConfig};
