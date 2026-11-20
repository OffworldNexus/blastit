//! `blastit dev`'s live-reload policy: what is watched, and when one reload fires.
//!
//! The kernel watcher ([`blastit_sys::fanotify`]) knows nothing about views,
//! routes or the files an application loaded. This module is that policy
//! (`docs/design/dev-fanotify-reload.md` §4.4–§4.6, `ADR-0014`):
//!
//! * **The watched set** is the union of every source's loaded-file report —
//!   the `preload` phase's and every view worker's — plus the manifest. A file
//!   is watched because it was *loaded*, not because it lives under a root,
//!   which is what makes an editable package or template outside the working
//!   directory reload at all.
//! * **One unit.** Any change to a tracked path produces exactly one
//!   [`Effect::Reload`]; there is no route, no per-view selection and no
//!   dependency map (§6). The supervisor turns that into one `Ctl::Reload`.
//! * **The debounce** is a function of the observed `Instant` alone: 50 ms
//!   quiet, 250 ms hard maximum, 500 ms floor between two reloads. No `sleep`
//!   anywhere in this module decides a verdict.
//! * **Every failure is inert.** A missing kernel feature, a denied container
//!   profile, a filesystem without file handles or a watcher that dies mid-run
//!   becomes one [`Unavailable`] and "serve without reload"; there is no retry
//!   and no second watcher (§4.7).
//!
//! # What a path may be, and what may trigger
//!
//! A report may name a path under the skip list or a framework cache directory
//! — it is still in the union, because the app did load it — but such a path is
//! never marked and never triggers: cache writes are dropped before the union
//! lookup, so a reload can never be caused by its own re-import, and
//! `__pycache__` writes report nothing at all (§4.8, `S8`). Everything else in
//! the union is marked by its parent directory (or by itself, for a directory)
//! and any `FAN_CLOSE_WRITE|FAN_MOVED_TO|FAN_MOVED_FROM|FAN_CREATE|FAN_DELETE`
//! on it is a tracked change.
//!
//! # Seams
//!
//! Two, both injected: [`ReloadWatcher`] (the real implementation over
//! `blastit_sys::fanotify::Watcher`) and [`Clock`] (the real one).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsStr;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use blastit_events::ReloadReason;
use blastit_sys::fanotify::{Event, Readout, Watcher, WatcherError};
use blastit_view::loaded::{Sequence, decode};

use rustix::event::PollFlags;

/// The `source` of the `preload` phase (§4.4).
///
/// It reports once, before the first view is forked; every other source is a
/// route index. The value is a self-check only — attribution is the descriptor
/// the datagram arrived on, never a field the sender chose.
pub const PRELOAD_SOURCE: u16 = 0xffff;

/// How long the window waits for a save burst to go quiet (§4.5).
pub const QUIET: Duration = Duration::from_millis(50);

/// The longest a stream of saves may postpone the reload (§4.5).
pub const MAX_WAIT: Duration = Duration::from_millis(250);

/// The least time between two reloads, whatever arrives (§4.5).
pub const FLOOR: Duration = Duration::from_millis(500);

/// The most tracked paths in the union (§4.10).
pub const MAX_PATHS: usize = 16_384;

/// The most directories this policy will ask to mark (§4.10).
///
/// A breach is the ladder's `ENOSPC` row: stop marking, keep the marks already
/// placed.
pub const MAX_MARKS: usize = 8_192;

/// The most bytes one tracked path may take (§4.10).
pub const MAX_PATH_BYTES: usize = 4096;

/// Directories that are never marked and never trigger (§1.5).
///
/// A path under one may still be *reported* — the app loaded it — but watching
/// VCS metadata, dependency trees or bytecode caches would only ever produce
/// reloads of files nothing serves.
pub const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    "target",
];

/// Framework cache directories, **relative to the app root** (§1.5).
///
/// These *are* marked — so the event exists — and every event under one is
/// dropped before the union lookup, so a cache write can never trigger the
/// reload that would rewrite it (§4.8).
pub const CACHE_DIRS: &[&str] = &["var/cache", "storage/framework/views"];

/// Cache directories matched by name anywhere: `dist` and `.next` are build
/// outputs wherever a framework puts them.
pub const CACHE_NAMES: &[&str] = &[".next", "dist"];

/// The event bits that make an event a trackable change (§1.6).
///
/// `FAN_MODIFY` is deliberately absent: a write without a close delivers no
/// event from a group created with this mask, and a synthetic one is ignored
/// here (`T2`).
#[must_use]
pub fn trigger_mask() -> u64 {
    libc::FAN_CLOSE_WRITE
        | libc::FAN_MOVED_TO
        | libc::FAN_MOVED_FROM
        | libc::FAN_CREATE
        | libc::FAN_DELETE
}

/// The source of a loaded-file report: [`PRELOAD_SOURCE`], or a route index.
pub type Source = u16;

/// The watcher seam: mark a directory, drain what the kernel queued (§4.2).
///
/// Exactly two operations, so an implementation can be a kernel, a scripted
/// sequence, or a hand-written test double without the policy knowing which.
pub trait ReloadWatcher {
    /// Mark `dir` with the inode mark and the frozen mask.
    ///
    /// # Errors
    ///
    /// Whatever `fanotify_mark`/`name_to_handle_at` reported: the policy's
    /// ladder classifies it from the typed [`WatcherError`] when one is
    /// carried.
    fn mark_dir(&mut self, dir: &Path) -> io::Result<()>;

    /// Drain the events the kernel has queued.
    ///
    /// # Errors
    ///
    /// `EAGAIN`/`EINTR` are an empty, non-overflow [`Readout`], not an error;
    /// a real failure latches the watcher dead (§4.7).
    fn read(&mut self, out: &mut Vec<Event>) -> io::Result<Readout>;
}

