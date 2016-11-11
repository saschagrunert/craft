use ops;
use package_id_spec::PackageIdSpec;
use util::CraftResult;
use workspace::Workspace;

pub fn pkgid(ws: &Workspace, spec: Option<&str>) -> CraftResult<PackageIdSpec> {
    let resolve = match ops::load_pkg_lockfile(ws)? {
        Some(resolve) => resolve,
        None => bail!("a Craft.lock must exist for this command"),
    };

    let pkgid = match spec {
        Some(spec) => PackageIdSpec::query_str(spec, resolve.iter())?,
        None => ws.current()?.package_id(),
    };
    Ok(PackageIdSpec::from_package_id(pkgid))
}
