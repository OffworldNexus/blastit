//! Forking a PHP view process (ADR-0010 §1, design §4.3).
//!
//! A PHP route's worker is an ordinary **view** worker: one unprivileged
//! process per slot, a private runtime directory holding only the proxy
//! sockets the route was granted, `max_requests` recycling. The only thing that
//! differs is the [`SeccompRole`] — [`SeccompRole::EmbeddedView`], which also
//! refuses `execve` and the process-creation calls `libphp` must never make —
//! and the runtime the child enters.
//!
//! # Why this is a sibling of [`crate::zygote`], not a branch in it
//!
//! `zygote::fork_view` is the CPython path: its child hands the interpreter to
//! `blastit_view::serve` and its parent carries a `Py<PyAny>` for the
//! application. A PHP child has no interpreter to hand over and no callable to
//! carry; it calls `dlopen` and then `PhpEmbedRuntime`. Sharing the fork would
//! mean a `match` on the backend kind inside the composition root — exactly the
//! dispatch branch ADR-0009 removes — so the fork is shared where it is shared
//! ([`crate::child::ChildIo`], [`crate::child::install_filter`], the
//! `blastit_sys` child sequence) and separate where it differs.
//!
//! # The child sequence, in order
//!
//! `ChildIo::prepare` (descriptors, limits, umask, cwd) → `drop_privileges` in
//! production → `harden_child` → the seccomp filter → `set_dumpable(false)`
//! **again**, after the privilege drop that cleared it (ADR-0010 §2) → the
//! saved signal mask → `PhpEmbedRuntime::load`, which is the last chance to
//! refuse: a load failure is `exit 1` with a message naming the file, never a
//! first-request fault. Only then does the child send `READY`.
//!
//! # What this module does not do
//!
//! It does not inspect ELF symbols or pick a library: stage 0's discovery gate
//! does that statically, and [`boot_bindings`] re-opens exactly what it
//! recorded. It also never `dlopen`s anything itself — the load happens in the
//! child, inside [`SeccompRole::EmbeddedView`], which is why no `libphp` symbol
//! is reachable in `init`, `http`, `scheduler` or `dbproxy`.
//!
//! # Two halves, decided once each
//!
//! [`boot_bindings`] is the boot half (`ADR-0010` §3): stage 1 re-opens the
//! `libphp` and the shim stage 0 inspected, asserts `(st_dev, st_ino)` against
//! the record it inherited, renders the `ZEND_INI_SYSTEM` blob once, and hands
//! the supervisor one [`PhpBinding`] per PHP route slot. From then on
//! `Ctl::Spawn` looks the slot up in that vector instead of asking what kind of
//! backend the route is — the choice is data, not a branch in dispatch
//! (`ADR-0009`, design §4.3). [`fork_php_view`] is the per-spawn half.
//!
//! **A host without a usable NTS `libphp` is not a fallback path**: stage 0's
//! discovery gate (`crate::stage0::php_boot_gate`) refuses it before a listener
//! binds, and a binding that cannot be built here is the same refusal one
//! process later.

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use blastit_frame::Limits;
use blastit_manifest::Manifest;
use blastit_php_sapi::{Egress, IniPolicy, SharedObject};
use blastit_scheduler::RouteInfo;
use blastit_sys::{
    ExitStatus, Fork, Pid, Rlimits, SeccompRole, Signal, SignalMask, harden_child, pipe,
    restore_signal_mask_in_child, set_dumpable, socketpair_stream,
};
use rustix::fs::{Mode, OFlags};

use crate::child::{ChildIo, child_exit, install_filter, wait_readable};
use crate::config::Config;

/// How many extra processes a view's uid may have beyond its workers (§7.2:
/// gVisor leaves out the main thread, so one unit of margin).
const VIEW_PROCESS_MARGIN: u32 = 16;

/// How many descriptors a PHP view may hold open (its channel, stdio, the
/// granted proxy sockets, `libphp` and its extensions).
const VIEW_NOFILE: u64 = 1024;

/// Everything the child needs to load and run `entrypoint php`.
///
/// Both shared objects are **descriptors**, never paths: the child keeps them
/// across `ChildIo::prepare` and `dlopen`s `/proc/self/fd/<n>`, so the file
/// cannot be swapped between the `(st_dev, st_ino)` verification and the load
/// (`ADR-0010` §3). There is no path variant and therefore no fallback: a
/// missing descriptor is a boot refusal, not a load of something unverified.
///
/// `entry` is pinned by its **record**, not by a descriptor, because
/// `SCRIPT_FILENAME` has to stay the path the application reads (and the path
/// `open_basedir` is written in terms of): the boot records the resolved file's
/// `(st_dev, st_ino)` **and** `st_size`/`st_ctime` here, holds an `O_PATH`
/// descriptor so that inode cannot be freed and its number handed back, and
/// every worker compares the file at that path against the record **before each
/// request** (`PinnedEntryRuntime`). A file that was replaced *or rewritten in
/// place* is a `500`, never a silent re-resolution to whatever is there now.
#[derive(Debug)]
pub struct PhpBinding {
    /// The host's NTS `libphp.so`, opened and identity-checked at boot.
    pub libphp: OwnedFd,
    /// The matching `libblastit_php8X.so` shim (`MB.3.12`).
    pub shim: OwnedFd,
    /// Absolute `SCRIPT_FILENAME` (the `entrypoint php`), pinned at boot: the
    /// record is [`PhpBinding::entry_pin`] beside it, and every request
    /// re-checks the file at this path against that record.
    pub entry: PathBuf,
    /// The record of the resolved `entry` taken at boot (`ADR-0010` §3): the
    /// inode, its size and its `st_ctime`. Every request compares against it
    /// before the script runs.
    pub entry_pin: EntryPin,
    /// `O_PATH | O_CLOEXEC` on that same inode, held from boot to the worker.
    ///
    /// Keeping the file open is what makes the record's `(st_dev, st_ino)`
    /// meaningful: without a reference, an unlinked inode may be freed and its
    /// number handed back to the next `create` (a measured 4-in-20 on xfs), and
    /// then a `rm` + recreate looks identical to the file the boot recorded.
    pub entry_fd: OwnedFd,
    /// The application root (`DOCUMENT_ROOT`, `open_basedir`).
    pub document_root: PathBuf,
    /// The `ZEND_INI_SYSTEM` blob the composition root rendered.
    pub ini: String,
    /// The view's private scratch directory: the child's `chdir`, `HOME`,
    /// `TMPDIR` and PHP's `sys_temp_dir` (`<runtime>/views/<uid>` in
    /// production).
    pub view_tmp: PathBuf,
    /// Requests per process before it retires.
    pub max_requests: u32,
    /// The largest request body the worker buffers (the route's `post_max_size`).
    pub max_body_bytes: usize,
}

impl PhpBinding {
    /// The descriptors the child must keep across `ChildIo::prepare`: the
    /// pinned `libphp` and shim, and the `O_PATH` descriptor on the entry.
    ///
    /// Without the first two `close_fds_except` closes exactly the files the
    /// child is about to `dlopen` through `/proc/self/fd/<n>`; without the third
    /// the recorded inode could be freed and its number reused between the
    /// per-request check and PHP's own open.
    fn pinned_fds(&self) -> Vec<RawFd> {
        vec![
            self.libphp.as_raw_fd(),
            self.shim.as_raw_fd(),
            self.entry_fd.as_raw_fd(),
        ]
    }
}

/// The `entrypoint php` as the boot recorded it (`MB.3.56`, `ADR-0010` §3).
///
/// `(st_dev, st_ino)` says *which* file, and the pair alone is not enough: a
/// rewrite **in place** — what `cp`, `tar -x` and a deploy script do — keeps the
/// inode and changes only the bytes. `st_size` catches a rewrite that changes
/// length; `st_ctime` catches one that does not **unless it lands inside the
/// same coarse-clock tick (~1–4 ms) as this record** — the documented residual,
/// `docs/security.md` "PHP front controllers" — because the kernel stamps it on
/// every write and on every metadata change, and unlike `st_mtime` no user can
/// set it back (only the clock can). The two together are the content half; the
/// `O_PATH` descriptor held beside them (`PhpBinding::entry_fd`) is the identity
/// half, so a freed inode number cannot be handed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryPin {
    /// `st_dev` of the recorded file.
    pub dev: u64,
    /// `st_ino` of the recorded file.
    pub ino: u64,
    /// `st_size` at record time.
    pub size: u64,
    /// `st_ctime`, whole seconds.
    pub ctime_s: i64,
    /// `st_ctime`, nanoseconds.
    pub ctime_ns: i64,
}

