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
use std::path::{Path, PathBuf};

use blastit_sys::elf::{Elf, ElfError, MAX_SECTION_BYTES};
use blastit_sys::pin::SharedObject;

use crate::abi::{ABI_VERSION, BlastitPyVtableV1, PyForkHooks, dotted_py_minor};

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
    FreeThreaded {
        /// The command the refusal names (`blastit <cmd>:`).
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
    },
    /// The shim was built for another CPython minor than the boot discovered.
    MinorMismatch {
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
                command,
                interpreter,
                soname,
            } => write!(
                f,
                "blastit {command}: CPython at {} is free-threaded ({soname}) and cannot serve \
                 WSGI views; reinstall it without --free-threaded (uv python install cpython-3.14)",
                interpreter.display()
            ),
            Self::NullVtable => {
                f.write_str("blastit_py_abi_v1() returned NULL; this is not a blastit Python shim")
            }
            Self::AbiVersion { found } => write!(
                f,
                "the shim was built for blastit_py_abi version {found}, but this blastit speaks \
                 version {ABI_VERSION}; install the blastit build that ships this shim"
            ),
            Self::MinorMismatch { shim, expected } => {
                let shim = dotted_py_minor(*shim);
                let expected = dotted_py_minor(*expected);
                write!(
                    f,
                    "the shim was built for CPython {shim}, but this boot discovered CPython \
                     {expected}; install the shim built for CPython {expected}"
                )
            }
            Self::Nul { what } => write!(f, "{what} holds a NUL byte"),
        }
    }
}

impl std::error::Error for LoadError {}

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
    let source = object.source().to_path_buf();
    let file = File::open(object.path()).map_err(|err| LoadError::Unreadable {
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
pub struct LoadedPy {
    // Declared before `_libpython` so the shim is closed first: its PLT bound
    // every `Py_*` call against the library's image.
    shim: Opened,
    _libpython: Opened,
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
        let library = Opened::open(libpython, true, py_minor)?;
        let shim = Opened::open(shim, false, py_minor).map_err(|err| match err {
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
            return Err(LoadError::AbiVersion { found: abi });
        }
        let reported = (table.py_minor)();
        if reported != py_minor {
            return Err(LoadError::MinorMismatch {
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    /// A directory under the workspace's `target/tmp`, removed when dropped.
    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/pysapi-loader");
            let dir = root.join(format!("{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("the fixture directory");
            Self { dir }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn a_text_file_is_refused_as_not_an_elf() {
        let fixture = Fixture::new("text");
        let path = fixture.dir.join("libpython3.14.so.1.0");
        std::fs::write(&path, b"this is not an ELF object").expect("the text file");
        let object = SharedObject::open(&path, None).expect("it is a regular file");

        let error = gate_libpython(&object, 314, "dev", Path::new("/usr/bin/python3.14"))
            .expect_err("a text file is not an ELF");
        assert!(matches!(error, LoadError::NotAnElf { .. }), "{error}");
        assert!(error.to_string().contains("not an ELF object"));
    }

    #[test]
    fn a_real_elf_without_the_frozen_entry_points_is_refused_by_name() {
        // The test binary is a real, dynamically linked ELF: it has `.dynsym`
        // and `.dynamic`, and no `Py_*` definition, which is exactly the shape
        // the gate must refuse (and never panic on).
        let path = std::env::current_exe().expect("the test binary");
        let object = SharedObject::open(&path, None).expect("a regular file");
        let error = gate_libpython(&object, 314, "dev", Path::new("/usr/bin/python3.14"))
            .expect_err("a non-libpython ELF");
        let message = error.to_string();
        match error {
            LoadError::MissingSymbols { missing, .. } => {
                for name in REQUIRED_SYMBOLS {
                    assert!(
                        missing.iter().any(|found| found == name),
                        "{name} was reported present: {message}"
                    );
                }
            }
            other => panic!("expected MissingSymbols, got {other}"),
        }
        for name in REQUIRED_SYMBOLS {
            assert!(message.contains(&format!("`{name}`")), "{message}");
        }
        assert!(message.contains("apt install libpython3.14"), "{message}");
    }

    #[test]
    fn the_abi_and_minor_refusals_name_both_versions() {
        let abi = LoadError::AbiVersion { found: 2 }.to_string();
        assert!(
            abi.contains("blastit_py_abi version 2") && abi.contains("speaks version 1"),
            "{abi}"
        );
        let mismatch = LoadError::MinorMismatch {
            shim: 313,
            expected: 314,
        }
        .to_string();
        assert!(mismatch.contains("CPython 3.13"), "{mismatch}");
        assert!(mismatch.contains("discovered CPython 3.14"), "{mismatch}");
        assert!(
            !mismatch.contains("313") && !mismatch.contains("314"),
            "a refusal never prints the wire encoding: {mismatch}"
        );
        let free = LoadError::FreeThreaded {
            command: "serve".to_owned(),
            interpreter: PathBuf::from("/opt/python/3.13t/bin/python3.13t"),
            soname: "libpython3.13t.so.1.0".to_owned(),
        }
        .to_string();
        assert_eq!(
            free,
            "blastit serve: CPython at /opt/python/3.13t/bin/python3.13t is free-threaded \
             (libpython3.13t.so.1.0) and cannot serve WSGI views; reinstall it without \
             --free-threaded (uv python install cpython-3.14)"
        );
        assert!(
            !free.contains("is libpython3.13t.so.1.0"),
            "the file must not be reported as its own cause: {free}"
        );
    }

    #[test]
    fn a_real_libpython_is_gated_and_loaded_when_the_caller_names_one() {
        // Lab-first, no mock: the only way this leg runs is a host that has a
        // real `libpython3.1x.so` and names it. A host without one records why.
        let Some(path) = std::env::var_os("BLASTIT_PY_SAPI_TEST_LIBPYTHON").map(PathBuf::from)
        else {
            eprintln!(
                "skipped: BLASTIT_PY_SAPI_TEST_LIBPYTHON is unset, so there is no real libpython \
                 to gate and dlopen; set it to a libpython3.13/3.14 to run this leg"
            );
            return;
        };
        let metadata = std::fs::metadata(&path).expect("the named libpython exists");
        let object = SharedObject::open(&path, Some((metadata.dev(), metadata.ino())))
            .expect("the named libpython is pinnable");

        if let Err(refusal) = gate_libpython(&object, 314, "dev", Path::new("/usr/bin/python3.14"))
        {
            // A free-threaded build is a legitimate refusal of a real library.
            assert!(
                matches!(refusal, LoadError::FreeThreaded { .. }),
                "{}: {refusal}",
                path.display()
            );
            return;
        }
        // The real `dlopen`, with no shim: the library must load.
        let library = Opened::open(&object, true, 314).expect("a real libpython loads");
        assert_eq!(library.path(), path);
    }
}
