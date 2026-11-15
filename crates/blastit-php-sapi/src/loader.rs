//! The one module that `dlopen`s `libphp` and a shim, and the safe wrappers a
//! view worker drives them through.
//!
//! The crate root says `deny(unsafe_code)`; this module (and `abi`, which only
//! *names* the types) is the whole hand-off to C, and the allow is on this
//! module rather than on the callers so `blastit-view` stays
//! `#![forbid(unsafe_code)]` while running a real PHP request.
//!
//! # The two-step load (ADR-0010 §1)
//!
//! 1. the host's NTS `libphp.so` with `RTLD_NOW | RTLD_GLOBAL`, so the shim's
//!    undefined Zend/SAPI symbols — and every extension the host loads later
//!    (`pdo_pgsql.so`, `intl.so`) — resolve against it;
//! 2. the matching `libblastit_php8X.so` shim with `RTLD_NOW | RTLD_LOCAL`, so
//!    its symbols never enter the global namespace.
//!
//! In production both files are already-open descriptors, and the load names
//! them `/proc/self/fd/<n>`: a path could be swapped between the stage-0
//! verification and this call. [`SharedObject::Path`] exists for native
//! development and the probe, never as a fallback for a missing fd.
//!
//! # Failures are values, not crashes
//!
//! Every refusal is a [`LoadError`] naming the file and the reason, including
//! "this is not a shared library" (`dlopen` of a directory or a text file), a
//! shim that cannot resolve (opened before `libphp`, or the wrong minor), an
//! ABI version other than 1, and a shim whose minor does not match the loaded
//! `php_version()`. The worker turns one into `exit 1` before it sends `READY`
//! — never a first-request fault.
//!
//! # Threading
//!
//! NTS `libphp` is *not* thread-safe: [`Loaded`] is `Send` (one process-wide
//! handle moved between threads is fine) but deliberately not `Sync`, so safe
//! code cannot run two requests at once. A caller that shares it across threads
//! does so behind a lock, which is exactly what `PhpEmbedRuntime` does.

#![allow(unsafe_code)]

use core::ffi::{c_char, c_int, c_void};
use std::ffi::{CStr, CString};
use std::fmt;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

use crate::abi::{
    AbiVersionFn, ConfigV1, MinorFn, OUTCOME_BAILOUT, OUTCOME_OK, RequestFn, RequestV1, ResultV1,
    SinkV1, WorkerInitFn, WorkerShutdownFn,
};
use crate::toolchain::{Minor, SUPPORTED_MINORS};

/// `RTLD_NOW` (glibc `dlfcn.h`): every undefined symbol must resolve now.
const RTLD_NOW: c_int = 2;
/// `RTLD_GLOBAL` (glibc `dlfcn.h`): the shim resolves against this image.
const RTLD_GLOBAL: c_int = 0x100;
/// `RTLD_LOCAL` (glibc `dlfcn.h`): the shim's symbols stay private.
const RTLD_LOCAL: c_int = 0;

// glibc >= 2.34 folds `libdl` into libc, so no `#[link]` and no dependency is
// needed; the workspace's oldest glibc target is 2.28, where `libdl.so.2` is
// still a separate library — but it is already a `DT_NEEDED` of every glibc
// image and its symbols are visible to a plain `extern` declaration.
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

/// Where a shared object is, as `dlopen` will be told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedObject {
    /// An open, verified descriptor. It is named `/proc/self/fd/<fd>`, which
    /// reopens the same inode however the directory around it changed.
    Fd(RawFd),
    /// A path, for native development and the probe. A production boot never
    /// takes this variant (ADR-0010 §3).
    Path(PathBuf),
}

impl SharedObject {
    /// The path `dlopen` is given: `/proc/self/fd/<fd>` for a descriptor.
    #[must_use]
    pub fn dlopen_path(&self) -> PathBuf {
        match self {
            Self::Fd(fd) => PathBuf::from(format!("/proc/self/fd/{fd}")),
            Self::Path(path) => path.clone(),
        }
    }
}

