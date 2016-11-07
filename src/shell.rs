use std::{io, fmt};
use std::io::prelude::*;

use term::color::{Color, BLACK, BRIGHT_RED, BRIGHT_GREEN, BRIGHT_YELLOW};
use term::{self, Terminal, TerminfoTerminal, color, Attr};

use self::AdequateTerminal::{NoColor, Colored};
use self::Verbosity::{Verbose, Quiet};
use self::ColorConfig::{Auto, Always, Never};

use util::errors::CraftResult;

#[derive(Clone, Copy, PartialEq)]
pub enum Verbosity {
    Verbose,
    Normal,
    Quiet,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ColorConfig {
    Auto,
    Always,
    Never,
}

impl fmt::Display for ColorConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
                ColorConfig::Auto => "auto",
                ColorConfig::Always => "always",
                ColorConfig::Never => "never",
            }
            .fmt(f)
    }
}

#[derive(Clone, Copy)]
pub struct ShellConfig {
    pub color_config: ColorConfig,
    pub tty: bool,
}

enum AdequateTerminal {
    NoColor(Box<Write + Send>),
    Colored(Box<Terminal<Output = Box<Write + Send>> + Send>),
}

pub struct Shell {
    terminal: AdequateTerminal,
    config: ShellConfig,
}

pub struct MultiShell {
    out: Shell,
    err: Shell,
    verbosity: Verbosity,
}

impl MultiShell {
    pub fn new(out: Shell, err: Shell, verbosity: Verbosity) -> MultiShell {
        MultiShell {
            out: out,
            err: err,
            verbosity: verbosity,
        }
    }

    pub fn out(&mut self) -> &mut Shell {
        &mut self.out
    }

    pub fn err(&mut self) -> &mut Shell {
        &mut self.err
    }

    pub fn say<T: ToString>(&mut self, message: T, color: Color) -> CraftResult<()> {
        match self.verbosity {
            Quiet => Ok(()),
            _ => self.out().say(message, color),
        }
    }

    pub fn status<T, U>(&mut self, status: T, message: U) -> CraftResult<()>
        where T: fmt::Display,
              U: fmt::Display
    {
        match self.verbosity {
            Quiet => Ok(()),
            _ => self.err().say_status(format!("[{}]", status), message, BRIGHT_GREEN),
        }
    }

    pub fn verbose<F>(&mut self, mut callback: F) -> CraftResult<()>
        where F: FnMut(&mut MultiShell) -> CraftResult<()>
    {
        match self.verbosity {
            Verbose => callback(self),
            _ => Ok(()),
        }
    }

    pub fn concise<F>(&mut self, mut callback: F) -> CraftResult<()>
        where F: FnMut(&mut MultiShell) -> CraftResult<()>
    {
        match self.verbosity {
            Verbose => Ok(()),
            _ => callback(self),
        }
    }

    pub fn error<T: fmt::Display>(&mut self, message: T) -> CraftResult<()> {
        self.err().say_status("[ERR!]", message, BRIGHT_RED)
    }

    pub fn warn<T: fmt::Display>(&mut self, message: T) -> CraftResult<()> {
        match self.verbosity {
            Quiet => Ok(()),
            _ => self.err().say_status("[WARN]", message, BRIGHT_YELLOW),
        }
    }

    pub fn set_verbosity(&mut self, verbosity: Verbosity) {
        self.verbosity = verbosity;
    }

    pub fn set_color_config(&mut self, color: Option<&str>) -> CraftResult<()> {
        let cfg = match color {
            Some("auto") => Auto,
            Some("always") => Always,
            Some("never") => Never,

            None => Auto,

            Some(arg) => {
                bail!("argument for --color must be auto, always, or never, but found `{}`",
                      arg)
            }
        };
        self.out.set_color_config(cfg);
        self.err.set_color_config(cfg);
        Ok(())
    }

    pub fn get_verbose(&self) -> Verbosity {
        self.verbosity
    }

    pub fn color_config(&self) -> ColorConfig {
        assert!(self.out.config.color_config == self.err.config.color_config);
        self.out.config.color_config
    }
}

