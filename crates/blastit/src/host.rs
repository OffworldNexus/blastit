//! How blastit was installed, and therefore how it starts itself again
//! (§2.1, §4).
//!
//! Every stage after stage 0 is a new program image: the `execve` barrier that
//! hides the real credentials also means blastit has to be able to name itself.
//! This module is the whole of that knowledge, and it is the only thing the
//! packaging changes:
//!
//! * the **binary** re-executes `/proc/self/exe`, exactly as §4 describes;
//! * a **wheel install** starts no interpreter at all: `blastit.__main__` hands
//!   the command line to the universal binary the wheel ships at
//!   `blastit/_bin/blastit` (`MB.42.2.2.2`). One `py3-none-*` artifact serves
//!   every CPython `>=3.12`, and the Python side never hosts stage 1.
//!
//! dbproxy holds the real credentials (§2.2), so it never runs an interpreter:
//! stage 0 `execve`s this same binary again with `--role=dbproxy` and an empty
//! environment.
//!
//! # D6: the file with the capabilities is never the file a child runs
//!
//! A rootless [`crate::Host`] is started from a file that carries the boot's
//! capabilities (B1, B2), and that file must be **init's alone**:
//!
//! * a child's bounding set is empty by the time it execs anything, and the
//!   kernel refuses to `execve` a file with file capabilities when the file's
//!   permitted set cannot be honoured — `EPERM`, "insufficient to execute
//!   correctly". So `--role=dbproxy` cannot be `/proc/self/exe` when that is the
//!   capable file: `BLASTIT_EXEC` names the capability-free copy for a
//!   standalone install;
//! * every other process of the same uid — an operator's `python manage.py`,
//!   a second `manage.py`, anything a deployment runs next to the server — must
//!   not find the capabilities either, which is why the capable file is a
//!   *dedicated copy* with mode `0500` owned by the uid init runs as, and the
//!   shared interpreter or binary stays capability-free.
//!
//! [`placement`] decides that, and refuses the two deployments that cannot
//! work, naming the file to fix.

use std::ffi::{CString, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use blastit_sys::elf::{Elf, ElfError};
use blastit_sys::exe_path;

/// The environment variable that names a standalone install's capability-free
/// binary (D6/B2).
///
/// Read by the standalone binary only; a wheel always `execve`s the
/// `blastit-dbproxy` it ships.
pub const EXEC: &str = "BLASTIT_EXEC";

/// `R7`/`B2-R5`: the **deployment** half of the `[route.php]` gate, for the
/// protected suite.
///
/// `stage0` is a private module, so `tests/critical_php_embed.rs` cannot call
/// `stage0::php_boot_in`; this re-export is the seam that runs the *same*
/// function a deployment boot runs, with a `Deployment`. Without it a leg can
/// only drive [`Provenance`] — the decision — so deleting the call *inside*
/// `php_boot_in` would leave the protected tier green, which is the defect this
/// seam closes (`MB.3.43`).
///
/// Hidden because it is not part of the published API: a `dev`/`check` boot
/// never passes a deployment, and an unprivileged process cannot reach a
/// `Production` boot at all (`LESSONS.md`, `MB.1.6`), so no other caller can
/// exist. It lives in `host` because [`Provenance`] is the decision it applies.
// A build without the `php` feature has no gate to expose: the deployable form
// of it refuses an `entrypoint php` with one sentence instead
// (`stage0::no_php_support`, `R13`).
#[cfg(feature = "php")]
#[doc(hidden)]
pub use crate::stage0::deployment_boot_check;

/// A program that plays one of blastit's roles, and how to ask it for one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reexec {
    /// The path `execve` is given.
    pub program: PathBuf,
    /// The arguments that come before the role's own: `-I -m blastit` under the
    /// wheel, nothing for the standalone binary.
    pub prefix: Vec<OsString>,
}

impl Reexec {
    /// A program asked for a role by its own arguments alone.
    pub fn program(program: &Path) -> Self {
        Self {
            program: program.to_path_buf(),
            prefix: Vec::new(),
        }
    }
}

/// The installation blastit is running from (§2.1, §4).
///
/// Stage 0 is the only stage that uses it: it is what the two `execve`s of §4
/// name, and it carries the interpreter's import path across the barrier into
/// stage 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    stage1: Reexec,
    dbproxy: Reexec,
    /// The interpreter's `sys.path` as the console script found it, in bytes
    /// (`os.fsencode`): empty for the binary, whose stage 1 `dlopen`s CPython.
    sys_path: Vec<Vec<u8>>,
    /// D6: the capability-free counterpart of the file that may carry the
    /// capabilities — the binary `BLASTIT_EXEC` names — **and every name it can
    /// be started by**.
    ///
    /// A list because a capability is a property of the *file*, not of the name:
    /// `python`, `python3`, `python3.13` and `sys._base_executable` can be one
    /// file, a symlink chain or several, and a deployment that gave any of them
    /// the capabilities gave them to init's uid at large (B1).
    ///
    /// Empty when the installation cannot name such a file, which is only
    /// allowed when nothing carries file capabilities: [`Host::placement`]
    /// refuses a deployment that has to be able to name it and cannot.
    plain: Vec<PathBuf>,
}

/// D6: the noun a capability refusal uses for the file to copy and `setcap`.
///
/// The program is one binary now (`MB.42.2.2.2`): a wheel starts it through
/// `blastit/_bin/blastit`, so no refusal anywhere names an interpreter.
pub const FILE_NOUN: &str = "binary";

/// What stage 0 execs, once the deployment has been checked (D6/B1, B2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The program stage 1 is `execve`d as — init's image, which is the file a
    /// deployment may have given the capabilities to.
    pub init: PathBuf,
    /// The program `--role=dbproxy` is `execve`d as: a child's `execve`, which
    /// may never be a file with file capabilities while the child's bounding set
    /// is empty.
    pub dbproxy: PathBuf,
}

impl Host {
    /// The standalone binary: every role is `/proc/self/exe` (§4).
    ///
    /// [`EXEC`] (`BLASTIT_EXEC`) names the capability-free copy of this binary
    /// when the deployment gave the capabilities to a dedicated one (D6/B2); it
    /// is what [`Host::placement`] then hands to `--role=dbproxy`.
    pub fn binary() -> Self {
        let program = Reexec::program(exe_path());
        let plain = std::env::var_os(EXEC)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .into_iter()
            .collect();
        Self {
            stage1: program.clone(),
            dbproxy: program,
            sys_path: Vec::new(),
            plain,
        }
    }

    /// What stage 0 `execve`s to cross the barrier into stage 1 (§4).
    pub fn stage1(&self) -> &Reexec {
        &self.stage1
    }

    /// What stage 0 `execve`s for the credentials-holding role, once the
    /// deployment has been checked (D6).
    ///
    /// [`Host::placement`] is the one to ask: this is the *unchecked* answer,
    /// which is `/proc/self/exe` for a standalone install even when that file
    /// carries the capabilities a child cannot be granted.
    pub fn dbproxy(&self) -> &Reexec {
        &self.dbproxy
    }

    /// The interpreter's import path, for stage 1 to restore (§4).
    pub fn sys_path(&self) -> &[Vec<u8>] {
        &self.sys_path
    }

    /// The capability-free program this installation can name, and every name
    /// it can be started by (D6).
    pub fn plain(&self) -> &[PathBuf] {
        &self.plain
    }

