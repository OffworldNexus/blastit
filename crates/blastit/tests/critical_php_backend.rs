//! `MB.3.13.3`: a `[route.php]` route is served by an embedded worker —
//! `200` through `view.chan`, with the worker's own CPU nanoseconds on the
//! request event — or the boot refuses with `ADR-0010` §4.1's package sentence.
//!
//! `ADR-0010` §1 gives PHP exactly one backend, so there are exactly two
//! outcomes for a `[route.php]` manifest and this suite asserts **one of them**,
//! never a silent skip:
//!
//! * a host with a usable NTS `libphp` and the matching shim boots, forks the
//!   PHP view, serves a file from `[route.static]`'s root from disk, forwards
//!   every other path to the front controller, and the `request` event carries
//!   the worker's `cpu_ns` beside the request's wall time with
//!   `cpu_ns <= wall_ns`. A tick-granular clock may report `0` for a cheap
//!   script, so the assertion is that inequality plus the documented tick floor,
//!   never `cpu_ns > 0` alone (`LESSONS.md`, `MB.3.12`);
//! * a host with no usable library — or one whose `BLASTIT_LIBPHP` is not a
//!   library — refuses at boot: exit `1`, nothing on stdout, and the sentence
//!   names the route, its `entry`, the distro package and `BLASTIT_LIBPHP`.
//!
//! **Which outcome is required is decided by [`host_php`], an independent
//! probe.** Branching on the boot's own exit status would accept a discovery
//! regression as a legitimate refusal — a `NoShim` that should have resolved
//! reads exactly like "this host has no PHP" — so the probe resolves the
//! library and the shim from raw `std` file access and the `ADR-0010` §4.1
//! names, never from the code under test, and the boot must agree with it.
//!
//! The sentence must not name a fallback transport: `ADR-0010` §2 deleted
//! FastCGI and the `php-fpm` pool, and a sentence that names one — even to deny
//! it — reads to an agent as a recipe. `BLASTIT_LIBPHP` is the one name a PHP
//! boot *has* to say, which is why `libphp` is not a banned word here.
//!
//! `blastit check` reaches the same discovery gate, so a CI audit of a manifest
//! this host cannot serve is exit `2` before pytest, and a static-only manifest
//! is untouched by it (the allowed twin).
//!
//! The real load — `make php-sapi-e2e` in a container with the packaged
//! `libphp-embed` — proves the packaged library, and `MB.3.39`'s
//! `make rs-test-php-image` runs **this** suite inside that image against its
//! `libphp8.4.so` and shim, so CI's `php-sapi` job is where the composition root
//! is proved on a host that has the pair; `make check`'s PHP-free run is the
//! refusal half.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// What the fixture's `public/index.php` echoes.
const BODY: &str = "blastit-php-ok";

/// The byte string `public/asset.txt` holds: the static responder's answer.
const ASSET: &str = "a static asset";

/// `ADR-0010` §4.1's package half: the sentence every refusal carries.
const PACKAGES: &str = "install the PHP embed package";

/// How long a boot may take to bind or refuse.
const BOOT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long one request may take once the port accepts connections.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// One clock tick at `CONFIG_HZ = 1000`: below this a `0 ns` CPU reading is the
/// floor, not a broken measurement (`LESSONS.md`, `MB.3.12`).
const TICK_FLOOR_NS: u64 = 1_000_000;

/// The documented page shape (`docs/frameworks/php.md`): a file under `public/`
/// is answered by the static responder, every other path reaches the front
/// controller, and a PHP route needs no `BLASTIT_APP` (`ADR-0010` §2).
const PHP_MANIFEST: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.static]
root = \"public\"

[route.php]
entry = \"public/index.php\"
";

/// The allowed twin: no PHP route, so the discovery gate has nothing to say.
const STATIC_ONLY: &str = "\
version = 1

[[route]]
name = \"docs\"
path = ['.*']

[route.static]
root = \"site\"
";

/// The fixture's script. The busy loop is deliberate: `CLOCK_PROCESS_CPUTIME_ID`
/// reports whole ticks, and a script that returns immediately can measure `0`
/// (`LESSONS.md`, `MB.3.12`).
const INDEX_PHP: &str = "\
<?php
for ($i = 0; $i < 3000000; $i++) { $x = $i * $i; }
header('Content-Type: text/plain');
echo \"blastit-php-ok\\n\";
";

