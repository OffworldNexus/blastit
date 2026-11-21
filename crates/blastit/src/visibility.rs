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
//! is the exception, and only when the entry below it belongs to nobody blastit
//! drops to, because the sticky bit stops a uid unlinking or renaming what it does
//! not own.
//! The first path that fails is the one the message names, because it is the one
//! an operator has to change.
//!
//! # Builds without an interpreter
//!
//! A `python`-less build serves no WSGI view, so nothing calls the
//! writability half — [`writable`], [`Writable`] and the helpers behind them —
//! and the read/execute checks keep running for `dbproxy` and the scheduler.
//! Those items stay **compiled** in that build (`cfg_attr(…allow(dead_code))`
//! per item, never a `cfg` on the module): a check that stops being compiled is a
//! check nobody notices changing, and the embedded-PHP worker imports
//! through `sys.path` exactly as a view does.

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
#[cfg_attr(not(feature = "python"), allow(dead_code))]
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
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
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
        let fix = if self.entry.is_some() && self.mode & 0o1000 != 0 {
            format!(
                "Move the project so that the entry is not below a sticky directory a uid \
                 blastit drops to owns — the owner of a sticky directory may rename or \
                 delete anything in it, whatever the entry's own owner is — or `chown` {} \
                 to root",
                self.path.display(),
            )
        } else {
            format!(
                "Start blastit in the code tree, next to the application — a directory only \
                 its owner (and root) can write — or make this one so: `chmod go-w {}`",
                self.path.display(),
            )
        };
        format!(
            "{what}: {answer}. `sys.path` is how a view finds code — the interpreter's own \
             entries, the working directory blastit was started in, and whatever \
             `PYTHONPATH` or a `.pth` file added — and a view process runs as a uid of its \
             own, so a route could plant a module another route imports, and whichever \
             route imports it is not the planter's choice. {fix}",
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
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
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
/// `hooks` is the live interpreter's fork protocol, exactly as
/// [`crate::forking::fork`] takes it: a wheel install forks under it (§9.3), and
/// the standalone binary's stage 0 has no interpreter at all (`None`).
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
    hooks: Option<crate::py::PyForkHooks<'_>>,
    dropped: &DroppedTo,
) -> Result<Spoken, TimedOut> {
    if targets.is_empty() {
        return Ok(Spoken::Fine);
    }
    let (read_end, write_end) = match pipe() {
        Ok(pair) => pair,
        Err(_) => return Ok(Spoken::NoProbe),
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let forked = crate::forking::fork(hooks);
    match forked {
        Ok(Fork::Child) => {
            drop(read_end);
            // The child never returns: no unwinding, no destructors, nothing of
            // the parent's state that a `fork` could have copied half-written.
            write_answer(write_end.as_raw_fd(), uid, targets, ask, dropped);
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
fn write_answer(fd: RawFd, uid: u32, targets: &[Target], ask: Probe, dropped: &DroppedTo) {
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
        if ask.reads_sticky()
            && mode & 0o1000 != 0
            && sticky_protects(metadata.uid(), &target.child, uid, dropped)
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
/// so a child owned by anybody a view uid is *not* is protected: root, init, or
/// whichever user a deployment installed its code as. A child that is not there
/// is not — a sticky directory lets a *new* name through — and neither is one
/// owned by a uid blastit drops to, which may rename its own entry.
fn sticky_child_is_safe(child: &Path, uid: u32, dropped: &DroppedTo) -> bool {
    let Ok(metadata) = std::fs::metadata(child) else {
        return false;
    };
    let owner = metadata.uid();
    owner != uid && !dropped.uids.contains(&owner)
}

/// The same for a [`Target`]'s child, which may be missing: a sticky directory
/// lets a *new* name through, so there is nothing there to protect.
///
/// `owner` is the owner of the sticky directory itself, and it comes first
/// because POSIX gives that uid the right to rename or delete *any* entry in it:
/// a sticky directory a uid blastit drops to owns protects nothing at all.
fn sticky_protects(owner: u32, child: &Option<PathBuf>, uid: u32, dropped: &DroppedTo) -> bool {
    if dropped.uids.contains(&owner) {
        return false;
    }
    match child {
        Some(child) => sticky_child_is_safe(child, uid, dropped),
        None => false,
    }
}

/// The same, for every uid blastit drops to: the fallback's exemption holds
/// only when no dropped-to uid can take the entry over.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
fn sticky_protects_for_all(owner: u32, child: &Option<PathBuf>, dropped: &DroppedTo) -> bool {
    if dropped.uids.contains(&owner) {
        return false;
    }
    let Some(child) = child else {
        return false;
    };
    let Ok(metadata) = std::fs::metadata(child) else {
        return false;
    };
    !dropped.uids.contains(&metadata.uid())
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
#[cfg_attr(not(feature = "python"), allow(dead_code))]
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
    hooks: Option<crate::py::PyForkHooks<'_>>,
) -> Result<Option<Blocker>, TimedOut> {
    by_probe_or_mode(
        &targets_of(paths),
        dropped.probe,
        Probe::Read,
        hooks,
        false,
        dropped,
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
    hooks: Option<crate::py::PyForkHooks<'_>>,
    dropped: &DroppedTo,
) -> Result<Option<Blocker>, TimedOut> {
    by_probe_or_mode(
        &targets_of(paths),
        uid,
        Probe::Execute,
        hooks,
        true,
        dropped,
    )
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
/// when the child below it belongs to nobody blastit drops to, because a uid cannot
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
#[cfg_attr(not(feature = "python"), allow(dead_code))]
pub fn writable(
    paths: &[PathBuf],
    dropped: &DroppedTo,
    hooks: Option<crate::py::PyForkHooks<'_>>,
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
        match probe(&targets, *uid, Probe::Write, hooks, dropped)? {
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
    for target in &targets {
        let Ok(metadata) = std::fs::metadata(&target.path) else {
            continue;
        };
        let mode = metadata.permissions().mode();
        if mode & 0o1000 != 0 && sticky_protects_for_all(metadata.uid(), &target.child, dropped) {
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
/// when the child below it belongs to nobody blastit drops to.
///
/// The uid is named when the group class is what answered, because the mode bits
/// were the only witness: the message has to say which uid a deployment has to
/// keep out.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
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
    hooks: Option<crate::py::PyForkHooks<'_>>,
    executable: bool,
    dropped: &DroppedTo,
) -> Result<Option<Blocker>, TimedOut> {
    match probe(targets, uid, ask, hooks, dropped)? {
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
