//! Whether the uids blastit drops to can reach what they need (§2.1, §5.3,
//! §7.2).
//!
//! blastit's other uids are not the owner's: the configuration, the interpreter
//! and the application's code belong to whoever installed them, and a view
//! process — or the child that runs dbproxy — runs as a uid of its own with no
//! supplementary groups. What it may read is therefore not a question the mode
//! bits answer on their own: the *group* class counts when the directory's gid is
//! the uid's own, an ACL counts, and the sticky bit protects nothing. The
//! failures that follow are all bad ones:
//!
//! * a view imports *lazily* — `stringprep` and `encodings.idna` behind Django's
//!   URL parsing, a template, a URLconf that is not in `BLASTIT_PRELOAD` — after
//!   the fork, from a directory nobody has looked at since the server started.
//!   `libpython`'s built-ins keep working, so the app starts, answers the health
//!   probe and serves some requests, and then one view raises
//!   `ModuleNotFoundError: No module named 'stringprep'` on one path and nothing
//!   else;
//! * the child that holds the credentials cannot `execve` dbproxy, and dies
//!   with `Permission denied (os error 13)` — before any of that, with nothing
//!   in the log saying which directory a deployment has to open;
//! * a path on `sys.path` that another uid can *write* — or a directory *above*
//!   one, since the uid can `rmdir` the entry and put its own back — lets one
//!   route plant a module another route imports: the sandbox is the uid, so code
//!   that runs under a route's own uid is code that route's grants apply to —
//!   but which route imports it is not the planter's choice to make.
//!
//! So the checks *ask the kernel*: [`probe`] forks a child, drops it to the uid
//! in question and calls `access(2)` on each path, which is the decision the
//! kernel will make inside a view. Only when a probe cannot run at all (a test
//! run as an ordinary user, a system without `/proc`) does the module fall back
//! to reading the mode bits, and the message says which of the two answered. The
//! child notes each path before it asks, and the parent's deadline — one second,
//! never blocking — kills it if it never answers: a `sys.path` that hangs is a
//! boot that stops and names the path (see [`TimedOut`]), not one that sits
//! there.
//!
//! # What is checked
//!
//! Every path a view imports *through*: the interpreter's prefixes, the standard
//! library, `site-packages`, and the directory the application's own module comes
//! from — read and traversed, [`unreadable`]. A child `execve`s *programs*, so
//! [`unexecutable`] asks the other question. And every path on the interpreter's
//! `sys.path` — its own entries, the working directory, whatever `PYTHONPATH` or
//! a `.pth` file added, a zip file on the path — must be one no uid blastit drops
//! to can write, *and* so must every directory above one up to `/`: [`writable`].
//! A sticky directory above an entry — `/tmp` is the one every deployment has —
//! is the exception, and only when the entry below it belongs to root or to init,
//! because the sticky bit stops a uid unlinking or renaming what it does not own.
//! The first path that fails is the one the message names, because it is the one
//! an operator has to change.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use blastit_sys::{Fork, Pid, pipe};

use crate::child::{read_one, wait_readable};

/// How long a probe waits for its child to answer.
///
/// The child only drops, `access(2)`s a handful of paths and writes ten bytes,
/// so a second is enormous — it is here so that a directory on a sick mount
/// cannot hang the boot.
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

/// What a uid blastit drops to cannot do with a path — or what another uid can —
/// in the words of the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Read it: list it, and read the files in it.
    Read,
    /// Traverse it: reach what is below it.
    Traverse,
    /// Execute it: `execve` a program a child has to run.
    Execute,
    /// Write to it: what no directory on `sys.path` may allow another uid.
    Write,
}

impl Access {
    /// The permission bit a path has to set for this access, which is what the
    /// mode-bit fallback reads.
    const fn bit(self) -> u32 {
        match self {
            Self::Read => 0o004,
            Self::Traverse | Self::Execute => 0o001,
            Self::Write => 0o002,
        }
    }

    /// The words the messages use for this access.
    pub const fn phrase(self) -> &'static str {
        match self {
            Self::Read => "not readable and traversable",
            Self::Traverse => "not traversable",
            Self::Execute => "not executable",
            Self::Write => "writable",
        }
    }
}

/// A path a uid blastit drops to cannot reach, and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocker {
    /// The path that blocks, as it was asked for.
    pub path: PathBuf,
    /// Its permission bits, exactly as `chmod` writes them.
    pub mode: u32,
    /// What that uid cannot do with it.
    pub access: Access,
    /// The uid the kernel was asked *as*, when the probe ran; `None` when the
    /// probe could not drop privileges and the mode bits had to answer.
    pub probed_uid: Option<u32>,
}

impl Blocker {
    /// The sentence an operator reads: what is wrong, and what to do about it.
    ///
    /// It names the path, its mode, and both fixes: moving what is unreachable
    /// somewhere every uid can read — for an interpreter, a system-wide Python,
    /// or `UV_PYTHON_INSTALL_DIR` for a uv-managed one — or opening the way to
    /// it, which is the whole path rather than the one directory, so the message
    /// says that too. Both symptoms are named, because the two callers refuse
    /// two different failures: a view losing a lazy import, and a child that
    /// cannot `execve` dbproxy. When the answer came from the kernel it says so,
    /// because a mode that looks wrong and is allowed (an ACL, a group class) is
    /// a different conversation.
    pub fn message(&self) -> String {
        let by = match self.probed_uid {
            Some(uid) => format!(
                "uid {uid} — what the kernel answers with `access(2)` in a process that \
                 dropped to it"
            ),
            None => "a uid blastit drops to — what the mode bits say, because this process \
                     could not drop privileges to ask the kernel"
                .to_string(),
        };
        format!(
            "{} (mode {:04o}) is {} by {}: a view process runs as a uid of its own, and \
             dbproxy as another, so what the owner can do with it says nothing about what \
             they can — and what breaks is a lazy import inside a view \
             (`ModuleNotFoundError: No module named 'stringprep'`, `encodings.idna`, …) or \
             an `execve` (`Permission denied (os error 13)`). Two fixes: move it somewhere \
             every uid blastit drops to can read — for an interpreter, a system-wide \
             Python, or `export UV_PYTHON_INSTALL_DIR=/opt/uv-python` before `uv python \
             install` — or open the way to it: `chmod o+rx {}` here and on every directory \
             above it",
            self.path.display(),
            self.mode,
            self.access.phrase(),
            by,
            self.path.display(),
        )
    }
}

/// A path on `sys.path` that another uid could write to.
///
/// The sandbox is the uid: code that runs in a view runs with that route's
/// grants, so a file planted in a directory on `sys.path` is not a hole in the
/// allowlist — but *which* route imports it is not the planter's choice, and a
/// route that plants a module another route imports is one route writing the
/// code of another. blastit refuses to start rather than serve that.
///
/// `path` is a directory (or a zip file, which is a file on `sys.path`) that has
/// to stop being writable; `entry` is the `sys.path` entry it is about, when
/// that is not `path` itself — an entry *below* a writable directory, one that
/// could be replaced, or created if it is not there yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Writable {
    /// What has to stop being writable.
    pub path: PathBuf,
    /// The `sys.path` entry, when it is not `path`: a path that does not exist
    /// yet and could be created in `path`.
    pub entry: Option<PathBuf>,
    /// The permission bits of `path`, exactly as `chmod` writes them.
    pub mode: u32,
    /// The uid the kernel was asked as, when the probe ran.
    pub probed_uid: Option<u32>,
    /// Which class let it through, for the message. It names the uid when the
    /// mode bits were the ones that answered, because that is what an operator has
    /// to look up.
    pub why: String,
}

