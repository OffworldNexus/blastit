#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `TESTING_PLAN.md` §6 — **fail-closed startup and runtime integrity**, end to
//! end through the real `blastit` image.
//!
//! One suite for the promise that a deployment which cannot keep its invariants
//! never serves: the caller must hold the isolation capabilities, no ancestor of
//! the tree may be writable by a uid a view runs as, the manifest must be a
//! plain file, an unknown preset is refused by the command line, an environment
//! that cannot be rewritten is refused by name (never by value), an interpreter
//! this build cannot embed is refused with its remedies, and every PHP boot
//! gate — package, `.dynsym`, minor, shim, timeout, inode — refuses before the
//! listener binds.
//!
//! Nothing here inspects a function: each case starts `blastit` and reads what an
//! operator reads, on the stream the capability gate really writes to (stage 0
//! refusals are raw text on fd 2; stage 1 refusals are `app.output` events on
//! fd 1, per `docs/knowledge/LESSONS.md`). A host that lacks a capability (root,
//! a compiler, a `libphp`) skips the leg **loudly** and never panics.
//!
//! Absorbed and deleted here: `critical_dev_and_fail_closed.rs`,
//! `no_python_build.rs`, the §6 residue of `critical_config_and_presets.rs` and
//! `smoke.rs`, and `critical_raw_ingress.rs`'s tampered-manifest re-read.

/// The host's NTS `libphp` + shim, resolved **without** the code under test, so
/// a discovery regression cannot read as "this host has no PHP".
#[cfg(feature = "php")]
#[path = "common/php_host.rs"]
mod php_host;

/// The real loopback PostgreSQL peer the dbproxy dials (§6 per-view rights).
#[cfg(feature = "python")]
#[path = "common/pg_upstream.rs"]
mod pg_upstream;

/// A **deployment** boot (root, real uid isolation) for the §6 serving legs.
#[cfg(feature = "python")]
#[path = "common/deployment.rs"]
mod deployment;

use std::fs;
#[cfg(any(feature = "python", feature = "php"))]
use std::io::BufRead;
use std::io::Read;
#[cfg(any(feature = "python", feature = "php"))]
use std::io::Write;
use std::net::{TcpListener, TcpStream};
#[cfg(any(feature = "python", feature = "php"))]
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(any(feature = "python", feature = "php"))]
use std::process::Child;
use std::process::{Command, Stdio};
#[cfg(any(feature = "python", feature = "php"))]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(any(feature = "python", feature = "php"))]
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a boot may take to bind, refuse, or answer.
const BOOT: Duration = Duration::from_secs(20);
/// The bound a manifest that is a FIFO must be refused inside: a read that
/// blocks would otherwise hang the suite instead of failing it.
const NO_HANG: Duration = Duration::from_secs(15);
/// How long a killed probe may linger in `/proc` before the reap is the failure.
#[cfg(feature = "php")]
const REAPED: Duration = Duration::from_secs(2);
/// One request's whole exchange.
#[cfg(any(feature = "python", feature = "php"))]
const REQUEST: Duration = Duration::from_secs(20);

/// A counter for unique fixture directories.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// The route every fixture declares.
#[cfg(feature = "php")]
const ROUTE: &str = "app";

/// `ADR-0010` §4.1's package half: the sentence every PHP refusal carries.
#[cfg(feature = "php")]
const PACKAGES: &str = "install the PHP embed package";

/// §10.2's production refusal for an unacknowledged `body raw` route.
#[cfg(feature = "python")]
const RAW_REFUSAL: &str = "view \"upload\": body raw hands the request body to the application \
                            unvalidated, and the production path refuses it without the \
                            acknowledgement; write `body raw unvalidated` in that view, or \
                            declare the codec it takes (json, form, none)";

// ------------------------------------------------------------------ fixtures

/// A `0755` fixture directory holding its `Blastfile`, removed on drop.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str, manifest: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "blastit-prod-{}-{tag}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a fixture directory");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("a readable fixture");
        fs::write(dir.join("Blastfile"), manifest).expect("write Blastfile");
        fs::set_permissions(dir.join("Blastfile"), fs::Permissions::from_mode(0o644))
            .expect("a readable Blastfile");
        Self { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    /// Writes `rel` under the fixture and returns its path.
    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a fixture subdirectory");
        }
        fs::write(&path, text).expect("write a fixture file");
        path
    }

    /// Writes a `0755` stand-in executable, as a WSGI host probe expects.
    #[cfg(feature = "python")]
    fn exec(&self, rel: &str) -> PathBuf {
        let path = self.write(rel, "#!/bin/sh\nexit 0\n");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("an executable");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A `Blastfile` whose one view is a WSGI route, for the Python discovery legs.
#[cfg(feature = "python")]
const WSGI_MANIFEST: &str = "\
app \"app\"
entrypoint python \"app:application\"

view app regex '.*' {}
";

/// The loader and interpreter variables the embedded CPython needs, exactly as
/// `make check` sets them; the children get `env_clear` so no ambient
/// `BLASTIT_*` can reach a case that is about the environment.
fn runtime_env(command: &mut Command) {
    for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

/// A TCP port nothing listens on right now.
fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("a probe listener")
        .local_addr()
        .expect("the probe address")
        .port()
}

/// This process's effective uid.
fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Runs `blastit` with `args` in `dir` under `env`, bounded by `bound`.
///
/// A boot that refused exits; a boot that served would run for ever, so the
/// bound is what turns "the gate never ran" into a failure instead of a hang.
fn run_bounded(
    dir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    bound: Duration,
) -> (Option<i32>, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(args)
        .env_clear()
        // A loaded host must not turn a refusal case into the watermark one.
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    runtime_env(&mut command);
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command.spawn().expect("the blastit binary starts");
    let deadline = Instant::now() + bound;
    let mut code = None;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("the child's exit status") {
            code = status.code();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if code.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    (code, stdout, stderr)
}

/// The refusal a process printed, whichever stream its stage uses, with the
/// JSON escaping of an `app.output` sentence undone (and a pretty boot's plain
/// sentence kept as it was written).
fn refusal_of(stdout: &str, stderr: &str) -> String {
    let mut parts = vec![stderr.to_owned()];
    for line in stdout.lines() {
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(event) if event["event"] == "app.output" => {
                if let Some(text) = event["line"].as_str() {
                    parts.push(text.to_owned());
                }
            }
            Ok(_) => {}
            Err(_) => parts.push(line.to_owned()),
        }
    }
    parts.join("\n")
}

/// Whether a process announced `ready` — the ordered signal, not a port probe.
fn announced_ready(stdout: &str) -> bool {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|event| event["event"] == "ready")
        || stdout
            .lines()
            .any(|line| line.split_whitespace().any(|word| word == "ready"))
}

// ------------------------------------------------------------- dev harness
// The `blastit dev` collector below is reachable from the WSGI **and** the PHP
// legs: a zero-feature build has no view to boot, so the whole harness is gated
// with them instead of failing closed on dead code.

/// A `libphp` and shim to move under a boot paused at the test rendezvous.
#[cfg(any(feature = "python", feature = "php"))]
struct Swap {
    point: PathBuf,
    from: PathBuf,
    to: PathBuf,
}

/// A running `blastit dev`, with both streams collected as they arrive.
#[cfg(any(feature = "python", feature = "php"))]
struct Boot {
    child: Child,
    port: u16,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    readers: Vec<std::thread::JoinHandle<()>>,
    swapped: Option<Arc<AtomicBool>>,
}

#[cfg(any(feature = "python", feature = "php"))]
impl Boot {
    fn start(dir: &Path, env: &[(&str, &str)]) -> Self {
        Self::launch(dir, env, None)
    }

    fn start_swapping(dir: &Path, env: &[(&str, &str)], swap: Swap) -> Self {
        Self::launch(dir, env, Some(swap))
    }

