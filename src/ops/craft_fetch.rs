use registry::PackageRegistry;
use core::{PackageId, PackageSet, Workspace};
use ops;
use util::CraftResult;
use resolver::Resolve;

/// Executes `craft fetch`.
pub fn fetch<'a>(ws: &Workspace<'a>) -> CraftResult<(Resolve, PackageSet<'a>)> {
    let mut registry = try!(PackageRegistry::new(ws.config()));
    let resolve = try!(ops::resolve_ws(&mut registry, ws));
    let packages = get_resolved_packages(&resolve, registry);
    for id in resolve.iter() {
        try!(packages.get(id));
    }
    Ok((resolve, packages))
}

pub fn get_resolved_packages<'a>(resolve: &Resolve, registry: PackageRegistry<'a>) -> PackageSet<'a> {
    let ids: Vec<PackageId> = resolve.iter().cloned().collect();
    registry.get(&ids)
}
