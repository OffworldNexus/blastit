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
//!   anywhere in this module decides a verdict, which is what lets the tests
//!   assert the exact edges with [`TestClock`].
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
//! Two, both injected: [`ReloadWatcher`] (one real implementation over
//! `blastit_sys::fanotify::Watcher`, one fake) and [`Clock`] (the real one, and
//! [`TestClock`]). Both fakes are `#[cfg(any(test, debug_assertions))]`, so a
//! debug build — which is what `crates/blastit/tests/critical_dev_reload.rs` is
//! — can drive the policy with no kernel and no sleeps, and a release build
//! carries neither.

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
// Only the debug-build test seam sends a batch; a release image has no reporter
// and must not carry the import or the type.
#[cfg(any(test, debug_assertions))]
use blastit_view::loaded::Batch;

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

/// A clock a test advances by hand: the debounce edges are asserted, not slept.
///
/// Present in debug builds so `crates/blastit/tests/critical_dev_reload.rs` can
/// drive the same policy a release build runs; a release build carries neither
/// seam (§4.3).
#[cfg(any(test, debug_assertions))]
#[derive(Debug, Clone)]
pub struct TestClock {
    base: Instant,
    at: Duration,
}

#[cfg(any(test, debug_assertions))]
impl TestClock {
    /// A clock starting at `base`.
    #[must_use]
    pub fn new(base: Instant) -> Self {
        Self {
            base,
            at: Duration::ZERO,
        }
    }

    /// Moves the clock forward by `by`.
    pub fn advance(&mut self, by: Duration) {
        self.at = self.at.saturating_add(by);
    }

    /// The time elapsed since the base.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.at
    }
}