/// Why a load was refused. Every variant is a value the worker can print and
/// exit on; none of them is a panic.
#[derive(Debug)]
pub enum LoadError {
    /// `dlopen` of the host `libphp` failed: missing, not a shared library, a
    /// dependency that will not resolve, or the wrong architecture.
    NotALibrary {
        /// The file that was opened.
        path: PathBuf,
        /// `dlerror()`'s message, which names the missing dependency.
        detail: String,
    },
    /// `libphp` loaded but the shim did not: the usual cause is a shim opened
    /// before `libphp` (undefined Zend symbols `RTLD_NOW` refuses), a shim
    /// built for another PHP minor, or another architecture.
    ShimRefused {
        /// The shim that was opened.
        path: PathBuf,
        /// `dlerror()`'s message.
        detail: String,
    },
    /// The shim reports [a version this crate does not speak](crate::ABI_VERSION).
    AbiVersion {
        /// What `blastit_php_abi_version()` returned.
        found: u32,
    },
    /// `blastit_php_minor()` is not one of [`SUPPORTED_MINORS`].
    MinorUnsupported {
        /// What `blastit_php_minor()` returned (`803`, `804`, `805`, …).
        found: u32,
        /// The shim that reported it.
        path: PathBuf,
    },
    /// The shim was built for one minor and the host's `libphp` is another.
    MinorMismatch {
        /// The shim's minor.
        shim: Minor,
        /// `php_version()` as the loaded `libphp` reported it.
        host: String,
    },
    /// A required entry point is missing from the loaded images.
    Symbol {
        /// The symbol's name.
        name: String,
        /// `dlerror()`'s message.
        detail: String,
    },
    /// `php_version()` returned a null pointer — not an embeddable `libphp`.
    NoVersion,
    /// A path or the INI blob holds a NUL, so it cannot be a C string.
    Nul {
        /// What held the byte.
        what: &'static str,
    },
    /// `blastit_php_worker_init` refused; PHP's own message is on stderr.
    Init {
        /// The negative code the shim returned.
        code: i32,
    },
    /// `blastit_php_request` refused (the worker is not initialised, a nested
    /// call, or a null argument) — a caller bug, never a script's outcome.
    Request {
        /// The negative code the shim returned.
        code: i32,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotALibrary { path, detail } => write!(
                f,
                "{} is not a usable NTS libphp: {detail} (install libphp-embed, or set \
                 BLASTIT_LIBPHP=/path/to/libphp.so)",
                path.display()
            ),
            Self::ShimRefused { path, detail } => write!(
                f,
                "the shim {} did not load: {detail} (it must be the build for this PHP minor \
                 and must be opened after libphp)",
                path.display()
            ),
            Self::AbiVersion { found } => write!(
                f,
                "the shim speaks blastit_php_abi version {found}, this build speaks {}",
                crate::ABI_VERSION
            ),
            Self::MinorUnsupported { found, path } => write!(
                f,
                "{} was built for PHP minor {found}, which this build has no ABI for \
                 (8.3, 8.4 and 8.5 are supported)",
                path.display()
            ),
            Self::MinorMismatch { shim, host } => write!(
                f,
                "the shim is built for PHP {} but the host's libphp reports {host}; install \
                 the libblastit_php{} shim for this host",
                shim.dotted(),
                shim.dotted().replace('.', "")
            ),
            Self::Symbol { name, detail } => {
                write!(f, "the loaded images have no `{name}`: {detail}")
            }
            Self::NoVersion => f.write_str(
                "libphp's php_version() returned NULL; this is not an embeddable NTS libphp",
            ),
            Self::Nul { what } => write!(f, "{what} holds a NUL byte"),
            Self::Init { code } => write!(
                f,
                "php_module_startup failed (code {code}); PHP's own message is above, and the \
                 module stays loaded because the worker exits"
            ),
            Self::Request { code } => write!(f, "the shim refused the request (code {code})"),
        }
    }
}

impl std::error::Error for LoadError {}

/// What a view worker implements for the shim's `blastit_php_sink_v1`.
///
/// Every method runs inside `blastit_php_request`, on this thread. `head` is
/// called once per response header and then once more with `name == None` (the
/// end-of-head marker, which always carries the status), so a head is written
/// exactly once even when the script sent none.
pub trait Sink {
    /// One response header, or the end-of-head marker. `false` aborts output.
    fn head(&mut self, status: i32, name: Option<&[u8]>, value: &[u8]) -> bool;