/// The one real watcher: `blastit_sys::fanotify::Watcher` (§4.2).
///
/// A thin adapter, because `blastit-sys` owns the syscall surface and this
/// module owns the trait; the supervisor duplicates [`FanotifyWatcher::fd`]
/// before it boxes this, so the trait keeps its two methods (§4.5).
pub struct FanotifyWatcher {
    inner: Watcher,
}

impl FanotifyWatcher {
    /// Opens the unprivileged file-handle group (`0x0c03`, `O_RDONLY`).
    ///
    /// # Errors
    ///
    /// The failure ladder's errno, as `fanotify_init` reported it.
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            inner: Watcher::new()?,
        })
    }

    /// The group descriptor, for the supervisor's poll duplicate.
    #[must_use]
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.inner.fd()
    }
}

impl ReloadWatcher for FanotifyWatcher {
    fn mark_dir(&mut self, dir: &Path) -> io::Result<()> {
        self.inner.mark_dir(dir)
    }

    fn read(&mut self, out: &mut Vec<Event>) -> io::Result<Readout> {
        self.inner.read(out)
    }
}

/// The one real watcher of a boot, with the poll duplicate it is polled on.
///
/// The duplicate is made here, **before** the watcher is boxed, so there is one
/// place that knows the rule: it must be a `F_DUPFD_CLOEXEC` clone, because a
/// duplicate without that flag would survive the reload `execve` and keep the
/// old group (and its marks) alive until `max_user_groups` ran out (§4.1).
///
/// # Errors
///
/// The ladder's errno from `fanotify_init`, or from the duplicate.
pub fn open() -> io::Result<(FanotifyWatcher, OwnedFd)> {
    let watcher = FanotifyWatcher::new()?;
    let poll = watcher.fd().try_clone_to_owned()?;
    Ok((watcher, poll))
}

/// The clock seam: the only thing the policy reads time from (§4.3).
pub trait Clock {
    /// The instant this observation happens at.
    fn now(&mut self) -> Instant;
}

/// The real clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&mut self) -> Instant {
        Instant::now()
    }
}

/// Everything the policy needs that is fixed at boot (§4.5).
#[derive(Debug, Clone)]
pub struct ReloadConfig {
    /// The app root: marked in addition to every tracked path's parent.
    pub root: PathBuf,
    /// `./Blastfile`, absolute: always in the union, class `Blastfile`.
    pub manifest: PathBuf,
    /// The filesystem type [`blastit_sys::fanotify::filesystem_type`] reported,
    /// or `"unknown"`.
    pub filesystem: String,
    /// The union's path cap (§4.10).
    pub max_paths: usize,
    /// The marks cap (§4.10).
    pub max_marks: usize,
}

impl ReloadConfig {
    /// The design's defaults: [`MAX_PATHS`], [`MAX_MARKS`], and the filesystem
    /// the boot read.
    #[must_use]
    pub fn new(root: PathBuf, manifest: PathBuf, filesystem: String) -> Self {
        Self {
            root,
            manifest,
            filesystem,
            max_paths: MAX_PATHS,
            max_marks: MAX_MARKS,
        }
    }
}

/// Why a path is in the union — and therefore which `reason` word it fires with.
///
/// The variant order is the precedence: `Blastfile` wins over `Preload`, which
/// wins over `Loaded` (§4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// A file a worker or the `preload` phase loaded.
    Loaded,
    /// A file of a module the manifest's `preload` list names.
    Preload,
    /// `./Blastfile` itself.
    Blastfile,
}

impl Class {
    /// The `dev.reload` `reason` word this class fires with (§4.9).
    #[must_use]
    pub fn reason(self) -> ReloadReason {
        match self {
            Class::Loaded => ReloadReason::Loaded,
            Class::Preload => ReloadReason::Preload,
            Class::Blastfile => ReloadReason::Blastfile,
        }
    }
}

/// One tracked path: why it is watched, and whether the boot imported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Which `reason` word its change fires with.
    pub class: Class,
    /// It was imported before the first fork, so the boot heap is stale and its
    /// change takes the reload cycle (§4.6).
    pub pre_fork: bool,
    /// Whether the path is a directory, read from the **one** `stat` the absorb
    /// already makes (§4.4).
    ///
    /// The marking pass must not `stat` the union again: it runs after every
    /// report datagram and every tick, so a second look at each tracked file
    /// would put one syscall per tracked path on the request path.
    pub is_dir: bool,
}

/// What one reported batch meant (§4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report {
    /// The batch was applied to its source's set.
    Applied {
        /// This batch replaced the source's set (a fresh worker).
        boot: bool,
        /// The source says "treat everything I reported as changed".
        resync: bool,
        /// How many paths this batch added to the source's set.
        added: usize,
    },
    /// The batch was dropped whole.
    Dropped(Drop),
}

/// Why a datagram — or a path in one — was dropped.
///
/// Dropping a *report* is counted once, never logged per path (§4.10). An event
/// under a skipped or cached path is not a drop at all: it is ignored at the
/// union lookup (§4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Drop {
    /// The union's path cap was reached (§4.10).
    Cap,
    /// The path does not exist as a file or directory right now.
    Missing,
    /// The path is not UTF-8, so the `files` array could not carry it.
    NotUtf8,
    /// The path is relative; the union is absolute only.
    NotAbsolute,
    /// The path is longer than [`MAX_PATH_BYTES`].
    TooLong,
    /// The datagram could not be decoded, or its `source` is not the one whose
    /// descriptor it arrived on.
    Malformed,
    /// The descriptor is not a registered report end.
    Mismatch,
}

/// What one batch changed in a source's set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Absorbed {
    /// Paths that were not in the source's set before.
    pub added: usize,
    /// Paths (and, for a dropped datagram, the batch) that were refused.
    pub dropped: Vec<Drop>,
}

impl Absorbed {
    /// Whether anything was refused.
    #[must_use]
    pub fn refused(&self) -> bool {
        !self.dropped.is_empty()
    }
}

