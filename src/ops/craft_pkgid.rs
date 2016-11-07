use ops;
use core::Workspace;
use package_id_spec::PackageIdSpec;
use util::CraftResult;

pub fn pkgid(ws: &Workspace, spec: Option<&str>) -> CraftResult<PackageIdSpec> {
    let resolve = match try!(ops::load_pkg_lockfile(ws)) {
        Some(resolve) => resolve,
        None => bail!("a Craft.lock must exist for this command"),
    };

    let pkgid = match spec {
        Some(spec) => try!(PackageIdSpec::query_str(spec, resolve.iter())),
        None => try!(ws.current()).package_id(),
    };
    Ok(PackageIdSpec::from_package_id(pkgid))
}