impl Writable {
    /// The sentence an operator reads for a writable `sys.path` path.
    pub fn message(&self) -> String {
        let answer = match self.probed_uid {
            Some(uid) => format!("`access(2)` says uid {uid} may write it ({})", self.why),
            None => format!("the mode bits say so ({})", self.why),
        };
        let what = match &self.entry {
            Some(entry) => format!(
                "{} (mode {:04o}) is writable by another uid, and {} is on `sys.path` below \
                 it: that uid could take the entry over — replace it, or create it if it is \
                 not there yet — and every route would import whatever it left",
                self.path.display(),
                self.mode,
                entry.display(),
            ),
            None => format!(
                "{} (mode {:04o}) is writable by another uid",
                self.path.display(),
                self.mode,
            ),
        };
        format!(
            "{what}: {answer}. `sys.path` is how a view finds code — the interpreter's own \
             entries, the working directory blastit was started in, and whatever \
             `PYTHONPATH` or a `.pth` file added — and a view process runs as a uid of its \
             own, so a route could plant a module another route imports, and whichever \
             route imports it is not the planter's choice. Start blastit in the code tree, \
             next to the application — a directory only its owner (and root) can write — \
             or make this one so: `chmod go-w {}`",
            self.path.display(),
        )
    }
}

/// The uids blastit drops to, and the one a probe asks the kernel as.
///
/// Every process of the layout that is not init: http, the scheduler, dbproxy
/// and one per route. They are what "another uid" means for a directory on
/// `sys.path` (a group class counts when the directory's gid is one of them),
/// and the first view uid is who a probe speaks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DroppedTo {
    /// Every uid a dropped-to process can run as.
    pub uids: BTreeSet<u32>,
    /// The uid a probe drops to: a view's.
    pub probe: u32,
}

impl DroppedTo {
    /// The layout's uids, with `routes` view uids.
    pub fn of(layout: blastit_manifest::UidLayout, routes: usize) -> Self {
        let mut uids = BTreeSet::new();
        uids.insert(layout.http());
        uids.insert(layout.scheduler());
        uids.insert(layout.dbproxy());
        for index in 0..routes {
            uids.insert(layout.view(index));
        }
        Self {
            uids,
            probe: layout.view(0),
        }
    }
}
/// What a probe asks of every [`Target`] it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Read and traverse it: what the interpreter and the application are read
    /// through.
    Read,
    /// Execute it: what a child `execve`s.
    Execute,
    /// Write to it: what no path on `sys.path` — and no directory above one —
    /// may allow another uid.
    Write,
}

impl Probe {
    /// The `access(2)` bits the child asks for.
    const fn bits(self) -> libc::c_int {
        match self {
            Self::Read => libc::R_OK | libc::X_OK,
            Self::Execute => libc::X_OK,
            Self::Write => libc::W_OK,
        }
    }

    /// Whether a *successful* `access(2)` is the answer this probe is looking
    /// for.
    ///
    /// The read and execute checks ask "can this uid reach it", and a failure is
    /// what they are looking for. `Write` asks the opposite — "may this uid
    /// write it" — so the child reports the first target where `access(2)`
    /// *succeeds*, and "nothing was reported" means "nothing is writable".
    const fn wants_grant(self) -> bool {
        matches!(self, Self::Write)
    }

    /// Whether the sticky bit is read for this question.
    ///
    /// It is only ever an exemption, and only for the *write* question: a
    /// `/tmp`-style directory stops a uid unlinking or renaming an entry it does
    /// not own, which is the second way into a `sys.path` entry. For the entry
    /// itself nothing changes — the sticky bit does not stop a *new* file being
    /// created — so [`Target::child`] is what it is read for.
    const fn reads_sticky(self) -> bool {
        matches!(self, Self::Write)
    }
}

/// One thing a probe asks about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The path the child `access(2)`s.
    path: PathBuf,
    /// The path *below* this one, whose removal or renaming is the other way a
    /// dropped-to uid could take the entry over. `None` for the entry itself,
    /// where writing it — or creating a file inside it — is the way in.
    child: Option<PathBuf>,
    /// The `sys.path` entry this is about, when it is not `path` itself. For the
    /// message: it is the entry a deployment has to protect, and `path` is the
    /// directory that has to stop being writable.
    entry: Option<PathBuf>,
}

/// What a probe found: the target that answered it, and what the kernel and the
/// filesystem said about the path the answer is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Index into the targets that were probed.
    pub index: usize,
    /// How many directories *above* the answered path the path the refusal is
    /// about is; `0` means the answered path itself. Only the read and execute
    /// answers walk upwards.
    pub depth: usize,
    /// (mode) The permission bits of the path the refusal is about.
    pub mode: u32,
    /// The uid that owns it.
    pub uid: u32,
    /// Its gid.
    pub gid: u32,
    /// The errno `access(2)` gave, when it gave one (the write question is
    /// answered by success, which has no errno).
    pub errno: i32,
}

/// A check that had to be killed before it answered.
///
/// The child that asks the kernel is inside a syscall on [`TimedOut::path`] — a
/// hung mount, an unreachable network filesystem — and the mode bits would need
/// a `stat` through the same filesystem, so they are *not* consulted: the boot
/// refuses instead, and names the path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedOut {
    /// The path the child was on when the deadline passed.
    pub path: PathBuf,
}

impl TimedOut {
    /// The sentence an operator reads.
    pub fn message(&self) -> String {
        format!(
            "the visibility check on {} did not answer in {} s: the child blastit asks the \
             kernel with is inside a syscall on it, and blastit refuses to serve code it \
             could not check. Look at that path — a hung mount, an unreachable network \
             filesystem — and at what is above it",
            self.path.display(),
            PROBE_TIMEOUT.as_secs_f32(),
        )
    }
}

/// What a probe had to say.
#[derive(Debug)]
pub enum Spoken {
    /// The kernel says every target is fine.
    Fine,
    /// The kernel answers about one of them.
    Found(Verdict),
    /// The probe could not run at all — no pipe, no fork, or a child that could
    /// not drop to the uid — so the mode bits are the only answer there is. What
    /// went wrong is not something a caller can act on: the fallback is the same
    /// whichever way the kernel went unasked.
    NoProbe,
}