    fn launch(dir: &Path, env: &[(&str, &str)], swap: Option<Swap>) -> Self {
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string(), "--no-reload"])
            .env_clear()
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .env("BLASTIT_LOG_FORMAT", "json")
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        runtime_env(&mut command);
        for (name, value) in env {
            command.env(name, value);
        }
        if let Some(swap) = &swap {
            // Stage 0 reads this before the `execve` that strips every
            // `BLASTIT_*` and carries it in the configuration document.
            command.env("BLASTIT_TEST_SWAP_POINT", &swap.point);
        }
        let mut child = command.spawn().expect("spawning `blastit dev`");
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let mut readers = Vec::new();
        if let Some(pipe) = child.stdout.take() {
            readers.push(spawn_reader(pipe, Arc::clone(&stdout)));
        }
        if let Some(pipe) = child.stderr.take() {
            readers.push(spawn_reader(pipe, Arc::clone(&stderr)));
        }
        let swapped = swap.as_ref().map(|_| Arc::new(AtomicBool::new(false)));
        if let Some((swap, done)) = swap.zip(swapped.clone()) {
            // The rendezvous: stage 1 creates `<point>.ready` once stage 0's
            // record exists and before it re-opens either object; this side
            // renames, then writes `<point>.go`. Neither can be early, so the
            // window does not depend on how fast the runner is.
            readers.push(std::thread::spawn(move || {
                let ready = sidecar(&swap.point, "ready");
                let go = sidecar(&swap.point, "go");
                let deadline = Instant::now() + BOOT;
                while !ready.exists() {
                    if Instant::now() > deadline {
                        let _ = fs::write(&go, b"go\n");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                if fs::rename(&swap.from, &swap.to).is_ok() {
                    done.store(true, Ordering::SeqCst);
                }
                let _ = fs::write(&go, b"go\n");
            }));
        }
        Self {
            child,
            port,
            stdout,
            stderr,
            readers,
            swapped,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Waits for the boot to announce `ready` or exit.
    fn await_boot(&mut self) -> Option<Option<i32>> {
        self.await_within(BOOT)
    }

    fn await_within(&mut self, bound: Duration) -> Option<Option<i32>> {
        let deadline = Instant::now() + bound;
        loop {
            if let Some(status) = self.child.try_wait().expect("waiting for the boot") {
                return Some(status.code());
            }
            if announced_ready(&self.stdout_text()) {
                return None;
            }
            assert!(
                Instant::now() < deadline,
                "`blastit dev` neither served nor refused within {bound:?}:\n{}\n{}",
                self.stdout_text(),
                self.stderr_text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `GET <path>` over a fresh connection, as the raw response.
    fn get(&self, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connecting");
        stream
            .set_read_timeout(Some(REQUEST))
            .expect("a read timeout");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .expect("writing the request");
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        String::from_utf8_lossy(&response).into_owned()
    }

    fn events(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lock()
            .expect("the stdout buffer")
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect()
    }

    /// The `app.output` sentences a stage-1 refusal emitted on stdout.
    fn refusal(&self) -> String {
        self.events()
            .into_iter()
            .filter(|event| event["event"] == "app.output")
            .filter_map(|event| event["line"].as_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn children(&self) -> Vec<u32> {
        let path = format!("/proc/{}/task/{}/children", self.pid(), self.pid());
        fs::read_to_string(path)
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect()
    }

    fn swapped(&self) -> bool {
        self.swapped
            .as_ref()
            .is_some_and(|done| done.load(Ordering::SeqCst))
    }

    fn stdout_text(&self) -> String {
        self.stdout.lock().expect("the stdout buffer").join("\n")
    }

    fn stderr_text(&self) -> String {
        self.stderr.lock().expect("the stderr buffer").join("\n")
    }

    /// Everything a refusal case reads, either stream.
    fn all_text(&self) -> String {
        format!("{}\n{}", self.stdout_text(), self.stderr_text())
    }

    fn stop(&mut self) {
        if self.child.try_wait().expect("the boot's state").is_none() {
            let _ = Command::new("kill")
                .args(["-TERM", &self.pid().to_string()])
                .status();
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if self.child.try_wait().expect("the boot's state").is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

#[cfg(any(feature = "python", feature = "php"))]
impl Drop for Boot {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

/// Reads a child's pipe into `sink`, line by line.
#[cfg(any(feature = "python", feature = "php"))]
fn spawn_reader(
    pipe: impl Read + Send + 'static,
    sink: Arc<Mutex<Vec<String>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(pipe).lines() {
            let Ok(line) = line else { break };
            sink.lock().expect("the line buffer").push(line);
        }
    })
}

/// `<point>.<suffix>`, the two halves of the swap rendezvous.
#[cfg(any(feature = "python", feature = "php"))]
fn sidecar(point: &Path, suffix: &str) -> PathBuf {
    let mut name = point.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

/// A boot that must never have served: no `ready`, nothing on the port.
#[cfg(any(feature = "python", feature = "php"))]
fn assert_never_served(boot: &mut Boot, what: &str) {
    let events = boot.events();
    for name in ["ready", "process.spawned", "request", "runtime"] {
        assert!(
            !events.iter().any(|event| event["event"] == name),
            "{what}: a refused boot must not emit `{name}`: {}",
            boot.stdout_text()
        );
    }
    assert!(
        TcpStream::connect(("127.0.0.1", boot.port)).is_err(),
        "{what}: a refused boot left something listening on port {}",
        boot.port
    );
}

/// A one-shot production boot left `port` closed, so "it refused" is not only
/// "it printed `no ready`".
fn assert_port_closed(port: u16, what: &str) {
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "{what}: a refused boot left something listening on port {port}"
    );
}

// --------------------------------------------------------------- production

/// §6: production refuses **before serving** when the caller lacks the OS
/// isolation capabilities, and the refusal is the one that names them and the
/// fix — never the single-uid `dev` fallback.
///
/// This leg also pins the ordering the rest of the suite relies on: an unknown
/// preset is the command line's exit `2`, while a known one is accepted and
/// reaches stage 0's capability gate (exit `1`), so nothing in the preset
/// grammar can smuggle a boot past the isolation check.
#[test]
fn production_never_serves_without_isolation_privileges() {
    let dir = Scratch::new(
        "caps",
        "app \"app\"\nentrypoint python \"app:application\"\n\nview home regex '/.*' {}\n",
    );

    let unknown = run_bounded(dir.path(), &["--preset", "compose-trafik"], &[], BOOT);
    assert_eq!(unknown.0, Some(2), "{unknown:?}");
    assert!(unknown.1.is_empty(), "stdout: {}", unknown.1);
    for expected in [
        "unknown preset `compose-trafik`",
        "--preset",
        "digitalocean-paas",
        "compose-traefik",
    ] {
        assert!(
            unknown.2.contains(expected),
            "the refusal names `{expected}`: {}",
            unknown.2
        );
    }

    // A known preset reaches stage 0 on a port nothing may bind: the boot must
    // refuse there, not serve.
    let port = free_port();
    let port_text = port.to_string();
    let known = run_bounded(
        dir.path(),
        &["--preset", "digitalocean-paas"],
        &[("PORT", &port_text)],
        BOOT,
    );
    assert_eq!(known.0, Some(1), "{known:?}");
    assert!(!announced_ready(&known.1), "nothing is served: {}", known.1);
    assert_port_closed(port, "the capability refusal");
    if effective_uid() == 0 {
        println!(
            "SKIP production_never_serves_without_isolation_privileges: this process is uid 0, \
             so the unprivileged capability refusal cannot be produced here; the CI leg runs this \
             binary as a real uid"
        );
        return;
    }
    let refusal = refusal_of(&known.1, &known.2);
    for expected in [
        "init runs as uid",
        "cap_setuid",
        "cap_setgid",
        "cap_chown",
        "cap_fowner",
        "cap_kill",
        "cap_setpcap",
        "setcap",
        "blastit dev",
        "single-uid simulation, not a sandbox",
    ] {
        assert!(
            refusal.contains(expected),
            "the capability refusal names `{expected}`: {refusal}"
        );
    }
    // The `boot` event is still the boot's first line: it says it knows nothing
    // about a mode it could not read.
    let first = known
        .1
        .lines()
        .next()
        .unwrap_or_else(|| panic!("no first line: {}", known.1));
    let first: serde_json::Value = serde_json::from_str(first).expect("the first line is an event");
    assert_eq!(first["event"], "boot", "{first}");
    assert!(first.get("privilege").is_none(), "{first}");
    assert!(
        !refusal.contains(r#""privilege":"unprivileged-dev""#),
        "production never becomes dev: {refusal}"
    );
}

/// §6: the manifest must be a plain file. A FIFO with no writer would block a
/// `read_to_string` for ever, so every entry point stats before it opens — and
/// this leg's bound is the assertion that the refusal never waits.
#[test]
fn a_blastfile_that_is_a_fifo_is_refused_without_reading_it() {
    let dir = Scratch::new("fifo", "");
    let fifo = dir.path().join("Blastfile");
    fs::remove_file(&fifo).expect("remove the fixture file");
    let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).expect("a path");
    // SAFETY: `path` is a NUL-terminated path in a live buffer; `mkfifo` takes
    // a mode and touches no Rust memory.
    assert_eq!(
        unsafe { libc::mkfifo(path.as_ptr(), 0o644) },
        0,
        "mkfifo: {}",
        std::io::Error::last_os_error()
    );

    let started = Instant::now();
    let (code, stdout, stderr) = run_bounded(dir.path(), &["dev"], &[], NO_HANG);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert!(
        started.elapsed() < NO_HANG,
        "a FIFO with no writer must not be opened: {:?}",
        started.elapsed()
    );
    assert!(
        stdout.is_empty(),
        "a startup error writes nothing on stdout"
    );
    assert!(
        stderr.contains("is not a regular file") && stderr.contains("Blastfile"),
        "the refusal says what the file must be: {stderr}"
    );
    assert!(
        !stderr.contains("init runs as uid"),
        "the file is refused before any capability question: {stderr}"
    );
}

/// §6: a tree a view uid can write is refused at boot, and the refusal names the
/// directory (and the entry it would take over) with the `chmod go-w` fix.
///
/// Needs root: an unprivileged process never reaches the `Production` gate, so
/// the two writable shapes and the accepted group-class twin are root-only and
/// skip loudly elsewhere.
#[cfg(feature = "python")]
#[test]
fn a_tree_another_uid_can_write_is_refused_before_serving() {
    if effective_uid() != 0 {
        println!(
            "SKIP a_tree_another_uid_can_write_is_refused_before_serving: needs root to reach the \
             Production gate, this process is uid {}",
            effective_uid()
        );
        return;
    }
    let dir = Scratch::new(
        "writable",
        "app \"app\"\nentrypoint python \"app:application\"\n\nview home regex '/.*' {}\n",
    );
    dir.write(
        "app.py",
        "def application(environ, start_response):\n    pass\n",
    );
    let manifest = dir.path().join("Blastfile");
    let walk = |start: &Path, base: &str, mode: u32| {
        fs::set_permissions(start, fs::Permissions::from_mode(mode)).expect("a mode");
        run_bounded(
            start,
            &[],
            &[
                ("BLASTIT_APP", "app:application"),
                ("BLASTIT_MANIFEST", &manifest.to_string_lossy()),
                ("PYTHONPATH", &dir.path().to_string_lossy()),
                ("BLASTIT_UID_BASE", base),
                ("BLASTIT_LOG_FORMAT", "json"),
            ],
            BOOT,
        )
    };

    // The working directory itself, 0777 and 1777 (the sticky bit is answered,
    // not obeyed: its owner can still rename the entry below it).
    let cwd = dir.path().join("cwd");
    fs::create_dir_all(&cwd).expect("a cwd");
    for mode in [0o777, 0o1777] {
        let (code, stdout, stderr) = walk(&cwd, "48300", mode);
        let refusal = refusal_of(&stdout, &stderr);
        assert_eq!(code, Some(1), "mode {mode:o}: {refusal}");
        assert!(
            refusal.contains("is writable by another uid"),
            "mode {mode:o}: {refusal}"
        );
        assert!(
            refusal.contains(&cwd.display().to_string()),
            "mode {mode:o}: the directory is named: {refusal}"
        );
        assert!(
            refusal.contains("chmod go-w") && refusal.contains("sys.path"),
            "mode {mode:o}: the fix and the reason: {refusal}"
        );
        assert!(!announced_ready(&stdout), "mode {mode:o}: {stdout}");
        if mode == 0o1777 {
            assert!(
                refusal.contains("sticky"),
                "the sticky bit is answered: {refusal}"
            );
        }
    }

    // An ancestor: `<parent>/sub`, with `<parent>` 0777 and `sub` 0755. Nothing
    // about `sub` is wrong, but a view uid can replace it.
    let parent = dir.path().join("parent");
    let started_in = parent.join("sub");
    fs::create_dir_all(&started_in).expect("the started-in directory");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).expect("a writable parent");
    fs::set_permissions(&started_in, fs::Permissions::from_mode(0o755)).expect("a plain subdir");
    let (code, stdout, stderr) = walk(&started_in, "48700", 0o755);
    let refusal = refusal_of(&stdout, &stderr);
    assert_eq!(code, Some(1), "{refusal}");
    assert!(
        refusal.contains(&parent.display().to_string())
            && refusal.contains(&started_in.display().to_string())
            && refusal.contains("chmod go-w"),
        "the ancestor is named, with the entry it would take over: {refusal}"
    );

    // An entry only `PYTHONPATH` names is imported through exactly as much: a
    // `0777` directory there is a hole even when the working directory is fine,
    // and the refusal says where the entry comes from.
    let shared = dir.path().join("shared");
    fs::create_dir(&shared).expect("a shared directory");
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).expect("a writable shared");
    let python_path = format!("{}:{}", dir.path().display(), shared.display());
    for mode in [0o777, 0o755] {
        fs::set_permissions(&shared, fs::Permissions::from_mode(mode)).expect("a mode");
        let (code, stdout, stderr) = run_bounded(
            dir.path(),
            &[],
            &[
                ("BLASTIT_APP", "app:application"),
                ("BLASTIT_MANIFEST", &manifest.to_string_lossy()),
                ("PYTHONPATH", &python_path),
                ("BLASTIT_UID_BASE", "48600"),
                ("BLASTIT_PRELOAD", "no.such.module"),
                ("BLASTIT_LOG_FORMAT", "json"),
            ],
            BOOT,
        );
        let refusal = refusal_of(&stdout, &stderr);
        assert_eq!(code, Some(1), "mode {mode:o}: {refusal}");
        if mode == 0o777 {
            assert!(
                refusal.contains("is writable by another uid")
                    && refusal.contains(&shared.display().to_string())
                    && refusal.contains("PYTHONPATH"),
                "the PYTHONPATH entry is named with its provenance: {refusal}"
            );
        } else {
            assert!(
                !refusal.contains("is writable by another uid"),
                "0755 is not a hole: {refusal}"
            );
            assert!(
                refusal.contains("BLASTIT_PRELOAD"),
                "and the boot stopped at the step after the check: {refusal}"
            );
        }
    }

    // The accepted twin: `root:<view uid> 0750` is readable by the view through
    // its own gid, so a boot that guesses from the mode bits refuses a working
    // install. The boot is made to fail *after* the check, on a missing module.
    let base = 48_400;
    std::os::unix::fs::chown(dir.path(), Some(0), Some(base + 1000)).expect("a group");
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o750)).expect("a group mode");
    let (code, stdout, stderr) = run_bounded(
        dir.path(),
        &[],
        &[
            ("BLASTIT_APP", "app:application"),
            ("BLASTIT_MANIFEST", &manifest.to_string_lossy()),
            ("PYTHONPATH", &dir.path().to_string_lossy()),
            ("BLASTIT_UID_BASE", "48400"),
            ("BLASTIT_PRELOAD", "no.such.module"),
            ("BLASTIT_LOG_FORMAT", "json"),
        ],
        BOOT,
    );
    let refusal = refusal_of(&stdout, &stderr);
    assert_eq!(code, Some(1), "{refusal}");
    assert!(
        !refusal.contains("is not readable"),
        "the view's own group reaches it: {refusal}"
    );
    assert!(
        refusal.contains("BLASTIT_PRELOAD"),
        "and the boot stopped at the step after the check: {refusal}"
    );
}

// ---------------------------------------------------- environment & config

/// §6/§5.3: every environment refusal lands in **one** boot, one line each,
/// naming the variable and never its value.
#[test]
fn one_boot_reports_every_environment_refusal_by_name_only() {
    const SECRET: &str = "S3NT1NEL-d0-not-log-me";
    let dir = Scratch::new(
        "env-refusals",
        "\
postgres $DATABASE_URL
postgres analytics $DATABASE_URL
postgres reports $REPORTS_IDENTITY
postgres reserved $BLASTIT_DATABASE
postgres multi $MULTI_URL

static \"/\" { root \"public\" }
",
    );
    dir.write("public/index.html", "ok\n");

    let (code, stdout, stderr) = run_bounded(
        dir.path(),
        &["dev"],
        &[
            (
                "DATABASE_URL",
                &format!("postgresql://real:{SECRET}@db.example.com:5432/shop?sslmode=disable"),
            ),
            (
                "MULTI_URL",
                &format!("postgresql://real:{SECRET}@h1.example.com,h2.example.com:5432/shop"),
            ),
            ("BLASTIT_DATABASE", "postgresql://u:secret@x:5432/db"),
        ],
        BOOT,
    );
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert!(
        stdout.is_empty(),
        "a startup error writes nothing on stdout"
    );
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| line.trim_start().starts_with("- "))
        .collect();
    assert_eq!(lines.len(), 4, "one line each, one boot:\n{stderr}");
    for expected in [
        "output \"DATABASE_URL\": produced by proxy \"analytics\" and proxy \"db\"",
        "proxy \"reports\" input \"url\": environment variable \"REPORTS_IDENTITY\" is not set",
        "proxy \"reserved\" output \"BLASTIT_DATABASE\": an output must not name a BLASTIT_ variable",
        "proxy \"multi\" input \"url\": variable \"MULTI_URL\" names more than one host; \
         multi-host input is not supported",
    ] {
        assert!(
            stderr.contains(expected),
            "the boot names `{expected}`:\n{stderr}"
        );
    }
    assert!(
        !stderr.contains(SECRET),
        "a refusal never carries a value from the environment:\n{stderr}"
    );
    assert!(
        !stderr.contains("environment rewrite has 1 error"),
        "the boot reports them together, not one per attempt:\n{stderr}"
    );
}

/// §6/§5.3: `BLASTIT_*` never reaches a worker, a stripped name is gone, and an
/// injected default never overrides a value that is already set.
#[cfg(feature = "python")]
#[test]
fn the_worker_environment_drops_blastit_and_keeps_a_present_default() {
    const APP: &str = r#"
import json
import os


def application(environ, start_response):
    body = json.dumps({"env": dict(os.environ)}).encode()
    start_response("200 OK", [("Content-Type", "application/json"),
                              ("Content-Length", str(len(body)))])
    return [body]
"#;
    let dir = Scratch::new(
        "worker-env",
        "\
app \"app\"
entrypoint python \"app:application\"

postgres $DATABASE_URL

view home regex '.*' {}
",
    );
    dir.write("app.py", APP);
    let mut boot = Boot::start(
        dir.path(),
        &[
            (
                "DATABASE_URL",
                "postgresql://real:secret@127.0.0.1:5432/shop?sslmode=disable",
            ),
            ("BLASTIT_SECRET_MARKER", "must-not-reach-a-view"),
            ("STRIP_ME", "strip-me-too"),
            ("BLASTIT_ENV_STRIP", "STRIP_ME"),
            // The injected default is `1`; a value already set must survive.
            ("PYTHONDONTWRITEBYTECODE", "0"),
        ],
    );
    assert!(
        boot.await_boot().is_none(),
        "the boot must serve: {}",
        boot.stderr_text()
    );
    let response = boot.get("/");
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    let value: serde_json::Value = serde_json::from_str(&body).expect("the app's JSON body");
    let env = value["env"].as_object().expect("the worker's environment");
    assert!(
        !env.keys().any(|name| name.starts_with("BLASTIT_")),
        "a BLASTIT_* variable reached a worker: {env:?}"
    );
    assert!(
        env.get("STRIP_ME").is_none() && env.get("BLASTIT_ENV_STRIP").is_none(),
        "a stripped name reached a worker: {env:?}"
    );
    assert_eq!(
        env.get("PYTHONDONTWRITEBYTECODE").and_then(|v| v.as_str()),
        Some("0"),
        "an injected default must not overwrite a present value: {env:?}"
    );
    assert!(
        env.get("DATABASE_URL")
            .and_then(|v| v.as_str())
            .is_some_and(|url| !url.contains("secret")),
        "the real credential reached a worker: {env:?}"
    );
    boot.stop();
}

/// §4/§6: a `PORT` that is not a port is the shell's exit `1` — the real
/// process, stdout empty, and the value named on stderr — not a serve that
/// silently picks another one.
#[test]
fn a_port_that_is_not_a_port_is_a_refusal_that_names_the_value() {
    let dir = Scratch::new("bad-port", "");
    let (code, stdout, stderr) = run_bounded(dir.path(), &["dev"], &[("PORT", "70000")], BOOT);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert!(
        stdout.is_empty(),
        "a CLI failure writes nothing: {stdout:?}"
    );
    assert!(stderr.contains("PORT"), "{stderr}");
    assert!(
        stderr.contains("70000"),
        "the refusal names the value: {stderr}"
    );
}

/// §4/§5: `blastit dev` and `blastit check` never read an exported
/// `BLASTIT_PRESET`, while the production path does — so a shell that happens to
/// export a deployment's preset cannot change what a developer sees, and cannot
/// quietly disarm the seccomp gate either.
///
/// The production half of this leg is `an_environment_driven_abort_is_a_real_exit_code`
/// below; here the promise is that `dev` *starts*.
#[cfg(feature = "python")]
#[test]
fn dev_ignores_an_exported_preset_and_an_unknown_one() {
    const APP: &str = "
def application(environ, start_response):
    start_response(\"200 OK\", [(\"Content-Length\", \"0\")])
    return [b\"\"]
";
    let dir = Scratch::new("dev-preset", WSGI_MANIFEST);
    dir.write("app.py", APP);

    for (label, preset) in [
        ("a known preset with the gate off", "compose-traefik"),
        ("an unknown preset", "compose-trafik"),
    ] {
        let mut boot = Boot::start(
            dir.path(),
            &[("BLASTIT_PRESET", preset), ("BLASTIT_SECCOMP", "0")],
        );
        assert!(
            boot.await_boot().is_none(),
            "{label}: `blastit dev` must serve, not read the variable: {}",
            boot.all_text()
        );
        assert!(
            boot.get("/").contains("200 OK"),
            "{label}: it answers: {}",
            boot.stdout_text()
        );
        boot.stop();
    }
}

/// §4: an empty directory names the **missing manifest** before it blames the
/// app setting; a manifest that exists but declares no `app` is the setting's
/// own refusal.
#[cfg(feature = "python")]
#[test]
fn an_empty_directory_names_the_missing_manifest_before_the_app() {
    let dir = Scratch::new("no-manifest", "");
    fs::remove_file(dir.path().join("Blastfile")).expect("the empty fixture has no manifest");

    let (code, stdout, stderr) = run_bounded(dir.path(), &["dev"], &[], BOOT);
    assert_eq!(code, Some(1), "{stdout:?} {stderr:?}");
    assert!(
        stdout.is_empty(),
        "a startup error writes nothing: {stdout:?}"
    );
    assert!(stderr.contains("./Blastfile"), "{stderr}");
    assert!(
        !stderr.contains("BLASTIT_APP"),
        "the file is missing, not the app: {stderr}"
    );

    fs::write(dir.path().join("Blastfile"), "view home GET \"/\" {}\n")
        .expect("a manifest with no app");
    let (code, _, stderr) = run_bounded(dir.path(), &["dev"], &[], BOOT);
    assert_eq!(code, Some(1), "{stderr:?}");
    assert!(stderr.contains("BLASTIT_APP"), "{stderr}");
    assert!(
        !stderr.contains("No such file"),
        "the manifest exists, so nothing is missing: {stderr}"
    );
}

/// §6: a `BLASTIT_SECCOMP=0` next to a preset never reaches a running server —
/// and where the boot gets far enough to read the configuration, it says which
/// variable it refused rather than dying on an `EPERM` nobody can place.
#[cfg(feature = "python")]
#[test]
fn an_environment_driven_abort_is_a_real_exit_code_and_names_its_cause() {
    let dir = Scratch::new("seccomp", WSGI_MANIFEST);
    dir.write(
        "app.py",
        "
def application(environ, start_response):
    start_response(\"200 OK\", [(\"Content-Length\", \"0\")])
    return [b\"\"]
",
    );
    let (code, stdout, stderr) = run_bounded(
        dir.path(),
        &[],
        &[
            ("BLASTIT_PRESET", "compose-traefik"),
            ("BLASTIT_SECCOMP", "0"),
        ],
        BOOT,
    );
    assert_eq!(code, Some(1), "{stdout:?} {stderr:?}");
    let combined = format!("{stdout}{stderr}");
    assert!(
        !combined.contains(r#""event":"ready""#),
        "the boot aborts before serving: {combined}"
    );
    if combined.contains(r#""stage":"config""#) {
        assert!(
            combined.contains("BLASTIT_SECCOMP"),
            "the abort names the gate: {combined}"
        );
    }
}

// ------------------------------------------------------------ WSGI host

/// §6/§2: the WSGI host's refusals, verbatim: an unsupported minor, a
/// free-threaded build, and a prefix without a shared `libpython` (which names
/// the minor and every remedy).
#[cfg(feature = "python")]
#[test]
fn an_unusable_cpython_is_refused_with_its_documented_sentences() {
    let dir = Scratch::new("py-refusals", WSGI_MANIFEST);
    dir.write(
        "app.py",
        "def application(environ, start_response):\n    pass\n",
    );
    let run = |value: &str| {
        run_bounded(
            dir.path(),
            &["dev", "--no-reload"],
            &[("BLASTIT_PYTHON", value)],
            BOOT,
        )
    };

    let old = dir.exec("py312/bin/python3.12");
    let (code, stdout, stderr) = run(&old.to_string_lossy());
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    for expected in [
        "blastit dev: CPython 3.12 at",
        "is not supported for WSGI views",
        "CPython 3.13, 3.14, 3.15 and 3.16 are supported",
        "run uv python install 3.16 && uv venv --python 3.16",
    ] {
        assert!(
            stderr.contains(expected),
            "the refusal names `{expected}`: {stderr}"
        );
    }

    let free = dir.exec("pyt/bin/python3.14t");
    let (_, _, stderr) = run(&free.to_string_lossy());
    for expected in [
        "CPython 3.14t at",
        "is free-threaded and cannot serve WSGI views",
        "uv python install cpython-3.16",
    ] {
        assert!(
            stderr.contains(expected),
            "the refusal names `{expected}`: {stderr}"
        );
    }

    // The missing-`libpython` sentence: one supported minor this host does not
    // ship, chosen from the discovery's own list so a host that ships both skips
    // loudly instead of failing a boot that would find one.
    let minor_number = |minor: &str| -> u32 {
        minor
            .rsplit_once('.')
            .and_then(|(_, digits)| digits.parse().ok())
            .expect("a dotted minor")
    };
    let Some(minor) = ["3.14", "3.13"]
        .into_iter()
        .find(|minor| !blastit_py_sapi::discovery::system_has_libpython(minor_number(minor)))
    else {
        println!(
            "SKIP the missing-libpython leg: this host ships a shared libpython for 3.14 and 3.13 \
             in {:?}, so no prefix fixture can make the boot refuse",
            blastit_py_sapi::discovery::system_libpython_directories()
        );
        return;
    };
    let prefix = dir.path().join("prefix");
    let interpreter = dir.exec(&format!("prefix/bin/python{minor}"));
    dir.write(
        ".venv/pyvenv.cfg",
        &format!(
            "home = {}\nversion = {minor}.0\ninclude-system-site-packages = false\n",
            prefix.join("bin").display()
        ),
    );
    dir.write(".venv/bin/python3", "");
    let _ = fs::remove_file(dir.path().join(".venv/bin/python3"));
    std::os::unix::fs::symlink(&interpreter, dir.path().join(".venv/bin/python3"))
        .expect("the venv interpreter");
    fs::create_dir_all(
        dir.path()
            .join(format!(".venv/lib/python{minor}/site-packages")),
    )
    .expect("the venv site-packages");
    let (code, stdout, stderr) = run("");
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    let head = format!(
        "blastit dev: CPython {minor} is installed at {}, but this host has no shared libpython \
         for CPython {minor}; install it",
        prefix.display()
    );
    assert!(
        stderr.contains(&head),
        "the refusal names the minor and the prefix `{head}`: {stderr}"
    );
    for remedy in [
        format!("apt install libpython{minor}"),
        "apk add python3".to_owned(),
        "dnf install python3-libs".to_owned(),
        format!("uv python install {minor}"),
    ] {
        assert!(
            stderr.contains(&remedy),
            "the refusal names the remedy `{remedy}`: {stderr}"
        );
    }
}

// ---------------------------------------------- §6 per-view containment (WSGI)

/// The WSGI app the containment legs serve: it connects through the route's own
/// PostgreSQL socket and reports the errno, so an ungranted route's missing
/// socket and an `egress = false` view's `EPERM` are measured, not inferred.
#[cfg(feature = "python")]
const PG_APP: &str = r#"
import json
import os
import socket
import struct


def _read_exact(sock, count):
    body = b""
    while len(body) < count:
        chunk = sock.recv(count - len(body))
        if not chunk:
            raise OSError(0, "the server closed the session")
        body += chunk
    return body


def _ready(sock):
    while True:
        header = _read_exact(sock, 5)
        length = struct.unpack("!I", header[1:])[0]
        body = _read_exact(sock, length - 4)
        if header[:1] == b"E":
            raise RuntimeError("pg: " + body.decode("utf-8", "replace"))
        if header[:1] == b"Z":
            return


def pg(sql):
    url = os.environ["DATABASE_URL"]
    query = url.split("?", 1)[1] if "?" in url else ""
    params = {}
    for part in query.split("&"):
        if "=" in part:
            name, value = part.split("=", 1)
            params[name] = value
    path = os.path.join(params.get("host", "/proc/self/cwd"),
                        ".s.PGSQL." + params.get("port", "5432"))
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(10)
    sock.connect(path)
    startup = struct.pack("!I", 196608)
    for name, value in (("user", "blastit"), ("database", "app")):
        startup += name.encode() + b"\0" + value.encode() + b"\0"
    startup += b"\0"
    sock.sendall(struct.pack("!I", len(startup) + 4) + startup)
    _ready(sock)
    body = sql.encode() + b"\0"
    sock.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
    _ready(sock)
    sock.close()


def pg_state(sql):
    try:
        pg(sql)
        return {"ok": True}
    except RuntimeError as err:
        fields = {}
        for part in str(err).split("\x00"):
            if len(part) > 1:
                fields[part[0]] = part[1:]
        return {"ok": False, "sqlstate": fields.get("C")}


def application(environ, start_response):
    path = environ.get("PATH_INFO", "/")
    try:
        if path.endswith("/inet"):
            try:
                probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                probe.close()
                payload = {"ok": True}
            except OSError as err:
                payload = {"errno": err.errno}
        elif path.endswith("/state"):
            payload = pg_state("SELECT 1")
        else:
            try:
                pg("SELECT 1")
                payload = {"ok": True}
            except OSError as err:
                payload = {"errno": err.errno}
        body = json.dumps(payload).encode()
        status = "200 OK"
    except Exception as err:  # noqa: BLE001 - the body is the diagnosis
        body = json.dumps({"error": str(err)}).encode()
        status = "500 Internal Server Error"
    start_response(status, [("Content-Type", "application/json"),
                            ("Content-Length", str(len(body)))])
    return [body]
"#;

/// A manifest with one `postgres` store and the routes a containment leg needs.
#[cfg(feature = "python")]
fn pg_manifest(routes: &str) -> String {
    format!(
        "app \"app\"\nentrypoint python \"app:application\"\n\npostgres $DATABASE_URL\n\n{routes}"
    )
}

/// The `DATABASE_URL` a leg hands the child: a real-looking credential for a
/// `PgUpstream` that answers trust authentication, TLS off because it is loopback.
#[cfg(feature = "python")]
fn database_url(server: &pg_upstream::PgUpstream) -> String {
    format!(
        "postgresql://real-user:real-secret@127.0.0.1:{}/shop?sslmode=disable",
        server.port()
    )
}

/// The JSON body of a served response.
#[cfg(feature = "python")]
fn json_body(response: &str) -> serde_json::Value {
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    serde_json::from_str(&body).unwrap_or_else(|err| panic!("{err}: {body}"))
}

/// The ephemeral root a `dev` boot created, from `/tmp`.
#[cfg(feature = "python")]
fn dev_root(pid: u32) -> PathBuf {
    let prefix = format!("blastit-dev-{}-{pid}-", effective_uid());
    fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .unwrap_or_else(|| panic!("no ephemeral root for pid {pid}"))
}

/// §6: only a route with a grant gets a PostgreSQL socket; the ungranted route
/// gets `ENOENT`, and both the socket (`0600`) and the route's directory
/// (`0700`) are private to it.
#[cfg(feature = "python")]
#[test]
fn only_a_granted_route_gets_a_pg_socket() {
    let server = pg_upstream::PgUpstream::start();
    let url = database_url(&server);
    let dir = Scratch::new(
        "pg-sockets",
        &pg_manifest(
            "\
view granted regex '/granted/.*' {
    postgres {
        query 'SELECT 1'
    }
}

view ungranted regex '/ungranted/.*' {}
",
        ),
    );
    dir.write("app.py", PG_APP);
    let mut boot = Boot::start(dir.path(), &[("DATABASE_URL", &url)]);
    assert!(
        boot.await_boot().is_none(),
        "the boot must serve: {}",
        boot.all_text()
    );

    assert_eq!(json_body(&boot.get("/granted/x"))["ok"], true);
    assert!(
        server
            .statements()
            .iter()
            .any(|statement| statement.contains("SELECT 1")),
        "the granted statement reached the upstream: {:?}",
        server.statements()
    );
    assert!(
        server
            .startups()
            .iter()
            .any(|startup| startup.user == "real-user"),
        "dbproxy authenticated with the proxy's real user: {:?}",
        server.startups()
    );
    let refused = json_body(&boot.get("/ungranted/x"));
    assert_eq!(
        refused["errno"].as_i64(),
        Some(i64::from(libc::ENOENT)),
        "an ungranted route has no socket: {refused}\n{}",
        boot.all_text()
    );

    let root = dev_root(boot.pid());
    let granted = root.join("views/0/.s.PGSQL.5432");
    let ungranted = root.join("views/1/.s.PGSQL.5432");
    assert_eq!(
        fs::metadata(&granted)
            .expect("the granted route's socket")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "the socket is 0600"
    );
    assert_eq!(
        fs::metadata(root.join("views/1"))
            .expect("the route directory")
            .permissions()
            .mode()
            & 0o777,
        0o700,
        "each route directory is private"
    );
    assert_eq!(
        fs::metadata(&ungranted).err().map(|err| err.kind()),
        Some(std::io::ErrorKind::NotFound),
        "an ungranted route must have no socket: {ungranted:?}"
    );
    boot.stop();
}

/// §6: `postgres {}` gives the route a listener that denies every statement, so
/// the app reaches PostgreSQL's `42501` instead of a missing socket — and the
/// granted twin still passes the same statement.
#[cfg(feature = "python")]
#[test]
fn an_empty_grant_gets_a_denying_listener() {
    let server = pg_upstream::PgUpstream::start();
    let url = database_url(&server);
    let dir = Scratch::new(
        "pg-denying",
        &pg_manifest(
            "\
view locked regex '/locked/.*' {
    postgres {}
}

view open regex '/open/.*' {
    postgres {
        query 'SELECT 1'
    }
}
",
        ),
    );
    dir.write("app.py", PG_APP);
    let mut boot = Boot::start(dir.path(), &[("DATABASE_URL", &url)]);
    assert!(
        boot.await_boot().is_none(),
        "the boot must serve: {}",
        boot.all_text()
    );

    let denied = json_body(&boot.get("/locked/state"));
    assert_eq!(
        denied["sqlstate"].as_str(),
        Some("42501"),
        "an empty grant denies with SQLSTATE 42501: {denied}\n{}",
        boot.all_text()
    );
    assert_eq!(
        json_body(&boot.get("/open/state"))["ok"],
        true,
        "the granted route still passes `SELECT 1`:\n{}",
        boot.all_text()
    );
    assert_eq!(
        fs::metadata(dev_root(boot.pid()).join("views/0/.s.PGSQL.5432"))
            .expect("the empty grant's socket exists")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "the denying listener is 0600"
    );
    boot.stop();
}

/// §6: `egress = false` refuses `socket(AF_INET)` with `EPERM`/`ENOSYS`, and the
/// `egress any` twin may open one.
#[cfg(feature = "python")]
#[test]
fn an_egress_false_view_cannot_open_inet() {
    let server = pg_upstream::PgUpstream::start();
    let url = database_url(&server);
    let dir = Scratch::new(
        "egress",
        &pg_manifest(
            "\
view closed regex '/closed/.*' {}

view open regex '/open/.*' {
    egress any
}
",
        ),
    );
    dir.write("app.py", PG_APP);
    let mut boot = Boot::start(dir.path(), &[("DATABASE_URL", &url)]);
    assert!(
        boot.await_boot().is_none(),
        "the boot must serve: {}",
        boot.all_text()
    );

    let closed = json_body(&boot.get("/closed/inet"));
    let errno = closed["errno"].as_i64().unwrap_or(0);
    assert!(
        errno == i64::from(libc::EPERM) || errno == i64::from(libc::ENOSYS),
        "an egress = false view must get EPERM or ENOSYS, got {closed}\n{}",
        boot.all_text()
    );
    assert_eq!(
        json_body(&boot.get("/open/inet"))["ok"],
        true,
        "an egress = true view may create AF_INET sockets"
    );
    boot.stop();
}

/// §6/`R22`: a JSON boot never carries a `port` key, at any depth — the address
/// is the operator's setting, not something the event stream re-publishes.
#[cfg(feature = "python")]
#[test]
fn json_events_never_carry_a_port_key() {
    fn assert_no_port_key(value: &serde_json::Value, raw: &str) {
        match value {
            serde_json::Value::Object(map) => {
                assert!(
                    !map.contains_key("port"),
                    "a log event must never carry a `port` key: {raw}"
                );
                for child in map.values() {
                    assert_no_port_key(child, raw);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    assert_no_port_key(item, raw);
                }
            }
            _ => {}
        }
    }

    let dir = Scratch::new("no-port", WSGI_MANIFEST);
    dir.write(
        "app.py",
        "def application(environ, start_response):\n    pass\n",
    );
    let mut boot = Boot::start(dir.path(), &[]);
    assert!(
        boot.await_boot().is_none(),
        "the boot must serve: {}",
        boot.all_text()
    );
    let lines = boot.stdout_text();
    let mut events = 0;
    for line in lines.lines() {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
            events += 1;
            assert_no_port_key(&event, line);
        }
    }
    assert!(events > 0, "the boot wrote no events: {lines}");
    boot.stop();
}

/// §6: a `dev` launch whose port is taken refuses by naming the bind failure —
/// the classifier the deleted suite's retry loop was built on, now pinned
/// against the real binary instead of the harness.
#[cfg(feature = "python")]
#[test]
fn a_dev_boot_on_an_occupied_port_names_the_bind_failure() {
    let held = TcpListener::bind(("127.0.0.1", 0)).expect("an occupied port");
    let port = held.local_addr().expect("the address").port();
    let dir = Scratch::new("occupied-port", WSGI_MANIFEST);
    dir.write(
        "app.py",
        "def application(environ, start_response):\n    pass\n",
    );
    let (code, stdout, stderr) = run_bounded(
        dir.path(),
        &["dev", "--port", &port.to_string(), "--no-reload"],
        &[],
        BOOT,
    );
    drop(held);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    let all = format!("{stdout}\n{stderr}");
    assert!(
        all.contains(&format!(":{port}:")) && all.contains("Address already in use"),
        "the refusal names the port and the reason: {all}"
    );
    assert!(!announced_ready(&stdout), "nothing is served: {stdout}");
}

// -------------------------------------------- §6 deployment: per-view identity

/// The app every deployment leg serves: it reports the identity and the
/// environment a **view worker** really got.
#[cfg(feature = "python")]
const DEPLOYED_APP: &str = r#"
import json
import os


def application(environ, start_response):
    body = json.dumps({
        "uid": os.getuid(),
        "pid": os.getpid(),
        "gid": os.getgid(),
        "route": environ.get("blastit.route"),
        "home": os.environ.get("HOME"),
        "tmpdir": os.environ.get("TMPDIR"),
        "cwd": os.getcwd(),
        "blastit": sorted(k for k in os.environ if k.startswith("BLASTIT_")),
        "remote_addr": environ.get("REMOTE_ADDR"),
        "remote_port": environ.get("REMOTE_PORT"),
    }).encode()
    start_response("200 OK", [("Content-Type", "application/json"),
                              ("Content-Length", str(len(body)))])
    return [body]
"#;

/// Two views, so "one route, one identity" can be observed at all.
#[cfg(feature = "python")]
const TWO_VIEWS: &str = "\
view one \"/\", \"/one\" {}

view two regex '/two/.*' {}
";

/// The uid base every deployment leg uses; the views are `base + 1000`, `+1001`.
#[cfg(feature = "python")]
const DEPLOY_UID_BASE: u32 = 52_000;

/// Starts a deployment boot, or returns `None` when this host cannot host one.
#[cfg(feature = "python")]
fn deployed(tag: &str, args: &[&str], env: &[(&str, &str)]) -> Option<deployment::Deployment> {
    deployment::Deployment::start(
        tag,
        DEPLOY_UID_BASE,
        TWO_VIEWS,
        &[("app.py", DEPLOYED_APP)],
        env,
        args,
    )
}

/// The JSON body a deployment view answered with.
#[cfg(feature = "python")]
fn deployed_body(response: &deployment::Response) -> serde_json::Value {
    assert_eq!(response.status, 200, "{}", response.body);
    serde_json::from_str(&response.body).expect("the view's JSON body")
}

/// §6: every view runs under its **own** OS user identity, in its own scratch
/// directory, with no `BLASTIT_*` variable and the client's address and port as
/// two CGI keys — the deployment path, as root, on the real binary.
#[cfg(feature = "python")]
#[test]
fn a_deployment_runs_each_route_in_its_own_process_and_uid() {
    let Some(deployment) = deployed("uids", &[], &[]) else {
        return;
    };
    let one = DEPLOY_UID_BASE + 1000;
    let two = DEPLOY_UID_BASE + 1001;

    let first = deployed_body(&deployment.get("/one"));
    assert_eq!(first["uid"], one, "{first}");
    assert_eq!(first["gid"], one, "gid == uid, no groups: {first}");
    assert_eq!(first["route"], "one", "{first}");
    assert_eq!(
        first["home"].as_str().map(|path| path.to_owned()),
        Some(format!("/run/blastit/views/{one}")),
        "{first}"
    );
    assert_eq!(first["tmpdir"], first["home"], "{first}");
    assert_eq!(
        first["cwd"], first["home"],
        "its own socket directory: {first}"
    );
    assert_eq!(
        first["blastit"].as_array().map(Vec::len),
        Some(0),
        "no BLASTIT_* survives the rewrite: {first}"
    );
    assert_eq!(first["remote_addr"], "127.0.0.1", "{first}");
    assert!(
        first["remote_port"]
            .as_str()
            .is_some_and(|port| port.parse::<u16>().is_ok_and(|port| port > 0)),
        "the connection's port, as a string: {first}"
    );

    let second = deployed_body(&deployment.get("/two/anything"));
    assert_eq!(second["uid"], two, "{second}");
    assert_eq!(second["route"], "two", "{second}");
    assert_ne!(
        first["pid"], second["pid"],
        "one route, one process: {first} {second}"
    );
    let again = deployed_body(&deployment.get("/one"));
    assert_eq!(
        again["pid"], first["pid"],
        "the same route keeps its process"
    );

    // Both view identities are live processes of init, and both are in the
    // uid's own table — not the harness's.
    for uid in [one, two] {
        assert!(
            deployment.component_uid(uid).is_some(),
            "no live process runs as uid {uid}: {}",
            deployment.stdout_text()
        );
    }

    // §6: the boot says which mode it ran in and, with no preset, that there is
    // none in force — a rootless boot and a root one look identical otherwise.
    let boots = deployment.events("boot");
    assert!(boots.len() >= 2, "stage 0 and config: {boots:?}");
    for boot in &boots {
        assert_eq!(boot["privilege"], "root", "{boot}");
        assert!(boot["preset"].is_null(), "{boot}");
        assert!(
            boot.get("port").is_none(),
            "a boot never carries a port: {boot}"
        );
    }
}

/// §6: a preset is reported by the boot (what it supplied, what the environment
/// overrode) and it decides **which** forwarded client header is believed — the
/// other one is ignored, even when the request sends it.
#[cfg(feature = "python")]
#[test]
fn a_deployment_preset_is_reported_and_decides_which_forwarded_header_is_trusted() {
    for (preset, believed, ignored) in [
        ("digitalocean-paas", "DO-Connecting-IP", "X-Forwarded-For"),
        ("compose-traefik", "X-Forwarded-For", "DO-Connecting-IP"),
    ] {
        let Some(deployment) = deployed(
            "preset",
            &["--preset", preset],
            &[("BLASTIT_LOG_FORMAT", "json")],
        ) else {
            return;
        };
        let boots = deployment.events("boot");
        let boot = boots.first().expect("a boot event");
        assert_eq!(boot["privilege"], "root", "{boot}");
        assert_eq!(boot["preset"], preset, "{boot}");
        let settings = boot["preset_settings"]
            .as_object()
            .expect("what the preset supplied");
        assert_eq!(settings.len(), 4, "{preset}: {settings:?}");
        let overridden = boot["preset_overridden"].as_array().expect("the overrides");
        assert!(
            overridden.iter().any(|name| name == "BLASTIT_LOG_FORMAT"),
            "the harness's own format won over the preset's: {preset}: {overridden:?}"
        );

        let trusted = deployed_body(&deployment.request_with(
            "/one",
            &[(believed, "198.51.100.7"), (ignored, "203.0.113.9")],
        ));
        assert_eq!(
            trusted["remote_addr"], "198.51.100.7",
            "{preset} believes {believed} and not {ignored}: {trusted}"
        );

        // The untrusted header alone is ignored: the peer's own address wins.
        let untrusted =
            deployed_body(&deployment.request_with("/one", &[(ignored, "203.0.113.9")]));
        assert_eq!(
            untrusted["remote_addr"], "127.0.0.1",
            "{preset} must ignore {ignored}: {untrusted}"
        );
    }
}

/// §6: `serve` writes JSON events by default — one line per event, each with
/// `ts`/`level`/`component` — and `BLASTIT_LOG_FORMAT=pretty` renders a
/// forwarded event as the human line instead.
#[cfg(feature = "python")]
#[test]
fn a_deployment_writes_json_events_unless_the_format_says_pretty() {
    let Some(json) = deployed("format-json", &[], &[]) else {
        return;
    };
    let value = deployed_body(&json.get("/one"));
    assert_eq!(value["uid"], DEPLOY_UID_BASE + 1000, "{value}");
    for line in json.stdout_lines() {
        let event: serde_json::Value = serde_json::from_str(&line)
            .unwrap_or_else(|err| panic!("a serve boot writes JSON: {err}: {line}"));
        assert_eq!(
            event["ts"].as_str().map(str::len),
            Some(24),
            "one timestamp shape: {line}"
        );
        assert!(event["level"].is_string(), "{line}");
        assert!(event["component"].is_string(), "{line}");
        assert!(line.len() <= 2048, "{} bytes: {line}", line.len());
    }
    let names: Vec<String> = json
        .events("boot")
        .iter()
        .filter_map(|event| event["stage"].as_str().map(str::to_owned))
        .collect();
    assert!(names.iter().any(|stage| stage == "preload"), "{names:?}");
    assert!(
        !json.events("ready").is_empty(),
        "the lifecycle reaches `ready`:\n{}",
        json.stdout_text()
    );

    let Some(pretty) = deployed("format-pretty", &[], &[("BLASTIT_LOG_FORMAT", "pretty")]) else {
        return;
    };
    deployed_body(&pretty.get("/one"));
    assert!(
        pretty.wait_for_event(" request "),
        "no request line:\n{}",
        pretty.stdout_text()
    );
    let log = pretty.stdout_text();
    let request = log
        .lines()
        .find(|line| line.contains(" request "))
        .expect("the request line");
    assert!(
        !request.starts_with('{') && !request.contains("\"event\":"),
        "one human line, not a JSON object: {request}"
    );
    assert!(
        request.contains("→ "),
        "the route reaches the line: {request}"
    );
    assert!(
        log.lines().filter(|line| line.starts_with('{')).count() <= 2,
        "only stage 0's first lines may be JSON:\n{log}"
    );
}

// ------------------------------------------------------------------- PHP

/// `ADR-0010` §2: a PHP file is its own executor and needs no `BLASTIT_APP`.
#[cfg(feature = "php")]
const PHP_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

/// The script every PHP fixture serves.
#[cfg(feature = "php")]
const NORMAL_PHP: &str = "\
<?php
header('Content-Type: text/plain');
echo \"blastit-php-ok\\n\";
";

/// A `libphp` stub: an NTS build exports `executor_globals` and `php_version`;
/// `BT_ZTS` adds the one symbol a thread-safe build must be refused for.
#[cfg(feature = "php")]
const LIBPHP_STUB_C: &str = r#"
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

__attribute__((constructor)) static void bt_libphp_canary(void)
{
    int fd = open(BT_CANARY, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd >= 0) {
        char line[96];
        int n = snprintf(line, sizeof line, "libphp pid=%d ppid=%d\n", (int)getpid(), (int)getppid());
        if (n > 0) { ssize_t ignored = write(fd, line, (size_t)n); (void)ignored; }
        close(fd);
    }
}

int executor_globals = 0;
#ifdef BT_ZTS
int tsrm_startup(void) { return 0; }
#endif
const char *php_version(void) { return BT_VERSION; }
"#;

/// A shim: the five symbols a worker resolves, with `BT_MINOR` the ABI minor it
/// reports, so a mismatch is `BT_MINOR` disagreeing with `php_version()`.
#[cfg(feature = "php")]
const SHIM_STUB_C: &str = r#"
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

__attribute__((constructor)) static void bt_shim_canary(void)
{
    int fd = open(BT_CANARY, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd >= 0) {
        char line[96];
        int n = snprintf(line, sizeof line, "shim pid=%d ppid=%d\n", (int)getpid(), (int)getppid());
        if (n > 0) { ssize_t ignored = write(fd, line, (size_t)n); (void)ignored; }
        close(fd);
    }
}

unsigned blastit_php_abi_version(void) { return 1u; }
unsigned blastit_php_minor(void) { return BT_MINOR; }
int blastit_php_worker_init(const void *config) { (void)config; return 0; }
int blastit_php_request(const void *request, const void *sink, void *out)
{
    (void)request; (void)sink; (void)out;
    return -1;
}
void blastit_php_worker_shutdown(void) {}
"#;

/// A shim that reports its containment and then never returns, so the stage-1
/// probe's `BLASTIT_PROBE_TIMEOUT_MS` is the only way past it.
#[cfg(feature = "php")]
const HANG_SHIM_STUB_C: &str = r#"
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static void bt_note(const char *line)
{
    int fd = open(BT_SHAPE, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0600);
    if (fd >= 0) {
        ssize_t ignored = write(fd, line, strlen(line));
        (void)ignored;
        close(fd);
    }
}

unsigned blastit_php_abi_version(void) { return 1u; }
unsigned blastit_php_minor(void) { return 804; }

int blastit_php_worker_init(const void *config)
{
    (void)config;
    int inet = socket(AF_INET, SOCK_STREAM, 0);
    int refused = inet < 0 ? errno : 0;
    if (inet >= 0) { close(inet); }
    char line[192];
    int n = snprintf(line, sizeof line,
                     "probe pid=%d ppid=%d pgid=%d uid=%d euid=%d inet=%d\n",
                     (int)getpid(), (int)getppid(), (int)getpgid(0),
                     (int)getuid(), (int)geteuid(), refused);
    if (n > 0) { bt_note(line); }
    for (;;) { pause(); }
}

int blastit_php_request(const void *request, const void *sink, void *out)
{
    (void)request; (void)sink; (void)out;
    return -1;
}
void blastit_php_worker_shutdown(void) {}
"#;

/// `cc` on `PATH`, or `None` so a PHP leg can skip loudly.
#[cfg(feature = "php")]
fn compiler() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join("cc"))
        .find(|path| path.is_file())
}

/// Compiles `source` into `dir/<file_name>` with `-D` defines; panics on a
/// compiler error because the stub is the fixture.
#[cfg(feature = "php")]
fn compile(dir: &Path, file_name: &str, text: &str, defines: &[(&str, String)]) -> PathBuf {
    let cc = compiler().expect("a C compiler");
    let source = dir.join(format!("{file_name}.c"));
    let out = dir.join(file_name);
    fs::write(&source, text).expect("writing the stub source");
    let mut command = Command::new(cc);
    command
        .args(["-shared", "-fPIC", "-o"])
        .arg(&out)
        .arg(&source);
    for (key, value) in defines {
        command.arg(format!("-D{key}={value}"));
    }
    let status = command.status().expect("running the compiler");
    assert!(status.success(), "compiling {file_name} failed: {status}");
    out
}

/// A `-D` string literal.
#[cfg(feature = "php")]
fn quote(value: &str) -> String {
    format!("\"{value}\"")
}

/// The shim's `libblastit_php<N><N>-<arch>-<libc>.so` spelling.
#[cfg(feature = "php")]
fn shim_name(minor: u32) -> String {
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    format!("libblastit_php{minor}-{}-{libc}.so", std::env::consts::ARCH)
}

/// The canary lines a stub wrote, as `(tag, pid, ppid)`.
#[cfg(feature = "php")]
fn canaries(path: &Path) -> Vec<(String, u32, u32)> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let tag = fields.next()?.to_owned();
            let pid = fields.next()?.strip_prefix("pid=")?.parse().ok()?;
            let ppid = fields.next()?.strip_prefix("ppid=")?.parse().ok()?;
            Some((tag, pid, ppid))
        })
        .collect()
}

/// `<route>: ...` — the route and `entry` every PHP refusal names.
#[cfg(feature = "php")]
fn assert_php_refusal(text: &str, what: &str) {
    assert!(
        text.contains("route \"app\""),
        "{what}: the route is named: {text}"
    );
    assert!(
        text.contains("entrypoint php \"public/index.php\""),
        "{what}: the entry is named: {text}"
    );
}

/// One backend, one sentence: a refusal never names a transport Blastit deleted.
#[cfg(feature = "php")]
fn assert_no_fallback(text: &str, what: &str) {
    let lower = text.to_ascii_lowercase();
    for banned in ["php-fpm", "fastcgi", "php-cgi"] {
        assert!(
            !lower.contains(banned),
            "{what}: the refusal names a fallback recipe ({banned}): {text}"
        );
    }
}

/// A PHP boot that must refuse at **stage 0** (raw text on fd 2, nothing on
/// stdout) with the package sentence.
#[cfg(feature = "php")]
fn assert_stage0_refusal(code: Option<i32>, stdout: &str, stderr: &str, what: &str) {
    assert_eq!(code, Some(1), "{what}: stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "{what}: a stage-0 refusal writes nothing on stdout"
    );
    assert_php_refusal(stderr, what);
    assert!(
        stderr.contains(PACKAGES) && stderr.contains("BLASTIT_LIBPHP"),
        "{what}: the package and the override are the fix: {stderr}"
    );
    assert_no_fallback(stderr, what);
}

/// §6/`ADR-0010` §3: a `BLASTIT_LIBPHP` that is not a shared object is refused
/// from its bytes, and the sentence names the package that provides one.
#[cfg(feature = "php")]
#[test]
fn a_libphp_that_is_not_a_shared_object_names_the_package() {
    let dir = Scratch::new("php-not-elf", PHP_MANIFEST);
    dir.write("public/index.php", NORMAL_PHP);
    let text = dir.write("libphp8.4.so", "this is not an ELF shared object");
    let (code, stdout, stderr) = run_bounded(
        dir.path(),
        &["dev"],
        &[("BLASTIT_LIBPHP", &text.to_string_lossy())],
        BOOT,
    );
    assert_stage0_refusal(code, &stdout, &stderr, "not an ELF object");
    assert!(
        stderr.contains("is not an ELF shared object")
            && stderr.contains(&text.display().to_string()),
        "the refusal says what the file is, and names it: {stderr}"
    );
}

/// §6/`ADR-0010` §3: a ZTS build is refused from `.dynsym` alone — the stub is
/// loadable, so the empty canary is the zero-`dlopen` proof.
#[cfg(feature = "php")]
#[test]
fn a_zts_libphp_is_refused_from_dynsym_without_a_dlopen() {
    if compiler().is_none() {
        println!("SKIP a_zts_libphp_is_refused_from_dynsym_without_a_dlopen: no `cc` on PATH");
        return;
    }
    let dir = Scratch::new("php-zts", PHP_MANIFEST);
    dir.write("public/index.php", NORMAL_PHP);
    let canary = dir.path().join("zts.canary");
    let source = format!(
        "#define BT_CANARY {}\n#define BT_VERSION {}\n{}",
        quote(&canary.display().to_string()),
        quote("8.4.27"),
        LIBPHP_STUB_C
    );
    let libphp = compile(
        dir.path(),
        "libphp8.4.so",
        &source,
        &[("BT_ZTS", "1".to_string())],
    );
    let (code, stdout, stderr) = run_bounded(
        dir.path(),
        &["dev"],
        &[("BLASTIT_LIBPHP", &libphp.to_string_lossy())],
        BOOT,
    );
    assert_stage0_refusal(code, &stdout, &stderr, "ZTS libphp");
    assert!(
        stderr.contains("is a ZTS (thread-safe) libphp"),
        "the refusal names the ABI and the fix: {stderr}"
    );
    assert!(
        canaries(&canary).is_empty(),
        "stage 0 loaded the object it refused: {:?}",
        canaries(&canary)
    );
}

/// §6/`ADR-0010` §4.1: a minor this build ships no shim for is refused with the
/// support sentence naming 8.3–8.6 — not with a missing-symbol story.
#[cfg(feature = "php")]
#[test]
fn an_unsupported_php_minor_names_the_supported_set() {
    if compiler().is_none() {
        println!("SKIP an_unsupported_php_minor_names_the_supported_set: no `cc` on PATH");
        return;
    }
    let dir = Scratch::new("php-minor", PHP_MANIFEST);
    dir.write("public/index.php", NORMAL_PHP);
    let canary = dir.path().join("libphp82.canary");
    let source = format!(
        "#define BT_CANARY {}\n#define BT_VERSION {}\n{}",
        quote(&canary.display().to_string()),
        quote("8.2.29"),
        LIBPHP_STUB_C
    );
    let libphp = compile(dir.path(), "libphp8.2.so", &source, &[]);
    let (code, stdout, stderr) = run_bounded(
        dir.path(),
        &["dev"],
        &[("BLASTIT_LIBPHP", &libphp.to_string_lossy())],
        BOOT,
    );
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(stdout, "", "a stage-0 refusal writes nothing on stdout");
    assert_php_refusal(&stderr, "unsupported minor");
    assert!(
        stderr.contains("PHP 8.2 is not supported — Blastit supports PHP 8.3, 8.4, 8.5 and 8.6"),
        "the refusal is the support sentence: {stderr}"
    );
    assert!(
        !stderr.contains("exports no `php_version`"),
        "an unsupported minor is not a missing symbol: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("php-fpm"),
        "the refusal names no fallback: {stderr}"
    );
}

/// §6/`ADR-0010` §3: a shim built for another minor refuses the boot **before**
/// the listener binds; the load happens only below the fork.
#[cfg(feature = "php")]
#[test]
fn a_shim_for_another_minor_refuses_before_the_listener() {
    if compiler().is_none() {
        println!("SKIP a_shim_for_another_minor_refuses_before_the_listener: no `cc` on PATH");
        return;
    }
    let dir = Scratch::new("php-shim", PHP_MANIFEST);
    dir.write("public/index.php", NORMAL_PHP);
    let libphp_canary = dir.path().join("libphp.canary");
    let shim_canary = dir.path().join("shim.canary");
    let libphp_source = format!(
        "#define BT_CANARY {}\n#define BT_VERSION {}\n{}",
        quote(&libphp_canary.display().to_string()),
        quote("8.4.27"),
        LIBPHP_STUB_C
    );
    let shim_source = format!(
        "#define BT_CANARY {}\n{}",
        quote(&shim_canary.display().to_string()),
        SHIM_STUB_C
    );
    let libphp = compile(dir.path(), "libphp8.4.so", &libphp_source, &[]);
    let shim = compile(
        dir.path(),
        &shim_name(84),
        &shim_source,
        &[("BT_MINOR", "803".to_string())],
    );
    assert_eq!(shim.file_name().unwrap(), shim_name(84).as_str());

    let mut boot = Boot::start(
        dir.path(),
        &[
            ("BLASTIT_LIBPHP", &libphp.to_string_lossy()),
            ("BLASTIT_PHP_SHIM_DIR", &dir.path().to_string_lossy()),
        ],
    );
    let init = boot.pid();
    let code = boot
        .await_boot()
        .expect("a shim of another minor must refuse the boot, not serve a route");
    boot.stop();
    let all = boot.all_text();
    assert_eq!(
        code,
        Some(1),
        "stdout={}\nstderr={}",
        boot.stdout_text(),
        boot.stderr_text()
    );
    let refusal = boot.refusal();
    assert_php_refusal(&refusal, "shim minor");
    assert_no_fallback(&refusal, "shim minor");
    for expected in [
        "the shim is built for PHP 8.3 but the host's libphp reports 8.4.27",
        "libblastit_php83",
        "(PHP 8.4, ",
    ] {
        assert!(
            all.contains(expected),
            "the refusal names `{expected}`: {all}"
        );
    }
    assert_never_served(&mut boot, "shim minor");
    let writers: Vec<(String, u32, u32)> = canaries(&libphp_canary)
        .into_iter()
        .chain(canaries(&shim_canary))
        .collect();
    assert!(
        !writers.is_empty(),
        "no stub was loaded at all, so nothing proves the load happened in a view: {all}"
    );
    for (tag, pid, ppid) in &writers {
        assert_ne!(*pid, init, "{tag} was mapped by the boot itself: {all}");
        assert_eq!(
            *ppid, init,
            "{tag} was mapped by pid {pid}, whose parent is {ppid}, not the boot ({init}): {all}"
        );
    }
}

/// §6/`ADR-0010` §3: `BLASTIT_LIBPHP` must be absolute, or the library the boot
/// inspected and the one a worker loads could differ.
#[cfg(feature = "php")]
#[test]
fn a_relative_blastit_libphp_is_refused() {
    let dir = Scratch::new("php-relative", PHP_MANIFEST);
    dir.write("public/index.php", NORMAL_PHP);
    dir.write("libphp8.4.so", "not an ELF object");
    let (code, stdout, stderr) = run_bounded(
        dir.path(),
        &["dev"],
        &[("BLASTIT_LIBPHP", "./libphp8.4.so")],
        BOOT,
    );
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(stdout, "", "a stage-0 refusal writes nothing on stdout");
    assert!(
        stderr.contains("BLASTIT_LIBPHP") && stderr.contains("must be an absolute path"),
        "the refusal names the variable and the rule: {stderr}"
    );
    assert!(
        stderr.contains("libphp.so"),
        "and what the path must point at: {stderr}"
    );
}

/// §6/`ADR-0010` §3: a probe child whose `worker_init` never returns is killed at
/// `BLASTIT_PROBE_TIMEOUT_MS` — and the shape it wrote proves the containment
/// the timeout killed (own process group, its uid, `AF_INET` `EPERM`), with the
/// child reaped rather than parked.
#[cfg(feature = "php")]
#[test]
fn a_probe_that_hangs_is_killed_by_its_timeout_and_reaped() {
    const TIMEOUT_MS: u32 = 2_000;
    if compiler().is_none() {
        println!("SKIP a_probe_that_hangs_is_killed_by_its_timeout_and_reaped: no `cc` on PATH");
        return;
    }
    let dir = Scratch::new("php-hang", PHP_MANIFEST);
    dir.write("public/index.php", NORMAL_PHP);
    let shape = dir.path().join("shape");
    let libphp_source = format!(
        "#define BT_CANARY {}\n#define BT_VERSION {}\n{}",
        quote(&dir.path().join("libphp.canary").display().to_string()),
        quote("8.4.27"),
        LIBPHP_STUB_C
    );
    let shim_source = format!(
        "#define BT_SHAPE {}\n{}",
        quote(&shape.display().to_string()),
        HANG_SHIM_STUB_C
    );
    let libphp = compile(dir.path(), "libphp8.4.so", &libphp_source, &[]);
    compile(
        dir.path(),
        &shim_name(84),
        &shim_source,
        &[("BT_MINOR", "804".to_string())],
    );
    let route_uid = fs::metadata(dir.path()).expect("the fixture").uid();

    let started = Instant::now();
    let mut boot = Boot::start(
        dir.path(),
        &[
            ("BLASTIT_LIBPHP", &libphp.to_string_lossy()),
            ("BLASTIT_PHP_SHIM_DIR", &dir.path().to_string_lossy()),
            ("BLASTIT_PROBE_TIMEOUT_MS", &TIMEOUT_MS.to_string()),
        ],
    );
    let init = boot.pid();
    let code = boot
        .await_within(Duration::from_secs(30))
        .expect("a probe that never returns must refuse the boot, not hang it");
    let elapsed = started.elapsed();
    boot.stop();
    let all = boot.all_text();
    assert_eq!(code, Some(1), "{all}");
    assert!(
        elapsed < Duration::from_secs(20),
        "the {TIMEOUT_MS} ms timeout did not bound the boot: {elapsed:?}\n{all}"
    );
    let refusal = boot.refusal();
    assert_php_refusal(&refusal, "probe timeout");
    assert!(
        all.contains(&format!("{TIMEOUT_MS} ms")) && all.contains("BLASTIT_PROBE_TIMEOUT_MS"),
        "the refusal names the timeout it applied and the knob: {all}"
    );
    assert_never_served(&mut boot, "probe timeout");

    let text = fs::read_to_string(&shape).unwrap_or_else(|err| {
        panic!(
            "no probe shape was written ({}): {err}\n{all}",
            shape.display()
        )
    });
    let line = text
        .lines()
        .find(|line| line.starts_with("probe "))
        .unwrap_or_else(|| panic!("no `probe` line in the shape file: {text}"));
    let field = |name: &str| -> u32 {
        line.split_whitespace()
            .find_map(|part| part.strip_prefix(&format!("{name}=")))
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("no `{name}` field in `{line}`"))
    };
    let pid = field("pid");
    assert_ne!(pid, init, "the probe is a fork, not the boot: {line}");
    assert_eq!(field("ppid"), init, "the probe is init's child: {line}");
    assert_eq!(
        field("pgid"),
        pid,
        "the probe leads its own process group, which is what `kill(-pgid)` names: {line}"
    );
    assert_eq!(
        field("uid"),
        route_uid,
        "the probe runs as the route's uid: {line}"
    );
    assert_eq!(field("euid"), route_uid, "and owns nothing more: {line}");
    assert_eq!(
        field("inet"),
        1,
        "`EmbeddedView` must answer `socket(AF_INET)` with EPERM before `worker_init`: {line}"
    );
    let deadline = Instant::now() + REAPED;
    while Path::new(&format!("/proc/{pid}")).exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "the boot refused but its probe child (pid {pid}) is still parked:\n{all}"
    );
}