/// A temp directory holding one case's `blastit.toml` and its front controller,
/// removed on drop.
struct Scratch {
    /// The directory `blastit dev` is started in.
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str, manifest: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("blastit-php-backend-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("public")).unwrap();
        let mut file = std::fs::File::create(dir.join("blastit.toml")).unwrap();
        file.write_all(manifest.as_bytes()).unwrap();
        Self { dir }
    }

    /// The documented page: a static root, an asset, and the front controller.
    fn with_page(self) -> Self {
        let mut index = std::fs::File::create(self.dir.join("public/index.php")).unwrap();
        index.write_all(INDEX_PHP.as_bytes()).unwrap();
        let mut asset = std::fs::File::create(self.dir.join("public/asset.txt")).unwrap();
        asset.write_all(ASSET.as_bytes()).unwrap();
        self
    }

    /// Creates a static root the boot can audit before it reaches the bind.
    fn with_static_root(self, root: &str) -> Self {
        std::fs::create_dir_all(self.dir.join(root)).unwrap();
        self
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The host's NTS `libphp` and the shim for its tuple, resolved **without** the
/// code under test.
///
/// Raw `std` file access plus the `ADR-0010` §4.1 names: the candidate list, the
/// `.dynsym` strings read as bytes (NTS = `executor_globals` and no
/// `tsrm_startup`), the shim's `libblastit_php<major><minor>-<arch>-<libc>.so`
/// spelling, and the same three shim directories a `dev` boot looks in. A
/// `None` here is the only licence this suite has to accept a refusal.
struct HostPhp {
    /// The real file a symlink must resolve to (`canonicalize`d).
    libphp: PathBuf,
    /// The shim for its `(major, minor)`.
    shim: PathBuf,
}

/// The library this host can embed, or `None`.
fn host_php() -> Option<HostPhp> {
    let libphp = libphp_path()?;
    // A `.dynsym` name is a plain string in the file; reading the bytes beats
    // duplicating an ELF walk, and it is still independent of `blastit`.
    let bytes = std::fs::read(&libphp).ok()?;
    if !contains(&bytes, b"executor_globals") || contains(&bytes, b"tsrm_startup") {
        return None;
    }
    let (major, minor) = minor_of(&libphp)?;
    let shim = shim_path(major, minor)?;
    Some(HostPhp { libphp, shim })
}

/// Whether `haystack` holds `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// `BLASTIT_LIBPHP` when it is a file, else the distro's fixed names, newest
/// first, in the multi-arch directories (`ADR-0010` §3) — the resolved path.
fn libphp_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BLASTIT_LIBPHP") {
        let path = PathBuf::from(path);
        return path.is_file().then(|| path.canonicalize().ok()).flatten();
    }
    const NAMES: [&str; 7] = [
        "libphp8.4.so",
        "libphp-8.4.so",
        "libphp8.3.so",
        "libphp-8.3.so",
        "libphp8.2.so",
        "libphp-8.2.so",
        "libphp.so",
    ];
    let triplet = format!("{}-linux-gnu", std::env::consts::ARCH);
    let dirs = [
        format!("/usr/lib/{triplet}"),
        "/usr/lib64".to_string(),
        "/usr/lib".to_string(),
        format!("/usr/local/lib/{triplet}"),
        "/usr/local/lib".to_string(),
    ];
    NAMES
        .iter()
        .flat_map(|name| dirs.iter().map(move |dir| Path::new(dir).join(name)))
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

/// The `(major, minor)` a `libphp<name>` states: `libphp8.4.so`,
/// `libphp-8.4.so`, `libphp8.4.so.0`.
fn minor_of(path: &Path) -> Option<(u32, u32)> {
    let name = path.file_name()?.to_str()?.strip_prefix("libphp")?;
    let mut rest = name.strip_prefix('-').unwrap_or(name);
    let major = digits(&mut rest)?;
    rest = rest.strip_prefix('.')?;
    let minor = digits(&mut rest)?;
    Some((major, minor))
}

/// The leading decimal run, consumed from `rest`.
fn digits(rest: &mut &str) -> Option<u32> {
    let len = rest.bytes().take_while(u8::is_ascii_digit).count();
    let value = rest.get(..len)?.parse().ok()?;
    *rest = rest.get(len..)?;
    Some(value)
}

/// `libblastit_php<major><minor>-<arch>-<libc>.so` in the directories a `dev`
/// boot looks in: `BLASTIT_PHP_SHIM_DIR`, the executable's directory, then the
/// `target/php-sapi` that `make php-sapi` writes.
fn shim_path(major: u32, minor: u32) -> Option<PathBuf> {
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    let name = format!(
        "libblastit_php{major}{minor}-{}-{libc}.so",
        std::env::consts::ARCH
    );
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_blastit"));
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("BLASTIT_PHP_SHIM_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = exe.parent() {
        dirs.push(dir.to_path_buf());
    }
    if let Some(target) = exe
        .ancestors()
        .find(|dir| dir.file_name() == Some(std::ffi::OsStr::new("target")))
    {
        dirs.push(target.join("php-sapi"));
    }
    dirs.into_iter()
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

/// A `blastit dev` boot under test: its process, its two output streams, and
/// the port it was asked to bind.
struct Dev {
    child: Child,
    /// Every stdout line, in order: the event stream (and, in the human format,
    /// the banner).
    stdout: Receiver<String>,
    /// Fired by the stdout reader once it has consumed the pipe to end of file,
    /// with whether the stream ended cleanly (`MB.3.48`).
    stdout_end: Receiver<bool>,
    /// Everything stderr has said so far.
    stderr: std::sync::Arc<std::sync::Mutex<String>>,
    /// The same handshake for stderr, which is where a refusal is written.
    stderr_end: Receiver<bool>,
    /// Stdout lines [`Dev::await_boot`] took out of the channel while it was
    /// waiting for `ready`, so that the caller's own snapshot still sees them.
    stdout_seen: Vec<String>,
    port: u16,
}

impl Dev {
    /// Starts `blastit dev` in `dir` with `env` on top of an empty environment.
    ///
    /// `env_clear` is what a developer's shell is not: every `BLASTIT_*` a test
    /// needs is named here, and nothing else can leak in — in particular no
    /// `BLASTIT_LIBPHP`, so the discovery gate looks where a real boot looks.
    fn start(dir: &Path, env: &[(&str, &str)]) -> Self {
        // A port nobody listens on: `dev` binds it, and the tests that expect a
        // refusal never reach the bind.
        let held = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port();
        drop(held);

        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .env_clear()
            // A loaded host must not turn this into the memory-watermark refusal.
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let (stdout_end_tx, stdout_end) = std::sync::mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut clean = true;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    clean = false;
                    break;
                };
                if tx.send(line).is_err() {
                    break;
                }
            }
            let _ = stdout_end_tx.send(clean);
        });
        let stderr_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let collected = std::sync::Arc::clone(&stderr_text);
        let (stderr_end_tx, stderr_end) = std::sync::mpsc::channel();
        let stderr = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut clean = true;
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(line) = line else {
                    clean = false;
                    break;
                };
                let mut text = collected.lock().unwrap();
                text.push_str(&line);
                text.push('\n');
            }
            let _ = stderr_end_tx.send(clean);
        });
        Self {
            child,
            stdout: rx,
            stdout_end,
            stderr: stderr_text,
            stderr_end,
            stdout_seen: Vec::new(),
            port,
        }
    }

    /// Everything stderr has said so far.
    ///
    /// "So far" is the whole of it once [`Dev::await_boot`] has returned the
    /// exit status: that path waits for both readers to reach end of file, so a
    /// boot that has already written its refusal cannot be read as an empty
    /// string (`MB.3.48`). While the boot is still running this is a snapshot.
    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    /// Waits for both pipe readers to reach end of file.
    ///
    /// Reaping the child says nothing about the two reader threads: they are
    /// scheduled independently and can still be behind a pipe that already
    /// holds the last line. A snapshot taken at that instant reads a boot's
    /// refusal as nothing — exactly the `not a library: the route is named: `
    /// red `MB.3.48` reproduced — so every wait on the exit status is followed
    /// by this handshake. It is **not** a retry or a tuned delay: a child that
    /// is gone has closed its descriptors, so the readers reach end of file and
    /// their lines are then in full.
    ///
    /// Bounded by [`BOOT_TIMEOUT`]: a pipe that has not reached end of file by
    /// then is held open by a process the boot itself forked, which makes the
    /// snapshot incomplete rather than late, and that is a failure worth naming.
    fn await_readers(&self) {
        for (stream, end) in [("stdout", &self.stdout_end), ("stderr", &self.stderr_end)] {
            match end.recv_timeout(BOOT_TIMEOUT) {
                Ok(true) => {}
                Ok(false) => panic!(
                    "`blastit dev`'s {stream} reader stopped on a read error, so what it read is \
                     not the whole stream"
                ),
                Err(RecvTimeoutError::Disconnected) => panic!(
                    "`blastit dev`'s {stream} reader died before end of file, so what it read is \
                     not the whole stream"
                ),
                Err(RecvTimeoutError::Timeout) => panic!(
                    "`blastit dev` is gone but its {stream} pipe never reached end of file within \
                     {BOOT_TIMEOUT:?}: a process it forked still holds that descriptor"
                ),
            }
        }
    }

    /// Waits for the boot to either announce `ready` or exit.
    ///
    /// A boot that refuses never binds — the gate runs before the listener — so
    /// "the boot says it is serving" and "the process is gone" is a complete
    /// partition of the two `ADR-0010` §1 outcomes. Both halves are the boot's
    /// own word, and neither is the port: the port is an ephemeral one this
    /// harness picks by binding and releasing it, so a *foreign* listener that
    /// takes it in the window makes "something accepts on 127.0.0.1" true for a
    /// boot that refuses. That misread cost 1 run in 1200 of the "not a library"
    /// leg under 24-way pressure (`MB.3.48`), and `MB.3.40`'s follow-up 4
    /// recorded the same shape for the sibling suite. `ready` is emitted by init
    /// only after the listener is bound and the roles are forked, so it is also
    /// the moment a `get` can succeed.
    ///
    /// The exit branch then waits for both readers to drain their pipe
    /// (`MB.3.48`): the reaped process has written its refusal, and the caller
    /// is about to assert on it.
    fn await_boot(&mut self) -> Option<Option<i32>> {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.await_readers();
                return Some(status.code());
            }
            if self.take_ready() {
                return None;
            }
            assert!(
                Instant::now() < deadline,
                "`blastit dev` neither served nor refused within {BOOT_TIMEOUT:?}: {}\n{}",
                self.stderr(),
                self.drain_stdout()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Reads what stdout has said that this wait has not consumed yet, keeping
    /// every line for the caller, and reports whether `ready` was among them.
    ///
    /// Awaiting the boot must not eat the evidence it is waiting for: a refusal
    /// leg asserts that a boot which exits wrote **nothing** on stdout, so the
    /// lines are buffered in [`Dev::stdout_seen`] and handed back by
    /// [`Dev::drain_stdout`].
    fn take_ready(&mut self) -> bool {
        let mut ready = false;
        while let Ok(line) = self.stdout.try_recv() {
            ready |= is_ready(&line);
            self.stdout_seen.push(line);
        }
        ready
    }

    /// `GET <path>` over a fresh connection, as the raw response.
    fn get(&self, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.set_read_timeout(Some(REQUEST_TIMEOUT)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        String::from_utf8_lossy(&response).into_owned()
    }

    /// The `request` event for `path`, or `None`.
    fn request_event(&mut self, path: &str) -> Option<serde_json::Value> {
        // Anything the boot announced before `ready` is already in hand; a
        // `request` cannot be, but the scan keeps the two sources one event
        // stream instead of one that silently drops its first line.
        if let Some(event) = request_event_in(&self.stdout_seen, path) {
            return Some(event);
        }
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = match self.stdout.recv_timeout(left) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if value["event"] == "request" && value["path"] == path {
                return Some(value);
            }
        }
    }

    /// Everything stdout has said so far, for a failure message.
    fn drain_stdout(&mut self) -> String {
        let mut text = String::new();
        for line in std::mem::take(&mut self.stdout_seen) {
            text.push_str(&line);
            text.push('\n');
        }
        while let Ok(line) = self.stdout.try_recv() {
            text.push_str(&line);
            text.push('\n');
        }
        text
    }

    /// Stops the boot: `SIGTERM` first (so `dev` removes its runtime root and
    /// drains its children), then `SIGKILL` if it is still there.
    fn stop(&mut self) {
        if self.child.try_wait().unwrap().is_some() {
            return;
        }
        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Whether `line` is the boot's `ready` announcement, in either log format.
///
/// JSON (the legs that assert on events) carries `"event": "ready"`; the human
/// format prints `12:34:56.789 INFO  ready       serving  routes=1  workers=16`.
/// Both are emitted by init *after* the listener is bound, which is what makes
/// this the partition line between "served" and "refused" (`MB.3.48`).
fn is_ready(line: &str) -> bool {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
        return value["event"] == "ready";
    }
    line.split_whitespace().nth(2) == Some("ready")
}

/// The first `request` event for `path` in `lines`, if one is there.
fn request_event_in(lines: &[String], path: &str) -> Option<serde_json::Value> {
    lines.iter().find_map(|line| {
        let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
        (value["event"] == "request" && value["path"] == path).then_some(value)
    })
}

/// One PHP backend, one sentence (`ADR-0010` §1): the refusal must not name an
/// alternative transport — even to deny it — because that reads as a fallback
/// recipe to an agent.
///
/// `libphp` is deliberately absent from this list: `BLASTIT_LIBPHP` is the name
/// the operator has to set, so every §4.1 refusal contains it.
fn assert_no_fallback_recipe(text: &str) {
    let lower = text.to_ascii_lowercase();
    for banned in ["php-fpm", "fastcgi", "route.fastcgi", "wsgi", "meanwhile"] {
        assert!(
            !lower.contains(banned),
            "the refusal names a fallback recipe ({banned}): {text}"
        );
    }
}

/// A boot that served the PHP route would print the dev banner and, at the
/// first spawn, a `view …` line. Neither may exist on a refusal: it happens
/// before the listener and before any fork.
fn assert_no_wsgi_surface(stdout: &str, stderr: &str) {
    let all = format!("{stdout}{stderr}");
    assert!(
        !all.contains("single-uid simulation"),
        "the dev banner means the boot got past stage 0: {all}"
    );
    assert!(
        !stdout
            .lines()
            .chain(stderr.lines())
            .any(|line| line.trim_start().starts_with("view")),
        "a view line means a worker was forked for the PHP route: {all}"
    );
}

/// The `[route.php]` sentence, in the shape `ADR-0010` §4.1 requires: the route,
/// its `entry`, the package and `BLASTIT_LIBPHP`.
fn assert_package_sentence(stderr: &str, what: &str) {
    assert!(
        stderr.contains("route \"app\""),
        "{what}: the route is named: {stderr}"
    );
    assert!(
        stderr.contains("[route.php] entry \"public/index.php\""),
        "{what}: the entry is named: {stderr}"
    );
    assert!(
        stderr.contains(PACKAGES),
        "{what}: the package sentence is the fix: {stderr}"
    );
    assert!(
        stderr.contains("BLASTIT_LIBPHP"),
        "{what}: the override is named, so an out-of-tree library is reachable: {stderr}"
    );
}

/// The release's proof: a `[route.php]` page answers `200` through `view.chan`
/// on a host that can embed PHP, and refuses the boot on one that cannot.
///
/// Which of the two is required comes from [`host_php`], **not** from this
/// boot's exit status: a discovery regression that refuses a host that has the
/// library and the shim must fail here, and so must a boot that serves when the
/// probe found nothing.
#[test]
fn a_php_route_is_served_or_the_boot_refuses_naming_the_package() {
    let host = host_php();
    let scratch = Scratch::new("serve", PHP_MANIFEST).with_page();
    // The probe and the boot must look at the **same** pair. `Dev::start`
    // clears the environment, so an ambient `BLASTIT_*` would be read by the
    // probe and not by the boot — and the served leg could then pass on a pair
    // nobody verified. So the boot is handed the probe's own answer, and the
    // served leg asserts the boot's log names that exact file.
    let libphp = host.as_ref().map(|host| host.libphp.display().to_string());
    let shim_dir = host
        .as_ref()
        .and_then(|host| host.shim.parent())
        .map(|dir| dir.display().to_string());
    let mut env = vec![("BLASTIT_LOG_FORMAT", "json")];
    if let (Some(libphp), Some(shim_dir)) = (&libphp, &shim_dir) {
        env.push(("BLASTIT_LIBPHP", libphp.as_str()));
        env.push(("BLASTIT_PHP_SHIM_DIR", shim_dir.as_str()));
    }
    // JSON events: the `request` line carries the worker's `cpu_ns` and the
    // request's `duration_ms`, which is what the CPU-accounting assertion reads.
    let mut dev = Dev::start(&scratch.dir, &env);
    let boot = dev.await_boot();

    match (host, boot) {
        (Some(host), None) => {
            // The documented page, in the order `docs/frameworks/php.md` states:
            // a file under `root` from disk, every other path to the front
            // controller, and never a `.php` source from the static root.
            let asset = dev.get("/asset.txt");
            assert!(
                asset.starts_with("HTTP/1.1 200"),
                "the static responder did not answer the asset: {asset}"
            );
            assert!(asset.contains(ASSET), "{asset}");

            let response = dev.get("/php-probe");
            assert!(
                response.starts_with("HTTP/1.1 200"),
                "the miss did not reach the front controller: {response}"
            );
            assert!(
                response.contains(BODY),
                "the front controller's body is not the response's: {response}"
            );

            let source = dev.get("/index.php");
            assert!(
                source.starts_with("HTTP/1.1 404"),
                "a `.php` file under the static root must never be served: {source}"
            );

            // The boot line (`MB.3.35`) must name **the pair the probe found**:
            // the boot was handed exactly those two paths, so a boot that
            // loaded anything else means the two halves of this test disagree
            // about what "this host can embed PHP" means.
            let stderr = dev.stderr();
            assert!(
                stderr.contains(&format!("libphp {}", host.libphp.display())),
                "the boot must load the library the probe found ({}): {stderr}",
                host.libphp.display()
            );
            assert!(
                stderr.contains(&format!("shim {}", host.shim.display())),
                "and the shim the probe found ({}): {stderr}",
                host.shim.display()
            );

            let event = dev.request_event("/php-probe").unwrap_or_else(|| {
                panic!(
                    "no `request` event for `/php-probe`: {}",
                    dev.drain_stdout()
                )
            });
            assert_eq!(event["status"], 200, "the event agrees: {event}");
            assert_eq!(
                event["route"], "app",
                "the event carries the route: {event}"
            );
            // `cpu_ns` is exact process CPU time around the request; the wall
            // time it is compared with is the event's own duration. A
            // tick-granular clock may report `0` for a cheap script, so the
            // inequality is the assertion, with the documented floor second.
            let cpu_ns = event["cpu_ns"]
                .as_u64()
                .unwrap_or_else(|| panic!("the worker reported no CPU cost: {event}"));
            let wall_ns = (event["duration_ms"].as_f64().expect("a duration") * 1e6) as u64;
            assert!(
                cpu_ns <= wall_ns,
                "cpu_ns {cpu_ns} > wall_ns {wall_ns}: one process's CPU time cannot exceed its \
                 wall time: {event}"
            );
            assert!(
                cpu_ns > 0 || wall_ns < TICK_FLOOR_NS,
                "a {wall_ns} ns request that reported 0 ns of CPU should have crossed a tick: \
                 {event}"
            );

            // `ADR-0010` §1: no `libphp` above a view worker. init only *opens*
            // the two objects (it never `dlopen`s them), so its own address
            // space is the proof that the load happened below it.
            let maps =
                std::fs::read_to_string(format!("/proc/{}/maps", dev.child.id())).expect("maps");
            assert!(
                !maps.contains("libphp"),
                "init mapped {}: the load belongs to the worker alone:\n{maps}",
                host.libphp.display()
            );
            assert!(
                !maps.contains("libblastit_php"),
                "init mapped a PHP shim: the load belongs to the worker alone:\n{maps}"
            );

            // Evidence for a human: the pair this release is judged by.
            println!(
                "served: {}  cpu_ns={cpu_ns}  wall_ns={wall_ns}  (libphp {})",
                response.lines().next().unwrap_or_default(),
                host.libphp.display()
            );
            dev.stop();
        }
        (Some(host), Some(code)) => {
            // The probe found the pair, so a refusal is a regression, not an
            // outcome: say exactly which files were there.
            let stdout = dev.drain_stdout();
            let stderr = dev.stderr();
            dev.stop();
            panic!(
                "this host has {} and {}, so the boot must serve the route, but it exited \
                 {code:?}:\nstdout={stdout}\nstderr={stderr}",
                host.libphp.display(),
                host.shim.display()
            );
        }
        (None, Some(code)) => {
            let stdout = dev.drain_stdout();
            let stderr = dev.stderr();
            dev.stop();
            assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
            assert_eq!(
                stdout, "",
                "a startup error writes nothing on stdout (§1.1)"
            );
            assert_package_sentence(&stderr, "no usable library");
            assert_no_fallback_recipe(&stderr);
            assert_no_wsgi_surface(&stdout, &stderr);
            println!("refused: {stderr}");
        }
        (None, None) => {
            let stdout = dev.drain_stdout();
            let stderr = dev.stderr();
            dev.stop();
            panic!(
                "the probe found no usable libphp + shim, yet the boot served the PHP route. \
                 The probe reads BLASTIT_LIBPHP/BLASTIT_PHP_SHIM_DIR from this process's \
                 environment while the boot is given the probe's own answer, so an ambient \
                 value that is not a library makes the two disagree:\nstdout={stdout}\n\
                 stderr={stderr}"
            );
        }
    }
}

/// A `BLASTIT_LIBPHP` that is a symlink is the distro's own shape
/// (`libphp8.so -> libphp8.4.so`). Stage 1 re-opens `O_NOFOLLOW`-strictly, so the
/// gate must record the **target**: a link that passed discovery and then died
/// in the worker read `Too many levels of symbolic links (os error 40)` after
/// the banner, with no §4.1 sentence.
///
/// Both host states are asserted: with the pair, the link must boot and serve;
/// without it, the link (to nothing) must refuse with the package sentence —
/// there is no host on which this test says nothing.
#[test]
fn a_symlinked_blastit_libphp_is_resolved_before_it_is_served() {
    let scratch = Scratch::new("symlink", PHP_MANIFEST).with_page();
    let link = scratch.dir.join("libphp8.so");

    let Some(host) = host_php() else {
        // No library to point at: the only symlink this host can have is a
        // dangling one, and that is `ADR-0010` §4.1's refusal.
        std::os::unix::fs::symlink(scratch.dir.join("not-installed.so"), &link).unwrap();
        let mut dev = Dev::start(&scratch.dir, &[("BLASTIT_LIBPHP", &link.to_string_lossy())]);
        let code = dev.await_boot().expect("a dangling link cannot be served");
        let stdout = dev.drain_stdout();
        let stderr = dev.stderr();
        dev.stop();
        assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
        assert_eq!(stdout, "", "a startup error writes nothing on stdout");
        assert_package_sentence(&stderr, "dangling link");
        assert!(
            !stderr.contains("Too many levels of symbolic links"),
            "the refusal is the sentence, not an errno: {stderr}"
        );
        return;
    };

    std::os::unix::fs::symlink(&host.libphp, &link).unwrap();
    let shim_dir = host.shim.parent().expect("a shim directory");
    let mut dev = Dev::start(
        &scratch.dir,
        &[
            ("BLASTIT_LOG_FORMAT", "json"),
            ("BLASTIT_LIBPHP", &link.to_string_lossy()),
            ("BLASTIT_PHP_SHIM_DIR", &shim_dir.to_string_lossy()),
        ],
    );
    let boot = dev.await_boot();
    assert!(
        boot.is_none(),
        "a symlink to {} must boot, but the process exited {boot:?}: {}",
        host.libphp.display(),
        dev.stderr()
    );
    let response = dev.get("/php-probe");
    assert!(
        response.starts_with("HTTP/1.1 200") && response.contains(BODY),
        "the symlinked library did not serve: {response}"
    );
    // The boot line names the **resolved** file, which is what the workers load
    // and what an operator has to check against their package.
    let stderr = dev.stderr();
    assert!(
        stderr.contains(&host.libphp.display().to_string()),
        "the boot names the resolved library {}: {stderr}",
        host.libphp.display()
    );
    assert!(
        !stderr.contains("Too many levels of symbolic links"),
        "the link was resolved, not handed to dlopen: {stderr}"
    );
    println!(
        "symlinked {} -> {}  served: {}",
        link.display(),
        host.libphp.display(),
        response.lines().next().unwrap_or_default()
    );
    dev.stop();
}

/// The denied twin, deterministically: `BLASTIT_LIBPHP` names a file that is
/// not a library, so the very same fixture must refuse at boot on **every**
/// host — no `libphp` is installed here, and no shim is consulted.
#[test]
fn a_blastit_libphp_that_is_not_a_library_refuses_the_boot() {
    let scratch = Scratch::new("not-a-library", PHP_MANIFEST).with_page();
    let not_a_library = scratch.dir.join("libphp.so");
    std::fs::write(&not_a_library, b"this is not an ELF shared object").unwrap();
    let mut dev = Dev::start(
        &scratch.dir,
        &[("BLASTIT_LIBPHP", &not_a_library.to_string_lossy())],
    );

    let code = dev
        .await_boot()
        .expect("a boot with a non-library BLASTIT_LIBPHP must refuse, not serve");
    let stdout = dev.drain_stdout();
    let stderr = dev.stderr();
    dev.stop();
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert_package_sentence(&stderr, "not a library");
    assert!(
        stderr.contains(&not_a_library.display().to_string()),
        "the refusal names the file the operator pointed at: {stderr}"
    );
    assert_no_fallback_recipe(&stderr);
    assert_no_wsgi_surface(&stdout, &stderr);
    println!("refused: {stderr}");
}

/// `blastit check` is the second entry point into the same discovery gate: a
/// manifest this host cannot serve is a configuration error (exit `2`, empty
/// stdout) before pytest can exercise a route with no worker behind it.
#[test]
fn check_reaches_the_same_discovery_gate() {
    let scratch = Scratch::new("check", PHP_MANIFEST).with_page();
    let not_a_library = scratch.dir.join("libphp.so");
    std::fs::write(&not_a_library, b"this is not an ELF shared object").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .arg("check")
        .env_clear()
        .env("BLASTIT_LIBPHP", &not_a_library)
        .current_dir(&scratch.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "`blastit check` did not refuse within {BOOT_TIMEOUT:?}: the gate never ran"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout={stdout:?} stderr={stderr:?}"
    );
    assert_eq!(
        stdout, "",
        "a configuration error writes nothing on stdout (§1.1)"
    );
    assert!(stderr.contains("blastit check: "), "{stderr}");
    assert_package_sentence(&stderr, "check");
    assert_no_fallback_recipe(&stderr);
    assert_no_wsgi_surface(&stdout, &stderr);
}

/// The allowed twin: no PHP route, so the discovery gate stays silent and the
/// boot runs on to the bind — which the held port makes fail for a reason this
/// test controls. The banner proves it got past the gate; the package sentence
/// must not be there.
#[test]
fn a_static_only_manifest_is_not_refused_by_this_gate() {
    let scratch = Scratch::new("static", STATIC_ONLY).with_static_root("site");
    // The port is bound and held by this process: a boot that got past the gate
    // cannot bind it, so a gate that went missing fails here instead of serving
    // forever and hanging the suite.
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port().to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .args(["dev", "--port", &port])
        .env_clear()
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .current_dir(&scratch.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + BOOT_TIMEOUT;
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "`blastit dev` neither served nor refused within {BOOT_TIMEOUT:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        out.status.code(),
        Some(1),
        "the held port is what refuses this boot: stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        stdout.contains("1 route"),
        "the boot reached its banner with the static route counted: {stdout}"
    );
    let all = format!("{stdout}{stderr}");
    assert!(
        !all.contains(PACKAGES),
        "a static-only manifest has no PHP route to refuse: {all}"
    );
}
