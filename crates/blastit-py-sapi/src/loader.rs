#![allow(unsafe_code)]

//! The one module that `dlopen`s `libpython` and a per-minor shim (design §2.4).
//!
//! The crate root says `deny(unsafe_code)`; this module and [`crate::abi`] are
//! the whole hand-off to C.
//!
//! # The gate, then the load
//!
//! [`gate_libpython`] runs **before** anything is loaded: it reads the pinned
//! descriptor through `/proc/self/fd/<fd>` with [`blastit_sys::elf`], refuses a
//! `libpython` whose `DT_SONAME` looks like a free-threaded object (`*t.so*`),
//! and refuses one that does not **define** `Py_InitializeEx`,
//! `PyGILState_Ensure` and `PyOS_BeforeFork` (an import of the name is not an
//! export). Stage 0 can therefore answer "can this host serve a Python view?"
//! without mapping an interpreter.
//!
//! [`LoadedPy::load`] then loads `libpython` with `RTLD_NOW | RTLD_GLOBAL` (the
//! shim's undefined `Py_*` symbols resolve against it) and the shim with
//! `RTLD_NOW | RTLD_LOCAL`, resolves `blastit_py_abi_v1()`, and checks that the
//! vtable's [`ABI_VERSION`] and `py_minor` are the ones this boot resolved.
//! Every failure is a value: `dlerror()` is read once, and nothing here unwraps.
//!
//! Refusals name the **source** path (what stage 0 opened), never the
//! `/proc/self/fd/<n>` spelling the load itself uses.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::{CStr, CString};
use std::fmt;
use std::fs::File;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

use blastit_sys::elf::{Elf, ElfError, MAX_SECTION_BYTES};
use blastit_sys::pin::SharedObject;

use crate::abi::{
    ABI_VERSION, BlastitPyVtableV1, PyBufV1, PyForkHooks, PyRuntime, PyZygote, dotted_py_minor,
};

/// `RTLD_NOW` (`dlfcn.h`): every undefined symbol must resolve now.
const RTLD_NOW: c_int = 2;
/// `RTLD_GLOBAL` (`dlfcn.h`): the shim resolves its `Py_*` symbols here.
const RTLD_GLOBAL: c_int = 0x100;
/// `RTLD_LOCAL` (`dlfcn.h`): the shim's symbols stay private.
const RTLD_LOCAL: c_int = 0;

/// `DT_SONAME` (`elf.h`): the dynamic entry naming this object.
const DT_SONAME: u64 = 14;

/// The entry points a `libpython` this build can serve a view from must export.
pub const REQUIRED_SYMBOLS: [&str; 3] = ["Py_InitializeEx", "PyGILState_Ensure", "PyOS_BeforeFork"];

// glibc >= 2.34 folds `libdl` into libc; on the oldest target (glibc 2.28) it is
// already a `DT_NEEDED` of every glibc image, so plain `extern` declarations
// resolve without a `#[link]`.
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