/// §6/`ADR-0010` §3: a `libphp` or shim renamed between stage 0's record and
/// stage 1's re-open is refused by the inode pin, before the listener binds.
#[cfg(feature = "php")]
#[test]
fn a_libphp_or_shim_replaced_between_the_stages_is_refused() {
    if compiler().is_none() {
        println!("SKIP a_libphp_or_shim_replaced_between_the_stages_is_refused: no `cc` on PATH");
        return;
    }
    let source = format!(
        "#define BT_CANARY {}\n#define BT_VERSION {}\n{}",
        quote(""),
        quote("8.4.27"),
        LIBPHP_STUB_C
    );
    // The shim's canary path is the same empty string: a loaded stub would try
    // to open `""` and fail, which is harmless — this leg asserts the pin, not
    // the load.
    let shim_source = format!("#define BT_CANARY {}\n{}", quote(""), SHIM_STUB_C);

    for what in ["libphp", "shim"] {
        let dir = Scratch::new(&format!("php-swap-{what}"), PHP_MANIFEST);
        dir.write("public/index.php", NORMAL_PHP);
        let libphp = compile(dir.path(), "libphp8.4.so", &source, &[]);
        let shim = compile(
            dir.path(),
            &shim_name(84),
            &shim_source,
            &[("BT_MINOR", "804".to_string())],
        );
        let (recorded, from) = if what == "libphp" {
            (libphp.clone(), dir.path().join("spare-libphp.so"))
        } else {
            (shim.clone(), dir.path().join("spare-shim.so"))
        };
        fs::copy(&recorded, &from).expect("a byte-identical spare");

        let mut boot = Boot::start_swapping(
            dir.path(),
            &[
                ("BLASTIT_LIBPHP", &libphp.to_string_lossy()),
                ("BLASTIT_PHP_SHIM_DIR", &dir.path().to_string_lossy()),
            ],
            Swap {
                point: dir.path().join("swap-point"),
                from,
                to: recorded.clone(),
            },
        );
        let code = boot.await_boot();
        let swapped = boot.swapped();
        boot.stop();
        let all = boot.all_text();
        assert!(
            swapped,
            "{what}: stage 1 never reached the swap point, so nothing was renamed — the seam is a \
             rendezvous, not a timing window:\n{all}"
        );
        assert_eq!(
            code,
            Some(Some(1)),
            "{what}: a boot that served here means stage 1 no longer compares the re-opened \
             object against stage 0's record — the regression this test exists to catch:\n{all}"
        );
        let pin = all
            .lines()
            .find(|line| line.contains("is not the file stage 0 inspected"))
            .unwrap_or_else(|| panic!("{what}: not the inode pin's sentence:\n{all}"));
        assert!(
            pin.contains(&recorded.display().to_string()),
            "{what}: the refusal names the file that moved: {pin}"
        );
        if what == "shim" {
            assert!(
                pin.contains("shim ") && !pin.contains(&libphp.display().to_string()),
                "{what}: the refusal is the shim's comparison, not the library's: {pin}"
            );
        }
        assert!(
            pin.contains("restore the package and restart blastit"),
            "{what}: and the fix: {pin}"
        );
        assert!(!announced_ready(&boot.stdout_text()), "{what}: {all}");
        assert_never_served(&mut boot, what);
    }
}

