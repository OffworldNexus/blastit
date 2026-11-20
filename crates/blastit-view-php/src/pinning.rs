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
use blastit_sys::pin::SharedObject;
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
    pub libphp: SharedObject,
    /// The matching `libblastit_php8X.so` shim.
    pub shim: SharedObject,
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
    /// `ADR-0010` §3.
    ///
    /// A `blastit dev` save replaces the file at the pinned path (in place, or
    /// by an atomic `rename`), which is exactly what `verify_entry` refuses:
    /// the record is the boot's, and only a boot may move it. A dev boot whose
    /// watcher is live is the one caller allowed to move it, and it does so by
    /// re-running the boot's declared join under `manifest_dir` with the
    /// reload's own confinement — `resolve_entry`, then `pin_entry` — so a save
    /// cannot widen what the boot decided: a `..` path, a missing file and a
    /// symlink whose target is outside the root are all `Err`, and the caller
    /// keeps the previous pin.
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

/// The `entrypoint php` as the boot recorded it (`ADR-0010` §3).
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

/// How long the test-only rendezvous may hold a boot before the wait
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

/// The deterministic window the inode-pin legs in
/// `crates/blastit/tests/e2e_prod_refusals.rs` need between stage 0's record
/// and stage 1's re-open.
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
        let entry = boot_entry(document_root, &php.entry, &values.route)?;
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
            libphp: pin_object(
                &record.libphp,
                record.dev,
                record.ino,
                &values.route,
                "libphp",
            )?,
            shim: pin_object(
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

/// Re-opens one inspected object through the shared owner
/// ([`blastit_sys::pin::SharedObject`]) and renders its refusal in this boot's
/// sentence — the route, which object, the file and the fix (`ADR-0010` §3).
///
/// The `(st_dev, st_ino)` record, the `O_RDONLY | O_CLOEXEC | O_NOFOLLOW` open
/// and the `/proc/self/fd/<fd>` spelling live in `blastit-sys` because the
/// Python `libpython` and shim pins are the same primitive (design
/// `universal-distribution.md` §1.1). The entry pin above is deliberately
/// **not** shared: PHP re-resolves `SCRIPT_FILENAME` by path on every request,
/// so it needs `st_size`/`st_ctime` and an `O_PATH` reference, while a
/// `libphp` descriptor is only ever `dlopen`ed.
fn pin_object(
    path: &Path,
    dev: u64,
    ino: u64,
    route: &str,
    what: &str,
) -> Result<SharedObject, String> {
    SharedObject::open(path, Some((dev, ino))).map_err(|err| {
        if matches!(err, blastit_sys::pin::PinError::IdentityChanged { .. }) {
            format!(
                "route {route:?}: {what} {} is not the file stage 0 inspected (st_dev/st_ino \
                 moved), so the worker would load something nobody verified; restore the package \
                 and restart blastit",
                path.display()
            )
        } else {
            format!("route {route:?}: {what}: {err}")
        }
    })
}

/// The boot's half of the entry resolution: the manifest's `entrypoint php`
/// joined under the manifest directory, refused when that join leaves it
/// **lexically**.
///
/// The boot resolves **nothing**. A deploy reaches its front controller through
/// a symlinked `public/` (or a symlinked `current/`), and `canonicalize` here
/// would make a boot that cannot start the moment a directory is swapped aside.
/// `e2e/rootless/php_entry_swap.py` restarts `blastit dev` while
/// `public -> /tmp/entry-swap-decoy` is a symlink and asserts the boot **starts**
/// and fails closed per request: PHP's `open_basedir` refuses the escaped script
/// with zero decoy opens. The confinement a reload needs lives in
/// [`resolve_entry`], which only a live watcher calls.
///
/// What the boot may not accept is a **declaration** that walks out of the root:
/// an absolute string, or one carrying a `..` component. The manifest parser
/// refuses both, and this is the same rule one layer lower, so a `Manifest`
/// built another way cannot start a pin outside `open_basedir` either.
///
/// # Errors
///
/// A sentence naming the route, the declared entry and the root.
fn boot_entry(manifest_dir: &Path, declared: &str, route: &str) -> Result<PathBuf, String> {
    let declared_path = Path::new(declared);
    let entry = manifest_dir.join(declared_path);
    let leaves = declared_path.is_absolute()
        || declared_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || !entry.starts_with(manifest_dir);
    if leaves {
        return Err(format!(
            "route {route:?}: entrypoint php {declared:?} leaves the application root {}; a \
             script outside it would also be outside open_basedir",
            manifest_dir.display()
        ));
    }
    Ok(entry)
}

/// The **live reload's** half of the entry resolution: the boot's lexical join
/// ([`boot_entry`]), plus the one confinement only a reload may add.
///
/// A boot resolves nothing; a reload *moves the pin*, so it may not move it onto
/// a file outside the root: the resolved file — symlinks followed, the way
/// `libphp` follows `SCRIPT_FILENAME` — must still be inside the resolved root.
/// A symlink pointing out of the tree is refused, so a save can never widen what
/// the boot confined.
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
    let entry = boot_entry(manifest_dir, declared, route)?;
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
/// over the entry (the inside-`manifest_dir` case) and a **rewrite in
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
