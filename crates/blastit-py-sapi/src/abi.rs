#![allow(unsafe_code)]

//! The frozen `blastit_py_abi_v1` surface (design §1.2), declared verbatim.
//!
//! These are **declarations only**: the semantics live in the shim
//! (`crates/blastit-py-shim`), and the loader calls them through the pointers a
//! loaded `libblastit_py3{13,14}-<arch>-<libc>.so` returns. Changing a field
//! here is an ABI break and must bump [`ABI_VERSION`] and every shim together.
//!
//! The exported symbols a shim must carry are `blastit_py_abi_version() -> u32`
//! (`1`), `blastit_py_minor() -> u32` ([`PY_MINOR_313`] or [`PY_MINOR_314`]) and
//! `blastit_py_abi_v1() -> *const BlastitPyVtableV1`.

use core::marker::PhantomData;

use crate::loader::LoadedPy;

/// The ABI this crate speaks: `blastit_py_abi_v1` is version 1.
pub const ABI_VERSION: u32 = 1;

/// The `py_minor` encoding: `major × 100 + minor`, so CPython `3.13` is `313`.
#[must_use]
pub const fn encode_py_minor(major: u32, minor: u32) -> u32 {
    major * 100 + minor
}

/// CPython 3.13 as a shim reports it through `py_minor`.
pub const PY_MINOR_313: u32 = encode_py_minor(3, 13);

/// CPython 3.14 as a shim reports it through `py_minor`.
pub const PY_MINOR_314: u32 = encode_py_minor(3, 14);

/// `313` -> `"3.13"`, for a sentence an operator reads.
///
/// The `py_minor` encoding is a wire value, never a version: a refusal that
/// printed `313` would name nothing a host can install.
#[must_use]
pub fn dotted_py_minor(py_minor: u32) -> String {
    format!("{}.{}", py_minor / 100, py_minor % 100)
}

/// One owned byte buffer a shim returns, freed by its `free_buf`.
///
/// # Ownership
///
/// A slot a boundary writes into (`out` in [`BlastitPyVtableV1::init_paths`],
/// `err` everywhere) **must arrive zeroed** — `ptr == null`, `len == 0`,
/// `cap == 0`. A non-null `ptr` therefore always means "a buffer this shim
/// returned": the shim allocates, the caller reclaims through
/// [`BlastitPyVtableV1::free_buf`] and never through `free(3)`. The shim does
/// not free a slot on entry (a slot it did not allocate cannot be reconstructed
/// as a `Vec`), so a caller that reuses a slot frees the previous answer first.
#[repr(C)]
pub struct PyBufV1 {
    /// The bytes, or null with `len == 0`.
    pub ptr: *mut u8,
    /// The used length.
    pub len: usize,
    /// The allocated capacity.
    pub cap: usize,
}

/// The preloaded interpreter image a child inherits. Opaque by construction.
#[repr(C)]
pub struct PyZygoteOpaque {
    _private: [u8; 0],
}

/// A preloaded interpreter image, as the composition root holds it.
///
/// The pointer names a box the **shim** allocated (`Box::into_raw` in
/// `preload`) and owns for the process's lifetime: `crates/blastit` never reads
/// it, never frees it, and there is no `free_zygote` entry to call (design
/// §1.2.1 item 2). Every forked view still references the `Py<PyAny>` inside it,
/// so dropping the box would strand them.
#[derive(Clone, Copy)]
pub struct PyZygote(*mut PyZygoteOpaque);

impl PyZygote {
    /// The handle of a boot that has not preloaded anything.
    #[must_use]
    pub fn null() -> Self {
        Self(core::ptr::null_mut())
    }

    /// Whether this handle names no interpreter image.
    #[must_use]
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    /// The opaque pointer, for the one caller that hands it back to the shim.
    #[must_use]
    pub fn as_raw(self) -> *mut PyZygoteOpaque {
        self.0
    }

    /// The handle a shim's `preload` wrote through its out parameter.
    #[must_use]
    pub fn from_raw(raw: *mut PyZygoteOpaque) -> Self {
        Self(raw)
    }
}

/// Everything `crates/blastit` knows about a loaded shim (design §1.2.1 item 2).
///
/// A `Copy` value of three parts, and the whole of the composition root's
/// Python surface: the opaque zygote, the three `PyOS_*Fork` hooks and the
/// frozen vtable the entry points live in. **No `pyo3` type appears anywhere in
/// `crates/blastit`** — this handle is what replaces them.
///
/// # The lifetime is the loader's
///
/// `vtable` and `hooks` point inside the `dlopen`ed shim, so `'a` is the borrow
/// of the [`crate::loader::LoadedPy`] that owns it. Stage 1's `Serving`/`Parts`
/// owns that loader and the supervisor borrows this value; a handle that
/// outlived its shim is not expressible through safe code, and there is no
/// `'static` spelling and no `transmute`.
#[derive(Clone, Copy)]
pub struct PyRuntime<'a> {
    /// The preloaded image every view is forked from.
    pub zygote: PyZygote,
    /// The three `PyOS_*Fork` entry points, called with the GIL held.
    pub hooks: PyForkHooks<'a>,
    /// The frozen table the entry points live in.
    pub vtable: &'a BlastitPyVtableV1,
}