    /// The programs stage 0 must `execve`, or the reason it cannot (D6/B1, B2).
    ///
    /// `drops_bounding` is whether the children's bounding sets will be emptied
    /// ([`blastit_sys::caps::detect`] says): a child cannot `execve` a file with
    /// file capabilities at all, so `--role=dbproxy` must not point at one.
    ///
    /// # Errors
    ///
    /// A sentence naming the file to fix: `BLASTIT_EXEC` for a standalone
    /// install, the interpreter for a wheel. See [`placement`].
    pub fn placement(&self, drops_bounding: bool) -> Result<Placement, String> {
        // `BLASTIT_EXEC` is an operator's variable, so it is the one worth
        // validating by name.
        if let Some(binary) = self.plain.first() {
            if !binary.is_file() {
                return Err(format!(
                    "{EXEC}={} is not a file: a rootless standalone install names the \
                     capability-free copy of the binary there",
                    binary.display()
                ));
            }
            // D6/B2: a child `execve`s this file, and it runs next to the real
            // credentials (§2.2). Root or init's uid owns it, nobody else may
            // write it or the directory it sits in.
            plain_is_protected(binary, rustix::process::geteuid().as_raw())?;
        }
        // A standalone install plays every role itself, and `BLASTIT_EXEC` is
        // the capability-free copy of it (D6/B2).
        let child = self
            .plain
            .first()
            .cloned()
            .unwrap_or_else(|| self.dbproxy.program.clone());
        placement(
            &self.stage1.program,
            &child,
            &self.plain,
            drops_bounding,
            same_file,
            has_file_caps,
        )
    }
}

/// Whether `path` carries file capabilities (`security.capability`, D6).
///
/// This is the attribute the kernel reads at an `execve` to grant the file's
/// permitted set; a child whose bounding set is empty cannot honour it, which is
/// why the boot refuses to point a child at such a file at all (B1, B2). The
/// path is followed (a symlink is as capable as its target, because the kernel
/// reads the resolved inode's attribute).
///
/// # Errors
///
/// Only the errnos that are *not* an answer. `ENODATA` is the ordinary "no
/// attribute"; `ENOTSUP` means this filesystem has no extended attributes at
/// all (`/proc`, a bare tmpfs), so nothing on it can carry file capabilities and
/// that is an answer too. Anything else — `EACCES` on a file we may not read,
/// `ENOENT` on one that is not there — is returned: a capability check that
/// cannot be made must not be read as "no capabilities".
pub fn has_file_caps(path: &Path) -> io::Result<bool> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("a path cannot contain a NUL byte"))?;
    // SAFETY: `getxattr` takes a NUL-terminated path, a NUL-terminated name, a
    // buffer and its length; a null buffer of length zero asks for the size, and
    // it reads no memory we do not own.
    let size = unsafe {
        libc::getxattr(
            path.as_ptr(),
            c"security.capability".as_ptr(),
            std::ptr::null_mut(),
            0,
        )
    };
    if size >= 0 {
        return Ok(size > 0);
    }
    let err = io::Error::last_os_error();
    match caps_from_errno(err.raw_os_error()) {
        Some(caps) => Ok(caps),
        None => Err(io::Error::new(
            err.kind(),
            format!("{}: {err}", path.to_string_lossy()),
        )),
    }
}

/// What an `errno` from `getxattr` means for a capability check (D6).
///
/// `Some(false)` for the two that are an *answer* — `ENODATA` ("there is no
/// such attribute") and `ENOTSUP` ("this filesystem has no extended attributes
/// at all", so nothing on it can carry file capabilities) — and `None` for
/// everything else, which the caller surfaces as an error: an `EACCES` on a file
/// this process may not read, or an `ENOENT` on one that is not there, must not
/// be mistaken for "no capabilities".
pub fn caps_from_errno(errno: Option<i32>) -> Option<bool> {
    match errno {
        Some(libc::ENODATA) | Some(libc::ENOTSUP) => Some(false),
        _ => None,
    }
}

/// Whether two paths are the same file, symlinks followed (D6/B1).
///
/// The device and inode numbers, not the names: `python`, `python3`,
/// `python3.13` and a copy called `python3.13-init` can be the same file, a
/// symlink chain, or different files, and what the kernel grants at an `execve`
/// depends on the inode.
pub fn same_file(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(left), std::fs::metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

/// The facts about a path that decide whether another uid could replace it
/// (D6/B2): its own permissions and its parent's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The uid that owns the file.
    pub owner: u32,
    /// Whether the file is group- or other-writable.
    pub writable_by_others: bool,
    /// Whether any directory above it is group- or other-writable.
    pub directory_writable_by_others: bool,
}

/// Reads [`Facts`] for `path`, following symlinks.
///
/// # Errors
///
/// [`std::io::Error`] when the file or one of its directories cannot be
/// inspected, which is a path that cannot be checked and therefore cannot be
/// used.
pub fn facts_of(path: &Path) -> io::Result<Facts> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = std::fs::metadata(path)?;
    let mode = metadata.permissions().mode();
    // `S_IWGRP | S_IWOTH`: anybody but the owner may write it, so anybody but
    // the owner may replace the binary the children `execve`.
    let writable_by_others = mode & (0o020 | 0o002) != 0;
    let mut directory_writable_by_others = false;
    let mut directory = path.parent();
    while let Some(current) = directory {
        if traditional_exists(current) {
            let parent = std::fs::metadata(current)?;
            directory_writable_by_others |= parent.permissions().mode() & (0o020 | 0o002) != 0;
        }
        directory = current.parent();
    }
    Ok(Facts {
        owner: metadata.uid(),
        writable_by_others,
        directory_writable_by_others,
    })
}

/// Whether a path exists, without following a final symlink (D6).
fn traditional_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Refuses a capability-free program another uid could replace (D6/B2).
///
/// `BLASTIT_EXEC` names the binary a *child* `execve`s for `--role=dbproxy`. If
/// that file — or any directory above it — can be written by a uid other than
/// root and init's own, then that uid decides what runs next: it could put a
/// program of its own there, and dbproxy holds the real credentials (§2.2).
/// `/tmp` is the obvious trap (mode `1777`), and the message says so.
///
/// # Errors
///
/// A sentence naming the file, what is wrong with it, and where such a binary
/// belongs. The facts come from [`facts_of`], so an unreadable path is an error
/// rather than an assumption.
pub fn plain_is_protected(path: &Path, init_uid: u32) -> Result<(), String> {
    let facts = facts_of(path).map_err(|err| {
        format!(
            "{EXEC}={}: cannot be inspected ({err}), so the boot cannot tell whether another \
             uid could replace it",
            path.display()
        )
    })?;
    protected(&facts, init_uid).map_err(|wrong| {
        format!(
            "{EXEC}={} could be replaced by another uid: {wrong}. Point it at a binary in a \
             directory only root or init's uid can write, owned by root or by init's uid and \
             not writable by anyone else",
            path.display()
        )
    })
}

/// Whether [`Facts`] describe a file no other uid can replace (D6/B2).
///
/// Pure, so every combination of owner and mode is a unit test.
///
/// # Errors
///
/// What is wrong with it, in the words of the message [`plain_is_protected`]
/// builds.
pub fn protected(facts: &Facts, init_uid: u32) -> Result<(), String> {
    let mut wrong = Vec::new();
    if facts.owner != 0 && facts.owner != init_uid {
        wrong.push(format!(
            "it is owned by uid {}, and only root or init's uid ({init_uid}) may own it",
            facts.owner
        ));
    }
    if facts.writable_by_others {
        wrong.push("it is group- or other-writable".to_string());
    }
    if facts.directory_writable_by_others {
        wrong.push(
            "a directory above it is group- or other-writable (a temporary directory, or \
             `/tmp`, is the usual one)"
                .to_string(),
        );
    }
    if wrong.is_empty() {
        return Ok(());
    }
    Err(wrong.join("; "))
}

