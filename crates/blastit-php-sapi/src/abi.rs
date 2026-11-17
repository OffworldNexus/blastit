//! The `blastit_php_abi_v1` surface, mirrored from `c/include/blastit_php_abi_v1.h`.
//!
//! This is the **only** module in this crate whose contents can be `unsafe`
//! (the crate root says `deny(unsafe_code)` and allows it here): naming an
//! `extern "C"` function pointer is the whole hand-off to C. No `unsafe` block
//! exists in this crate — the worker that calls through these pointers
//! (`MB.3.13`) owns the `dlopen` handle and every precondition, and the probe in
//! `src/probe/ffi.rs` is the one place that calls them here.
//!
//! Layout drift is a `segfault` at `exit()` time, so it is checked twice: the
//! `_Static_assert`s at the top of `c/blastit_php_sapi.c` (compile time, in the
//! container) and `tests/critical_shim_contract.rs` (Rust side, every
//! `make check`).

use core::ffi::{c_char, c_void};

/// `BLASTIT_PHP_ABI_VERSION`: the version this crate speaks.
pub const ABI_VERSION: u32 = 1;

/// `BLASTIT_PHP_ABI_MAJOR`: every shim speaks the PHP 8 line.
pub const ABI_MAJOR: u32 = 8;

/// `BLASTIT_PHP_OUTCOME_OK`: the script ran to completion.
pub const OUTCOME_OK: i32 = 0;

/// `BLASTIT_PHP_OUTCOME_BAILOUT`: `exit()`, `die()` or a fatal was trapped.
pub const OUTCOME_BAILOUT: i32 = 1;

/// `blastit_php_sink_v1.body`: one response body chunk.
pub type BodyFn = unsafe extern "C" fn(ctx: *mut c_void, data: *const u8, len: usize) -> i32;

/// `blastit_php_sink_v1.head`: one header, or the end-of-head marker
/// (`name == NULL && name_len == 0`) that always carries the status.
pub type HeadFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    status: i32,
    name: *const c_char,
    name_len: usize,
    value: *const c_char,
    value_len: usize,
) -> i32;

/// `blastit_php_sink_v1.log`: one bounded log line.
pub type LogFn =
    unsafe extern "C" fn(ctx: *mut c_void, level: i32, line: *const c_char, len: usize);

/// `blastit_php_sink_v1`: what the worker implements for the shim.
#[repr(C)]
pub struct SinkV1 {
    /// Opaque worker pointer handed back to every callback.
    pub ctx: *mut c_void,
    /// A response body chunk.
    pub body: Option<BodyFn>,
    /// One response header, then the end-of-head marker.
    pub head: Option<HeadFn>,
    /// One bounded log line.
    pub log: Option<LogFn>,
}

/// `blastit_php_server_var_v1`: one already-validated `$_SERVER` entry.
#[repr(C)]
pub struct ServerVarV1 {
    /// The name; `[A-Z0-9_]`, not necessarily NUL-terminated.
    pub name: *const c_char,
    /// The name length in bytes.
    pub name_len: usize,
    /// The value; never NUL-terminated.
    pub value: *const c_char,
    /// The value length in bytes.
    pub value_len: usize,
}

/// `blastit_php_request_v1`: one request. Every pointer borrows for the call.
#[repr(C)]
pub struct RequestV1 {
    /// Absolute `SCRIPT_FILENAME`, pinned by the worker.
    pub entry: *const c_char,
    /// The application root.
    pub document_root: *const c_char,
    /// The request path as routed.
    pub script_name: *const c_char,
    /// The request URI.
    pub request_uri: *const c_char,
    /// `NULL` when absent.
    pub query_string: *const c_char,
    /// The method token.
    pub request_method: *const c_char,
    /// `NULL` when there is no body.
    pub content_type: *const c_char,
    /// 0 when there is no body.
    pub content_length: i64,
    /// The `Cookie` header value, or `NULL`.
    pub cookie: *const c_char,
    /// The validated server variables, or `NULL`.
    pub server_vars: *const ServerVarV1,
    /// How many entries `server_vars` holds.
    pub server_var_count: usize,
    /// The already-read body, or `NULL`.
    pub body: *const u8,
    /// How many body bytes `body` holds.
    pub body_len: usize,
}

