//! Error reporting and conversion
use std::error::Error;
use std::ffi;
use std::fmt;
use std::io;
use std::num;
use std::process::{Output, ExitStatus};
use std::str;
use std::string;

use curl;
use git2;
use rustc_serialize::json;
use semver;
use term;
use toml;
use url;

pub type CraftResult<T> = Result<T, Box<CraftError>>;

// =============================================================================
// CraftError trait

pub trait CraftError: Error + Send + 'static {
    fn is_human(&self) -> bool {
        false
    }
    fn craft_cause(&self) -> Option<&CraftError> {
        None
    }
    fn as_error(&self) -> &Error
        where Self: Sized
    {
        self as &Error
    }
}

impl Error for Box<CraftError> {
    fn description(&self) -> &str {
        (**self).description()
    }
    fn cause(&self) -> Option<&Error> {
        (**self).cause()
    }
}

impl CraftError for Box<CraftError> {
    fn is_human(&self) -> bool {
        (**self).is_human()
    }
    fn craft_cause(&self) -> Option<&CraftError> {
        (**self).craft_cause()
    }
}

// =============================================================================
// Chaining errors

pub trait ChainError<T> {
    fn chain_error<E, F>(self, callback: F) -> CraftResult<T>
        where E: CraftError,
              F: FnOnce() -> E;
}

#[derive(Debug)]
struct ChainedError<E> {
    error: E,
    cause: Box<CraftError>,
}

impl<'a, T, F> ChainError<T> for F
    where F: FnOnce() -> CraftResult<T>
{
    fn chain_error<E, C>(self, callback: C) -> CraftResult<T>
        where E: CraftError,
              C: FnOnce() -> E
    {
        self().chain_error(callback)
    }
}

impl<T, E: CraftError + 'static> ChainError<T> for Result<T, E> {
    fn chain_error<E2: 'static, C>(self, callback: C) -> CraftResult<T>
        where E2: CraftError,
              C: FnOnce() -> E2
    {
        self.map_err(move |err| {
            Box::new(ChainedError {
                error: callback(),
                cause: Box::new(err),
            }) as Box<CraftError>
        })
    }
}

impl<T> ChainError<T> for Box<CraftError> {
    fn chain_error<E2, C>(self, callback: C) -> CraftResult<T>
        where E2: CraftError,
              C: FnOnce() -> E2
    {
        Err(Box::new(ChainedError {
            error: callback(),
            cause: self,
        }))
    }
}

impl<T> ChainError<T> for Option<T> {
    fn chain_error<E: 'static, C>(self, callback: C) -> CraftResult<T>
        where E: CraftError,
              C: FnOnce() -> E
    {
        match self {
            Some(t) => Ok(t),
            None => Err(Box::new(callback())),
        }
    }
}

impl<E: Error> Error for ChainedError<E> {
    fn description(&self) -> &str {
        self.error.description()
    }
}

impl<E: fmt::Display> fmt::Display for ChainedError<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl<E: CraftError> CraftError for ChainedError<E> {
    fn is_human(&self) -> bool {
        self.error.is_human()
    }
    fn craft_cause(&self) -> Option<&CraftError> {
        Some(&*self.cause)
    }
}

// =============================================================================
// Process errors

pub struct ProcessError {
    pub desc: String,
    pub exit: Option<ExitStatus>,
    pub output: Option<Output>,
    cause: Option<Box<CraftError>>,
}

impl Error for ProcessError {
    fn description(&self) -> &str {
        &self.desc
    }
    fn cause(&self) -> Option<&Error> {
        self.cause.as_ref().map(|e| e.as_error())
    }
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.desc, f)
    }
}
impl fmt::Debug for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

// =============================================================================
// Craft test errors.

/// Error when testcases fail
pub struct CraftTestError {
    pub desc: String,
    pub exit: Option<ExitStatus>,
    pub causes: Vec<ProcessError>,
}

impl CraftTestError {
    pub fn new(errors: Vec<ProcessError>) -> Self {
        if errors.is_empty() {
            panic!("Cannot create CraftTestError from empty Vec")
        }
        let desc = errors.iter()
            .map(|error| error.desc.clone())
            .collect::<Vec<String>>()
            .join("\n");
        CraftTestError {
            desc: desc,
            exit: errors[0].exit,
            causes: errors,
        }
    }
}

impl fmt::Display for CraftTestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.desc, f)
    }
}