    /// One response body chunk. `false` aborts output.
    fn body(&mut self, data: &[u8]) -> bool;

    /// One bounded log line (the shim truncates to 2048 bytes).
    fn log(&mut self, level: i32, line: &str);
}

/// A loaded `libphp` + shim pair with every frozen entry point resolved.
pub struct Loaded {
    /// `libphp`'s handle. Never closed once a module startup has run.
    libphp: *mut c_void,
    /// The shim's handle; closed only when the load itself failed after it.
    shim: *mut c_void,
    /// What `php_version()` reported, e.g. `"8.4.27"`.
    host_version: String,
    /// The shim's minor, already checked against [`Loaded::host_version`].
    minor: Minor,
    abi_version: AbiVersionFn,
    worker_init: WorkerInitFn,
    worker_shutdown: WorkerShutdownFn,
    request: RequestFn,
    /// Set by [`Loaded::worker_init`]. A request before it is a caller bug.
    initialized: bool,
    /// Set the instant `worker_init` is entered, whether or not it succeeds.
    ///
    /// The shim's `-2` means `sapi_startup` and `php_module_startup` *ran* and
    /// failed; unloading `libphp` after that would run PHP's own library and
    /// module teardown paths on a half-initialised module table. So the
    /// handles are leaked deliberately once a startup has been attempted — the
    /// worker process exits instead.
    startup_attempted: bool,
}

// SAFETY: `libphp` is process-wide, non-thread-safe state. Handing the one
// handle to another thread is fine — there is still exactly one of it — and
// `Sync` is deliberately **not** implemented, so two threads cannot reach
// `blastit_php_request` at once through safe code. `PhpEmbedRuntime` puts it
// behind a `Mutex` for that reason.
unsafe impl Send for Loaded {}

