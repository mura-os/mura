//! The kernel ABI the intake rests on, as raw ioctls (libc only):
//!
//! - DRM syncobj timelines (`include/uapi/drm/drm.h`): create/destroy, export/import as fds
//!   (transferred once at registration, §3), `TIMELINE_SIGNAL` from userspace (the harness's
//!   fake GPU; wlroots signals release points the same way — references/wlroots/render/
//!   drm_syncobj.c:180-186), and `QUERY` for "has this point signalled?" without ever waiting
//!   (§5 rule 1; wlroots polls with a zero-timeout wait, :170-178 — the query is the same
//!   non-blocking question asked of the core, and it covers a point nothing has submitted to,
//!   which a wait would refuse with EINVAL).
//! - udmabuf (`include/uapi/linux/udmabuf.h`): a dmabuf over a sealed memfd — wlroots' software
//!   allocator (references/wlroots/render/allocator/udmabuf.c) — so the harness has real dmabufs
//!   with no GPU, CPU-visible on both sides (the stamp checks of §8 rest on that).
//! - memfd + mmap for the register (§4).
//!
//! The only *waiting* entry point is `timeline_wait`, kept for producers/tests; the library
//! counts every call with a non-zero timeout (`NONZERO_TIMEOUT_WAITS`) so a consumer can assert
//! it never issued one (§8 item 4).

use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicU64, Ordering};

pub static NONZERO_TIMEOUT_WAITS: AtomicU64 = AtomicU64::new(0);

// ---- ioctl numbers (linux/ioctl.h: dir<<30 | size<<16 | type<<8 | nr) ------------------------

const fn iowr(ty: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((3u32 << 30) | ((size as u32) << 16) | (ty << 8) | nr) as libc::c_ulong
}
const fn iow(ty: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((1u32 << 30) | ((size as u32) << 16) | (ty << 8) | nr) as libc::c_ulong
}

const DRM: u32 = b'd' as u32;

#[repr(C)]
struct DrmGetCap {
    capability: u64,
    value: u64,
}
#[repr(C)]
struct DrmSyncobjCreate {
    handle: u32,
    flags: u32,
}
#[repr(C)]
struct DrmSyncobjDestroy {
    handle: u32,
    pad: u32,
}
#[repr(C)]
struct DrmSyncobjHandle {
    handle: u32,
    flags: u32,
    fd: i32,
    pad: u32,
}
#[repr(C)]
struct DrmSyncobjTimelineWait {
    handles: u64,
    points: u64,
    timeout_nsec: i64,
    count_handles: u32,
    flags: u32,
    first_signaled: u32,
    pad: u32,
}
#[repr(C)]
struct DrmSyncobjTimelineArray {
    handles: u64,
    points: u64,
    count_handles: u32,
    flags: u32,
}

const DRM_IOCTL_GET_CAP: libc::c_ulong = iowr(DRM, 0x0c, std::mem::size_of::<DrmGetCap>());
const DRM_IOCTL_SYNCOBJ_CREATE: libc::c_ulong = iowr(DRM, 0xBF, std::mem::size_of::<DrmSyncobjCreate>());
const DRM_IOCTL_SYNCOBJ_DESTROY: libc::c_ulong = iowr(DRM, 0xC0, std::mem::size_of::<DrmSyncobjDestroy>());
const DRM_IOCTL_SYNCOBJ_HANDLE_TO_FD: libc::c_ulong = iowr(DRM, 0xC1, std::mem::size_of::<DrmSyncobjHandle>());
const DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE: libc::c_ulong = iowr(DRM, 0xC2, std::mem::size_of::<DrmSyncobjHandle>());
const DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT: libc::c_ulong = iowr(DRM, 0xCA, std::mem::size_of::<DrmSyncobjTimelineWait>());
const DRM_IOCTL_SYNCOBJ_QUERY: libc::c_ulong = iowr(DRM, 0xCB, std::mem::size_of::<DrmSyncobjTimelineArray>());
const DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL: libc::c_ulong = iowr(DRM, 0xCD, std::mem::size_of::<DrmSyncobjTimelineArray>());

pub const DRM_CAP_SYNCOBJ: u64 = 0x13;
pub const DRM_CAP_SYNCOBJ_TIMELINE: u64 = 0x14;
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL: u32 = 1;
pub const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT: u32 = 2;

