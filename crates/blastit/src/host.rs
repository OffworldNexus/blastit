//! How blastit was installed, and therefore how it starts itself again
//! (§2.1, §4).
//!
//! Every stage after stage 0 is a new program image: the `execve` barrier that
//! hides the real credentials also means blastit has to be able to name itself.
//! This module is the whole of that knowledge, and it is the only thing the
//! packaging changes:
//!
//! * the **standalone binary** re-executes `/proc/self/exe`, exactly as §4
//!   describes;
//! * a **wheel install** is a Python program: `blastit` is the console script
//!   of the interpreter the wheel was installed into, `blastit._blastit.main()`
//!   runs inside it, and `/proc/self/exe` is that interpreter. Stage 1 is
//!   therefore re-executed as `python -I -m blastit` — the *same* interpreter,
//!   which is what makes the application's dependencies importable, because the
//!   `site-packages` CPython finds next to its own executable are the venv's.
//!
//! dbproxy is the exception either way. It holds the real credentials (§2.2),
//! so it must not run an interpreter at all: a wheel ships it as a separate
//! Python-free binary (`blastit/_bin/blastit-dbproxy`, mode `0755`) and stage 0
//! `execve`s that instead of itself. The standalone binary keeps playing the
//! role itself, with the `--role=dbproxy` it has always used.
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

use blastit_sys::exe_path;

/// The environment variable that names a standalone install's capability-free
/// binary (D6/B2).
///
/// Read by the standalone binary only; a wheel always `execve`s the
/// `blastit-dbproxy` it ships.
pub const EXEC: &str = "BLASTIT_EXEC";

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
    /// (`os.fsencode`): empty for the standalone binary, whose stage 1 embeds
    /// CPython.
    sys_path: Vec<Vec<u8>>,
    /// D6: how this installation was made, which decides what a capability
    /// mistake tells an operator to fix.
    install: Install,
    /// D6: the capability-free counterpart of the file that may carry the
    /// capabilities — the interpreter a wheel's `sys.prefix` names, or the
    /// binary `BLASTIT_EXEC` names — **and every name it can be started by**.
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

/// Which install this is (D6).
///
/// It changes nothing about the roles, only about the sentence a capability
/// mistake produces: the file to `setcap` is an interpreter under a wheel and a
/// binary under a standalone install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    /// The standalone binary, started directly (§2.1).
    Binary,
    /// A wheel install: a console script on a project's interpreter (D2).
    Wheel,
}