impl Loaded {
    /// Loads `libphp` and then `shim`, and verifies the ABI and the minor.
    ///
    /// # Errors
    ///
    /// [`LoadError`] for every refusal; see the module documentation.
    pub fn load(libphp: SharedObject, shim: SharedObject) -> Result<Self, LoadError> {
        let libphp_path = libphp.dlopen_path();
        let shim_path = shim.dlopen_path();
        let libphp_c = c_path(&libphp_path, "the libphp path")?;
        let shim_c = c_path(&shim_path, "the shim path")?;

        // SAFETY: both names are NUL-terminated and outlive the calls. GLOBAL
        // is what lets the shim's undefined Zend/SAPI symbols resolve.
        let libphp_handle = unsafe { dlopen(libphp_c.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
        if libphp_handle.is_null() {
            return Err(LoadError::NotALibrary {
                path: libphp_path,
                detail: dl_error(),
            });
        }
        // SAFETY: as above; LOCAL keeps the shim's symbols out of the global
        // namespace so no other `dlopen` in this process can pick them up.
        let shim_handle = unsafe { dlopen(shim_c.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if shim_handle.is_null() {
            let detail = dl_error();
            // SAFETY: `libphp_handle` is live and nothing else owns it.
            unsafe { dlclose(libphp_handle) };
            return Err(LoadError::ShimRefused {
                path: shim_path,
                detail,
            });
        }

        let loaded = (|| {
            // SAFETY: each symbol is looked up in a live handle and transmuted
            // to the signature `abi` mirrors from the C header. `transmute`
            // checks the *size*; the signature itself is guarded by `abi.rs`,
            // the shim's `_Static_assert`s and `critical_shim_contract`.
            let abi_version: AbiVersionFn =
                unsafe { core::mem::transmute(symbol(shim_handle, b"blastit_php_abi_version\0")?) };
            let minor_fn: MinorFn =
                unsafe { core::mem::transmute(symbol(shim_handle, b"blastit_php_minor\0")?) };
            let worker_init: WorkerInitFn =
                unsafe { core::mem::transmute(symbol(shim_handle, b"blastit_php_worker_init\0")?) };
            let worker_shutdown: WorkerShutdownFn = unsafe {
                core::mem::transmute(symbol(shim_handle, b"blastit_php_worker_shutdown\0")?)
            };
            let request: RequestFn =
                unsafe { core::mem::transmute(symbol(shim_handle, b"blastit_php_request\0")?) };
            // SAFETY: `php_version` is a data symbol returning a static,
            // NUL-terminated string owned by libphp.
            let version_fn: unsafe extern "C" fn() -> *const c_char =
                unsafe { core::mem::transmute(symbol(libphp_handle, b"php_version\0")?) };
            // SAFETY: called once; a null result is refused below rather than
            // handed to `CStr::from_ptr` (which would be undefined behaviour).
            let version_ptr = unsafe { version_fn() };
            if version_ptr.is_null() {
                return Err(LoadError::NoVersion);
            }
            // SAFETY: non-null, and `php_version()`'s contract is a static,
            // NUL-terminated string.
            let host_version = unsafe { CStr::from_ptr(version_ptr) }
                .to_string_lossy()
                .into_owned();

            // SAFETY: called once, before anything else in the shim.
            let abi = unsafe { abi_version() };
            if abi != crate::ABI_VERSION {
                return Err(LoadError::AbiVersion { found: abi });
            }
            let code = unsafe { minor_fn() };
            let minor = SUPPORTED_MINORS
                .iter()
                .copied()
                .find(|minor| minor.accepts(code))
                .ok_or_else(|| LoadError::MinorUnsupported {
                    found: code,
                    path: shim_path.clone(),
                })?;
            if Minor::parse(&host_version) != Some(minor) {
                return Err(LoadError::MinorMismatch {
                    shim: minor,
                    host: host_version,
                });
            }
            Ok(Self {
                libphp: libphp_handle,
                shim: shim_handle,
                host_version,
                minor,
                abi_version,
                worker_init,
                worker_shutdown,
                request,
                initialized: false,
                startup_attempted: false,
            })
        })();

        if loaded.is_err() {
            // SAFETY: both handles are live, nothing ran on them (module
            // startup is what would make unloading unsafe), and they are
            // closed exactly once.
            unsafe {
                dlclose(shim_handle);
                dlclose(libphp_handle);
            }
        }
        loaded
    }

    /// `blastit_php_abi_version()`: always [`crate::ABI_VERSION`] after a
    /// successful [`Loaded::load`].
    #[must_use]
    pub fn abi_version(&self) -> u32 {
        // SAFETY: resolved from the live shim handle by `load`.
        unsafe { (self.abi_version)() }
    }

    /// The shim's PHP minor, cross-checked against the host's `php_version()`.
    #[must_use]
    pub fn minor(&self) -> Minor {
        self.minor
    }

    /// The host `libphp`'s version string (`"8.4.27"`).
    #[must_use]
    pub fn php_version(&self) -> &str {
        &self.host_version
    }

    /// Runs `php_module_startup` with this worker's INI lockdown.
    ///
    /// `ini` is the blob [`crate::IniPolicy`] rendered; `view_tmp` is the
    /// view's private scratch directory. Both must outlive the call (they do:
    /// they are copied into C strings here).
    ///
    /// # Errors
    ///
    /// [`LoadError::Init`] when the shim refuses, or [`LoadError::Nul`] when a
    /// value could never be a C string.
    pub fn worker_init(&mut self, ini: &str, view_tmp: &Path) -> Result<(), LoadError> {
        let ini = CString::new(ini).map_err(|_| LoadError::Nul {
            what: "the INI blob",
        })?;
        let tmp =
            CString::new(view_tmp.to_string_lossy().as_bytes()).map_err(|_| LoadError::Nul {
                what: "the view temp path",
            })?;
        let config = ConfigV1 {
            ini: ini.as_ptr(),
            view_tmp: tmp.as_ptr(),
        };
        // SAFETY: both C strings outlive the call; `config` borrows them.
        //
        // The flag goes up *before* the call: a `-2` means the module startup
        // ran and failed, and the shim has already called `sapi_startup`.
        self.startup_attempted = true;
        let code = unsafe { (self.worker_init)(core::ptr::from_ref(&config)) };
        if code != 0 {
            return Err(LoadError::Init { code });
        }
        self.initialized = true;
        Ok(())
    }

    /// `php_module_shutdown` + `sapi_shutdown`. Idempotent; the worker calls it
    /// on the way out, never between requests.
    pub fn worker_shutdown(&mut self) {
        if !self.initialized {
            return;
        }
        self.initialized = false;
        // SAFETY: called once after a successful `worker_init`, as the header
        // documents.
        unsafe { (self.worker_shutdown)() };
    }

    /// Runs one request. The shim traps `exit()`/`die()`/fatals itself and
    /// reports the outcome in the returned [`ResultV1`]; a returned `Err` is a
    /// caller bug (no `worker_init`, a nested call, a null argument).
    ///
    /// # Errors
    ///
    /// [`LoadError::Request`] when the shim's own sanity checks refuse.
    pub fn request(&self, req: &RequestV1, sink: &mut dyn Sink) -> Result<ResultV1, LoadError> {
        // The callbacks receive a *thin* pointer to this cell: a `&mut dyn
        // Sink` is fat, and `SinkV1::ctx` is a `void *`, so the fat pointer
        // lives in a field and only the cell's address crosses the boundary.
        let mut cell = SinkCtx {
            sink: core::ptr::from_mut(sink),
        };
        let ctx = core::ptr::from_mut(&mut cell).cast::<c_void>();
        let vtable = SinkV1 {
            ctx,
            body: Some(sink_body),
            head: Some(sink_head),
            log: Some(sink_log),
        };
        let mut out = ResultV1 {
            outcome: OUTCOME_OK,
            status: 500,
            contained: 0,
            reserved: 0,
            cpu_ns: 0,
            body_bytes: 0,
        };
        // SAFETY: `req` borrows live buffers for this call, `vtable` points at
        // the live `cell`, and `out` is a local. `cell` is borrowed for the
        // length of the call and is not touched again afterwards.
        let code = unsafe {
            (self.request)(
                req,
                core::ptr::from_ref(&vtable),
                core::ptr::from_mut(&mut out),
            )
        };
        if code != 0 {
            return Err(LoadError::Request { code });
        }
        Ok(out)
    }
}

impl Drop for Loaded {
    fn drop(&mut self) {
        // Once a module startup has been *attempted*, unloading `libphp` would
        // run PHP's own library destructors and every module's unload path on
        // a table that may be half-built (the shim's `-2`). The worker exits
        // instead; a leak at process exit is the safe side of that choice.
        // Only a load that failed before any startup is closed here.
        if self.startup_attempted {
            return;
        }
        // SAFETY: only reached when `load` succeeded and `worker_init` was
        // never entered: the handles are live and closed exactly once.
        if !self.shim.is_null() {
            unsafe { dlclose(self.shim) };
        }
        if !self.libphp.is_null() {
            unsafe { dlclose(self.libphp) };
        }
    }
}

/// The last `dlerror()` message, or a placeholder when there is none.
fn dl_error() -> String {
    // SAFETY: `dlerror()` returns a NUL-terminated string owned by libdl, valid
    // until the next dl* call on this thread; it is copied immediately.
    let raw = unsafe { dlerror() };
    if raw.is_null() {
        return "no dlerror() message".to_owned();
    }
    // SAFETY: as above — a non-null `dlerror()` result is NUL-terminated.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}

/// A path as a C string, naming the field in the refusal.
fn c_path(path: &Path, what: &'static str) -> Result<CString, LoadError> {
    CString::new(path.to_string_lossy().as_bytes()).map_err(|_| LoadError::Nul { what })
}

/// Looks one NUL-terminated symbol up in `handle`.
///
/// The returned pointer is a raw `void *`. Every entry point this loader
/// resolves is a function (or, for `php_version`, a function returning a
/// pointer), so the pointer size is the same for all of them; the call sites
/// `transmute` the pointer to the signature `c/include/blastit_php_abi_v1.h`
/// declares. That conversion cannot check a *signature* — a same-size function
/// with different arguments would transmute silently — so the declarations are
/// guarded where they are written: `src/abi.rs` mirrors the header field by
/// field, the shim's `_Static_assert`s pin the struct layout at compile time,
/// and `tests/critical_shim_contract.rs` keeps the mirror and the header in
/// step on every `make check`.
///
/// # Safety
///
/// `handle` must be a live `dlopen` handle and `name` NUL-terminated.
unsafe fn symbol(handle: *mut c_void, name: &'static [u8]) -> Result<*mut c_void, LoadError> {
    debug_assert_eq!(name.last(), Some(&0));
    // SAFETY: the caller guarantees a live handle and a NUL-terminated name.
    let ptr = unsafe { dlsym(handle, name.as_ptr().cast::<c_char>()) };
    if ptr.is_null() {
        let name = std::str::from_utf8(name).unwrap_or("?");
        return Err(LoadError::Symbol {
            name: name.trim_end_matches('\0').to_owned(),
            detail: dl_error(),
        });
    }
    Ok(ptr)
}

/// The one thing `SinkV1::ctx` points at: the borrowed sink, whose fat pointer
/// lives in a field so the cell itself has a thin address.
struct SinkCtx<'a> {
    sink: *mut (dyn Sink + 'a),
}

/// Runs `f` with the sink behind a callback's `ctx`.
///
/// The reference handed to `f` has a **fresh, local** lifetime: it is created
/// by this function's own borrow of the cell and can only be used inside `f`,
/// so a callback cannot smuggle a `&mut dyn Sink` out of the C call by choosing
/// a lifetime at the call site.
///
/// # Safety
///
/// `ctx` must be the `SinkV1::ctx` [`Loaded::request`] installed, on this
/// thread, for the duration of the one `blastit_php_request` call.
unsafe fn with_sink<R>(ctx: *mut c_void, f: impl FnOnce(&mut dyn Sink) -> R) -> R {
    // SAFETY: the caller guarantees `ctx` points at a live `SinkCtx` whose sink
    // outlives this call; the borrow cannot escape `f`.
    let cell: &mut SinkCtx<'_> = unsafe { &mut *ctx.cast::<SinkCtx<'_>>() };
    // SAFETY: `cell.sink` was built from a `&mut dyn Sink` that outlives the C
    // call, and nothing else dereferences it while `f` runs.
    f(unsafe { &mut *cell.sink })
}

/// `blastit_php_sink_v1.head`: one header, or the end-of-head marker.
///
/// # Safety
///
/// The C ABI guarantees `ctx` is the worker's pointer and that `name`/`value`
/// are readable for their lengths, or NULL.
unsafe extern "C" fn sink_head(
    ctx: *mut c_void,
    status: i32,
    name: *const c_char,
    name_len: usize,
    value: *const c_char,
    value_len: usize,
) -> i32 {
    // A panic must never unwind into C: the shim's `zend_try` frames are not
    // Rust frames. A panic becomes "abort the output", the C side's failure.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: `ctx` is the live sink cell `Loaded::request` installed, for
        // this call only; the slices borrow the shim's buffers for the same
        // call.
        unsafe {
            with_sink(ctx, |sink| {
                let name = bytes_or_empty(name, name_len);
                let value = bytes_or_empty(value, value_len).unwrap_or(&[]);
                i32::from(!sink.head(status, name, value))
            })
        }
    }))
    .unwrap_or(1)
}

/// `blastit_php_sink_v1.body`: one response body chunk.
///
/// # Safety
///
/// The C ABI guarantees `data` is readable for `len` bytes, or NULL when empty.
unsafe extern "C" fn sink_body(ctx: *mut c_void, data: *const u8, len: usize) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // An empty chunk is not an error, and never becomes a slice from a
        // null pointer (`from_raw_parts(NULL, 0)` is undefined behaviour even
        // for a zero length).
        if data.is_null() {
            return i32::from(len != 0);
        }
        // SAFETY: as `sink_head`; `data` is non-null and readable for `len`.
        unsafe {
            with_sink(ctx, |sink| {
                let data = core::slice::from_raw_parts(data, len);
                i32::from(!sink.body(data))
            })
        }
    }))
    .unwrap_or(1)
}