/// §6/`R11`: with a real pair the route answers, and no process above the view
/// maps `libphp` or its shim.
#[cfg(feature = "php")]
#[test]
fn no_libphp_is_mapped_above_the_view_worker() {
    let Some(host) = php_host::host_php() else {
        println!(
            "SKIP no_libphp_is_mapped_above_the_view_worker: this host ships no usable NTS libphp \
             and no shim, so no route can be served"
        );
        return;
    };
    let dir = Scratch::new("php-maps", PHP_MANIFEST);
    dir.write("public/index.php", NORMAL_PHP);
    let mut boot = Boot::start(
        dir.path(),
        &[
            ("BLASTIT_LIBPHP", &host.libphp.to_string_lossy()),
            (
                "BLASTIT_PHP_SHIM_DIR",
                &host
                    .shim
                    .parent()
                    .expect("a shim directory")
                    .to_string_lossy(),
            ),
        ],
    );
    assert!(
        boot.await_boot().is_none(),
        "this host has {} and {}, so the route must be served: {}",
        host.libphp.display(),
        host.shim.display(),
        boot.all_text()
    );
    let response = boot.get("/");
    assert!(
        response.starts_with("HTTP/1.1 200") && response.contains("blastit-php-ok"),
        "the embedded worker did not answer: {response}"
    );
    let init = boot.pid();
    let maps = fs::read_to_string(format!("/proc/{init}/maps")).expect("init's maps");
    assert!(
        !maps.contains("libphp") && !maps.contains("libblastit_php"),
        "init mapped a PHP image; the load belongs to the worker alone:\n{maps}"
    );
    let views: Vec<u32> = boot
        .events()
        .into_iter()
        .filter(|event| event["event"] == "process.spawned" && event["route"] == ROUTE)
        .filter_map(|event| event["pid"].as_u64())
        .map(|pid| pid as u32)
        .collect();
    let above: Vec<u32> = boot
        .children()
        .into_iter()
        .filter(|pid| !views.contains(pid))
        .collect();
    assert!(
        !above.is_empty(),
        "no role above the view among init's children, so this test would assert nothing"
    );
    let mut denied = 0;
    for pid in &above {
        match fs::read_to_string(format!("/proc/{pid}/maps")) {
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => denied += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!("reading /proc/{pid}/maps: {err}"),
            Ok(text) => assert!(
                !text.contains("libphp") && !text.contains("libblastit_php"),
                "/proc/{pid}/maps (a role above the view) holds a PHP image:\n{text}"
            ),
        }
    }
    if effective_uid() == 0 {
        // Root reads every map, so each role was content-checked or gone; the
        // assert above is the witness.
    } else {
        assert!(
            denied > 0,
            "the forked roles must keep PR_SET_DUMPABLE(0): {denied} denied of {}",
            above.len()
        );
    }
    boot.stop();
}

