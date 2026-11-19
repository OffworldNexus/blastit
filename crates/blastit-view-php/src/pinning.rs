//! The boot's PHP pins and the descriptors a worker is forked with
//! (`ADR-0010` §3, design §4.4 D2).
//!
//! Two halves, decided once each. Stage 0 inspected a host NTS `libphp` and the
//! shim for its tuple and recorded `(path, st_dev, st_ino)`; [`boot_bindings`]
//! is the boot half that re-opens exactly those objects — refusing a file whose
//! identity moved — records the resolved `entrypoint php` with an `O_PATH`
//! descriptor behind it, renders the `ZEND_INI_SYSTEM` blob once, and hands the
//! supervisor one [`PhpBinding`] per PHP slot. From then on `Ctl::Spawn` looks
//! the slot up in that vector instead of asking what kind of backend the route
//! is: the choice is data, not a branch in dispatch (`ADR-0009`).
//!
//! `verify_entry` is the record applied per request — the worker calls it
//! before the body is read and before `libphp` opens `SCRIPT_FILENAME` — and
//! [`EntryPin`] is its record.
//!
//! # Why the pin cannot live anywhere else
//!
//! The recording and the verification both need the boot's `O_PATH` descriptor
//! and the file it names. Neither may move to a crate `init` does not link, and
//! neither loses its refusal message: a swapped entry must read as the route,
//! the entry and the fix, never as `errno`.
//!
//! # What this module is told
//!
//! Values, never the root's configuration: a [`PhpSlot`] per view slot (its
//! route, uid, directory, egress, mount point, and the identity/filter stage 1
//! decided), the [`PhpBootLimits`] the deployment set, and stage 0's
//! [`PhpRuntime`] record. Design §4.4 D2 rejected passing `Config` or
//! `RouteInfo`: the first is a private root module and the second would drag
//! the scheduler and ingress into every view crate.

use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Duration;
// The swap-point rendezvous exists only in a `debug_assertions` build, so the
// deadline it reads is not imported by a release artifact.
#[cfg(debug_assertions)]
use std::time::Instant;

use blastit_manifest::Manifest;
use blastit_php_sapi::{Egress, IniPolicy};
use blastit_sys::child::{Filter, Privileges};
use rustix::fs::{Mode, OFlags};

use crate::discovery::PhpRuntime;

/// Everything the child needs to load and run `entrypoint php`.
///
/// Both shared objects are **descriptors**, never paths: the child keeps them
/// across [`blastit_sys::child::ChildIo::prepare`] and `dlopen`s
/// `/proc/self/fd/<n>`, so the file cannot be swapped between the
/// `(st_dev, st_ino)` verification and the load (`ADR-0010` §3). There is no
/// path variant and therefore no fallback: a missing descriptor is a boot
/// refusal, not a load of something unverified.
///
/// `entry` is pinned by its **record**, not by a descriptor, because
/// `SCRIPT_FILENAME` has to stay the path the application reads (and the path
/// `open_basedir` is written in terms of): the boot records the resolved file's
/// `(st_dev, st_ino)` **and** `st_size`/`st_ctime` here, holds an `O_PATH`
/// descriptor so that inode cannot be freed and its number handed back, and
/// every worker compares the file at that path against the record **before each
/// request** (the worker calls `verify_entry`).
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
    /// The manifest's own spelling of [`PhpBinding::entry`] — the `entrypoint
    /// php` string, relative to the application root.
    ///
    /// A live reload re-resolves **this**, never a path a save happens to leave
    /// behind: [`PhpBinding::repin_entry`] joins it under the manifest directory
    /// again, so a reload can never widen what the boot confined.
    pub declared_entry: String,
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
    /// The route this binding serves: every refusal below names it.
    pub route: String,
    /// The uid the route runs under, for the probe child's identity and its
    /// refusals. A `dev`/`check` boot keeps init's identity instead of dropping
    /// to it (design §4.4 D1 decides that), and this is still the route's own
    /// number.
    pub uid: u32,
    /// `SCRIPT_NAME`, normalised by the composition root: empty at the root,
    /// otherwise `/prefix`.
    pub script_name: String,
    /// The identity the child keeps, as stage 1 decided it (`DropTo(uid)` in
    /// production, `Shared` in `dev`/`check`).
    ///
    /// Carried here, not re-derived at the fork: this crate may not build a
    /// `Privileges` (design §4.4 D1), and the fork must be given the value the
    /// binding was built with.
    pub privileges: Privileges,
    /// The filter the child installs, as stage 1 decided it:
    /// `Seccomp(EmbeddedView { egress })` in every mode, `Off` only for an
    /// explicit `BLASTIT_SECCOMP=0`.
    pub filter: Filter,
}

