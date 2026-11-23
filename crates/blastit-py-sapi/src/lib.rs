#![deny(unsafe_code)]
#![deny(missing_docs)]

//! Per-minor CPython shims: the frozen `blastit_py_abi_v1` surface, the
//! filesystem-only stage-0 discovery, and the one `dlopen` module.
//!
//! # What this crate is
//!
//! Blastit serves a Python WSGI view from a forked, unprivileged `view` worker
//! (design `docs/design/universal-distribution.md` §1). The shipped `blastit`
//! binary links no `libpython`; instead each supported CPython minor gets one
//! `libblastit_py<major><minor>-<arch>-<libc>.so` shim, loaded by descriptor,
//! that owns the interpreter. This crate holds the three pieces the composition
//! root and the shim share:
//!
//! * [`abi`] — the frozen `#[repr(C)]` types of §1.2, verbatim: [`PyBufV1`],
//!   [`PyZygoteOpaque`], [`PyForkHooks`] and [`BlastitPyVtableV1`] with its ten
//!   function fields (including `free_buf`).
//! * [`discovery`] — §2's interpreter order, `PYTHONHOME`/`site-packages`, the
//!   `libpython` search, the shim search, the activation gate and the three
//!   refusal sentences. Filesystem-only: **stage 0 never executes Python**, and
//!   the environment is passed in rather than read.
//! * [`loader`] — the `.dynsym`/`DT_SONAME` gate on `libpython`, then
//!   `dlopen("/proc/self/fd/<fd>")` of the library and the shim, the ABI and
//!   minor checks, and [`LoadedPy::hooks`] for the fork wiring.
//!
//! `abi` and `loader` are the only modules allowed `unsafe`: the crate root says
//! `#![deny(unsafe_code)]`, so a change anywhere else is a compile error rather
//! than a review question.
//!
//! ```
//! use blastit_py_sapi::{ABI_VERSION, activates, encode_py_minor};
//!
//! assert_eq!(ABI_VERSION, 1);
//! assert_eq!(encode_py_minor(3, 13), 313);
//! assert_eq!(encode_py_minor(3, 14), 314);
//! // A PHP-only or static-only boot never searches for an interpreter (§2.1).
//! assert!(!activates(true, false));
//! assert!(activates(false, false));
//! ```

// The CPython table this crate publishes, generated from
// `packaging/languages.tsv` at build time (`build.rs`).
include!(concat!(env!("OUT_DIR"), "/languages.rs"));

pub mod abi;
mod boundary;
pub mod discovery;
pub mod loader;

pub use abi::{
    ABI_VERSION, BlastitPyVtableV1, PyBufV1, PyForkHooks, PyRuntime, PyZygote, PyZygoteOpaque,
    dotted_py_minor, encode_py_minor,
};
pub use discovery::{
    DevEnv, DiscoveredPython, DiscoveryEnv, DiscoveryError, ProductionEnv, activates, discover,
    shim_file,
};
pub use loader::{
    LoadError, LoadedPy, Opened, PinnedRef, REQUIRED_SYMBOLS, gate_libpython, gate_pinned,
};