/// `blastit_php_sink_v1.log`: one bounded log line.
///
/// # Safety
///
/// The C ABI guarantees `line` is readable for `len` bytes, or NULL.
unsafe extern "C" fn sink_log(ctx: *mut c_void, level: i32, line: *const c_char, len: usize) {
    // Losing a diagnostic line must never fail the request, so a panic here is
    // swallowed as well.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if line.is_null() || len == 0 {
            return;
        }
        // SAFETY: as `sink_head`.
        unsafe {
            with_sink(ctx, |sink| {
                let bytes = core::slice::from_raw_parts(line.cast::<u8>(), len);
                sink.log(level, &String::from_utf8_lossy(bytes));
            });
        }
    }));
}

/// A `(ptr, len)` pair as a slice, or `None` for the end-of-head marker.
///
/// # Safety
///
/// `ptr` must be readable for `len` bytes when non-NULL.
unsafe fn bytes_or_empty<'a>(ptr: *const c_char, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees readability. A zero-length slice from a
    // non-null pointer is valid.
    Some(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len) })
}

/// The outcome a [`ResultV1`] reports, for callers that only need the status of
/// the request rather than which end-of-script path it took.
#[must_use]
pub fn bailed_out(result: &ResultV1) -> bool {
    result.outcome == OUTCOME_BAILOUT && result.contained != 0
}