/// Why a library or shim was refused. Every variant is a boot sentence.
#[derive(Debug)]
#[non_exhaustive]
pub enum LoadError {
    /// The pinned descriptor could not be read.
    Unreadable {
        /// The path stage 0 opened.
        path: PathBuf,
        /// The kernel's own message.
        detail: String,
    },
    /// The object is not an ELF this build can read.
    NotAnElf {
        /// The path stage 0 opened.
        path: PathBuf,
        /// The bounds-checked reader's refusal.
        detail: String,
    },
    /// `dlopen` of `libpython` failed.
    NotALibrary {
        /// The path stage 0 opened.
        path: PathBuf,
        /// `dlerror()`'s message.
        detail: String,
        /// The CPython minor the boot discovered.
        py_minor: u32,
    },
    /// `libpython` loaded but the shim did not (wrong minor, wrong arch, or the
    /// shim's `Py_*` symbols did not resolve).
    ShimRefused {
        /// The shim stage 0 opened.
        path: PathBuf,
        /// `dlerror()`'s message.
        detail: String,
    },
    /// A symbol this crate resolves is absent from the image.
    MissingSymbol {
        /// The symbol's name.
        name: String,
        /// `dlerror()`'s message.
        detail: String,
    },
    /// The `.dynsym` gate did not find every required entry point.
    MissingSymbols {
        /// The path stage 0 opened.
        path: PathBuf,
        /// The names that were not defined by the object.
        missing: Vec<String>,
        /// The CPython minor the boot discovered.
        py_minor: u32,
    },
    /// `DT_SONAME` names a free-threaded CPython (`*t.so*`).
    ///
    /// Headless: the caller is the one that knows the boot's verb and prefixes
    /// `blastit <cmd>: ` once (a wheel's stage 0 and stage 1 both do).
    FreeThreaded {
        /// The command the boot asked as (`dev`, `check`, `serve`), for a caller
        /// that wants to name it in its own sentence.
        command: String,
        /// The interpreter the boot resolved — never the library path.
        interpreter: PathBuf,
        /// The SONAME that was read.
        soname: String,
    },
    /// `blastit_py_abi_v1()` returned NULL.
    NullVtable,
    /// The shim speaks another [`ABI_VERSION`].
    AbiVersion {
        /// What `abi_version()` reported.
        found: u32,
        /// The shim that spoke it.
        path: PathBuf,
    },
    /// The shim was built for another CPython minor than the boot discovered.
    MinorMismatch {
        /// The shim that was built for it.
        path: PathBuf,
        /// What `py_minor()` reported.
        shim: u32,
        /// What the boot's discovery resolved.
        expected: u32,
    },
    /// A path or name holds a NUL, so it cannot be a C string.
    Nul {
        /// What held the byte.
        what: &'static str,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, detail } => {
                write!(f, "{}: cannot be read ({detail})", path.display())
            }
            Self::NotAnElf { path, detail } => write!(
                f,
                "{} is not an ELF object this build can inspect ({detail})",
                path.display()
            ),
            Self::NotALibrary {
                path,
                detail,
                py_minor,
            } => {
                let minor = dotted_py_minor(*py_minor);
                write!(
                    f,
                    "{} is not a usable CPython library: {detail} (install the shared libpython \
                     for CPython {minor}: apt install libpython{minor})",
                    path.display()
                )
            }
            Self::ShimRefused { path, detail } => write!(
                f,
                "the shim {} did not load: {detail} (it must be the build for this CPython minor \
                 and must be opened after libpython)",
                path.display()
            ),
            Self::MissingSymbol { name, detail } => {
                write!(f, "the loaded images have no `{name}`: {detail}")
            }
            Self::MissingSymbols {
                path,
                missing,
                py_minor,
            } => {
                let minor = dotted_py_minor(*py_minor);
                let quoted = missing
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "{} does not export {quoted}; it is not a shared CPython this build can \
                     embed (install the shared libpython for CPython {minor}: apt install \
                     libpython{minor})",
                    path.display()
                )
            }
            Self::FreeThreaded {
                command: _,
                interpreter,
                soname,
            } => write!(
                f,
                "CPython at {} is free-threaded ({soname}) and cannot serve WSGI views; \
                 reinstall it without --free-threaded (uv python install cpython-3.14)",
                interpreter.display()
            ),
            Self::NullVtable => {
                f.write_str("blastit_py_abi_v1() returned NULL; this is not a blastit Python shim")
            }
            Self::AbiVersion { found, path } => write!(
                f,
                "the shim {} was built for blastit_py_abi version {found}, but this blastit \
                 speaks version {ABI_VERSION}; install the blastit build that ships this shim",
                path.display()
            ),
            Self::MinorMismatch {
                path,
                shim,
                expected,
            } => {
                let shim = dotted_py_minor(*shim);
                let expected = dotted_py_minor(*expected);
                write!(
                    f,
                    "the shim {} was built for CPython {shim}, but this boot discovered CPython \
                     {expected}; install the shim built for CPython {expected}",
                    path.display()
                )
            }
            Self::Nul { what } => write!(f, "{what} holds a NUL byte"),
        }
    }
}

impl std::error::Error for LoadError {}

/// One pinned descriptor and the path it was opened from (design §1.1).
///
/// Stage 1 holds the descriptor stage 0 verified; it never re-opens the object
/// by path. The load reads `/proc/self/fd/<fd>` and every refusal names
/// `source`, the file an operator can find.
#[derive(Clone, Copy)]
pub struct PinnedRef<'a> {
    /// The descriptor this process inherited (still open, `FD_CLOEXEC` set).
    pub fd: RawFd,
    /// The path stage 0 opened it from — what a refusal names.
    pub source: &'a Path,
}

