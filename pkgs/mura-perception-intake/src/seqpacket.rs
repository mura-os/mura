//! SOCK_SEQPACKET over AF_UNIX with SCM_RIGHTS (libc only): one datagram = one record (§7),
//! at most `MAX_FDS_PER_DATAGRAM` fds riding along. Receive is `MSG_CMSG_CLOEXEC`; a truncated
//! datagram (`MSG_TRUNC`) is reported as such — the peer's framing rules decide what to do.

use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};

pub const MAX_FDS_PER_DATAGRAM: usize = 16;
/// Largest control record: a REGISTER/REGISTER_MORE with 16 image entries; GENERATION is 1 KiB.
pub const MAX_DATAGRAM: usize = 4096;
// cmsghdr + 16 RawFds is < 256 bytes on every supported Linux ABI. Stack storage keeps the
// consumer's nonblocking per-pass recv path allocation-free when generation notifications carry
// no fds.
const CMSG_BUFFER_SIZE: usize = 256;
#[repr(C)]
union CmsgBuffer {
    _align: libc::cmsghdr,
    bytes: [u8; CMSG_BUFFER_SIZE],
}

pub struct Socket {
    fd: OwnedFd,
}

impl Socket {
    /// Listen on `path` (unlinked first). The consumer's side.
    pub fn listen(path: &str) -> io::Result<Listener> {
        let _ = std::fs::remove_file(path);
        let fd = socket()?;
        let addr = sockaddr(path)?;
        // SAFETY: valid fd and sockaddr.
        if unsafe { libc::bind(fd.as_raw_fd(), &addr.0 as *const _ as *const libc::sockaddr, addr.1) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::listen(fd.as_raw_fd(), 4) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Listener { fd })
    }

    /// Connect to `path`. The producer's side.
    pub fn connect(path: &str) -> io::Result<Socket> {
        let fd = socket()?;
        let addr = sockaddr(path)?;
        // SAFETY: valid fd and sockaddr.
        if unsafe { libc::connect(fd.as_raw_fd(), &addr.0 as *const _ as *const libc::sockaddr, addr.1) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Socket { fd })
    }

    pub fn raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Send one datagram with up to 16 fds.
    pub fn send(&self, bytes: &[u8], fds: &[RawFd]) -> io::Result<()> {
        assert!(fds.len() <= MAX_FDS_PER_DATAGRAM);
        let mut iov = libc::iovec { iov_base: bytes.as_ptr() as *mut libc::c_void, iov_len: bytes.len() };
        let space = unsafe { libc::CMSG_SPACE((fds.len() * std::mem::size_of::<RawFd>()) as u32) } as usize;
        assert!(space <= CMSG_BUFFER_SIZE);
        let mut cbuf = CmsgBuffer { bytes: [0u8; CMSG_BUFFER_SIZE] };
        // SAFETY: msghdr assembled per cmsg(3); buffers outlive the call.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        if !fds.is_empty() {
            // SAFETY: the union gives the byte storage cmsghdr alignment.
            msg.msg_control = unsafe { cbuf.bytes.as_mut_ptr() } as *mut libc::c_void;
            msg.msg_controllen = space as _;
            unsafe {
                let c = libc::CMSG_FIRSTHDR(&msg);
                (*c).cmsg_level = libc::SOL_SOCKET;
                (*c).cmsg_type = libc::SCM_RIGHTS;
                (*c).cmsg_len = libc::CMSG_LEN((fds.len() * std::mem::size_of::<RawFd>()) as u32) as _;
                std::ptr::copy_nonoverlapping(fds.as_ptr(), libc::CMSG_DATA(c) as *mut RawFd, fds.len());
            }
        }
        let n = unsafe { libc::sendmsg(self.fd.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Receive one datagram. `Ok(None)` = EOF (peer gone). `nonblocking` uses MSG_DONTWAIT and
    /// returns `Err(WouldBlock)` when nothing is queued — the consumer's per-pass drain.
    pub fn recv(&self, buf: &mut [u8], nonblocking: bool) -> io::Result<Option<Datagram>> {
        let mut iov = libc::iovec { iov_base: buf.as_mut_ptr() as *mut libc::c_void, iov_len: buf.len() };
        let space = unsafe { libc::CMSG_SPACE((MAX_FDS_PER_DATAGRAM * std::mem::size_of::<RawFd>()) as u32) } as usize;
        assert!(space <= CMSG_BUFFER_SIZE);
        let mut cbuf = CmsgBuffer { bytes: [0u8; CMSG_BUFFER_SIZE] };
        // SAFETY: as in `send`.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        // SAFETY: the union gives the byte storage cmsghdr alignment.
        msg.msg_control = unsafe { cbuf.bytes.as_mut_ptr() } as *mut libc::c_void;
        msg.msg_controllen = space as _;
        let flags = libc::MSG_CMSG_CLOEXEC | if nonblocking { libc::MSG_DONTWAIT } else { 0 };
        let n = unsafe { libc::recvmsg(self.fd.as_raw_fd(), &mut msg, flags) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut fds = Vec::new();
        unsafe {
            let mut c = libc::CMSG_FIRSTHDR(&msg);
            while !c.is_null() {
                if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                    let bytes = (*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                    let count = bytes / std::mem::size_of::<RawFd>();
                    let data = libc::CMSG_DATA(c) as *const RawFd;
                    for i in 0..count {
                        fds.push(OwnedFd::from_raw_fd(*data.add(i)));
                    }
                }
                c = libc::CMSG_NXTHDR(&msg, c);
            }
        }
        if n == 0 && fds.is_empty() {
            return Ok(None); // EOF
        }
        Ok(Some(Datagram { len: n as usize, truncated: msg.msg_flags & libc::MSG_TRUNC != 0, fds }))
    }
}

pub struct Datagram {
    pub len: usize,
    pub truncated: bool,
    pub fds: Vec<OwnedFd>,
}

pub struct Listener {
    fd: OwnedFd,
}

impl Listener {
    pub fn accept(&self) -> io::Result<Socket> {
        // SAFETY: valid listening fd.
        let fd = unsafe { libc::accept4(self.fd.as_raw_fd(), std::ptr::null_mut(), std::ptr::null_mut(), libc::SOCK_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fresh fd we own.
        Ok(Socket { fd: unsafe { OwnedFd::from_raw_fd(fd) } })
    }
}

fn socket() -> io::Result<OwnedFd> {
    // SAFETY: plain socket(2).
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fresh fd we own.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn sockaddr(path: &str) -> io::Result<(libc::sockaddr_un, libc::socklen_t)> {
    // SAFETY: zeroed sockaddr_un is a valid value.
    let mut a: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    a.sun_family = libc::AF_UNIX as _;
    let b = path.as_bytes();
    if b.len() >= a.sun_path.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "socket path too long"));
    }
    for (i, c) in b.iter().enumerate() {
        a.sun_path[i] = *c as _;
    }
    let len = std::mem::size_of::<libc::sa_family_t>() + b.len() + 1;
    Ok((a, len as libc::socklen_t))
}
