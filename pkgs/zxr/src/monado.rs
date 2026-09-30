//! The controller link to Monado (specs/composition.md §5.3, C0; native-openxr-apps §4).
//!
//! zxr is the session's controller: it connects to the runtime's *control* socket
//! (`monado_comp_ipc_control`) through libmonado, where the service stamps it `controller` at
//! accept and hands it the one controller lease (holder, or pending behind a holder that is
//! still going away). Everything zxr asks the runtime to do to other clients — primary, focus,
//! io — goes through this root; the OpenXR session (xr.rs) is an ordinary client and never
//! carries authority.
//!
//! libmonado is dlopen'd by path (`MURA_LIBMONADO`, baked by Nix; `ZXR_LIBMONADO` overrides for
//! a host run), the same shape as the OpenXR loader (`MURA_OPENXR_LOADER`), so the binary needs
//! no environment. Only `libc` is used for the loading: no new crate.
//!
//! Today the link is an observer at 1 Hz on the state loop: which client is primary (a native
//! app → quiet mode, `Zxr::primary_changed`) and whether zxr holds the lease. libmonado older
//! than 1.9 (no control socket) degrades to the application socket with `controller =
//! Unavailable`: the same observer, no authority — exactly upstream Monado's behaviour.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::time::Duration;

use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::LoopHandle;

use crate::state::Zxr;

/// `mnd_result_t` values we act on (libmonado monado.h).
const MND_SUCCESS: c_int = 0;
const MND_ERROR_NOT_CONTROLLER: c_int = -9;
/// `mnd_client_flags`.
const MND_CLIENT_PRIMARY_APP: u32 = 1 << 0;
/// `mnd_socket_t`.
const MND_SOCKET_CONTROL: c_int = 1;

/// This connection's standing with the controller lease (`mnd_controller_state_t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    /// libmonado predates the lease (< 1.9) or the control socket was refused: observer only.
    Unavailable,
    None,
    Holder,
    Pending,
}

impl Controller {
    pub fn as_str(self) -> &'static str {
        match self {
            Controller::Unavailable => "unavailable",
            Controller::None => "none",
            Controller::Holder => "holder",
            Controller::Pending => "pending",
        }
    }
    fn from_mnd(v: c_int) -> Self {
        match v {
            1 => Controller::Holder,
            2 => Controller::Pending,
            _ => Controller::None,
        }
    }
}

type MndRoot = c_void;

#[allow(non_snake_case)]
struct Api {
    _lib: *mut c_void,
    mnd_api_get_version: unsafe extern "C" fn(*mut u32, *mut u32, *mut u32),
    mnd_root_create: unsafe extern "C" fn(*mut *mut MndRoot) -> c_int,
    mnd_root_create_with_socket: Option<unsafe extern "C" fn(c_int, *mut *mut MndRoot) -> c_int>,
    mnd_root_destroy: unsafe extern "C" fn(*mut *mut MndRoot),
    mnd_root_update_client_list: unsafe extern "C" fn(*mut MndRoot) -> c_int,
    mnd_root_get_number_clients: unsafe extern "C" fn(*mut MndRoot, *mut u32) -> c_int,
    mnd_root_get_client_id_at_index: unsafe extern "C" fn(*mut MndRoot, u32, *mut u32) -> c_int,
    mnd_root_get_client_name: unsafe extern "C" fn(*mut MndRoot, u32, *mut *const c_char) -> c_int,
    mnd_root_get_client_state: unsafe extern "C" fn(*mut MndRoot, u32, *mut u32) -> c_int,
    mnd_root_get_controller_state: Option<unsafe extern "C" fn(*mut MndRoot, *mut c_int) -> c_int>,
    mnd_root_set_client_primary: unsafe extern "C" fn(*mut MndRoot, u32) -> c_int,
}

unsafe fn sym<T>(lib: *mut c_void, name: &str) -> Option<T> {
    let c = CString::new(name).ok()?;
    let p = libc::dlsym(lib, c.as_ptr());
    if p.is_null() {
        None
    } else {
        // SAFETY: the caller names a function whose signature matches T (libmonado's header).
        Some(std::mem::transmute_copy::<*mut c_void, T>(&p))
    }
}

