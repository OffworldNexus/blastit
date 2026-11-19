#![deny(unsafe_code)]
#![deny(missing_docs)]

//! One per-minor CPython shim: the `blastit_py_abi_v1` surface a universal
//! `blastit` `dlopen`s (design `universal-distribution.md` §1.1, §1.2).
//!
//! # What this artifact is
//!
//! `make py-sapi` builds this crate with `--features extension-module` once per
//! supported CPython, and names each result
//! `libblastit_py3{N}-<arch>-<libc>.so`. The shipped `blastit` links no
//! `libpython`: stage 1 `dlopen`s the host's `libpython` with `RTLD_GLOBAL` and
//! then this file with `RTLD_LOCAL`, resolves `blastit_py_abi_v1()`, and from
//! then on the interpreter is behind ten function pointers. Every `Py_*` call
//! this crate makes is therefore an **undefined dynamic** symbol in the
//! artifact — `nm -uD` shows `Py_InitializeEx` — and the `DT_NEEDED` on
//! `libpython` belongs to this shim, never to `blastit`. That distinction is
//! what `scripts/check-portable-elf.sh` and the `make py-sapi` assertions check.
//!
//! # The exported symbols are three, deliberately
//!
//! * `blastit_py_abi_version()` — `ABI_VERSION` (`1`);
//! * `blastit_py_minor()` — the build's `PYO3_PYTHON` minor (`313`/`314`),
//!   recorded by `build.rs` and never a literal;
//! * `blastit_py_abi_v1()` — the static vtable.
//!
//! The other seven surfaces (`before_fork`, `after_fork_parent`,
//! `after_fork_child`, `init_paths`, `preload`, `check_syntax`, `serve_view`)
//! and `free_buf` are reached **through the table**, which is what keeps the
//! dynamic symbol namespace of a `dlopen`ed process small and the ABI frozen:
//! one entry point, one shape.
//!
//! # Never unwind across FFI
//!
//! `extern "C"` is not unwind-safe: a Rust panic crossing the boundary is
//! undefined behaviour, and a process that aborts inside the interpreter's
//! parent takes every forked view with it. So **every** boundary — including
//! `blastit_py_abi_version` — runs under `std::panic::catch_unwind`, and a
//! panic becomes a refusal (`-1`, or `70` where the child sequence's code is the
//! caller's) plus one JSON sentence on the error channel. Nothing here may
//! `unwrap()` caller input, and nothing here may `dlclose`, `abort` or exit: the
//! process's fate belongs to the composition root.
//!
//! # Objects are opaque
//!
//! A preloaded interpreter travels as `PyZygoteOpaque`: a pointer to a box this
//! crate owns, whose contents (`Py<PyAny>`) never appear in a public signature.
//! A caller that loses the pointer leaks the parent; it can never observe or
//! fabricate one.
//!
//! # Why one `cfg` mentions `test`
//!
//! An `extension-module` build links no `libpython`, so every `Py*` reference in
//! it is an undefined dynamic symbol by design. A *test executable* built in that
//! configuration has no library to resolve them against (that is exactly why
//! pyo3's `extension-module` feature is not meant for `cargo test`), so the
//! interpreter-backed half — the `surface` module, and the four modules it
//! owns — is compiled out of that one build shape. The pure half (the JSON
//! codec and the buffer boundary) keeps its tests everywhere, the artifact
//! itself never sees `cfg(test)`, and the default
//! `cargo test -p blastit-py-shim` runs all of it with the interpreter linked.

mod buffer;
mod request;

#[cfg(not(all(test, feature = "extension-module")))]
mod boot;
#[cfg(not(all(test, feature = "extension-module")))]
mod env;
#[cfg(not(all(test, feature = "extension-module")))]
mod serve;
#[cfg(not(all(test, feature = "extension-module")))]
mod syntax;

/// The frozen C surface: three exports, one static vtable, ten boundaries.
///
/// Compiled into every artifact; absent only from a test build that enabled
/// `extension-module` (see the crate documentation), where there is no
/// `libpython` to link the `Py*` references against.
#[cfg(not(all(test, feature = "extension-module")))]
mod surface {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use blastit_py_sapi::ABI_VERSION;
    use blastit_py_sapi::{BlastitPyVtableV1, PyBufV1, PyZygoteOpaque};

