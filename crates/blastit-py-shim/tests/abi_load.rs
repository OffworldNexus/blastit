//! The self-contained load probe: `dlopen` a built shim, read its vtable, and
//! prove `abi_version == 1` and `py_minor` matches the artifact's own filename
//! — **without booting `blastit`** (design §1.2, `MB.42.2.1.2`).
//!
//! This is a lab test, not a `check` step: it needs an artifact `make py-sapi`
//! produced, so it is `#[ignore]`d and runs with an explicit path:
//!
//! ```text
//! BLASTIT_PY_SAPI_SHIM=target/py-sapi/libblastit_py313-x86_64-gnu.so \
//! BLASTIT_PY_SAPI_LIBPYTHON=<the libpython it was built against> \
//!     cargo test -p blastit-py-shim --features extension-module --test abi_load -- --ignored
//! ```
//!
//! With a `libpython` named, the shim is loaded `RTLD_NOW` after it (exactly
//! stage 1's order), so **every** `Py*` reference the shim carries has to
//! resolve against the real CPython: a typo'd or removed symbol fails this
//! build rather than stage 1's `dlopen`. Without one, `RTLD_LAZY` is the only
//! mode that can work — pyo3 reaches some of CPython through data relocations
//! (`_Py_Dealloc`) that nothing defers — and the probe still proves the three
//! exports and the vtable head.
//!
//! With no `BLASTIT_PY_SAPI_SHIM` the test prints why and passes, so a bare host
//! keeps `make fast-check` green; `make py-sapi` always names the artifact, so
//! the build fails there rather than in a later slice.

#![allow(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::{CString, c_char, c_int, c_void};
use std::path::Path;

/// `RTLD_LAZY` (`dlfcn.h`): resolve a function's target on first call.
const RTLD_LAZY: c_int = 1;
/// `RTLD_NOW` (`dlfcn.h`): every symbol must resolve at load time.
const RTLD_NOW: c_int = 2;
/// `RTLD_LOCAL` (`dlfcn.h`): the shim's symbols stay private, as in stage 1.
const RTLD_LOCAL: c_int = 0;
/// `RTLD_GLOBAL` (`dlfcn.h`): the shim's `Py_*` symbols resolve here.
const RTLD_GLOBAL: c_int = 0x100;

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

/// The first two fields of `BlastitPyVtableV1`, which is all this probe reads:
/// the rest of the ten-slot table is not needed to prove the frozen head.
#[repr(C)]
struct VtableHead {
    abi_version: extern "C" fn() -> u32,
    py_minor: extern "C" fn() -> u32,
}

