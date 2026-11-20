//! §4: hand `dev`, `check`, `fmt` and `lsp` to the project's own `blastit`.
//!
//! The `blastit` on `PATH` is the machine's install. A project that pinned its
//! own — `uv add --dev blastit` writes `.venv/bin/blastit`, `composer require`
//! writes `vendor/bin/blastit`, `npm` writes `node_modules/.bin/blastit` — means
//! *that* one, so those four commands re-exec it with the same command line and
//! print nothing. `Serve` never does: `--preset`, `--stage=1`, `--role=*`,
//! `--internal-json`, `--help` and `--version` are answered by the binary that
//! was typed, because a deployment must be the artifact the operator installed
//! (`R6`–`R8`).
//!
//! # Why every decision is made on a descriptor
//!
//! `canonicalize` only *names* the binary. The directory that names it and the
//! binary itself are then opened with `O_NOFOLLOW|O_CLOEXEC`, every check reads
//! `fstat` of those two descriptors, and an ELF image is started as
//! `execveat(fd, "", …, AT_EMPTY_PATH)`: the file that was checked is the file
//! the kernel runs, with no second look at the path in between.
//!
//! A `#!` script has to be `execve`d by path — `binfmt_script` resolves the
//! interpreter itself — so `parent_fd` and `bin_fd` stay open across that call
//! (the inode cannot be recycled while a descriptor names it) and the kernel
//! keeps `$0`/`__file__` pointing at the project's own script.
//!
//! Keeping a descriptor open is **not** what makes that path safe: `execve(path)`
//! resolves the path again. What makes it safe is [`check_tree`], the rule that
//! no directory on the way to the binary can be written by anyone but its owner
//! (or, for `/tmp`, a root-owned sticky directory holding our own child), so no
//! other uid can substitute a file at any component. A future refactor must keep
//! the tree rule, not "keep the fd".
//!
//! # Skipping versus refusing
//!
//! An *auto-discovered* candidate that fails a check is skipped with one
//! `stderr` line and probing continues: a `0775` CI checkout or a
//! third-party-owned `node_modules` must never brick `blastit dev`.
//! `$VIRTUAL_ENV` belongs to that set — an activated venv is *ambient*, a shell
//! that happens to have one did not point at a binary. Only `$BLASTIT_BIN` fails
//! closed (`exit 1`), because that one really does name a path.

use std::env;
use std::ffi::{CString, OsStr};
use std::fs::{File, Metadata};
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::ptr;

use rustix::fs::{Mode, OFlags};
use rustix::process::geteuid;

use super::Command;

/// Non-empty once a handoff has happened, so the project's `blastit` does not
/// hop again (a `vendor/bin/blastit` shim of the same install would loop).
const DONE: &str = "BLASTIT_HANDOFF_DONE";

/// The documented escape hatch: any value but `""`/`"0"` turns handoff off.
const NO_HANDOFF: &str = "BLASTIT_NO_LOCAL_HANDOFF";

/// The venv the user activated. Ambient: when it is set there is **no**
/// `./.venv` fallback, so a shell outside the project cannot silently hop — but
/// a failure is a skip, exactly like the auto-discovered locations.
const VIRTUAL_ENV: &str = "VIRTUAL_ENV";

/// The one name that *does* point at a path (`BLASTIT_BIN`, the wheel's own
/// override): a failure is a refusal (`exit 1`), not a skip.
const EXPLICIT: &str = "BLASTIT_BIN";

/// One location that may hold the project's own `blastit`.
struct Candidate {
    path: PathBuf,
    /// `true` only for `$BLASTIT_BIN`, where the user named this exact path: a
    /// failure is then a refusal (`exit 1`), not a skip.
    explicit: bool,
}

/// A candidate that passed every check, kept as the two descriptors the checks
/// ran on so the exec cannot be pointed at anything else.
struct Verified {
    /// The canonical parent directory. Never read after verification, and that is
    /// the point: it stays open (this struct is borrowed for the whole of
    /// [`exec_verified`]) across the `execve` of a `#!` script, so the directory
    /// `binfmt_script` resolves through cannot have its inode recycled while the
    /// kernel runs the script.
    #[allow(dead_code)]
    parent: File,
    /// The canonical binary itself, `O_RDONLY|O_CLOEXEC|O_NOFOLLOW`.
    bin: File,
    path: PathBuf,
    dev: u64,
    ino: u64,
}

