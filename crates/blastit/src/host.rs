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
/// Read by the standalone binary only; a wheel `execve`s the same
/// `blastit/_bin/blastit` image it was started from.
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
