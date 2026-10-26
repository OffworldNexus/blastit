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
//! * a directory on `sys.path` that another uid can *write* lets one route plant
//!   a module another route imports: the sandbox is the uid, so code that runs
//!   under a route's own uid is code that route's grants apply to — but which
//!   route imports it is not the planter's choice to make.
//!
//! So the checks *ask the kernel*: [`probe`] forks a child, drops it to the uid
//! in question and calls `access(2)` on each path, which is the decision the
//! kernel will make inside a view. Only when a probe cannot run at all (a test
//! run as an ordinary user, a system without `/proc`) does the module fall back
//! to reading the mode bits, and the message says which of the two answered.
//!
//! # What is checked
//!
//! Every path a view imports *through*: the interpreter's prefixes, the standard
//! library, `site-packages`, and the directory the application's own module comes
//! from — read and traversed, [`unreadable`]. A child `execve`s *programs*, so
//! [`unexecutable`] asks the other question. And every directory blastit itself
//! adds to `sys.path` must be one no other uid can write, [`writable`]. The first
//! path that fails is the one the message names, because it is the one an
//! operator has to change.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use blastit_sys::{Fork, pipe};

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
/// that is not `path` itself — an entry that is not there yet and could be
/// *created* in a writable directory above it.
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
                "{} (mode {:04o}) is writable by another uid, and {} is on `sys.path` but \
                 is not there yet: it could be created",
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

/// What a probe asks of every path it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Read and traverse it: what the interpreter and the application are read
    /// through.
    Read,
    /// Execute it: what a child `execve`s.
    Execute,
    /// Write to it: what no path on `sys.path` may allow another uid.
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
    /// The read and execute checks ask "can this uid reach it", and a failure
    /// is the answer that matters. `Write` asks the opposite — "may this uid
    /// write it" — so the child reports the first path where `access(2)`
    /// *succeeds*, and "nothing was reported" means "nothing is writable".
    const fn wants_grant(self) -> bool {
        matches!(self, Self::Write)
    }
}

/// What one probe found: the path that answered the question, and how far above
/// it the directory that blocks is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Index into the list that was probed.
    pub index: usize,
    /// How many directories *above* that path the blocker is; `0` is the path
    /// itself. Only the read and execute questions walk upwards — what may be
    /// written is written where `sys.path` names it.
    pub depth: usize,
    /// The errno `access(2)` gave, when it gave one (the write question is
    /// answered by success, which has no errno).
    pub errno: i32,
}

/// Asks the kernel, as `uid` would, about every path in `paths`.
///
/// The answer comes from a short-lived child: it `fork`s, drops to `uid` — the
/// same setgroups/setgid/setuid sequence a view goes through, so the *group*
/// class and any ACL are the ones a view would meet — and calls `access(2)` on
/// each path in turn. When a path fails, the child walks its ancestors from the
/// outermost directory down and reports the first one that does not let `uid`
/// through, which is the directory an operator has to change first.
///
/// `interpreter` says whether CPython is live in this image and the caller holds
/// the GIL, exactly as [`crate::forking::fork`] does: a wheel install forks under
/// the interpreter's fork protocol (§9.3), and the standalone binary's stage 0
/// has no interpreter at all.
///
/// # Errors
///
/// When the probe could not run: the fork or the pipe failed, the child could
/// not drop to `uid` (which is what a test run as an ordinary user gets), or the
/// child did not answer in [`PROBE_TIMEOUT`]. The caller then falls back to
/// reading the mode bits, which is a guess the kernel does not have to agree
/// with — and the message says which of the two answered.
pub fn probe(
    paths: &[PathBuf],
    uid: u32,
    ask: Probe,
    interpreter: bool,
) -> io::Result<Option<Verdict>> {
    if paths.is_empty() {
        return Ok(None);
    }
    let (read_end, write_end) = pipe()?;
    match crate::forking::fork(interpreter)? {
        Fork::Child => {
            drop(read_end);
            // The child speaks in ten bytes and never returns: no unwinding, no
            // destructors, nothing of the parent's state that a `fork` could
            // have copied half-written.
            write_record(write_end.as_raw_fd(), &probe_as(uid, paths, ask));
            // SAFETY: `_exit` is async-signal-safe and never returns.
            unsafe { libc::_exit(0) }
        }
        Fork::Parent(pid) => {
            drop(write_end);
            let answer = read_record(read_end.as_raw_fd());
            // SAFETY: `waitpid` writes one status word into `status`, and the
            // child is this process's own.
            unsafe { libc::waitpid(pid.as_raw(), std::ptr::null_mut(), 0) };
            answer
        }
    }
}