/// `blastit_php_config_v1`: fixed for the worker's lifetime.
#[repr(C)]
pub struct ConfigV1 {
    /// The NUL-terminated, newline-separated INI lockdown.
    pub ini: *const c_char,
    /// The view's private temp directory (informational).
    pub view_tmp: *const c_char,
}

/// `blastit_php_result_v1`: what one request cost and produced.
#[repr(C)]
pub struct ResultV1 {
    /// [`OUTCOME_OK`] or [`OUTCOME_BAILOUT`].
    pub outcome: i32,
    /// The response status.
    pub status: i32,
    /// 1 when a `zend_bailout()` was trapped.
    pub contained: i32,
    /// Padding, kept explicit so the layout is unambiguous on both sides.
    pub reserved: i32,
    /// `CLOCK_PROCESS_CPUTIME_ID` delta, in nanoseconds.
    pub cpu_ns: u64,
    /// Body bytes handed to the sink.
    pub body_bytes: u64,
}

/// `uint32_t blastit_php_abi_version(void)`.
pub type AbiVersionFn = unsafe extern "C" fn() -> u32;

/// `uint32_t blastit_php_minor(void)`.
pub type MinorFn = unsafe extern "C" fn() -> u32;

/// `int32_t blastit_php_worker_init(const blastit_php_config_v1 *)`.
pub type WorkerInitFn = unsafe extern "C" fn(cfg: *const ConfigV1) -> i32;

/// `void blastit_php_worker_shutdown(void)`.
pub type WorkerShutdownFn = unsafe extern "C" fn();

/// `int32_t blastit_php_request(const request *, const sink *, result *)`.
pub type RequestFn =
    unsafe extern "C" fn(req: *const RequestV1, sink: *const SinkV1, out: *mut ResultV1) -> i32;

/// `blastit_php_file_sink_install`'s `included` callback: one included file.
///
/// `path` is non-`NULL`, **not** NUL-terminated and readable for `len` bytes; a
/// zero length is never passed. The return value is ignored by the shim.
pub type FileIncludedFn =
    unsafe extern "C" fn(ctx: *mut c_void, path: *const c_char, len: usize) -> i32;

/// `int32_t blastit_php_file_sink_install(void *ctx, included)`.
///
/// **Optional and additive**: a shim that does not export it reports no files,
/// and the worker treats its absence as "no report" for the process lifetime
/// rather than as a load failure — which is why `BLASTIT_PHP_ABI_VERSION` is
/// still 1. `None` uninstalls. Resolved with `dlsym` on the *shim's* handle,
/// never `RTLD_DEFAULT`: the shim is opened `RTLD_LOCAL`, so only that handle
/// sees it.
pub type FileSinkInstallFn =
    unsafe extern "C" fn(ctx: *mut c_void, included: Option<FileIncludedFn>) -> i32;

// The C side asserts the same numbers with `_Static_assert`; a change to either
// file that moves a field fails one build or the other, never a request.
const _: () = assert!(core::mem::size_of::<ResultV1>() == 32);
const _: () = assert!(core::mem::offset_of!(ResultV1, cpu_ns) == 16);
const _: () = assert!(core::mem::offset_of!(ResultV1, body_bytes) == 24);
const _: () = assert!(core::mem::size_of::<SinkV1>() == 4 * core::mem::size_of::<usize>());
const _: () = assert!(core::mem::offset_of!(SinkV1, head) == 2 * core::mem::size_of::<usize>());
const _: () = assert!(core::mem::offset_of!(RequestV1, content_length) == 7 * 8);
const _: () = assert!(core::mem::offset_of!(RequestV1, server_vars) == 9 * 8);
const _: () = assert!(core::mem::offset_of!(RequestV1, body_len) == 12 * 8);
