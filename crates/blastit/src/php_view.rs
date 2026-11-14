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
//! It does not discover `libphp` (`MB.3.13.2` resolves it and the shim at
//! stage 0/1 and hands the descriptors here) and it does not render the INI
//! policy (`IniPolicy` does, at the composition root). It forks what the
//! binding says, so the same function serves production's fd-pinned objects and
//! native development's paths.
//!
//! # Interim state: no caller, and a `[route.php]` route is not served yet
//!
//! Nothing in this crate calls [`fork_php_view`] yet. `init`'s `Ctl::Spawn`
//! still forks only [`crate::zygote`]'s WSGI path, and stage 0 does not yet
//! resolve `BLASTIT_LIBPHP`/the shim into a [`PhpBinding`] (`MB.3.13.2`).
//! **A manifest with a `[route.php]` route is accepted at boot but is not
//! served**: no PHP worker is forked, and the route's requests fail rather than
//! reaching PHP. `MB.3.13.3` owns both halves of the fix — selecting
//! `SpawnSpec::Php` in `Ctl::Spawn` and the boot refusal that names the route
//! and the package when `libphp` is missing, ZTS or the wrong minor — and
//! neither may be missing from a release that ships a PHP route. Until then the
//! module is gated `#[allow(dead_code)]` so the seam is explicit rather than a
//! warning.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};

use blastit_frame::Limits;
use blastit_php_sapi::SharedObject;
use blastit_scheduler::RouteInfo;
use blastit_sys::{
    Fork, Pid, Rlimits, SeccompRole, Signal, SignalMask, harden_child, pipe,
    restore_signal_mask_in_child, set_dumpable, socketpair_stream,
};

use crate::child::{ChildIo, child_exit, install_filter};
use crate::config::Config;

/// How many extra processes a view's uid may have beyond its workers (§7.2:
/// gVisor leaves out the main thread, so one unit of margin).
const VIEW_PROCESS_MARGIN: u32 = 16;

/// How many descriptors a PHP view may hold open (its channel, stdio, the
/// granted proxy sockets, `libphp` and its extensions).
const VIEW_NOFILE: u64 = 1024;

/// Where one shared object the child loads comes from.
///
/// Production passes the descriptor stage 0 opened and stage 1 re-verified: the
/// child keeps it open across `ChildIo::prepare`, and `dlopen` names it
/// `/proc/self/fd/<n>` so the file cannot be swapped between the verification
/// and the load. A [`PhpObject::Path`] exists for native development and the
/// harness, never as a fallback for a missing descriptor.
#[derive(Debug)]
pub enum PhpObject {
    /// An owned, already-verified descriptor.
    Fd(OwnedFd),
    /// A path (native development only).
    Path(PathBuf),
}

impl PhpObject {
    /// The raw descriptor, when this object is one.
    pub fn raw_fd(&self) -> Option<RawFd> {
        match self {
            Self::Fd(fd) => Some(fd.as_raw_fd()),
            Self::Path(_) => None,
        }
    }

    /// The handle the loader is given.
    fn shared(&self) -> SharedObject {
        match self {
            Self::Fd(fd) => SharedObject::Fd(fd.as_raw_fd()),
            Self::Path(path) => SharedObject::Path(path.clone()),
        }
    }
}

/// Everything the child needs to load and run `[route.php]`.
#[derive(Debug)]
pub struct PhpBinding {
    /// The host's NTS `libphp.so`: an fd-pinned object in production, a path
    /// for native development. Never a `dlopen` of an unverified path in
    /// `Production` (ADR-0010 §3).
    pub libphp: PhpObject,
    /// The matching `libblastit_php8X.so` shim (`MB.3.12`).
    pub shim: PhpObject,
    /// Absolute `SCRIPT_FILENAME`, pinned at boot (`[route.php] entry`).
    pub entry: PathBuf,
    /// The application root (`DOCUMENT_ROOT`, `open_basedir`).
    pub document_root: PathBuf,
    /// The `ZEND_INI_SYSTEM` blob the composition root rendered.
    pub ini: String,
    /// The view's private scratch directory: the child's `HOME`, `TMPDIR` and
    /// PHP's `sys_temp_dir` (`<runtime>/views/<slot>/tmp` in production).
    pub view_tmp: PathBuf,
    /// Requests per process before it retires.
    pub max_requests: u32,
    /// The largest request body the worker buffers (the route's `post_max_size`).
    pub max_body_bytes: usize,
}