impl PhpBinding {
    /// The descriptors the child must keep across `ChildIo::prepare`: the
    /// pinned `libphp` and shim, and the `O_PATH` descriptor on the entry.
    ///
    /// Without the first two `close_fds_except` closes exactly the files the
    /// child is about to `dlopen` through `/proc/self/fd/<n>`; without the third
    /// the recorded inode could be freed and its number reused between the
    /// per-request check and PHP's own open.
    pub(crate) fn pinned_fds(&self) -> Vec<RawFd> {
        vec![
            self.libphp.as_raw_fd(),
            self.shim.as_raw_fd(),
            self.entry_fd.as_raw_fd(),
        ]
    }

    /// Whether the file at [`PhpBinding::entry`] is still the one
    /// [`PhpBinding::entry_pin`] records (size and `st_ctime` included).
    ///
    /// The supervisor asks this before it forks a PHP worker on a `dev` boot
    /// with reload on: a save that landed before the watcher's debounce fired
    /// (or before the very first `SPAWN`) has to be pinned **now**, or the
    /// worker would refuse it as "changed since this boot recorded it".
    /// It is a read-only question — a file that cannot be read is *not*
    /// current, so the caller repins and turns the failure into its one
    /// `manifest.warning`.
    #[must_use]
    pub fn entry_is_current(&self) -> bool {
        std::fs::metadata(&self.entry)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .is_some_and(|metadata| EntryPin::of(&metadata) == self.entry_pin)
    }