/// §6: there is no PHP server beside the library — no `php-fpm`, no FastCGI
/// socket, no second listener. The route is served, so a process tree that
/// carries one would be the fallback the architecture forbids.
#[cfg(feature = "php")]
#[test]
fn no_php_server_stands_between_blastit_and_the_library() {
    let Some(host) = php_host::host_php() else {
        println!(
            "SKIP no_php_server_stands_between_blastit_and_the_library: this host ships no usable \
             NTS libphp and no shim"
        );
        return;
    };
    let dir = Scratch::new("php-beside", PHP_MANIFEST);
    dir.write("public/index.php", NORMAL_PHP);
    let mut boot = Boot::start(
        dir.path(),
        &[
            ("BLASTIT_LIBPHP", &host.libphp.to_string_lossy()),
            (
                "BLASTIT_PHP_SHIM_DIR",
                &host
                    .shim
                    .parent()
                    .expect("a shim directory")
                    .to_string_lossy(),
            ),
        ],
    );
    assert!(
        boot.await_boot().is_none(),
        "the route must be served: {}",
        boot.all_text()
    );
    assert!(
        boot.get("/").starts_with("HTTP/1.1 200"),
        "the worker answers"
    );

    // The real socket tables of the forked tree: every socket descriptor the
    // tree holds is matched, by inode, against the network namespace's TCP and
    // Unix tables (`/proc/<pid>/net/{tcp,unix}`). A CGI/FastCGI server would be
    // a second listener (TCP) or a named Unix socket in that set.
    //
    // A forked role keeps `PR_SET_DUMPABLE(0)`, so its descriptor table is
    // `EACCES` to an unprivileged reader: init's own table is the witness there,
    // and the root job (which runs this suite with `--nocapture`) sees every
    // role. Without even init's table the leg skips loudly rather than assert
    // nothing.
    let init = boot.pid();
    let mut fds = socket_inodes(init);
    if fds.is_empty() {
        println!(
            "SKIP no_php_server_stands_between_blastit_and_the_library: init's socket table is not \
             readable to uid {} (a non-dumpable role needs the CI root job), so no socket could be \
             attributed",
            effective_uid()
        );
        boot.stop();
        return;
    }
    let mut tree = vec![init];
    let mut index = 0;
    while index < tree.len() {
        let pid = tree[index];
        index += 1;
        fds.extend(socket_inodes(pid));
        if let Ok(text) = fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")) {
            tree.extend(
                text.split_whitespace()
                    .filter_map(|pid| pid.parse::<u32>().ok()),
            );
        }
    }
    let listeners = tcp_listeners(init, &fds);
    if listeners.is_empty() {
        // The listener is held by the forked `http` role, whose table is
        // `EACCES` unprivileged; the CI root job sees it and asserts below.
        println!(
            "SKIP (listener half) no_php_server_stands_between_blastit_and_the_library: the http \
             listener is held by a non-dumpable forked role, unreadable to uid {}",
            effective_uid()
        );
    } else {
        assert_eq!(
            listeners.len(),
            1,
            "the tree listens on more than the http port: {listeners:?} (tree {tree:?})"
        );
        assert_eq!(
            listeners[0], boot.port,
            "the only listener in the tree is the http port"
        );
    }
    let named: Vec<String> = unix_paths(init, &fds);
    for path in &named {
        let lower = path.to_ascii_lowercase();
        assert!(
            !lower.contains("php") && !lower.contains("fastcgi") && !lower.contains("cgi"),
            "a FastCGI socket stands between Blastit and PHP: {path}"
        );
    }
    boot.stop();
}