impl fmt::Debug for CraftTestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for CraftTestError {
    fn description(&self) -> &str {
        &self.desc
    }
    fn cause(&self) -> Option<&Error> {
        self.causes.get(0).map(|s| s as &Error)
    }
}


// =============================================================================
// Concrete errors

struct ConcreteCraftError {
    description: String,
    detail: Option<String>,
    cause: Option<Box<Error + Send>>,
    is_human: bool,
}

impl fmt::Display for ConcreteCraftError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.description)?;
        if let Some(ref s) = self.detail {
            write!(f, " ({})", s)?;
        }
        Ok(())
    }
}
impl fmt::Debug for ConcreteCraftError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for ConcreteCraftError {
    fn description(&self) -> &str {
        &self.description
    }
    fn cause(&self) -> Option<&Error> {
        self.cause.as_ref().map(|c| {
            let e: &Error = &**c;
            e
        })
    }
}

impl CraftError for ConcreteCraftError {
    fn is_human(&self) -> bool {
        self.is_human
    }
}

// =============================================================================
// Human errors

#[derive(Debug)]
pub struct Human<E>(pub E);

impl<E: Error> Error for Human<E> {
    fn description(&self) -> &str {
        self.0.description()
    }
    fn cause(&self) -> Option<&Error> {
        self.0.cause()
    }
}

impl<E: fmt::Display> fmt::Display for Human<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl<E: CraftError> CraftError for Human<E> {
    fn is_human(&self) -> bool {
        true
    }
    fn craft_cause(&self) -> Option<&CraftError> {
        self.0.craft_cause()
    }
}

// =============================================================================
// CLI errors

pub type CliResult<T> = Result<T, CliError>;

#[derive(Debug)]
pub struct CliError {
    pub error: Option<Box<CraftError>>,
    pub unknown: bool,
    pub exit_code: i32,
}

impl Error for CliError {
    fn description(&self) -> &str {
        self.error
            .as_ref()
            .map(|e| e.description())
            .unwrap_or("unknown cli error")
    }

    fn cause(&self) -> Option<&Error> {
        self.error.as_ref().and_then(|e| e.cause())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref error) = self.error {
            error.fmt(f)
        } else {
            self.description().fmt(f)
        }
    }
}

impl CliError {
    pub fn new(error: Box<CraftError>, code: i32) -> CliError {
        let human = error.is_human();
        CliError {
            error: Some(error),
            exit_code: code,
            unknown: !human,
        }
    }

    pub fn code(code: i32) -> CliError {
        CliError {
            error: None,
            exit_code: code,
            unknown: false,
        }
    }
}

impl From<Box<CraftError>> for CliError {
    fn from(err: Box<CraftError>) -> CliError {
        CliError::new(err, 101)
    }
}

// =============================================================================
// NetworkError trait

pub trait NetworkError: CraftError {
    fn maybe_spurious(&self) -> bool;
}

impl NetworkError for git2::Error {
    fn maybe_spurious(&self) -> bool {
        match self.class() {
            git2::ErrorClass::Net |
            git2::ErrorClass::Os => true,
            _ => false,
        }
    }
}
impl NetworkError for curl::Error {
    fn maybe_spurious(&self) -> bool {
        self.is_couldnt_connect() || self.is_couldnt_resolve_proxy() || self.is_couldnt_resolve_host() ||
        self.is_operation_timedout() || self.is_recv_error()
    }
}

// =============================================================================
// various impls

macro_rules! from_error {
    ($($p:ty,)*) => (
        $(impl From<$p> for Box<CraftError> {
            fn from(t: $p) -> Box<CraftError> { Box::new(t) }
        })*
    )
}

from_error! {
    semver::ReqParseError,
    io::Error,
    ProcessError,
    git2::Error,
    json::DecoderError,
    json::EncoderError,
    curl::Error,
    CliError,
    toml::Error,
    url::ParseError,
    toml::DecodeError,
    ffi::NulError,
    term::Error,
    num::ParseIntError,
    str::ParseBoolError,
}

impl From<string::ParseError> for Box<CraftError> {
    fn from(t: string::ParseError) -> Box<CraftError> {
        match t {}
    }
}

impl<E: CraftError> From<Human<E>> for Box<CraftError> {
    fn from(t: Human<E>) -> Box<CraftError> {
        Box::new(t)
    }
}