/// The first byte of a record: what the child is reporting.
const RECORD_AGAIN: u8 = 0;
const RECORD_NOTHING: u8 = 1;
const RECORD_BLOCKED: u8 = 2;

/// Runs in the child: drops to `uid`, then asks `access(2)` about each path.
fn probe_as(uid: u32, paths: &[PathBuf], ask: Probe) -> [u8; 10] {
    if let Err(error) = blastit_sys::drop_privileges(uid) {
        // The kernel cannot be asked from here at all: the errno is the best
        // that can be said, and the caller falls back to the mode bits.
        let errno = error.raw_os_error().unwrap_or(libc::EPERM);
        return record(RECORD_NOTHING, 0, 0, errno);
    }
    for (index, path) in paths.iter().enumerate() {
        let Some(name) = cstring(path) else {
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
        // For the read and execute questions the path itself failed. Name the
        // first directory above it that does not let `uid` through — from the
        // outermost down, because that is the one an operator has to open first
        // — and the path itself when every ancestor is passable (the file is
        // simply not readable or not executable).
        let mut depth = 0usize;
        if !ask.wants_grant() {
            let ancestors: Vec<PathBuf> = path.ancestors().skip(1).map(Path::to_path_buf).collect();
            for (at, ancestor) in ancestors.iter().enumerate().rev() {
                let Some(name) = cstring(ancestor) else {
                    continue;
                };
                // SAFETY: as above; an ancestor only has to be traversable.
                if unsafe { libc::access(name.as_ptr(), libc::X_OK) } != 0 {
                    depth = at + 1;
                    break;
                }
            }
        }
        return record(RECORD_BLOCKED, index as u32, depth, errno);
    }
    record(RECORD_AGAIN, 0, 0, 0)
}

/// One ten-byte answer: what happened, which path, how far above it the blocker
/// is, and the errno.
fn record(kind: u8, index: u32, depth: usize, errno: i32) -> [u8; 10] {
    let mut out = [kind, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    out[1..5].copy_from_slice(&index.to_le_bytes());
    // A path deeper than 255 components is not a path this message can name
    // better than by its leaf, so the depth saturates.
    out[5] = u8::try_from(depth).unwrap_or(u8::MAX);
    out[6..10].copy_from_slice(&errno.to_le_bytes());
    out
}

fn cstring(path: &Path) -> Option<CString> {
    CString::new(path.as_os_str().as_bytes()).ok()
}

/// Writes the child's answer, in one `write`.
fn write_record(fd: RawFd, bytes: &[u8; 10]) {
    // SAFETY: `write` takes a descriptor, a buffer and its length.
    unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
}

/// Reads the child's answer, or `Err` when it did not come.
fn read_record(fd: RawFd) -> io::Result<Option<Verdict>> {
    // SAFETY: the descriptor is the read end of a pipe this process made and
    // still owns.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let mut bytes = [0u8; 10];
    for slot in &mut bytes {
        if !wait_readable(borrowed, PROBE_TIMEOUT)? {
            return Err(io::Error::other("the probe did not answer"));
        }
        match read_one(borrowed)? {
            Some(byte) => *slot = byte,
            None => return Err(io::Error::other("the probe wrote nothing")),
        }
    }
    let index = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    let depth = usize::from(bytes[5]);
    let errno = i32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
    match bytes[0] {
        RECORD_AGAIN => Ok(None),
        RECORD_BLOCKED => Ok(Some(Verdict {
            index,
            depth,
            errno,
        })),
        _ => Err(io::Error::other("the probe could not drop privileges")),
    }
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
pub fn unreadable(paths: &[PathBuf], dropped: &DroppedTo, interpreter: bool) -> Option<Blocker> {
    by_probe_or_mode(paths, dropped.probe, Probe::Read, interpreter, false)
}

/// The first program of `paths` a child uid could not `execve`.
///
/// dbproxy is `execve`'d as `BLASTIT_UID_BASE + 3`, which is not the owner of
/// anything the deployment installed: the file itself has to be executable by
/// that uid, and every directory on the way to it traversable. Without this the
/// child dies in `execve` with `Permission denied (os error 13)`, which says
/// nothing about which directory a deployment has to open — the same failure
/// mode [`unreadable`] refuses, one stage later.
pub fn unexecutable(paths: &[PathBuf], uid: u32, interpreter: bool) -> Option<Blocker> {
    by_probe_or_mode(paths, uid, Probe::Execute, interpreter, true)
}

/// The first path on `sys.path` another uid could write.
///
/// The question is the opposite of [`unreadable`]'s: not "can a view read it"
/// but "can a uid blastit drops to *write* it". These are the paths the
/// interpreter will import through — its own entries, the working directory
/// blastit was started in, whatever `PYTHONPATH` or a `.pth` file added, and a
/// zip file if one is on the path — because a writable one is a cross-route code
/// plant whoever put it there.
///
/// The kernel answers, and it is asked **once per uid**, because an ACL can
/// grant exactly one route's uid what it denies every other: the child reports
/// the first path that uid may write, and the refusal names the path and the
/// uid. Each entry is asked about as it is — a directory, or the zip file itself
/// — except when it is not there at all: then the question is its nearest
/// existing directory, which is where somebody could *create* the entry.
///
/// The mode bits are the fallback for a uid the probe could not drop to, and
/// they are also what *explains* a refusal: other-writable, group-writable when
/// the path's gid is that uid's own (a dropped-to process has no supplementary
/// groups), or sticky — a `/tmp`-style directory takes a file with a fresh name
/// whatever the sticky bit does about deleting one that is already there.
pub fn writable(paths: &[PathBuf], dropped: &DroppedTo, interpreter: bool) -> Option<Writable> {
    // One question per entry, in the order `sys.path` lists them.
    let questions: Vec<Question> = paths
        .iter()
        .filter_map(|entry| Question::of(entry))
        .collect();
    let targets: Vec<PathBuf> = questions.iter().map(|q| q.target.clone()).collect();

    // One fork per uid: the child reports the first target that uid may write,
    // and "nothing" is an answer too — it means the kernel was asked and found
    // nothing.
    let mut found: Vec<(usize, u32)> = Vec::new();
    let mut asked: BTreeSet<u32> = BTreeSet::new();
    for uid in &dropped.uids {
        match probe(&targets, *uid, Probe::Write, interpreter) {
            Ok(Some(verdict)) => found.push((verdict.index, *uid)),
            Ok(None) => {
                asked.insert(*uid);
            }
            // No answer: the mode bits speak for this uid below.
            Err(_) => {}
        }
    }
    found.sort_unstable();
    let first = found
        .first()
        .and_then(|(index, uid)| questions.get(*index).map(|question| (question, *uid)));
    if let Some((question, uid)) = first {
        let why = writable_by_mode(question.mode, question.gid, uid)
            .unwrap_or_else(|| "an ACL lets it through".to_string());
        return Some(question.refusal(Some(uid), why));
    }

    // The uids the kernel could not be asked for, and what the mode bits say
    // about them: the documented fallback, which cannot see an ACL either way.
    for question in &questions {
        for uid in dropped.uids.iter().filter(|uid| !asked.contains(uid)) {
            if let Some(why) = writable_by_mode(question.mode, question.gid, *uid) {
                return Some(question.refusal(None, why));
            }
        }
    }
    None
}

/// One `sys.path` entry, and what to ask about it.
struct Question {
    /// What must not be writable: the entry itself, or the nearest existing
    /// directory above an entry that is not there.
    target: PathBuf,
    /// The `sys.path` entry, when it is not `target`.
    entry: Option<PathBuf>,
    /// `target`'s permission bits.
    mode: u32,
    /// `target`'s gid, which is the group a dropped-to uid would have to be in.
    gid: u32,
}

impl Question {
    /// The question one `sys.path` entry asks, or `None` when there is nothing
    /// to ask.
    ///
    /// A path that is there is asked about as it is: a directory, or a file —
    /// a zip on `sys.path` is a file, and writing it is writing code. A path
    /// that is *not* there is asked about through its nearest existing
    /// directory, because that is where it could be created; an entry whose
    /// ancestors are all files, or which has no directory above it at all,
    /// cannot be created and is not this check's business.
    fn of(entry: &Path) -> Option<Self> {
        if let Ok(metadata) = std::fs::metadata(entry) {
            return Some(Self {
                target: entry.to_path_buf(),
                entry: None,
                mode: metadata.permissions().mode(),
                gid: metadata.gid(),
            });
        }
        let above = entry.ancestors().skip(1).find(|path| path.exists())?;
        let metadata = std::fs::metadata(above).ok()?;
        if !metadata.is_dir() {
            return None;
        }
        Some(Self {
            target: above.to_path_buf(),
            entry: Some(entry.to_path_buf()),
            mode: metadata.permissions().mode(),
            gid: metadata.gid(),
        })
    }

    /// The refusal this question produced.
    fn refusal(&self, probed_uid: Option<u32>, why: String) -> Writable {
        Writable {
            path: self.target.clone(),
            entry: self.entry.clone(),
            mode: self.mode & 0o7777,
            probed_uid,
            why,
        }
    }
}

/// Whether the mode bits alone say `uid` could write a path with this `mode` and
/// `gid`.
///
/// The classes this check names: **other** may write it; the **group** may write
/// it *and* the path's gid is the uid's own (a group class only counts for a uid
/// that is in that group, and a dropped-to process has no supplementary groups,
/// so the gid has to be its own); or it is **sticky** — a `/tmp`-style directory
/// takes a file with a fresh name whatever the sticky bit does about deleting
/// one that is there.
///
/// The uid is named when the group class is what answered, because the mode
/// bits were the only witness: the message has to say which uid a deployment has
/// to keep out.
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

/// Asks the kernel about `paths`, in the terms the checks use.
fn checked(paths: &[PathBuf], uid: u32, ask: Probe, interpreter: bool) -> Answer {
    match probe(paths, uid, ask, interpreter) {
        Ok(None) => Answer::Fine,
        Ok(Some(verdict)) => {
            let Some(path) = paths.get(verdict.index) else {
                return Answer::Fine;
            };
            // The blocker is `depth` directories above the path — the directory
            // an operator has to open first — and the path itself when nothing
            // above it blocks, which leaves the path's own access as the answer.
            let blocked = if verdict.depth == 0 {
                path.clone()
            } else {
                path.ancestors()
                    .nth(verdict.depth)
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| path.clone())
            };
            let mode = std::fs::metadata(&blocked)
                .map(|metadata| metadata.permissions().mode() & 0o777)
                .unwrap_or(0);
            let access = match (verdict.depth, ask) {
                // A directory on the way is only traversed.
                (depth, _) if depth > 0 => Access::Traverse,
                (_, Probe::Read) => Access::Read,
                (_, Probe::Execute) => Access::Execute,
                (_, Probe::Write) => Access::Write,
            };
            Answer::Blocked(Blocker {
                path: blocked,
                mode,
                access,
                probed_uid: Some(uid),
            })
        }
        Err(_) => Answer::Unasked,
    }
}

/// What one check got back.
enum Answer {
    /// The kernel says every path is fine.
    Fine,
    /// The kernel names a path that blocks.
    Blocked(Blocker),
    /// The probe could not run at all, so the mode bits are the only answer
    /// there is.
    Unasked,
}

/// One check: the kernel's answer when it can be asked, the mode bits when it
/// cannot.
///
/// The fallback runs on [`Answer::Unasked`] only. A probe that answered "every
/// path is fine" is an answer, and answering it again with the mode bits would
/// refuse exactly the deployments the probe exists for — a directory a view
/// reaches through a group class, or an ACL nobody can read out of the bits.
fn by_probe_or_mode(
    paths: &[PathBuf],
    uid: u32,
    ask: Probe,
    interpreter: bool,
    executable: bool,
) -> Option<Blocker> {
    match checked(paths, uid, ask, interpreter) {
        Answer::Blocked(blocker) => Some(blocker),
        Answer::Fine => None,
        Answer::Unasked => paths
            .iter()
            .find_map(|path| blocks(path, executable).map(unprobed)),
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
/// by walking through it — while a program is executed and a file is read. The
/// order is the order the message names them in: readability first, because
/// that is the one an operator usually has to change.
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

    /// The layout a probe speaks for in these tests.
    fn dropped() -> DroppedTo {
        DroppedTo::of(blastit_manifest::UidLayout::new(PROBE_BASE), 2)
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
                    Ok(None) => 0,
                    Ok(Some(_)) => 1,
                    Err(_) => 2,
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
                && refusal.message().contains("not there yet"),
            "the message explains how it could be created: {}",
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