/// The socket inodes `pid` holds, from its descriptor table.
#[cfg(feature = "php")]
fn socket_inodes(pid: u32) -> Vec<u64> {
    let mut inodes = Vec::new();
    for entry in fs::read_dir(format!("/proc/{pid}/fd"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let Ok(target) = fs::read_link(entry.path()) else {
            continue;
        };
        let text = target.to_string_lossy().into_owned();
        if let Some(inode) = text
            .strip_prefix("socket:[")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|digits| digits.parse().ok())
        {
            inodes.push(inode);
        }
    }
    inodes
}

/// The local ports (`LISTEN`, state `0A`) of the sockets in `wanted`, from the
/// network namespace's `/proc/<pid>/net/tcp`.
#[cfg(feature = "php")]
fn tcp_listeners(pid: u32, wanted: &[u64]) -> Vec<u16> {
    let table = fs::read_to_string(format!("/proc/{pid}/net/tcp")).unwrap_or_default();
    let mut ports = Vec::new();
    for line in table.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 || fields[3] != "0A" {
            continue;
        }
        let inode: u64 = fields[9].parse().unwrap_or(0);
        if !wanted.contains(&inode) {
            continue;
        }
        if let Some((_, port)) = fields[1].rsplit_once(':')
            && let Ok(port) = u16::from_str_radix(port, 16)
        {
            ports.push(port);
        }
    }
    ports
}