/// Asks the kernel, as `uid` would, about every target in `targets`.
///
/// The answer comes from a short-lived child: it `fork`s, drops to `uid` — the
/// same setgroups/setgid/setuid sequence a view goes through, so the *group*
/// class and any ACL are the ones a view would meet — and calls `access(2)` on
/// each target in turn. It notes each target it starts on, so a child that never
/// answers can be named, and it reports the mode, owner and group of the path
/// the answer is about, so the parent never has to `stat` at all.
///
/// `interpreter` says whether CPython is live in this image and the caller holds
/// the GIL, exactly as [`crate::forking::fork`] does: a wheel install forks under
/// the interpreter's fork protocol (§9.3), and the standalone binary's stage 0
/// has no interpreter at all.
///
/// # Errors
///
/// [`TimedOut`], when the child did not answer before [`PROBE_TIMEOUT`]: the
/// caller refuses rather than ask the same hung filesystem for the mode bits. The
/// child is killed and reaped without blocking; what is left of it, if anything,
/// is init's own supervisor's to reap.
pub fn probe(
    targets: &[Target],
    uid: u32,
    ask: Probe,
    interpreter: bool,
) -> Result<Spoken, TimedOut> {
    if targets.is_empty() {
        return Ok(Spoken::Fine);
    }
    let (read_end, write_end) = match pipe() {
        Ok(pair) => pair,
        Err(_) => return Ok(Spoken::NoProbe),
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let forked = crate::forking::fork(interpreter);
    match forked {
        Ok(Fork::Child) => {
            drop(read_end);
            // The child never returns: no unwinding, no destructors, nothing of
            // the parent's state that a `fork` could have copied half-written.
            write_answer(write_end.as_raw_fd(), uid, targets, ask);
            // SAFETY: `_exit` is async-signal-safe and never returns.
            unsafe { libc::_exit(0) }
        }
        Ok(Fork::Parent(pid)) => {
            drop(write_end);
            let answer = read_answer(read_end.as_raw_fd(), targets, deadline);
            reap(pid, deadline);
            answer
        }
        Err(_) => Ok(Spoken::NoProbe),
    }
}

/// The first byte of an answer.
const ANSWER_FINE: u8 = 0;
const ANSWER_NOTHING: u8 = 1;
const ANSWER_FOUND: u8 = 2;

/// The first byte of a "this is the target I am starting on" note.
const ANSWER_WORKING: u8 = 0xff;

/// The bytes of one answer: kind, index, depth, mode, uid, gid, errno.
const ANSWER_BYTES: usize = 1 + 4 * 6;

/// Writes the child's answer, noting each target before it is asked about.
///
/// The notes are what makes a timeout nameable: they are written *before* the
/// `access(2)` that may never return, so the parent always knows which path the
/// child was on.
fn write_answer(fd: RawFd, uid: u32, targets: &[Target], ask: Probe) {
    // The uid init runs as, which is one of the owners the sticky bit protects a
    // child from being replaced by. It has to be read before the drop.
    let init_uid = rustix::process::geteuid().as_raw();
    if let Err(error) = blastit_sys::drop_privileges(uid) {
        // The kernel cannot be asked from here at all: the caller falls back to
        // the mode bits.
        let errno = error.raw_os_error().unwrap_or(libc::EPERM);
        write_bytes(fd, &answer(ANSWER_NOTHING, 0, 0, 0, 0, 0, errno));
        return;
    }
    for (index, target) in targets.iter().enumerate() {
        write_bytes(fd, &working(index));
        let Some(name) = cstring(&target.path) else {
            continue;
        };
        // SAFETY: `access` takes a NUL-terminated path and a bitmask of R_OK,
        // W_OK, X_OK and F_OK; it reads no memory of ours.
        let granted = unsafe { libc::access(name.as_ptr(), ask.bits()) } == 0;
        if granted != ask.wants_grant() {
            // Not the answer this question is looking for: reachable, when the
            // question was "can this uid reach it", or unreachable, when it was
            // "may this uid write it".
            continue;
        }
        let errno = if ask.wants_grant() {
            // Success has no errno, and the write refusal does not report one.
            0
        } else {
            io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EACCES)
        };
        // For the read and execute questions the target itself failed. Name the
        // first directory above it that does not let `uid` through — from the
        // outermost down, because that is the one an operator has to open first
        // — and the target itself when every ancestor is passable (the file is
        // simply not readable or not executable).
        let (depth, blocked) = if ask.wants_grant() {
            (0usize, target.path.clone())
        } else {
            let depth = first_blocking_ancestor(&target.path);
            let blocked = if depth == 0 {
                target.path.clone()
            } else {
                target
                    .path
                    .ancestors()
                    .nth(depth)
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| target.path.clone())
            };
            (depth, blocked)
        };
        // What the refusal is about, as the kernel and the filesystem see it. A
        // directory that is already open to `uid` through its own group is not a
        // blocker, so this is only read for the path the answer names.
        let Ok(metadata) = std::fs::metadata(&blocked) else {
            // Gone between the `access` and the `stat`: nothing to name.
            continue;
        };
        let mode = metadata.permissions().mode();
        // A sticky directory a uid may write is not a way into the entry below
        // it: the sticky bit stops that uid unlinking or renaming an entry it
        // does not own. It *is* one when the child is the uid's own, or when
        // there is nothing there yet — a file with a fresh name is what a sticky
        // directory does allow.
        if ask.reads_sticky() && mode & 0o1000 != 0 && sticky_protects(&target.child, uid, init_uid)
        {
            continue;
        }
        write_bytes(
            fd,
            &answer(
                ANSWER_FOUND,
                index,
                depth,
                mode & 0o7777,
                metadata.uid(),
                metadata.gid(),
                errno,
            ),
        );
        return;
    }
    write_bytes(fd, &answer(ANSWER_FINE, 0, 0, 0, 0, 0, 0));
}

/// Whether a sticky directory's child is one a dropped-to uid cannot replace.
///
/// The sticky bit stops unlinking or renaming an entry the caller does not own,
/// so a child owned by root — or by init, which is the uid a deployment's code
/// usually belongs to — is out of that uid's reach. A child that is not there,
/// or that belongs to the uid in question, is not.
fn sticky_child_is_safe(child: &Path, uid: u32, init_uid: u32) -> bool {
    let Ok(metadata) = std::fs::metadata(child) else {
        return false;
    };
    let owner = metadata.uid();
    owner != uid && (owner == 0 || owner == init_uid)
}

/// The same for a [`Target`]'s child, which may be missing: a sticky directory
/// lets a *new* name through, so there is nothing there to protect.
fn sticky_protects(child: &Option<PathBuf>, uid: u32, init_uid: u32) -> bool {
    match child {
        Some(child) => sticky_child_is_safe(child, uid, init_uid),
        None => false,
    }
}

/// The same, for every uid blastit drops to: the fallback's exemption holds
/// only when no dropped-to uid can replace the child.
fn sticky_protects_for_all(child: &Option<PathBuf>, uids: &BTreeSet<u32>, init_uid: u32) -> bool {
    uids.iter()
        .all(|uid| sticky_protects(child, *uid, init_uid))
}

/// The first directory above `path` a dropped-to uid could not pass, as a depth.
///
/// Runs in the child, after the drop, so the answer is the kernel's.
fn first_blocking_ancestor(path: &Path) -> usize {
    let ancestors: Vec<PathBuf> = path.ancestors().skip(1).map(Path::to_path_buf).collect();
    for (at, ancestor) in ancestors.iter().enumerate().rev() {
        let Some(name) = cstring(ancestor) else {
            continue;
        };
        // SAFETY: as above; an ancestor only has to be traversable.
        if unsafe { libc::access(name.as_ptr(), libc::X_OK) } != 0 {
            return at + 1;
        }
    }
    0
}

/// One answer: what happened, which target, how far above it, and the facts.
fn answer(
    kind: u8,
    index: usize,
    depth: usize,
    mode: u32,
    uid: u32,
    gid: u32,
    errno: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(ANSWER_BYTES);
    out.push(kind);
    for field in [index as u32, depth as u32, mode, uid, gid, errno as u32] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    out
}

/// The note that says which target the child is starting on.
fn working(index: usize) -> [u8; 5] {
    let mut out = [ANSWER_WORKING, 0, 0, 0, 0];
    out[1..5].copy_from_slice(&(index as u32).to_le_bytes());
    out
}

fn cstring(path: &Path) -> Option<CString> {
    CString::new(path.as_os_str().as_bytes()).ok()
}

/// Writes bytes to the pipe, ignoring a short write: the child says what it can
/// and `_exit`s, and the parent's deadline bounds the rest.
fn write_bytes(fd: RawFd, bytes: &[u8]) {
    // SAFETY: `write` takes a descriptor, a buffer and its length.
    unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
}

/// Reads the child's answer, within `deadline`.
///
/// The stream is the notes and then one answer, so a child that never answers
/// still says which target it was on.
///
/// # Errors
///
/// [`TimedOut`], naming the target the child was on, when the deadline passed
/// before the answer did. Nothing here waits past it.
fn read_answer(fd: RawFd, targets: &[Target], deadline: Instant) -> Result<Spoken, TimedOut> {
    // SAFETY: the descriptor is the read end of a pipe this process made and
    // still owns.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let mut working_on = None;
    let mut header = [0u8; 1];
    loop {
        match read_exactly(borrowed, &mut header, deadline) {
            Ok(Read::Filled) => {}
            Ok(Read::Timeout) => {
                return Err(TimedOut {
                    path: name_of(targets, working_on),
                });
            }
            // The child is gone without answering — it could not drop, or a
            // signal took it: the mode bits are the only answer there is.
            Ok(Read::End) | Err(_) => return Ok(Spoken::NoProbe),
        }
        let kind = header[0];
        let rest = if kind == ANSWER_WORKING {
            4
        } else {
            ANSWER_BYTES - 1
        };
        let mut bytes = vec![0u8; rest];
        match read_exactly(borrowed, &mut bytes, deadline) {
            Ok(Read::Filled) => {}
            Ok(Read::Timeout) => {
                return Err(TimedOut {
                    path: name_of(targets, working_on),
                });
            }
            Ok(Read::End) | Err(_) => return Ok(Spoken::NoProbe),
        }
        if kind == ANSWER_WORKING {
            working_on =
                Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize);
            continue;
        }
        let field = |at: usize| -> u32 {
            let mut four = [0u8; 4];
            four.copy_from_slice(&bytes[at * 4..at * 4 + 4]);
            u32::from_le_bytes(four)
        };
        let verdict = Verdict {
            index: field(0) as usize,
            depth: field(1) as usize,
            mode: field(2),
            uid: field(3),
            gid: field(4),
            errno: field(5) as i32,
        };
        return match kind {
            ANSWER_FINE => Ok(Spoken::Fine),
            ANSWER_FOUND => Ok(Spoken::Found(verdict)),
            _ => Ok(Spoken::NoProbe),
        };
    }
}