/// One source's report end and set, and the union they derive (§4.4).
///
/// The ends are keyed by the descriptor itself, never a number that could be
/// reused: a socketpair that recycles a reaped view's number must not inherit
/// its source. A path is in the union once and stays for the boot's life (a
/// mark is never unmapped), so revoking a source drops its *set* without
/// un-tracking files another view may still serve.
#[derive(Debug, Default)]
pub struct ReportTable {
    ends: HashMap<RawFd, End>,
    by_source: HashMap<Source, RawFd>,
    union: BTreeMap<PathBuf, Entry>,
    truncated: bool,
}

/// One registered report end.
#[derive(Debug)]
struct End {
    fd: OwnedFd,
    source: Source,
    class: Class,
    pre_fork: bool,
    seq: Sequence,
    paths: BTreeSet<PathBuf>,
}

impl ReportTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the supervisor's end of one view's (or the preload phase's)
    /// reporting socket.
    ///
    /// Called by the supervisor at `spawn_now`, before the fork, and once at
    /// boot for [`PRELOAD_SOURCE`]. A source that already has an end — a
    /// route's stale worker replaced by a fresh one that has not been reaped
    /// yet — gives its previous end back here: the table keys ends by
    /// descriptor, so leaving both would keep a dead peer in the poll set and
    /// leak its read end for the boot's life.
    pub fn register(&mut self, end: OwnedFd, source: Source, class: Class, pre_fork: bool) {
        let raw = end.as_raw_fd();
        if let Some(previous) = self.by_source.insert(source, raw)
            && previous != raw
        {
            self.ends.remove(&previous);
        }
        self.ends.insert(
            raw,
            End {
                fd: end,
                source,
                class,
                pre_fork,
                seq: Sequence::new(),
                paths: BTreeSet::new(),
            },
        );
    }

    /// Closes a source's end and forgets its set (§4.4).
    ///
    /// A closed report end is revoked in the same wakeup that noticed
    /// `POLLHUP`, so the poll set never spins on a dead peer.
    pub fn revoke(&mut self, source: Source) {
        if let Some(raw) = self.by_source.remove(&source)
            && let Some(end) = self.ends.remove(&raw)
        {
            drop(end);
        }
    }

    /// The source a report end belongs to.
    #[must_use]
    pub fn source_of(&self, fd: RawFd) -> Option<Source> {
        self.ends.get(&fd).map(|end| end.source)
    }

    /// Every live report end's descriptor, for the supervisor's poll set.
    pub fn sources(&self) -> impl Iterator<Item = RawFd> + '_ {
        self.ends.keys().copied()
    }

    /// Reads one datagram from a registered end.
    ///
    /// # Errors
    ///
    /// The socket's error: a peer that closed reads as end-of-file, which is
    /// the supervisor's `revoke`, not a policy decision.
    pub fn recv(&self, fd: RawFd, out: &mut [u8]) -> io::Result<usize> {
        match self.ends.get(&fd) {
            // rustix returns the datagram it filled and the length the kernel
            // reported; a datagram is never truncated here because the buffer is
            // the codec's own bound.
            Some(end) => rustix::net::recv(&end.fd, out, rustix::net::RecvFlags::empty())
                .map(|(filled, _)| filled)
                .map_err(io::Error::from),
            None => Err(io::Error::from_raw_os_error(libc::EBADF)),
        }
    }

    /// The union: every tracked path with the class it fires as (§4.4).
    #[must_use]
    pub fn union(&self) -> &BTreeMap<PathBuf, Entry> {
        &self.union
    }

    /// One source's whole set after the last applied batch.
    #[must_use]
    pub fn set_of(&self, source: Source) -> Vec<PathBuf> {
        self.by_source
            .get(&source)
            .and_then(|raw| self.ends.get(raw))
            .map(|end| end.paths.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The directories to mark: every union path's parent, a directory itself,
    /// and the root — minus the skip list (§4.4).
    ///
    /// A framework cache directory *is* returned: it is marked so its events
    /// exist, and dropped later at the lookup (§4.8). Nothing here touches the
    /// filesystem: [`Entry::is_dir`] was decided by the absorb's single `stat`,
    /// because this runs after every report datagram and every tick.
    #[must_use]
    pub fn dirs_to_mark(&self, root: &Path) -> Vec<PathBuf> {
        let mut dirs = BTreeSet::new();
        if !is_skipped(root) {
            dirs.insert(root.to_path_buf());
        }
        for (path, entry) in &self.union {
            if is_skipped(path) {
                continue;
            }
            if entry.is_dir {
                dirs.insert(path.clone());
            }
            if let Some(parent) = path.parent()
                && !is_skipped(parent)
            {
                dirs.insert(parent.to_path_buf());
            }
        }
        dirs.into_iter().collect()
    }

    /// Applies one datagram, keyed by the descriptor it arrived on.
    ///
    /// The descriptor decides the source; the batch's own `source` is a
    /// self-check, and a mismatch drops the datagram, so a view can only ever
    /// speak as itself (§4.4, `S2`).
    pub fn on_datagram(&mut self, fd: RawFd, bytes: &[u8], max_paths: usize) -> Report {
        let Some(end) = self.ends.get(&fd) else {
            return Report::Dropped(Drop::Mismatch);
        };
        let source = end.source;
        let class = end.class;
        let pre_fork = end.pre_fork;
        let batch = match decode(bytes) {
            Ok(batch) => batch,
            Err(_) => return Report::Dropped(Drop::Malformed),
        };
        if batch.source != source {
            return Report::Dropped(Drop::Mismatch);
        }
        let step = match self.ends.get_mut(&fd) {
            Some(end) => end.seq.accept(&batch),
            None => return Report::Dropped(Drop::Mismatch),
        };
        let paths: Vec<PathBuf> = batch.paths.iter().map(PathBuf::from).collect();
        let absorbed = self.absorb(source, class, pre_fork, &paths, step.rebuilds(), max_paths);
        Report::Applied {
            boot: step.boot,
            resync: step.resync,
            added: absorbed.added,
        }
    }

    /// Adds `paths` to `source`'s set (replacing it when `replace`) and to the
    /// union.
    ///
    /// A path that is relative, non-UTF-8, too long, missing, or past the
    /// union's cap is refused once and never marked (`T11`, `T32`).
    pub fn absorb(
        &mut self,
        source: Source,
        class: Class,
        pre_fork: bool,
        paths: &[PathBuf],
        replace: bool,
        max_paths: usize,
    ) -> Absorbed {
        let mut absorbed = Absorbed::default();
        let Some(raw) = self.by_source.get(&source).copied() else {
            absorbed.dropped.push(Drop::Mismatch);
            return absorbed;
        };
        if replace && let Some(end) = self.ends.get_mut(&raw) {
            end.paths.clear();
        }
        for path in paths {
            let is_dir = match self.path_refusal(path, max_paths) {
                Ok(is_dir) => is_dir,
                Err(drop) => {
                    absorbed.dropped.push(drop);
                    continue;
                }
            };
            let fresh = match self.ends.get_mut(&raw) {
                Some(end) => end.paths.insert(path.clone()),
                None => false,
            };
            if fresh {
                absorbed.added += 1;
            }
            let entry = self.union.entry(path.clone()).or_insert(Entry {
                class,
                pre_fork,
                is_dir,
            });
            entry.class = entry.class.max(class);
            entry.pre_fork |= pre_fork;
            entry.is_dir = is_dir;
        }
        absorbed
    }

    /// Whether `path` may enter the union, and why not (§4.4, §4.10).
    ///
    /// On success, whether it is a directory — the one `stat` this path ever
    /// gets, kept in [`Entry::is_dir`] so the marking pass never repeats it.
    fn path_refusal(&mut self, path: &Path, max_paths: usize) -> Result<bool, Drop> {
        if !path.is_absolute() {
            return Err(Drop::NotAbsolute);
        }
        let bytes = path.as_os_str().as_bytes();
        if bytes.len() > MAX_PATH_BYTES {
            return Err(Drop::TooLong);
        }
        if std::str::from_utf8(bytes).is_err() {
            return Err(Drop::NotUtf8);
        }
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return Err(Drop::Missing);
        };
        match self.union.contains_key(path) {
            true => Ok(meta.is_dir()),
            false if self.union.len() >= max_paths => {
                self.truncated = true;
                Err(Drop::Cap)
            }
            false => Ok(meta.is_dir()),
        }
    }

    /// Whether the union's cap has been hit in this boot.
    #[must_use]
    pub fn cap_reached(&self) -> bool {
        self.truncated
    }

    /// Seeds the union with the manifest: always present, always `Blastfile`
    /// (§4.4).
    pub fn track_manifest(&mut self, manifest: &Path) {
        self.union.insert(
            manifest.to_path_buf(),
            Entry {
                class: Class::Blastfile,
                pre_fork: true,
                is_dir: false,
            },
        );
    }

    /// Seeds the union with paths the **boot** already serves, before any
    /// worker has reported (`MB.40.9`).
    ///
    /// A view's first report is its `boot` batch, and that batch is what puts
    /// its files in the union — so an app whose front controller is edited
    /// before the first request was not watched at all, and the save was lost
    /// until something else armed the window. The one caller is stage 1 on a
    /// `dev` boot with reload on: it hands over every `entrypoint php` and the
    /// application root of each PHP slot, which are exactly the paths a PHP
    /// worker would report one request later.
    ///
    /// `pre_fork` is `false`: a PHP file is not in any view's heap, so a change
    /// to it is the all-views transaction, never the cycle. A path that does
    /// not stat right now is skipped — a boot never invents a file.
    pub fn track_boot_files(&mut self, paths: &[PathBuf], class: Class) {
        for path in paths {
            let Ok(metadata) = std::fs::metadata(path) else {
                continue;
            };
            let is_dir = metadata.is_dir();
            if !is_dir && !metadata.is_file() {
                continue;
            }
            self.union.entry(path.clone()).or_insert(Entry {
                class,
                pre_fork: false,
                is_dir,
            });
        }
    }
}