impl PinnedRef<'_> {
    /// `/proc/self/fd/<fd>`, the only name a load uses.
    fn proc_path(self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.fd))
    }
}

/// The `.dynsym`/`DT_SONAME` gate of §2.4, over the pinned descriptor.
///
/// `py_minor` is the minor the boot discovered; it only names the package in the
/// refusal (`libpython3.14`). `command` is the boot's verb (`dev`, `check`,
/// `serve`) and `interpreter` the interpreter it resolved, so a refusal reads as
/// one sentence an operator can act on and never repeats the library path as if
/// it were the cause.
///
/// # Errors
///
/// [`LoadError::Unreadable`], [`LoadError::NotAnElf`],
/// [`LoadError::FreeThreaded`] and [`LoadError::MissingSymbols`].
pub fn gate_libpython(
    object: &SharedObject,
    py_minor: u32,
    command: &str,
    interpreter: &Path,
) -> Result<(), LoadError> {
    gate_pinned(
        PinnedRef {
            fd: object.as_raw_fd(),
            source: object.source(),
        },
        py_minor,
        command,
        interpreter,
    )
}

/// [`gate_libpython`] over a descriptor stage 1 already holds.
///
/// # Errors
///
/// As [`gate_libpython`].
pub fn gate_pinned(
    object: PinnedRef<'_>,
    py_minor: u32,
    command: &str,
    interpreter: &Path,
) -> Result<(), LoadError> {
    let source = object.source.to_path_buf();
    let file = File::open(object.proc_path()).map_err(|err| LoadError::Unreadable {
        path: source.clone(),
        detail: err.to_string(),
    })?;
    let mut elf = Elf::parse(file).map_err(|err| not_an_elf(&source, &err))?;
    let sonames = match elf.dynamic_strings(DT_SONAME, MAX_SECTION_BYTES) {
        Ok(sonames) => sonames,
        Err(ElfError::NoDynamic) => Vec::new(),
        Err(err) => return Err(not_an_elf(&source, &err)),
    };
    if let Some(soname) = sonames.into_iter().find(|soname| soname.contains("t.so")) {
        return Err(LoadError::FreeThreaded {
            command: command.to_owned(),
            interpreter: interpreter.to_path_buf(),
            soname,
        });
    }
    let exported = match elf.dynsym_names(MAX_SECTION_BYTES) {
        Ok(symbols) => symbols,
        Err(ElfError::NoDynsym) => Vec::new(),
        Err(err) => return Err(not_an_elf(&source, &err)),
    };
    let missing: Vec<String> = REQUIRED_SYMBOLS
        .into_iter()
        .filter(|name| {
            !exported
                .iter()
                .any(|symbol| symbol.defined && symbol.name == *name)
        })
        .map(str::to_owned)
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(LoadError::MissingSymbols {
            path: source,
            missing,
            py_minor,
        })
    }
}

/// One `dlopen`ed shared object, closed when dropped.
pub struct Opened {
    handle: *mut c_void,
    path: PathBuf,
}

impl Opened {
    /// `dlopen`s the object's `/proc/self/fd/<fd>` name.
    ///
    /// `global` is `true` for `libpython` (the shim resolves `Py_*` against it)
    /// and `false` for the shim. The stored `path` is the **source** path, so a
    /// refusal names a file an operator can find.
    ///
    /// # Errors
    ///
    /// [`LoadError::NotALibrary`] when `dlopen` fails, [`LoadError::Nul`] when
    /// the path cannot be a C string.
    pub fn open(object: &SharedObject, global: bool, py_minor: u32) -> Result<Self, LoadError> {
        let load_path = object.path();
        let path = object.source().to_path_buf();
        let name =
            CString::new(load_path.to_string_lossy().as_bytes()).map_err(|_| LoadError::Nul {
                what: "the shared-object path",
            })?;
        let flags = RTLD_NOW | if global { RTLD_GLOBAL } else { RTLD_LOCAL };
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let handle = unsafe { dlopen(name.as_ptr(), flags) };
        if handle.is_null() {
            return Err(LoadError::NotALibrary {
                path,
                detail: dl_error(),
                py_minor,
            });
        }
        Ok(Self { handle, path })
    }