/// The path of every named Unix socket in `wanted`, from
/// `/proc/<pid>/net/unix` (unnamed channels have no path and are ignored).
#[cfg(feature = "php")]
fn unix_paths(pid: u32, wanted: &[u64]) -> Vec<String> {
    let table = fs::read_to_string(format!("/proc/{pid}/net/unix")).unwrap_or_default();
    let mut paths = Vec::new();
    for line in table.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }
        let inode: u64 = fields[6].parse().unwrap_or(0);
        if wanted.contains(&inode) {
            paths.push(fields[7..].join(" "));
        }
    }
    paths
}

// ------------------------------------------------- production stage 1 re-read

/// The configuration document stage 0 hands stage 1: a path and no record of
/// what stage 0 approved.
#[cfg(feature = "python")]
fn config_document(dir: &Path, port: u16) -> Vec<u8> {
    let config = blastit::config::Config {
        manifest: dir.join("Blastfile"),
        manifest_dir: dir.to_path_buf(),
        port,
        server_port: port,
        log_format: blastit::config::LogFormat::Json,
        ..blastit::config::Config::default()
    };
    serde_json::to_vec(&config.to_json()).expect("the configuration document")
}

/// Runs `blastit --stage=1 --config-fd=N` over `document`.
#[cfg(feature = "python")]
fn stage_one(dir: &Path, document: &[u8]) -> (Option<i32>, String, String) {
    let path = dir.join("stage1-config.json");
    fs::write(&path, document).expect("write the configuration document");
    let file = fs::File::open(&path).expect("open the configuration document");
    let fd = {
        use std::os::fd::AsRawFd;
        file.as_raw_fd()
    };
    // The child inherits this descriptor at this number, exactly as stage 0
    // hands stage 1 its pipe.
    rustix::io::fcntl_setfd(&file, rustix::io::FdFlags::empty()).expect("clear FD_CLOEXEC");
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .arg("--stage=1")
        .arg(format!("--config-fd={fd}"))
        .env_clear()
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    runtime_env(&mut command);
    let result = run_spawned(command, BOOT);
    drop(file);
    result
}

