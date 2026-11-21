//! The probe's one `unsafe` module: `dlopen`, a `socketpair`, and the C calls.
//!
//! Everything in this file exists to prove the linkage rule rather than assume
//! it: `libphp` is opened `RTLD_NOW | RTLD_GLOBAL` first, the shim second with
//! `RTLD_NOW | RTLD_LOCAL`, and opening the shim alone must fail with
//! `undefined symbol` instead of crashing. `c_int` constants are the Linux
//! glibc values; the container is `debian:trixie-slim` on x86_64.

//! glibc >= 2.34 folds libdl and the socket calls into libc, and trixie is 2.41,
//! so no `#[link]` is needed and there is nothing else to link against.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::{CStr, CString};
use std::path::Path;

use blastit_php_sapi::abi::{
    AbiVersionFn, ConfigV1, MinorFn, RequestFn, RequestV1, ResultV1, SinkV1, WorkerInitFn,
    WorkerShutdownFn,
};

/// `RTLD_NOW` (glibc `dlfcn.h`).
const RTLD_NOW: c_int = 2;
/// `RTLD_GLOBAL` (glibc `dlfcn.h`).
const RTLD_GLOBAL: c_int = 0x100;
/// `RTLD_LOCAL` (glibc `dlfcn.h`).
const RTLD_LOCAL: c_int = 0;
/// `AF_UNIX`.
const AF_UNIX: c_int = 1;
/// `SOCK_STREAM`.
const SOCK_STREAM: c_int = 1;
/// `MSG_DONTWAIT` (Linux `sys/socket.h`).
const MSG_DONTWAIT: c_int = 0x40;

/// Frame kinds on the probe's stand-in for `view.chan`.
const FRAME_HEAD: u8 = 1;
const FRAME_BODY: u8 = 2;
const FRAME_LOG: u8 = 3;

// glibc >= 2.34 folds libdl and the socket calls into libc, and trixie is 2.41,
// so no `#[link]` is needed and there is nothing else to link against.
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
    fn socketpair(domain: c_int, socket_type: c_int, protocol: c_int, sv: *mut c_int) -> c_int;
    fn socket(domain: c_int, socket_type: c_int, protocol: c_int) -> c_int;
    fn recv(fd: c_int, buf: *mut c_void, len: usize, flags: c_int) -> isize;
    fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    fn close(fd: c_int) -> c_int;
}

/// `AF_INET` (glibc `sys/socket.h`).
const AF_INET: c_int = 2;

/// `socket(AF_INET, SOCK_STREAM, 0)`, reported as an errno.
///
/// `0` means the call succeeded, i.e. **no egress filter is installed** in this
/// process; `1` (`EPERM`) is the seccomp `View { egress: false }` refusal. The
/// probe asks once before PHP starts and once after the requests: it is the
/// only evidence that does not depend on how a PHP extension chooses to word a
/// failure, and it is why `curl`'s generic "Could not connect to server" cannot
/// be mistaken for proof of anything.
pub fn inet_socket_errno() -> i32 {
    // SAFETY: `socket(2)` with constant arguments. A descriptor it returns is
    // closed here; the call is the same one PHP's streams and curl make.
    let fd = unsafe { socket(AF_INET, SOCK_STREAM, 0) };
    if fd < 0 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
    } else {
        // SAFETY: `fd` was just returned by `socket` and is owned here.
        unsafe { close(fd) };
        0
    }
}

/// One frame the worker would have written to `view.chan`.
#[derive(Debug)]
pub enum Frame {
    /// One header, or the end-of-head marker (`name` is empty).
    Head {
        /// The response status carried on every head frame.
        status: i32,
        /// The header name, empty on the marker.
        name: String,
        /// The header value, empty on the marker.
        value: String,
    },
    /// One body chunk.
    Body(Vec<u8>),
    /// One bounded log line.
    Log {
        /// The PHP `syslog_type` (error level).
        level: i32,
        /// The line, already truncated to 2048 bytes by the shim.
        line: String,
    },
}