/// Whether `path` is under the skip list (§1.5).
#[must_use]
pub fn is_skipped(path: &Path) -> bool {
    path.components().any(|component| match component {
        std::path::Component::Normal(name) => SKIP_DIRS.iter().any(|skip| OsStr::new(skip) == name),
        _ => false,
    })
}

/// Whether `path` is under a framework cache directory (§1.5).
#[must_use]
pub fn is_cached(path: &Path, root: &Path) -> bool {
    if path.components().any(|component| match component {
        std::path::Component::Normal(name) => {
            CACHE_NAMES.iter().any(|cache| OsStr::new(cache) == name)
        }
        _ => false,
    }) {
        return true;
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    CACHE_DIRS.iter().any(|cache| {
        let cache = Path::new(cache);
        relative == cache || relative.starts_with(cache)
    })
}

/// Where the debounce window is (§4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Nothing has arrived since the last reload.
    Idle,
    /// A change arrived at `first`; the newest one at `last`.
    Armed { first: Instant, last: Instant },
}

/// What one reload asks the supervisor to do (§4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fire {
    /// The tracked paths that changed, deduplicated and sorted.
    pub files: Vec<PathBuf>,
    /// What changed — the `dev.reload` `reason` word (§4.9).
    pub reason: ReloadReason,
    /// Whether this change needs the reload **cycle** (a fresh interpreter
    /// image) rather than the all-views transaction (§4.6).
    ///
    /// True for the `Blastfile`, for anything the boot imported before the
    /// fork (`pre_fork`) and for an overflow, whose dropped events may have
    /// included the `Blastfile`. The `reason` word says what changed; this says
    /// what init has to do about it.
    cycle: bool,
}

