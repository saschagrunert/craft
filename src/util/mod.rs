//! General purpose utility functions and structures
pub use self::cfg::{Cfg, CfgExpr};
pub use self::config::Config;
pub use self::dependency_queue::{DependencyQueue, Fresh, Dirty, Freshness};
pub use self::errors::{CliError, ProcessError, CraftTestError, CraftResult, CraftError, ChainError, CliResult, Human,
                       caused_human, process_error, internal_error, internal, human};
pub use self::flock::{FileLock, Filesystem};
pub use self::graph::Graph;
pub use self::hex::{to_hex, short_hash, hash_u64};
pub use self::lazy_cell::LazyCell;
pub use self::lev_distance::lev_distance;
pub use self::paths::{join_paths, path2bytes, bytes2path, dylib_path, normalize_path, dylib_path_envvar,
                      without_prefix};
pub use self::process_builder::{process, ProcessBuilder};
pub use self::read2::read2;
pub use self::rustc::Rustc;
pub use self::sha256::Sha256;
pub use self::to_semver::ToSemver;
pub use self::to_url::ToUrl;
pub use self::vcs::{GitRepo, HgRepo};

pub mod config;
pub mod errors;
pub mod graph;
pub mod hex;
pub mod important_paths;
pub mod job;
pub mod lev_distance;
pub mod machine_message;
pub mod network;
pub mod paths;
pub mod process_builder;
pub mod profile;
pub mod to_semver;
pub mod to_url;
pub mod toml;
mod cfg;
mod dependency_queue;
mod flock;
mod lazy_cell;
mod read2;
mod rustc;
mod sha256;
mod shell_escape;
mod vcs;