impl Api {
    fn load(path: &str) -> Result<Api, String> {
        let c = CString::new(path).map_err(|e| e.to_string())?;
        // SAFETY: dlopen of a path; symbols are looked up by their documented names.
        let lib = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if lib.is_null() {
            let err = unsafe { libc::dlerror() };
            let msg = if err.is_null() { "dlopen failed".to_string() } else { unsafe { CStr::from_ptr(err) }.to_string_lossy().into_owned() };
            return Err(format!("{path}: {msg}"));
        }
        macro_rules! req {
            ($name:ident) => {
                unsafe { sym(lib, stringify!($name)) }.ok_or_else(|| format!("{path}: missing {}", stringify!($name)))?
            };
        }
        Ok(Api {
            _lib: lib,
            mnd_api_get_version: req!(mnd_api_get_version),
            mnd_root_create: req!(mnd_root_create),
            mnd_root_create_with_socket: unsafe { sym(lib, "mnd_root_create_with_socket") },
            mnd_root_destroy: req!(mnd_root_destroy),
            mnd_root_update_client_list: req!(mnd_root_update_client_list),
            mnd_root_get_number_clients: req!(mnd_root_get_number_clients),
            mnd_root_get_client_id_at_index: req!(mnd_root_get_client_id_at_index),
            mnd_root_get_client_name: req!(mnd_root_get_client_name),
            mnd_root_get_client_state: req!(mnd_root_get_client_state),
            mnd_root_get_controller_state: unsafe { sym(lib, "mnd_root_get_controller_state") },
            mnd_root_set_client_primary: req!(mnd_root_set_client_primary),
        })
    }
}

/// One libmonado root, on the control socket when the library offers it.
pub struct Link {
    api: Api,
    root: *mut MndRoot,
    pub version: (u32, u32, u32),
    pub controller: Controller,
    /// the client that is primary and is not zxr's own session (a native OpenXR app), by name
    pub native_primary: Option<String>,
    pub clients: u32,
    /// polls that failed since the last success; the link is dropped after a few
    failures: u32,
}

// The pointer is libmonado state used only from the state loop.
unsafe impl Send for Link {}

impl Link {
    /// Open libmonado and connect: the control socket first, the application socket when the
    /// library or the service predates it.
    pub fn connect() -> Result<Link, String> {
        let path = std::env::var("ZXR_LIBMONADO").ok().or_else(|| option_env!("MURA_LIBMONADO").map(String::from)).unwrap_or_else(|| "libmonado.so".into());
        let api = Api::load(&path)?;
        let (mut major, mut minor, mut patch) = (0u32, 0u32, 0u32);
        unsafe { (api.mnd_api_get_version)(&mut major, &mut minor, &mut patch) };
        if major != 1 {
            return Err(format!("libmonado API {major}.{minor}.{patch}: major 1 expected"));
        }
        let mut root: *mut MndRoot = std::ptr::null_mut();
        let mut controller = Controller::Unavailable;
        if let (Some(create), Some(_)) = (api.mnd_root_create_with_socket, api.mnd_root_get_controller_state) {
            let r = unsafe { create(MND_SOCKET_CONTROL, &mut root) };
            if r == MND_SUCCESS && !root.is_null() {
                controller = Controller::None;
            } else {
                tracing::warn!(result = r, "libmonado: control socket refused; observing on the application socket");
                root = std::ptr::null_mut();
            }
        } else {
            tracing::info!(version = format!("{major}.{minor}.{patch}"), "libmonado predates the controller lease (1.9); observing only");
        }
        if root.is_null() {
            let r = unsafe { (api.mnd_root_create)(&mut root) };
            if r != MND_SUCCESS || root.is_null() {
                return Err(format!("mnd_root_create: {r}"));
            }
        }
        let mut link = Link { api, root, version: (major, minor, patch), controller, native_primary: None, clients: 0, failures: 0 };
        link.poll();
        Ok(link)
    }