#[test]
#[ignore = "lab probe: name the artifact with BLASTIT_PY_SAPI_SHIM=<path> and pass --ignored"]
fn the_built_shim_speaks_the_frozen_abi() {
    let Some(path) = std::env::var_os("BLASTIT_PY_SAPI_SHIM") else {
        eprintln!(
            "skipped: BLASTIT_PY_SAPI_SHIM is unset, so there is no built shim to load; \
             run `make py-sapi` (it names each artifact) or set it to a \
             target/py-sapi/libblastit_py3{{13,14}}-<arch>-<libc>.so"
        );
        return;
    };
    let path = std::path::PathBuf::from(path);
    assert!(path.is_file(), "{} is not a file", path.display());
    let expected = minor_from_filename(&path);

    // Stage 1's order, exactly: the interpreter first (`RTLD_GLOBAL`), the shim
    // second. pyo3 reaches some of CPython through data relocations
    // (`_Py_Dealloc`), which no `RTLD_LAZY` can defer, so the library that
    // provides them has to be loaded first here too.
    // Held open for the rest of the process: nothing clobbers it, and the probe
    // never unloads the interpreter it proved against.
    let _library = match std::env::var_os("BLASTIT_PY_SAPI_LIBPYTHON") {
        Some(library) => {
            let library = std::path::PathBuf::from(library);
            assert!(
                library.is_file(),
                "BLASTIT_PY_SAPI_LIBPYTHON ({}) is not a file",
                library.display()
            );
            let name = CString::new(library.as_os_str().as_encoded_bytes())
                .expect("a library path without a NUL byte");
            // SAFETY: `name` is NUL-terminated and outlives the call.
            let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
            assert!(
                !handle.is_null(),
                "dlopen({}) failed: {}",
                library.display(),
                last_error()
            );
            Some(handle)
        }
        None => None,
    };

    let name =
        CString::new(path.as_os_str().as_encoded_bytes()).expect("a shim path without a NUL byte");
    // `RTLD_NOW` once libpython is loaded: every `Py*` reference must resolve
    // against the interpreter this shim was built for. `RTLD_LAZY` only for a
    // hand-run without one.
    let resolved = if _library.is_some() {
        RTLD_NOW | RTLD_LOCAL
    } else {
        RTLD_LAZY
    };
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let handle = unsafe { dlopen(name.as_ptr(), resolved) };
    assert!(
        !handle.is_null(),
        "dlopen({}) failed: {} (name the CPython the shim was built for with \
         BLASTIT_PY_SAPI_LIBPYTHON=<libpython .so>)",
        path.display(),
        last_error()
    );

    // SAFETY: the names are `c"…"` literals and `handle` came from `dlopen`.
    let table_symbol = unsafe { dlsym(handle, c"blastit_py_abi_v1".as_ptr()) };
    let version_symbol = unsafe { dlsym(handle, c"blastit_py_abi_version".as_ptr()) };
    let minor_symbol = unsafe { dlsym(handle, c"blastit_py_minor".as_ptr()) };
    for (name, symbol) in [
        ("blastit_py_abi_v1", table_symbol),
        ("blastit_py_abi_version", version_symbol),
        ("blastit_py_minor", minor_symbol),
    ] {
        assert!(
            !symbol.is_null(),
            "{} exports no `{name}`: {}",
            path.display(),
            last_error()
        );
    }
    // SAFETY: the shim's own documented exports, whose signatures the
    // `blastit-py-sapi` declarations freeze.
    let entry: extern "C" fn() -> *const VtableHead = unsafe { std::mem::transmute(table_symbol) };
    let abi_export: extern "C" fn() -> u32 = unsafe { std::mem::transmute(version_symbol) };
    let minor_export: extern "C" fn() -> u32 = unsafe { std::mem::transmute(minor_symbol) };
    let table = entry();
    assert!(
        !table.is_null(),
        "{} returned a NULL vtable",
        path.display()
    );
    // SAFETY: a non-null table is the shim's static, valid while it is loaded.
    let table = unsafe { &*table };
    let abi = (table.abi_version)();
    let minor = (table.py_minor)();
    assert_eq!(
        abi_export(),
        abi,
        "{}: blastit_py_abi_version() disagrees with the vtable",
        path.display()
    );
    assert_eq!(
        minor_export(),
        minor,
        "{}: blastit_py_minor() disagrees with the vtable",
        path.display()
    );

    // Both handles are left open on purpose: this process is a probe, and
    // `dlclose`ing CPython while its statics still carry destructors is a
    // teardown crash that would say nothing about the ABI. The test process
    // exits immediately after this line, and the kernel reclaims the mappings.

    assert_eq!(
        abi,
        1,
        "{} speaks blastit_py_abi version {abi}",
        path.display()
    );
    assert_eq!(
        minor,
        expected,
        "{} reports CPython minor {minor} but its filename says {expected}",
        path.display()
    );
    assert!(
        minor == 313 || minor == 314,
        "{} reports an unsupported minor {minor}",
        path.display()
    );
    eprintln!(
        "ok   {}: blastit_py_abi_v1 version {abi}, CPython minor {minor}",
        path.display()
    );
}

/// `libblastit_py313-x86_64-gnu.so` -> `313`.
fn minor_from_filename(path: &Path) -> u32 {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|name| name.strip_prefix("libblastit_py"))
        .and_then(|rest| rest.split('-').next())
        .and_then(|digits| digits.parse().ok())
        .unwrap_or_else(|| {
            panic!(
                "{} is not libblastit_py<digits>-<arch>-<libc>.so",
                path.display()
            )
        })
}

/// The last `dlerror()` message, for a refusal that names the cause.
fn last_error() -> String {
    // SAFETY: `dlerror()` returns a NUL-terminated string owned by libdl.
    let raw = unsafe { dlerror() };
    if raw.is_null() {
        return "no dlerror() message".to_owned();
    }
    // SAFETY: as above.
    unsafe { std::ffi::CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}