#[cfg(any(test, debug_assertions))]
impl Clock for TestClock {
    fn now(&mut self) -> Instant {
        self.base.checked_add(self.at).unwrap_or(self.base)
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

/// One report socket a test can drive: the write end, with the read end
/// registered on `table` (§4.4).
///
/// Present in debug builds so `crates/blastit/tests/critical_dev_reload.rs` can
/// feed the policy the real codec with no kernel; a release build carries no
/// test seam.
///
/// # Errors
///
/// The `socketpair` errno, which a normal uid never sees.
#[cfg(any(test, debug_assertions))]
pub fn report_pair(
    table: &mut ReportTable,
    source: Source,
    class: Class,
    pre_fork: bool,
) -> io::Result<OwnedFd> {
    let (read, write) = report_socket()?;
    table.register(OwnedFd::from(read), source, class, pre_fork);
    Ok(OwnedFd::from(write))
}

/// Sends one batch over a [`report_pair`]'s write end, as a worker would.
#[cfg(any(test, debug_assertions))]
pub fn send_batch(write: &OwnedFd, batch: &Batch) {
    let bytes = batch.encode();
    let _ = rustix::net::send(write, &bytes, rustix::net::SendFlags::empty());
}

/// One scripted watcher: the seam's test double (§4.2).
///
/// Present in debug builds so the integration suite drives the same policy the
/// release build runs; a release build carries neither it nor [`TestClock`].
#[cfg(any(test, debug_assertions))]
#[derive(Debug, Default)]
pub struct FakeWatcher {
    /// Every directory `mark_dir` was asked for, so a test that boxed the
    /// watcher into a [`Reload`] can still read it back.
    marked: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>,
    /// What every `mark_dir` does: `None` succeeds, `Some(errno)` fails.
    pub mark_failure: Option<i32>,
    /// Fail from the `n`-th `mark_dir` call on (1-based), with this errno, so a
    /// test can make a marking pass *after* the boot die: the boot marks the
    /// app root, a later pass marks a new parent and dies there.
    pub fail_from: Option<(usize, i32)>,
    /// How many `mark_dir` calls this watcher has seen.
    mark_calls: usize,
    script: std::collections::VecDeque<Script>,
}

#[cfg(any(test, debug_assertions))]
#[derive(Debug)]
enum Script {
    Events(Vec<Event>, bool),
    Failure(i32),
}

#[cfg(any(test, debug_assertions))]
impl FakeWatcher {
    /// A watcher with nothing scripted.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The list of directories marked so far, shared with every clone.
    #[must_use]
    pub fn marks(&self) -> std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>> {
        std::sync::Arc::clone(&self.marked)
    }

    /// Queues one readout: these events, and whether it overflowed.
    pub fn push(&mut self, events: Vec<Event>, overflow: bool) {
        self.script.push_back(Script::Events(events, overflow));
    }

    /// Queues one failed read with this errno.
    pub fn push_failure(&mut self, errno: i32) {
        self.script.push_back(Script::Failure(errno));
    }
}

#[cfg(any(test, debug_assertions))]
impl ReloadWatcher for FakeWatcher {
    fn mark_dir(&mut self, dir: &Path) -> io::Result<()> {
        self.mark_calls += 1;
        let late = self
            .fail_from
            .filter(|(from, _)| self.mark_calls >= *from)
            .map(|(_, errno)| errno);
        match self.mark_failure.or(late) {
            Some(errno) => Err(io::Error::from_raw_os_error(errno)),
            None => {
                if let Ok(mut marked) = self.marked.lock() {
                    marked.push(dir.to_path_buf());
                }
                Ok(())
            }
        }
    }

    fn read(&mut self, out: &mut Vec<Event>) -> io::Result<Readout> {
        match self.script.pop_front() {
            Some(Script::Events(events, overflow)) => {
                let count = events.len();
                out.extend(events);
                Ok(Readout {
                    events: count,
                    overflow,
                    skipped: 0,
                })
            }
            Some(Script::Failure(errno)) => Err(io::Error::from_raw_os_error(errno)),
            // An empty script is an idle `EAGAIN`: no events, no overflow.
            None => Ok(Readout::default()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A temporary tree with the files the tests report.
    struct Tree {
        dir: PathBuf,
        manifest: PathBuf,
    }

    impl Tree {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("blastit-dev-reload-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let manifest = dir.join("Blastfile");
            std::fs::write(&manifest, "").unwrap();
            std::fs::create_dir_all(dir.join("app")).unwrap();
            Self { dir, manifest }
        }

        /// Creates `relative` (and its parents) and returns its absolute path.
        fn file(&self, relative: &str) -> PathBuf {
            let path = self.dir.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, "x").unwrap();
            path
        }

        fn config(&self) -> ReloadConfig {
            ReloadConfig::new(self.dir.clone(), self.manifest.clone(), "tmpfs".to_string())
        }

        fn policy(&self) -> Policy {
            Policy::new(self.config())
        }
    }

    impl std::ops::Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn event(path: &Path, mask: u64) -> Event {
        Event {
            mask,
            dir: path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("/")),
            name: path
                .file_name()
                .map(|name| name.as_bytes().to_vec())
                .unwrap_or_default(),
        }
    }

    fn closed(path: &Path) -> Event {
        event(path, libc::FAN_CLOSE_WRITE)
    }

    fn readout() -> Readout {
        Readout {
            events: 1,
            overflow: false,
            skipped: 0,
        }
    }

    fn clock() -> TestClock {
        TestClock::new(Instant::now())
    }

    /// A policy that tracks `paths` as one view loaded them.
    fn tracking(tree: &Tree, paths: &[PathBuf]) -> Policy {
        let mut policy = tree.policy();
        let write = report_pair(policy.reports(), 0, Class::Loaded, false).unwrap();
        let absorbed = policy
            .reports()
            .absorb(0, Class::Loaded, false, paths, true, MAX_PATHS);
        assert!(!absorbed.refused(), "{absorbed:?}");
        drop(write);
        policy
    }

    /// Delivers one datagram from `source`'s write end to the policy.
    fn deliver(policy: &mut Policy, write: &OwnedFd, batch: &Batch, now: Instant) -> Vec<Effect> {
        send_batch(write, batch);
        let fd = policy
            .reports()
            .sources()
            .next()
            .expect("one registered report end");
        let mut buffer = [0u8; blastit_view::loaded::MAX_BATCH_BYTES];
        let read = policy
            .reports()
            .recv(fd, &mut buffer)
            .expect("the datagram is queued");
        policy.on_datagram(fd, &buffer[..read], now)
    }

    /// The action, not the reason word, is what `pre_fork` decides (§4.5).
    ///
    /// A view's file needs the all-views transaction; a file the boot imported
    /// before the fork needs the cycle, because every view's heap copy of it is
    /// already stale. The `reason` stays `loaded` in both cases — it says what
    /// changed, never what init did about it.
    #[test]
    fn a_pre_fork_change_needs_the_cycle_and_a_view_change_does_not() {
        let tree = Tree::new("cycle");
        let view = tree.file("app/views.py");
        let shared = tree.file("app/models.py");
        let mut policy = tree.policy();
        let view_end = report_pair(policy.reports(), 0, Class::Loaded, false).unwrap();
        let preload_end =
            report_pair(policy.reports(), PRELOAD_SOURCE, Class::Loaded, true).unwrap();
        assert!(
            !policy
                .reports()
                .absorb(
                    0,
                    Class::Loaded,
                    false,
                    std::slice::from_ref(&view),
                    true,
                    MAX_PATHS,
                )
                .refused()
        );
        assert!(
            !policy
                .reports()
                .absorb(
                    PRELOAD_SOURCE,
                    Class::Loaded,
                    true,
                    std::slice::from_ref(&shared),
                    true,
                    MAX_PATHS,
                )
                .refused()
        );
        let mut clock = clock();
        policy.observe(&[closed(&view)], readout(), clock.now());
        clock.advance(QUIET);
        let effects = policy.tick(clock.now());
        let [Effect::Reload(fire)] = &effects[..] else {
            panic!("one reload, got {effects:?}");
        };
        assert_eq!(fire.files, vec![view]);
        assert_eq!(fire.reason, ReloadReason::Loaded);
        assert!(!fire.needs_cycle(), "a view's own file needs no re-exec");

        policy.observe(&[closed(&shared)], readout(), clock.now());
        clock.advance(FLOOR);
        let effects = policy.tick(clock.now());
        let [Effect::Reload(fire)] = &effects[..] else {
            panic!("one reload, got {effects:?}");
        };
        assert_eq!(fire.files, vec![shared]);
        assert_eq!(fire.reason, ReloadReason::Loaded);
        assert!(
            fire.needs_cycle(),
            "a pre-fork file is stale in every view's heap: the cycle is required"
        );
        drop(view_end);
        drop(preload_end);
    }

    #[test]
    fn modify_is_never_a_trigger() {
        let tree = Tree::new("modify");
        let path = tree.file("app/views.py");
        let mut policy = tracking(&tree, std::slice::from_ref(&path));
        let mut clock = clock();
        let effects = policy.observe(&[event(&path, libc::FAN_MODIFY)], readout(), clock.now());
        assert!(
            effects.is_empty(),
            "a write without a close is not a change"
        );
        clock.advance(Duration::from_secs(1));
        assert!(policy.tick(clock.now()).is_empty());
        assert_eq!(policy.next_deadline(), None);
    }

    #[test]
    fn atomic_save_is_one_reload() {
        let tree = Tree::new("atomic");
        let path = tree.file("app/views.py");
        let mut policy = tracking(&tree, std::slice::from_ref(&path));
        let mut clock = clock();
        let start = clock.now();
        // The editor's temp write, then the rename onto the tracked name.
        policy.observe(&[closed(&potential_temp(&tree))], readout(), start);
        clock.advance(Duration::from_millis(5));
        let tracked = clock.now();
        policy.observe(&[event(&path, libc::FAN_MOVED_TO)], readout(), tracked);
        // The window starts at the *tracked* change: the untracked temp write
        // arms nothing.
        clock.advance(Duration::from_millis(49));
        assert!(
            policy.tick(clock.now()).is_empty(),
            "49 ms of quiet is inside the window"
        );
        clock.advance(Duration::from_millis(1));
        assert_eq!(
            policy.tick(clock.now()),
            vec![Effect::Reload(Fire {
                files: vec![path.clone()],
                reason: ReloadReason::Loaded,
                cycle: false,
            })],
            "the temp write and the rename coalesce into one reload"
        );
    }

    /// The temp file an editor writes before the rename: untracked, so it is
    /// not a change at all.
    fn potential_temp(tree: &Tree) -> PathBuf {
        tree.file("app/.views.py.swp")
    }

    #[test]
    fn debounce_edges_and_the_floor() {
        let tree = Tree::new("debounce");
        let path = tree.file("app/views.py");
        let mut policy = tracking(&tree, std::slice::from_ref(&path));
        let mut clock = clock();
        // 49 ms quiet: nothing.
        policy.observe(&[closed(&path)], readout(), clock.now());
        clock.advance(Duration::from_millis(49));
        assert!(policy.tick(clock.now()).is_empty());
        // 50 ms: one reload, and only one per window.
        clock.advance(Duration::from_millis(1));
        assert_eq!(policy.tick(clock.now()).len(), 1);
        assert!(policy.tick(clock.now()).is_empty());

        // A stream that keeps arriving fires at the 250 ms hard maximum.
        clock.advance(FLOOR);
        let stream_start = clock.now();
        for step in 0..=24u64 {
            let now = stream_start
                .checked_add(Duration::from_millis(step * 10))
                .expect("no overflow");
            policy.observe(&[closed(&path)], readout(), now);
        }
        assert_eq!(
            policy.next_deadline(),
            stream_start.checked_add(MAX_WAIT),
            "the hard maximum wins over the quiet window"
        );
        let due = policy.next_deadline().unwrap();
        assert_eq!(policy.tick(due).len(), 1);
    }

    #[test]
    fn two_windows_four_hundred_ms_apart_respect_the_floor() {
        let tree = Tree::new("floor");
        let path = tree.file("app/views.py");
        let mut policy = tracking(&tree, std::slice::from_ref(&path));
        let mut clock = clock();
        policy.observe(&[closed(&path)], readout(), clock.now());
        clock.advance(QUIET);
        assert_eq!(policy.tick(clock.now()).len(), 1);
        let first_fire = clock.now();
        // A second save 400 ms after the first change would be due 450 ms after
        // it — before the floor. It waits for the floor.
        clock.advance(Duration::from_millis(350));
        let second = clock.now();
        policy.observe(&[closed(&path)], readout(), second);
        assert_eq!(
            policy.next_deadline(),
            first_fire.checked_add(FLOOR),
            "the floor wins over the 50 ms quiet window"
        );
        assert!(policy.tick(second).is_empty());
        clock.advance(Duration::from_millis(150));
        assert_eq!(policy.tick(clock.now()).len(), 1);
    }

    #[test]
    fn a_burst_of_twelve_files_is_one_reload() {
        let tree = Tree::new("burst");
        let files: Vec<PathBuf> = (0..12)
            .map(|index| tree.file(&format!("app/module_{index}.py")))
            .collect();
        let mut policy = tracking(&tree, &files);
        let mut clock = clock();
        let start = clock.now();
        for (index, path) in files.iter().enumerate() {
            let at = start
                .checked_add(Duration::from_millis(index as u64))
                .expect("no overflow");
            policy.observe(&[closed(path)], readout(), at);
        }
        clock.advance(Duration::from_millis(61));
        let effects = policy.tick(clock.now());
        assert_eq!(effects.len(), 1, "a Save-All is one reload, never twelve");
        let mut sorted = files.clone();
        sorted.sort();
        match effects.first() {
            Some(Effect::Reload(fire)) => assert_eq!(fire.files, sorted),
            other => panic!("expected one reload, got {other:?}"),
        }
    }

    #[test]
    fn unavailable_errno_mapping_and_the_sentence() {
        for errno in [
            libc::ENOSYS,
            libc::EPERM,
            libc::EINVAL,
            libc::EOPNOTSUPP,
            libc::ENOSPC,
            libc::EMFILE,
        ] {
            let typed = io::Error::other(WatcherError::Unavailable {
                op: blastit_sys::fanotify::Op::Init,
                errno,
            });
            let why = Unavailable::of(&typed, "overlayfs");
            assert_eq!(why.errno, errno);
            assert_eq!(why.filesystem, "overlayfs");
            assert_eq!(why.errno_name(), errno_symbol(errno));
        }
        // A filesystem without handles is `EOPNOTSUPP` even though the typed
        // error carries no errno of its own.
        let no_handle = io::Error::other(WatcherError::NoFileHandle {
            fs_type: "9p".to_string(),
        });
        assert_eq!(Unavailable::of(&no_handle, "9p").errno, libc::EOPNOTSUPP);
        // Anything outside the ladder still names its number.
        assert_eq!(errno_symbol(9999), "E9999");
        assert_eq!(
            Unavailable::of(&io::Error::from_raw_os_error(9999), "tmpfs").errno_name(),
            "E9999"
        );
    }

    #[test]
    fn a_mid_run_watcher_error_stops_once_and_is_never_retried() {
        let tree = Tree::new("latch");
        let mut watcher = FakeWatcher::new();
        watcher.push_failure(libc::EIO);
        let (poll, keep) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut reload = Reload::boot(
            tree.config(),
            Box::new(watcher),
            OwnedFd::from(poll),
            Box::new(clock()),
        )
        .expect("marking the boot union succeeds");
        let mut fds = Vec::new();
        assert_eq!(
            reload.poll_fds(&mut fds),
            1,
            "no view yet, so one descriptor"
        );
        let fd = fds.first().copied().unwrap();
        let now = reload.now();
        let effects = reload.on_fd(fd, PollFlags::IN, now);
        assert_eq!(effects.len(), 1, "one warning, not a stream of them");
        assert!(matches!(effects.first(), Some(Effect::Stopped(_))));
        assert!(reload.policy().is_dead());
        // A later wakeup is silent: the latch is never retried.
        assert!(reload.on_fd(fd, PollFlags::IN, now).is_empty());
        assert!(reload.tick(now).is_empty());
        drop(keep);
    }

    #[test]
    fn overflow_reloads_every_view_once() {
        let tree = Tree::new("overflow");
        let mut policy = tree.policy();
        let mut clock = clock();
        let overflow = Readout {
            events: 0,
            overflow: true,
            skipped: 0,
        };
        let start = clock.now();
        assert!(policy.observe(&[], overflow, start).is_empty());
        clock.advance(QUIET);
        let effects = policy.tick(clock.now());
        assert_eq!(effects.len(), 1);
        match effects.first() {
            Some(Effect::Reload(fire)) => {
                assert_eq!(fire.reason, ReloadReason::Overflow);
                assert!(fire.files.is_empty(), "the changed set is unknown");
            }
            other => panic!("expected an overflow reload, got {other:?}"),
        }
        assert!(policy.tick(clock.now()).is_empty(), "exactly once");
    }

    #[test]
    fn skipped_missing_and_untracked_paths_do_not_fire() {
        let tree = Tree::new("skip");
        let tracked = tree.file("app/views.py");
        let git = tree.file(".git/index");
        let modules = tree.file("node_modules/left-pad/index.js");
        let log = tree.file("var/log/dev.log");
        let outside = tree.file("templates/cart.html");
        let mut policy = tracking(
            &tree,
            &[
                tracked.clone(),
                git.clone(),
                modules.clone(),
                outside.clone(),
            ],
        );
        let marks = policy.pending_dirs();
        assert!(marks.contains(&tree.dir.join("app")));
        assert!(marks.contains(&tree.dir.join("templates")));
        assert!(
            !marks.iter().any(|dir| dir.ends_with(".git")),
            "the skip list is never marked"
        );
        assert!(!marks.iter().any(|dir| dir.ends_with("node_modules")));

        let mut clock = clock();
        let start = clock.now();
        for path in [&git, &modules, &log] {
            assert!(
                policy.observe(&[closed(path)], readout(), start).is_empty(),
                "{} is not a tracked change",
                path.display()
            );
        }
        // A file the app loaded from outside the working directory does fire,
        // and this first observation is inside the window, so nothing has fired
        // yet at the same instant.
        assert!(
            policy
                .observe(&[closed(&outside)], readout(), start)
                .is_empty()
        );
        clock.advance(QUIET);
        assert_eq!(
            policy.tick(clock.now()),
            vec![Effect::Reload(Fire {
                files: vec![outside],
                reason: ReloadReason::Loaded,
                cycle: false,
            })],
            "one reload, naming the template outside the root"
        );
    }

    #[test]
    fn a_framework_cache_write_never_triggers() {
        let tree = Tree::new("cache");
        let template = tree.file("storage/framework/views/9c1f.php");
        let next = tree.file(".next/build/module.js");
        let mut policy = tracking(&tree, &[template.clone(), next.clone()]);
        let marks = policy.pending_dirs();
        assert!(
            marks
                .iter()
                .any(|dir| dir.ends_with("storage/framework/views")),
            "a cache directory is marked so its events exist"
        );
        let mut clock = clock();
        let start = clock.now();
        assert!(
            policy
                .observe(&[closed(&template)], readout(), start)
                .is_empty()
        );
        assert!(
            policy
                .observe(&[closed(&next)], readout(), start)
                .is_empty()
        );
        clock.advance(FLOOR);
        assert!(
            policy.tick(clock.now()).is_empty(),
            "a cache write is dropped before the union lookup"
        );
    }

    #[test]
    fn path_refusals_are_counted_once() {
        let tree = Tree::new("refusals");
        let mut table = ReportTable::new();
        let write = report_pair(&mut table, 0, Class::Loaded, false).unwrap();
        let long = tree.dir.join("a".repeat(MAX_PATH_BYTES));
        let non_utf8 = {
            let mut path = tree.dir.clone().into_os_string();
            path.push("/");
            path.push(OsStr::from_bytes(b"caf\xe9.py"));
            PathBuf::from(path)
        };
        let reported = [
            PathBuf::from("app/views.py"),
            tree.dir.join("missing.py"),
            long,
            non_utf8,
        ];
        let absorbed = table.absorb(0, Class::Loaded, false, &reported, true, MAX_PATHS);
        assert_eq!(absorbed.added, 0);
        assert_eq!(
            absorbed.dropped,
            vec![
                Drop::NotAbsolute,
                Drop::Missing,
                Drop::TooLong,
                Drop::NotUtf8
            ]
        );
        assert_eq!(
            table.dirs_to_mark(&tree.dir),
            vec![tree.dir.clone()],
            "no refused path is ever marked"
        );
        drop(write);
    }

    #[test]
    fn the_union_cap_refuses_the_last_path_with_one_warning() {
        let tree = Tree::new("cap");
        let mut table = ReportTable::new();
        let write = report_pair(&mut table, 0, Class::Loaded, false).unwrap();
        let files: Vec<PathBuf> = (0..5).map(|n| tree.file(&format!("app/m{n}.py"))).collect();
        let absorbed = table.absorb(0, Class::Loaded, false, &files, true, 4);
        assert_eq!(absorbed.added, 4);
        assert_eq!(absorbed.dropped, vec![Drop::Cap]);
        assert!(table.cap_reached());
        assert_eq!(table.union().len(), 4);
        drop(write);
    }

    #[test]
    fn enospc_stops_marking_and_the_marks_keep_firing() {
        let tree = Tree::new("enospc");
        let path = tree.file("app/views.py");
        let mut watcher = FakeWatcher::new();
        watcher.mark_failure = Some(libc::ENOSPC);
        let (poll, keep) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut reload = Reload::boot(
            tree.config(),
            Box::new(watcher),
            OwnedFd::from(poll),
            Box::new(clock()),
        )
        .expect("ENOSPC keeps the boot serving");
        assert!(reload.policy().marking_stopped());
        // The ladder's one warning is not swallowed: the boot hands it to the
        // caller exactly once, so the boot line can be skipped (§4.7, S10).
        let latch = reload.take_latch().expect("ENOSPC owes one warning");
        assert_eq!(latch.errno_name(), "ENOSPC");
        assert_eq!(
            reload.take_latch(),
            None,
            "the warning is emitted once, never per marking pass"
        );
        assert!(reload.policy().pending_dirs().is_empty());
        assert!(
            reload.policy().pending_dirs().is_empty(),
            "nothing new is ever asked for after ENOSPC"
        );
        // The marks already placed keep working: an event on a tracked path
        // still fires, exactly as the ladder says.
        let write =
            report_pair(reload.policy().reports(), 0, Class::Loaded, false).expect("a socketpair");
        reload.policy().reports().absorb(
            0,
            Class::Loaded,
            false,
            std::slice::from_ref(&path),
            true,
            MAX_PATHS,
        );
        let now = reload.now();
        reload.policy().observe(&[closed(&path)], readout(), now);
        let due = now.checked_add(QUIET).expect("no overflow");
        assert_eq!(reload.tick(due).len(), 1, "a placed mark keeps watching");
        drop(write);
        drop(keep);
    }

    /// Finding: the `MAX_MARKS` cap used to `break` without latching, so a boot
    /// printed `reload: on` over a policy that could never mark again. The cap
    /// is the `ENOSPC` row (`T32`): one latch, one warning, no second ask.
    #[test]
    fn the_mark_cap_latches_like_enospc_and_reports_once() {
        let tree = Tree::new("cap-marks");
        let mut config = tree.config();
        config.max_marks = 1;
        let mut policy = Policy::new(config);
        let write = report_pair(policy.reports(), 0, Class::Loaded, false).unwrap();
        // Enough parents that the cap is what stops the pass, not the union.
        let files: Vec<PathBuf> = (0..3)
            .map(|n| tree.file(&format!("app/m{n}/views.py")))
            .collect();
        let absorbed = policy
            .reports()
            .absorb(0, Class::Loaded, false, &files, true, MAX_PATHS);
        assert!(!absorbed.refused(), "{absorbed:?}");
        let mut reload = Reload {
            watcher: Box::new(FakeWatcher::new()),
            poll: OwnedFd::from(std::os::unix::net::UnixStream::pair().unwrap().0),
            policy,
            clock: Box::new(clock()),
            latch: None,
        };
        let outcome = reload.sync_marks();
        assert_eq!(
            outcome,
            MarkOutcome::Exhausted(Unavailable {
                errno: libc::ENOSPC,
                filesystem: "tmpfs".to_string(),
            }),
            "the cap is the ladder's no-marks-left row"
        );
        assert_eq!(reload.marked_dirs(), 1, "the cap is a ceiling");
        assert!(reload.marking_stopped(), "the cap latches stopping");
        assert_eq!(
            reload.take_latch(),
            None,
            "a running pass owes no boot latch"
        );
        assert_eq!(
            reload.sync_marks(),
            MarkOutcome::Marked,
            "a latched policy has nothing pending and never asks again"
        );
        drop(write);
    }

    /// Finding: `dirs_to_mark` re-`stat`ed every union path on every datagram and
    /// every tick. The `is_dir` bit is decided by the absorb's one `stat`, so a
    /// directory that has since been deleted is still marked — the marking pass
    /// never looks at the filesystem.
    #[test]
    fn dirs_to_mark_never_restats_the_union() {
        let tree = Tree::new("no-stat");
        let file = tree.file("app/views.py");
        let dir = tree.dir.join("app/sub");
        std::fs::create_dir_all(&dir).unwrap();
        let mut table = ReportTable::new();
        let write = report_pair(&mut table, 0, Class::Loaded, false).unwrap();
        let absorbed = table.absorb(
            0,
            Class::Loaded,
            false,
            &[file, dir.clone()],
            true,
            MAX_PATHS,
        );
        assert!(!absorbed.refused(), "{absorbed:?}");
        assert!(table.dirs_to_mark(&tree.dir).contains(&dir));
        // The directory is gone; a pass that re-stat'ed would drop it, and the
        // union would have lost a mark that the kernel still holds.
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            table.dirs_to_mark(&tree.dir).contains(&dir),
            "the marking pass uses the absorb's `is_dir`, never a second `stat`"
        );
        drop(write);
    }

    /// Finding: `register` overwrote `by_source` without dropping the previous
    /// end, so a route's second concurrent view leaked its read end.
    #[test]
    fn registering_a_source_again_releases_the_previous_end() {
        Tree::new("re-register");
        let mut table = ReportTable::new();
        let (read, write) = report_socket().unwrap();
        let first = read.as_raw_fd();
        table.register(OwnedFd::from(read), 0, Class::Loaded, false);
        assert_eq!(table.sources().collect::<Vec<_>>(), vec![first]);
        // A fresh view for the same route: the old end is closed, not kept.
        let (read, _write2) = report_socket().unwrap();
        let second = read.as_raw_fd();
        table.register(OwnedFd::from(read), 0, Class::Loaded, false);
        let live: Vec<RawFd> = table.sources().collect();
        assert_eq!(live, vec![second], "one end per source, always");
        assert_eq!(table.source_of(first), None, "the stale end is gone");
        assert_eq!(table.source_of(second), Some(0));
        drop(write);
    }

    /// Finding: this test used to send an **empty datagram** over a live socket
    /// and call that end-of-file, while `Reporter::send` never emits an empty
    /// batch. The peer is closed here and `on_fd` is driven with the flags
    /// `poll` reports for it.
    ///
    /// Linux truth, which is why the flags exist: an `AF_UNIX` `SOCK_DGRAM` end
    /// whose writer exited reports **no** hangup, no readability and no
    /// zero-length read (`unix_release_sock` releases only stream and seqpacket
    /// peers), so a dead view's end is really revoked by the reap path; a
    /// hangup or error, when a transport does report one, must still close it in
    /// the same wakeup or the level-triggered poll spins on it forever.
    #[test]
    fn a_closed_report_end_is_revoked_on_its_own_wakeup() {
        let tree = Tree::new("eof");
        let mut policy = tree.policy();
        let (read, write) = report_socket().unwrap();
        let fd = read.as_raw_fd();
        policy
            .reports()
            .register(read.into(), 7, Class::Loaded, false);
        let mut reload = Reload {
            watcher: Box::new(FakeWatcher::new()),
            poll: OwnedFd::from(std::os::unix::net::UnixStream::pair().unwrap().0),
            policy,
            clock: Box::new(clock()),
            latch: None,
        };
        let now = reload.now();
        // The view went away: its end of the socketpair is closed.
        drop(write);
        assert!(
            reload.on_fd(fd, PollFlags::HUP, now).is_empty(),
            "nothing to apply"
        );
        assert_eq!(
            reload.policy().reports().source_of(fd),
            None,
            "a dead end leaves the poll set in the wakeup that saw it"
        );
        assert!(
            reload.policy().reports().sources().next().is_none(),
            "nothing in the poll set spins on a dead peer"
        );
    }

    /// The other half of the same finding: a zero-length read is an **empty
    /// batch**, not a hangup. A view that loaded nothing reports nothing, and
    /// treating that as the peer's end would silently unwatch its route.
    #[test]
    fn a_zero_length_datagram_is_not_a_hangup() {
        let tree = Tree::new("empty");
        let mut policy = tree.policy();
        let (read, write) = report_socket().unwrap();
        let fd = read.as_raw_fd();
        policy
            .reports()
            .register(read.into(), 7, Class::Loaded, false);
        let mut reload = Reload {
            watcher: Box::new(FakeWatcher::new()),
            poll: OwnedFd::from(std::os::unix::net::UnixStream::pair().unwrap().0),
            policy,
            clock: Box::new(clock()),
            latch: None,
        };
        let now = reload.now();
        rustix::net::send(&write, &[], rustix::net::SendFlags::empty())
            .expect("an empty datagram reaches the supervisor");
        assert!(reload.on_fd(fd, PollFlags::IN, now).is_empty());
        assert_eq!(
            reload.policy().reports().source_of(fd),
            Some(7),
            "an empty batch is not a death: the end stays registered"
        );
    }

    /// The two predicates the live negatives cannot reach: the Python reporter
    /// filters `.git`, `node_modules` and non-source suffixes before a datagram
    /// exists, so `is_skipped`/`is_cached` are asserted here directly.
    #[test]
    fn the_skip_and_cache_predicates_are_exact() {
        let root = Path::new("/srv/app");
        assert!(is_skipped(Path::new("/srv/app/.git/index")));
        assert!(is_skipped(Path::new("/srv/app/node_modules/pkg/i.js")));
        assert!(is_skipped(Path::new("/srv/app/target/debug/x")));
        assert!(!is_skipped(Path::new("/srv/app/app/views.py")));
        assert!(!is_skipped(Path::new("/srv/app/.gitignore")));

        assert!(is_cached(
            Path::new("/srv/app/storage/framework/views/9c1f.php"),
            root
        ));
        assert!(is_cached(Path::new("/srv/app/var/cache/twig/9c"), root));
        assert!(is_cached(Path::new("/srv/app/.next/build/m.js"), root));
        assert!(!is_cached(Path::new("/srv/app/storage/logs/dev.log"), root));
        assert!(!is_cached(Path::new("/srv/app/app/views.py"), root));
        assert!(
            !is_cached(Path::new("/elsewhere/storage/framework/views/x"), root),
            "a cache path outside the root is only cached by name"
        );
    }

    #[test]
    fn a_boot_batch_does_not_rearm_but_a_resync_does() {
        let tree = Tree::new("boot");
        let mut policy = tree.policy();
        let write = report_pair(policy.reports(), 0, Class::Loaded, false).expect("a socketpair");
        let mut clock = clock();
        let files: Vec<String> = (0..3)
            .map(|n| tree.file(&format!("app/m{n}.py")).display().to_string())
            .collect();
        let boot = Batch::new(0, 0, true, false, files.clone()).unwrap();
        assert!(deliver(&mut policy, &write, &boot, clock.now()).is_empty());
        clock.advance(MAX_WAIT);
        assert!(
            policy.tick(clock.now()).is_empty(),
            "a boot batch replaces a set without arming the window"
        );
        // A resync arms it: everything the source ever reported may have
        // changed.
        let resync = Batch::new(0, 1, false, true, files.clone()).unwrap();
        assert!(deliver(&mut policy, &write, &resync, clock.now()).is_empty());
        clock.advance(QUIET);
        let effects = policy.tick(clock.now());
        assert_eq!(effects.len(), 1, "resync reloads, once");
        drop(write);
    }

    #[test]
    fn a_view_report_marks_its_parent_through_the_wakeup() {
        let tree = Tree::new("report-marks");
        let watcher = FakeWatcher::new();
        let marks = watcher.marks();
        let (poll, keep) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut reload = Reload::boot(
            tree.config(),
            Box::new(watcher),
            OwnedFd::from(poll),
            Box::new(clock()),
        )
        .expect("marking the boot union succeeds");
        let write =
            report_pair(reload.policy().reports(), 0, Class::Loaded, false).expect("a socketpair");
        let fd = reload
            .policy()
            .reports()
            .sources()
            .next()
            .expect("one report end");
        let path = tree.file("app/views.py");
        let batch =
            Batch::new(0, 0, true, false, vec![path.display().to_string()]).expect("a boot batch");
        send_batch(&write, &batch);
        let now = reload.now();
        assert!(
            reload.on_fd(fd, PollFlags::IN, now).is_empty(),
            "a boot batch adds paths without arming"
        );
        let marked = marks.lock().unwrap().clone();
        assert!(
            marked.contains(&tree.dir.join("app")),
            "a reported path's parent is marked in the same wakeup: {marked:?}"
        );
        drop(write);
        drop(keep);
    }

    #[test]
    fn a_seq_gap_rebuilds_the_source_set() {
        let tree = Tree::new("gap");
        let mut policy = tree.policy();
        let write = report_pair(policy.reports(), 0, Class::Loaded, false).expect("a socketpair");
        let mut clock = clock();
        let first: Vec<String> = vec![tree.file("app/a.py").display().to_string()];
        let later: Vec<String> = vec![tree.file("app/b.py").display().to_string()];
        let boot = Batch::new(0, 0, true, false, first).unwrap();
        deliver(&mut policy, &write, &boot, clock.now());
        // `seq` 5 after `seq` 0 is a gap: the set is rebuilt, so `a.py` leaves
        // this source's set and `b.py` is the whole of it.
        let gap = Batch::new(0, 5, false, false, later.clone()).unwrap();
        deliver(&mut policy, &write, &gap, clock.now());
        let set: Vec<String> = policy
            .reports()
            .set_of(0)
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        assert_eq!(set, later, "a gap rebuilds the source's contribution");
        drop(write);
    }
}