    /// Refresh the client list, the primary and the lease standing. Cheap: one IPC round trip
    /// per client plus one for the lease.
    pub fn poll(&mut self) -> bool {
        let r = unsafe { (self.api.mnd_root_update_client_list)(self.root) };
        if r != MND_SUCCESS {
            self.failures += 1;
            return false;
        }
        let mut n = 0u32;
        unsafe { (self.api.mnd_root_get_number_clients)(self.root, &mut n) };
        self.clients = n;
        let mut native_primary = None;
        for i in 0..n {
            let mut id = 0u32;
            if unsafe { (self.api.mnd_root_get_client_id_at_index)(self.root, i, &mut id) } != MND_SUCCESS {
                continue;
            }
            let mut flags = 0u32;
            if unsafe { (self.api.mnd_root_get_client_state)(self.root, id, &mut flags) } != MND_SUCCESS {
                continue;
            }
            if flags & MND_CLIENT_PRIMARY_APP == 0 {
                continue;
            }
            let mut name: *const c_char = std::ptr::null();
            let name = if unsafe { (self.api.mnd_root_get_client_name)(self.root, id, &mut name) } == MND_SUCCESS && !name.is_null() {
                unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
            } else {
                format!("client {id}")
            };
            // zxr's own session (xr.rs names it "zxr") being primary is the greeter/lock
            // picture, not a native app.
            if name != "zxr" && name != "libmonado" {
                native_primary = Some(name);
            }
        }
        self.native_primary = native_primary;
        if self.controller != Controller::Unavailable {
            if let Some(get) = self.api.mnd_root_get_controller_state {
                let mut s: c_int = 0;
                if unsafe { get(self.root, &mut s) } == MND_SUCCESS {
                    self.controller = Controller::from_mnd(s);
                }
            }
        }
        self.failures = 0;
        true
    }

    /// Make `client_id` primary: the lease holder's verb (§5.3.3); `Err` names the refusal.
    pub fn set_primary(&mut self, client_id: u32) -> Result<(), String> {
        match unsafe { (self.api.mnd_root_set_client_primary)(self.root, client_id) } {
            MND_SUCCESS => Ok(()),
            MND_ERROR_NOT_CONTROLLER => Err(format!("not the controller ({})", self.controller.as_str())),
            r => Err(format!("mnd_root_set_client_primary: {r}")),
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "monado: libmonado={}.{}.{} controller={} clients={} native_primary={}",
            self.version.0,
            self.version.1,
            self.version.2,
            self.controller.as_str(),
            self.clients,
            self.native_primary.as_deref().unwrap_or("none")
        )
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if !self.root.is_null() {
            unsafe { (self.api.mnd_root_destroy)(&mut self.root) };
        }
    }
}

/// Connect and install the 1 Hz observer on the state loop. Failure to connect is logged, not
/// fatal: the session runs without the runtime link (4.2's default policy applies).
pub fn install(st: &mut Zxr, handle: &LoopHandle<'static, Zxr>) {
    match Link::connect() {
        Ok(link) => {
            tracing::info!("{}", link.describe());
            let native = link.native_primary.is_some();
            st.monado = Some(link);
            st.primary_changed(native);
        }
        Err(e) => {
            tracing::warn!("libmonado: {e}; running without the runtime link");
            return;
        }
    }
    let _ = handle.insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, st| {
        let Some(link) = st.monado.as_mut() else { return TimeoutAction::Drop };
        let before = (link.controller, link.native_primary.clone());
        if !link.poll() && link.failures >= 3 {
            tracing::warn!("libmonado: the runtime link failed three times; dropping it");
            st.monado = None;
            return TimeoutAction::Drop;
        }
        let after = (link.controller, link.native_primary.clone());
        if before.0 != after.0 {
            tracing::info!(from = before.0.as_str(), to = after.0.as_str(), "controller lease");
        }
        if before.1 != after.1 {
            tracing::info!(primary = ?after.1, "runtime primary changed");
            let native = after.1.is_some();
            st.primary_changed(native);
        }
        TimeoutAction::ToDuration(Duration::from_secs(1))
    });
}
