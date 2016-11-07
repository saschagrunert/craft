pub use self::imp::read2;

#[cfg(unix)]
mod imp {
    use std::cmp;
    use std::io::prelude::*;
    use std::io;
    use std::mem;
    use std::os::unix::prelude::*;
    use std::process::{ChildStdout, ChildStderr};
    use libc;

    pub fn read2(mut out_pipe: ChildStdout,
                 mut err_pipe: ChildStderr,
                 mut data: &mut FnMut(bool, &mut Vec<u8>, bool))
                 -> io::Result<()> {
        unsafe {
            libc::fcntl(out_pipe.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
            libc::fcntl(err_pipe.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
        }

        let mut out_done = false;
        let mut err_done = false;
        let mut out = Vec::new();
        let mut err = Vec::new();

        let max = cmp::max(out_pipe.as_raw_fd(), err_pipe.as_raw_fd());
        loop {
            // wait for either pipe to become readable using `select`
            let r = unsafe {
                let mut read: libc::fd_set = mem::zeroed();
                if !out_done {
                    libc::FD_SET(out_pipe.as_raw_fd(), &mut read);
                }
                if !err_done {
                    libc::FD_SET(err_pipe.as_raw_fd(), &mut read);
                }
                libc::select(max + 1, &mut read, 0 as *mut _, 0 as *mut _, 0 as *mut _)
            };
            if r == -1 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }

            // Read as much as we can from each pipe, ignoring EWOULDBLOCK or
            // EAGAIN. If we hit EOF, then this will happen because the underlying
            // reader will return Ok(0), in which case we'll see `Ok` ourselves. In
            // this case we flip the other fd back into blocking mode and read
            // whatever's leftover on that file descriptor.
            let handle = |res: io::Result<_>| {
                match res {
                    Ok(_) => Ok(true),
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            Ok(false)
                        } else {
                            Err(e)
                        }
                    }
                }
            };
            if !out_done && try!(handle(out_pipe.read_to_end(&mut out))) {
                out_done = true;
            }
            data(true, &mut out, out_done);
            if !err_done && try!(handle(err_pipe.read_to_end(&mut err))) {
                err_done = true;
            }
            data(false, &mut err, err_done);

            if out_done && err_done {
                return Ok(());
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    extern crate miow;
    extern crate winapi;

    use std::io;
    use std::os::windows::prelude::*;
    use std::process::{ChildStdout, ChildStderr};
    use std::slice;

    use self::miow::iocp::{CompletionPort, CompletionStatus};
    use self::miow::pipe::NamedPipe;
    use self::miow::Overlapped;
    use self::winapi::ERROR_BROKEN_PIPE;

    struct Pipe<'a> {
        dst: &'a mut Vec<u8>,
        overlapped: Overlapped,
        pipe: NamedPipe,
        done: bool,
    }

    macro_rules! try {
        ($e:expr) => (match $e {
            Ok(e) => e,
            Err(e) => {
                println!("{} failed with {}", stringify!($e), e);
                return Err(e)
            }
        })
    }

    pub fn read2(out_pipe: ChildStdout,
                 err_pipe: ChildStderr,
                 mut data: &mut FnMut(bool, &mut Vec<u8>, bool))
                 -> io::Result<()> {
        let mut out = Vec::new();
        let mut err = Vec::new();

        let port = try!(CompletionPort::new(1));
        try!(port.add_handle(0, &out_pipe));
        try!(port.add_handle(1, &err_pipe));

        unsafe {
            let mut out_pipe = Pipe::new(out_pipe, &mut out);
            let mut err_pipe = Pipe::new(err_pipe, &mut err);

            try!(out_pipe.read());
            try!(err_pipe.read());

            let mut status = [CompletionStatus::zero(), CompletionStatus::zero()];

            while !out_pipe.done || !err_pipe.done {
                for status in try!(port.get_many(&mut status, None)) {
                    if status.token() == 0 {
                        out_pipe.complete(status);
                        data(true, out_pipe.dst, out_pipe.done);
                        try!(out_pipe.read());
                    } else {
                        err_pipe.complete(status);
                        data(false, err_pipe.dst, err_pipe.done);
                        try!(err_pipe.read());
                    }
                }
            }

            Ok(())
        }
    }

    impl<'a> Pipe<'a> {
        unsafe fn new<P: IntoRawHandle>(p: P, dst: &'a mut Vec<u8>) -> Pipe<'a> {
            Pipe {
                dst: dst,
                pipe: NamedPipe::from_raw_handle(p.into_raw_handle()),
                overlapped: Overlapped::zero(),
                done: false,
            }
        }

        unsafe fn read(&mut self) -> io::Result<()> {
            let dst = slice_to_end(self.dst);
            match self.pipe.read_overlapped(dst, &mut self.overlapped) {
                Ok(_) => Ok(()),
                Err(e) => {
                    if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                        self.done = true;
                        Ok(())
                    } else {
                        Err(e)
                    }
                }
            }
        }

        unsafe fn complete(&mut self, status: &CompletionStatus) {
            let prev = self.dst.len();
            self.dst.set_len(prev + status.bytes_transferred() as usize);
            if status.bytes_transferred() == 0 {
                self.done = true;
            }
        }
    }

    unsafe fn slice_to_end(v: &mut Vec<u8>) -> &mut [u8] {
        if v.capacity() == 0 {
            v.reserve(16);
        }
        if v.capacity() == v.len() {
            v.reserve(1);
        }
        slice::from_raw_parts_mut(v.as_mut_ptr().offset(v.len() as isize),
                                  v.capacity() - v.len())
    }
}
