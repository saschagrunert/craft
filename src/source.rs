//! Manipulations on different kind of sources
use std::cmp::{self, Ordering};
use std::collections::hash_map::{HashMap, Values, IterMut};
use std::fmt::{self, Formatter};
use std::hash;
use std::path::Path;
use std::sync::Arc;

use rustc_serialize::{Decodable, Decoder, Encodable, Encoder};
use url::Url;

use package::Package;
use package_id::PackageId;
use registry::Registry;
use sources::{git, PathSource, GitSource, RegistrySource, DirectorySource};
use util::{human, Config, CraftResult, ToUrl};

/// A Source finds and downloads remote packages based on names and versions.
pub trait Source: Registry {
    /// The update method performs any network operations required to get the entire list of all
    /// names, versions and dependencies of packages managed by the Source.
    fn update(&mut self) -> CraftResult<()>;

    /// The download method fetches the full package for each name and version specified.
    fn download(&mut self, package: &PackageId) -> CraftResult<Package>;

    /// Generates a unique string which represents the fingerprint of the current state of the
    /// source.
    ///
    /// This fingerprint is used to determine the "fresheness" of the source later on. It must be
    /// guaranteed that the fingerprint of a source is constant if and only if the output product
    /// will remain constant.
    ///
    /// The `pkg` argument is the package which this fingerprint should only be interested in for
    /// when this source may contain multiple packages.
    fn fingerprint(&self, pkg: &Package) -> CraftResult<String>;

    /// If this source supports it, verifies the source of the package specified.
    ///
    /// Note that the source may also have performed other checksum-based verification during the
    /// `download` step, but this is intended to be run just before a chest is compiled so it may
    /// perform more expensive checks which may not be cacheable.
    fn verify(&self, _pkg: &PackageId) -> CraftResult<()> {
        Ok(())
    }
}

impl<'a, T: Source + ?Sized + 'a> Source for Box<T> {
    fn update(&mut self) -> CraftResult<()> {
        (**self).update()
    }

    fn download(&mut self, id: &PackageId) -> CraftResult<Package> {
        (**self).download(id)
    }

    fn fingerprint(&self, pkg: &Package) -> CraftResult<String> {
        (**self).fingerprint(pkg)
    }