impl Fire {
    /// Whether the supervisor must re-parse and re-exec this image (§4.6).
    #[must_use]
    pub const fn needs_cycle(&self) -> bool {
        self.cycle
    }
}

/// Why live reload cannot run here, in the terms the ladder and the event use
/// (§4.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable {
    /// The errno the failing syscall reported.
    pub errno: i32,
    /// The filesystem type, or `"unknown"`.
    pub filesystem: String,
}

impl Unavailable {
    /// Classifies a watcher failure.
    ///
    /// The typed [`WatcherError`] is the classification, never
    /// `io::Error::raw_os_error` — it is `None` on the errors this module
    /// returns, and reading it would lose the errno the ladder printed.
    #[must_use]
    pub fn of(err: &io::Error, filesystem: &str) -> Self {
        Self {
            errno: errno_of(err),
            filesystem: filesystem.to_string(),
        }
    }

    /// The symbolic errno the event carries: the ladder's names, else
    /// `E<number>` (§4.7).
    #[must_use]
    pub fn errno_name(&self) -> String {
        errno_symbol(self.errno)
    }
}

/// The errno behind a watcher failure, typed first (§4.2).
fn errno_of(err: &io::Error) -> i32 {
    match WatcherError::of(err) {
        Some(typed) => typed.errno(),
        None => err.raw_os_error().unwrap_or(libc::EIO),
    }
}

/// The symbolic errno a log line spells (§4.7).
#[must_use]
pub fn errno_symbol(errno: i32) -> String {
    match errno {
        libc::ENOSYS => "ENOSYS".to_string(),
        libc::EPERM => "EPERM".to_string(),
        libc::EINVAL => "EINVAL".to_string(),
        libc::EOPNOTSUPP => "EOPNOTSUPP".to_string(),
        libc::ENOSPC => "ENOSPC".to_string(),
        libc::EMFILE => "EMFILE".to_string(),
        libc::EIO => "EIO".to_string(),
        other => format!("E{other}"),
    }
}

/// What one policy step produced (§4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// One reload of every view, for these files and this reason.
    Reload(Fire),
    /// Marking ran out of room: the `ENOSPC` row of the ladder (§4.7).
    ///
    /// It is one `dev.reload.unavailable` and the boot keeps serving: the marks
    /// already placed keep watching, and nothing new is asked for. A boot emits
    /// it in place of the boot line; a running boot emits it after a fire (so
    /// one wakeup may carry a `Reload` and this).
    Unavailable(Unavailable),
    /// A running watcher died: one warning, and no retry (§4.7).
    Stopped(Unavailable),
}

/// What marking the pending directories did (§4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkOutcome {
    /// Every pending directory is marked.
    Marked,
    /// No marks are left — `ENOSPC`, or the `MAX_MARKS` cap: the marks already
    /// placed keep working, and nothing new is asked for (§4.7, `T32`).
    Exhausted(Unavailable),
    /// A mark failed for another reason: the watcher is dead.
    Dead(Unavailable),
}

/// The union, the classes and the debounce window — time injected (§4.5).
#[derive(Debug)]
pub struct Policy {
    cfg: ReloadConfig,
    reports: ReportTable,
    phase: Phase,
    /// Paths that changed but have not fired yet.
    changed: BTreeSet<PathBuf>,
    last_fire: Option<Instant>,
    /// Directories this policy has already asked the watcher to mark.
    requested: BTreeSet<PathBuf>,
    overflow: bool,
    dead: bool,
    marking_stopped: bool,
    /// The `MAX_MARKS` cap latched in [`Policy::pending_dirs`] and not yet
    /// reported to [`Policy::take_cap_hit`] (§4.7, `T32`).
    cap_hit: bool,
}

impl Policy {
    /// A policy whose union already holds the manifest (§4.4).
    #[must_use]
    pub fn new(cfg: ReloadConfig) -> Self {
        let mut reports = ReportTable::new();
        reports.track_manifest(&cfg.manifest);
        Self {
            cfg,
            reports,
            phase: Phase::Idle,
            changed: BTreeSet::new(),
            last_fire: None,
            requested: BTreeSet::new(),
            overflow: false,
            dead: false,
            marking_stopped: false,
            cap_hit: false,
        }
    }

    /// The report table, for the supervisor's `register`/`revoke` calls.
    pub fn reports(&mut self) -> &mut ReportTable {
        &mut self.reports
    }

    /// Seeds the union with the paths the boot itself serves (`MB.40.9`).
    ///
    /// Stage 1 calls this once, after the watcher exists and before the marking
    /// pass: an `entrypoint php` edited before the first request is watched from
    /// boot, exactly as if its worker had already reported it.
    pub fn track_boot_files(&mut self, paths: &[PathBuf], class: Class) {
        self.reports.track_boot_files(paths, class);
    }

    /// The union, read-only.
    #[must_use]
    pub fn union(&self) -> &BTreeMap<PathBuf, Entry> {
        self.reports.union()
    }

    /// The configuration the policy was built with.
    #[must_use]
    pub fn config(&self) -> &ReloadConfig {
        &self.cfg
    }

    /// The directories that still need a mark, drained once (§4.5).
    ///
    /// Empty once [`Policy::stop_marking`] latched `ENOSPC`: the marks already
    /// placed keep working, and nothing new is asked for. Reaching the
    /// [`ReloadConfig::max_marks`] cap is the same row of the ladder — the
    /// kernel has no room for another mark — so it latches here too and is
    /// reported once through [`Policy::take_cap_hit`] (design `T32`).
    pub fn pending_dirs(&mut self) -> Vec<PathBuf> {
        if self.dead || self.marking_stopped {
            return Vec::new();
        }
        let mut pending = Vec::new();
        for dir in self.reports.dirs_to_mark(&self.cfg.root) {
            if self.requested.contains(&dir) {
                continue;
            }
            if self.requested.len() >= self.cfg.max_marks {
                self.marking_stopped = true;
                self.cap_hit = true;
                break;
            }
            self.requested.insert(dir.clone());
            pending.push(dir);
        }
        pending
    }