/// The path a timeout is reported against: the target the child was on, or the
/// first one when it never said.
fn name_of(targets: &[Target], working_on: Option<usize>) -> PathBuf {
    let index = working_on.unwrap_or(0);
    targets
        .get(index)
        .or_else(|| targets.first())
        .map(|target| target.path.clone())
        .unwrap_or_default()
}

/// How one `read_exactly` ended.
enum Read {
    /// The bytes asked for arrived.
    Filled,
    /// The deadline passed first.
    Timeout,
    /// The other end is closed.
    End,
}

/// Reads exactly `bytes` before `deadline`; the budget shrinks as it goes, so a
/// child that trickles cannot hold the boot past it.
fn read_exactly(fd: BorrowedFd<'_>, bytes: &mut [u8], deadline: Instant) -> io::Result<Read> {
    for slot in bytes.iter_mut() {
        let budget = deadline.saturating_duration_since(Instant::now());
        if !wait_readable(fd, budget)? {
            return Ok(Read::Timeout);
        }
        match read_one(fd)? {
            Some(byte) => *slot = byte,
            None => return Ok(Read::End),
        }
    }
    Ok(Read::Filled)
}

/// Reaps the probe child, without ever blocking the boot for longer than the
/// deadline it was given.
///
/// `waitpid(WNOHANG)` until the deadline, then `SIGKILL` and one last
/// non-blocking wait: a child that is still inside a syscall cannot be waited
/// for at all, and init's own supervisor reaps whatever is left.
fn reap(pid: Pid, deadline: Instant) {
    loop {
        if reap_once(pid) {
            return;
        }
        if Instant::now() >= deadline {
            // SAFETY: `kill` takes a pid and a signal number.
            unsafe { libc::kill(pid.as_raw(), libc::SIGKILL) };
            reap_once(pid);
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// One `waitpid(WNOHANG)`: true when the child is gone.
fn reap_once(pid: Pid) -> bool {
    let mut status = 0;
    // SAFETY: `waitpid` writes one status word into `status`, and the child is
    // this process's own.
    let waited = unsafe { libc::waitpid(pid.as_raw(), &mut status, libc::WNOHANG) };
    waited == pid.as_raw() || waited == -1
}

/// The read and execute questions of a list of paths: the paths themselves.
fn targets_of(paths: &[PathBuf]) -> Vec<Target> {
    paths
        .iter()
        .map(|path| Target {
            path: path.clone(),
            child: None,
            entry: None,
        })
        .collect()
}

/// The write questions of a list of `sys.path` entries.
///
/// Every entry *and every directory above it up to `/`*, because a writable
/// directory above an entry is a way in: a uid that may write `/a` may `rmdir`
/// `/a/b` and put its own directory back. The entry itself comes first, then its
/// parents, and each parent carries the child below it — the one a sticky
/// directory would protect.
fn targets_above(entries: &[PathBuf]) -> Vec<Target> {
    let mut targets = Vec::new();
    for entry in entries {
        let mut child: Option<PathBuf> = None;
        for (level, path) in std::iter::once(entry.clone())
            .chain(entry.ancestors().skip(1).map(Path::to_path_buf))
            .enumerate()
        {
            if path.as_os_str().is_empty() {
                break;
            }
            targets.push(Target {
                path: path.clone(),
                child: child.clone(),
                entry: if level == 0 {
                    None
                } else {
                    Some(entry.clone())
                },
            });
            child = Some(path);
        }
    }
    targets
}

/// The first directory of `paths` a view uid could not read or traverse.
///
/// The paths are what a view imports through (see the module documentation), in
/// the order an operator should read them: the interpreter first, because that
/// is where a managed Python under a private home blocks, then the application's
/// own code. `dropped` is the layout: a probe asks as its first view uid, and
/// the mode-bit fallback uses every uid it lists.
///
/// A path that is not there is not this check's business: an import that needs a
/// missing directory fails with its own message, and claiming a permission
/// problem there would send an operator to `chmod` something that does not
/// exist.
///
/// # Errors
///
/// [`TimedOut`], when the probe the check asks the kernel with did not answer.
pub fn unreadable(
    paths: &[PathBuf],
    dropped: &DroppedTo,
    interpreter: bool,
) -> Result<Option<Blocker>, TimedOut> {
    by_probe_or_mode(
        &targets_of(paths),
        dropped.probe,
        Probe::Read,
        interpreter,
        false,
    )
}

/// The first program of `paths` a child uid could not `execve`.
///
/// dbproxy is `execve`'d as `BLASTIT_UID_BASE + 3`, which is not the owner of
/// anything the deployment installed: the file itself has to be executable by
/// that uid, and every directory on the way to it traversable. Without this the
/// child dies in `execve` with `Permission denied (os error 13)`, which says
/// nothing about which directory a deployment has to open — the same failure
/// mode [`unreadable`] refuses, one stage later.
///
/// # Errors
///
/// [`TimedOut`], when the probe did not answer.
pub fn unexecutable(
    paths: &[PathBuf],
    uid: u32,
    interpreter: bool,
) -> Result<Option<Blocker>, TimedOut> {
    by_probe_or_mode(&targets_of(paths), uid, Probe::Execute, interpreter, true)
}

/// The first path on `sys.path`, or above one, another uid could write.
///
/// The question is the opposite of [`unreadable`]'s: not "can a view read it"
/// but "can a uid blastit drops to *write* it" — or take it over from above,
/// which for a directory is the same thing. These are the paths the interpreter
/// will import through — its own entries, the working directory blastit was
/// started in, whatever `PYTHONPATH` or a `.pth` file added, and a zip file if
/// one is on the path — because a writable one, or a writable directory above
/// one, is a cross-route code plant whoever put it there.
///
/// The kernel answers, and it is asked **once per uid**, because an ACL can
/// grant exactly one route's uid what it denies every other: the child reports
/// the first path that uid may write, and the refusal names the path and the
/// uid. Every entry is asked about with its parents (see [`targets_above`]);
/// a *sticky* parent — `/tmp` is the one every deployment has — is not a way in
/// when the child below it belongs to root or to init, because a uid cannot
/// unlink or rename what it does not own there.
///
/// The mode bits are the fallback for a uid the probe could not drop to, and
/// they are also what *explains* a refusal: other-writable, group-writable when
/// the path's gid is that uid's own (a dropped-to process has no supplementary
/// groups), or sticky — with the same exemption, modelled the same way.
///
/// # Errors
///
/// [`TimedOut`], when a probe did not answer: a `sys.path` that hung is a boot
/// that has to stop, not one that may guess from the mode bits through the same
/// filesystem.
pub fn writable(
    paths: &[PathBuf],
    dropped: &DroppedTo,
    interpreter: bool,
) -> Result<Option<Writable>, TimedOut> {
    let targets = targets_above(paths);
    if targets.is_empty() {
        return Ok(None);
    }
    // One fork per uid: the child reports the first target that uid may write,
    // and "nothing" is an answer too.
    let mut found: Vec<(usize, u32, Verdict)> = Vec::new();
    let mut unasked: Vec<u32> = Vec::new();
    for uid in &dropped.uids {
        match probe(&targets, *uid, Probe::Write, interpreter)? {
            Spoken::Found(verdict) => found.push((verdict.index, *uid, verdict)),
            Spoken::Fine => {}
            Spoken::NoProbe => unasked.push(*uid),
        }
    }
    found.sort_unstable_by_key(|(index, uid, _)| (*index, *uid));
    if let Some((index, uid, verdict)) = found.first() {
        let Some(target) = targets.get(*index) else {
            return Ok(None);
        };
        let why = writable_by_mode(verdict.mode, verdict.gid, *uid)
            .unwrap_or_else(|| "an ACL lets it through".to_string());
        return Ok(Some(Writable {
            path: target.path.clone(),
            entry: target.entry.clone(),
            mode: verdict.mode,
            probed_uid: Some(*uid),
            why,
        }));
    }
    if unasked.is_empty() {
        return Ok(None);
    }

    // The uids the kernel could not be asked for, and what the mode bits say
    // about them: the documented fallback, which cannot see an ACL either way.
    let init_uid = rustix::process::geteuid().as_raw();
    for target in &targets {
        let Ok(metadata) = std::fs::metadata(&target.path) else {
            continue;
        };
        let mode = metadata.permissions().mode();
        if mode & 0o1000 != 0 && sticky_protects_for_all(&target.child, &dropped.uids, init_uid) {
            continue;
        }
        for uid in &unasked {
            if let Some(why) = writable_by_mode(mode, metadata.gid(), *uid) {
                return Ok(Some(Writable {
                    path: target.path.clone(),
                    entry: target.entry.clone(),
                    mode: mode & 0o7777,
                    probed_uid: None,
                    why,
                }));
            }
        }
    }
    Ok(None)
}

/// Whether the mode bits alone say `uid` could write a path with this `mode` and
/// `gid`.
///
/// The classes this check names: **other** may write it; the **group** may write
/// it *and* the path's gid is the uid's own (a group class only counts for a uid
/// that is in that group, and a dropped-to process has no supplementary groups,
/// so the gid has to be its own); or it is **sticky** — a `/tmp`-style directory
/// takes a file with a fresh name whatever the sticky bit does about deleting
/// one that is there, which is why [`writable`] exempts a sticky *parent* only
/// when the child below it belongs to root or init.
///
/// The uid is named when the group class is what answered, because the mode bits
/// were the only witness: the message has to say which uid a deployment has to
/// keep out.
fn writable_by_mode(mode: u32, gid: u32, uid: u32) -> Option<String> {
    if mode & Access::Write.bit() != 0 {
        return Some(
            if mode & 0o1000 != 0 {
                "the sticky bit stops another uid deleting what is there, not writing to it"
            } else {
                "other-writable"
            }
            .to_string(),
        );
    }
    if mode & 0o020 != 0 && gid == uid {
        return Some(format!("group-writable for uid {uid}'s own group"));
    }
    None
}

/// One check: the kernel's answer when it can be asked, the mode bits when it
/// cannot.
///
/// The fallback runs on [`Spoken::NoProbe`] only. A probe that answered "every
/// path is fine" is an answer, and answering it again with the mode bits would
/// refuse exactly the deployments the probe exists for — a directory a view
/// reaches through a group class, or an ACL nobody can read out of the bits.
fn by_probe_or_mode(
    targets: &[Target],
    uid: u32,
    ask: Probe,
    interpreter: bool,
    executable: bool,
) -> Result<Option<Blocker>, TimedOut> {
    match probe(targets, uid, ask, interpreter)? {
        Spoken::Found(verdict) => {
            let Some(target) = targets.get(verdict.index) else {
                return Ok(None);
            };
            // The path the refusal is about: the target itself, or the directory
            // above it that does not let the uid through.
            let blocked = if verdict.depth == 0 {
                target.path.clone()
            } else {
                target
                    .path
                    .ancestors()
                    .nth(verdict.depth)
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| target.path.clone())
            };
            let access = match (verdict.depth, ask) {
                // A directory on the way is only traversed.
                (depth, _) if depth > 0 => Access::Traverse,
                (_, Probe::Read) => Access::Read,
                (_, Probe::Execute) => Access::Execute,
                (_, Probe::Write) => Access::Write,
            };
            Ok(Some(Blocker {
                path: blocked,
                mode: verdict.mode & 0o777,
                access,
                probed_uid: Some(uid),
            }))
        }
        Spoken::Fine => Ok(None),
        Spoken::NoProbe => Ok(targets
            .iter()
            .find_map(|target| blocks(&target.path, executable).map(unprobed))),
    }
}

/// The same refusal, from the mode bits rather than from the kernel.
fn unprobed(blocker: Blocker) -> Blocker {
    Blocker {
        probed_uid: None,
        ..blocker
    }
}

/// The first path on the way to `path` that a dropped-to uid could not use,
/// read from the mode bits.
///
/// `executable` says what the leaf is for: a program a child `execve`s
/// ([`unexecutable`], `true`) or a directory the interpreter and the application
/// are read through ([`unreadable`], `false`). This is the *fallback*: it cannot
/// see an ACL, and it is only used when a probe could not run at all. Symlinks
/// are followed, because the kernel resolves them.
fn blocks(path: &Path, executable: bool) -> Option<Blocker> {
    for (depth, directory) in path.ancestors().enumerate() {
        if directory.as_os_str().is_empty() {
            continue;
        }
        let Ok(metadata) = std::fs::metadata(directory) else {
            // The path is not there (or not readable to us, which as root or
            // init's uid means it is not there): whatever needed it fails with
            // its own message.
            continue;
        };
        let mode = metadata.permissions().mode() & 0o777;
        if depth == 0 {
            let Some(wanted) = leaf_accesses(metadata.is_dir(), executable) else {
                continue;
            };
            if let Some(access) = wanted.iter().find(|access| mode & access.bit() == 0) {
                return Some(Blocker {
                    path: directory.to_path_buf(),
                    mode,
                    access: *access,
                    probed_uid: None,
                });
            }
            continue;
        }
        if mode & Access::Traverse.bit() == 0 {
            return Some(Blocker {
                path: directory.to_path_buf(),
                mode,
                access: Access::Traverse,
                probed_uid: None,
            });
        }
    }
    None
}

/// What the last path on the way has to allow.
///
/// A directory is both read and descended into — a module inside it is reached
/// by reading the directory and traversing it — and a program only has to be
/// executable. A file that is neither is not this check's business: whether a
/// view could read it is not a permission a deployment has to grant.
fn leaf_accesses(is_dir: bool, executable: bool) -> Option<&'static [Access]> {
    match (executable, is_dir) {
        // A program a child `execve`s: the file itself has to be executable.
        (true, _) => Some(&[Access::Execute]),
        // A directory the interpreter and the application are read through: it
        // is both read and descended into. A file is neither — nothing here runs
        // it, and whether a view could read it is not this check's business.
        (false, true) => Some(&[Access::Read, Access::Traverse]),
        (false, false) => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// The base of the layout these tests probe with.
    ///
    /// The uids are nobody this test process belongs to, so the group class of a
    /// directory the test made (its gid is the test's own) is not one a
    /// dropped-to uid is in. A test that needs the group class to *count* has to
    /// `chown` the directory, which needs root — and so does the probe itself,
    /// which is why those tests say so.
    const PROBE_BASE: u32 = 30_000;

    /// The uid `unexecutable` is asked as in these tests: dbproxy's.
    const DBPROXY_UID: u32 = PROBE_BASE + 3;

    /// A uid that is nobody in the layout and nobody running the tests: the
    /// owner of a directory that a sticky parent should have protected, and
    /// does not, because it is not root and not init.
    const OTHERS_UID: u32 = 60_000;

    /// The layout a probe speaks for in these tests.
    fn dropped() -> DroppedTo {
        DroppedTo::of(blastit_manifest::UidLayout::new(PROBE_BASE), 2)
    }

    /// [`unreadable`], [`unexecutable`] and [`writable`] with the timeout turned into a
    /// test failure: nothing these tests ask about hangs, and a `TimedOut` is a bug
    /// in the test rather than a case it is about (the timeout has its own test).
    fn unreadable(paths: &[PathBuf], dropped: &DroppedTo, interpreter: bool) -> Option<Blocker> {
        super::unreadable(paths, dropped, interpreter).expect("no timeout")
    }

    fn unexecutable(paths: &[PathBuf], uid: u32, interpreter: bool) -> Option<Blocker> {
        super::unexecutable(paths, uid, interpreter).expect("no timeout")
    }

    fn writable(paths: &[PathBuf], dropped: &DroppedTo, interpreter: bool) -> Option<Writable> {
        super::writable(paths, dropped, interpreter).expect("no timeout")
    }

    /// The same for [`probe`], whose targets these tests build from paths.
    fn probe(
        paths: &[PathBuf],
        uid: u32,
        ask: Probe,
        interpreter: bool,
    ) -> Result<Spoken, TimedOut> {
        super::probe(&targets_of(paths), uid, ask, interpreter)
    }

    /// Whether this process can drop to another uid, which is what a probe has
    /// to do before it can ask the kernel anything.
    fn can_probe() -> bool {
        if rustix::process::geteuid().is_root() {
            return true;
        }
        println!("skipped: a probe has to drop to another uid, which needs root");
        false
    }

    /// `chown(2)`, which is how a test puts a directory in a group a dropped-to
    /// uid is in — root only.
    fn chown(path: &Path, uid: u32, gid: u32) {
        let name = CString::new(path.as_os_str().as_bytes()).expect("no NUL");
        // SAFETY: `chown` takes a NUL-terminated path and two ids.
        let changed = unsafe { libc::chown(name.as_ptr(), uid, gid) };
        assert_eq!(changed, 0, "chown: {}", io::Error::last_os_error());
    }

    /// Runs `body` in a child of this process, and returns the code it reports.
    ///
    /// The probe forks, and `fork()` refuses in a multi-threaded process — the
    /// harness runs each test on a thread of one — so a test that wants the
    /// kernel's answer has to reach a single-threaded process first. The body
    /// only drops privileges, calls `access(2)` and answers with one small code;
    /// nothing in it allocates more than a `CString`, takes a lock, or unwinds,
    /// which is the shape `fork_unchecked` asks for (the codebase's own
    /// `testutil` does the same thing for the same reason).
    fn in_a_child(body: impl FnOnce() -> i32) -> i32 {
        // SAFETY: as above — the child is short, allocation-light and never
        // returns to the harness. The check `fork_unchecked` skips is about a
        // multi-threaded parent, and the harness thread that is copied here
        // holds nothing the body touches.
        match unsafe { blastit_sys::fork_unchecked() } {
            Ok(Fork::Child) => {
                // SAFETY: `_exit` never returns.
                unsafe { libc::_exit(body()) }
            }
            Ok(Fork::Parent(pid)) => {
                let mut status = 0;
                // SAFETY: `waitpid` writes one status word into `status`, and
                // the child is this process's own.
                let waited = unsafe { libc::waitpid(pid.as_raw(), &mut status, 0) };
                if waited != pid.as_raw() {
                    return -1;
                }
                match blastit_sys::ExitStatus::from_raw(status) {
                    Some(blastit_sys::ExitStatus::Code(code)) => code,
                    _ => -1,
                }
            }
            Err(_) => -1,
        }
    }

    /// A directory tree under `/tmp`, removed when it is dropped.
    ///
    /// `/tmp` rather than `$HOME`, and deliberately: these tests are about the
    /// bits another uid needs, and every directory *above* the tree is part of
    /// the answer. `/tmp` is `1777` and `/` is `0755` on every machine blastit
    /// runs on, so the first blocker a test finds is one it created itself —
    /// where a `$HOME` tree would report the real home above it (`0750` in the
    /// very layout this module exists for).
    struct TempTree {
        path: PathBuf,
    }

    impl TempTree {
        fn new(tag: &str) -> Self {
            let path = PathBuf::from("/tmp")
                .join(format!(".blastit-visibility-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { path }
        }

        /// A directory inside the tree, with `mode`.
        fn dir(&self, name: &str, mode: u32) -> PathBuf {
            let path = self.path.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn a_directory_every_one_can_read_is_not_a_blocker() {
        let tree = TempTree::new("open");
        let open = tree.dir("lib", 0o755);
        let inside = tree.dir("lib/python3.14", 0o755);
        assert_eq!(unreadable(&[open.clone(), inside], &dropped(), false), None);
        // A directory that is not there is not a permission problem.
        assert_eq!(
            unreadable(&[tree.path.join("missing")], &dropped(), false),
            None
        );
        // Neither is a file: `sys.prefix` is a directory, but a path that is
        // not one has no mode to report.
        let file = tree.path.join("os.py");
        std::fs::write(&file, b"").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(unreadable(&[file], &dropped(), false), None);
    }

    #[test]
    fn a_leaf_a_view_could_not_read_names_itself_and_its_mode() {
        let tree = TempTree::new("leaf");
        let closed = tree.dir("site-packages", 0o750);
        let blocker =
            unreadable(std::slice::from_ref(&closed), &dropped(), false).expect("a blocker");
        assert_eq!(blocker.path, closed);
        assert_eq!(blocker.mode, 0o750);
        assert_eq!(blocker.access, Access::Read);
        assert!(blocker.message().contains("0750"), "{}", blocker.message());
        assert!(
            blocker.message().contains(&closed.display().to_string()),
            "{}",
            blocker.message()
        );
        assert!(
            blocker.message().contains("stringprep"),
            "{}",
            blocker.message()
        );

        // A directory others may read but not descend into is traversable's
        // problem, and the message says which of the two it is.
        let opaque = tree.dir("opaque", 0o744);
        let blocker = unreadable(&[opaque], &dropped(), false).expect("a blocker");
        assert_eq!(blocker.access, Access::Traverse);
        assert!(
            blocker.message().contains("traversable"),
            "{}",
            blocker.message()
        );
    }

    #[test]
    fn an_ancestor_a_view_could_not_traverse_is_named_not_the_leaf() {
        let tree = TempTree::new("ancestor");
        let private = tree.dir("home", 0o750);
        let inside = private.join("python");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::set_permissions(&inside, std::fs::Permissions::from_mode(0o755)).unwrap();

        // The interpreter directory itself is open; the home above it is not,
        // and that is the directory an operator has to change.
        let blocker = unreadable(&[inside], &dropped(), false).expect("a blocker");
        assert_eq!(blocker.path, private);
        assert_eq!(blocker.access, Access::Traverse);
        assert_eq!(blocker.mode, 0o750);
    }

    #[test]
    fn the_first_blocker_wins_so_the_interpreter_is_reported_first() {
        let tree = TempTree::new("order");
        let first = tree.dir("interpreter", 0o700);
        let second = tree.dir("project", 0o700);
        let blocker = unreadable(&[first.clone(), second], &dropped(), false).expect("a blocker");
        assert_eq!(blocker.path, first);
    }

    #[test]
    fn a_program_a_child_could_not_execute_is_named_and_explained() {
        // dbproxy is `execve`'d as uid `BLASTIT_UID_BASE + 3`, which owns
        // nothing a deployment installed: the file itself has to be executable
        // by others, and so does every directory on the way to it.
        let tree = TempTree::new("program");
        let program = tree.path.join("blastit-dbproxy");
        std::fs::write(&program, b"#!/bin/sh\n").unwrap();

        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let blocker =
            unexecutable(std::slice::from_ref(&program), DBPROXY_UID, false).expect("a blocker");
        assert_eq!(blocker.path, program);
        assert_eq!(blocker.mode, 0o700);
        assert_eq!(blocker.access, Access::Execute);
        let message = blocker.message();
        assert!(message.contains("is not executable by"), "{message}");
        assert!(message.contains("Permission denied"), "{message}");

        // Readable but not executable is still not executable…
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            unexecutable(std::slice::from_ref(&program), DBPROXY_UID, false)
                .unwrap()
                .access,
            Access::Execute
        );
        // …and `0755`, which is what a shipped binary is, is fine.
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            unexecutable(std::slice::from_ref(&program), DBPROXY_UID, false),
            None
        );

        // A program behind a directory nobody may pass: the *directory* is what
        // an operator has to change, so that is what is named.
        let private = tree.dir("private", 0o700);
        let inside = private.join("blastit-dbproxy");
        std::fs::write(&inside, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&inside, std::fs::Permissions::from_mode(0o755)).unwrap();
        let blocker = unexecutable(&[inside], DBPROXY_UID, false).expect("a blocker");
        assert_eq!(blocker.path, private);
        assert_eq!(blocker.access, Access::Traverse);

        // A path that is not there is not a permission problem.
        assert_eq!(
            unexecutable(&[tree.path.join("missing")], DBPROXY_UID, false),
            None
        );
    }

    #[test]
    fn a_group_class_that_belongs_to_the_uid_counts() {
        // `root:<view uid> 0750`: the mode bits have nothing for "other", and a
        // check that reads them alone refuses a directory the view can read —
        // its gid *is* its own, and being in one's own group needs no
        // supplementary group at all. Only the kernel can tell the two apart,
        // which is what the probe is for.
        if !can_probe() {
            return;
        }
        let tree = TempTree::new("group-class");
        let shared = tree.dir("shared", 0o750);
        let view = dropped().probe;
        chown(&shared, 0, view);
        // In a child: the probe forks, and this harness is multi-threaded.
        assert_eq!(
            in_a_child(
                || match probe(std::slice::from_ref(&shared), view, Probe::Read, false) {
                    Ok(Spoken::Fine) => 0,
                    Ok(Spoken::Found(_)) => 1,
                    Ok(Spoken::NoProbe) | Err(_) => 2,
                }
            ),
            0,
            "the group class is the uid's own group (1 = refused, 2 = no answer)"
        );

        // The same mode, in a group no dropped-to uid is in: now the kernel
        // refuses, and the refusal carries what the message needs.
        chown(&shared, 0, PROBE_BASE + 500);
        assert_eq!(
            in_a_child(|| {
                match unreadable(std::slice::from_ref(&shared), &dropped(), false) {
                    Some(blocker)
                        if blocker.path == shared
                            && blocker.access == Access::Read
                            && blocker.mode == 0o750
                            && blocker.probed_uid == Some(view)
                            && blocker.message().contains(&view.to_string()) =>
                    {
                        0
                    }
                    Some(_) => 1,
                    None => 2,
                }
            }),
            0,
            "1 = the refusal is not the one the kernel's answer describes, 2 = none"
        );
    }

    #[test]
    fn a_directory_another_uid_could_write_is_refused() {
        // A path on `sys.path` may not be writable by a uid blastit drops to:
        // one route could plant a module another route imports.
        let tree = TempTree::new("writable");
        // 0755 is what a code tree is.
        assert_eq!(
            writable(std::slice::from_ref(&tree.path), &dropped(), false),
            None
        );

        // 0777 — the reported `cwd` — and a sticky `/tmp`-style 1777.
        for (name, mode) in [("open", 0o777), ("sticky", 0o1777)] {
            let directory = tree.dir(name, mode);
            let refusal = writable(std::slice::from_ref(&directory), &dropped(), false)
                .unwrap_or_else(|| panic!("{name} must be refused"));
            assert_eq!(refusal.path, directory, "{name}");
            assert_eq!(refusal.entry, None, "{name}");
            assert!(
                refusal.message().contains("chmod go-w"),
                "{}",
                refusal.message()
            );
            assert!(
                refusal.message().contains("sys.path"),
                "and says why it matters: {}",
                refusal.message()
            );
            assert!(
                refusal.message().contains(&directory.display().to_string()),
                "and names it: {}",
                refusal.message()
            );
            if mode == 0o1777 {
                assert!(
                    refusal.message().contains("sticky"),
                    "the sticky bit is answered, not obeyed: {}",
                    refusal.message()
                );
            }
        }

        // Group-writable is a hole only when the gid is one blastit drops to:
        // this one is the test's own group, which no view is in.
        let shared = tree.dir("shared", 0o775);
        assert_eq!(
            writable(std::slice::from_ref(&shared), &dropped(), false),
            None
        );
        // An empty list has nothing to refuse, and a path that is not there
        // under a directory nobody else can write is not a hole either.
        assert_eq!(writable(&[], &dropped(), false), None);
        assert_eq!(
            writable(&[tree.path.join("missing")], &dropped(), false),
            None,
            "the tree is 0755, so nobody else could create it"
        );

        // …and when the gid *is* a uid blastit drops to, the mode bits say so
        // and the kernel agrees (root only: only root can put a directory in
        // that group). In a child, because the probe forks.
        if can_probe() {
            chown(&shared, 0, dropped().probe);
            assert_eq!(
                in_a_child(|| {
                    match writable(std::slice::from_ref(&shared), &dropped(), false) {
                        Some(refusal)
                            if refusal.probed_uid == Some(dropped().probe)
                                && refusal.message().contains("group-writable") =>
                        {
                            0
                        }
                        Some(_) => 1,
                        None => 2,
                    }
                }),
                0,
                "a group a dropped-to uid is in (1 = the refusal is not the kernel's, 2 = none)"
            );
        }
    }

    #[test]
    fn every_uid_that_could_write_is_asked() {
        // An ACL, or a group class, can grant one route's uid what it denies
        // every other: the check asks the kernel once per uid, not once for the
        // first route. `root:<a later view uid> 0770` is that case without an
        // ACL — the first view uid is not in that group and cannot write, the
        // later one *is* and can.
        if !can_probe() {
            return;
        }
        let tree = TempTree::new("every-uid");
        let shared = tree.dir("shared", 0o770);
        let later = *dropped().uids.iter().next_back().expect("a uid");
        assert_ne!(later, dropped().probe, "the first uid is not the answer");
        chown(&shared, 0, later);
        assert_eq!(
            in_a_child(|| {
                match writable(std::slice::from_ref(&shared), &dropped(), false) {
                    Some(refusal) if refusal.probed_uid == Some(later) => 0,
                    Some(_) => 1,
                    None => 2,
                }
            }),
            0,
            "the later uid's write access (1 = another uid was blamed, 2 = none found)"
        );
        // The message names that uid, which is what an operator has to look up.
        assert!(
            writable(std::slice::from_ref(&shared), &dropped(), false)
                .map(|refusal| refusal.message().contains(&later.to_string()))
                .unwrap_or(false)
                || !can_probe(),
            "the message names the uid"
        );
    }

    #[test]
    fn a_writable_directory_above_an_entry_is_refused() {
        // The review's repro: `/tmp/ex/parent` is `0777` and
        // `/tmp/ex/parent/sub` is `0755` and on `PYTHONPATH`. A uid that may
        // write `parent` may `rmdir` `sub` and put its own directory back, so
        // the entry is only as safe as everything above it.
        let tree = TempTree::new("above-entry");
        let parent = tree.dir("parent", 0o777);
        let sub = tree.dir("parent/sub", 0o755);
        let refusal = writable(std::slice::from_ref(&sub), &dropped(), false)
            .unwrap_or_else(|| panic!("{parent:?} must be refused"));
        assert_eq!(refusal.path, parent, "the directory that has to change");
        assert_eq!(refusal.entry, Some(sub.clone()), "and the entry below it");
        assert!(
            refusal.message().contains(&sub.display().to_string())
                && refusal.message().contains(&parent.display().to_string())
                && refusal.message().contains("chmod go-w"),
            "the message names both, and the fix: {}",
            refusal.message()
        );
        assert!(
            refusal.message().contains("take the entry over"),
            "and says how it would be taken over: {}",
            refusal.message()
        );

        // The entry itself being the writable one is still reported first.
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o777)).unwrap();
        let refusal = writable(std::slice::from_ref(&sub), &dropped(), false)
            .unwrap_or_else(|| panic!("{sub:?} must be refused"));
        assert_eq!(refusal.path, sub);
        assert_eq!(
            refusal.entry, None,
            "the entry itself is not a child of one"
        );
    }

    #[test]
    fn a_sticky_directory_above_an_entry_is_not_a_way_in() {
        // `/tmp` itself: sticky and writable by everyone, and *not* a way into
        // what is below it — the sticky bit stops a uid unlinking or renaming an
        // entry it does not own, and the directory below belongs to root or to
        // init, which is the deployment's own uid. Nothing here is root-only:
        // the test's own uid plays init, and its own tree is what is protected.
        let tree = TempTree::new("sticky-safe");
        let safe = tree.dir("safe", 0o1777);
        let inside = tree.dir("safe/inside", 0o755);
        assert_eq!(
            writable(std::slice::from_ref(&inside), &dropped(), false),
            None,
            "a sticky parent over a root- or init-owned child is not a hole"
        );

        // The same parent, with a child owned by somebody a dropped-to uid
        // neither is nor can touch: then the sticky bit protects nothing, because
        // root can hand a directory to anyone and a uid that owns an entry in a
        // sticky directory may rename it. Root only, for the `chown`.
        if can_probe() {
            let taken = tree.dir("safe/taken", 0o755);
            chown(&taken, OTHERS_UID, OTHERS_UID);
            assert_eq!(
                in_a_child(|| {
                    match writable(std::slice::from_ref(&taken), &dropped(), false) {
                        Some(refusal)
                            if refusal.path == safe
                                && refusal.entry == Some(taken.clone())
                                && refusal.message().contains("sticky") =>
                        {
                            0
                        }
                        Some(_) => 1,
                        None => 2,
                    }
                }),
                0,
                "a sticky parent over somebody else's child (1 = another refusal, 2 = none)"
            );
        }
    }

    #[test]
    fn a_probe_that_does_not_answer_names_its_path_and_is_killed() {
        // The deadline really has to bound the boot: a child that is inside a
        // syscall it will never leave gets `SIGKILL`ed, the check reports the
        // path it was on, and nothing blocks. The child here is the test's own,
        // because the probe's child only ever calls `access(2)` and `stat`.
        let tree = TempTree::new("timeout");
        let slow = tree.path.join("slow");
        std::fs::create_dir(&slow).unwrap();
        let targets = targets_of(std::slice::from_ref(&slow));
        let (read_end, write_end) = blastit_sys::pipe().unwrap();
        let forked = unsafe { blastit_sys::fork_unchecked() }.unwrap();
        let pid = match forked {
            Fork::Child => {
                drop(read_end);
                // Say which target this child is "on", then never answer.
                write_bytes(write_end.as_raw_fd(), &working(0));
                std::thread::sleep(Duration::from_secs(30));
                // SAFETY: `_exit` never returns.
                unsafe { libc::_exit(0) }
            }
            Fork::Parent(pid) => {
                drop(write_end);
                pid
            }
        };
        let deadline = Instant::now() + Duration::from_millis(50);
        let answer = read_answer(read_end.as_raw_fd(), &targets, deadline);
        assert!(
            matches!(answer, Err(TimedOut { ref path }) if path == &slow),
            "the timeout names the path the child was on"
        );
        let started = Instant::now();
        reap(pid, deadline);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the reap does not block past the deadline either"
        );
        // Killed and reaped here, not left for a supervisor that is not running
        // in a test. `SIGKILL` is asynchronous, so this waits for it — with a
        // bound, because a child that is not reaped at all is the failure.
        let mut status = 0;
        let mut waited = 0;
        let give_up = Instant::now() + Duration::from_secs(5);
        while Instant::now() < give_up {
            // SAFETY: `waitpid` writes one status word into `status`.
            waited = unsafe { libc::waitpid(pid.as_raw(), &mut status, libc::WNOHANG) };
            if waited == -1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(waited, -1, "the child was killed and reaped: {waited}");
    }

    #[test]
    fn a_probe_leaves_no_child_behind() {
        // The other half of the deadline: a child that answers is reaped inside
        // it, so no probe leaves a zombie. Run in a child of our own, so
        // `waitpid(-1)` has nothing else to report.
        let uid = *dropped().uids.iter().next().expect("a uid");
        let code = in_a_child(|| {
            let targets = targets_of(&[PathBuf::from("/")]);
            let _ = super::probe(&targets, uid, Probe::Read, false);
            let mut status = 0;
            // SAFETY: `waitpid` writes one status word into `status`.
            let left = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            i32::from(left != -1)
        });
        assert_eq!(code, 0, "no probe child is left unreaped");
    }

    #[test]
    fn a_missing_entry_under_a_writable_directory_is_refused() {
        // An entry that is not there yet is a hole all the same, when the
        // directory it would be created in is writable: the interpreter would
        // import from it the moment somebody made it.
        let tree = TempTree::new("missing-entry");
        let open = tree.dir("open", 0o777);
        let missing = open.join("not-there");
        let refusal = writable(std::slice::from_ref(&missing), &dropped(), false)
            .unwrap_or_else(|| panic!("{missing:?} must be refused"));
        assert_eq!(refusal.path, open, "what has to change is the directory");
        assert_eq!(
            refusal.entry,
            Some(missing.clone()),
            "and the entry is named"
        );
        assert!(
            refusal.message().contains(&missing.display().to_string())
                && refusal.message().contains("take the entry over"),
            "the message explains how the entry could be taken over: {}",
            refusal.message()
        );
        assert!(
            refusal.message().contains("chmod go-w"),
            "{}",
            refusal.message()
        );

        // The nearest *existing* directory is the one that matters: a level
        // deeper changes nothing.
        assert_eq!(
            writable(&[open.join("a/b/c")], &dropped(), false)
                .map(|refusal| refusal.path)
                .as_deref(),
            Some(open.as_path())
        );
    }

    #[test]
    fn a_writable_zip_on_sys_path_is_refused() {
        // A zip is a file on `sys.path`, and `zipimport` reads modules out of
        // it: writing the *file* is what plants code, so that is what is asked
        // about — not the directory it sits in, which is fine here.
        let tree = TempTree::new("zip");
        let archive = tree.path.join("app.zip");
        std::fs::write(&archive, b"PK\x05\x06").unwrap();
        std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            writable(std::slice::from_ref(&archive), &dropped(), false),
            None,
            "0644 in a 0755 tree is what a shipped archive looks like"
        );

        std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o666)).unwrap();
        let refusal = writable(std::slice::from_ref(&archive), &dropped(), false)
            .unwrap_or_else(|| panic!("{archive:?} must be refused"));
        assert_eq!(refusal.path, archive, "the file itself");
        assert_eq!(refusal.entry, None);
        assert_eq!(refusal.mode, 0o666);
        assert!(
            refusal.message().contains(&archive.display().to_string()),
            "{}",
            refusal.message()
        );
    }

    #[test]
    fn a_private_home_refuses_a_uv_managed_interpreter() {
        // The layout the papercut is about: `~/.local/share/uv/python/...` under
        // a `0750` home, where the interpreter and its own directories are
        // perfectly open — and unreachable, because nobody else may pass the
        // home.
        let tree = TempTree::new("uv");
        let home = tree.dir("me", 0o750);
        let store = home.join(".local/share/uv/python/cpython-3.14");
        std::fs::create_dir_all(&store).unwrap();
        for directory in [
            home.join(".local"),
            home.join(".local/share"),
            home.join(".local/share/uv"),
            home.join(".local/share/uv/python"),
            store.clone(),
        ] {
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let blocker = unreadable(
            &[
                store,
                home.join(".local/share/uv/python/cpython-3.14/lib/python3.14"),
            ],
            &dropped(),
            false,
        )
        .expect("a blocker");
        assert_eq!(blocker.path, home);
        assert!(
            blocker.message().contains("UV_PYTHON_INSTALL_DIR"),
            "the fix is named: {}",
            blocker.message()
        );
    }
}