    fn verify(&self, pkg: &PackageId) -> CraftResult<()> {
        (**self).verify(pkg)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Kind {
    /// Kind::Git(<git reference>) represents a git repository
    Git(GitReference),

    /// represents a local path
    Path,

    /// represents the central registry
    Registry,

    /// represents a local filesystem-based registry
    LocalRegistry,

    /// represents a directory-based registry
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GitReference {
    Tag(String),
    Branch(String),
    Rev(String),
}

/// Unique identifier for a source of packages.
#[derive(Clone, Eq, Debug)]
pub struct SourceId {
    inner: Arc<SourceIdInner>,
}

#[derive(Eq, Clone, Debug)]
struct SourceIdInner {
    url: Url,
    canonical_url: Url,
    kind: Kind,
    // e.g. the exact git revision of the specified branch for a Git Source
    precise: Option<String>,
}

impl SourceId {
    fn new(kind: Kind, url: Url) -> SourceId {
        SourceId {
            inner: Arc::new(SourceIdInner {
                kind: kind,
                canonical_url: git::canonicalize_url(&url),
                url: url,
                precise: None,
            }),
        }
    }

    /// Parses a source URL and returns the corresponding ID.
    ///
    /// ## Example
    ///
    /// ```
    /// use craft::source::SourceId;
    /// SourceId::from_url("git+https://github.com/alexcrichton/\
    ///                     libssh2-static-sys#80e71a3021618eb05\
    ///                     656c58fb7c5ef5f12bc747f");
    /// ```
    pub fn from_url(string: &str) -> CraftResult<SourceId> {
        let mut parts = string.splitn(2, '+');
        let kind = parts.next().unwrap();
        let url = try!(parts.next().ok_or(human(format!("invalid source `{}`", string))));

        match kind {
            "git" => {
                let mut url = try!(url.to_url());
                let mut reference = GitReference::Branch("master".to_string());
                for (k, v) in url.query_pairs() {
                    match &k[..] {
                        // map older 'ref' to branch
                        "branch" | "ref" => reference = GitReference::Branch(v.into_owned()),

                        "rev" => reference = GitReference::Rev(v.into_owned()),
                        "tag" => reference = GitReference::Tag(v.into_owned()),
                        _ => {}
                    }
                }
                let precise = url.fragment().map(|s| s.to_owned());
                url.set_fragment(None);
                url.set_query(None);
                Ok(SourceId::for_git(&url, reference).with_precise(precise))
            }
            "registry" => {
                let url = try!(url.to_url());
                Ok(SourceId::new(Kind::Registry, url).with_precise(Some("locked".to_string())))
            }
            "path" => {
                let url = try!(url.to_url());
                Ok(SourceId::new(Kind::Path, url))
            }
            kind => Err(human(format!("unsupported source protocol: {}", kind))),
        }
    }

    pub fn to_url(&self) -> String {
        match *self.inner {
            SourceIdInner { kind: Kind::Path, ref url, .. } => format!("path+{}", url),
            SourceIdInner { kind: Kind::Git(ref reference), ref url, ref precise, .. } => {
                let ref_str = reference.url_ref();

                let precise_str = if precise.is_some() {
                    format!("#{}", precise.as_ref().unwrap())
                } else {
                    "".to_string()
                };

                format!("git+{}{}{}", url, ref_str, precise_str)
            }
            SourceIdInner { kind: Kind::Registry, ref url, .. } => format!("registry+{}", url),
            SourceIdInner { kind: Kind::LocalRegistry, ref url, .. } => format!("local-registry+{}", url),
            SourceIdInner { kind: Kind::Directory, ref url, .. } => format!("directory+{}", url),
        }
    }

    // Pass absolute path
    pub fn for_path(path: &Path) -> CraftResult<SourceId> {
        let url = try!(path.to_url());
        Ok(SourceId::new(Kind::Path, url))
    }

    pub fn for_git(url: &Url, reference: GitReference) -> SourceId {
        SourceId::new(Kind::Git(reference), url.clone())
    }

    pub fn for_registry(url: &Url) -> SourceId {
        SourceId::new(Kind::Registry, url.clone())
    }

    pub fn for_local_registry(path: &Path) -> CraftResult<SourceId> {
        let url = try!(path.to_url());
        Ok(SourceId::new(Kind::LocalRegistry, url))
    }

    pub fn for_directory(path: &Path) -> CraftResult<SourceId> {
        let url = try!(path.to_url());
        Ok(SourceId::new(Kind::Directory, url))
    }

    pub fn url(&self) -> &Url {
        &self.inner.url
    }

    pub fn is_path(&self) -> bool {
        self.inner.kind == Kind::Path
    }

    pub fn is_registry(&self) -> bool {
        self.inner.kind == Kind::Registry || self.inner.kind == Kind::LocalRegistry
    }

    pub fn is_git(&self) -> bool {
        match self.inner.kind {
            Kind::Git(_) => true,
            _ => false,
        }
    }

    /// Creates an implementation of `Source` corresponding to this ID.
    pub fn load<'a>(&self, config: &'a Config) -> Box<Source + 'a> {
        trace!("loading SourceId; {}", self);
        match self.inner.kind {
            Kind::Git(..) => Box::new(GitSource::new(self, config)),
            Kind::Path => {
                let path = match self.inner.url.to_file_path() {
                    Ok(p) => p,
                    Err(()) => panic!("path sources cannot be remote"),
                };
                Box::new(PathSource::new(&path, self, config))
            }
            Kind::Registry => Box::new(RegistrySource::remote(self, config)),
            Kind::LocalRegistry => {
                let path = match self.inner.url.to_file_path() {
                    Ok(p) => p,
                    Err(()) => panic!("path sources cannot be remote"),
                };
                Box::new(RegistrySource::local(self, &path, config))
            }
            Kind::Directory => {
                let path = match self.inner.url.to_file_path() {
                    Ok(p) => p,
                    Err(()) => panic!("path sources cannot be remote"),
                };
                Box::new(DirectorySource::new(&path, self, config))
            }
        }
    }

    pub fn precise(&self) -> Option<&str> {
        self.inner.precise.as_ref().map(|s| &s[..])
    }

    pub fn git_reference(&self) -> Option<&GitReference> {
        match self.inner.kind {
            Kind::Git(ref s) => Some(s),
            _ => None,
        }
    }

    pub fn with_precise(&self, v: Option<String>) -> SourceId {
        SourceId { inner: Arc::new(SourceIdInner { precise: v, ..(*self.inner).clone() }) }
    }
}

impl PartialEq for SourceId {
    fn eq(&self, other: &SourceId) -> bool {
        (*self.inner).eq(&*other.inner)
    }
}

impl PartialOrd for SourceId {
    fn partial_cmp(&self, other: &SourceId) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SourceId {
    fn cmp(&self, other: &SourceId) -> Ordering {
        self.inner.cmp(&other.inner)
    }
}

impl Encodable for SourceId {
    fn encode<S: Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        if self.is_path() {
            s.emit_option_none()
        } else {
            self.to_url().encode(s)
        }
    }
}

impl Decodable for SourceId {
    fn decode<D: Decoder>(d: &mut D) -> Result<SourceId, D::Error> {
        let string: String = try!(Decodable::decode(d));
        SourceId::from_url(&string).map_err(|e| d.error(&e.to_string()))
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match *self.inner {
            SourceIdInner { kind: Kind::Path, ref url, .. } => fmt::Display::fmt(url, f),
            SourceIdInner { kind: Kind::Git(ref reference), ref url, ref precise, .. } => {
                try!(write!(f, "{}{}", url, reference.url_ref()));

                if let Some(ref s) = *precise {
                    let len = cmp::min(s.len(), 8);
                    try!(write!(f, "#{}", &s[..len]));
                }
                Ok(())
            }
            SourceIdInner { kind: Kind::Registry, ref url, .. } |
            SourceIdInner { kind: Kind::LocalRegistry, ref url, .. } => write!(f, "registry {}", url),
            SourceIdInner { kind: Kind::Directory, ref url, .. } => write!(f, "dir {}", url),
        }
    }
}

// This custom implementation handles situations such as when two git sources point at *almost* the
// same URL, but not quite, even when they actually point to the same repository.
impl PartialEq for SourceIdInner {
    fn eq(&self, other: &SourceIdInner) -> bool {
        if self.kind != other.kind {
            return false;
        }
        if self.url == other.url {
            return true;
        }

        match (&self.kind, &other.kind) {
            (&Kind::Git(ref ref1), &Kind::Git(ref ref2)) => ref1 == ref2 && self.canonical_url == other.canonical_url,
            _ => false,
        }
    }
}

impl PartialOrd for SourceIdInner {
    fn partial_cmp(&self, other: &SourceIdInner) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SourceIdInner {
    fn cmp(&self, other: &SourceIdInner) -> Ordering {
        match self.kind.cmp(&other.kind) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.url.cmp(&other.url) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match (&self.kind, &other.kind) {
            (&Kind::Git(ref ref1), &Kind::Git(ref ref2)) => {
                (ref1, &self.canonical_url).cmp(&(ref2, &other.canonical_url))
            }
            _ => self.kind.cmp(&other.kind),
        }
    }
}

// The hash of SourceId is used in the name of some Craft folders, so shouldn't vary. `as_str`
// gives the serialisation of a url (which has a spec) and so insulates against possible changes in
// how the url chest does hashing.
impl hash::Hash for SourceId {
    fn hash<S: hash::Hasher>(&self, into: &mut S) {
        self.inner.kind.hash(into);
        match *self.inner {
            SourceIdInner { kind: Kind::Git(..), ref canonical_url, .. } => canonical_url.as_str().hash(into),
            _ => self.inner.url.as_str().hash(into),
        }
    }
}

impl GitReference {
    pub fn to_ref_string(&self) -> Option<String> {
        match *self {
            GitReference::Branch(ref s) => {
                if *s == "master" {
                    None
                } else {
                    Some(format!("branch={}", s))
                }
            }
            GitReference::Tag(ref s) => Some(format!("tag={}", s)),
            GitReference::Rev(ref s) => Some(format!("rev={}", s)),
        }
    }