/// Decides which program plays each role, or names what a deployment must
/// change (D6/B1, B2).
///
/// Pure — `same_file` and `caps_of` are the two things it cannot compute by
/// itself — so every deployment shape, and the sentence it produces, is a unit
/// test:
///
/// * a file that carries the capabilities must be a **dedicated copy**: it may
///   not be the file the children use ([`Placement::init`] against
///   [`Placement::dbproxy`]), compared by device and inode rather than by name,
///   so `blastit` and `blastit-init` are two files even when one is a symlink
///   (B1);
/// * **no** name of the shared binary may carry a file capability: the
///   capability is granted per *file*, and every one of those names is a way to
///   run it;
/// * a child may not `execve` a file with file capabilities **at all** when its
///   bounding set will be emptied, so `--role=dbproxy` may not point at the
///   capable file; the fix is `BLASTIT_EXEC`.
///
/// # Errors
///
/// The sentence an operator reads, naming the file and the `setcap` line.
pub fn placement(
    init: &Path,
    child: &Path,
    plain: &[PathBuf],
    drops_bounding: bool,
    same_file: impl Fn(&Path, &Path) -> bool,
    caps_of: impl Fn(&Path) -> io::Result<bool>,
) -> Result<Placement, String> {
    let caps = |path: &Path| -> Result<bool, String> {
        caps_of(path).map_err(|err| {
            format!(
                "cannot read the file capabilities of {}: {err}: the boot cannot tell whether \
                 the kernel would grant them to a process that runs it",
                path.display()
            )
        })
    };
    // Every name of the shared binary must be capability-free: whichever one a
    // deployment starts, the capability belongs to init's file alone (B1). A
    // name that *is* init's file (the dedicated copy the deployment starts) is
    // not a shared name, and may carry them.
    for name in plain {
        if same_file(name, init) {
            continue;
        }
        if caps(name)? {
            return Err(capable_copy_advice_for(name, init));
        }
    }
    if caps(init)? {
        // The capable file must be init's alone. What it must *not* be is the
        // file the shared binary is: that is the case where the boot cannot name
        // a capability-free file other than init's own image, either because it
        // cannot name one at all or because every name it has *is* that image.
        let shared: Vec<&PathBuf> = plain.iter().filter(|name| !same_file(name, init)).collect();
        if shared.is_empty() {
            return Err(match plain.first() {
                None => format!(
                    "{} carries file capabilities and this installation cannot name the \
                     capability-free {} the children must use instead: {EXEC}",
                    init.display(),
                    FILE_NOUN,
                ),
                Some(named) => capable_copy_advice_for(named, init),
            });
        }
    }
    if drops_bounding && caps(child)? {
        return Err(format!(
            "{} carries file capabilities, and the children of this boot cannot \
             `execve` such a file at all (their bounding set is empty): point {EXEC} at \
             the capability-free copy",
            child.display(),
        ));
    }
    Ok(Placement {
        init: init.to_path_buf(),
        dbproxy: child.to_path_buf(),
    })
}

/// The sentence an operator gets when a file that is *not* init's copy carries
/// the capabilities (D6/B1).
fn capable_copy_advice_for(capable: &Path, init: &Path) -> String {
    format!(
        "{} carries file capabilities, and init runs from {} instead: every process of that \
         uid that can run it becomes init.\n{}",
        capable.display(),
        init.display(),
        capable_copy_advice(capable)
    )
}

/// The recipe that turns the shared file into init's private copy (D6/B1, B2).
///
/// `shared` is the file that has the capabilities and must not: the binary, or
/// any of the names it can be started by — the file itself is what the kernel
/// looks at, so a symlink chain makes them all one mistake.
fn capable_copy_advice(shared: &Path) -> String {
    let resolved = std::fs::read_link(shared).unwrap_or_else(|_| shared.to_path_buf());
    // A symlink target may be relative (`python` → `python3.13`): a recipe that
    // reads `<dir>/python3.13-init` has to be about the *same* directory as the
    // link, not about the current one.
    let resolved = if resolved.is_relative() {
        shared.parent().unwrap_or(Path::new(".")).join(resolved)
    } else {
        resolved
    };
    let copy = resolved.with_file_name(format!(
        "{}-init",
        resolved
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let tail = format!(
        "  then start the copy, and set {EXEC}={} so that the database proxy runs the \
         capability-free binary",
        resolved.display()
    );
    format!(
        "{} carries file capabilities: give them to a *dedicated copy* instead, because \
         every process of the uid init runs as could run that file otherwise — and init must \
         be the only one that can.\n  \
         cp {src} {copy}\n  chown <the uid init runs as>:<its gid> {copy}\n  \
         chmod 0500 {copy}\n  setcap '{setcap}+ep' {copy}\n{tail}",
        shared.display(),
        src = resolved.display(),
        copy = copy.display(),
        setcap = blastit_sys::caps::Capability::SETCAP,
        tail = tail,
    )
}

// ---------------------------------------------------------------------------
// Production provenance (`ADR-0010` §3, design `R7` / `B2-R5`)
// ---------------------------------------------------------------------------
//
// A deployment boot hands its workers two kinds of path: files a worker
// `dlopen`s (`libphp`, the per-minor shim, an interpreter) and trees it reads
// (`manifest_dir` — every PHP worker's `cwd` and `include_path` — plus a route's
// `entry` and `static.root`). Every one of them is chosen by the route uids'
// code at some point, so the boot authorises them once, in stage 0, before a
// listener binds. `blastit dev` and `blastit check` deliberately do not: the
// shim a developer just built lives in `target/php-sapi`, inside a checkout that
// uid owns and writes, so the same rule would refuse every development boot
// while protecting nothing (the uid that could replace the shim is the uid that
// already runs the boot). A deployment has no such coincidence.

/// `DT_RPATH`: the search path the legacy linker writes for the loader.
const DT_RPATH: u64 = 15;

/// `DT_RUNPATH`: the same for the modern linker (`--enable-new-dtags`).
const DT_RUNPATH: u64 = 29;

/// The largest dynamic string table this walk will read into memory. An object's
/// `DT_STRTAB` is a few hundred bytes; this is the bound that keeps a hostile
/// file from being an allocation.
const MAX_DYNAMIC_STRTAB: u64 = 4 * 1024 * 1024;

/// The directories an ELF object's `DT_RPATH`/`DT_RUNPATH` add to the dynamic
/// loader's search path, read from the file itself.
///
/// This is the static half of design `R7`: an object whose search path names a
/// directory a route uid can write is one that uid chooses the `DT_NEEDED`
/// resolution of, so the boot refuses it. **Nothing is loaded** — no `dlopen`,
/// no `mmap`: the header, the section header table and the `SHT_DYNAMIC` array
/// are `seek`/`read` through [`blastit_sys::elf`], which bounds-checks every
/// offset and length against the file's own length and refuses a truncated or
/// hostile object instead of panicking.
///
/// # Errors
///
/// A sentence naming what could not be read: a file that is not a
/// little-endian ELF, one with no section headers or no dynamic section, and one
/// that is truncated. A search path that cannot be read must never be read as
/// "no search path" — that is the difference between a check and an assumption.
pub fn dynamic_search_paths(path: &Path) -> Result<Vec<String>, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|err| format!("{}: it cannot be read ({err})", path.display()))?;
    let mut elf = Elf::parse(&mut file).map_err(|err| object_error(path, &err))?;
    let entries = match elf.dynamic_strings_any(&[DT_RPATH, DT_RUNPATH], MAX_DYNAMIC_STRTAB) {
        Ok(entries) => entries,
        // An object with no dynamic section has no search path at all; a
        // dynamic entry that is not a string is a refusal, never "nothing".
        Err(ElfError::NoDynamic) => return Ok(Vec::new()),
        Err(err) => return Err(object_error(path, &err)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        paths.extend(entry.split(':').map(ToOwned::to_owned));
    }
    Ok(paths)
}

/// The sentence [`dynamic_search_paths`] prints for a failure in the shared
/// reader.
///
/// The wording of a provenance refusal is this module's: an operator reads "its
/// section header table is past the end of the file", and the shared layer's
/// `Display` is only the generic spelling of the same fact.
fn object_error(path: &Path, err: &ElfError) -> String {
    let detail = match err {
        ElfError::Io(err) => return format!("{}: it cannot be read ({err})", path.display()),
        ElfError::NoMagic => "it is not an ELF object (no ELF magic)",
        ElfError::IdentTooShort => "its ELF identification is truncated",
        ElfError::UnknownClass(_) => "it is not an ELF object (unknown class)",
        ElfError::BigEndian => {
            "it is a big-endian ELF object, and only little-endian objects are embedded"
        }
        ElfError::HeaderTruncated => "its ELF header is truncated",
        ElfError::NoSections => "it has no section headers, so its dynamic section cannot be read",
        ElfError::SectionEntriesTooSmall => {
            "its section header table's entries are too small to be section headers"
        }
        ElfError::SectionTableTooLarge => "its section header table is too large",
        ElfError::SectionTablePastEnd => "its section header table is past the end of the file",
        ElfError::DynamicString => {
            "a DT_RPATH/DT_RUNPATH entry is not a string this build can read"
        }
        other => return format!("{}: {other}", path.display()),
    };
    format!("{}: {detail}", path.display())
}

/// One path a deployment boot authorises before a worker is given it
/// (`ADR-0010` §3, design `R7`/`B2-R5`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    /// How a refusal names it: `the NTS libphp`, `route "app": [route.php]
    /// entry "public/index.php"`, ….
    pub what: String,
    /// The path, resolved as the boot will open it.
    pub path: PathBuf,
}