impl CraftError for semver::ReqParseError {}
impl CraftError for io::Error {}
impl CraftError for git2::Error {}
impl CraftError for json::DecoderError {}
impl CraftError for json::EncoderError {}
impl CraftError for curl::Error {}
impl CraftError for ProcessError {}
impl CraftError for CraftTestError {}
impl CraftError for CliError {}
impl CraftError for toml::Error {}
impl CraftError for toml::DecodeError {}
impl CraftError for url::ParseError {}
impl CraftError for ffi::NulError {}
impl CraftError for term::Error {}
impl CraftError for num::ParseIntError {}
impl CraftError for str::ParseBoolError {}

// =============================================================================
// Construction helpers

pub fn process_error(msg: &str,
                     cause: Option<Box<CraftError>>,
                     status: Option<&ExitStatus>,
                     output: Option<&Output>)
                     -> ProcessError {
    let exit = match status {
        Some(s) => status_to_string(s),
        None => "never executed".to_string(),
    };
    let mut desc = format!("{} ({})", &msg, exit);

    if let Some(out) = output {
        match str::from_utf8(&out.stdout) {
            Ok(s) if s.trim().len() > 0 => {
                desc.push_str("\n--- stdout\n");
                desc.push_str(s);
            }
            Ok(..) | Err(..) => {}
        }
        match str::from_utf8(&out.stderr) {
            Ok(s) if s.trim().len() > 0 => {
                desc.push_str("\n--- stderr\n");
                desc.push_str(s);
            }
            Ok(..) | Err(..) => {}
        }
    }

    return ProcessError {
        desc: desc,
        exit: status.cloned(),
        output: output.cloned(),
        cause: cause,
    };

    #[cfg(unix)]
    fn status_to_string(status: &ExitStatus) -> String {
        use std::os::unix::process::*;
        use libc;

        if let Some(signal) = status.signal() {
            let name = match signal as libc::c_int {
                libc::SIGABRT => ", SIGABRT: process abort signal",
                libc::SIGALRM => ", SIGALRM: alarm clock",
                libc::SIGFPE => ", SIGFPE: erroneous arithmetic operation",
                libc::SIGHUP => ", SIGHUP: hangup",
                libc::SIGILL => ", SIGILL: illegal instruction",
                libc::SIGINT => ", SIGINT: terminal interrupt signal",
                libc::SIGKILL => ", SIGKILL: kill",
                libc::SIGPIPE => ", SIGPIPE: write on a pipe with no one to read",
                libc::SIGQUIT => ", SIGQUIT: terminal quite signal",
                libc::SIGSEGV => ", SIGSEGV: invalid memory reference",
                libc::SIGTERM => ", SIGTERM: termination signal",
                libc::SIGBUS => ", SIGBUS: access to undefined memory",
                libc::SIGSYS => ", SIGSYS: bad system call",
                libc::SIGTRAP => ", SIGTRAP: trace/breakpoint trap",
                _ => "",
            };
            format!("signal: {}{}", signal, name)
        } else {
            status.to_string()
        }
    }

    #[cfg(windows)]
    fn status_to_string(status: &ExitStatus) -> String {
        status.to_string()
    }
}

pub fn internal_error(error: &str, detail: &str) -> Box<CraftError> {
    Box::new(ConcreteCraftError {
        description: error.to_string(),
        detail: Some(detail.to_string()),
        cause: None,
        is_human: false,
    })
}

pub fn internal<S: fmt::Display>(error: S) -> Box<CraftError> {
    _internal(&error)
}

fn _internal(error: &fmt::Display) -> Box<CraftError> {
    Box::new(ConcreteCraftError {
        description: error.to_string(),
        detail: None,
        cause: None,
        is_human: false,
    })
}

pub fn human<S: fmt::Display>(error: S) -> Box<CraftError> {
    _human(&error)
}

fn _human(error: &fmt::Display) -> Box<CraftError> {
    Box::new(ConcreteCraftError {
        description: error.to_string(),
        detail: None,
        cause: None,
        is_human: true,
    })
}

pub fn caused_human<S, E>(error: S, cause: E) -> Box<CraftError>
    where S: fmt::Display,
          E: Error + Send + 'static
{
    Box::new(ConcreteCraftError {
        description: error.to_string(),
        detail: None,
        cause: Some(Box::new(cause)),
        is_human: true,
    })
}