impl EntryPin {
    /// The record of `metadata` — the same five numbers the boot reads.
    #[must_use]
    pub fn of(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            size: metadata.size(),
            ctime_s: metadata.ctime(),
            ctime_ns: metadata.ctime_nsec(),
        }
    }
}

/// `MB.3.64`: how long the test-only rendezvous may hold a boot before the wait
/// becomes a refusal rather than a hang.
///
/// Longer than any `cargo test` needs to rename one file (milliseconds), short
/// enough that a stale variable in a developer's shell is a boot failure with a
/// sentence instead of a process nobody can explain.
#[cfg(all(debug_assertions, feature = "php"))]
const TEST_SWAP_TIMEOUT: Duration = Duration::from_secs(30);

/// `<point>.<suffix>`: the two sidecars that make the rendezvous an ordinary
/// pair of files, so both sides talk about the same path without a socket, a
/// pipe, or a second descriptor to pass.
#[cfg(all(debug_assertions, feature = "php"))]
fn swap_sidecar(point: &Path, suffix: &str) -> PathBuf {
    let mut name = point.as_os_str().to_os_string();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

/// `MB.3.64`: the deterministic window `critical_php_embed`'s inode-pin legs
/// need between stage 0's record and stage 1's re-open.
///
/// Stage 0 read `BLASTIT_TEST_SWAP_POINT` before the `execve` that strips every
/// `BLASTIT_*` variable and carried the path in the configuration document
/// ([`crate::stage0::TEST_SWAP_POINT_MEMBER`]); stage 1 reaches this once,
/// immediately before [`boot_bindings`] opens the pair. The boot announces
/// itself by creating `<point>.ready`, then blocks until `<point>.go` exists.
/// The test therefore cannot be early (it waits for `.ready`) and stage 1 cannot
/// be early (it waits for `.go`), so a rename the test makes in between is
/// inside the window because the two sides say so — not because the runner was
/// slow.
///
/// **A test seam with no production arm.** The whole function — and the
/// variable's name in [`crate::stage0::TEST_SWAP_POINT_VAR`] — exists only under
/// `debug_assertions`, which is what a `cargo test` build is; a release artifact
/// compiles it out and carries no name to act on, which
/// `scripts/check-no-test-seam.sh` measures on the packaged binary instead of
/// trusting the `cfg`. Nothing here is reachable from a route process either:
/// the value lives in the stage-0/stage-1 configuration document, and
/// `blastit-env` strips every `BLASTIT_*` variable from the environment a view
/// or http child is given.
///
/// # Errors
///
/// A sentence naming the point and the fix when `<point>.ready` cannot be
/// written or `<point>.go` does not arrive within [`TEST_SWAP_TIMEOUT`]. Every
/// call site turns that into this boot's `exit 1` before any listener binds.
#[cfg(all(debug_assertions, feature = "php"))]
pub fn await_test_swap(point: &Path) -> Result<(), String> {
    let ready = swap_sidecar(point, "ready");
    let go = swap_sidecar(point, "go");
    let refuse = |what: &str, path: &Path, err: &str| {
        format!(
            "BLASTIT_TEST_SWAP_POINT={}: {what} {}: {err}; this is a test-only rendezvous and a \
             boot that waits at it never serves — unset it to boot normally",
            point.display(),
            path.display()
        )
    };
    if let Err(err) = std::fs::write(&ready, b"stage 1 is at the swap point\n") {
        return Err(refuse("creating", &ready, &err.to_string()));
    }
    let deadline = Instant::now() + TEST_SWAP_TIMEOUT;
    loop {
        if go.exists() {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(refuse(
                "waited",
                &go,
                &format!("for {TEST_SWAP_TIMEOUT:?} without the test releasing the boot"),
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// `ADR-0010` §3: builds every PHP slot's [`PhpBinding`], or the sentence the
/// boot refuses with.
///
/// Stage 0 recorded `(path, st_dev, st_ino)` for the NTS `libphp` and the shim
/// it inspected in [`crate::config::Config::php`]; this re-opens both with
/// `O_RDONLY | O_CLOEXEC | O_NOFOLLOW` and refuses a file whose identity moved
/// (a package upgraded between the two stages would otherwise run something
/// nobody inspected). Nothing is loaded here: the descriptors travel into the
/// view child through [`PhpBinding::pinned_fds`] and the worker `dlopen`s
/// `/proc/self/fd/<n>`.
///
/// `routes` is the supervisor's slot list, so a `dev --learn` boot — whose slot
/// `0` is the unrouted-request fallback view — keeps its numbering. `dev` also
/// decides where the view's private scratch directory is: the caller's
/// per-slot directory there, `<runtime>/views/<uid>` in production.
///
/// # Errors
///
/// A message naming the route, the file and the fix. Every case is a boot
/// refusal (`exit 1`, no listener bound), never a first-request fault.
pub fn boot_bindings(
    config: &Config,
    manifest: &Manifest,
    routes: &[RouteInfo],
    dev: Option<&crate::dev_learn::Dev>,
) -> Result<Vec<Option<PhpBinding>>, String> {
    let mut bindings: Vec<Option<PhpBinding>> = routes.iter().map(|_| None).collect();
    // §12.2: one `entrypoint php` for the whole file, so **every** view runs it.
    // A file with no front controller binds nothing.
    let Some(php) = manifest.php() else {
        return Ok(bindings);
    };
    // The manifest's route `i` is slot `i + offset`.
    let offset = routes.len().saturating_sub(manifest.routes().len());
    for (index, route) in manifest.routes().iter().enumerate() {
        let slot = index + offset;
        let Some(info) = routes.get(slot) else {
            return Err(format!(
                "route {:?}: no view slot for entrypoint php {:?}; the manifest and the \
                 scheduler's routing table disagree",
                route.name(),
                php.entry
            ));
        };
        let Some(record) = config.php.as_ref() else {
            return Err(format!(
                "route {:?}: stage 0 recorded no NTS libphp for entrypoint php {:?}, so there \
                 is nothing to load; restart blastit so the discovery gate runs again",
                route.name(),
                php.entry
            ));
        };
        let document_root = config.manifest_dir.clone();
        let entry = document_root.join(&php.entry);
        if !entry.starts_with(&document_root) {
            return Err(format!(
                "route {:?}: entrypoint php {:?} leaves the application root {}; a script \
                 outside it would also be outside open_basedir",
                route.name(),
                php.entry,
                document_root.display()
            ));
        }
        let (entry_pin, entry_fd) = pin_entry(&entry, route.name(), &php.entry)?;
        // The same private directory the fork makes the child's `chdir`,
        // `HOME` and `TMPDIR`, so `sys_temp_dir` cannot disagree with them.
        let dir = match dev {
            Some(dev) => dev.view_dir(slot),
            None => crate::config::view_dir(info.uid),
        };
        let egress = if route.egress() {
            Egress::Allowed
        } else {
            Egress::Denied
        };
        let ini = IniPolicy::new(&document_root, &dir, request_timeout_s(config), egress).map_err(
            |err| {
                format!(
                    "route {:?}: the INI lockdown for {} could not be rendered: {err}",
                    route.name(),
                    dir.display()
                )
            },
        )?;
        bindings[slot] = Some(PhpBinding {
            libphp: open_pinned(
                &record.libphp,
                record.dev,
                record.ino,
                route.name(),
                "libphp",
            )?,
            shim: open_pinned(
                &record.shim,
                record.shim_dev,
                record.shim_ino,
                route.name(),
                "shim",
            )?,
            entry,
            entry_pin,
            entry_fd,
            document_root,
            ini: ini.as_str().to_owned(),
            view_tmp: dir,
            max_requests: max_requests(config),
            max_body_bytes: max_body_bytes(config),
        });
    }
    Ok(bindings)
}

/// Re-opens one inspected object and refuses a file that is not the one stage 0
/// read (`ADR-0010` §3).
fn open_pinned(
    path: &Path,
    dev: u64,
    ino: u64,
    route: &str,
    what: &str,
) -> Result<OwnedFd, String> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|err| format!("route {route:?}: {what} {}: {err}", path.display()))?;
    let stat = rustix::fs::fstat(&fd)
        .map_err(|err| format!("route {route:?}: {what} {}: {err}", path.display()))?;
    if stat.st_dev != dev || stat.st_ino != ino {
        return Err(format!(
            "route {route:?}: {what} {} is not the file stage 0 inspected (st_dev/st_ino \
             moved), so the worker would load something nobody verified; restore the package \
             and restart blastit",
            path.display()
        ));
    }
    Ok(fd)
}

/// `ADR-0010` §3: records the resolved `entrypoint php` — the same record the
/// pair carries, plus `st_size`/`st_ctime` and an `O_PATH` descriptor — or the
/// sentence the boot refuses with.
///
/// The path is followed (`stat(2)`, not `lstat`): `SCRIPT_FILENAME` is a path
/// `libphp` opens, so a deploy that reaches its front controller through a
/// symlinked `current/` has a perfectly ordinary entry. What is recorded is the
/// **inode at the end of that path**, which is what every request re-checks, and
/// what the descriptor keeps allocated.
///
/// The descriptor is `O_PATH`, not the script's own `SCRIPT_FILENAME`:
/// `SCRIPT_FILENAME` must stay the application's own path (a script reads it, and
/// `open_basedir` is written in terms of it), so a descriptor cannot stand in for
/// it the way `/proc/self/fd/<n>` does for `libphp` and the shim. It is held for
/// one reason — an `O_PATH` reference on the inode stops the kernel handing that
/// inode number to a later `create` after an `unlink`, which is the one way a
/// `rm` + recreate could present the recorded `(st_dev, st_ino)` again.
fn pin_entry(entry: &Path, route: &str, declared: &str) -> Result<(EntryPin, OwnedFd), String> {
    let not_a_file = |detail: String| {
        format!(
            "route {route:?}: entrypoint php {declared:?} is not a file ({}: {detail}); \
             SCRIPT_FILENAME is pinned to it and never taken from the request",
            entry.display()
        )
    };
    let fd = rustix::fs::open(entry, OFlags::PATH | OFlags::CLOEXEC, Mode::empty())
        .map_err(|err| not_a_file(err.to_string()))?;
    // One resolution: the record is `fstat` of the descriptor that is kept, so
    // the numbers and the reference cannot disagree.
    let stat = rustix::fs::fstat(&fd).map_err(|err| not_a_file(err.to_string()))?;
    if !rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file() {
        return Err(not_a_file("it is not a regular file".to_string()));
    }
    let pin = EntryPin {
        dev: stat.st_dev,
        ino: stat.st_ino,
        size: u64::try_from(stat.st_size)
            .map_err(|_| not_a_file("its st_size is negative".to_string()))?,
        ctime_s: stat.st_ctime,
        ctime_ns: i64::try_from(stat.st_ctime_nsec)
            .map_err(|_| not_a_file("its st_ctime is out of range".to_string()))?,
    };
    Ok((pin, fd))
}

/// `ADR-0010` §3, the per-request half: is the file at `entry` still the one the
/// boot recorded?
///
/// Compares the whole record — `(st_dev, st_ino)`, `st_size` and `st_ctime` — of
/// the path as it resolves **now** against what [`boot_bindings`] recorded, so
/// both shapes of swap are refused instead of executed: a decoy inode *renamed*
/// over the entry (`MB.3.7`'s inside-`manifest_dir` case) and a **rewrite in
/// place**, which keeps the inode and changes only the bytes (what `cp`, `tar -x`
/// and a deploy script do). The comparison follows symlinks because PHP does.
///
/// # Errors
///
/// The refusal sentence: the route, the entry, why it no longer matches, and the
/// fix. It names no secret — every part of it is in the manifest.
#[cfg(feature = "php")]
fn verify_entry(route: &str, entry: &Path, pin: EntryPin) -> Result<(), String> {
    let changed = |why: String| {
        format!(
            "route {route:?}: entrypoint php {} changed since this boot recorded it ({why}), \
             so this worker will not run it; restart blastit to run the file as it is now, and \
             restore it first if you did not change it",
            entry.display()
        )
    };
    let metadata =
        std::fs::metadata(entry).map_err(|err| changed(format!("cannot be read now: {err}")))?;
    if !metadata.is_file() {
        return Err(changed("it is not a regular file now".to_string()));
    }
    let now = EntryPin::of(&metadata);
    if now != pin {
        let why = if now.dev != pin.dev || now.ino != pin.ino {
            "its st_dev/st_ino moved: it is another file"
        } else {
            "its st_size/st_ctime moved: it was rewritten in place"
        };
        return Err(changed(why.to_string()));
    }
    Ok(())
}

/// The worker's runtime, wrapped so the pinned `entry` is re-checked before every
/// request (`MB.3.56`, `ADR-0010` §3).
///
/// The boot records the entry beside the pair's and holds its inode; this is that
/// record applied at the start of every request — before the request body is
/// read, before `Prepared::new` and before `libphp` re-opens `SCRIPT_FILENAME`. A
/// mismatch answers `500` (with the sentence on the worker's stderr, never in the
/// body) instead of running whatever is at the path now. The wrapper is a
/// passthrough for everything else, so `kind()`, concurrency and recycling stay
/// the PHP runtime's own.
///
/// The check runs here, not inside `PhpEmbedRuntime::serve`'s last line: the pin
/// is the composition root's record, and the view crate's `PhpRuntimeConfig` is
/// the wrong place for a value only a boot can produce (a runtime that could be
/// built without one would be a fail-open seam). The window between this check
/// and PHP's open is the client-paced body read plus `php_request_startup`,
/// bounded by `max_body_bytes` and `request_timeout`; closing it entirely means
/// moving the comparison beside `handle.request`, which is a change to the view
/// crate's seam, not to the pin.
#[cfg(feature = "php")]
struct PinnedEntryRuntime {
    /// The runtime this wraps: [`blastit_view::php::PhpEmbedRuntime`].
    inner: blastit_view::php::PhpEmbedRuntime,
    /// The route whose entry is pinned, for the refusal.
    route: String,
    /// The absolute `SCRIPT_FILENAME` the boot resolved.
    entry: PathBuf,
    /// The record the boot took of it.
    pin: EntryPin,
}

#[cfg(feature = "php")]
impl blastit_view::WorkerRuntime for PinnedEntryRuntime {
    fn kind(&self) -> &'static str {
        self.inner.kind()
    }

    fn max_inflight(&self) -> u32 {
        self.inner.max_inflight()
    }

    fn serve(
        &self,
        req: blastit_frame::ReqHead,
        io: &mut dyn blastit_view::RequestIo,
    ) -> blastit_view::RequestEnd {
        if let Err(sentence) = verify_entry(&self.route, &self.entry, self.pin) {
            // The response is an empty `500`: the reason belongs in the log, not
            // in an answer a client can steer (the path is the operator's, but
            // the rule is the same one every refusal follows).
            io.output(blastit_view::OutputStream::Stderr, &sentence);
            let head = blastit_frame::RespHead {
                status: 500,
                reason: Vec::new(),
                headers: Vec::new(),
            };
            let outcome = match io.write_head(head).and_then(|()| io.flush_response()) {
                Ok(()) => blastit_frame::DoneOutcome::Completed,
                Err(_) => blastit_frame::DoneOutcome::Aborted,
            };
            return blastit_view::RequestEnd::new(outcome, None);
        }
        self.inner.serve(req, io)
    }

    fn recycle(&self) -> bool {
        self.inner.recycle()
    }
}

/// Everything a boot's `entrypoint php` probe needs (`ADR-0010` §3).
///
/// `bindings` is what [`boot_bindings`] built — the **pinned** descriptors stage
/// 0 inspected — so the probe loads exactly the files the workers will, one step
/// earlier. A slot without a binding is another backend and has nothing to
/// prove.
pub struct PhpProbeBoot<'a> {
    /// Process-wide configuration (`BLASTIT_SECCOMP`, the view worker cap).
    pub cfg: &'a Config,
    /// The supervisor's slot list: the probe names the route it refuses.
    pub routes: &'a [RouteInfo],
    /// One binding per PHP slot, `None` for every other backend.
    pub bindings: &'a [Option<PhpBinding>],
    /// Whether each slot's filter keeps `AF_INET`/`AF_INET6` (§7.3).
    pub egress: &'a [bool],
    /// `BLASTIT_PROBE_TIMEOUT_MS`, parsed by stage 0 (`stage0`'s default).
    pub timeout: Duration,
    /// `true` while CPython is live in this image and the GIL is held (§9.3).
    pub interpreter_live: bool,
    /// `true` in production (`setresuid` + `RLIMIT_NPROC`), `false` in `dev`.
    pub drop_privileges: bool,
}

/// The most of a probe child's stderr a refusal quotes: PHP's own startup log
/// can be long, and this becomes one log line (≤ 2048 bytes, §1.1).
const PROBE_MESSAGE_MAX: usize = 512;

/// `ADR-0010` §3: one contained probe child per `entrypoint php`, forked
/// **before** `bind_tcp`, that loads the pair stage 0 inspected and runs
/// `blastit_php_worker_init` + `blastit_php_worker_shutdown` in it.
///
/// The load stays below the fork. Nothing in stage 0 or stage 1 proper reaches a
/// `libphp` symbol, and a pair that cannot start is this boot's refusal — `exit
/// 1` with `ADR-0010` §4.1's sentence — instead of a worker's first request.
///
/// The child is contained exactly as its worker will be: its own process group,
/// the route's uid (production), `PR_SET_PDEATHSIG(SIGKILL)`, the route's
/// `RLIMIT_*` and [`SeccompRole::EmbeddedView`] with the route's `egress`.
///
/// # Errors
///
/// The refusal sentence: the route, `(PHP X.Y, <arch>-<libc>)`, the distro
/// package and `BLASTIT_LIBPHP`, plus what the child said (or that it hung).
pub fn probe_bindings(probe: PhpProbeBoot<'_>) -> Result<(), String> {
    for (slot, binding) in probe.bindings.iter().enumerate() {
        let Some(binding) = binding else {
            continue;
        };
        let Some(info) = probe.routes.get(slot) else {
            return Err(format!(
                "PHP view slot {slot} holds a libphp binding but no route; the manifest and the \
                 scheduler's routing table disagree"
            ));
        };
        let egress = probe.egress.get(slot).copied().unwrap_or(false);
        probe_route(&probe, info, binding, egress)?;
    }
    Ok(())
}

/// Forks `info`'s probe child and reads the verdict.
fn probe_route(
    probe: &PhpProbeBoot<'_>,
    info: &RouteInfo,
    binding: &PhpBinding,
    egress: bool,
) -> Result<(), String> {
    let (message, message_write) = pipe().map_err(|err| {
        format!(
            "route {:?}: the entrypoint php probe child's diagnostic pipe: {err}",
            info.name
        )
    })?;
    // The child keeps the pinned descriptors and nothing else: no channel, no
    // config pipe, no listener. Its stdout is `/dev/null`; its stderr is the
    // pipe this reads, so the reason a load failed comes back with the verdict.
    let child = ChildIo {
        stdout: None,
        stderr: Some(message_write),
        keep: binding.pinned_fds(),
        limits: Rlimits {
            nproc: probe.drop_privileges.then(|| {
                probe
                    .cfg
                    .view_worker_cap()
                    .saturating_add(VIEW_PROCESS_MARGIN) as u64
            }),
            nofile: Some(VIEW_NOFILE),
            core_zero: true,
        },
        working_dir: Some(binding.view_tmp.clone()),
    };
    let fork = crate::forking::fork(probe.interpreter_live).map_err(|err| {
        format!(
            "route {:?}: the entrypoint php probe child could not be forked: {err}",
            info.name
        )
    })?;
    let pid = match fork {
        Fork::Parent(pid) => {
            // The write end is the child's alone: holding a copy here would make
            // the read below wait for a writer that never closes.
            drop(child);
            pid
        }
        Fork::Child => probe_child(probe, info, binding, egress, child, message),
    };
    // Best effort, and the child asks for the same group a moment later: this
    // closes the window in which the timeout's `kill(-pid)` would name a group
    // that does not exist yet.
    join_group(pid);
    await_probe(pid, &message, probe.timeout).map_or_else(
        |problem| {
            // The manifest's `entry`, as written: the absolute path is what the
            // worker uses, and the operator edits the manifest.
            let declared = binding
                .entry
                .strip_prefix(&binding.document_root)
                .unwrap_or(&binding.entry);
            Err(crate::stage0::php_probe_refusal(
                &info.name,
                &declared.display().to_string(),
                &problem,
                probe.cfg.php.as_ref().map(|record| record.version),
            ))
        },
        |()| Ok(()),
    )
}

/// Everything the probe child does after the fork. Never returns.
fn probe_child(
    probe: &PhpProbeBoot<'_>,
    info: &RouteInfo,
    binding: &PhpBinding,
    egress: bool,
    child: ChildIo,
    message: OwnedFd,
) -> ! {
    // `ChildIo::prepare` closes every descriptor that is not in `keep`, this one
    // included; the `OwnedFd` must not drop afterwards or it would close a
    // number the child may since have reused.
    std::mem::forget(message);
    // The parent composes the sentence (it knows the route and the manifest's
    // `entry`), so the child says only what went wrong.
    let fail = |what: &str| -> ! { child_exit(what, 1) };
    // `setpgid(0, 0)` before the filter, which refuses it afterwards (§7.3).
    if let Err(err) = child.prepare().and_then(|()| own_process_group()) {
        fail(&err.to_string());
    }
    if probe.drop_privileges
        && let Err(err) = blastit_sys::drop_privileges(info.uid)
    {
        fail(&format!("dropping to uid {}: {err}", info.uid));
    }
    if let Err(err) = harden_child(Signal::KILL) {
        fail(&format!("{err}"));
    }
    if let Err(err) = install_filter(probe.cfg.seccomp, SeccompRole::EmbeddedView { egress }) {
        fail(&format!("{err}"));
    }
    // Re-applied after `drop_privileges`, which clears the flag (ADR-0010 §2).
    let _ = set_dumpable(false);
    if let Err(err) = set_view_environment(&binding.view_tmp) {
        fail(&format!("HOME/TMPDIR: {err}"));
    }
    match load_and_shut_down(binding) {
        Ok(()) => child_exit("", 0),
        Err(message) => fail(&message),
    }
}

/// Loads the pinned pair, starts the module, and shuts it down again: the whole
/// of what `ADR-0010` §3 asks a probe to prove.
///
/// [`blastit_php_sapi::Loaded::load`] is the ABI check
/// (`blastit_php_abi_version() == 1`) and the minor check
/// (`blastit_php_minor()` against the host's `php_version()`), so a pair that
/// disagrees never reaches `worker_init`; `worker_init` is what proves the
/// module and the INI lockdown actually start on this host.
fn load_and_shut_down(binding: &PhpBinding) -> Result<(), String> {
    let mut loaded = blastit_php_sapi::Loaded::load(
        SharedObject::Fd(binding.libphp.as_raw_fd()),
        SharedObject::Fd(binding.shim.as_raw_fd()),
    )
    .map_err(|err| err.to_string())?;
    loaded
        .worker_init(&binding.ini, &binding.view_tmp)
        .map_err(|err| err.to_string())?;
    loaded.worker_shutdown();
    Ok(())
}

/// Makes the calling process the leader of a new process group.
///
/// Runs in the child, **before** the filter: [`SeccompRole::EmbeddedView`]
/// refuses `setpgid` once installed, so the one legitimate call cannot be the
/// one the filter answers.
fn own_process_group() -> std::io::Result<()> {
    // SAFETY: `setpgid` takes two pids by value; 0 means the calling process, so
    // this is the child putting itself in a group of its own.
    if unsafe { libc::setpgid(0, 0) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The parent's half of [`own_process_group`], for the fork's race.
///
/// `EACCES`/`ESRCH` mean the child already did it or has exited, which is the
/// answer this asked for.
fn join_group(pid: Pid) {
    // SAFETY: `setpgid` takes two pids by value, and the second names this boot's
    // own child.
    let _ = unsafe { libc::setpgid(pid.as_raw(), pid.as_raw()) };
}

/// Waits for `pid` to finish, or kills its group at `cap`.
///
/// The verdict is the child's own exit plus what it printed: a clean `0` is the
/// only success. Every other outcome — a non-zero exit, a signal, a child that
/// never returns — is a boot refusal, and the timeout kills `-pgid` (then the
/// pid, if the group never came to exist) and reaps before answering.
fn await_probe(pid: Pid, message: &OwnedFd, cap: Duration) -> Result<(), String> {
    let deadline = Instant::now() + cap;
    let mut text = Vec::new();
    loop {
        if let Some(status) = try_wait(pid) {
            // The child is gone, so it holds no writer: this reads to end of
            // file instead of blocking on a live process.
            drain_to_end(message, &mut text);
            return match status {
                ExitStatus::Code(0) => Ok(()),
                other => Err(format!(
                    "the probe child (pid {pid}) {other}{}",
                    detail(&text)
                )),
            };
        }
        let now = Instant::now();
        if now >= deadline {
            kill_probe(pid);
            return Err(format!(
                "the probe child (pid {pid}) did not load `libphp` and its shim within {} ms, \
                 so it was killed; raise BLASTIT_PROBE_TIMEOUT_MS if this host needs longer",
                cap.as_millis()
            ));
        }
        let wait = (deadline - now).min(Duration::from_millis(20));
        if wait_readable(message.as_fd(), wait)
            .map_err(|err| format!("the probe child (pid {pid}) diagnostic pipe: {err}"))?
        {
            read_available(message, &mut text);
        }
    }
}

/// `waitpid(pid, WNOHANG)`: the child's status once it ended, `None` while it is
/// still running.
fn try_wait(pid: Pid) -> Option<ExitStatus> {
    let mut status = 0;
    // SAFETY: `waitpid` writes one status word into `status`; `WNOHANG` makes it
    // return immediately, and the pid is this boot's own child.
    let waited = unsafe { libc::waitpid(pid.as_raw(), &mut status, libc::WNOHANG) };
    if waited == pid.as_raw() {
        ExitStatus::from_raw(status)
    } else {
        None
    }
}

/// `SIGKILL`s the probe's process group, then reaps it.
///
/// The group is what the child made for itself; the single-pid `kill` is the
/// fallback for the race where the parent's `setpgid` and the child's have not
/// landed. Reaping is bounded, so a child stuck in uninterruptible sleep cannot
/// hold the boot that is refusing on its account — `PR_SET_PDEATHSIG` is the
/// backstop.
fn kill_probe(pid: Pid) {
    // SAFETY: a negative pid names a process group and a positive one a process;
    // both are this boot's own child's.
    unsafe {
        libc::kill(-pid.as_raw(), libc::SIGKILL);
        libc::kill(pid.as_raw(), libc::SIGKILL);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if try_wait(pid).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Reads what is already in the pipe, without waiting for more.
fn read_available(fd: &OwnedFd, text: &mut Vec<u8>) {
    let mut buf = [0u8; 256];
    if let Ok(read) = rustix::io::read(fd, &mut buf) {
        keep_probe_text(text, &buf[..read]);
    }
}

/// Reads the pipe to end of file: only correct once every writer is gone.
fn drain_to_end(fd: &OwnedFd, text: &mut Vec<u8>) {
    let mut buf = [0u8; 256];
    loop {
        match rustix::io::read(fd, &mut buf) {
            Ok(0) => return,
            Ok(read) => keep_probe_text(text, &buf[..read]),
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => return,
        }
    }
}

/// Appends at most [`PROBE_MESSAGE_MAX`] bytes: the rest is read and dropped, so
/// a chatty child never fills the pipe and blocks.
fn keep_probe_text(text: &mut Vec<u8>, bytes: &[u8]) {
    let room = PROBE_MESSAGE_MAX.saturating_sub(text.len());
    if room > 0 {
        text.extend_from_slice(&bytes[..bytes.len().min(room)]);
    }
}

/// The child's stderr as one clause: whitespace collapsed (PHP prints several
/// lines), empty when it said nothing.
fn detail(text: &[u8]) -> String {
    let text = String::from_utf8_lossy(text);
    let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.is_empty() {
        String::new()
    } else {
        format!(": {words}")
    }
}

/// `max_execution_time`, in whole seconds, from the scheduler's wall clock: a
/// runaway script bails out as a contained fatal instead of a stuck worker.
fn request_timeout_s(config: &Config) -> u32 {
    let seconds = config.request_timeout.as_secs();
    u32::try_from(seconds).unwrap_or(u32::MAX).max(1)
}

/// Requests per worker before it retires. `0` means "no cap" in the
/// configuration and `u32::MAX` in the worker, which is the same rule.
fn max_requests(config: &Config) -> u32 {
    if config.view_max_requests == 0 {
        u32::MAX
    } else {
        u32::try_from(config.view_max_requests).unwrap_or(u32::MAX)
    }
}

/// The body the worker buffers before PHP runs (`post_max_size`'s twin).
fn max_body_bytes(config: &Config) -> usize {
    usize::try_from(config.max_body_bytes)
        .unwrap_or(usize::MAX)
        .max(1)
}

/// Everything the fork needs about the route it is forking (§4.3's `SpawnSpec`
/// minus the fields `MB.3.13.2` owns).
pub struct PhpViewSpawn<'a> {
    /// Process-wide configuration (`BLASTIT_SECCOMP`, `view_worker_cap`).
    pub cfg: &'a Config,
    /// Frame limits of `view.chan`.
    pub limits: Limits,
    /// Route metadata and target uid.
    pub info: &'a RouteInfo,
    /// What the child loads and runs.
    pub binding: &'a PhpBinding,
    /// Signal mask to restore in the child.
    pub mask: SignalMask,
    /// Whether the route's filter keeps `AF_INET`/`AF_INET6` (§7.3).
    pub egress: bool,
    /// Private scratch directory (`HOME`, `TMPDIR`, `chdir`).
    pub dir: PathBuf,
    /// `true` in production (`setresuid` + `RLIMIT_NPROC`), `false` in `dev`.
    pub drop_privileges: bool,
}

/// A freshly forked PHP view, as the parent sees it.
pub struct ForkedPhpView {
    /// The new process.
    pub pid: Pid,
    /// The scheduler's end of `view.chan`.
    pub scheduler_end: OwnedFd,
    /// The read end of the view's stdout pipe.
    pub stdout: OwnedFd,
    /// The read end of the view's stderr pipe.
    pub stderr: OwnedFd,
    /// How long the fork took, in milliseconds.
    pub fork_ms: f64,
}

/// Forks a PHP view process for `spawn`, or returns the error and leaves the
/// process as it was.
///
/// `interpreter_live` is `true` exactly when CPython is initialised in this
/// image and the caller holds the GIL — a wheel install, where init forks on
/// the interpreter's thread (§9.3). The standalone binary forks with `false`
/// and has no interpreter to stop. [`crate::supervise::Supervisor`] passes
/// [`crate::supervise::interpreter_live`], which is the same answer the WSGI
/// zygote gives.
///
/// The configuration is validated **before** the fork, so a bad entry or an
/// empty INI is an error the parent can log rather than a child that dies on
/// its own.
///
/// # Errors
///
/// [`std::io::Error`] when a socketpair or pipe cannot be created, `fork`
/// refuses to run, or the route's configuration cannot be served.
#[cfg(feature = "php")]
pub fn fork_php_view(
    spawn: PhpViewSpawn<'_>,
    interpreter_live: bool,
) -> std::io::Result<ForkedPhpView> {
    let config = runtime_config(&spawn)?;
    let (view_end, scheduler_end) = socketpair_stream()?;
    let (stdout_read, stdout_write) = pipe()?;
    let (stderr_read, stderr_write) = pipe()?;

    // The pinned `libphp`/shim descriptors must survive the child sequence:
    // `dlopen` names them `/proc/self/fd/<n>`, so `close_fds_except` closing
    // them would make the load fail with `ENOENT` in production.
    let mut keep = vec![view_end.as_raw_fd()];
    keep.extend(spawn.binding.pinned_fds());
    let child = ChildIo {
        stdout: Some(stdout_write),
        stderr: Some(stderr_write),
        keep,
        limits: Rlimits {
            nproc: spawn.drop_privileges.then(|| {
                spawn
                    .cfg
                    .view_worker_cap()
                    .saturating_add(VIEW_PROCESS_MARGIN) as u64
            }),
            nofile: Some(VIEW_NOFILE),
            core_zero: true,
        },
        working_dir: Some(spawn.dir.clone()),
    };

    let started = std::time::Instant::now();
    let fork = crate::forking::fork(interpreter_live)?;
    let fork_ms = started.elapsed().as_secs_f64() * 1000.0;

    match fork {
        Fork::Parent(pid) => Ok(ForkedPhpView {
            pid,
            scheduler_end,
            stdout: stdout_read,
            stderr: stderr_read,
            fork_ms,
        }),
        Fork::Child => child_main(spawn, config, child, view_end),
    }
}

/// The runtime configuration for `spawn`, validated in the parent.
#[cfg(feature = "php")]
fn runtime_config(
    spawn: &PhpViewSpawn<'_>,
) -> std::io::Result<blastit_view::php::PhpRuntimeConfig> {
    let config = blastit_view::php::PhpRuntimeConfig {
        entry: spawn.binding.entry.clone(),
        document_root: spawn.binding.document_root.clone(),
        script_name: crate::config::mount_point(spawn.cfg.script_name.as_deref()),
        ini: spawn.binding.ini.clone(),
        view_tmp: spawn.binding.view_tmp.clone(),
        max_requests: spawn.binding.max_requests,
        max_body_bytes: spawn.binding.max_body_bytes,
    };
    config.validate().map_err(std::io::Error::other)?;
    Ok(config)
}

/// Everything the child does after the fork. Never returns.
///
/// The child sequence is the one `blastit_sys` documents, with
/// [`SeccompRole::EmbeddedView`] in place of `View`: `execute`-family syscalls
/// and process creation are refused, because `libphp` must never make them and
/// no script may reach them even through an extension.
#[cfg(feature = "php")]
fn child_main(
    spawn: PhpViewSpawn<'_>,
    config: blastit_view::php::PhpRuntimeConfig,
    child: ChildIo,
    view_end: OwnedFd,
) -> ! {
    let outcome = child.prepare().and_then(|()| {
        if spawn.drop_privileges {
            blastit_sys::drop_privileges(spawn.info.uid)?;
        }
        harden_child(Signal::KILL)
    });
    let outcome = outcome.and_then(|()| {
        install_filter(
            spawn.cfg.seccomp,
            SeccompRole::EmbeddedView {
                egress: spawn.egress,
            },
        )
    });
    if let Err(err) = outcome {
        child_exit(&format!("view {}: {err}", spawn.info.name), 1);
    }
    // Re-applied after `drop_privileges`, which clears the flag (ADR-0010 §2).
    let _ = set_dumpable(false);
    if let Err(err) = set_view_environment(&spawn.binding.view_tmp) {
        child_exit(&format!("view {}: HOME/TMPDIR: {err}", spawn.info.name), 1);
    }
    if let Err(err) = restore_signal_mask_in_child(spawn.mask) {
        child_exit(&format!("view {}: {err}", spawn.info.name), 1);
    }

    let name = spawn.info.name.clone();
    let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        serve(spawn, config, view_end)
    }));
    match loaded {
        Ok(Ok(())) => child_exit("", 0),
        Ok(Err(message)) => child_exit(&format!("view {name}: {message}"), 1),
        Err(payload) => child_exit(
            &format!(
                "view {name}: panicked: {}",
                crate::child::panic_message(&payload)
            ),
            70,
        ),
    }
}

/// Loads `libphp` and serves until `max_requests`, a fatal bailout, or the
/// channel's end. The interpreter, once started, is never shut down here: the
/// process exits, which is what a worker whose module state is suspect must do.
#[cfg(feature = "php")]
fn serve(
    spawn: PhpViewSpawn<'_>,
    config: blastit_view::php::PhpRuntimeConfig,
    view_end: OwnedFd,
) -> Result<(), String> {
    let runtime = blastit_view::php::PhpEmbedRuntime::load(
        SharedObject::Fd(spawn.binding.libphp.as_raw_fd()),
        SharedObject::Fd(spawn.binding.shim.as_raw_fd()),
        config,
    )
    .map_err(|err| format!("{err}"))?;
    // `ADR-0010` §3 / `MB.3.56`: every request re-checks the resolved entry
    // against the record (`(st_dev, st_ino)`, `st_size`, `st_ctime`) this boot
    // took, before `libphp` opens `SCRIPT_FILENAME` for it.
    let pinned = PinnedEntryRuntime {
        inner: runtime,
        route: spawn.info.name.clone(),
        entry: spawn.binding.entry.clone(),
        pin: spawn.binding.entry_pin,
    };

    // SAFETY: `view_end` is this process's end of `view.chan`, created for it
    // before the fork; nothing else in this process owns the number.
    let raw = view_end.as_raw_fd();
    let chan = unsafe { std::os::unix::net::UnixStream::from_raw_fd(raw) };
    // The stream owns the descriptor from here and `serve_forever` closes it
    // when it is done, so the `OwnedFd` must not close it a second time.
    std::mem::forget(view_end);
    blastit_view::php::serve_forever(chan, std::sync::Arc::new(pinned), spawn.limits)
        .map_err(|err| format!("{err}"))
}

/// Pins the child's `HOME` and `TMPDIR` at its private scratch directory, the
/// way the WSGI view does (`zygote::set_view_environment`): a PHP application
/// that writes to `sys_get_temp_dir()`, `TMPDIR` or `$HOME` must land inside
/// the view's own `0700` directory and nowhere else. There is no interpreter
/// here, so this is `setenv(3)` through `std::env`.
///
/// # Errors
///
/// A path that cannot be an environment value (a NUL byte).
#[cfg(feature = "php")]
fn set_view_environment(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    if dir.as_os_str().as_bytes().contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} holds a NUL byte", dir.display()),
        ));
    }
    // SAFETY: this runs in the freshly forked child, which is single-threaded
    // and has not started anything else yet; `std::env::set_var` is unsafe only
    // because a concurrent reader of the process environment would race.
    unsafe {
        std::env::set_var("HOME", dir);
        std::env::set_var("TMPDIR", dir);
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Write;

    /// The child test's fully qualified name, re-run by its parent.
    const CHILD: &str = "php_view::tests::the_child_sequence_keeps_a_pinned_descriptor";
    /// Set in the child so it knows it is the re-exec'd process.
    const MARKER: &str = "BLASTIT_PHP_VIEW_CHILD";
    /// The file the child fd-pins.
    const FILE: &str = "BLASTIT_PHP_VIEW_CHILD_FILE";
    /// Where the child writes its one-line report.
    const REPORT: &str = "BLASTIT_PHP_VIEW_CHILD_REPORT";
    /// The directory `HOME`/`TMPDIR` must be pinned to.
    const TMP: &str = "BLASTIT_PHP_VIEW_CHILD_TMP";

    fn env_path(name: &str) -> PathBuf {
        PathBuf::from(
            std::env::var_os(name).unwrap_or_else(|| {
                panic!("{name} is not set; this test is re-exec'd by its parent")
            }),
        )
    }

    /// A `ChildIo` with nothing but the descriptors it is told to keep.
    fn child_io(keep: Vec<RawFd>) -> ChildIo {
        ChildIo {
            stdout: None,
            stderr: None,
            keep,
            limits: Rlimits {
                nproc: None,
                nofile: Some(64),
                core_zero: true,
            },
            working_dir: None,
        }
    }

    /// A one-view `entrypoint php` manifest, its front controller on disk, a
    /// `libphp`-shaped file, and a shim for 8.4 — the whole input of
    /// [`boot_bindings`].
    struct Boot {
        dir: PathBuf,
        config: Config,
        manifest: Manifest,
        routes: Vec<RouteInfo>,
    }

    const PAGE: &str = "\
app \"shop\"
entrypoint php \"public/index.php\"

view app \"/*\" {}
";

    impl Boot {
        /// `name` keeps two runs in one process apart.
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("blastit-php-binding-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("public")).expect("app dir");
            std::fs::create_dir_all(dir.join("shims")).expect("shim dir");
            std::fs::write(dir.join("public/index.php"), b"<?php echo 1;\n").expect("entry");
            let libphp = dir.join("libphp8.4.so");
            std::fs::write(&libphp, b"the library stage 0 inspected").expect("libphp");
            let shim = dir.join("shims").join("libblastit_php84-x86_64-gnu.so");
            std::fs::write(&shim, b"the shim stage 0 inspected").expect("shim");
            let (manifest, _warnings) =
                Manifest::parse_blastfile(PAGE, 20_000).expect("the page's manifest");
            let config = Config {
                manifest_dir: dir.clone(),
                php: Some(record(&libphp, &shim, 8, 4)),
                ..Config::default()
            };
            let routes = vec![RouteInfo::new("app", false, config.uid_base + 1000)];
            Self {
                dir,
                config,
                manifest,
                routes,
            }
        }

        /// The paths `boot_bindings` reads back from the record.
        fn libphp(&self) -> PathBuf {
            self.dir.join("libphp8.4.so")
        }

        fn shim(&self) -> PathBuf {
            self.dir
                .join("shims")
                .join("libblastit_php84-x86_64-gnu.so")
        }

        fn bindings(&self) -> Result<Vec<Option<PhpBinding>>, String> {
            boot_bindings(&self.config, &self.manifest, &self.routes, None)
        }
    }

    impl Drop for Boot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A `PhpRuntime` recording what `ADR-0010` §3 records for both objects.
    fn record(libphp: &Path, shim: &Path, major: u8, minor: u8) -> crate::config::PhpRuntime {
        use std::os::unix::fs::MetadataExt;
        let library = std::fs::metadata(libphp).expect("libphp metadata");
        let shim = std::fs::metadata(shim).expect("shim metadata");
        crate::config::PhpRuntime {
            libphp: libphp.to_path_buf(),
            dev: library.dev(),
            ino: library.ino(),
            version: crate::config::PhpVersion::new(major, minor),
            shim: shim_path_of(libphp),
            shim_dev: shim.dev(),
            shim_ino: shim.ino(),
        }
    }

    /// The shim path a `PhpRuntime` for `libphp` names, so a test can replace it.
    fn shim_path_of(libphp: &Path) -> PathBuf {
        libphp
            .parent()
            .expect("a directory")
            .join("shims")
            .join("libblastit_php84-x86_64-gnu.so")
    }

    /// The `(st_dev, st_ino)` stage 0 records for `path` and `open_pinned` checks.
    fn identity(path: &Path) -> (u64, u64) {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path)
            .unwrap_or_else(|err| panic!("{} must exist to be recorded: {err}", path.display()));
        (metadata.dev(), metadata.ino())
    }

    /// Replaces `path`'s contents with `contents` **and** proves the result is a
    /// different file from the one recorded as `recorded`.
    ///
    /// Write a side path in the same directory while the recorded object is still
    /// allocated, then `rename()` it over the recorded path: the replacement's
    /// inode exists while the recorded one is live, and POSIX only promises
    /// distinct inode numbers among files that are simultaneously alive, so the
    /// two cannot be equal. `remove_file()` followed by `fs::write()` is **not**
    /// a way to obtain a new inode — a filesystem may hand the freed number
    /// straight back, and then a pin that correctly saw a new file and one that
    /// saw the old file are indistinguishable.
    ///
    /// The trailing assertion is for the fixture, not the implementation: if it
    /// ever trips, this test stopped replacing anything and must fail here rather
    /// than accuse `open_pinned` of not noticing.
    fn replace_by_rename(path: &Path, recorded: (u64, u64), contents: &[u8]) {
        let name = path
            .file_name()
            .expect("the recorded path names a file")
            .to_string_lossy()
            .into_owned();
        let side = path.with_file_name(format!(".{name}.replacement"));
        std::fs::write(&side, contents).expect("the replacement's side path");
        std::fs::rename(&side, path).expect("renaming the replacement over the recorded file");
        let replaced = identity(path);
        assert_ne!(
            replaced,
            recorded,
            "the fixture did not replace the file {}: it still has stage 0's (st_dev, st_ino) \
             {recorded:?}, so this test would assert nothing",
            path.display()
        );
    }

    /// The accept branch: one binding per PHP slot, with the manifest's absolute
    /// `entry`, the rendered INI, the view's own directory, and the two
    /// descriptors the child loads through `/proc/self/fd/<n>`.
    #[test]
    fn a_php_slot_gets_one_binding_and_a_wsgi_slot_none() {
        let boot = Boot::new("accept");
        let bindings = boot.bindings().expect("the recorded pair resolves");
        assert_eq!(bindings.len(), 1);

        let binding = bindings[0].as_ref().expect("the PHP slot has a binding");
        assert_eq!(binding.entry, boot.dir.join("public/index.php"));
        assert_eq!(binding.document_root, boot.dir);
        assert_eq!(
            binding.view_tmp,
            crate::config::view_dir(boot.routes[0].uid)
        );
        assert!(
            binding.ini.contains("open_basedir=") && binding.ini.contains("sys_temp_dir="),
            "the INI lockdown is rendered once here: {}",
            binding.ini
        );
        assert_eq!(binding.max_requests, u32::MAX, "0 means no cap");
        // `MB.3.56`: the entry's whole record is taken beside the pair's, so a
        // worker that loses any half of it would be checking less than it says.
        // The protected legs (`critical_php_embed`) prove the refusals end to
        // end; this proves the boot records what it resolved, and holds it.
        assert_eq!(
            binding.entry_pin,
            EntryPin::of(&std::fs::metadata(&binding.entry).expect("the entry the boot recorded")),
            "the boot must record the resolved entry's identity, size and ctime"
        );
        // The `O_PATH` descriptor is on that same inode: this is the reference
        // that stops the kernel handing the number to a later `create`.
        let held = std::fs::read_link(format!("/proc/self/fd/{}", binding.entry_fd.as_raw_fd()))
            .expect("the entry's O_PATH descriptor");
        assert_eq!(
            held,
            std::fs::canonicalize(&binding.entry).expect("the entry's canonical path"),
            "the kept descriptor names the entry the boot pinned"
        );

        // The descriptors are the *files* stage 0 inspected, not paths, plus the
        // held entry.
        let fds = binding.pinned_fds();
        assert_eq!(fds.len(), 3, "libphp, shim and the pinned entry");
        let library = std::fs::read(format!("/proc/self/fd/{}", fds[0])).expect("the library fd");
        assert_eq!(library, b"the library stage 0 inspected");
        let shim = std::fs::read(format!("/proc/self/fd/{}", fds[1])).expect("the shim fd");
        assert_eq!(shim, b"the shim stage 0 inspected");

        // A view with no `entrypoint php` is a WSGI slot and gets nothing.
        let (wsgi, _warnings) =
            Manifest::parse_blastfile("view web \"/*\" {}\n", 20_000).expect("a WSGI manifest");
        let routes = vec![RouteInfo::new("web", false, boot.config.uid_base + 1000)];
        let none = boot_bindings(&boot.config, &wsgi, &routes, None).expect("nothing to do");
        assert_eq!(none.len(), 1);
        assert!(none[0].is_none(), "a WSGI slot carries no PHP binding");
    }

    /// `MB.3.56`'s per-request half, without a `libphp`: the file at the pinned
    /// path is the recorded file, and anything else at that path is the sentence
    /// (route, entry, fix) — never `Ok`. Two shapes, because one is not the
    /// other: a **rename** (another inode, the shape this check's identity half
    /// catches) and a **rewrite in place** (the same inode, which only the
    /// size/ctime half catches — the gap attempt 1's review measured).
    #[test]
    fn a_swapped_entry_is_refused_with_the_route_the_entry_and_the_fix() {
        let boot = Boot::new("entry-pin");
        let bindings = boot.bindings().expect("the recorded pair resolves");
        let binding = bindings[0].as_ref().expect("the PHP slot has a binding");
        let recorded = binding.entry_pin;
        assert_eq!(
            recorded,
            EntryPin::of(&std::fs::metadata(&binding.entry).expect("the entry")),
            "the record is the entry the boot resolved"
        );
        assert_eq!(
            verify_entry("app", &binding.entry, recorded),
            Ok(()),
            "the pinned entry verifies before anything swaps it"
        );

        replace_by_rename(
            &binding.entry,
            (recorded.dev, recorded.ino),
            b"<?php echo 'DECOY-RAN';\n",
        );
        let refusal = verify_entry("app", &binding.entry, recorded)
            .expect_err("a different inode at the pinned path must be refused");
        assert!(
            refusal.contains("route \"app\""),
            "the sentence names the route: {refusal}"
        );
        assert!(
            refusal.contains(&binding.entry.display().to_string()),
            "and the entry: {refusal}"
        );
        assert!(
            refusal.contains("changed since this boot recorded it")
                && refusal.contains("another file"),
            "and what moved: {refusal}"
        );
        assert!(
            refusal.contains("restart blastit to run the file as it is now"),
            "and the fix: {refusal}"
        );

        // An entry that is gone is the same refusal, not a re-resolution.
        std::fs::remove_file(&binding.entry).expect("removing the entry");
        let missing = verify_entry("app", &binding.entry, recorded)
            .expect_err("a missing entry must be refused");
        assert!(
            missing.contains("cannot be read now")
                && missing.contains("restart blastit to run the file as it is now"),
            "a missing entry is refused with the fix: {missing}"
        );
    }

    /// The shape attempt 1 missed: a **rewrite in place**. `cp`, `tar -x` and a
    /// deploy script open the entry and write through it, so the inode does not
    /// change and an identity-only pin sees nothing; `st_size`/`st_ctime` are
    /// what catch it.
    ///
    /// The rewrite is same-length in one leg and longer in the other, so neither
    /// half of the content record is decorative: a same-size rewrite still moves
    /// `st_ctime`, and a longer one moves both.
    ///
    /// Mutation this kills: comparing only `(st_dev, st_ino)` (attempt 1) leaves
    /// this test red on the same-size leg, which is exactly the live
    /// `200 IN-PLACE-DECOY-RAN` the reviewer measured.
    #[test]
    fn an_entry_rewritten_in_place_is_refused() {
        let boot = Boot::new("entry-in-place");
        let bindings = boot.bindings().expect("the recorded pair resolves");
        let binding = bindings[0].as_ref().expect("the PHP slot has a binding");
        let recorded = binding.entry_pin;
        let inode = |path: &Path| {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(path).expect("the entry's metadata");
            (metadata.dev(), metadata.ino())
        };
        // The page the boot recorded is `<?php echo 1;\n` (14 bytes, `Boot`'s
        // fixture); this decoy is the same length, so only `st_ctime` can catch
        // it. The assertion is on the fixture: change `Boot`'s page and this leg
        // must change with it instead of silently testing the easy case.
        let original = std::fs::read(&binding.entry).expect("the entry");
        let same_length = b"<?php echo 2;\n";
        assert_eq!(
            same_length.len(),
            original.len(),
            "the same-length leg must keep st_size"
        );
        assert_ne!(
            same_length.as_slice(),
            original.as_slice(),
            "the decoy must actually differ from the recorded bytes"
        );
        // `st_ctime` comes from the kernel's *coarse* clock, which advances one
        // tick (~1–4 ms) at a time: two writes inside one tick carry the same
        // stamp, and a same-size rewrite between them would be invisible to any
        // metadata record (`LESSONS.md`, `MB.3.56`). A live swap is always later
        // than the boot's own `stat`; this fixture has to cross a tick to model
        // that, or it measures the tick rather than the pin.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&binding.entry, same_length).expect("rewriting the entry in place");
        assert_eq!(
            inode(&binding.entry),
            (recorded.dev, recorded.ino),
            "a write in place must keep the inode, or this is the rename leg again"
        );
        let same_size_now = EntryPin::of(&std::fs::metadata(&binding.entry).expect("the entry"));
        assert_eq!(
            same_size_now.size, recorded.size,
            "the fixture must keep st_size so only st_ctime can catch it"
        );
        assert_ne!(
            (same_size_now.ctime_s, same_size_now.ctime_ns),
            (recorded.ctime_s, recorded.ctime_ns),
            "a write in place must move st_ctime, or nothing can catch it"
        );
        let refusal = verify_entry("app", &binding.entry, recorded)
            .expect_err("a same-inode rewrite must be refused");
        assert!(
            refusal.contains("rewritten in place"),
            "the sentence names the shape: {refusal}"
        );
        assert!(
            refusal.contains("restart blastit to run the file as it is now"),
            "and the fix: {refusal}"
        );

        // And a longer rewrite (a real deploy) is the same refusal.
        std::fs::write(
            &binding.entry,
            b"<?php echo 'IN-PLACE-DECOY-RAN';\n/* longer */\n",
        )
        .expect("a longer rewrite");
        assert_eq!(
            inode(&binding.entry),
            (recorded.dev, recorded.ino),
            "still the same inode"
        );
        assert!(
            verify_entry("app", &binding.entry, recorded).is_err(),
            "a longer rewrite is refused too"
        );
    }

    /// The denied twin, per object: a file replaced between the record and the
    /// open is not the *file* stage 0 inspected, so the boot refuses it by name.
    ///
    /// The premise is a file identity, not a permission: `(st_dev, st_ino)` tells
    /// the inspected object apart from every other file at every uid, so this
    /// holds unprivileged and as root alike and belongs in no privileged tier.
    /// That also means the replacement has to be a genuinely different file —
    /// see [`replace_by_rename`], which writes a side path while the recorded
    /// object is still allocated and `rename()`s it over the recorded path. Do
    /// not "simplify" that back to `remove_file()` + `fs::write()`: an unlinked
    /// inode number may be handed straight back, the pin would then correctly see
    /// the same identity, and the fixture would silently test nothing.
    ///
    /// Each object is checked on its own — a record that verified only the
    /// library would pass the shim half of this test.
    #[test]
    fn a_replaced_libphp_or_shim_is_refused_by_name() {
        let boot = Boot::new("replaced");
        let record = boot.config.php.as_ref().expect("stage 0's record");
        replace_by_rename(
            &boot.libphp(),
            (record.dev, record.ino),
            b"a different library",
        );
        let message = boot
            .bindings()
            .expect_err("a replaced library is not the inspected one");
        assert!(message.contains("libphp"), "{message}");
        assert!(
            message.contains("not the file stage 0 inspected"),
            "{message}"
        );
        assert!(
            !message.contains(": shim"),
            "the library is the culprit: {message}"
        );

        let boot = Boot::new("replaced-shim");
        let record = boot.config.php.as_ref().expect("stage 0's record");
        replace_by_rename(
            &boot.shim(),
            (record.shim_dev, record.shim_ino),
            b"a different shim",
        );
        let message = boot
            .bindings()
            .expect_err("a replaced shim is not the inspected one");
        assert!(message.contains("shim"), "{message}");
        assert!(
            message.contains("not the file stage 0 inspected"),
            "{message}"
        );
        assert!(
            !message.contains(": libphp"),
            "the shim is the culprit: {message}"
        );
    }

    /// A boot that lost stage 0's record cannot load anything: that is a
    /// refusal naming the route, never a first-request fault.
    #[test]
    fn a_missing_record_is_refused_by_route() {
        let mut boot = Boot::new("no-record");
        boot.config.php = None;
        let message = boot.bindings().expect_err("no record, no worker");
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("stage 0 recorded no NTS libphp"),
            "{message}"
        );
    }

    #[test]
    fn a_pinned_descriptor_survives_the_child_sequence() {
        // A real `fork` refuses a multi-threaded image, so the child sequence
        // runs in a re-exec'd, single-threaded copy of this test binary — the
        // same shape `crate::immortal`'s marker tests use.
        let dir = std::env::temp_dir().join(format!("blastit-php-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let pinned = dir.join("libphp.so");
        std::fs::write(&pinned, b"a pinned payload").expect("pinned file");
        let report = dir.join("report");
        let exe = std::env::current_exe().expect("this test binary's path");
        let output = std::process::Command::new(exe)
            .args([CHILD, "--exact", "--ignored", "--nocapture"])
            .env(MARKER, "1")
            .env(FILE, &pinned)
            .env(REPORT, &report)
            .env(TMP, &dir)
            .output()
            .expect("re-running this test binary as the child");
        assert!(
            output.status.success(),
            "the child sequence failed: {output:?}"
        );
        let text = std::fs::read_to_string(&report).expect("the child's report");
        assert_eq!(
            text,
            format!("ok:16:{}:{}", dir.display(), dir.display()),
            "the pinned descriptor or HOME/TMPDIR did not survive"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The re-exec'd child: pins two descriptors, runs the real child sequence,
    /// and reports what is still open afterwards.
    #[test]
    #[ignore = "re-exec'd as a single-threaded child by the test above"]
    fn the_child_sequence_keeps_a_pinned_descriptor() {
        let Ok(_marker) = std::env::var(MARKER) else {
            // `cargo test -- --ignored` by hand: there is no pinned file and no
            // report to write, so this is not the child run.
            return;
        };
        let file = env_path(FILE);
        let report_path = env_path(REPORT);
        let tmp = env_path(TMP);

        // A real entry this child can pin: the same file the pair uses, so the
        // `O_PATH` descriptor has something to hold. Its record is taken the way
        // a boot takes it.
        let (entry_pin, entry_fd) =
            pin_entry(&file, "app", "entry.php").expect("pinning the entry");
        let binding = PhpBinding {
            libphp: std::fs::File::open(&file).expect("pin libphp").into(),
            shim: std::fs::File::open(&file).expect("pin shim").into(),
            entry: file.clone(),
            entry_pin,
            entry_fd,
            document_root: PathBuf::from("/"),
            ini: "memory_limit=256M\n".to_string(),
            view_tmp: tmp.clone(),
            max_requests: 1,
            max_body_bytes: 1024,
        };
        let mut keep = binding.pinned_fds();
        assert_eq!(
            keep.len(),
            3,
            "libphp, the shim and the pinned entry are all descriptors"
        );
        let report = std::fs::File::create(&report_path).expect("report");
        keep.push(report.as_raw_fd());

        child_io(keep).prepare().expect("the child sequence");

        // `close_fds_except` must have left the pinned descriptors alone, and
        // the loader is handed an fd (not a path) so it can name them through
        // `/proc/self/fd/<n>`.
        let raw = binding.libphp.as_raw_fd();
        let kept = std::fs::read(format!("/proc/self/fd/{raw}")).expect("/proc/self/fd/<n>");
        assert_eq!(
            kept,
            std::fs::read(&file).expect("the file"),
            "the fd moved"
        );
        // The entry's `O_PATH` descriptor is the identity half of the pin
        // (`MB.3.56`): if `close_fds_except` dropped it, the recorded inode could
        // be freed and its number handed back between two requests.
        let entry_raw = binding.entry_fd.as_raw_fd();
        let entry_kept = std::fs::read_link(format!("/proc/self/fd/{entry_raw}"))
            .expect("the entry descriptor survives the child sequence");
        assert_eq!(
            entry_kept,
            std::fs::canonicalize(&binding.entry).expect("the entry's canonical path"),
            "the kept entry descriptor is not the pinned entry"
        );

        set_view_environment(&tmp).expect("HOME/TMPDIR");
        let home = std::env::var("HOME").expect("HOME");
        let temp = std::env::var("TMPDIR").expect("TMPDIR");
        (&report)
            .write_all(format!("ok:{}:{home}:{temp}", kept.len()).as_bytes())
            .expect("write the report");
    }
}