    /// Whether the `MAX_MARKS` cap latched since the last call, reported once.
    pub fn take_cap_hit(&mut self) -> bool {
        std::mem::take(&mut self.cap_hit)
    }

    /// The `ENOSPC` latch: stop marking and keep every mark already placed.
    pub fn stop_marking(&mut self) {
        self.marking_stopped = true;
    }

    /// How many directories this policy has asked the watcher to mark.
    ///
    /// The boot line's `N` (`reload: on (N directories)`, §4.7): the marks the
    /// boot placed, not the paths the union holds — several loaded files share
    /// one parent. It never grows once [`Policy::stop_marking`] latched.
    #[must_use]
    pub fn marked_dirs(&self) -> usize {
        self.requested.len()
    }

    /// Whether the watcher latched dead (§4.7).
    #[must_use]
    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// Whether marking latched off (§4.7).
    #[must_use]
    pub fn marking_stopped(&self) -> bool {
        self.marking_stopped
    }

    /// Latches the watcher dead: the group is gone, so nothing more can fire.
    pub fn kill(&mut self, _why: Unavailable) {
        self.dead = true;
        self.phase = Phase::Idle;
        self.changed.clear();
    }

    /// Applies one datagram and arms the window if it says to (§4.5).
    pub fn on_datagram(&mut self, fd: RawFd, bytes: &[u8], now: Instant) -> Vec<Effect> {
        let max_paths = self.cfg.max_paths;
        let report = self.reports.on_datagram(fd, bytes, max_paths);
        let paths = match self.reports.source_of(fd) {
            Some(source) => self.reports.set_of(source),
            None => Vec::new(),
        };
        self.apply_report(report, &paths, now)
    }

    /// The one entry point for a batch after its datagram was applied (§4.5).
    ///
    /// `paths` is the source's whole set after the batch. A `boot` batch
    /// replaces a set without arming — otherwise the first batch of every
    /// reloaded view would fire the reload it was born from (`T30`). A
    /// `resync` batch arms: the sender lost a datagram, so everything it ever
    /// reported may have changed.
    pub fn apply_report(&mut self, report: Report, paths: &[PathBuf], now: Instant) -> Vec<Effect> {
        match report {
            Report::Dropped(_) => Vec::new(),
            Report::Applied { resync, .. } => {
                if !resync || self.dead {
                    return Vec::new();
                }
                for path in paths {
                    self.note_change(path, now);
                }
                self.tick(now)
            }
        }
    }

    /// Folds one readout's events into the window (§4.5).
    ///
    /// Events under the skip list or a cache directory are dropped before the
    /// union lookup, so a worker writing a cache file can never trigger the
    /// reload that would rewrite it (§4.8). An overflow arms the window at
    /// once with reason `overflow`: the changed set is unknown, so exactly one
    /// all-views reload is the only honest answer.
    pub fn observe(&mut self, events: &[Event], readout: Readout, now: Instant) -> Vec<Effect> {
        if self.dead {
            return Vec::new();
        }
        let triggers = trigger_mask();
        for event in events {
            if event.mask & triggers == 0 || event.name.is_empty() {
                continue;
            }
            let joined = event.dir.join(OsStr::from_bytes(&event.name));
            if is_skipped(&joined) || is_cached(&joined, &self.cfg.root) {
                continue;
            }
            if self.reports.union().contains_key(&joined) {
                self.note_change(&joined, now);
            }
        }
        if readout.overflow {
            self.overflow = true;
            self.arm(now);
        }
        self.tick(now)
    }

    /// Records one changed tracked path and arms the window (§4.5).
    ///
    /// A path that is not in the union is not a tracked change: nothing loaded
    /// it, so no view can be serving an old copy of it.
    pub fn note_change(&mut self, path: &Path, now: Instant) {
        if self.dead || !self.reports.union().contains_key(path) {
            return;
        }
        self.changed.insert(path.to_path_buf());
        self.arm(now);
    }

    /// Moves the window: `first` is pinned by its first change, `last` by the
    /// newest.
    fn arm(&mut self, now: Instant) {
        self.phase = match self.phase {
            Phase::Idle => Phase::Armed {
                first: now,
                last: now,
            },
            Phase::Armed { first, .. } => Phase::Armed { first, last: now },
        };
    }