/// The last `dlerror()` message, or a placeholder when there is none.
fn dl_error() -> String {
    // SAFETY: dlerror() returns a NUL-terminated string owned by libdl, valid
    // until the next dlerror()/dlopen() call on this thread; it is copied here
    // before any other dl* call.
    let message = unsafe {
        let raw = dlerror();
        if raw.is_null() {
            None
        } else {
            Some(CStr::from_ptr(raw).to_string_lossy().into_owned())
        }
    };
    message.unwrap_or_else(|| "no dlerror() message".to_owned())
}

/// Opens the shim *alone*: it must be refused, never crash the probe.
///
/// Returns `"refused: <dlerror>"` when the shim's undefined Zend/SAPI symbols
/// cannot resolve (the expected outcome), or `"loaded ..."` when it was not.
pub fn probe_shim_alone(shim: &Path) -> String {
    let Ok(shim) = CString::new(shim.to_string_lossy().as_bytes()) else {
        return "error: the shim path holds a NUL".to_owned();
    };
    // SAFETY: `shim` is a NUL-terminated path that outlives the call. RTLD_NOW
    // makes every unresolved symbol an error now, so a shim without libphp
    // fails here instead of at the first request.
    let handle = unsafe { dlopen(shim.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
    if handle.is_null() {
        return format!("refused: {}", dl_error());
    }
    // SAFETY: `handle` is a live handle from dlopen above and is closed once.
    unsafe { dlclose(handle) };
    "loaded (the shim resolved without libphp)".to_owned()
}

/// A loaded `libphp` + shim pair, with the frozen entry points resolved.
pub struct Shim {
    libphp: *mut c_void,
    shim: *mut c_void,
    abi_version: AbiVersionFn,
    minor: MinorFn,
    worker_init: WorkerInitFn,
    worker_shutdown: WorkerShutdownFn,
    request: RequestFn,
}

impl Shim {
    /// `dlopen(libphp, RTLD_NOW|RTLD_GLOBAL)` then `dlopen(shim, RTLD_NOW|RTLD_LOCAL)`.
    pub fn load(libphp: &Path, shim: &Path) -> Result<Self, String> {
        let libphp_c = CString::new(libphp.to_string_lossy().as_bytes())
            .map_err(|_| "the libphp path holds a NUL".to_owned())?;
        let shim_c = CString::new(shim.to_string_lossy().as_bytes())
            .map_err(|_| "the shim path holds a NUL".to_owned())?;

        // SAFETY: both paths are NUL-terminated and outlive the call. GLOBAL is
        // what lets the shim's undefined Zend/SAPI symbols resolve against it.
        let libphp_handle = unsafe { dlopen(libphp_c.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
        if libphp_handle.is_null() {
            return Err(format!(
                "dlopen({}) failed: {}",
                libphp.display(),
                dl_error()
            ));
        }
        // SAFETY: as above; LOCAL keeps the shim's symbols out of the global
        // namespace, so no other dlopen in this process can pick them up.
        let shim_handle = unsafe { dlopen(shim_c.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if shim_handle.is_null() {
            let error = dl_error();
            // SAFETY: libphp_handle is live and is closed exactly once here.
            unsafe { dlclose(libphp_handle) };
            return Err(format!("dlopen({}) failed: {error}", shim.display()));
        }

        // SAFETY: each symbol is looked up in the live shim handle and its
        // address is transmuted to the exact signature in `blastit_php_sapi::abi`,
        // which mirrors `c/include/blastit_php_abi_v1.h`; the probe asserts the
        // reported ABI version before calling anything else.
        unsafe {
            let abi_version = symbol(shim_handle, b"blastit_php_abi_version\0")?;
            let minor = symbol(shim_handle, b"blastit_php_minor\0")?;
            let worker_init = symbol(shim_handle, b"blastit_php_worker_init\0")?;
            let worker_shutdown = symbol(shim_handle, b"blastit_php_worker_shutdown\0")?;
            let request = symbol(shim_handle, b"blastit_php_request\0")?;
            Ok(Self {
                libphp: libphp_handle,
                shim: shim_handle,
                abi_version: std::mem::transmute::<*mut c_void, AbiVersionFn>(abi_version),
                minor: std::mem::transmute::<*mut c_void, MinorFn>(minor),
                worker_init: std::mem::transmute::<*mut c_void, WorkerInitFn>(worker_init),
                worker_shutdown: std::mem::transmute::<*mut c_void, WorkerShutdownFn>(
                    worker_shutdown,
                ),
                request: std::mem::transmute::<*mut c_void, RequestFn>(request),
            })
        }
    }

    /// `blastit_php_abi_version()`.
    pub fn abi_version(&self) -> u32 {
        // SAFETY: the pointer came from dlsym on the live shim handle and its
        // type matches the header; the probe calls it before anything else.
        unsafe { (self.abi_version)() }
    }

    /// `blastit_php_minor()`, e.g. `804`.
    pub fn minor(&self) -> u32 {
        // SAFETY: as `abi_version`.
        unsafe { (self.minor)() }
    }

    /// `blastit_php_worker_init()`, which runs `php_module_startup`.
    pub fn worker_init(&self, cfg: &ConfigV1) -> i32 {
        // SAFETY: `cfg` borrows live `CString`s of the INI blob for this call.
        unsafe { (self.worker_init)(cfg) }
    }

    /// `blastit_php_request()`; the shim traps every `zend_bailout()` itself.
    pub fn request(&self, req: &RequestV1, sink: &SinkV1, out: &mut ResultV1) -> i32 {
        // SAFETY: `req` points at buffers that outlive the call, `sink` at the
        // probe's live state, `out` at a local ResultV1.
        unsafe { (self.request)(req, sink, out) }
    }

    /// `blastit_php_worker_shutdown()`, then both handles are dropped.
    pub fn worker_shutdown(&self) {
        // SAFETY: called once, after `worker_init` succeeded.
        unsafe { (self.worker_shutdown)() };
    }
}

impl Drop for Shim {
    fn drop(&mut self) {
        // SAFETY: both handles came from dlopen and are closed exactly once.
        unsafe {
            if !self.shim.is_null() {
                dlclose(self.shim);
            }
            if !self.libphp.is_null() {
                dlclose(self.libphp);
            }
        }
    }
}

/// Looks one NUL-terminated symbol name up in `handle`.
///
/// # Safety
///
/// `handle` must be a live `dlopen` handle and `name` NUL-terminated.
unsafe fn symbol(handle: *mut c_void, name: &[u8]) -> Result<*mut c_void, String> {
    debug_assert_eq!(name.last(), Some(&0));
    // SAFETY: the caller guarantees a live handle and a NUL-terminated name.
    let ptr = unsafe { dlsym(handle, name.as_ptr().cast::<c_char>()) };
    if ptr.is_null() {
        return Err(format!(
            "dlsym({}) failed: {}",
            String::from_utf8_lossy(name).trim_end_matches('\0'),
            dl_error()
        ));
    }
    Ok(ptr)
}

/// The probe's stand-in for `view.chan`: the write end of a `socketpair`.
pub struct Sink {
    tx: c_int,
    rx: c_int,
    state: Box<SinkState>,
}

struct SinkState {
    fd: c_int,
}

impl Sink {
    /// Creates the socketpair and the C sink the shim writes into.
    pub fn new() -> Result<Self, String> {
        let mut fds = [0 as c_int; 2];
        // SAFETY: `fds` is two initialised c_int slots.
        let rc = unsafe { socketpair(AF_UNIX, SOCK_STREAM, 0, fds.as_mut_ptr()) };
        if rc != 0 {
            return Err(format!("socketpair failed (rc {rc})"));
        }
        let state = Box::new(SinkState { fd: fds[0] });
        Ok(Self {
            tx: fds[0],
            rx: fds[1],
            state,
        })
    }

    /// The `blastit_php_sink_v1` the shim receives.
    pub fn vtable(&self) -> SinkV1 {
        SinkV1 {
            ctx: (&*self.state as *const SinkState)
                .cast_mut()
                .cast::<c_void>(),
            body: Some(sink_body),
            head: Some(sink_head),
            log: Some(sink_log),
        }
    }

    /// Everything written since the last drain, parsed into frames.
    pub fn drain(&self) -> Result<Vec<Frame>, String> {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            // SAFETY: `chunk` is a live buffer of `chunk.len()` bytes.
            let read = unsafe {
                recv(
                    self.rx,
                    chunk.as_mut_ptr().cast::<c_void>(),
                    chunk.len(),
                    MSG_DONTWAIT,
                )
            };
            if read > 0 {
                let read = usize::try_from(read).map_err(|_| "recv returned < 0".to_owned())?;
                bytes.extend_from_slice(&chunk[..read]);
                continue;
            }
            // EAGAIN (or a closed peer) ends the drain; the shim never blocks
            // because the frames are far smaller than the socket buffer.
            break;
        }
        parse_frames(&bytes)
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        // SAFETY: both descriptors came from socketpair and are closed once.
        unsafe {
            close(self.tx);
            close(self.rx);
        }
    }
}

/// Writes one frame and returns 0 on success, the way the C ABI expects.
///
/// # Safety
///
/// `ctx` must be the live `SinkState` pointer this probe put in `SinkV1.ctx`.
unsafe extern "C" fn sink_body(ctx: *mut c_void, data: *const u8, len: usize) -> i32 {
    let Some(state) = (unsafe { ctx.cast::<SinkState>().as_ref() }) else {
        return 1;
    };
    if data.is_null() {
        return 1;
    }
    // SAFETY: the shim passes `data`/`len` as a live slice for this call.
    let payload = unsafe { core::slice::from_raw_parts(data, len) };
    unsafe { write_frame(state.fd, FRAME_BODY, payload) }
}

/// Emits one head frame, or the end-of-head marker when `name` is null.
///
/// # Safety
///
/// `ctx` must be the probe's live `SinkState`; `name`/`value` must be readable
/// for their declared lengths.
unsafe extern "C" fn sink_head(
    ctx: *mut c_void,
    status: i32,
    name: *const c_char,
    name_len: usize,
    value: *const c_char,
    value_len: usize,
) -> i32 {
    let Some(state) = (unsafe { ctx.cast::<SinkState>().as_ref() }) else {
        return 1;
    };
    let name_bytes = if name.is_null() || name_len == 0 {
        &[][..]
    } else {
        // SAFETY: the shim declares `name` readable for `name_len` bytes.
        unsafe { core::slice::from_raw_parts(name.cast::<u8>(), name_len) }
    };
    let value_bytes = if value.is_null() || value_len == 0 {
        &[][..]
    } else {
        // SAFETY: the shim declares `value` readable for `value_len` bytes.
        unsafe { core::slice::from_raw_parts(value.cast::<u8>(), value_len) }
    };

    let mut payload = Vec::with_capacity(10 + name_bytes.len() + value_bytes.len());
    payload.extend_from_slice(&status.to_le_bytes());
    let Ok(name_len16) = u16::try_from(name_bytes.len()) else {
        return 1;
    };
    payload.extend_from_slice(&name_len16.to_le_bytes());
    payload.extend_from_slice(name_bytes);
    let Ok(value_len32) = u32::try_from(value_bytes.len()) else {
        return 1;
    };
    payload.extend_from_slice(&value_len32.to_le_bytes());
    payload.extend_from_slice(value_bytes);

    unsafe { write_frame(state.fd, FRAME_HEAD, &payload) }
}

/// Emits one log frame.
///
/// # Safety
///
/// `ctx` must be the probe's live `SinkState`; `line` must be readable for `len`.
unsafe extern "C" fn sink_log(ctx: *mut c_void, level: i32, line: *const c_char, len: usize) {
    let Some(state) = (unsafe { ctx.cast::<SinkState>().as_ref() }) else {
        return;
    };
    if line.is_null() || len == 0 {
        return;
    }
    // SAFETY: the shim declares `line` readable for `len` bytes.
    let line_bytes = unsafe { core::slice::from_raw_parts(line.cast::<u8>(), len) };
    let mut payload = Vec::with_capacity(4 + line_bytes.len());
    payload.extend_from_slice(&level.to_le_bytes());
    payload.extend_from_slice(line_bytes);
    // A log write failure is not a request failure: drop it.
    let _ = unsafe { write_frame(state.fd, FRAME_LOG, &payload) };
}

/// Writes `kind | len | payload`, looping over short writes.
///
/// # Safety
///
/// `fd` must be the probe's socketpair write end.
unsafe fn write_frame(fd: c_int, kind: u8, payload: &[u8]) -> i32 {
    let Ok(len) = u32::try_from(payload.len()) else {
        return 1;
    };
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(kind);
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(payload);

    let mut written = 0_usize;
    while written < frame.len() {
        // SAFETY: the slice starts at `written` and `fd` is the write end.
        let rc = unsafe {
            write(
                fd,
                frame[written..].as_ptr().cast::<c_void>(),
                frame.len() - written,
            )
        };
        if rc <= 0 {
            return 1;
        }
        written += usize::try_from(rc).unwrap_or(frame.len() - written);
    }

    0
}

/// Splits a drained byte stream into frames.
fn parse_frames(bytes: &[u8]) -> Result<Vec<Frame>, String> {
    let mut frames = Vec::new();
    let mut pos = 0_usize;
    while pos < bytes.len() {
        if pos + 5 > bytes.len() {
            return Err("a frame header was truncated".to_owned());
        }
        let kind = bytes[pos];
        let len = u32::from_le_bytes([
            bytes[pos + 1],
            bytes[pos + 2],
            bytes[pos + 3],
            bytes[pos + 4],
        ]);
        let len = usize::try_from(len).map_err(|_| "a frame length overflowed".to_owned())?;
        let start = pos + 5;
        let end = start
            .checked_add(len)
            .ok_or_else(|| "a frame length overflowed".to_owned())?;
        if end > bytes.len() {
            return Err("a frame body was truncated".to_owned());
        }
        let payload = &bytes[start..end];
        frames.push(match kind {
            FRAME_HEAD => {
                if payload.len() < 10 {
                    return Err("a head frame was truncated".to_owned());
                }
                let status = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let name_len = usize::from(u16::from_le_bytes([payload[4], payload[5]]));
                let value_start = 6 + name_len;
                if value_start + 4 > payload.len() {
                    return Err("a head frame name overflowed".to_owned());
                }
                let value_len = u32::from_le_bytes([
                    payload[value_start],
                    payload[value_start + 1],
                    payload[value_start + 2],
                    payload[value_start + 3],
                ]);
                let value_len =
                    usize::try_from(value_len).map_err(|_| "a head value overflowed".to_owned())?;
                let value_start = value_start + 4;
                if value_start + value_len > payload.len() {
                    return Err("a head frame value overflowed".to_owned());
                }
                Frame::Head {
                    status,
                    name: String::from_utf8_lossy(&payload[6..6 + name_len]).into_owned(),
                    value: String::from_utf8_lossy(&payload[value_start..value_start + value_len])
                        .into_owned(),
                }
            }
            FRAME_BODY => Frame::Body(payload.to_vec()),
            FRAME_LOG => {
                if payload.len() < 4 {
                    return Err("a log frame was truncated".to_owned());
                }
                let level = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                Frame::Log {
                    level,
                    line: String::from_utf8_lossy(&payload[4..]).into_owned(),
                }
            }
            other => return Err(format!("unknown frame kind {other}")),
        });
        pos = end;
    }
    Ok(frames)
}
