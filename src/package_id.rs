use std::cmp::Ordering;
use std::error::Error;
use std::fmt::{self, Formatter};
use std::hash::Hash;
use std::hash;
use std::sync::Arc;

use regex::Regex;
use rustc_serialize::{Encodable, Encoder, Decodable, Decoder};
use semver;

use util::{CraftResult, CraftError, short_hash, ToSemver};
use source::SourceId;

/// Identifier for a specific version of a package in a specific source.
#[derive(Clone)]
pub struct PackageId {
    inner: Arc<PackageIdInner>,
}

#[derive(PartialEq, PartialOrd, Eq, Ord)]
struct PackageIdInner {
    name: String,
    version: semver::Version,
    source_id: SourceId,
}

impl Encodable for PackageId {
    fn encode<S: Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        let source = self.inner.source_id.to_url();
        let encoded = format!("{} {} ({})", self.inner.name, self.inner.version, source);
        encoded.encode(s)
    }
}

impl Decodable for PackageId {
    fn decode<D: Decoder>(d: &mut D) -> Result<PackageId, D::Error> {
        let string: String = try!(Decodable::decode(d));
        let regex = Regex::new(r"^([^ ]+) ([^ ]+) \(([^\)]+)\)$").unwrap();
        let captures = try!(regex.captures(&string).ok_or_else(|| d.error("invalid serialized PackageId")));

        let name = captures.at(1).unwrap();
        let version = captures.at(2).unwrap();
        let url = captures.at(3).unwrap();
        let version = try!(semver::Version::parse(version).map_err(|_| d.error("invalid version")));
        let source_id = try!(SourceId::from_url(url).map_err(|e| d.error(&e.to_string())));

        Ok(PackageId {
            inner: Arc::new(PackageIdInner {
                name: name.to_string(),
                version: version,
                source_id: source_id,
            }),
        })
    }
}

impl Hash for PackageId {
    fn hash<S: hash::Hasher>(&self, state: &mut S) {
        self.inner.name.hash(state);
        self.inner.version.hash(state);
        self.inner.source_id.hash(state);
    }
}

impl PartialEq for PackageId {
    fn eq(&self, other: &PackageId) -> bool {
        (*self.inner).eq(&*other.inner)
    }
}
impl PartialOrd for PackageId {
    fn partial_cmp(&self, other: &PackageId) -> Option<Ordering> {
        (*self.inner).partial_cmp(&*other.inner)
    }
}
impl Eq for PackageId {}
impl Ord for PackageId {
    fn cmp(&self, other: &PackageId) -> Ordering {
        (*self.inner).cmp(&*other.inner)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PackageIdError {
    InvalidVersion(String),
    InvalidNamespace(String),
}

impl Error for PackageIdError {
    fn description(&self) -> &str {
        "failed to parse package id"
    }
}

impl fmt::Display for PackageIdError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            PackageIdError::InvalidVersion(ref v) => write!(f, "invalid version: {}", *v),
            PackageIdError::InvalidNamespace(ref ns) => write!(f, "invalid namespace: {}", *ns),
        }
    }
}

impl CraftError for PackageIdError {
    fn is_human(&self) -> bool {
        true
    }
}

impl From<PackageIdError> for Box<CraftError> {
    fn from(t: PackageIdError) -> Box<CraftError> {
        Box::new(t)
    }
}

#[derive(PartialEq, Eq, Hash, Clone, RustcEncodable, Debug)]
pub struct Metadata {
    pub metadata: String,
    pub extra_filename: String,
}

impl PackageId {
    pub fn new<T: ToSemver>(name: &str, version: T, sid: &SourceId) -> CraftResult<PackageId> {
        let v = try!(version.to_semver().map_err(PackageIdError::InvalidVersion));
        Ok(PackageId {
            inner: Arc::new(PackageIdInner {
                name: name.to_string(),
                version: v,
                source_id: sid.clone(),
            }),
        })
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }
    pub fn version(&self) -> &semver::Version {
        &self.inner.version
    }
    pub fn source_id(&self) -> &SourceId {
        &self.inner.source_id
    }

    pub fn generate_metadata(&self) -> Metadata {
        let metadata = short_hash(self);
        let extra_filename = format!("-{}", metadata);

        Metadata {
            metadata: metadata,
            extra_filename: extra_filename,
        }
    }

    pub fn with_precise(&self, precise: Option<String>) -> PackageId {
        PackageId {
            inner: Arc::new(PackageIdInner {
                name: self.inner.name.to_string(),
                version: self.inner.version.clone(),
                source_id: self.inner.source_id.with_precise(precise),
            }),
        }
    }

    pub fn with_source_id(&self, source: &SourceId) -> PackageId {
        PackageId {
            inner: Arc::new(PackageIdInner {
                name: self.inner.name.to_string(),
                version: self.inner.version.clone(),
                source_id: source.clone(),
            }),
        }
    }
}

impl Metadata {
    pub fn mix<T: Hash>(&mut self, t: &T) {
        let new_metadata = short_hash(&(&self.metadata, t));
        self.extra_filename = format!("-{}", new_metadata);
        self.metadata = new_metadata;
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        try!(write!(f, "{} v{}", self.inner.name, self.inner.version));

        if !self.inner.source_id.is_default_registry() {
            try!(write!(f, " ({})", self.inner.source_id));
        }

        Ok(())
    }
}

impl fmt::Debug for PackageId {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("PackageId")
            .field("name", &self.inner.name)
            .field("version", &self.inner.version.to_string())
            .field("source", &self.inner.source_id.to_string())
            .finish()
    }
}