    use crate::buffer::{self, free, guarded, input, write_error, write_json};

    /// The CPython minor this shim was built for (`313`/`314`).
    ///
    /// `build.rs` records the minor of the interpreter `PYO3_PYTHON` named, so
    /// the same source produces a truthful `py_minor` for every artifact
    /// `make py-sapi` writes.
    fn built_minor() -> u32 {
        env!("BLASTIT_PY_MINOR").parse().unwrap_or(0)
    }

    /// The ABI this shim implements: `ABI_VERSION` (`1`).
    ///
    /// # Safety
    ///
    /// Callable with any arguments (there are none); it never panics.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn blastit_py_abi_version() -> u32 {
        catch_unwind(|| ABI_VERSION).unwrap_or(0)
    }

    /// The CPython minor this shim was built for.
    ///
    /// # Safety
    ///
    /// Callable with any arguments (there are none); it never panics.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn blastit_py_minor() -> u32 {
        catch_unwind(built_minor).unwrap_or(0)
    }

    /// The frozen vtable, static for the process's lifetime.
    ///
    /// # Safety
    ///
    /// The returned pointer is valid for as long as this shim is loaded, and
    /// points at an immutable table: a caller may read it, never write it. A
    /// panic (impossible in practice) is a NULL rather than an unwind.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "C" fn blastit_py_abi_v1() -> *const BlastitPyVtableV1 {
        catch_unwind(|| &raw const VTABLE).unwrap_or(std::ptr::null())
    }

    /// `PyOS_BeforeFork`, called by the caller with the GIL held (§1.2.1).
    #[allow(unsafe_code)]
    extern "C" fn before_fork() {
        let _ = catch_unwind(|| {
            // SAFETY: CPython's own fork protocol, called with the GIL held
            // exactly as `forking.rs` documents.
            unsafe { pyo3::ffi::PyOS_BeforeFork() };
        });
    }

    /// `PyOS_AfterFork_Parent`, the matching half in the parent.
    #[allow(unsafe_code)]
    extern "C" fn after_fork_parent() {
        let _ = catch_unwind(|| {
            // SAFETY: as `before_fork`.
            unsafe { pyo3::ffi::PyOS_AfterFork_Parent() };
        });
    }

    /// `PyOS_AfterFork_Child`, the matching half in the child.
    #[allow(unsafe_code)]
    extern "C" fn after_fork_child() {
        let _ = catch_unwind(|| {
            // SAFETY: as `before_fork`; the child holds the inherited GIL.
            unsafe { pyo3::ffi::PyOS_AfterFork_Child() };
        });
    }

    /// `init_paths`: `Py_InitializeEx(0)`, the import path, the minor check and
    /// the two path sets (design §1.2).
    extern "C" fn init_paths(
        req: *const u8,
        req_len: usize,
        out: *mut PyBufV1,
        err: *mut PyBufV1,
    ) -> i32 {
        guarded(err, -1, || {
            if out.is_null() {
                write_error(err, "init_paths: the answer buffer is NULL");
                return -1;
            }
            let parsed =
                match crate::request::parse::<crate::request::InitPaths>(input(req, req_len)) {
                    Ok(parsed) => parsed,
                    Err(message) => {
                        write_error(err, &message);
                        return -1;
                    }
                };
            match crate::env::init_paths(&parsed) {
                Ok(answer) => {
                    write_json(out, &answer);
                    0
                }
                Err(message) => {
                    write_error(err, &message);
                    -1
                }
            }
        })
    }

    /// `preload`: the immortal parent, and the opaque pointer to it (§1.2).
    #[allow(unsafe_code)]
    extern "C" fn preload(
        req: *const u8,
        req_len: usize,
        zygote: *mut *mut PyZygoteOpaque,
        out: *mut PyBufV1,
        err: *mut PyBufV1,
    ) -> i32 {
        guarded(err, -1, || {
            if zygote.is_null() || out.is_null() {
                write_error(
                    err,
                    "preload: the zygote pointer and the answer buffer are both mandatory",
                );
                return -1;
            }
            let parsed = match crate::request::parse::<crate::request::Preload>(input(req, req_len))
            {
                Ok(parsed) => parsed,
                Err(message) => {
                    write_error(err, &message);
                    return -1;
                }
            };
            match crate::boot::preload(&parsed) {
                Ok((state, answer)) => {
                    // SAFETY: the caller passed a writable
                    // `*mut *mut PyZygoteOpaque` (design §1.2), checked non-null
                    // above, and takes ownership of the box, which stays alive
                    // for the process's lifetime.
                    unsafe { *zygote = Box::into_raw(state) as *mut PyZygoteOpaque };
                    write_json(out, &answer);
                    0
                }
                Err(message) => {
                    write_error(err, &message);
                    -1
                }
            }
        })
    }

    /// `check_syntax`: compile the changed file, execute nothing (§1.2).
    ///
    /// The zygote is an identity check only — the compile runs here — but a
    /// null or misaligned pointer is refused exactly as `serve_view` refuses
    /// one.
    #[allow(unsafe_code)]
    extern "C" fn check_syntax(
        zygote: *mut PyZygoteOpaque,
        code: *const u8,
        code_len: usize,
        path: *const u8,
        path_len: usize,
        err: *mut PyBufV1,
    ) -> i32 {
        guarded(err, -1, || {
            if unsafe { zygote_state(zygote) }.is_none() {
                write_error(
                    err,
                    "check_syntax was called without a preloaded interpreter",
                );
                return -1;
            }
            match crate::syntax::check_syntax(input(code, code_len), input(path, path_len)) {
                Ok(()) => 0,
                Err(message) => {
                    write_error(err, &message);
                    -1
                }
            }
        })
    }

    /// `serve_view`: the post-`enter` child entry, returning the child's exit
    /// code (design §1.2: `0` served and shut down cleanly, `1` a refusal, `70`
    /// a panic).
    #[allow(unsafe_code)]
    extern "C" fn serve_view(
        zygote: *mut PyZygoteOpaque,
        req: *const u8,
        req_len: usize,
        err: *mut PyBufV1,
    ) -> i32 {
        guarded(err, buffer::PANIC, || {
            let Some(state) = (unsafe { zygote_state(zygote) }) else {
                write_error(err, "serve_view was called without a preloaded interpreter");
                return buffer::REFUSED;
            };
            let parsed = match crate::request::parse::<crate::request::Serve>(input(req, req_len)) {
                Ok(parsed) => parsed,
                Err(message) => {
                    write_error(err, &message);
                    return buffer::REFUSED;
                }
            };
            match crate::serve::serve_view(state, &parsed) {
                // The channel ended: every request it accepted was served.
                Ok(()) => 0,
                Err(message) => {
                    write_error(err, &message);
                    buffer::REFUSED
                }
            }
        })
    }

    /// `free_buf`: reclaims a buffer this shim allocated.
    extern "C" fn free_buf(buf: PyBufV1) {
        let _ = catch_unwind(AssertUnwindSafe(|| free(buf)));
    }

    /// The boxed zygote a caller's opaque pointer names.
    ///
    /// # Safety
    ///
    /// `zygote` is null, or a pointer [`preload`] wrote and the caller has not
    /// freed: it must be aligned for [`crate::boot::Zygote`] and still name the
    /// box. A misaligned value is refused rather than read.
    #[allow(unsafe_code)]
    unsafe fn zygote_state<'a>(zygote: *mut PyZygoteOpaque) -> Option<&'a crate::boot::Zygote> {
        if zygote.is_null()
            || !(zygote as usize).is_multiple_of(std::mem::align_of::<crate::boot::Zygote>())
        {
            return None;
        }
        // SAFETY: the caller upholds this function's contract, and the only
        // pointer `preload` ever wrote stays alive for the process's lifetime.
        Some(unsafe { &*(zygote as *const crate::boot::Zygote) })
    }

    /// The frozen table: `blastit-py-sapi`'s declaration, filled with this
    /// shim's functions in its declared order.
    static VTABLE: BlastitPyVtableV1 = BlastitPyVtableV1 {
        abi_version: blastit_py_abi_version,
        py_minor: blastit_py_minor,
        before_fork,
        after_fork_parent,
        after_fork_child,
        init_paths,
        preload,
        check_syntax,
        serve_view,
        free_buf,
    };

    #[cfg(test)]
    #[allow(unsafe_code, clippy::unwrap_used, clippy::expect_used)]
    mod tests {
        use core::ptr::fn_addr_eq;

        use super::*;

        #[test]
        fn every_vtable_slot_is_the_matching_boundary() {
            // The shim and `blastit-py-sapi` must not disagree: each slot of the
            // frozen table is assigned the function the design names, and this
            // is the test that fails when one is swapped.
            assert!(fn_addr_eq(
                VTABLE.abi_version,
                blastit_py_abi_version as extern "C" fn() -> u32
            ));
            assert!(fn_addr_eq(
                VTABLE.py_minor,
                blastit_py_minor as extern "C" fn() -> u32
            ));
            assert!(fn_addr_eq(
                VTABLE.before_fork,
                before_fork as extern "C" fn()
            ));
            assert!(fn_addr_eq(
                VTABLE.after_fork_parent,
                after_fork_parent as extern "C" fn()
            ));
            assert!(fn_addr_eq(
                VTABLE.after_fork_child,
                after_fork_child as extern "C" fn()
            ));
            assert!(fn_addr_eq(
                VTABLE.init_paths,
                init_paths as extern "C" fn(*const u8, usize, *mut PyBufV1, *mut PyBufV1) -> i32
            ));
            assert!(fn_addr_eq(
                VTABLE.preload,
                preload
                    as extern "C" fn(
                        *const u8,
                        usize,
                        *mut *mut PyZygoteOpaque,
                        *mut PyBufV1,
                        *mut PyBufV1,
                    ) -> i32
            ));
            assert!(fn_addr_eq(
                VTABLE.check_syntax,
                check_syntax
                    as extern "C" fn(
                        *mut PyZygoteOpaque,
                        *const u8,
                        usize,
                        *const u8,
                        usize,
                        *mut PyBufV1,
                    ) -> i32
            ));
            assert!(fn_addr_eq(
                VTABLE.serve_view,
                serve_view
                    as extern "C" fn(*mut PyZygoteOpaque, *const u8, usize, *mut PyBufV1) -> i32
            ));
            assert!(fn_addr_eq(
                VTABLE.free_buf,
                free_buf as extern "C" fn(PyBufV1)
            ));
        }

        #[test]
        fn the_exports_answer_the_frozen_values() {
            assert_eq!(blastit_py_abi_version(), ABI_VERSION);
            assert_eq!(blastit_py_abi_version(), 1);
            let minor = blastit_py_minor();
            assert!(minor >= 300, "a minor encoding, got {minor}");
            assert_eq!(minor.to_string(), env!("BLASTIT_PY_MINOR"));
            assert!(!blastit_py_abi_v1().is_null());
        }

        #[test]
        fn the_vtable_reports_the_same_versions_as_the_exports() {
            // SAFETY: `blastit_py_abi_v1` is this crate's own static table.
            let table = unsafe { &*blastit_py_abi_v1() };
            assert_eq!((table.abi_version)(), blastit_py_abi_version());
            assert_eq!((table.py_minor)(), blastit_py_minor());
        }

        #[test]
        fn a_null_zygote_is_refused_with_a_sentence() {
            let mut err = PyBufV1 {
                ptr: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            let code = serve_view(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut err as *mut PyBufV1,
            );
            assert_eq!(
                code,
                buffer::REFUSED,
                "an ordinary refusal is 1, not the panic code"
            );
            // SAFETY: the refusal path wrote the sentence into `err`.
            let bytes = unsafe { std::slice::from_raw_parts(err.ptr, err.len) };
            let parsed: serde_json::Value = serde_json::from_slice(bytes).expect("valid JSON");
            assert!(
                parsed["error"]
                    .as_str()
                    .is_some_and(|text| text.contains("preloaded interpreter")),
                "{parsed}"
            );
            free(err);
        }

        #[test]
        fn a_free_buf_of_nothing_is_harmless() {
            free_buf(PyBufV1 {
                ptr: std::ptr::null_mut(),
                len: 0,
                cap: 0,
            });
        }
    }
}