    /// Records the `entrypoint php` again — the live-reload half of
    /// `ADR-0010` §3 (`MB.40.9`).
    ///
    /// A `blastit dev` save replaces the file at the pinned path (in place, or
    /// by an atomic `rename`), which is exactly what [`verify_entry`] refuses:
    /// the record is the boot's, and only a boot may move it. A dev boot that
    /// promises live reload is the one caller allowed to move it, and it does so
    /// by re-running the **boot's own resolution** — [`resolve_entry`] under
    /// `manifest_dir`, then [`pin_entry`] — so a save cannot widen the
    /// confinement the boot decided: a `..` path, a missing file and a symlink
    /// whose target is outside the root are all `Err`, and the caller keeps the
    /// previous pin.
    ///
    /// # Errors
    ///
    /// The sentence naming the route, the declared entry and why it cannot be
    /// pinned now. Whoever sees it emits one `manifest.warning`; the pins that
    /// were already in force stay, so nothing fails open.
    pub fn repin_entry(&mut self, manifest_dir: &Path, declared_entry: &str) -> Result<(), String> {
        let entry = resolve_entry(manifest_dir, declared_entry, &self.route)?;
        let (entry_pin, entry_fd) = pin_entry(&entry, &self.route, declared_entry)?;
        self.entry = entry;
        self.entry_pin = entry_pin;
        self.entry_fd = entry_fd;
        Ok(())
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
#[cfg(debug_assertions)]
const TEST_SWAP_TIMEOUT: Duration = Duration::from_secs(30);

/// `<point>.<suffix>`: the two sidecars that make the rendezvous an ordinary
/// pair of files, so both sides talk about the same path without a socket, a
/// pipe, or a second descriptor to pass.
#[cfg(debug_assertions)]
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
/// (`stage0::TEST_SWAP_POINT_MEMBER`); stage 1 reaches this once,
/// immediately before [`boot_bindings`] opens the pair. The boot announces
/// itself by creating `<point>.ready`, then blocks until `<point>.go` exists.
/// The test therefore cannot be early (it waits for `.ready`) and stage 1 cannot
/// be early (it waits for `.go`), so a rename the test makes in between is
/// inside the window because the two sides say so — not because the runner was
/// slow.
///
/// **A test seam with no production arm.** The whole function — and the
/// variable's name in `stage0::TEST_SWAP_POINT_VAR` — exists only under
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
/// written or `<point>.go` does not arrive within `TEST_SWAP_TIMEOUT`. Every
/// call site turns that into this boot's `exit 1` before any listener binds.
#[cfg(debug_assertions)]
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

/// One PHP slot, as stage 1 knows it: the values [`boot_bindings`] builds a
/// binding from, and the values a forked child is described by
/// (`view-crates-architecture` §4.4 D2).
///
/// One `Privileges`/`Filter` per slot, decided by stage 1 — never here, and
/// never in the child, where every `BLASTIT_*` is gone after the `execve`
/// barrier and a mode read there would be forgeable.
#[derive(Debug, Clone)]
pub struct PhpSlot {
    /// The route's name: the child's log prefix and the refusal sentence's
    /// first word.
    pub route: String,
    /// The uid the route runs under.
    pub uid: u32,
    /// The view's private scratch directory: the child's `chdir`, `HOME`,
    /// `TMPDIR`, PHP's `sys_temp_dir` — one value, never two (S6).
    pub dir: PathBuf,
    /// Whether the route's filter keeps `AF_INET`/`AF_INET6` (§7.3), which also
    /// pins `allow_url_fopen` in the INI blob.
    pub egress: bool,
    /// `SCRIPT_NAME`, normalised by the composition root: empty at the root.
    pub script_name: String,
    /// The identity the child keeps: `DropTo(uid)` in production, `Shared` in
    /// `dev`/`check`.
    pub privileges: Privileges,
    /// The filter the child installs: `Seccomp(EmbeddedView { egress })` in
    /// every mode, `Off` only for an explicit `BLASTIT_SECCOMP=0`.
    pub filter: Filter,
}

/// The boot's PHP budgets: the deployment's, not a route's (`ADR-0012` §12.3
/// removed the per-view `workers` keyword, so a file declares none).
#[derive(Debug, Clone, Copy)]
pub struct PhpBootLimits {
    /// The scheduler's wall clock, which `max_execution_time` mirrors.
    pub request_timeout: Duration,
    /// Requests per worker before it retires; `u32::MAX` is "no cap".
    pub max_requests: u32,
    /// The largest request body one worker buffers.
    pub max_body_bytes: usize,
}

impl PhpBootLimits {
    /// `max_execution_time`, in whole seconds: a runaway script bails out as a
    /// contained fatal instead of a stuck worker.
    ///
    /// Clamped at one second: PHP reads the directive as an integer, and a zero
    /// would mean "no limit", which is the opposite of what the boot wants.
    fn request_timeout_s(&self) -> u32 {
        let seconds = self.request_timeout.as_secs();
        u32::try_from(seconds).unwrap_or(u32::MAX).max(1)
    }
}

/// `ADR-0010` §3: builds every PHP slot's [`PhpBinding`], or the sentence the
/// boot refuses with.
///
/// Stage 0 recorded `(path, st_dev, st_ino)` for the NTS `libphp` and the shim
/// it inspected in [`PhpRuntime`]; this re-opens both with
/// `O_RDONLY | O_CLOEXEC | O_NOFOLLOW` and refuses a file whose identity moved
/// (a package upgraded between the two stages would otherwise run something
/// nobody inspected). Nothing is loaded here: the descriptors travel into the
/// view child as kept descriptors and the worker `dlopen`s
/// `/proc/self/fd/<n>`.
///
/// `slots` is indexed by **view slot**, with `None` for a slot no manifest route
/// names (a `dev --learn` boot's slot 0 is the unrouted-request fallback), so
/// today's numbering survives the move to values.
///
/// # Errors
///
/// A message naming the route, the file and the fix. Every case is a boot
/// refusal (`exit 1`, no listener bound), never a first-request fault.
pub fn boot_bindings(
    manifest: &Manifest,
    slots: &[Option<PhpSlot>],
    libphp: Option<&PhpRuntime>,
    document_root: &Path,
    limits: PhpBootLimits,
) -> Result<Vec<Option<PhpBinding>>, String> {
    let mut bindings: Vec<Option<PhpBinding>> = slots.iter().map(|_| None).collect();
    // §12.2: one `entrypoint php` for the whole file, so **every** view runs it.
    // A file with no front controller binds nothing.
    let Some(php) = manifest.php() else {
        return Ok(bindings);
    };
    for (slot, values) in slots.iter().enumerate() {
        let Some(values) = values else {
            continue;
        };
        let Some(record) = libphp else {
            return Err(format!(
                "route {:?}: stage 0 recorded no NTS libphp for entrypoint php {:?}, so there \
                 is nothing to load; restart blastit so the discovery gate runs again",
                values.route, php.entry
            ));
        };
        let entry = resolve_entry(document_root, &php.entry, &values.route)?;
        let (entry_pin, entry_fd) = pin_entry(&entry, &values.route, &php.entry)?;
        // The same private directory the fork makes the child's `chdir`,
        // `HOME` and `TMPDIR`, so `sys_temp_dir` cannot disagree with them.
        let egress = if values.egress {
            Egress::Allowed
        } else {
            Egress::Denied
        };
        let ini = IniPolicy::new(
            document_root,
            &values.dir,
            limits.request_timeout_s(),
            egress,
        )
        .map_err(|err| {
            format!(
                "route {:?}: the INI lockdown for {} could not be rendered: {err}",
                values.route,
                values.dir.display()
            )
        })?;
        bindings[slot] = Some(PhpBinding {
            libphp: open_pinned(
                &record.libphp,
                record.dev,
                record.ino,
                &values.route,
                "libphp",
            )?,
            shim: open_pinned(
                &record.shim,
                record.shim_dev,
                record.shim_ino,
                &values.route,
                "shim",
            )?,
            entry,
            declared_entry: php.entry.clone(),
            entry_pin,
            entry_fd,
            document_root: document_root.to_path_buf(),
            ini: ini.as_str().to_owned(),
            view_tmp: values.dir.clone(),
            max_requests: limits.max_requests,
            max_body_bytes: limits.max_body_bytes,
            route: values.route.clone(),
            uid: values.uid,
            script_name: values.script_name.clone(),
            privileges: values.privileges,
            filter: values.filter,
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

/// Resolves the manifest's `entrypoint php` under `manifest_dir`, or the
/// sentence the boot (and a live reload) refuses with.
///
/// One resolution for both callers, so a save can never pin something the boot
/// would not have: the declared string is joined under the manifest directory
/// (a `..` component is refused before anything is opened), the result must
/// exist, and the **resolved** file — symlinks followed, the way `libphp`
/// follows `SCRIPT_FILENAME` — must still be inside the resolved root. A deploy
/// that reaches its front controller through a symlinked `current/` therefore
/// keeps an ordinary entry; a symlink pointing out of the tree is refused, so
/// the pin can never be moved onto a file `open_basedir` does not cover.
///
/// The returned path is the **declared** join, not the resolved one: it is
/// `SCRIPT_FILENAME`, which the application reads and `open_basedir` is written
/// in terms of. The resolved path is only the confinement check.
///
/// # Errors
///
/// A sentence naming the route, the declared entry and the reason — a path that
/// leaves the root, one that cannot be resolved now (a save caught mid-write,
/// or a file that is gone), or a symlink whose target is outside it.
fn resolve_entry(manifest_dir: &Path, declared: &str, route: &str) -> Result<PathBuf, String> {
    let entry = manifest_dir.join(declared);
    if !entry.starts_with(manifest_dir) {
        return Err(format!(
            "route {route:?}: entrypoint php {declared:?} leaves the application root {}; a \
             script outside it would also be outside open_basedir",
            manifest_dir.display()
        ));
    }
    let outside = |resolved: &Path| {
        format!(
            "route {route:?}: entrypoint php {declared:?} resolves to {}, which is outside the \
             application root {}; a script outside it would also be outside open_basedir",
            resolved.display(),
            manifest_dir.display()
        )
    };
    let root = std::fs::canonicalize(manifest_dir).map_err(|err| {
        format!(
            "route {route:?}: entrypoint php {declared:?}: the application root {} cannot be \
             resolved now: {err}",
            manifest_dir.display()
        )
    })?;
    let resolved = std::fs::canonicalize(&entry).map_err(|err| {
        format!(
            "route {route:?}: entrypoint php {declared:?} {} cannot be resolved now: {err}; a save \
             caught mid-write is never pinned, and the file must exist before it can be",
            entry.display()
        )
    })?;
    if !resolved.starts_with(&root) {
        return Err(outside(&resolved));
    }
    Ok(entry)
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
pub(crate) fn verify_entry(route: &str, entry: &Path, pin: EntryPin) -> Result<(), String> {
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::Write;

    use blastit_sys::Rlimits;
    use blastit_sys::child::{ChildIo, ChildPlan};

    use super::*;
    use crate::spawn::set_view_environment;

    /// The child test's fully qualified name, re-run by its parent.
    const CHILD: &str = "pinning::tests::the_child_sequence_keeps_a_pinned_descriptor";
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
    /// [`boot_bindings`]: the values stage 1 decides (`PhpSlot`) and stage 0's
    /// record, never the root's `Config` or the scheduler's `RouteInfo`
    /// (design §4.4 D2).
    struct Boot {
        dir: PathBuf,
        manifest: Manifest,
        /// One entry per view slot, as stage 1 hands it over.
        slots: Vec<Option<PhpSlot>>,
        /// What stage 0 recorded for the pair.
        record: PhpRuntime,
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
            // The uid a `uid_base = 20_000` layout gives its first route, and
            // the directory `dev` or `<runtime>/views/<uid>` would name: the
            // test's own value, never re-derived from the root.
            let uid = 20_000 + 1000;
            let slots = vec![Some(PhpSlot {
                route: "app".to_string(),
                uid,
                dir: dir.join("views").join(uid.to_string()),
                egress: false,
                script_name: String::new(),
                // What stage 1 decides in production; `boot_bindings` only
                // copies these into the binding.
                privileges: Privileges::DropTo(uid),
                filter: Filter::Seccomp(blastit_sys::SeccompRole::EmbeddedView { egress: false }),
            })];
            std::fs::create_dir_all(&slots[0].as_ref().expect("the slot").dir).expect("view dir");
            Self {
                dir,
                manifest,
                slots,
                record: record(&libphp, &shim, 8, 4),
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
            boot_bindings(
                &self.manifest,
                &self.slots,
                Some(&self.record),
                &self.dir,
                PhpBootLimits {
                    request_timeout: Duration::from_secs(30),
                    max_requests: u32::MAX,
                    max_body_bytes: 1024 * 1024,
                },
            )
        }
    }

    impl Drop for Boot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A `PhpRuntime` recording what `ADR-0010` §3 records for both objects.
    fn record(libphp: &Path, shim: &Path, major: u8, minor: u8) -> PhpRuntime {
        use std::os::unix::fs::MetadataExt;
        let library = std::fs::metadata(libphp).expect("libphp metadata");
        let shim = std::fs::metadata(shim).expect("shim metadata");
        PhpRuntime {
            libphp: libphp.to_path_buf(),
            dev: library.dev(),
            ino: library.ino(),
            version: crate::discovery::PhpVersion::new(major, minor),
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
            Some(&binding.view_tmp),
            boot.slots[0].as_ref().map(|slot| &slot.dir),
            "the binding's directory is the slot's, so the fork, HOME/TMPDIR and \
             sys_temp_dir cannot disagree"
        );
        assert!(
            binding.ini.contains("open_basedir=") && binding.ini.contains("sys_temp_dir="),
            "the INI lockdown is rendered once here: {}",
            binding.ini
        );
        assert_eq!(binding.max_requests, u32::MAX, "0 means no cap");
        assert_eq!(binding.route, "app", "the binding names its route");
        let slot = boot.slots[0].as_ref().expect("the PHP slot");
        assert_eq!(
            binding.uid, slot.uid,
            "and carries the uid its slot was decided with"
        );
        assert_eq!(
            Some(binding.filter),
            boot.slots[0].as_ref().map(|slot| slot.filter),
            "and the filter stage 1 decided"
        );
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
        let none = boot_bindings(
            &wsgi,
            &[None],
            Some(&boot.record),
            &boot.dir,
            PhpBootLimits {
                request_timeout: Duration::from_secs(30),
                max_requests: 1,
                max_body_bytes: 1024,
            },
        )
        .expect("nothing to do");
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

    /// `MB.40.9`: the live-reload half of the pin. A save is a new record for
    /// the **same** confined path, so [`PhpBinding::repin_entry`] re-resolves
    /// the manifest's own `entrypoint php` before it pins anything.
    ///
    /// The refused twin is the one that must never fail open: a symlink planted
    /// at the entry that points out of the manifest directory is refused with a
    /// sentence, the previous pin stays in force (so the worker keeps refusing
    /// the path), and nothing outside the root is ever recorded.
    #[test]
    fn a_repin_records_the_saved_entry_and_refuses_one_that_left_the_root() {
        let boot = Boot::new("repin");
        let mut bindings = boot.bindings().expect("the recorded pair resolves");
        let binding = bindings[0].as_mut().expect("the PHP slot has a binding");
        let first = binding.entry_pin;
        assert!(
            binding.entry_is_current(),
            "the boot's own entry is current"
        );

        // A save in place: the current check sees it, and the repin records it.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&binding.entry, b"<?php echo 'second';\n").expect("the save");
        assert!(
            !binding.entry_is_current(),
            "a rewrite in place must read as a different file before the repin"
        );
        binding
            .repin_entry(&boot.dir, "public/index.php")
            .expect("the saved entry is re-pinned");
        assert!(binding.entry_is_current());
        assert_ne!(binding.entry_pin, first, "the record moved with the file");
        assert_eq!(
            verify_entry("app", &binding.entry, binding.entry_pin),
            Ok(()),
            "the worker accepts exactly what the repin recorded"
        );

        // The refused twin: a symlink at the entry that resolves out of the root.
        let outside =
            std::env::temp_dir().join(format!("blastit-php-outside-{}", std::process::id()));
        std::fs::write(&outside, b"<?php echo 'OUTSIDE-THE-ROOT';\n").expect("the outside file");
        std::fs::remove_file(&binding.entry).expect("removing the entry");
        std::os::unix::fs::symlink(&outside, &binding.entry).expect("planting the symlink");
        let pinned = binding.entry_pin;
        let refusal = binding
            .repin_entry(&boot.dir, "public/index.php")
            .expect_err("a symlink out of the manifest directory must not be pinned");
        assert!(
            refusal.contains("outside the application root")
                && refusal.contains(&outside.display().to_string()),
            "the sentence names the target and the root: {refusal}"
        );
        assert_eq!(
            binding.entry_pin, pinned,
            "a refused repin keeps the previous pin, so the path stays refused"
        );
        assert!(
            verify_entry("app", &binding.entry, binding.entry_pin).is_err(),
            "the symlink is not the recorded file, so every request still refuses it"
        );
        let _ = std::fs::remove_file(&outside);
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
        let record = &boot.record;
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
        let record = &boot.record;
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
        let boot = Boot::new("no-record");
        let message = boot_bindings(
            &boot.manifest,
            &boot.slots,
            None,
            &boot.dir,
            PhpBootLimits {
                request_timeout: Duration::from_secs(30),
                max_requests: 1,
                max_body_bytes: 1024,
            },
        )
        .expect_err("no record, no worker");
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
        // same shape `blastit-view-python`'s marker tests use.
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
            // The accepted `HOME`/`TMPDIR` shape, seeded from outside: the
            // child then asserts the sequence leaves them exactly as it was
            // handed them — the same values `set_view_environment` writes, read
            // back by the process that will serve.
            .env("HOME", &dir)
            .env("TMPDIR", &dir)
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
            "the pinned descriptor, or the HOME/TMPDIR the child was handed, did \
             not survive the child sequence"
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
            declared_entry: "entry.php".to_string(),
            entry_pin,
            entry_fd,
            document_root: PathBuf::from("/"),
            ini: "memory_limit=256M\n".to_string(),
            view_tmp: tmp.clone(),
            max_requests: 1,
            max_body_bytes: 1024,
            route: "app".to_string(),
            uid: 0,
            script_name: String::new(),
            // The fixture's child never enters with the binding's values —
            // `ChildPlan::for_test` supplies `Shared` + `Off` from the crate
            // that owns the sequence — so these are the production *shape*: a
            // view crate may not name the two dev/check-only variants, and the
            // design's census counts every mention of them.
            privileges: Privileges::DropTo(0),
            filter: Filter::Seccomp(blastit_sys::SeccompRole::EmbeddedView { egress: false }),
        };
        let mut keep = binding.pinned_fds();
        assert_eq!(
            keep.len(),
            3,
            "libphp, the shim and the pinned entry are all descriptors"
        );
        let report = std::fs::File::create(&report_path).expect("report");
        keep.push(report.as_raw_fd());

        // The fully qualified spelling is the census the design greps (R4):
        // this is the one test-only caller of `enter`, and `ChildPlan::for_test`
        // is `Shared` + `Off` so it needs no privilege and installs no filter.
        blastit_sys::child::enter(ChildPlan::for_test(child_io(keep))).expect("the child sequence");

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

        // `HOME`/`TMPDIR` (`MB.36.5`), the **accepted** shape: a child handed
        // them reaches its first request with them intact — the sequence this
        // case just ran `(prepare`, the privilege drop, `harden_child`, the
        // filter, dumpability, the mask) touches descriptors and credentials,
        // never the environment, which is why `set_view_environment` may write
        // them *after* `enter`. Seeded by the parent, so the witness is not
        // `set_env` itself.
        let home = std::env::var("HOME").expect("HOME, seeded by the parent");
        let temp = std::env::var("TMPDIR").expect("TMPDIR, seeded by the parent");
        assert_eq!(
            home,
            tmp.display().to_string(),
            "the child sequence changed HOME"
        );
        assert_eq!(
            temp,
            tmp.display().to_string(),
            "the child sequence changed TMPDIR"
        );
        // And the **denied** shape of the safe spelling: `set_env` refuses a
        // process another thread could be reading, which a libtest binary always
        // is and a forked view child never is (`fork` clones only the calling
        // thread). The accepted write path is therefore the real boot's —
        // `critical_php_embed` serves PHP routes whose worker could only reach
        // `READY` if `HOME`/`TMPDIR` had been set, and `MB.36.7`'s e2e PHP
        // scenario re-proves it against a shipped artifact.
        let refusal = set_view_environment(&tmp)
            .expect_err("a threaded process must not have its environment rewritten");
        assert!(
            refusal.to_string().contains("setenv is not thread-safe"),
            "the refusal names why: {refusal}"
        );
        (&report)
            .write_all(format!("ok:{}:{home}:{temp}", kept.len()).as_bytes())
            .expect("write the report");
    }
}