impl Shell {
    pub fn create<T: FnMut() -> Box<Write + Send>>(mut out_fn: T, config: ShellConfig) -> Shell {
        let term = match Shell::get_term(out_fn()) {
            Ok(t) => t,
            Err(_) => NoColor(out_fn()),
        };

        Shell {
            terminal: term,
            config: config,
        }
    }

    #[cfg(any(windows))]
    fn get_term(out: Box<Write + Send>) -> CraftResult<AdequateTerminal> {
        // Check if the creation of a console will succeed
        if ::term::WinConsole::new(vec![0u8; 0]).is_ok() {
            let t = try!(::term::WinConsole::new(out));
            if !t.supports_color() {
                Ok(NoColor(Box::new(t)))
            } else {
                Ok(Colored(Box::new(t)))
            }
        } else {
            // If we fail to get a windows console, we try to get a `TermInfo` one
            Ok(Shell::get_terminfo_term(out))
        }
    }

    #[cfg(any(unix))]
    fn get_term(out: Box<Write + Send>) -> CraftResult<AdequateTerminal> {
        Ok(Shell::get_terminfo_term(out))
    }

    fn get_terminfo_term(out: Box<Write + Send>) -> AdequateTerminal {
        // Use `TermInfo::from_env()` and `TerminfoTerminal::supports_color()`
        // to determine if creation of a TerminfoTerminal is possible regardless
        // of the tty status. --color options are parsed after Shell creation so
        // always try to create a terminal that supports color output. Fall back
        // to a no-color terminal regardless of whether or not a tty is present
        // and if color output is not possible.
        match ::term::terminfo::TermInfo::from_env() {
            Ok(ti) => {
                let term = TerminfoTerminal::new_with_terminfo(out, ti);
                if !term.supports_color() {
                    NoColor(term.into_inner())
                } else {
                    // Color output is possible.
                    Colored(Box::new(term))
                }
            }
            Err(_) => NoColor(out),
        }
    }

    pub fn set_color_config(&mut self, color_config: ColorConfig) {
        self.config.color_config = color_config;
    }

    pub fn say<T: ToString>(&mut self, message: T, color: Color) -> CraftResult<()> {
        try!(self.reset());
        if color != BLACK {
            try!(self.fg(color));
        }
        try!(writeln!(self, "{}", message.to_string()));
        try!(self.reset());
        try!(self.flush());
        Ok(())
    }

    pub fn say_status<T, U>(&mut self, status: T, message: U, color: Color) -> CraftResult<()>
        where T: fmt::Display,
              U: fmt::Display
    {
        try!(self.reset());
        if color != BLACK {
            try!(self.fg(color));
        }
        if self.supports_attr(Attr::Bold) {
            try!(self.attr(Attr::Bold));
        }
        try!(write!(self, "{}", status));
        try!(self.reset());
        try!(writeln!(self, " {}", message));
        try!(self.flush());
        Ok(())
    }

    fn fg(&mut self, color: color::Color) -> CraftResult<bool> {
        let colored = self.colored();

        match self.terminal {
            Colored(ref mut c) if colored => try!(c.fg(color)),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn attr(&mut self, attr: Attr) -> CraftResult<bool> {
        let colored = self.colored();

        match self.terminal {
            Colored(ref mut c) if colored => try!(c.attr(attr)),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn supports_attr(&self, attr: Attr) -> bool {
        let colored = self.colored();

        match self.terminal {
            Colored(ref c) if colored => c.supports_attr(attr),
            _ => false,
        }
    }

    fn reset(&mut self) -> term::Result<()> {
        let colored = self.colored();

        match self.terminal {
            Colored(ref mut c) if colored => try!(c.reset()),
            _ => (),
        }
        Ok(())
    }

    fn colored(&self) -> bool {
        self.config.tty && Auto == self.config.color_config || Always == self.config.color_config
    }
}

impl Write for Shell {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.terminal {
            Colored(ref mut c) => c.write(buf),
            NoColor(ref mut n) => n.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.terminal {
            Colored(ref mut c) => c.flush(),
            NoColor(ref mut n) => n.flush(),
        }
    }
}