impl Install {
    /// The noun an operator reads: what has to be copied and `setcap`ed.
    pub const fn file(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Wheel => "interpreter",
        }
    }

    /// The variable that names the capability-free file, for the standalone
    /// install; a wheel finds its own.
    pub const fn naming(self) -> &'static str {
        match self {
            Self::Binary => "BLASTIT_EXEC",
            Self::Wheel => "sys.prefix/bin/python (the console script's own interpreter)",
        }
    }
}

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
            install: Install::Binary,
            plain,
        }
    }

    /// A wheel install: the extension is running inside `executable`, and
    /// dbproxy is the Python-free binary the wheel ships next to it.
    ///
    /// `sys_path` is the interpreter's import path at the moment the console
    /// script started — `PYTHONPATH`, the working directory, the venv's
    /// `site-packages` — as bytes (`os.fsencode`, so a path that is not valid
    /// UTF-8 survives). Stage 1 is re-executed with `-I`, which drops the first
    /// two, so stage 0 records them here and stage 1 puts them back (§4).
    ///
    /// `plain` is the interpreter a *child* must use — `sys.prefix/bin/python`,
    /// which is what a project's own `python` is (D6/B1). It is the same file as
    /// `executable` under an ambient-capability deployment, and a *different*
    /// one when the deployment gave the capabilities to a dedicated copy.
    pub fn wheel(
        executable: PathBuf,
        dbproxy: PathBuf,
        plain: Vec<PathBuf>,
        sys_path: Vec<Vec<u8>>,
    ) -> Self {
        Self {
            stage1: Reexec {
                program: executable,
                // `-I`: stage 1 is `execve`'d with the application's
                // environment, and the isolation keeps that environment from
                // changing how the interpreter starts (§4). The venv is still
                // found, because it is found next to the executable, and
                // `sys_path` is restored explicitly.
                prefix: vec![
                    OsString::from("-I"),
                    OsString::from("-m"),
                    OsString::from("blastit"),
                ],
            },
            dbproxy: Reexec::program(&dbproxy),
            sys_path,
            install: Install::Wheel,
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

    /// Which install this is (D6).
    pub fn install(&self) -> Install {
        self.install
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
        // validating by name: a wheel's capability-free interpreter comes from
        // the installation's own detection, which checks it is there.
        if self.install == Install::Binary
            && let Some(binary) = self.plain.first()
        {
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
        let child = match self.install {
            // A standalone install plays every role itself, and `BLASTIT_EXEC`
            // is the capability-free copy of it (D6/B2).
            Install::Binary => self
                .plain
                .first()
                .cloned()
                .unwrap_or_else(|| self.dbproxy.program.clone()),
            // A wheel's dbproxy runs no interpreter and needs no capability, so
            // the shipped binary is always a child program (D6/B1).
            Install::Wheel => self.dbproxy.program.clone(),
        };
        placement(
            self.install,
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
///   not be the file the children use ([`Install::Wheel`]'s interpreter,
///   [`Install::Binary`]'s binary), compared by device and inode rather than by
///   name, so `python`, `python3` and a symlink chain all count (B1);
/// * **no** name of the shared interpreter may carry a file capability: the
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
    install: Install,
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
    // Every name of the shared interpreter must be capability-free: whichever
    // one a deployment starts, the capability belongs to init's file alone (B1).
    // A name that *is* init's file (the dedicated copy the deployment starts, and
    // `sys.executable` with it) is not a shared name, and may carry them.
    for name in plain {
        if same_file(name, init) {
            continue;
        }
        if caps(name)? {
            return Err(capable_copy_advice_for(install, name, init));
        }
    }
    if caps(init)? {
        // The capable file must be init's alone. What it must *not* be is the
        // file the shared interpreter is: that is the case where the boot cannot
        // name a capability-free file other than init's own image, either because
        // it cannot name one at all or because every name it has *is* that image.
        let shared: Vec<&PathBuf> = plain.iter().filter(|name| !same_file(name, init)).collect();
        if shared.is_empty() {
            return Err(match plain.first() {
                None => format!(
                    "{} carries file capabilities and this installation cannot name the \
                     capability-free {} the children must use instead: {}",
                    init.display(),
                    install.file(),
                    install.naming()
                ),
                Some(named) => capable_copy_advice_for(install, named, init),
            });
        }
    }
    if drops_bounding && caps(child)? {
        return Err(format!(
            "{} carries file capabilities, and the children of this boot cannot \
             `execve` such a file at all (their bounding set is empty): point {} at \
             the capability-free copy",
            child.display(),
            install.naming()
        ));
    }
    Ok(Placement {
        init: init.to_path_buf(),
        dbproxy: child.to_path_buf(),
    })
}

/// The sentence an operator gets when a file that is *not* init's copy carries
/// the capabilities (D6/B1).
fn capable_copy_advice_for(install: Install, capable: &Path, init: &Path) -> String {
    format!(
        "{} carries file capabilities, and init runs from {} instead: every process of that \
         uid that can run it becomes init.\n{}",
        capable.display(),
        init.display(),
        capable_copy_advice(install, capable)
    )
}

/// The recipe that turns the shared file into init's private copy (D6/B1, B2).
///
/// `shared` is the file that has the capabilities and must not: a wheel's
/// interpreter, a standalone install's binary, or any of the names either can be
/// started by — the file itself is what the kernel looks at, so a symlink chain
/// makes them all one mistake.
fn capable_copy_advice(install: Install, shared: &Path) -> String {
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
    let tail = match install {
        Install::Wheel => format!(
            "  then start that copy (`{} -m blastit`) and leave {} capability-free: a copied \
             interpreter has to stay in the same directory, because CPython finds its stdlib \
             and its venv from the executable's own path",
            copy.display(),
            shared.display()
        ),
        Install::Binary => format!(
            "  then start the copy, and set {EXEC}={} so that `--role=dbproxy` runs the \
             capability-free binary",
            resolved.display()
        ),
    };
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

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

    /// A [`TempDir`] whose paths no other uid can write, or `None` when this
    /// environment has none (an odd `$HOME`, a shared build directory): the tests
    /// that need one print and pass, the way a root-only test does.
    fn safe_dir(tag: &str) -> Option<TempDir> {
        let dir = TempDir::new(tag);
        let probe = dir.file("probe", 0o755);
        match plain_is_protected(&probe, init_uid()) {
            Ok(()) => Some(dir),
            Err(err) => {
                println!(
                    "skipped: no directory only its owner can write, in this environment ({err})"
                );
                None
            }
        }
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
        assert_eq!(host.install(), Install::Binary);
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
        let Some(dir) = safe_dir("rootless-binary") else {
            return;
        };
        let plain = dir.file("blastit", 0o755);
        // SAFETY: `env()` is held, so no other test reads or writes the variable.
        unsafe { std::env::set_var(EXEC, &plain) };
        let host = Host::binary();
        assert_eq!(host.install(), Install::Binary);
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

    #[test]
    fn a_wheel_reexecutes_the_interpreter_and_the_dbproxy_binary() {
        let site_packages = "/app/.venv/lib/python3.14/site-packages/blastit";
        let interpreter = PathBuf::from("/app/.venv/bin/python");
        let host = Host::wheel(
            interpreter.clone(),
            PathBuf::from(format!("{site_packages}/_bin/blastit-dbproxy")),
            vec![interpreter.clone()],
            vec![b"/app".to_vec(), vec![b'/', b'a', 0xff, b'p']],
        );
        assert_eq!(
            host.stage1().program,
            Path::new("/app/.venv/bin/python"),
            "stage 1 is the interpreter the wheel was installed into"
        );
        assert_eq!(
            host.stage1().prefix,
            vec![
                OsString::from("-I"),
                OsString::from("-m"),
                OsString::from("blastit")
            ]
        );
        assert_eq!(
            host.dbproxy().program,
            Path::new(&format!("{site_packages}/_bin/blastit-dbproxy")),
            "dbproxy runs no interpreter"
        );
        assert!(host.dbproxy().prefix.is_empty());
        assert_eq!(
            host.sys_path(),
            [b"/app".to_vec(), vec![b'/', b'a', 0xff, b'p']],
            "the import path travels as bytes, invalid UTF-8 included"
        );
        assert_eq!(host.install(), Install::Wheel);
        // Nothing carries file capabilities here, so the wheel's dbproxy binary
        // is the child program, and the check passes (B1).
        // `placement` itself is exercised on real paths and with the pure
        // fnctions below: these paths are imaginary, and the check refuses what
        // it cannot read rather than assuming it is capability-free.
        assert!(host.placement(true).is_err());
    }

    /// The mistake B1 is about: `setcap` on the interpreter a project's `python`
    /// is, started under any of its names.
    #[test]
    fn every_name_of_a_capable_interpreter_is_refused() {
        let names = [
            "/usr/local/bin/python",
            "/usr/local/bin/python3",
            "/usr/local/bin/python3.13",
        ];
        for capable in names {
            // init runs from a *copy*, so the refusal can only come from the
            // capability on the shared file.
            let err = placement(
                Install::Wheel,
                Path::new("/usr/local/bin/python3.13-init"),
                Path::new("/w/_bin/blastit-dbproxy"),
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
            assert!(err.contains("-m blastit"), "{err}");
        }
    }

    /// A real `security.capability` (root, as `setcap` would write it) on a real
    /// file that `python`, `python3` and `python3.13` all resolve to: the check
    /// runs with the real `same_file` and the real `has_file_caps`.
    #[test]
    fn a_real_capability_on_the_shared_interpreter_is_refused_under_every_name() {
        if !rustix::process::geteuid().is_root() {
            println!("skipped: setting security.capability needs CAP_SETFCAP");
            return;
        }
        let Some(dir) = safe_dir("names") else {
            return;
        };
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
                Install::Wheel,
                &init,
                Path::new("/w/_bin/blastit-dbproxy"),
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
            Install::Wheel,
            &real,
            Path::new("/w/_bin/blastit-dbproxy"),
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
    fn a_shared_interpreter_that_is_init_itself_is_refused() {
        // The same file under two names: `BLASTIT_EXEC` (or `sys.executable`)
        // resolving to init's own file, by device and inode rather than by name.
        let err = placement(
            Install::Wheel,
            Path::new("/usr/local/bin/python3.13"),
            Path::new("/w/_bin/blastit-dbproxy"),
            &[PathBuf::from("/usr/local/bin/python")],
            true,
            same(&[("/usr/local/bin/python", "/usr/local/bin/python3.13")]),
            caps_of(&["/usr/local/bin/python3.13"]),
        )
        .unwrap_err();
        assert!(err.contains("dedicated copy"), "{err}");
        assert!(
            err.contains("/usr/local/bin/python") && err.contains("-init"),
            "the recipe is about the file the names reach: {err}"
        );
        assert!(err.contains("capabilities"), "{err}");
    }

    #[test]
    fn a_capability_that_cannot_be_named_is_refused_by_name() {
        let err = placement(
            Install::Wheel,
            Path::new("/opt/blastit/python-init"),
            Path::new("/w/_bin/blastit-dbproxy"),
            &[],
            true,
            distinct,
            caps_of(&["/opt/blastit/python-init"]),
        )
        .unwrap_err();
        assert!(err.contains("cannot name"), "{err}");
        assert!(err.contains("sys.prefix/bin/python"), "{err}");
    }

    #[test]
    fn a_capability_check_that_fails_is_a_refusal_not_an_assumption() {
        // An xattr that cannot be read must not be read as "no capabilities": a
        // sandbox that cannot prove something must not assume it.
        let err = placement(
            Install::Binary,
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
            Install::Binary,
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
                Install::Binary,
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
    /// interpreter (`sys.executable`): that name is init's own file, so the
    /// capability on it is the point of the deployment, not a mistake.
    #[test]
    fn init_own_copy_among_the_names_is_not_a_refusal() {
        let init = "/usr/local/bin/python3.13-init";
        let placement = placement(
            Install::Wheel,
            Path::new(init),
            Path::new("/w/_bin/blastit-dbproxy"),
            &[
                // What `sys.executable` and its resolved target are, under this
                // deployment: init's own file.
                PathBuf::from(init),
                // The shared interpreter, capability-free.
                PathBuf::from("/usr/local/bin/python3.13"),
                PathBuf::from("/usr/local/bin/python"),
            ],
            true,
            same(&[(init, init)]),
            caps_of(&[init]),
        )
        .unwrap();
        assert_eq!(placement.init, Path::new(init));
    }

    #[test]
    fn a_rootless_layout_is_accepted_and_keeps_the_wheels_own_dbproxy() {
        // init runs from the copy, which carries the capabilities; the shared
        // interpreter and the shipped dbproxy binary have none: exactly the
        // deployment this step ships (B1).
        let placement = placement(
            Install::Wheel,
            Path::new("/usr/local/bin/python3.13-init"),
            Path::new("/app/.venv/lib/python3.13/site-packages/blastit/_bin/blastit-dbproxy"),
            &[
                PathBuf::from("/usr/local/bin/python3.13"),
                PathBuf::from("/usr/local/bin/python"),
            ],
            true,
            same(&[]),
            caps_of(&["/usr/local/bin/python3.13-init"]),
        )
        .unwrap();
        assert_eq!(placement.init, Path::new("/usr/local/bin/python3.13-init"));
        assert_eq!(
            placement.dbproxy,
            Path::new("/app/.venv/lib/python3.13/site-packages/blastit/_bin/blastit-dbproxy")
        );
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
}
