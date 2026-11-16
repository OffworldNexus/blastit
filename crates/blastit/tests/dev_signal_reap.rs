//! Standard-tier regression for `blastit dev` reaping its whole tree (`MB.15`).
//!
//! The contract: after `blastit dev` exits, nothing of it is still listening and
//! nothing it forked is still running. `MB.14` reported the opposite, and the
//! report was an artifact of two tooling traps rather than of the binary:
//!
//! 1. `blastit dev` `execve`s **in place** into `--stage=1`
//!    (`stage0::exec_stage1_dev`): the pid a caller starts is the same process as
//!    init, and its command line no longer says `dev`. A probe that signals
//!    whatever `pgrep -f 'blastit dev --port N'` finds therefore finds *nothing*
//!    after boot, and one whose `$!` was a shell wrapper signals the wrapper.
//! 2. A `pgrep -f` run through `bash -c "… <pattern> …"` matches the shell that
//!    carries the pattern in its own `argv`, which reads as an orphan that never
//!    existed.
//!
//! So this test signals **the pid it started** and looks for survivors in
//! `/proc` by identity, never by a command-line pattern:
//!
//! * the port refuses connections within the drain's grace plus slack,
//! * `waitpid` returns within the same bound, with exit `0`,
//! * every descendant recorded before the signal is gone (pid + `starttime`,
//!   so a recycled pid cannot pass for a survivor),
//! * the `--dev-root` directory of that run is gone.
//!
//! It is unprivileged (`blastit dev` is the single-uid simulation), binds an
//! ephemeral port (retrying once another process steals it between the probe and
//! the child's `bind`), polls with deadlines instead of sleeping, and on every
//! failure path stops the tree and removes the run's `--dev-root` through
//! [`Dev`]'s `Drop` — a `SIGKILL`ed init runs no handler and cleans up nothing.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long the boot may take before the test gives up on it.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// `BLASTIT_GRACEFUL_TIMEOUT` of the spawned boot: the drain's own budget.
const GRACE: Duration = Duration::from_secs(5);
/// What the port and the exit may additionally take after the grace: a loaded
/// machine, the `TERMINATE_GRACE` `SIGKILL` path, and process teardown. The
/// assertion is "well under a minute", not "instantaneous".
const SLACK: Duration = Duration::from_secs(10);
/// How long one poll iteration waits.
const POLL: Duration = Duration::from_millis(20);
/// How many ports one launch may try before a lost bind is the gate's problem.
///
/// [`free_port`] is a hint, so the child can lose its `bind` to another process
/// between the probe and the launch; that is a harness race, not this contract.
const PORT_ATTEMPTS: u32 = 3;

/// The all-static manifest `MB.14` dogfooded: one mount at the URL root, no
/// `app`.
const MANIFEST: &str = "\
static \"/\" {
    root \"site\"
}
";

#[test]
fn sigterm_to_the_dev_init_pid_reaps_the_tree_and_closes_the_port() {
    let fixture = Fixture::new("sigterm");
    let mut dev = Dev::start(&fixture);
    let port = dev.port;

    // The pid this test started *is* init, after the in-place `execve` — that is
    // the whole trap, so it is asserted rather than assumed.
    let cmdline = dev.cmdline();
    assert!(
        cmdline.contains("--stage=1"),
        "the pid of the launch is stage 1 (the in-place execve): {cmdline:?}"
    );
    let root = dev.dev_root();
    assert!(
        root.is_dir(),
        "the run's --dev-root exists while it serves: {root:?}"
    );

    // It serves, from the real `static` mount.
    for target in ["/", "/sub/page.html"] {
        assert_eq!(dev.get(target).0, 200, "GET {target}\n{}", dev.log_text());
    }

    // Everything init forked, by identity, before the signal: http and the
    // scheduler at least (a view is forked on the request above).
    let descendants = dev.descendants();
    assert!(
        descendants.len() >= 2,
        "init forked its components: {descendants:?}\n{}",
        dev.log_text()
    );

    let signalled = Instant::now();
    dev.signal(libc::SIGTERM);

    // (a) the listening socket refuses within the grace plus slack.
    let deadline = signalled + GRACE + SLACK;
    while TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), POLL).is_ok() {
        assert!(
            Instant::now() < deadline,
            "127.0.0.1:{port} still answers {:?} after SIGTERM\n{}",
            signalled.elapsed(),
            dev.log_text()
        );
        std::thread::sleep(POLL);
    }

    // (b) `waitpid` returns within the same bound, and the drain exited `0`.
    let status = dev.wait_for_exit(deadline);
    let status = status.unwrap_or_else(|| {
        panic!(
            "blastit dev did not exit within {GRACE:?} + {SLACK:?}\n{}",
            dev.log_text()
        )
    });
    assert_eq!(
        status.code(),
        Some(0),
        "a drained dev boot exits 0, not {status:?}\n{}",
        dev.log_text()
    );
    let log = dev.log_text();
    assert!(
        log.contains("\"stage\":\"drain\""),
        "the drain is logged: {log}"
    );
    assert!(
        log.contains("\"stage\":\"stopped\""),
        "and so is its end: {log}"
    );

    // (c) no descendant of that tree survived. `starttime` tells a survivor from
    // a recycled pid: a pid that now belongs to another process is not ours.
    for descendant in &descendants {
        if let Some(started) = starttime(descendant.pid) {
            assert_ne!(
                started, descendant.starttime,
                "pid {} of the dev tree survived the SIGTERM\n{}",
                descendant.pid, log
            );
        }
    }

    // (d) the ephemeral root is gone: `supervise::finish` removes it (`R20`).
    assert!(
        !root.exists(),
        "the --dev-root survived the exit: {root:?}\n{log}"
    );
}

