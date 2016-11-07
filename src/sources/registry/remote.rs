use std::io::SeekFrom;
use std::io::prelude::*;
use std::path::Path;
use std::time::Duration;
use std::env;

use curl::easy::{Easy, List};
use git2;
use rustc_serialize::hex::ToHex;
use rustc_serialize::json;
use url::Url;

use core::{PackageId, SourceId};
use sources::git;
use sources::registry::{RegistryData, RegistryConfig, INDEX_LOCK};
use util::network;
use util::paths;
use util::{FileLock, Filesystem};
use util::{Config, CraftResult, ChainError, human, Sha256, ToUrl};

pub struct RemoteRegistry<'cfg> {
    index_path: Filesystem,
    cache_path: Filesystem,
    source_id: SourceId,
    config: &'cfg Config,
    handle: Option<Easy>,
}

impl<'cfg> RemoteRegistry<'cfg> {
    pub fn new(source_id: &SourceId, config: &'cfg Config, name: &str) -> RemoteRegistry<'cfg> {
        RemoteRegistry {
            index_path: config.registry_index_path().join(name),
            cache_path: config.registry_cache_path().join(name),
            source_id: source_id.clone(),
            config: config,
            handle: None,
        }
    }
}

impl<'cfg> RegistryData for RemoteRegistry<'cfg> {
    fn index_path(&self) -> &Filesystem {
        &self.index_path
    }

    fn config(&self) -> CraftResult<Option<RegistryConfig>> {
        let lock = try!(self.index_path.open_ro(Path::new(INDEX_LOCK), self.config, "the registry index"));
        let path = lock.path().parent().unwrap();
        let contents = try!(paths::read(&path.join("config.json")));
        let config = try!(json::decode(&contents));
        Ok(Some(config))
    }

    fn update_index(&mut self) -> CraftResult<()> {
        // Ensure that we'll actually be able to acquire an HTTP handle later on
        // once we start trying to download crates. This will weed out any
        // problems with `.craft/config` configuration related to HTTP.
        //
        // This way if there's a problem the error gets printed before we even
        // hit the index, which may not actually read this configuration.
        try!(http_handle(self.config));

        // Then we actually update the index
        try!(self.index_path.create_dir());
        let lock = try!(self.index_path.open_rw(Path::new(INDEX_LOCK), self.config, "the registry index"));
        let path = lock.path().parent().unwrap();

        try!(self.config.shell().status("Updating", format!("registry `{}`", self.source_id.url())));

        let repo = try!(git2::Repository::open(path).or_else(|_| {
            let _ = lock.remove_siblings();
            git2::Repository::init(path)
        }));

        if self.source_id.url().host_str() == Some("github.com") {
            if let Ok(oid) = repo.refname_to_id("refs/heads/master") {
                let handle = match self.handle {
                    Some(ref mut handle) => handle,
                    None => {
                        self.handle = Some(try!(http_handle(self.config)));
                        self.handle.as_mut().unwrap()
                    }
                };
                debug!("attempting github fast path for {}", self.source_id.url());
                if github_up_to_date(handle, &self.source_id.url(), &oid) {
                    return Ok(());
                }
                debug!("fast path failed, falling back to a git fetch");
            }
        }

        // git fetch origin
        let url = self.source_id.url().to_string();
        let refspec = "refs/heads/*:refs/remotes/origin/*";

        try!(git::fetch(&repo, &url, refspec, &self.config)
            .chain_error(|| human(format!("failed to fetch `{}`", url))));

        // git reset --hard origin/master
        let reference = "refs/remotes/origin/master";
        let oid = try!(repo.refname_to_id(reference));
        trace!("[{}] updating to rev {}", self.source_id, oid);
        let object = try!(repo.find_object(oid, None));
        try!(repo.reset(&object, git2::ResetType::Hard, None));
        Ok(())
    }