/// Hands `command` to the project's `blastit`, or answers why this image keeps
/// running instead.
///
/// Returns `Some(status)` only for a refusal this image must report: a
/// `$BLASTIT_BIN` that failed a check. On a valid handoff the process image is
/// replaced and this never returns.
pub(super) fn attempt(command: Command, argv: &[String]) -> Option<u8> {
    let name = name(command, argv)?;
    if disabled() {
        return None;
    }
    // A working directory that cannot be named has no project in it.
    let cwd = env::current_dir().ok()?;
    for candidate in candidates(&cwd) {
        // Absent is not a failure: the project simply has no `blastit` there.
        if std::fs::symlink_metadata(&candidate.path).is_err() {
            continue;
        }
        let verified = match verify(&candidate.path) {
            Ok(verified) => verified,
            Err(reason) => {
                if candidate.explicit {
                    return Some(refuse(name, &candidate.path, &reason));
                }
                eprintln!(
                    "blastit {name}: skipping local binary {} ({reason}); continuing with the \
                     next candidate or the global binary",
                    candidate.path.display()
                );
                continue;
            }
        };
        if is_this_image(&verified) {
            // The project's `blastit` *is* this image (a console script linked
            // to it): handing off would loop, and the global binary is exactly
            // what it would run.
            return None;
        }
        // Never returns on success. A file that cannot be started is the same
        // class of failure as a check that failed, with the same policy.
        let err = exec_verified(&verified, argv);
        let reason = err.to_string();
        if candidate.explicit {
            return Some(refuse(name, &verified.path, &reason));
        }
        eprintln!(
            "blastit {name}: cannot run local binary {} (exec failed: {reason}); continuing \
             with the next candidate or the global binary",
            verified.path.display()
        );
    }
    None
}

/// The word the user typed (`dev`, `check`, `test`, `fmt`, `lsp`), or `None` for
/// the two shapes that never hand off.
///
/// It comes from `argv`, not from [`Command`]: `blastit test` is `Check`, and a
/// handoff that reported `blastit check:` would name a command nobody typed.
fn name(command: Command, argv: &[String]) -> Option<&str> {
    match command {
        Command::Dev | Command::Check | Command::Fmt | Command::Lsp => {
            argv.first().map(String::as_str)
        }
        Command::Serve | Command::InternalJson => None,
    }
}

/// `§4`'s two switches: a handoff already happened, or the user turned it off.
fn disabled() -> bool {
    if env::var_os(DONE).is_some_and(|value| !value.is_empty()) {
        return true;
    }
    env::var_os(NO_HANDOFF).is_some_and(|value| !value.is_empty() && value != "0")
}

/// Writes the fixed refusal sentence and returns the status for it.
fn refuse(name: &str, path: &Path, reason: &str) -> u8 {
    eprintln!(
        "blastit {name}: refusing local handoff to {}: {reason}; set {NO_HANDOFF}=1 to use \
         the global blastit",
        path.display()
    );
    super::FAILURE
}