impl Claim {
    /// A claim on `path`, named `what` in every refusal.
    #[must_use]
    pub fn new(what: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            what: what.into(),
            path: path.into(),
        }
    }
}

/// The paths a deployment boot hands its workers, and the uid that must own them
/// (design `R7`/`B2-R5`).
///
/// Two lists, because the two questions differ:
///
/// * [`objects`](Self::objects) are files a worker `dlopen`s or `execve`s
///   (`libphp`, a shim, an interpreter). Besides "no other uid can replace it"
///   they may carry no file capabilities a child's emptied bounding set cannot
///   honour, and no `DT_RPATH`/`DT_RUNPATH` entry into a directory a route uid
///   can write — the loader resolves their `DT_NEEDED` from there;
/// * [`trees`](Self::trees) are read, never loaded (`manifest_dir`, a route's
///   `entry`, a `static.root`): a PHP worker's `cwd` and `include_path` are that
///   tree, so a route uid that can write a file into it chooses what the worker
///   will `include`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// The uid that owns the installation: root, or init's own uid in a
    /// rootless deployment. Only root and this uid may own the paths.
    pub init_uid: u32,
    /// The files a worker loads.
    pub objects: Vec<Claim>,
    /// The trees a worker reads.
    pub trees: Vec<Claim>,
}

impl Provenance {
    /// Authorises [`Self::objects`]: ownership and permissions, no file
    /// capabilities, and zero writable `DT_RPATH`/`DT_RUNPATH` entries.
    ///
    /// # Errors
    ///
    /// The first refusal, naming the claim, its path, what is wrong with it and
    /// the command that fixes it.
    pub fn check_objects(&self) -> Result<(), String> {
        self.check_objects_with(has_file_caps)
    }

    /// [`check_objects`](Self::check_objects) with the capability predicate
    /// **injected**, the way [`placement`] takes `caps_of`.
    ///
    /// `setxattr` on `security.capability` needs `CAP_SETFCAP`, so no
    /// unprivileged test can put the attribute on a file it made — and a test
    /// that early-returns when it is not root leaves the `!has_file_caps`
    /// refusal with no witness in `make check` (`MB.3.43`). Private on purpose:
    /// the only caller besides [`check_objects`](Self::check_objects) is this
    /// module's tests, and the boot always reads the real attribute.
    fn check_objects_with(
        &self,
        caps_of: impl Fn(&Path) -> io::Result<bool>,
    ) -> Result<(), String> {
        for claim in &self.objects {
            self.cannot_be_replaced(claim)?;
            match caps_of(&claim.path) {
                Ok(false) => {}
                Ok(true) => {
                    return Err(format!(
                        "{} ({}) carries file capabilities (security.capability), and a child \
                         whose bounding set is empty cannot honour them: strip them with \
                         `setcap -r {}`, and give the capability to the file init runs instead \
                         (D6/B1)",
                        claim.what,
                        claim.path.display(),
                        claim.path.display()
                    ));
                }
                Err(err) => {
                    return Err(format!(
                        "{} ({}): its file capabilities cannot be read ({err}), so the boot \
                         cannot tell whether a child would inherit a privilege it must not have",
                        claim.what,
                        claim.path.display()
                    ));
                }
            }
            self.no_writable_search_path(claim)?;
        }
        Ok(())
    }

    /// Authorises [`Self::trees`]: ownership and permissions only, because the
    /// tree is read rather than loaded.
    ///
    /// # Errors
    ///
    /// The first refusal, naming the claim, its path, what is wrong with it and
    /// the command that fixes it.
    pub fn check_trees(&self) -> Result<(), String> {
        for claim in &self.trees {
            self.cannot_be_replaced(claim)?;
        }
        Ok(())
    }

    /// `B2`: one path another uid could replace, and the fix.
    fn cannot_be_replaced(&self, claim: &Claim) -> Result<(), String> {
        let facts = facts_of(&claim.path).map_err(|err| {
            format!(
                "{} ({}): it cannot be inspected ({err}), so the boot cannot tell whether \
                 another uid could replace it",
                claim.what,
                claim.path.display()
            )
        })?;
        protected(&facts, self.init_uid).map_err(|wrong| {
            format!(
                "{} ({}) could be replaced by another uid: {wrong}. chown it to root (or to \
                 init's uid, {}), remove the group and other write bits (chmod 0644 for a file, \
                 0755 for a directory), and do the same to every directory above it",
                claim.what,
                claim.path.display(),
                self.init_uid
            )
        })
    }

    /// `R7`: zero `DT_RPATH`/`DT_RUNPATH` entries into a directory another uid
    /// can write, decided from the file's dynamic section without loading it.
    fn no_writable_search_path(&self, claim: &Claim) -> Result<(), String> {
        let origin = std::fs::canonicalize(&claim.path)
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        for entry in dynamic_search_paths(&claim.path)? {
            let Some(directory) = resolve_search_entry(&entry, origin.as_deref()) else {
                return Err(format!(
                    "{} ({}) has a DT_RPATH/DT_RUNPATH entry {entry:?} this boot cannot resolve \
                     statically — a relative path, an empty element (the loader reads that as \
                     the worker's own working directory), or a token other than `$ORIGIN` — so \
                     the boot cannot tell where the loader would look: relink it with no \
                     `-rpath` (a distro package does), or with an absolute directory only root \
                     or init's uid can write",
                    claim.what,
                    claim.path.display()
                ));
            };
            let facts = self.facts_of_nearest(&directory).map_err(|err| {
                format!(
                    "{} ({}) has a DT_RPATH/DT_RUNPATH entry {entry:?} naming {}: {err}",
                    claim.what,
                    claim.path.display(),
                    directory.display()
                )
            })?;
            if let Err(wrong) = protected(&facts, self.init_uid) {
                return Err(format!(
                    "{} ({}) has a DT_RPATH/DT_RUNPATH entry {entry:?} naming {}, which another \
                     uid could write: {wrong}. The loader resolves the object's DT_NEEDED from \
                     there, so that uid chooses code this worker loads: relink it with no \
                     `-rpath` (a distro package does), or with a directory only root or init's \
                     uid can write",
                    claim.what,
                    claim.path.display(),
                    directory.display()
                ));
            }
        }
        Ok(())
    }

    /// [`facts_of`] for a directory that need not be there.
    ///
    /// A search path naming a directory that does not exist is not a place
    /// anything can be loaded from *today* — what decides is the nearest
    /// existing directory above it, because a writable one means a route uid
    /// could create the rest and drop a `DT_NEEDED` object in it.
    fn facts_of_nearest(&self, directory: &Path) -> Result<Facts, String> {
        let mut candidate = directory;
        loop {
            match facts_of(candidate) {
                Ok(facts) => return Ok(facts),
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    candidate = candidate.parent().ok_or_else(|| {
                        format!("{} cannot be inspected ({err})", directory.display())
                    })?;
                }
                Err(err) => {
                    return Err(format!(
                        "{} cannot be inspected ({err})",
                        directory.display()
                    ));
                }
            }
        }
    }
}