    /// Fires when the window is due (§4.5).
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        if self.dead {
            return Vec::new();
        }
        let due = match self.phase {
            Phase::Idle => return Vec::new(),
            Phase::Armed { first, last } => self.deadline(first, last),
        };
        if now < due {
            return Vec::new();
        }
        vec![Effect::Reload(self.fire(now))]
    }

    /// `max(last_fire + 500 ms, min(last + 50 ms, first + 250 ms))` (§4.5).
    #[must_use]
    pub fn deadline(&self, first: Instant, last: Instant) -> Instant {
        let quiet = last.checked_add(QUIET).unwrap_or(last);
        let hard_max = first.checked_add(MAX_WAIT).unwrap_or(first);
        let mut due = quiet.min(hard_max);
        if let Some(fired) = self.last_fire {
            due = due.max(fired.checked_add(FLOOR).unwrap_or(fired));
        }
        due
    }

    /// The instant this policy next needs a [`Policy::tick`], while armed.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Idle => None,
            Phase::Armed { first, last } => Some(self.deadline(first, last)),
        }
    }

    /// Takes the accumulated set, deduplicated and sorted, and clears it.
    fn fire(&mut self, now: Instant) -> Fire {
        let files: Vec<PathBuf> = std::mem::take(&mut self.changed).into_iter().collect();
        let mut class = Class::Loaded;
        // The action and the reason word are decided separately (§4.5): the
        // reason says what changed (`blastfile` > `preload` > `loaded`), while
        // `cycle` says whether the heap every view was forked from is stale —
        // which is exactly the union's `pre_fork` bit, not its class.
        let mut cycle = self.overflow;
        for path in &files {
            if let Some(entry) = self.reports.union().get(path) {
                class = class.max(entry.class);
                cycle |= entry.pre_fork || entry.class == Class::Blastfile;
            }
        }
        let reason = match self.overflow {
            true => ReloadReason::Overflow,
            false => class.reason(),
        };
        self.overflow = false;
        self.phase = Phase::Idle;
        self.last_fire = Some(now);
        Fire {
            files,
            reason,
            cycle,
        }
    }

    /// The one-shot gap closure after a reload `execve` (§4.6 step 7).
    ///
    /// A union path whose `mtime` lies between `since` and now is observed as a
    /// change, so a save that landed while no watcher existed is not lost; a
    /// future `mtime` cannot re-arm it. One `stat` per path, once.
    pub fn reconcile(&mut self, since: SystemTime, now: Instant) {
        if self.dead {
            return;
        }
        let upper = SystemTime::now();
        let paths: Vec<PathBuf> = self.reports.union().keys().cloned().collect();
        for path in paths {
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let Ok(mtime) = meta.modified() else {
                continue;
            };
            if mtime >= since && mtime <= upper {
                self.note_change(&path, now);
            }
        }
    }
}

/// The policy, its watcher and its poll duplicate: what the supervisor owns.
///
/// One watcher per boot image, one poll duplicate, one policy. The duplicate is
/// made by the supervisor with `F_DUPFD_CLOEXEC` before the box goes in (§4.1),
/// so the group and its clone die with the image and two groups are never open
/// at once.
pub struct Reload {
    watcher: Box<dyn ReloadWatcher>,
    poll: OwnedFd,
    policy: Policy,
    clock: Box<dyn Clock>,
    /// An `ENOSPC` (or `MAX_MARKS`) latch the boot has not reported yet: the
    /// ladder's one `dev.reload.unavailable`, taken by the boot's caller.
    latch: Option<Unavailable>,
}

impl Reload {
    /// Marks the boot union and returns the watcher, or the ladder's one value.
    ///
    /// `ENOSPC` (and the `MAX_MARKS` cap that is the same row) still boots: the
    /// marks already placed keep watching. The one warning it owes is not
    /// dropped, though — it is kept for [`Reload::take_latch`], so the caller
    /// emits it exactly once instead of printing `reload: on` over a policy
    /// that can no longer mark anything.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] naming the errno and the filesystem: the supervisor
    /// turns it into exactly one `dev.reload.unavailable` and serves without
    /// reload. There is no retry (§4.7).
    pub fn boot(
        cfg: ReloadConfig,
        watcher: Box<dyn ReloadWatcher>,
        poll: OwnedFd,
        clock: Box<dyn Clock>,
    ) -> Result<Self, Unavailable> {
        let mut reload = Self {
            watcher,
            poll,
            policy: Policy::new(cfg),
            clock,
            latch: None,
        };
        match reload.sync_marks() {
            MarkOutcome::Marked => Ok(reload),
            MarkOutcome::Exhausted(why) => {
                reload.latch = Some(why);
                Ok(reload)
            }
            MarkOutcome::Dead(why) => Err(why),
        }
    }

    /// The marking warning this boot owes, once.
    ///
    /// `Some` for the `ENOSPC`/`MAX_MARKS` row of the ladder (§4.7, `T32`): the
    /// caller emits one `dev.reload.unavailable` and skips the boot line. Every
    /// later call returns `None` — a running boot reports a new latch through
    /// [`Effect::Unavailable`] instead.
    pub fn take_latch(&mut self) -> Option<Unavailable> {
        self.latch.take()
    }

    /// Whether the watcher latched dead (§4.7).
    #[must_use]
    pub fn is_dead(&self) -> bool {
        self.policy.is_dead()
    }

    /// Whether marking latched off: `ENOSPC` or the `MAX_MARKS` cap.
    #[must_use]
    pub fn marking_stopped(&self) -> bool {
        self.policy.marking_stopped()
    }

    /// The policy, for the supervisor's report registration.
    pub fn policy(&mut self) -> &mut Policy {
        &mut self.policy
    }

    /// The clock's current instant.
    pub fn now(&mut self) -> Instant {
        self.clock.now()
    }

    /// The descriptors this reload owns: the poll duplicate first, then every
    /// live report end (§4.5).
    ///
    /// Appends to the caller's vector and returns how many entries it added, so
    /// a ready flag maps back to the descriptor the supervisor polled.
    pub fn poll_fds(&mut self, out: &mut Vec<RawFd>) -> usize {
        let before = out.len();
        out.push(self.poll.as_raw_fd());
        for fd in self.policy.reports.sources() {
            out.push(fd);
        }
        out.len() - before
    }