    fn url_ref(&self) -> String {
        match self.to_ref_string() {
            None => "".to_string(),
            Some(s) => format!("?{}", s),
        }
    }
}

pub struct SourceMap<'src> {
    map: HashMap<SourceId, Box<Source + 'src>>,
}

pub type Sources<'a, 'src> = Values<'a, SourceId, Box<Source + 'src>>;

pub struct SourcesMut<'a, 'src: 'a> {
    inner: IterMut<'a, SourceId, Box<Source + 'src>>,
}

impl<'src> SourceMap<'src> {
    pub fn new() -> SourceMap<'src> {
        SourceMap { map: HashMap::new() }
    }

    pub fn contains(&self, id: &SourceId) -> bool {
        self.map.contains_key(id)
    }

    pub fn get(&self, id: &SourceId) -> Option<&(Source + 'src)> {
        let source = self.map.get(id);

        source.map(|s| {
            let s: &(Source + 'src) = &**s;
            s
        })
    }

    pub fn get_mut(&mut self, id: &SourceId) -> Option<&mut (Source + 'src)> {
        self.map.get_mut(id).map(|s| {
            let s: &mut (Source + 'src) = &mut **s;
            s
        })
    }

    pub fn get_by_package_id(&self, pkg_id: &PackageId) -> Option<&(Source + 'src)> {
        self.get(pkg_id.source_id())
    }

    pub fn insert(&mut self, id: &SourceId, source: Box<Source + 'src>) {
        self.map.insert(id.clone(), source);
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn sources<'a>(&'a self) -> Sources<'a, 'src> {
        self.map.values()
    }

    pub fn sources_mut<'a>(&'a mut self) -> SourcesMut<'a, 'src> {
        SourcesMut { inner: self.map.iter_mut() }
    }
}

impl<'a, 'src> Iterator for SourcesMut<'a, 'src> {
    type Item = (&'a SourceId, &'a mut (Source + 'src));
    fn next(&mut self) -> Option<(&'a SourceId, &'a mut (Source + 'src))> {
        self.inner.next().map(|(a, b)| (a, &mut **b))
    }
}