/// The vtable one shim returns from `blastit_py_abi_v1()`.
#[repr(C)]
pub struct BlastitPyVtableV1 {
    /// [`ABI_VERSION`] as the shim was built against.
    pub abi_version: extern "C" fn() -> u32,
    /// The CPython minor, encoded as [`PY_MINOR_313`]/[`PY_MINOR_314`].
    pub py_minor: extern "C" fn() -> u32,
    /// `PyOS_BeforeFork`, called with the GIL held.
    pub before_fork: extern "C" fn(),
    /// `PyOS_AfterFork_Parent`.
    pub after_fork_parent: extern "C" fn(),
    /// `PyOS_AfterFork_Child`.
    pub after_fork_child: extern "C" fn(),
    /// `Py_InitializeEx(0)`, `SITE_PACKAGES`, restores `sys_path` (`Vec<Vec<u8>>`) and
    /// cwd, checks the minor, evaluates `VIEW_PATHS` & `SYS_PATH` ->
    /// `{"version","prefix","view_paths","sys_path"}`.
    pub init_paths:
        extern "C" fn(req: *const u8, req_len: usize, out: *mut PyBufV1, err: *mut PyBufV1) -> i32,
    /// `LoadedTracker` (if `report_fd >= 0`), imports app + preload + warmup, flushes
    /// the boot batch, `CLOSE_AND_COLLECT`, `immortal::apply`, `gc.freeze()` ->
    /// `{"decision","objects","reason","elapsed_ns"}`.
    pub preload: extern "C" fn(
        req: *const u8,
        req_len: usize,
        zygote: *mut *mut PyZygoteOpaque,
        out: *mut PyBufV1,
        err: *mut PyBufV1,
    ) -> i32,
    /// `PyCode::compile` with `PyCodeInput::File` without execution (`dev` reload preflight).
    ///
    /// `zygote` is an identity check only — the compile runs in the calling
    /// process, so the preloaded parent is not used — but a null or misaligned
    /// pointer is refused like every other entry that takes one.
    pub check_syntax: extern "C" fn(
        zygote: *mut PyZygoteOpaque,
        code: *const u8,
        code_len: usize,
        path: *const u8,
        path_len: usize,
        err: *mut PyBufV1,
    ) -> i32,
    /// Post-`blastit_sys::child::enter(plan)` child entry: sets Python
    /// `os.environ["HOME"/"TMPDIR"]`, adopts `view_fd` + optional
    /// `(loaded_fd, loaded_source)`, runs `serve_reporting`.
    ///
    /// Returns the child's exit code, and the three values are the child
    /// sequence's own: `0` the channel ended and every accepted request was
    /// served, `1` a refusal (one sentence in `err`), `70` a panic caught at the
    /// boundary.
    pub serve_view: extern "C" fn(
        zygote: *mut PyZygoteOpaque,
        req: *const u8,
        req_len: usize,
        err: *mut PyBufV1,
    ) -> i32,
    /// Releases a [`PyBufV1`] the shim produced.
    pub free_buf: extern "C" fn(buf: PyBufV1),
}

/// The three `fork(2)` hooks, as one `Copy` value the composition root injects
/// (design §1.2, "zero-global fork wiring").
///
/// A `Copy` struct rather than a trait or a global: `forking.rs` takes
/// `Option<PyForkHooks>` and hands the three pointers to its own call sites, so
/// no Python symbol is ever resolved outside this crate and the loader.
///
/// # The lifetime is the shim's, not decoration
///
/// The pointers are addresses inside the loaded shim, and `LoadedPy`'s `drop`
/// may `dlclose` it. Carrying `PhantomData<&'a LoadedPy>` makes the compiler
/// tie every hooks value to the borrow it came from, so a `Copy` that outlived
/// its loader is no longer expressible through safe code. `LoadedPy` is
/// process-lifetime in practice (a started interpreter is never unloaded), and
/// its sole holder owns the hooks.
#[derive(Clone, Copy)]
pub struct PyForkHooks<'a> {
    /// `PyOS_BeforeFork`, called with the GIL held.
    pub before_fork: extern "C" fn(),
    /// `PyOS_AfterFork_Parent`.
    pub after_fork_parent: extern "C" fn(),
    /// `PyOS_AfterFork_Child`.
    pub after_fork_child: extern "C" fn(),
    /// Ties the hooks to the [`LoadedPy`] whose shim owns the pointers.
    _owner: PhantomData<&'a LoadedPy>,
}

impl<'a> PyForkHooks<'a> {
    /// The hooks of a vtable `loaded` owns: constructible only by [`LoadedPy`]
    /// and by this crate's tests.
    pub(crate) fn of(
        _loaded: &'a LoadedPy,
        before_fork: extern "C" fn(),
        after_fork_parent: extern "C" fn(),
        after_fork_child: extern "C" fn(),
    ) -> Self {
        Self {
            before_fork,
            after_fork_parent,
            after_fork_child,
            _owner: PhantomData,
        }
    }
}