    /// `dlopen`s `/proc/self/fd/<fd>` for an object stage 1 already holds.
    ///
    /// # Errors
    ///
    /// [`LoadError::NotALibrary`] when `dlopen` fails, [`LoadError::Nul`] when
    /// the name cannot be a C string.
    pub fn open_pinned(
        object: PinnedRef<'_>,
        global: bool,
        py_minor: u32,
    ) -> Result<Self, LoadError> {
        let path = object.source.to_path_buf();
        let proc_name = object.proc_path();
        let proc_spelling = proc_name.to_string_lossy().into_owned();
        let name = CString::new(proc_spelling.as_bytes()).map_err(|_| LoadError::Nul {
            what: "the shared-object path",
        })?;
        let flags = RTLD_NOW | if global { RTLD_GLOBAL } else { RTLD_LOCAL };
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let handle = unsafe { dlopen(name.as_ptr(), flags) };
        if handle.is_null() {
            // glibc echoes the name it was given, and that name is this
            // process's own descriptor spelling: report the file the operator
            // can find instead (`from_shim` copies this `detail` verbatim, so
            // one substitution covers both refusals).
            let detail = dl_error().replace(&proc_spelling, &path.display().to_string());
            return Err(LoadError::NotALibrary {
                path,
                detail,
                py_minor,
            });
        }
        Ok(Self { handle, path })
    }

    /// The source path this object was loaded from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The raw `dlopen` handle, for a caller resolving its own symbols.
    #[must_use]
    pub fn handle(&self) -> *mut c_void {
        self.handle
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: `handle` came from a successful `dlopen` and is closed once.
        unsafe { dlclose(self.handle) };
    }
}

/// A loaded `libpython` + shim pair with the frozen vtable resolved.
///
/// `Embed` (the universal binary) owns both objects; `Hosted` (a wheel whose
/// CPython is already live) owns the shim alone — there is nothing to load and
/// nothing to gate, and `init_paths` is never called (design §1.2.1 item 1).
pub struct LoadedPy {
    // Declared before `_libpython` so the shim is closed first: its PLT bound
    // every `Py_*` call against the library's image.
    shim: Opened,
    _libpython: Option<Opened>,
    vtable: *const BlastitPyVtableV1,
}

impl LoadedPy {
    /// Loads `libpython` and then `shim`, and checks the ABI and the minor.
    ///
    /// # Errors
    ///
    /// Everything [`Opened::open`] refuses, plus [`LoadError::NullVtable`],
    /// [`LoadError::AbiVersion`] and [`LoadError::MinorMismatch`].
    pub fn load(
        libpython: &SharedObject,
        shim: &SharedObject,
        py_minor: u32,
    ) -> Result<Self, LoadError> {
        Self::load_pinned(
            PinnedRef {
                fd: libpython.as_raw_fd(),
                source: libpython.source(),
            },
            PinnedRef {
                fd: shim.as_raw_fd(),
                source: shim.source(),
            },
            py_minor,
        )
    }

    /// [`LoadedPy::load`] over the two descriptors stage 1 already holds.
    ///
    /// `libpython` is `RTLD_NOW | RTLD_GLOBAL`, then `shim` is
    /// `RTLD_NOW | RTLD_LOCAL` — the documented order (design §1.2.1).
    ///
    /// # Errors
    ///
    /// As [`LoadedPy::load`].
    pub fn load_pinned(
        libpython: PinnedRef<'_>,
        shim: PinnedRef<'_>,
        py_minor: u32,
    ) -> Result<Self, LoadError> {
        let library = Opened::open_pinned(libpython, true, py_minor)?;
        Self::from_shim(shim, Some(library), py_minor)
    }