    /// Handles one ready descriptor (§4.5).
    ///
    /// The descriptor decides whether this is the watcher's poll duplicate or
    /// one view's report end. `flags` is what the supervisor's `poll` reported
    /// for it, because a report end can die in ways its `read` cannot show: an
    /// `AF_UNIX` `SOCK_DGRAM` end whose writer exited reports **no** hangup, no
    /// readability and no zero-length read on Linux (only stream and seqpacket
    /// peers are shut down when their peer is released), so the reap path's
    /// [`Policy::reports`] revoke is what removes a dead view's end in practice.
    /// A hangup or an error, when a transport does report one, is handled here
    /// in the same wakeup — otherwise the descriptor stays level-triggered and
    /// the loop spins on it forever.
    ///
    /// A zero-length read is an **empty batch**, not the peer's end: nothing is
    /// loaded, so nothing is revoked. `EAGAIN` is the idle race that costs
    /// nothing, not a death.
    pub fn on_fd(&mut self, fd: RawFd, flags: PollFlags, now: Instant) -> Vec<Effect> {
        if fd == self.poll.as_raw_fd() {
            return self.on_watcher(now);
        }
        let hung_up = flags.intersects(PollFlags::HUP | PollFlags::ERR);
        let source = self.policy.reports.source_of(fd);
        let mut buffer = [0u8; blastit_view::loaded::MAX_BATCH_BYTES];
        let mut gone = hung_up;
        let mut effects = match self.policy.reports.recv(fd, &mut buffer) {
            Ok(0) => Vec::new(),
            Ok(read) => self.policy.on_datagram(fd, &buffer[..read], now),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Vec::new(),
            Err(_) => {
                // A read that fails outright is the descriptor going away.
                gone = true;
                Vec::new()
            }
        };
        if gone {
            self.revoke(source);
        }
        // A batch may have added paths, and a new union path needs its parent
        // marked before any event for it could matter (§4.4).
        self.fold_marks(&mut effects);
        effects
    }

    /// Closes one source's end after its peer went away (§4.4).
    fn revoke(&mut self, source: Option<Source>) {
        if let Some(source) = source {
            self.policy.reports().revoke(source);
        }
    }

    /// Drains the kernel's queue once (§4.5).
    fn on_watcher(&mut self, now: Instant) -> Vec<Effect> {
        if self.policy.is_dead() {
            return Vec::new();
        }
        let mut events = Vec::new();
        let readout = match self.watcher.read(&mut events) {
            Ok(readout) => readout,
            Err(err) => {
                let why = Unavailable::of(&err, &self.policy.config().filesystem);
                self.policy.kill(why.clone());
                return vec![Effect::Stopped(why)];
            }
        };
        let mut effects = self.policy.observe(&events, readout, now);
        self.fold_marks(&mut effects);
        effects
    }

    /// The deadline tick: fires the window when it is due (§4.5).
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = self.policy.tick(now);
        self.fold_marks(&mut effects);
        effects
    }

    /// Marks every pending directory (§4.5).
    ///
    /// `ENOSPC` is "stop marking, keep the marks": one `Exhausted` and no
    /// retry, because the latch means nothing is ever asked for again. The
    /// `MAX_MARKS` cap is the same row of the ladder (`T32`) and reports the
    /// same way. Any other errno is the watcher's death, reported once by the
    /// caller.
    pub fn sync_marks(&mut self) -> MarkOutcome {
        // A dead watcher has no marks to make and owes no further warning: its
        // one report was the `Effect::Stopped` that killed it (§4.7).
        if self.policy.is_dead() {
            return MarkOutcome::Marked;
        }
        for dir in self.policy.pending_dirs() {
            match self.watcher.mark_dir(&dir) {
                Ok(()) => {}
                Err(err) if errno_of(&err) == libc::ENOSPC => {
                    self.policy.stop_marking();
                    return MarkOutcome::Exhausted(Unavailable::of(
                        &err,
                        &self.policy.config().filesystem,
                    ));
                }
                Err(err) => {
                    let why = Unavailable::of(&err, &self.policy.config().filesystem);
                    self.policy.kill(why.clone());
                    return MarkOutcome::Dead(why);
                }
            }
        }
        // The cap latched while `pending_dirs` was building the list: the marks
        // it did return are placed, and the row still owes its one warning.
        if self.policy.take_cap_hit() {
            return MarkOutcome::Exhausted(self.exhausted());
        }
        MarkOutcome::Marked
    }

    /// The `ENOSPC` row's value: the cap has no errno of its own, and the ladder
    /// spells "no marks left" exactly this way (§4.7, `T32`).
    fn exhausted(&self) -> Unavailable {
        Unavailable {
            errno: libc::ENOSPC,
            filesystem: self.policy.config().filesystem.clone(),
        }
    }

    /// The instant the supervisor next has to wake up, while the window is
    /// armed (§4.5).
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.policy.next_deadline()
    }

    /// How many directories the boot marked (§4.7's boot line).
    #[must_use]
    pub fn marked_dirs(&self) -> usize {
        self.policy.marked_dirs()
    }

    /// Turns a failed marking pass into its effect, after a batch (§4.5).
    ///
    /// `Dead` stops the watcher (`Effect::Stopped`); `Exhausted` is the
    /// `ENOSPC` row, which keeps serving and keeps the marks, so it is the
    /// ladder's one `dev.reload.unavailable` (`Effect::Unavailable`). A wakeup
    /// can therefore carry a `Reload` and an `Unavailable` together — the
    /// design's "bounded to two".
    fn fold_marks(&mut self, effects: &mut Vec<Effect>) {
        match self.sync_marks() {
            MarkOutcome::Marked => {}
            MarkOutcome::Exhausted(why) => effects.push(Effect::Unavailable(why)),
            MarkOutcome::Dead(why) => effects.push(Effect::Stopped(why)),
        }
    }
}

/// Creates one report socketpair: both ends `SOCK_DGRAM`, both non-blocking
/// (§4.4, §4.6).
///
/// Neither side may ever block: a worker reporting must not wait for the
/// supervisor, and a supervisor draining a level-triggered poll set must not
/// wait for a worker. `CLOEXEC` is the std pair's default; the read end is what
/// is made non-blocking here, and the write end's own [`blastit_view::Reporter`]
/// enforces the same on the view side.
///
/// # Errors
///
/// The `socketpair` errno, or the `fcntl` that clears blocking.
pub fn report_socket() -> std::io::Result<(
    std::os::unix::net::UnixDatagram,
    std::os::unix::net::UnixDatagram,
)> {
    let (read, write) = std::os::unix::net::UnixDatagram::pair()?;
    read.set_nonblocking(true)?;
    write.set_nonblocking(true)?;
    Ok((read, write))
}