/// The candidate locations, in the order §4 pins.
///
/// `$VIRTUAL_ENV` replaces the auto-discovered `.venv` rather than joining it:
/// an activated venv is the environment the user is working in, and silently
/// preferring a `./.venv` next to the working directory would answer a different
/// question. `$BLASTIT_BIN` is appended last and is the only *explicit* one: the
/// user named a path there, so it fails closed instead of being skipped.
fn candidates(cwd: &Path) -> Vec<Candidate> {
    let mut candidates = Vec::with_capacity(4);
    let venv = env::var_os(VIRTUAL_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    candidates.push(Candidate {
        path: match venv {
            Some(venv) => venv.join("bin").join("blastit"),
            None => cwd.join(".venv/bin/blastit"),
        },
        explicit: false,
    });
    candidates.push(Candidate {
        path: cwd.join("vendor/bin/blastit"),
        explicit: false,
    });
    candidates.push(Candidate {
        path: cwd.join("node_modules/.bin/blastit"),
        explicit: false,
    });
    if let Some(named) = env::var_os(EXPLICIT).filter(|value| !value.is_empty()) {
        candidates.push(Candidate {
            path: PathBuf::from(named),
            explicit: true,
        });
    }
    candidates
}

/// §4's checks, all read from the two descriptors.
fn verify(path: &Path) -> Result<Verified, String> {
    let resolved =
        std::fs::canonicalize(path).map_err(|err| format!("the link cannot be resolved: {err}"))?;
    let (Some(parent_dir), Some(file_name)) = (resolved.parent(), resolved.file_name()) else {
        return Err("it has no parent directory".to_string());
    };
    let parent = blastit_sys::open_dir_nofollow(parent_dir)
        .map_err(|err| format!("cannot open {}: {err}", parent_dir.display()))?;
    let parent = File::from(parent);
    let bin =
        open_leaf(&parent, file_name).map_err(|err| format!("cannot open the binary: {err}"))?;
    let bin = File::from(bin);
    let meta = bin
        .metadata()
        .map_err(|err| format!("cannot stat the binary: {err}"))?;
    if !meta.is_file() {
        return Err("it is not a regular file".to_string());
    }
    let mode = meta.permissions().mode() & 0o7777;
    if mode & 0o111 == 0 {
        return Err(format!("it is mode {mode:04o}, which is not executable"));
    }
    let euid = geteuid().as_raw();
    if meta.uid() != euid && meta.uid() != 0 {
        return Err(format!(
            "it is owned by uid {}, not {euid} or 0",
            meta.uid()
        ));
    }
    if mode & 0o022 != 0 {
        return Err(format!(
            "it is mode {mode:04o}, so another user could have written it"
        ));
    }
    check_tree(&parent, parent_dir, meta.uid())?;
    Ok(Verified {
        parent,
        bin,
        path: resolved,
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// No directory on the way to `/` may be writable by anyone but its owner.
///
/// The exception is the one every real Linux has: a root-owned sticky directory
/// (`/tmp`) whose immediate child is owned by this uid or root — nothing another
/// user can remove or replace.
fn check_tree(parent: &File, parent_path: &Path, bin_uid: u32) -> Result<(), String> {
    let mut child_uid = bin_uid;
    let meta = parent
        .metadata()
        .map_err(|err| format!("cannot stat {}: {err}", parent_path.display()))?;
    check_dir(&meta, parent_path, child_uid)?;
    child_uid = meta.uid();
    for dir in parent_path.ancestors().skip(1) {
        let opened = blastit_sys::open_dir_nofollow(dir)
            .map_err(|err| format!("cannot open {}: {err}", dir.display()))?;
        let meta = File::from(opened)
            .metadata()
            .map_err(|err| format!("cannot stat {}: {err}", dir.display()))?;
        check_dir(&meta, dir, child_uid)?;
        child_uid = meta.uid();
    }
    Ok(())
}

/// One directory of [`check_tree`].
fn check_dir(meta: &Metadata, dir: &Path, child_uid: u32) -> Result<(), String> {
    let mode = meta.permissions().mode();
    if mode & 0o022 == 0 {
        return Ok(());
    }
    let sticky_root = meta.uid() == 0
        && mode & 0o1000 != 0
        && (child_uid == geteuid().as_raw() || child_uid == 0);
    if sticky_root {
        return Ok(());
    }
    Err(format!(
        "{} is mode {:04o}, so another user could replace the binary",
        dir.display(),
        mode & 0o7777
    ))
}

/// Is `verified` the running image? (`/proc/self/exe`'s own `(dev, ino)`.)
fn is_this_image(verified: &Verified) -> bool {
    match std::fs::metadata(blastit_sys::exe_path()) {
        Ok(meta) => meta.dev() == verified.dev && meta.ino() == verified.ino,
        Err(_) => false,
    }
}

/// Opens the binary *inside* the verified directory — never by its own path.
fn open_leaf(dir: &File, file_name: &OsStr) -> rustix::io::Result<OwnedFd> {
    rustix::fs::openat(
        dir,
        file_name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
}

/// §4 step 6: replace this image with the verified one.
///
/// `arguments` is the whole command line as this process received it — `dev`,
/// `--json`, … with no program name — and the project's own script becomes
/// `argv[0]`, so a `#!` script keeps `$0` and an ELF image keeps its own name.
///
/// Never returns on success. Everything this needs — `argv`, `envp`, the
/// finished strings, and both descriptors — outlives the call, which is the
/// whole contract of `execve`.
fn exec_verified(verified: &Verified, arguments: &[String]) -> io::Error {
    let mut magic = [0u8; 4];
    match verified.bin.read_at(&mut magic, 0) {
        Ok(read) if read == magic.len() => {}
        Ok(_) => {
            return io::Error::new(
                io::ErrorKind::InvalidData,
                "it is too short to be a program",
            );
        }
        Err(err) => return err,
    }
    let Ok(argv0) = CString::new(OsStr::new(&verified.path).as_bytes()) else {
        return io::Error::new(io::ErrorKind::InvalidInput, "the path contains a NUL byte");
    };
    let mut argv: Vec<CString> = Vec::with_capacity(arguments.len() + 1);
    argv.push(argv0);
    for argument in arguments {
        match CString::new(argument.as_bytes()) {
            Ok(argument) => argv.push(argument),
            Err(_) => {
                return io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "an argument contains a NUL byte",
                );
            }
        }
    }
    let entries = environment();
    let mut argv_ptrs: Vec<*mut libc::c_char> =
        argv.iter().map(|arg| arg.as_ptr().cast_mut()).collect();
    argv_ptrs.push(ptr::null_mut());
    let mut env_ptrs: Vec<*mut libc::c_char> = entries
        .iter()
        .map(|entry| entry.as_ptr().cast_mut())
        .collect();
    env_ptrs.push(ptr::null_mut());

    if magic == *b"\x7fELF" {
        // The kernel starts the **verified descriptor** —
        // `execveat(bin_fd, "", …, AT_EMPTY_PATH)` — so the file `fstat`ed above
        // is the file that runs and no path is resolved a second time.
        //
        // It goes through `libc::syscall` rather than the `libc::execveat`
        // wrapper because that wrapper is declared **only** for glibc
        // (`libc-0.2.190/src/unix/linux_like/linux/gnu/mod.rs:1205`); there is
        // no `execveat` in the musl module, which is what left every
        // `*-unknown-linux-musl` build of this crate uncompilable. The syscall
        // *number* is declared for both libcs (`…/musl/b64/x86_64/mod.rs:490`
        // and `…/musl/b64/aarch64/mod.rs:566`: `SYS_execveat` is 322 and 281),
        // and `AT_EMPTY_PATH` is the shared `…/linux_like/mod.rs:1218` `0x1000`
        // (`include/uapi/linux/fcntl.h`), so one code path serves both libcs
        // with no `cfg(target_env)` fork.
        //
        // Every argument is widened to `c_long`: `syscall` is C-variadic and the
        // libc reads each one back as `long`, so passing a `c_int` would leave
        // the upper half of the argument register unspecified.
        //
        // SAFETY: `argv_ptrs` and `env_ptrs` are NUL-terminated arrays of
        // pointers into `argv`/`entries`, all alive for the call; `bin_fd` is
        // open and refers to the ELF image checked above, and `c""` is the empty
        // path `AT_EMPTY_PATH` requires.
        unsafe {
            libc::syscall(
                libc::SYS_execveat,
                verified.bin.as_raw_fd() as libc::c_long,
                c"".as_ptr(),
                argv_ptrs.as_ptr(),
                env_ptrs.as_ptr(),
                libc::AT_EMPTY_PATH as libc::c_long,
            );
        }
    } else if magic[..2] == *b"#!" {
        let Ok(path) = CString::new(OsStr::new(&verified.path).as_bytes()) else {
            return io::Error::new(io::ErrorKind::InvalidInput, "the path contains a NUL byte");
        };
        // SAFETY: the same arrays, and `path` is a NUL-terminated path to the
        // very file whose descriptor was verified; the kernel resolves the
        // interpreter named by the `#!` line. The pointer arrays are the same
        // layout either way (`execve` wants them `*const`).
        unsafe {
            libc::execve(
                path.as_ptr(),
                argv_ptrs.as_ptr().cast::<*const libc::c_char>(),
                env_ptrs.as_ptr().cast::<*const libc::c_char>(),
            );
        }
    } else {
        return io::Error::new(
            io::ErrorKind::InvalidData,
            "it is neither an ELF image nor a `#!` script",
        );
    }
    io::Error::last_os_error()
}

/// The environment the new image gets: this one, plus [`DONE`].
fn environment() -> Vec<CString> {
    let mut entries = Vec::new();
    for (key, value) in env::vars_os() {
        if key == DONE {
            continue;
        }
        let mut entry = key.into_vec();
        entry.push(b'=');
        entry.extend_from_slice(&value.into_vec());
        if let Ok(entry) = CString::new(entry) {
            entries.push(entry);
        }
    }
    if let Ok(entry) = CString::new(format!("{DONE}=1")) {
        entries.push(entry);
    }
    entries
}