/// One process init forked, by identity (`starttime` from `/proc/<pid>/stat`).
#[derive(Debug, Clone, Copy)]
struct Descendant {
    pid: i32,
    /// Field 22 of `/proc/<pid>/stat`: the boot time in clock ticks, which is
    /// what tells "the same process" from "a process that reused the pid".
    starttime: u64,
}

/// A fresh fixture directory: the manifest and the two pages it serves.
struct Fixture {
    path: PathBuf,
}

impl Fixture {
    /// Creates `<tmp>/blastit-mb15-<pid>-<tag>`, mode `0755`.
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("blastit-mb15-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("site/sub")).expect("the serve root");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("a readable fixture directory");
        fs::write(path.join("Blastfile"), MANIFEST).expect("write Blastfile");
        fs::write(
            path.join("site/index.html"),
            "<!doctype html><title>mb15</title><h1>index</h1>\n",
        )
        .expect("write the index");
        fs::write(
            path.join("site/sub/page.html"),
            "<!doctype html><title>mb15</title><h1>subpage</h1>\n",
        )
        .expect("write the subpage");
        Self { path }
    }

    fn manifest(&self) -> PathBuf {
        self.path.join("Blastfile")
    }

    fn log(&self) -> PathBuf {
        self.path.join("dev.log")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A spawned `blastit dev`, killed and cleaned up after on every failure path.
struct Dev {
    child: Child,
    /// The port the child was told to bind: stage 1's argv, after the `execve`,
    /// no longer carries `--port`.
    port: u16,
    /// Where the child's stdout and stderr go, so a failure is debuggable.
    fixture: PathBuf,
    /// The `--dev-root` this run created, once it has been read out of the
    /// child's argv. A failure path `SIGKILL`s init, which runs no handler and
    /// removes nothing, so `Drop` has to know where the root is (`R20`).
    root: Option<PathBuf>,
    /// Every descendant recorded before the signal, so `Drop` can stop them if
    /// the test panicked before init reaped them.
    descendants: Vec<Descendant>,
}

/// How a freshly spawned `blastit dev` ended up, before any assertion sees it.
enum Launch {
    /// `ready`, and `GET /` answered `200`.
    Serving,
    /// It exited because another process took the port between [`free_port`]
    /// and the child's `bind` — a harness race, not this contract.
    LostThePort,
    /// It exited before it served, for any other reason.
    Exited(std::process::ExitStatus),
    /// It stayed alive but never answered within [`READY_TIMEOUT`].
    Timeout,
}

impl Dev {
    /// Starts `blastit dev` on the fixture's manifest, retrying a lost port.
    ///
    /// [`free_port`] is a hint, not a reservation, so the child can lose its
    /// `bind` to another process between the probe and the launch. That
    /// `EADDRINUSE` is a property of the host's load and would read as "never
    /// answered within 30 s", so the launch is retried on a fresh port
    /// (`PORT_ATTEMPTS`), as the protected dev harness does.
    fn start(fixture: &Fixture) -> Self {
        let mut attempt = 1;
        loop {
            let port = free_port();
            let mut dev = Self::spawn(fixture, port);
            match dev.settle() {
                Launch::Serving => return dev,
                Launch::LostThePort if attempt < PORT_ATTEMPTS => {
                    eprintln!(
                        "blastit dev: port {port} was taken between free_port() and the \
                         child's bind (EADDRINUSE); retrying with a fresh port \
                         ({attempt}/{PORT_ATTEMPTS})"
                    );
                    drop(dev);
                    attempt += 1;
                }
                Launch::LostThePort => panic!(
                    "blastit dev could not bind {PORT_ATTEMPTS} ports in a row: another \
                     process kept taking the port between free_port() and the child's \
                     `bind`. A harness isolation failure, not this contract.\n{}",
                    dev.log_text()
                ),
                Launch::Exited(status) => panic!(
                    "blastit dev exited {status} before it served\n{}",
                    dev.log_text()
                ),
                Launch::Timeout => panic!(
                    "blastit dev did not answer within {READY_TIMEOUT:?}\n{}",
                    dev.log_text()
                ),
            }
        }
    }

    /// Spawns one `blastit dev --port <port>` attempt.
    fn spawn(fixture: &Fixture, port: u16) -> Self {
        let log = File::create(fixture.log()).expect("the log file");
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .env_clear()
            // Everything the boot is allowed to see is set here, one variable at
            // a time, so a developer's `BLASTIT_*` cannot reach it.
            .env("BLASTIT_MANIFEST", fixture.manifest())
            .env("BLASTIT_LOG_FORMAT", "json")
            .env("BLASTIT_GRACEFUL_TIMEOUT", GRACE.as_secs().to_string())
            // A sizing knob, not the behaviour under test: a host above its
            // memory watermark refuses to boot at all (`§8 Sizing`), which would
            // make this test about the host's load.
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .current_dir(&fixture.path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().expect("the log handle")))
            .stderr(Stdio::from(log));
        // The loader and interpreter variables the embedded CPython needs, which
        // `env_clear` would otherwise take away (`make check` sets them).
        for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let child = command.spawn().expect("the blastit binary starts");
        Self {
            child,
            port,
            fixture: fixture.path.clone(),
            root: None,
            descendants: Vec::new(),
        }
    }

    /// Polls until the port answers `200` for `/`, and says why it did not.
    fn settle(&mut self) -> Launch {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return if lost_the_port(self.port, &self.log_text()) {
                    Launch::LostThePort
                } else {
                    Launch::Exited(status)
                };
            }
            if self.try_get("/").is_some_and(|status| status == 200) {
                return Launch::Serving;
            }
            if Instant::now() >= deadline {
                return Launch::Timeout;
            }
            std::thread::sleep(POLL);
        }
    }

    /// One request, answered in full: `(status, body)`.
    fn get(&self, target: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .unwrap_or_else(|err| panic!("connecting for {target}: {err}\n{}", self.log_text()));
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .expect("a read timeout");
        stream
            .write_all(
                format!("GET {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .expect("the request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("the answer");
        parse(&raw)
    }

    /// [`Dev::get`], for the readiness poll: `None` when the server is not up.
    ///
    /// A short read timeout on purpose: the poll has a 30 s deadline, and one
    /// stalled connection must not be able to spend it.
    fn try_get(&self, target: &str) -> Option<u16> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        stream
            .write_all(
                format!("GET {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .ok()?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).ok()?;
        Some(parse(&raw).0)
    }

    /// The child's command line, from `/proc`.
    ///
    /// Before the execve it is the launch; after it, stage 1's. Either way it is
    /// the same pid, which is the point.
    fn cmdline(&self) -> String {
        let bytes = fs::read(format!("/proc/{}/cmdline", self.child.id()))
            .unwrap_or_else(|err| panic!("reading the cmdline of {}: {err}", self.child.id()));
        String::from_utf8_lossy(&bytes)
            .split('\0')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The `--dev-root` stage 1 was given, as a path, kept for [`Drop`].
    fn dev_root(&mut self) -> PathBuf {
        let cmdline = self.cmdline();
        let root = cmdline
            .split_whitespace()
            .find_map(|arg| arg.strip_prefix("--dev-root="))
            .unwrap_or_else(|| panic!("no --dev-root in {cmdline:?}"));
        let root = PathBuf::from(root);
        self.root = Some(root.clone());
        root
    }

    /// Every process init forked, recursively, with its `starttime`.
    ///
    /// Kept on the struct as well: a failure path `SIGKILL`s init, so the tree it
    /// forked is [`Drop`]'s to stop.
    fn descendants(&mut self) -> Vec<Descendant> {
        let mut found = Vec::new();
        let mut frontier = vec![self.child.id() as i32];
        while let Some(pid) = frontier.pop() {
            for child in children_of(pid) {
                if let Some(starttime) = starttime(child) {
                    found.push(Descendant {
                        pid: child,
                        starttime,
                    });
                    frontier.push(child);
                }
            }
        }
        self.descendants = found.clone();
        found
    }

    /// `SIGTERM` to the pid this test started — init, after the execve.
    fn signal(&self, signal: i32) {
        // SAFETY: the pid came from `spawn`, so it is this test's own child, and
        // `kill` takes two integers.
        let killed = unsafe { libc::kill(self.child.id() as i32, signal) };
        assert_eq!(killed, 0, "signalling pid {}", self.child.id());
    }

    /// `waitpid` within `deadline`, or `None`.
    fn wait_for_exit(&mut self, deadline: Instant) -> Option<std::process::ExitStatus> {
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return Some(status);
            }
            std::thread::sleep(POLL);
        }
        None
    }

    /// The child's own output, whole.
    fn log_text(&self) -> String {
        fs::read_to_string(self.fixture.join("dev.log")).unwrap_or_default()
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        // Whatever happened, nothing of this run outlives the test. `kill` on an
        // already-reaped child is a no-op and `wait` reaps what is left.
        let _ = self.child.kill();
        let _ = self.child.wait();
        // A failure that panicked before init drained `SIGKILL`s init, and
        // `_exit` runs no handler: whatever it forked and its ephemeral root are
        // this test's to clean up (`R20`). Best effort, and never a panic in a
        // `Drop` — a cleanup failure must not replace the test's own failure.
        for descendant in &self.descendants {
            // Only the process this test recorded: a recycled pid belongs to
            // somebody else, and `starttime` is what tells them apart.
            if starttime(descendant.pid) == Some(descendant.starttime) {
                // SAFETY: `kill` takes two integers; ESRCH (already gone) and
                // EPERM (never ours to signal) are both fine to ignore here.
                unsafe { libc::kill(descendant.pid, libc::SIGKILL) };
            }
        }
        if let Some(root) = &self.root
            && root.is_dir()
        {
            let _ = fs::remove_dir_all(root);
        }
    }
}

/// A `(status, body)` pair from one raw HTTP/1.1 response.
fn parse(raw: &[u8]) -> (u16, String) {
    let text = String::from_utf8_lossy(raw);
    let status = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status line in {:?}", &text[..text.len().min(200)]));
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

/// A port nothing listens on yet.
///
/// A hint, not a reservation: another process can take it between this `bind`
/// and the child's, so a launch that loses it is retried ([`PORT_ATTEMPTS`])
/// rather than reported as a slow boot.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    let port = listener.local_addr().expect("the bound address").port();
    drop(listener);
    port
}

/// Whether a boot that never served failed because another process took the
/// port [`free_port`] handed out before the child could `bind` it.
///
/// Stage 1 reports it as `binding <host>:<port>: Address already in use (os
/// error 98)`; in `dev` that reaches the log as an `app.output` event, so the
/// whole captured log is searched. It names a harness race, not the drain this
/// test is about.
fn lost_the_port(port: u16, log: &str) -> bool {
    let address = format!(":{port}:");
    log.lines().any(|line| {
        line.contains("binding ")
            && line.contains(&address)
            && (line.contains("Address already in use")
                || line.contains("os error 98")
                || line.contains("EADDRINUSE"))
    })
}

/// The pids of `pid`'s children, from the kernel's own list.
fn children_of(pid: i32) -> Vec<i32> {
    let dir = format!("/proc/{pid}/task");
    let mut children = Vec::new();
    let Ok(tasks) = fs::read_dir(&dir) else {
        return children;
    };
    for task in tasks.flatten() {
        let path = task.path().join("children");
        if let Ok(text) = fs::read_to_string(&path) {
            children.extend(
                text.split_whitespace()
                    .filter_map(|pid| pid.parse::<i32>().ok()),
            );
        }
    }
    children
}

/// Field 22 of `/proc/<pid>/stat` (the start time), or `None` when the process
/// is gone.
fn starttime(pid: i32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` is parenthesised and may contain spaces and `)`, so the fields
    // resume after the *last* `)`.
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().nth(19)?.parse().ok()
}