fn ioctl<T>(fd: RawFd, req: libc::c_ulong, arg: &mut T) -> io::Result<()> {
    // SAFETY: `arg` is the exact struct the request expects, by construction above.
    let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// ---- DRM device --------------------------------------------------------------------------------

/// A DRM node with syncobj timelines. Opens the first `/dev/dri/renderD*` that reports
/// `DRM_CAP_SYNCOBJ_TIMELINE` (the VM's virtio-gpu render node does; vgem does not).
pub struct Drm {
    fd: OwnedFd,
    pub path: String,
}

impl Drm {
    pub fn open_any() -> io::Result<Drm> {
        let mut names: Vec<_> = std::fs::read_dir("/dev/dri")?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("renderD")).unwrap_or(false))
            .collect();
        names.sort();
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no /dev/dri/renderD* with DRM_CAP_SYNCOBJ_TIMELINE");
        for p in names {
            match Drm::open(&p.to_string_lossy()) {
                Ok(d) => return Ok(d),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub fn open(path: &str) -> io::Result<Drm> {
        let f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
        let drm = Drm { fd: OwnedFd::from(f), path: path.to_string() };
        if drm.cap(DRM_CAP_SYNCOBJ_TIMELINE)? != 1 {
            return Err(io::Error::new(io::ErrorKind::Unsupported, format!("{path}: no DRM_CAP_SYNCOBJ_TIMELINE")));
        }
        Ok(drm)
    }

    pub fn cap(&self, capability: u64) -> io::Result<u64> {
        let mut a = DrmGetCap { capability, value: 0 };
        ioctl(self.fd.as_raw_fd(), DRM_IOCTL_GET_CAP, &mut a)?;
        Ok(a.value)
    }

    pub fn raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

/// One syncobj timeline on a `Drm`. Points are signalled by whoever owns the "GPU" side; a
/// timeline's signalled value is monotonic: signalling N signals every point ≤ N (drm_syncobj
/// semantics), which is what lets the producer reclaim generations the consumer skipped once it
/// releases a newer one (§4 reclamation, per image).
pub struct Timeline<'d> {
    drm: &'d Drm,
    pub handle: u32,
}

impl<'d> Timeline<'d> {
    pub fn create(drm: &'d Drm) -> io::Result<Timeline<'d>> {
        let mut a = DrmSyncobjCreate { handle: 0, flags: 0 };
        ioctl(drm.raw_fd(), DRM_IOCTL_SYNCOBJ_CREATE, &mut a)?;
        Ok(Timeline { drm, handle: a.handle })
    }

    /// Import from an fd exported by `export` (registration, §3).
    pub fn import(drm: &'d Drm, fd: RawFd) -> io::Result<Timeline<'d>> {
        let mut a = DrmSyncobjHandle { handle: 0, flags: 0, fd, pad: 0 };
        ioctl(drm.raw_fd(), DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE, &mut a)?;
        Ok(Timeline { drm, handle: a.handle })
    }

    pub fn export(&self) -> io::Result<OwnedFd> {
        let mut a = DrmSyncobjHandle { handle: self.handle, flags: 0, fd: -1, pad: 0 };
        ioctl(self.drm.raw_fd(), DRM_IOCTL_SYNCOBJ_HANDLE_TO_FD, &mut a)?;
        // SAFETY: the kernel returned a fresh fd we own.
        Ok(unsafe { OwnedFd::from_raw_fd(a.fd) })
    }

    /// Signal `point` (and thereby every lower point) from userspace.
    pub fn signal(&self, point: u64) -> io::Result<()> {
        let handles = [self.handle];
        let points = [point];
        let mut a = DrmSyncobjTimelineArray { handles: handles.as_ptr() as u64, points: points.as_ptr() as u64, count_handles: 1, flags: 0 };
        ioctl(self.drm.raw_fd(), DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL, &mut a)
    }

    /// The last signalled point (0 if none). Never waits.
    pub fn signalled(&self) -> io::Result<u64> {
        let mut out = [0u64];
        query_many(self.drm, &[self.handle], &mut out)?;
        Ok(out[0])
    }

    /// Wait (the producer's or a test's business, never the consumer's pass). Counted when the
    /// timeout is non-zero. `timeout_ns` is relative; 0 = poll. Ok(true) signalled, Ok(false) timed out.
    pub fn wait(&self, point: u64, timeout_ns: i64) -> io::Result<bool> {
        if timeout_ns != 0 {
            NONZERO_TIMEOUT_WAITS.fetch_add(1, Ordering::Relaxed);
        }
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: plain clock read.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        let abs = ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64 + timeout_ns;
        let handles = [self.handle];
        let points = [point];
        let mut a = DrmSyncobjTimelineWait {
            handles: handles.as_ptr() as u64,
            points: points.as_ptr() as u64,
            timeout_nsec: abs,
            count_handles: 1,
            flags: DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL | DRM_SYNCOBJ_WAIT_FLAGS_WAIT_FOR_SUBMIT,
            first_signaled: 0,
            pad: 0,
        };
        match ioctl(self.drm.raw_fd(), DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT, &mut a) {
            Ok(()) => Ok(true),
            Err(e) if e.raw_os_error() == Some(libc::ETIME) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

impl Drop for Timeline<'_> {
    fn drop(&mut self) {
        let mut a = DrmSyncobjDestroy { handle: self.handle, pad: 0 };
        let _ = ioctl(self.drm.raw_fd(), DRM_IOCTL_SYNCOBJ_DESTROY, &mut a);
    }
}

/// One ioctl answering "last signalled point" for many timelines — the consumer's one
/// fence question per pass (§4 selection) for every image of a generation.
pub fn query_many(drm: &Drm, handles: &[u32], out: &mut [u64]) -> io::Result<()> {
    assert_eq!(handles.len(), out.len());
    if handles.is_empty() {
        return Ok(());
    }
    let mut a = DrmSyncobjTimelineArray { handles: handles.as_ptr() as u64, points: out.as_mut_ptr() as u64, count_handles: handles.len() as u32, flags: 0 };
    ioctl(drm.raw_fd(), DRM_IOCTL_SYNCOBJ_QUERY, &mut a)
}

// ---- memfd + udmabuf ---------------------------------------------------------------------------

/// A sealed memfd of `size` bytes (page-rounded).
pub fn memfd(name: &str, size: usize) -> io::Result<OwnedFd> {
    let c = std::ffi::CString::new(name).unwrap();
    // SAFETY: valid C string; flags are constants.
    let fd = unsafe { libc::memfd_create(c.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fresh fd we own.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let size = page_round(size);
    if unsafe { libc::ftruncate(fd.as_raw_fd(), size as libc::off_t) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // udmabuf requires F_SEAL_SHRINK; seal growth too — the tables are fixed at registration
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_ADD_SEALS, libc::F_SEAL_SHRINK | libc::F_SEAL_GROW) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

pub fn page_round(n: usize) -> usize {
    // SAFETY: sysconf is always safe to call.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    (n + page - 1) / page * page
}

#[repr(C)]
struct UdmabufCreate {
    memfd: u32,
    flags: u32,
    offset: u64,
    size: u64,
}
const UDMABUF_CREATE: libc::c_ulong = iow(b'u' as u32, 0x42, std::mem::size_of::<UdmabufCreate>());
const UDMABUF_FLAGS_CLOEXEC: u32 = 1;

/// `/dev/udmabuf`: dmabufs over memfds.
pub struct Udmabuf {
    dev: OwnedFd,
}

impl Udmabuf {
    pub fn open() -> io::Result<Udmabuf> {
        let f = std::fs::OpenOptions::new().read(true).write(true).open("/dev/udmabuf")?;
        Ok(Udmabuf { dev: OwnedFd::from(f) })
    }

    /// A dmabuf over the whole of a sealed, page-sized memfd.
    pub fn create(&self, mem: &OwnedFd, size: usize) -> io::Result<OwnedFd> {
        let mut a = UdmabufCreate { memfd: mem.as_raw_fd() as u32, flags: UDMABUF_FLAGS_CLOEXEC, offset: 0, size: page_round(size) as u64 };
        // SAFETY: exact struct for the request; the ioctl returns the new fd.
        let r = unsafe { libc::ioctl(self.dev.as_raw_fd(), UDMABUF_CREATE as _, &mut a as *mut UdmabufCreate) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fresh fd we own.
        Ok(unsafe { OwnedFd::from_raw_fd(r) })
    }
}

/// A shared mapping of an fd (memfd or dmabuf), unmapped on drop.
pub struct Mapping {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: the mapping is plain shared memory; callers coordinate through the protocol's atomics.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn map(fd: RawFd, len: usize, writable: bool) -> io::Result<Mapping> {
        let prot = if writable { libc::PROT_READ | libc::PROT_WRITE } else { libc::PROT_READ };
        // SAFETY: mmap with valid arguments; failure is checked.
        let p = unsafe { libc::mmap(std::ptr::null_mut(), len, prot, libc::MAP_SHARED, fd, 0) };
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Mapping { ptr: p as *mut u8, len })
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// The mapping as bytes. Shared memory: the other side may write concurrently; the protocol
    /// (§4) guarantees the regions a reader looks at are not being written.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: valid mapping of `len` bytes for its lifetime.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as above; writable mappings only reach here through the producer.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: unmapping what we mapped.
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
    }
}

/// Monotonic nanoseconds (the harness's stand-in for the Monado clock).
pub fn now_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: plain clock read.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_drm_h() {
        // drm.h: DRM_IOCTL_SYNCOBJ_CREATE = DRM_IOWR(0xBF, struct drm_syncobj_create) = 0xC00864BF
        assert_eq!(DRM_IOCTL_SYNCOBJ_CREATE, 0xC00864BF);
        assert_eq!(DRM_IOCTL_GET_CAP, 0xC010640C);
        assert_eq!(DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL, 0xC01864CD);
        assert_eq!(DRM_IOCTL_SYNCOBJ_QUERY, 0xC01864CB);
        assert_eq!(DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT, 0xC02864CA); // 40-byte struct
        // udmabuf.h: UDMABUF_CREATE = _IOW('u', 0x42, struct udmabuf_create) = 0x40187542
        assert_eq!(UDMABUF_CREATE, 0x40187542);
    }
}