impl PhpBinding {
    /// The descriptors the child must keep across `ChildIo::prepare`: the
    /// fd-pinned `libphp` and shim. Without them `close_fds_except` closes
    /// exactly the files the child is about to `dlopen` through
    /// `/proc/self/fd/<n>`.
    fn pinned_fds(&self) -> Vec<RawFd> {
        [&self.libphp, &self.shim]
            .into_iter()
            .filter_map(PhpObject::raw_fd)
            .collect()
    }
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
/// and has no interpreter to stop.
///
/// The configuration is validated **before** the fork, so a bad entry or an
/// empty INI is an error the parent can log rather than a child that dies on
/// its own.
///
/// # No caller yet (`MB.3.13.3`)
///
/// Nothing in this crate calls this function. `init`'s `Ctl::Spawn` still forks
/// only the WSGI path ([`crate::zygote`]), and stage 0 does not yet resolve
/// `BLASTIT_LIBPHP`/the shim into a [`PhpBinding`] (`MB.3.13.2`). The interim
/// consequence is explicit: **a `[route.php]` manifest is accepted at boot but
/// is not served** — the route's view is the WSGI fork with no application, so
/// its requests fail rather than reaching PHP. `MB.3.13.3` must add the
/// `SpawnSpec::Php` selection *and* the boot refusal (missing/ZTS/mismatched
/// `libphp` names the route and the package) before any release ships a PHP
/// route; until then this module is the seam, gated `#[allow(dead_code)]`.
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
        spawn.binding.libphp.shared(),
        spawn.binding.shim.shared(),
        config,
    )
    .map_err(|err| format!("{err}"))?;

    // SAFETY: `view_end` is this process's end of `view.chan`, created for it
    // before the fork; nothing else in this process owns the number.
    let raw = view_end.as_raw_fd();
    let chan = unsafe { std::os::unix::net::UnixStream::from_raw_fd(raw) };
    // The stream owns the descriptor from here and `serve_forever` closes it
    // when it is done, so the `OwnedFd` must not close it a second time.
    std::mem::forget(view_end);
    blastit_view::php::serve_forever(chan, std::sync::Arc::new(runtime), spawn.limits)
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

        let binding = PhpBinding {
            libphp: PhpObject::Fd(std::fs::File::open(&file).expect("pin libphp").into()),
            shim: PhpObject::Fd(std::fs::File::open(&file).expect("pin shim").into()),
            entry: PathBuf::from("/entry.php"),
            document_root: PathBuf::from("/"),
            ini: "memory_limit=256M\n".to_string(),
            view_tmp: tmp.clone(),
            max_requests: 1,
            max_body_bytes: 1024,
        };
        let mut keep = binding.pinned_fds();
        assert_eq!(keep.len(), 2, "both objects are descriptors");
        let report = std::fs::File::create(&report_path).expect("report");
        keep.push(report.as_raw_fd());

        child_io(keep).prepare().expect("the child sequence");

        // `close_fds_except` must have left the pinned descriptors alone, and
        // the loader is handed an fd (not a path) so it can name them through
        // `/proc/self/fd/<n>`.
        let raw = binding.libphp.raw_fd().expect("an fd-pinned libphp");
        let kept = std::fs::read(format!("/proc/self/fd/{raw}")).expect("/proc/self/fd/<n>");
        assert_eq!(
            kept,
            std::fs::read(&file).expect("the file"),
            "the fd moved"
        );
        assert!(matches!(binding.libphp.shared(), SharedObject::Fd(fd) if fd == raw));

        set_view_environment(&tmp).expect("HOME/TMPDIR");
        let home = std::env::var("HOME").expect("HOME");
        let temp = std::env::var("TMPDIR").expect("TMPDIR");
        (&report)
            .write_all(format!("ok:{}:{home}:{temp}", kept.len()).as_bytes())
            .expect("write the report");
    }
}
