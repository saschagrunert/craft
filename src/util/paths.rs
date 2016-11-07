use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::path::{Path, PathBuf, Component};

use util::{human, internal, CraftResult, ChainError};

pub fn join_paths<T: AsRef<OsStr>>(paths: &[T], env: &str) -> CraftResult<OsString> {
    env::join_paths(paths.iter()).or_else(|e| {
        let paths = paths.iter().map(Path::new).collect::<Vec<_>>();
        internal(format!("failed to join path array: {:?}", paths)).chain_error(|| {
            human(format!("failed to join search paths together: {}\n\
                           Does ${} have an unterminated quote character?",
                          e,
                          env))
        })
    })
}

pub fn dylib_path_envvar() -> &'static str {
    if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    }
}

pub fn dylib_path() -> Vec<PathBuf> {
    match env::var_os(dylib_path_envvar()) {
        Some(var) => env::split_paths(&var).collect(),
        None => Vec::new(),
    }
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek()
        .cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

pub fn without_prefix<'a>(a: &'a Path, b: &'a Path) -> Option<&'a Path> {
    let mut a = a.components();
    let mut b = b.components();
    loop {
        match b.next() {
            Some(y) => {
                match a.next() {
                    Some(x) if x == y => continue,
                    _ => return None,
                }
            }
            None => return Some(a.as_path()),
        }
    }
}

pub fn read(path: &Path) -> CraftResult<String> {
    (|| -> CraftResult<_> {
            let mut ret = String::new();
            let mut f = try!(File::open(path));
            try!(f.read_to_string(&mut ret));
            Ok(ret)
        })()
        .map_err(human)
        .chain_error(|| human(format!("failed to read `{}`", path.display())))
}

pub fn read_bytes(path: &Path) -> CraftResult<Vec<u8>> {
    (|| -> CraftResult<_> {
            let mut ret = Vec::new();
            let mut f = try!(File::open(path));
            try!(f.read_to_end(&mut ret));
            Ok(ret)
        })()
        .map_err(human)
        .chain_error(|| human(format!("failed to read `{}`", path.display())))
}

pub fn write(path: &Path, contents: &[u8]) -> CraftResult<()> {
    (|| -> CraftResult<()> {
            let mut f = try!(File::create(path));
            try!(f.write_all(contents));
            Ok(())
        })()
        .map_err(human)
        .chain_error(|| human(format!("failed to write `{}`", path.display())))
}

pub fn append(path: &Path, contents: &[u8]) -> CraftResult<()> {
    (|| -> CraftResult<()> {
            let mut f = try!(OpenOptions::new()
                .write(true)
                .append(true)
                .create(true)
                .open(path));

            try!(f.write_all(contents));
            Ok(())
        })
        .chain_error(|| internal(format!("failed to write `{}`", path.display())))
}

#[cfg(unix)]
pub fn path2bytes(path: &Path) -> CraftResult<&[u8]> {
    use std::os::unix::prelude::*;
    Ok(path.as_os_str().as_bytes())
}
#[cfg(windows)]
pub fn path2bytes(path: &Path) -> CraftResult<&[u8]> {
    match path.as_os_str().to_str() {
        Some(s) => Ok(s.as_bytes()),
        None => Err(human(format!("invalid non-unicode path: {}", path.display()))),
    }
}

#[cfg(unix)]
pub fn bytes2path(bytes: &[u8]) -> CraftResult<PathBuf> {
    use std::os::unix::prelude::*;
    use std::ffi::OsStr;
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}
#[cfg(windows)]
pub fn bytes2path(bytes: &[u8]) -> CraftResult<PathBuf> {
    use std::str;
    match str::from_utf8(bytes) {
        Ok(s) => Ok(PathBuf::from(s)),
        Err(..) => Err(human("invalid non-unicode path")),
    }
}