/// Where one `DT_RPATH`/`DT_RUNPATH` element points, or `None` when the boot
/// cannot resolve it without the loader.
///
/// `$ORIGIN` (and `${ORIGIN}`) is the one token that is decidable statically —
/// it is the directory the object itself was loaded from — so it is expanded.
/// Everything else is refused rather than guessed: `$LIB`, `$PLATFORM`, a bare
/// relative path and the empty element are all resolved by the dynamic loader
/// against state the stage-0 process does not share with the worker.
fn resolve_search_entry(entry: &str, origin: Option<&Path>) -> Option<PathBuf> {
    let mut text = entry.to_owned();
    if let Some(origin) = origin {
        let origin = origin.to_string_lossy();
        text = text.replace("${ORIGIN}", &origin);
        text = text.replace("$ORIGIN", &origin);
    }
    if text.is_empty() || text.contains('$') || !Path::new(&text).is_absolute() {
        return None;
    }
    Some(PathBuf::from(text))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use blastit_sys::elf::{SHT_DYNAMIC, SHT_STRTAB};

    /// Serialises the tests that read or write `BLASTIT_EXEC`.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The lock described on [`ENV`]; a poisoned lock still runs the next test.
    fn env() -> std::sync::MutexGuard<'static, ()> {
        ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A directory with a name of its own, under `$HOME`, removed when dropped.
    ///
    /// Not `temp_dir()`: the checks in this module refuse a program any other uid
    /// could replace, and `/tmp` is group- and other-writable — exactly what they
    /// refuse.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let root = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir);
            let path = root.join(format!(".blastit-host-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { path }
        }

        /// Writes a file with `mode`, as this (test) process's uid.
        fn file(&self, name: &str, mode: u32) -> PathBuf {
            let path = self.path.join(name);
            std::fs::write(&path, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        }

        /// A directory inside it, with `mode`.
        fn dir(&self, name: &str, mode: u32) -> PathBuf {
            let path = self.path.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// The uid init runs as in these tests: this process's.
    fn init_uid() -> u32 {
        rustix::process::geteuid().as_raw()
    }

    /// A [`TempDir`] whose paths no other uid can write, or the reason this
    /// environment has none (an odd `$HOME`, a shared build directory).
    ///
    /// A `Result` rather than the `Option` a test could silently `return` on:
    /// the caller that *needs* one panics with the reason (`MB.3.43`), the way
    /// the protected suite's `protected_root` does.
    fn safe_dir(tag: &str) -> Result<TempDir, String> {
        let dir = TempDir::new(tag);
        let probe = dir.file("probe", 0o755);
        plain_is_protected(&probe, init_uid())?;
        Ok(dir)
    }

    /// `caps_of` for the pure `placement` tests: exactly these paths carry file
    /// capabilities, and every other one does not.
    fn caps_of<'a>(paths: &'a [&'a str]) -> impl Fn(&Path) -> io::Result<bool> + 'a {
        move |path| Ok(paths.iter().any(|held| Path::new(held) == path))
    }

    /// `caps_of` for an attribute that cannot be read: this path is an error.
    fn caps_of_broken(path: &'static str) -> impl Fn(&Path) -> io::Result<bool> {
        move |candidate| {
            if candidate == Path::new(path) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "no xattr for you",
                ));
            }
            Ok(false)
        }
    }

    /// `same_file` for the pure tests: exactly these pairs are one file.
    fn same<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&Path, &Path) -> bool + 'a {
        move |left, right| {
            pairs.iter().any(|(one, other)| {
                (Path::new(one) == left && Path::new(other) == right)
                    || (Path::new(one) == right && Path::new(other) == left)
            })
        }
    }

    /// No two paths are the same file.
    fn distinct(_left: &Path, _right: &Path) -> bool {
        false
    }

    #[test]
    fn the_binary_reexecutes_itself_for_every_role() {
        let _env = env();
        // SAFETY: `env()` is held, so no other test reads or writes the variable.
        unsafe { std::env::remove_var(EXEC) };
        let host = Host::binary();
        assert_eq!(host.stage1().program, Path::new("/proc/self/exe"));
        assert_eq!(host.dbproxy().program, Path::new("/proc/self/exe"));
        assert!(host.stage1().prefix.is_empty());
        assert!(host.dbproxy().prefix.is_empty());
        assert!(host.sys_path().is_empty());
        assert!(host.plain().is_empty());
        // Nothing carries file capabilities, so there is nothing to keep apart.
        assert_eq!(
            host.placement(true).unwrap(),
            Placement {
                init: PathBuf::from("/proc/self/exe"),
                dbproxy: PathBuf::from("/proc/self/exe"),
            }
        );
    }

    #[test]
    fn a_rootless_binary_plays_dbproxy_as_the_capability_free_copy() {
        let _env = env();
        let dir = safe_dir("rootless-binary").expect("a directory only its owner can write");
        let plain = dir.file("blastit", 0o755);
        // SAFETY: `env()` is held, so no other test reads or writes the variable.
        unsafe { std::env::set_var(EXEC, &plain) };
        let host = Host::binary();
        assert_eq!(host.plain(), std::slice::from_ref(&plain));
        assert_eq!(host.placement(true).unwrap().dbproxy, plain);
        // SAFETY: `env()` is still held.
        unsafe { std::env::remove_var(EXEC) };
    }

    #[test]
    fn a_binary_that_is_not_there_is_refused_by_name() {
        let _env = env();
        // SAFETY: `env()` is held, so no other test reads or writes the variable.
        unsafe { std::env::set_var(EXEC, "/nonexistent/blastit") };
        let err = Host::binary().placement(true).unwrap_err();
        assert!(err.contains("BLASTIT_EXEC=/nonexistent/blastit"), "{err}");
        assert!(err.contains("not a file"), "{err}");
        // SAFETY: `env()` is still held.
        unsafe { std::env::remove_var(EXEC) };
    }

    #[test]
    fn a_capability_free_binary_another_uid_could_replace_is_refused() {
        let _env = env();
        let dir = TempDir::new("replaceable");
        // A binary every uid may write: whoever replaces it decides what dbproxy
        // runs next (B2).
        let writable = dir.file("blastit", 0o777);
        // SAFETY: `env()` is held, so no other test reads or writes the variable.
        unsafe { std::env::set_var(EXEC, &writable) };
        let err = Host::binary().placement(true).unwrap_err();
        assert!(err.contains("could be replaced by another uid"), "{err}");
        assert!(err.contains("group- or other-writable"), "{err}");
        assert!(err.contains("init's uid"), "{err}");
        // SAFETY: `env()` is still held.
        unsafe { std::env::remove_var(EXEC) };
    }

    /// The mistake B1 is about: `setcap` on the binary a project's shell runs,
    /// named by `BLASTIT_EXEC`.
    #[test]
    fn every_named_program_that_carries_capabilities_is_refused() {
        // Synthetic paths: the refusal's sentences are what this proves, and
        // `placement()` follows the names with a real `fs::read_link` — a host
        // `/usr/local/bin/python3.13 -> ...` symlink would decide the outcome.
        let names = [
            "/nonexistent/blastit-test/python",
            "/nonexistent/blastit-test/python3",
            "/nonexistent/blastit-test/python3.13",
        ];
        for capable in names {
            // init runs from a *copy*, so the refusal can only come from the
            // capability on the shared file.
            let err = placement(
                Path::new("/opt/blastit/blastit-init"),
                Path::new("/opt/blastit/blastit"),
                &names.iter().map(PathBuf::from).collect::<Vec<_>>(),
                true,
                distinct,
                caps_of(&[capable]),
            )
            .unwrap_err();
            assert!(
                err.contains(capable),
                "{capable} is named in the refusal: {err}"
            );
            assert!(err.contains("dedicated copy"), "{err}");
            assert!(
                err.contains(&format!("cp {capable} {capable}-init")),
                "the recipe copies the file that has the capabilities: {err}"
            );
            assert!(err.contains("chmod 0500"), "{err}");
            assert!(err.contains(blastit_sys::caps::Capability::SETCAP), "{err}");
            assert!(err.contains(EXEC), "{err}");
        }
    }

    /// A real `security.capability` (root, as `setcap` would write it) on a real
    /// file another name resolves to: the check runs with the real `same_file`
    /// and the real `has_file_caps`.
    #[test]
    fn a_real_capability_on_a_shared_name_is_refused() {
        if !rustix::process::geteuid().is_root() {
            println!("skipped: setting security.capability needs CAP_SETFCAP");
            return;
        }
        let dir = safe_dir("names").expect("a directory only its owner can write");
        let real = dir.file("python3.13", 0o755);
        // The two other names a project's interpreter is reached by: relative
        // symlinks, which is what a distribution ships.
        std::os::unix::fs::symlink("python3.13", dir.path.join("python3")).unwrap();
        std::os::unix::fs::symlink("python3", dir.path.join("python")).unwrap();
        let mut value = [0_u8; 20];
        value[..4].copy_from_slice(&0x0200_0001_u32.to_le_bytes());
        value[4..8].copy_from_slice(&1_u32.to_le_bytes());
        let name = c"security.capability";
        let path_c = CString::new(real.as_os_str().as_bytes()).unwrap();
        // SAFETY: `setxattr` takes a NUL-terminated path and name, a buffer of
        // the length given and a flags word.
        let wrote = unsafe {
            libc::setxattr(
                path_c.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        assert_eq!(wrote, 0, "setxattr: {}", io::Error::last_os_error());

        let init = dir.file("python3.13-init", 0o500);
        let names: Vec<PathBuf> = ["python", "python3", "python3.13"]
            .iter()
            .map(|name| dir.path.join(name))
            .collect();
        for capable in &names {
            let err = placement(
                &init,
                Path::new("/w/_bin/blastit"),
                &names,
                true,
                same_file,
                has_file_caps,
            )
            .unwrap_err();
            assert!(
                err.contains(&capable.to_string_lossy().into_owned()),
                "{} is named: {err}",
                capable.display()
            );
            assert!(err.contains("dedicated copy"), "{err}");
            // The recipe is about a file *in this directory* (the symlinks are
            // relative, so this is what proves the target was joined to the
            // link's own directory rather than to the current one).
            assert!(
                err.contains(&format!("cp {}", dir.path.display())),
                "the copy is made from a file in the same directory: {err}"
            );
            assert!(err.contains("-init"), "{err}");
        }
    }

    #[test]
    fn the_recipe_reads_a_relative_symlink_target_in_the_right_directory() {
        // `read_link` on a relative symlink (`python` → `python3.13`) gives a
        // relative path: the copy the recipe names has to be in the *link's*
        // directory, not in the current one.
        let dir = TempDir::new("relative");
        let real = dir.file("python3.13", 0o755);
        let link = dir.path.join("python");
        std::os::unix::fs::symlink("python3.13", &link).unwrap();
        // init is that file, and `python` is one of its names.
        let capable_init = |path: &Path| Ok(path == real.as_path());
        let err = placement(
            &real,
            Path::new("/w/_bin/blastit"),
            std::slice::from_ref(&link),
            true,
            same_file,
            capable_init,
        )
        .unwrap_err();
        assert!(
            err.contains(&format!("cp {} {}-init", real.display(), real.display())),
            "an absolute destination, in the symlink's directory: {err}"
        );
    }

    #[test]
    fn a_shared_name_that_is_init_itself_is_refused() {
        // The same file under two names: `BLASTIT_EXEC` resolving to init's own
        // file, by device and inode rather than by name.
        let err = placement(
            Path::new("/usr/local/bin/blastit-init"),
            Path::new("/w/blastit"),
            &[PathBuf::from("/usr/local/bin/blastit")],
            true,
            same(&[("/usr/local/bin/blastit", "/usr/local/bin/blastit-init")]),
            caps_of(&["/usr/local/bin/blastit-init"]),
        )
        .unwrap_err();
        assert!(err.contains("dedicated copy"), "{err}");
        assert!(
            err.contains("/usr/local/bin/blastit") && err.contains("-init"),
            "the recipe is about the file the names reach: {err}"
        );
        assert!(err.contains("capabilities"), "{err}");
    }

    #[test]
    fn a_capability_that_cannot_be_named_is_refused_by_name() {
        let err = placement(
            Path::new("/opt/blastit/blastit-init"),
            Path::new("/w/blastit"),
            &[],
            true,
            distinct,
            caps_of(&["/opt/blastit/blastit-init"]),
        )
        .unwrap_err();
        assert!(err.contains("cannot name"), "{err}");
        assert!(err.contains(EXEC), "{err}");
    }

    #[test]
    fn a_capability_check_that_fails_is_a_refusal_not_an_assumption() {
        // An xattr that cannot be read must not be read as "no capabilities": a
        // sandbox that cannot prove something must not assume it.
        let err = placement(
            Path::new("/proc/self/exe"),
            Path::new("/proc/self/exe"),
            &[PathBuf::from("/not/readable/blastit")],
            true,
            distinct,
            caps_of_broken("/not/readable/blastit"),
        )
        .unwrap_err();
        assert!(err.contains("cannot read the file capabilities"), "{err}");
        assert!(err.contains("/not/readable/blastit"), "{err}");
    }

    #[test]
    fn a_child_may_not_be_pointed_at_a_file_with_capabilities() {
        // B2's other half: with an empty bounding set the `execve` itself fails,
        // so the boot refuses and says which variable to point elsewhere.
        let err = placement(
            Path::new("/proc/self/exe"),
            Path::new("/usr/local/bin/blastit-init"),
            &[PathBuf::from("/usr/local/bin/blastit")],
            true,
            distinct,
            caps_of(&["/usr/local/bin/blastit-init"]),
        )
        .unwrap_err();
        assert!(err.contains("bounding set is empty"), "{err}");
        assert!(err.contains(EXEC), "{err}");

        // M1: without `cap_setpcap` the bounding set survives, the `execve`
        // works, and `no_new_privs` is what stops the grant — so the same
        // deployment is allowed, and it was allowed before D6 too.
        assert!(
            placement(
                Path::new("/proc/self/exe"),
                Path::new("/usr/local/bin/blastit-init"),
                &[PathBuf::from("/usr/local/bin/blastit")],
                false,
                distinct,
                caps_of(&["/usr/local/bin/blastit-init"]),
            )
            .is_ok()
        );
    }

    /// A deployment that starts the *copy* lists it among the names of its
    /// binary (`BLASTIT_EXEC`): that name is init's own file, so the capability
    /// on it is the point of the deployment, not a mistake.
    #[test]
    fn init_own_copy_among_the_names_is_not_a_refusal() {
        let init = "/usr/local/bin/blastit-init";
        let placement = placement(
            Path::new(init),
            Path::new("/usr/local/bin/blastit"),
            &[
                // What `BLASTIT_EXEC` and its resolved target are, under this
                // deployment: init's own file.
                PathBuf::from(init),
                // The shared binary, capability-free.
                PathBuf::from("/usr/local/bin/blastit"),
            ],
            true,
            same(&[(init, init)]),
            caps_of(&[init]),
        )
        .unwrap();
        assert_eq!(placement.init, Path::new(init));
    }

    #[test]
    fn a_rootless_layout_is_accepted_and_keeps_the_capability_free_binary() {
        // init runs from the copy, which carries the capabilities; the binary
        // `BLASTIT_EXEC` names has none: exactly the deployment this step ships
        // (B1), and the child runs it (`--role=dbproxy`).
        let placement = placement(
            Path::new("/usr/local/bin/blastit-init"),
            Path::new("/usr/local/bin/blastit"),
            &[PathBuf::from("/usr/local/bin/blastit")],
            true,
            same(&[]),
            caps_of(&["/usr/local/bin/blastit-init"]),
        )
        .unwrap();
        assert_eq!(placement.init, Path::new("/usr/local/bin/blastit-init"));
        assert_eq!(placement.dbproxy, Path::new("/usr/local/bin/blastit"));
    }

    #[test]
    fn same_file_follows_symlinks() {
        let dir = TempDir::new("same-file");
        let real = dir.file("python3.13", 0o755);
        let link = dir.path.join("python3");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(same_file(&real, &link), "a symlink is its target");
        assert!(same_file(&link, &real));
        assert!(same_file(&real, &real));
        let other = dir.file("other", 0o755);
        assert!(!same_file(&real, &other));
        assert!(!same_file(&real, Path::new("/nonexistent")));
    }

    #[test]
    fn has_file_caps_tells_absence_from_ignorance() {
        let dir = TempDir::new("caps-absence");
        let plain = dir.file("plain", 0o755);
        assert!(!has_file_caps(&plain).unwrap(), "no attribute is an answer");
        let missing = has_file_caps(Path::new("/nonexistent/blastit")).unwrap_err();
        assert!(
            missing.to_string().contains("/nonexistent/blastit"),
            "an unreadable attribute is an error, not a `false`: {missing}"
        );
        // The two errnos that are an *answer* rather than a failure: no
        // attribute, or a filesystem with no extended attributes at all.
        assert_eq!(caps_from_errno(Some(libc::ENODATA)), Some(false));
        assert_eq!(caps_from_errno(Some(libc::ENOTSUP)), Some(false));
        assert_eq!(caps_from_errno(Some(libc::EPERM)), None);
        assert_eq!(caps_from_errno(Some(libc::ENOENT)), None);
        assert_eq!(caps_from_errno(None), None);
        // `/proc` answers one of the two (it has no `security.capability`).
        assert!(
            !has_file_caps(Path::new("/proc/self/status")).unwrap(),
            "nothing in `/proc` carries file capabilities"
        );
    }

    #[test]
    fn has_file_caps_sees_one_that_is_there() {
        if !rustix::process::geteuid().is_root() {
            println!("skipped: setting security.capability needs CAP_SETFCAP");
            return;
        }
        // Write the attribute the kernel reads at an `execve`: a `vfs_cap_data`
        // with `VFS_CAP_REVISION_2 | VFS_CAP_FLAGS_EFFECTIVE`.
        let dir = TempDir::new("caps-there");
        let path = dir.file("capable", 0o755);
        let mut value = [0_u8; 20];
        value[..4].copy_from_slice(&0x0200_0001_u32.to_le_bytes());
        value[4..8].copy_from_slice(&1_u32.to_le_bytes());
        let name = c"security.capability";
        let path_c = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `setxattr` takes a NUL-terminated path, a NUL-terminated name,
        // a buffer of the length given and a flags word.
        let wrote = unsafe {
            libc::setxattr(
                path_c.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        assert_eq!(wrote, 0, "setxattr: {}", io::Error::last_os_error());
        assert!(has_file_caps(&path).unwrap(), "an attribute that is there");
        std::fs::remove_file(&path).unwrap();
        assert!(
            !has_file_caps(&path).unwrap_or(false),
            "and gone when the file is"
        );
    }

    #[test]
    fn protected_accepts_root_or_the_service_uid_and_nothing_writable() {
        let root_owned = Facts {
            owner: 0,
            writable_by_others: false,
            directory_writable_by_others: false,
        };
        assert_eq!(protected(&root_owned, 1000), Ok(()));
        let ours = Facts {
            owner: 1000,
            ..root_owned
        };
        assert_eq!(protected(&ours, 1000), Ok(()));
        let someone_else = Facts {
            owner: 1001,
            ..root_owned
        };
        let err = protected(&someone_else, 1000).unwrap_err();
        assert!(err.contains("owned by uid 1001"), "{err}");
        let writable = Facts {
            writable_by_others: true,
            ..root_owned
        };
        let err = protected(&writable, 1000).unwrap_err();
        assert!(err.contains("group- or other-writable"), "{err}");
        let directory = Facts {
            directory_writable_by_others: true,
            ..root_owned
        };
        let err = protected(&directory, 1000).unwrap_err();
        assert!(err.contains("/tmp"), "{err}");
        let both = Facts {
            writable_by_others: true,
            directory_writable_by_others: true,
            owner: 1001,
        };
        let err = protected(&both, 1000).unwrap_err();
        assert!(err.contains("owned by"), "{err}");
        assert!(err.contains("writable"), "{err}");
    }

    #[test]
    fn the_facts_of_a_file_read_its_owner_and_walk_its_directories() {
        let dir = TempDir::new("facts");
        let file = dir.file("blastit", 0o755);
        let facts = facts_of(&file).unwrap();
        assert_eq!(facts.owner, init_uid());
        assert!(!facts.writable_by_others);
        let open = dir.file("open", 0o666);
        assert!(facts_of(&open).unwrap().writable_by_others);
        // A world-writable directory *above* the file: `/tmp` is the one the
        // message names, and a copy of it here says the same thing.
        let shared = dir.dir("shared", 0o777);
        let inside = shared.join("blastit");
        std::fs::write(&inside, b"#!/bin/sh\n").unwrap();
        // The mode the umask would give it is not the point: 0644 is what the
        // assertion below is about.
        std::fs::set_permissions(&inside, std::fs::Permissions::from_mode(0o644)).unwrap();
        let facts = facts_of(&inside).unwrap();
        assert!(!facts.writable_by_others, "the file itself is 0644");
        assert!(
            facts.directory_writable_by_others,
            "its directory is writable by anyone"
        );
        assert!(
            !plain_is_protected(&inside, init_uid()).is_ok(),
            "and that is a refusal"
        );
    }

    // ------------------------------------------------ production provenance

    /// A little-endian ELF shared object with one `.dynamic` section whose
    /// entry is `tag` → `search`, and nothing else.
    ///
    /// `tag = 0` writes `DT_NULL` first, which is an object with **no** search
    /// path at all. Layout: the ELF header, the dynamic string table, the
    /// dynamic array, then three section headers (null, `SHT_STRTAB`,
    /// `SHT_DYNAMIC` linked to it). Nothing is loaded: the walk reads these bytes
    /// the way it reads a distro `libphp`.
    fn object_with_search_path(wide: bool, tag: u64, search: &str) -> Vec<u8> {
        let head_len = if wide { 64usize } else { 52 };
        let shentsize = if wide { 64usize } else { 40 };
        let entry_size = if wide { 16usize } else { 8 };

        let mut strtab = vec![0u8];
        let value = strtab.len() as u64;
        strtab.extend_from_slice(search.as_bytes());
        strtab.push(0);

        let strtab_off = head_len as u64;
        let dynamic_off = strtab_off + strtab.len() as u64;
        let dynamic_len = (2 * entry_size) as u64;
        let shoff = dynamic_off + dynamic_len;

        let mut file = vec![0u8; (shoff + 3 * shentsize as u64) as usize];
        file[..4].copy_from_slice(b"\x7fELF");
        file[4] = if wide { 2 } else { 1 };
        file[5] = 1;
        file[6] = 1;
        if wide {
            file[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
            file[0x3a..0x3c].copy_from_slice(&(shentsize as u16).to_le_bytes());
            file[0x3c..0x3e].copy_from_slice(&3u16.to_le_bytes());
        } else {
            file[0x20..0x24].copy_from_slice(&(shoff as u32).to_le_bytes());
            file[0x2e..0x30].copy_from_slice(&(shentsize as u16).to_le_bytes());
            file[0x30..0x32].copy_from_slice(&3u16.to_le_bytes());
        }
        let at = strtab_off as usize;
        file[at..at + strtab.len()].copy_from_slice(&strtab);

        let at = dynamic_off as usize;
        if wide {
            file[at..at + 8].copy_from_slice(&(tag as i64).to_le_bytes());
            file[at + 8..at + 16].copy_from_slice(&value.to_le_bytes());
        } else {
            file[at..at + 4].copy_from_slice(&(tag as u32).to_le_bytes());
            file[at + 4..at + 8].copy_from_slice(&(value as u32).to_le_bytes());
        }

        // Section 1 is the string table, section 2 the dynamic array linked to
        // it; section 0 stays the all-zero null section.
        let at = shoff as usize + shentsize;
        write_section(
            &mut file,
            wide,
            at,
            &SectionHeader {
                kind: SHT_STRTAB,
                offset: strtab_off,
                size: strtab.len() as u64,
                link: 0,
                entsize: 0,
            },
        );
        let at = at + shentsize;
        write_section(
            &mut file,
            wide,
            at,
            &SectionHeader {
                kind: SHT_DYNAMIC,
                offset: dynamic_off,
                size: dynamic_len,
                link: 1,
                entsize: entry_size as u64,
            },
        );
        file
    }

    /// The fields one section header needs, grouped because they are one
    /// record: `rust-standards` §3's rule about a parameter clump.
    struct SectionHeader {
        kind: u32,
        offset: u64,
        size: u64,
        link: u32,
        entsize: u64,
    }

    /// Writes `header` as the section header at `at`.
    fn write_section(file: &mut [u8], wide: bool, at: usize, header: &SectionHeader) {
        file[at + 4..at + 8].copy_from_slice(&header.kind.to_le_bytes());
        if wide {
            file[at + 0x18..at + 0x20].copy_from_slice(&header.offset.to_le_bytes());
            file[at + 0x20..at + 0x28].copy_from_slice(&header.size.to_le_bytes());
            file[at + 0x28..at + 0x2c].copy_from_slice(&header.link.to_le_bytes());
            file[at + 0x38..at + 0x40].copy_from_slice(&header.entsize.to_le_bytes());
        } else {
            file[at + 0x10..at + 0x14].copy_from_slice(&(header.offset as u32).to_le_bytes());
            file[at + 0x14..at + 0x18].copy_from_slice(&(header.size as u32).to_le_bytes());
            file[at + 0x18..at + 0x1c].copy_from_slice(&header.link.to_le_bytes());
            file[at + 0x24..at + 0x28].copy_from_slice(&(header.entsize as u32).to_le_bytes());
        }
    }

    /// The search path is read from the file, for `DT_RPATH`, `DT_RUNPATH` and
    /// both ELF classes — and an object with neither tag reports none.
    #[test]
    fn dynamic_search_paths_reads_both_tags_and_both_classes() {
        let dir = safe_dir("dtags").expect("a directory only its owner can write");
        for (name, image, expected) in [
            (
                "elftwo",
                object_with_search_path(false, DT_RPATH, "/opt/one:/opt/two"),
                vec!["/opt/one".to_string(), "/opt/two".to_string()],
            ),
            (
                "elfwide",
                object_with_search_path(true, DT_RUNPATH, "/opt/three"),
                vec!["/opt/three".to_string()],
            ),
            (
                "none",
                object_with_search_path(true, 0, "/opt/never"),
                Vec::<String>::new(),
            ),
        ] {
            let path = dir.path.join(name);
            std::fs::write(&path, &image).unwrap();
            assert_eq!(
                dynamic_search_paths(&path).unwrap(),
                expected,
                "{}",
                path.display()
            );
        }
        // Not an ELF object is an *error*, never "no search path": a check that
        // cannot be made must not be read as an answer.
        let text = dir.file("text", 0o644);
        let err = dynamic_search_paths(&text).unwrap_err();
        assert!(err.contains("no ELF magic"), "{err}");
    }

    /// `$ORIGIN` is the one token the boot can resolve; everything else is
    /// refused rather than guessed.
    #[test]
    fn a_search_entry_is_resolved_only_when_the_boot_can() {
        let origin = Path::new("/usr/lib");
        assert_eq!(
            resolve_search_entry("$ORIGIN/../lib", Some(origin)),
            Some(PathBuf::from("/usr/lib/../lib"))
        );
        assert_eq!(
            resolve_search_entry("${ORIGIN}/lib", Some(origin)),
            Some(PathBuf::from("/usr/lib/lib"))
        );
        assert_eq!(
            resolve_search_entry("/opt/lib", None),
            Some(PathBuf::from("/opt/lib"))
        );
        for unresolved in ["", "relative/lib", "$LIB", "$PLATFORM/lib", "$ORIGIN/lib"] {
            assert_eq!(
                resolve_search_entry(unresolved, Some(origin)),
                if unresolved == "$ORIGIN/lib" {
                    Some(PathBuf::from("/usr/lib/lib"))
                } else {
                    None
                },
                "{unresolved:?}"
            );
        }
        assert_eq!(resolve_search_entry("$ORIGIN/lib", None), None);
    }

    /// A `DT_RUNPATH` into a directory anyone can write is refused, naming the
    /// entry and the fix; the same object in a protected directory is not.
    #[test]
    fn a_writable_search_path_is_refused_and_a_protected_one_is_not() {
        let dir = safe_dir("dtags-policy").expect("a directory only its owner can write");
        let shared = dir.dir("shared", 0o777);
        let entry = shared.display().to_string();

        let writable = dir.path.join("writable.so");
        std::fs::write(&writable, object_with_search_path(true, DT_RUNPATH, &entry)).unwrap();
        let claim = Claim::new("the embedded shim", writable.clone());
        let err = Provenance {
            init_uid: init_uid(),
            objects: vec![claim.clone()],
            trees: Vec::new(),
        }
        .check_objects()
        .unwrap_err();
        assert!(err.contains("the embedded shim"), "{err}");
        assert!(err.contains(&entry), "the refusal names the entry: {err}");
        assert!(err.contains("DT_RPATH/DT_RUNPATH"), "{err}");
        assert!(err.contains("-rpath"), "and the fix: {err}");

        // The same object with no search path at all: accepted, so the refusal
        // above is the search path and not the empty file capabilities.
        let clean = dir.path.join("clean.so");
        std::fs::write(&clean, object_with_search_path(true, 0, "")).unwrap();
        assert!(
            Provenance {
                init_uid: init_uid(),
                objects: vec![Claim::new("the embedded shim", clean)],
                trees: Vec::new(),
            }
            .check_objects()
            .is_ok()
        );
    }

    /// A tree anyone can write is refused with the path and the `chmod`; the
    /// same tree at `0755` is accepted. The **bits** are the decision.
    #[test]
    fn a_writable_tree_is_refused_and_a_protected_one_is_not() {
        let dir = safe_dir("tree").expect("a directory only its owner can write");
        let tree = dir.dir("app", 0o755);
        let checked = |path: &Path| {
            Provenance {
                init_uid: init_uid(),
                objects: Vec::new(),
                trees: vec![Claim::new(
                    "route \"app\": entrypoint php \"public/index.php\"",
                    path,
                )],
            }
            .check_trees()
        };
        assert!(checked(&tree).is_ok(), "0755, owned by init's uid");
        std::fs::set_permissions(&tree, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = checked(&tree).unwrap_err();
        assert!(err.contains(&tree.display().to_string()), "{err}");
        // The refusal names the claim the way a `Blastfile` writes it: the
        // `entrypoint php` the tree belongs to, not the retired per-route table.
        assert!(
            err.contains("route \"app\": entrypoint php \"public/index.php\""),
            "{err}"
        );
        assert!(!err.contains("[route."), "{err}");
        assert!(err.contains("could be replaced by another uid"), "{err}");
        assert!(err.contains("chown"), "the fix is a chown/chmod: {err}");
    }

    /// A file capability is refused before anything can hand the file to a
    /// child whose bounding set is empty.
    ///
    /// The capability predicate is **injected** the way [`placement`] takes
    /// `caps_of`: `setxattr` on `security.capability` needs `CAP_SETFCAP`, which
    /// no unprivileged test has, so a `geteuid().is_root()` early return here
    /// would leave this refusal with no witness in `make check` (`MB.3.43`).
    /// The real attribute — that `has_file_caps` sees one that is there — is
    /// [`has_file_caps_sees_one_that_is_there`]'s half, which is root-only by
    /// nature because the attribute is what needs the capability.
    #[test]
    fn a_capable_object_is_refused() {
        let dir = safe_dir("caps-object").expect("a directory only its owner can write");
        let path = dir.path.join("shim.so");
        std::fs::write(&path, object_with_search_path(true, 0, "")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let provenance = Provenance {
            init_uid: init_uid(),
            objects: vec![Claim::new("the embedded shim", path.clone())],
            trees: Vec::new(),
        };
        // The accepted shape first: the same file with no capability, read
        // through the boot's own predicate.
        assert!(
            provenance.check_objects().is_ok(),
            "the fixture is a 0755 object with no attribute"
        );
        let err = provenance
            .check_objects_with(caps_of(&[path.to_str().expect("a UTF-8 fixture path")]))
            .expect_err("a capable object must be refused");
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert!(err.contains("file capabilities"), "{err}");
        assert!(err.contains("setcap -r"), "and the fix: {err}");
        // And an attribute that cannot be read is a refusal too, never "no
        // capabilities" (`caps_from_errno`'s own rule, applied by the gate).
        let err = provenance
            .check_objects_with(|_| Err(io::Error::other("no xattr for you")))
            .expect_err("an unreadable attribute must refuse");
        assert!(err.contains("cannot be read"), "{err}");
        assert!(err.contains("no xattr for you"), "{err}");
    }
}