impl PyForkHooks<'static> {
    /// The hooks of an interpreter that is **already live** in this image.
    ///
    /// A wheel install runs stage 1 inside the interpreter the console script
    /// started, so its three `PyOS_*Fork` entry points are reachable before any
    /// shim is loaded. The composition root (`crates/blastit/src/py.rs`) is the
    /// only caller: it wraps the interpreter's own `pyo3::ffi` entry points and
    /// hands the result down as the same `Option<PyForkHooks<'_>>` every fork
    /// takes. Once the shim loader owns every hook (design §1.2.1), this
    /// constructor and its caller are deleted.
    #[must_use]
    pub fn hosted(
        before_fork: extern "C" fn(),
        after_fork_parent: extern "C" fn(),
        after_fork_child: extern "C" fn(),
    ) -> Self {
        Self {
            before_fork,
            after_fork_parent,
            after_fork_child,
            _owner: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use super::*;

    #[test]
    fn py_minor_is_encoded_major_times_one_hundred() {
        assert_eq!(encode_py_minor(3, 13), 313);
        assert_eq!(encode_py_minor(3, 14), 314);
        assert_eq!((PY_MINOR_313, PY_MINOR_314), (313, 314));
        assert_eq!(dotted_py_minor(PY_MINOR_313), "3.13");
        assert_eq!(dotted_py_minor(PY_MINOR_314), "3.14");
    }

    #[test]
    fn the_buffer_is_three_words_in_the_design_order() {
        let word = size_of::<usize>();
        assert_eq!(size_of::<PyBufV1>(), 3 * word);
        assert_eq!(offset_of!(PyBufV1, ptr), 0);
        assert_eq!(offset_of!(PyBufV1, len), word);
        assert_eq!(offset_of!(PyBufV1, cap), 2 * word);
    }

    #[test]
    fn the_zygote_marker_occupies_no_storage() {
        assert_eq!(size_of::<PyZygoteOpaque>(), 0);
        assert_eq!(align_of::<PyZygoteOpaque>(), 1);
    }

    #[test]
    fn the_vtable_is_ten_function_pointers_in_the_frozen_order() {
        let pointer = size_of::<extern "C" fn()>();
        assert_eq!(size_of::<BlastitPyVtableV1>(), 10 * pointer);
        let offsets = [
            offset_of!(BlastitPyVtableV1, abi_version),
            offset_of!(BlastitPyVtableV1, py_minor),
            offset_of!(BlastitPyVtableV1, before_fork),
            offset_of!(BlastitPyVtableV1, after_fork_parent),
            offset_of!(BlastitPyVtableV1, after_fork_child),
            offset_of!(BlastitPyVtableV1, init_paths),
            offset_of!(BlastitPyVtableV1, preload),
            offset_of!(BlastitPyVtableV1, check_syntax),
            offset_of!(BlastitPyVtableV1, serve_view),
            offset_of!(BlastitPyVtableV1, free_buf),
        ];
        for (index, offset) in offsets.into_iter().enumerate() {
            assert_eq!(offset, index * pointer, "field {index} moved");
        }
    }

    #[test]
    fn the_fork_hooks_are_three_pointers_plus_a_phantom_lifetime() {
        assert_eq!(
            size_of::<PyForkHooks<'static>>(),
            3 * size_of::<extern "C" fn()>(),
            "the PhantomData<&LoadedPy> must cost no storage"
        );
    }

    #[test]
    fn the_vtable_field_types_are_the_frozen_signatures() {
        // A compile-time witness: each field's declared type, so a shim that
        // exports a different signature cannot agree with this mirror by
        // accident.
        fn _abi(table: &BlastitPyVtableV1) {
            let _: extern "C" fn() -> u32 = table.abi_version;
            let _: extern "C" fn() -> u32 = table.py_minor;
            let _: extern "C" fn() = table.before_fork;
            let _: extern "C" fn() = table.after_fork_parent;
            let _: extern "C" fn() = table.after_fork_child;
            let _: extern "C" fn(*const u8, usize, *mut PyBufV1, *mut PyBufV1) -> i32 =
                table.init_paths;
            let _: extern "C" fn(
                *const u8,
                usize,
                *mut *mut PyZygoteOpaque,
                *mut PyBufV1,
                *mut PyBufV1,
            ) -> i32 = table.preload;
            let _: extern "C" fn(
                *mut PyZygoteOpaque,
                *const u8,
                usize,
                *const u8,
                usize,
                *mut PyBufV1,
            ) -> i32 = table.check_syntax;
            let _: extern "C" fn(*mut PyZygoteOpaque, *const u8, usize, *mut PyBufV1) -> i32 =
                table.serve_view;
            let _: extern "C" fn(PyBufV1) = table.free_buf;
        }
    }
}