/// `OUTCOME_OK` is 0 and `OUTCOME_BAILOUT` is 1; this is the one place that
/// compares them, so a stray raw `1` in a caller is a compile error waiting to
/// be caught by review rather than a silent miscount. Kept private to the
/// mapping above.
const _: () = assert!(OUTCOME_OK == 0 && OUTCOME_BAILOUT == 1);

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in a fresh temporary directory; removed with the directory.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str, contents: &[u8]) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "blastit-loader-{}-{}",
                std::process::id(),
                name
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let path = dir.join(name);
            std::fs::write(&path, contents).expect("write");
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    #[test]
    fn a_descriptor_is_named_through_proc() {
        assert_eq!(
            SharedObject::Fd(7).dlopen_path(),
            PathBuf::from("/proc/self/fd/7")
        );
        assert_eq!(
            SharedObject::Path(PathBuf::from("/usr/lib/libphp.so")).dlopen_path(),
            PathBuf::from("/usr/lib/libphp.so")
        );
    }

    #[test]
    fn a_file_that_is_not_a_library_is_a_typed_refusal() {
        let fake = TempFile::new("libphp.so", b"this is not an ELF shared object");
        let err = Loaded::load(
            SharedObject::Path(fake.0.clone()),
            SharedObject::Path(PathBuf::from("/nonexistent/shim.so")),
        )
        .err()
        .expect("a text file is not a library");
        assert!(
            matches!(&err, LoadError::NotALibrary { path, .. } if path == &fake.0),
            "{err}"
        );
        // The message names the file and the fix, and never a secret.
        let text = err.to_string();
        assert!(text.contains("libphp"), "{text}");
        assert!(text.contains("BLASTIT_LIBPHP"), "{text}");
    }

    #[test]
    fn a_missing_library_is_a_refusal_not_a_panic() {
        let err = Loaded::load(
            SharedObject::Path(PathBuf::from("/nonexistent/libphp.so")),
            SharedObject::Path(PathBuf::from("/nonexistent/shim.so")),
        )
        .err()
        .expect("a missing file cannot load");
        assert!(matches!(err, LoadError::NotALibrary { .. }), "{err}");
    }

    #[test]
    fn a_path_holding_a_nul_is_refused_before_dlopen() {
        use std::os::unix::ffi::OsStrExt;
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/usr/lib/lib\0php.so"));
        let err = Loaded::load(
            SharedObject::Path(path),
            SharedObject::Path(PathBuf::from("/nonexistent/shim.so")),
        )
        .err()
        .expect("a NUL cannot be a C path");
        assert!(matches!(err, LoadError::Nul { .. }), "{err}");
    }

    #[test]
    fn the_abi_and_minor_messages_name_the_fix() {
        let abi = LoadError::AbiVersion { found: 2 }.to_string();
        assert!(abi.contains("version 2"), "{abi}");
        let mismatch = LoadError::MinorMismatch {
            shim: Minor::Php84,
            host: "8.3.6".to_owned(),
        }
        .to_string();
        assert!(mismatch.contains("PHP 8.4"), "{mismatch}");
        assert!(mismatch.contains("8.3.6"), "{mismatch}");
        assert!(mismatch.contains("libblastit_php84"), "{mismatch}");
    }
}