/// Runs a prepared command with the same bound `run_bounded` applies.
#[cfg(feature = "python")]
fn run_spawned(mut command: Command, bound: Duration) -> (Option<i32>, String, String) {
    let mut child = command.spawn().expect("the blastit binary starts");
    let deadline = Instant::now() + bound;
    let mut code = None;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("the child's exit status") {
            code = status.code();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if code.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    (code, stdout, stderr)
}

/// A raw-body manifest with the acknowledgement as the caller wrote it.
#[cfg(feature = "python")]
fn raw_manifest(acknowledgement: &str) -> String {
    format!(
        "app \"app\"\nentrypoint python \"app:application\"\n\nview upload POST \"/upload\" {{\n    \
         body raw{acknowledgement}\n}}\n"
    )
}

/// §6: production stage 1 (the process that binds and forks) refuses a
/// unacknowledged `body raw` route, in the format it was configured with.
#[cfg(feature = "python")]
#[test]
fn production_refuses_a_raw_route_without_the_acknowledgement() {
    for format in [
        blastit::config::LogFormat::Json,
        blastit::config::LogFormat::Pretty,
    ] {
        let dir = Scratch::new("raw", &raw_manifest(""));
        let port = free_port();
        let config = blastit::config::Config {
            manifest: dir.path().join("Blastfile"),
            manifest_dir: dir.path().to_path_buf(),
            port,
            server_port: port,
            log_format: format,
            ..blastit::config::Config::default()
        };
        let document = serde_json::to_vec(&config.to_json()).expect("the document");
        let (code, stdout, stderr) = stage_one(dir.path(), &document);
        assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
        let refusal = refusal_of(&stdout, &stderr);
        assert!(
            refusal.contains(RAW_REFUSAL),
            "the refusal is §10.2's sentence: {refusal}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            refusal.contains("view \"upload\"") && refusal.contains("body raw unvalidated"),
            "the refusal names the view and the node that fixes it: {refusal}"
        );
        assert!(
            !announced_ready(&stdout),
            "a refused boot never announces `ready`: {stdout}"
        );
        assert_port_closed(port, "the unacknowledged raw route");
    }
}

/// §6: a manifest rewritten between stage 0's approval and stage 1's re-read is
/// refused with the same sentence — the document carries a path, not a digest,
/// so stage 1 must re-apply the gate.
#[cfg(feature = "python")]
#[test]
fn a_manifest_edited_after_stage_zero_approved_it_is_still_refused() {
    let dir = Scratch::new("toctou", &raw_manifest(" unvalidated\n"));
    let port = free_port();
    let document = config_document(dir.path(), port);
    let text = String::from_utf8_lossy(&document).into_owned();
    assert!(
        text.contains(&*dir.path().join("Blastfile").to_string_lossy()),
        "the document carries the manifest's path: {text}"
    );
    assert!(
        !text.contains("unvalidated") && !text.contains("body raw"),
        "the document carries no record of what stage 0 approved: {text}"
    );
    // The edit, between the stages: the acknowledgement is gone.
    fs::write(dir.path().join("Blastfile"), raw_manifest("")).expect("rewrite Blastfile");

    let (code, stdout, stderr) = stage_one(dir.path(), &document);
    assert_eq!(
        code,
        Some(1),
        "the re-read refuses the edited manifest\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let refusal = refusal_of(&stdout, &stderr);
    assert!(
        refusal.contains(RAW_REFUSAL),
        "the re-read refuses with the same sentence: {refusal}"
    );
    assert!(
        !announced_ready(&stdout),
        "the edited manifest never reaches a serving boot on {port}: {stdout}"
    );
    assert_port_closed(port, "the tampered manifest re-read");
}

// ------------------------------------------------------- build without python

/// A build without the `python` feature refuses a WSGI manifest in stage 0,
/// naming the artifact that *can* serve it — never `app_rule`'s `BLASTIT_APP`
/// advice, which no operator of that binary can act on.
#[cfg(not(feature = "python"))]
#[test]
fn a_wsgi_manifest_is_refused_by_the_capability_gate() {
    let capability = "this build has no Python support (built without feature \"python\")";
    let install = "install the Python package: uv add blastit";
    let app_rule = "BLASTIT_APP: the WSGI callable (`module:attribute`) is required";

    let dir = Scratch::new("no-python", "view web regex '.*' {}\n");
    let (code, stdout, stderr) = run_bounded(dir.path(), &["dev"], &[], BOOT);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(stdout, "", "a startup error writes nothing on stdout");
    assert!(
        stderr.contains("view \"web\"") && stderr.contains(capability) && stderr.contains(install),
        "the gate's own sentence and the fix: {stderr}"
    );
    assert!(
        stderr.contains("`static` mount") && stderr.contains("`entrypoint php`"),
        "the fix names the file's own executors: {stderr}"
    );
    assert!(
        !stderr.contains(app_rule),
        "the capability gate answered: {stderr}"
    );
}

/// A PHP-only manifest is **not** a WSGI manifest: the interpreter gate must
/// stay silent, and the boot is refused by the PHP discovery gate for the
/// library this build does not carry — never by `app_rule`, never by
/// installing Python.
#[cfg(all(not(feature = "python"), feature = "php"))]
#[test]
fn a_php_only_manifest_is_refused_by_the_php_discovery_gate() {
    let capability = "this build has no Python support (built without feature \"python\")";
    let install = "install the Python package: uv add blastit";
    let app_rule = "BLASTIT_APP: the WSGI callable (`module:attribute`) is required";
    let php = Scratch::new(
        "php-only",
        "entrypoint php \"public/index.php\"\n\nview app regex '.*' {}\n",
    );
    php.write("public/index.php", "<?php echo \"ok\\n\";");
    let not_a_library = php.write("libphp.so", "not an ELF object");
    let (code, stdout, stderr) = run_bounded(
        php.path(),
        &["dev"],
        &[("BLASTIT_LIBPHP", &not_a_library.to_string_lossy())],
        BOOT,
    );
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(stdout, "", "a startup error writes nothing on stdout");
    assert!(
        stderr.contains("route \"app\"") && stderr.contains("entrypoint php \"public/index.php\""),
        "the discovery gate names the route and its entry: {stderr}"
    );
    let all = format!("{stdout}{stderr}");
    assert!(
        !all.contains(capability) && !all.contains(install),
        "a PHP route needs no interpreter: {all}"
    );
    assert!(
        !all.contains(app_rule),
        "a PHP route is its own executor: {all}"
    );
    assert!(
        stderr.contains("install the PHP embed package"),
        "the discovery gate answered instead: {stderr}"
    );
}