    fn download(&mut self, pkg: &PackageId, checksum: &str) -> CraftResult<FileLock> {
        let filename = format!("{}-{}.crate", pkg.name(), pkg.version());
        let path = Path::new(&filename);
        let mut dst = try!(self.cache_path.open_rw(path, self.config, &filename));
        let meta = try!(dst.file().metadata());
        if meta.len() > 0 {
            return Ok(dst);
        }
        try!(self.config.shell().status("Downloading", pkg));

        let config = try!(self.config()).unwrap();
        let mut url = try!(config.dl.to_url());
        url.path_segments_mut()
            .unwrap()
            .push(pkg.name())
            .push(&pkg.version().to_string())
            .push("download");

        let handle = match self.handle {
            Some(ref mut handle) => handle,
            None => {
                self.handle = Some(try!(http_handle(self.config)));
                self.handle.as_mut().unwrap()
            }
        };
        // TODO: don't download into memory, but ensure that if we ctrl-c a
        //       download we should resume either from the start or the middle
        //       on the next time
        try!(handle.get(true));
        try!(handle.url(&url.to_string()));
        try!(handle.follow_location(true));
        let mut state = Sha256::new();
        let mut body = Vec::new();
        {
            let mut handle = handle.transfer();
            try!(handle.write_function(|buf| {
                state.update(buf);
                body.extend_from_slice(buf);
                Ok(buf.len())
            }));
            try!(network::with_retry(self.config, || handle.perform()))
        }
        let code = try!(handle.response_code());
        if code != 200 && code != 0 {
            bail!("failed to get 200 response from `{}`, got {}", url, code)
        }

        // Verify what we just downloaded
        if state.finish().to_hex() != checksum {
            bail!("failed to verify the checksum of `{}`", pkg)
        }

        try!(dst.write_all(&body));
        try!(dst.seek(SeekFrom::Start(0)));
        Ok(dst)
    }
}

/// Updating the index is done pretty regularly so we want it to be as fast as
/// possible. For registries hosted on github (like the crates.io index) there's
/// a fast path available to use [1] to tell us that there's no updates to be
/// made.
///
/// This function will attempt to hit that fast path and verify that the `oid`
/// is actually the current `master` branch of the repository. If `true` is
/// returned then no update needs to be performed, but if `false` is returned
/// then the standard update logic still needs to happen.
///
/// [1]: https://developer.github.com/v3/repos/commits/#get-the-sha-1-of-a-commit-reference
///
/// Note that this function should never cause an actual failure because it's
/// just a fast path. As a result all errors are ignored in this function and we
/// just return a `bool`. Any real errors will be reported through the normal
/// update path above.
fn github_up_to_date(handle: &mut Easy, url: &Url, oid: &git2::Oid) -> bool {
    macro_rules! try {
        ($e:expr) => (match $e {
            Some(e) => e,
            None => return false,
        })
    }

    // This expects github urls in the form `github.com/user/repo` and nothing
    // else
    let mut pieces = try!(url.path_segments());
    let username = try!(pieces.next());
    let repo = try!(pieces.next());
    if pieces.next().is_some() {
        return false;
    }

    let url = format!("https://api.github.com/repos/{}/{}/commits/master",
                      username,
                      repo);
    try!(handle.get(true).ok());
    try!(handle.url(&url).ok());
    try!(handle.useragent("craft").ok());
    let mut headers = List::new();
    try!(headers.append("Accept: application/vnd.github.3.sha").ok());
    try!(headers.append(&format!("If-None-Match: \"{}\"", oid)).ok());
    try!(handle.http_headers(headers).ok());
    try!(handle.perform().ok());

    try!(handle.response_code().ok()) == 304
}

/// Create a new HTTP handle with appropriate global configuration for craft.
pub fn http_handle(config: &Config) -> CraftResult<Easy> {
    if !config.network_allowed() {
        bail!("attempting to make an HTTP request, but --frozen was \
               specified")
    }

    // The timeout option for libcurl by default times out the entire transfer,
    // but we probably don't want this. Instead we only set timeouts for the
    // connect phase as well as a "low speed" timeout so if we don't receive
    // many bytes in a large-ish period of time then we time out.
    let mut handle = Easy::new();
    try!(handle.connect_timeout(Duration::new(30, 0)));
    try!(handle.low_speed_limit(10 /* bytes per second */));
    try!(handle.low_speed_time(Duration::new(30, 0)));
    if let Some(proxy) = try!(http_proxy(config)) {
        try!(handle.proxy(&proxy));
    }
    if let Some(cainfo) = try!(config.get_path("http.cainfo")) {
        try!(handle.cainfo(&cainfo.val));
    }
    if let Some(timeout) = try!(http_timeout(config)) {
        try!(handle.connect_timeout(Duration::new(timeout as u64, 0)));
        try!(handle.low_speed_time(Duration::new(timeout as u64, 0)));
    }
    Ok(handle)
}

/// Find an explicit HTTP proxy if one is available.
///
/// Favor craft's `http.proxy`, then git's `http.proxy`. Proxies specified
/// via environment variables are picked up by libcurl.
fn http_proxy(config: &Config) -> CraftResult<Option<String>> {
    match try!(config.get_string("http.proxy")) {
        Some(s) => return Ok(Some(s.val)),
        None => {}
    }
    match git2::Config::open_default() {
        Ok(cfg) => {
            match cfg.get_str("http.proxy") {
                Ok(s) => return Ok(Some(s.to_string())),
                Err(..) => {}
            }
        }
        Err(..) => {}
    }
    Ok(None)
}

fn http_timeout(config: &Config) -> CraftResult<Option<i64>> {
    match try!(config.get_i64("http.timeout")) {
        Some(s) => return Ok(Some(s.val)),
        None => {}
    }
    Ok(env::var("HTTP_TIMEOUT").ok().and_then(|s| s.parse().ok()))
}