    /// Resolves `blastit_py_abi_v1` out of a freshly opened shim and checks it.
    fn from_shim(
        shim: PinnedRef<'_>,
        library: Option<Opened>,
        py_minor: u32,
    ) -> Result<Self, LoadError> {
        let shim = Opened::open_pinned(shim, false, py_minor).map_err(|err| match err {
            LoadError::NotALibrary { path, detail, .. } => LoadError::ShimRefused { path, detail },
            other => other,
        })?;
        // SAFETY: `shim` is a live handle and the name is a `c"…"` literal.
        let pointer = unsafe { dlsym(shim.handle(), c"blastit_py_abi_v1".as_ptr()) };
        if pointer.is_null() {
            return Err(LoadError::MissingSymbol {
                name: "blastit_py_abi_v1".to_owned(),
                detail: dl_error(),
            });
        }
        // SAFETY: the shim's own export, whose signature `abi` freezes.
        let entry: extern "C" fn() -> *const BlastitPyVtableV1 = unsafe {
            core::mem::transmute::<*mut c_void, extern "C" fn() -> *const BlastitPyVtableV1>(
                pointer,
            )
        };
        let vtable = entry();
        if vtable.is_null() {
            return Err(LoadError::NullVtable);
        }
        // SAFETY: non-null, and the vtable is static for the process's lifetime.
        let table = unsafe { &*vtable };
        let abi = (table.abi_version)();
        if abi != ABI_VERSION {
            return Err(LoadError::AbiVersion {
                found: abi,
                path: shim.path().to_path_buf(),
            });
        }
        let reported = (table.py_minor)();
        if reported != py_minor {
            return Err(LoadError::MinorMismatch {
                path: shim.path().to_path_buf(),
                shim: reported,
                expected: py_minor,
            });
        }
        Ok(Self {
            shim,
            _libpython: library,
            vtable,
        })
    }

    /// The composition root's one handle on this shim (design §1.2.1 item 2).
    ///
    /// Carries the loader's lifetime, so a `PyRuntime` cannot outlive the
    /// `dlopen` it points into.
    #[must_use]
    pub fn runtime<'a>(&'a self, zygote: PyZygote) -> PyRuntime<'a> {
        PyRuntime {
            zygote,
            hooks: self.hooks(),
            vtable: self.vtable(),
        }
    }

    /// The frozen vtable this shim exported.
    #[must_use]
    pub fn vtable(&self) -> &BlastitPyVtableV1 {
        // SAFETY: `load` proved the pointer non-null and static.
        unsafe { &*self.vtable }
    }

    /// The three fork hooks, borrowed from this loader (§1.2).
    ///
    /// The returned `Copy` value carries `self`'s lifetime, so it cannot outlive
    /// the `dlopen` handle it points into.
    #[must_use]
    pub fn hooks(&self) -> PyForkHooks<'_> {
        let table = self.vtable();
        PyForkHooks::of(
            self,
            table.before_fork,
            table.after_fork_parent,
            table.after_fork_child,
        )
    }

    /// The shim's CPython minor, already checked against the discovery.
    #[must_use]
    pub fn py_minor(&self) -> u32 {
        (self.vtable().py_minor)()
    }

    /// The shim's source path, for a caller's refusal sentences.
    #[must_use]
    pub fn shim_path(&self) -> &Path {
        self.shim.path()
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
    // SAFETY: a non-null `dlerror()` result is NUL-terminated.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}

/// The ELF reader's refusal, in this module's sentence, naming the source path.
fn not_an_elf(path: &Path, err: &ElfError) -> LoadError {
    LoadError::NotAnElf {
        path: path.to_path_buf(),
        detail: err.to_string(),
    }
}

/// Copies one [`PyBufV1`] slot out and reclaims it through the shim's `free_buf`.
///
/// A slot the shim never wrote is null and is only handed back to `free_buf`
/// (which ignores it). A slot whose length exceeds its capacity is refused
/// without being read: this crate would rather drop an answer than read outside
/// an allocation.
///
/// It lives here because it is the crate's **one** `from_raw_parts`: design §1.1
/// allows `unsafe` in `abi.rs` and `loader.rs` only, so the byte boundary
/// (`boundary.rs`) stays a safe translation layer.
pub(crate) fn take_slot(vtable: &BlastitPyVtableV1, slot: PyBufV1) -> Vec<u8> {
    let bytes = if slot.ptr.is_null() || slot.cap == 0 || slot.len > slot.cap {
        Vec::new()
    } else {
        // SAFETY: the shim wrote `len` initialised bytes at `ptr`, `len <= cap`,
        // and both are this call's own allocation.
        unsafe { core::slice::from_raw_parts(slot.ptr, slot.len) }.to_vec()
    };
    (vtable.free_buf)(slot);
    bytes
}
