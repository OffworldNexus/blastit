//! `MB.24.4`: the wall-clock proof of `ADR-0015`'s **measured promise** — four
//! concurrent 200 ms requests to **one** route finish in under 450 ms, while the
//! same four against a serialized route still take at least 800 ms.
//!
//! `ADR-0015` decision 1 makes a route's concurrency **boot data**: an NTS
//! `libphp` worker runs exactly one request, so the composition root writes
//! `per_process_cap = Some(1)` + `process_pool = true` on every route whose
//! binding slot is bound and the scheduler forks one worker per concurrent
//! request. A WSGI route keeps the threaded shape (`None` + `false`, one
//! process) — decision 9 says both are unchanged where it counts.
//!
//! Each leg measures its **own control in the same function**
//! (`BLASTIT_MAX_WORKERS=1`, the same four requests one at a time, `>= 800 ms`)
//! and asserts `parallel <= control - 400 ms` beside `< 450 ms`, so neither a
//! fast host nor a deleted pool can pass: `ADR-0015`'s promise is a comparison,
//! not a constant. No `static`, no env var and no second test carries a number
//! across a leg.
//!
//! **"Warm" is part of the claim, not a footnote.** A `libphp` worker `dlopen`s
//! the library and runs `php_module_startup` after the fork (`ADR-0010`
//! decision 2), and `prefork` warms exactly one worker, so
//! [`serialized_then_parallel_php`] declares `prefork` **and** sends one
//! four-wide warm-up burst before the clock — asserting four `200`s, four
//! markers and four distinct pids — and then asserts the measured window reuses
//! that pid set with no new `process.spawned`. A cold route's first requests pay
//! the startup; that is pre-existing behaviour (`prefork` remains its lever),
//! and the warm-up burst's own duration is recorded in
//! `plan/nodes/MB/MB.24/MB.24.4/RESULT.md`.
//!
//! [`the_cap_holds_and_the_pool_shrinks`] proves the other half of decision 5:
//! `BLASTIT_MAX_WORKERS` still strictly caps total concurrency (8 concurrent
//! with a budget of 4 is two waves over exactly four pids, never eight parallel
//! workers), and the pool gives its extras back — every burst pid retires with
//! reason `idle` once `BLASTIT_VIEW_IDLE_TIMEOUT` expires, and the next request
//! gets a fresh one.
//!
//! # Which outcome is required is decided by [`host_php`], never by the boot
//!
//! As in `critical_php_embed` (whose probe this one copies, `MB.3.62`'s drift
//! test included), the branch comes from an independent probe: raw `std` file
//! access plus `ADR-0010` §4.1's names, never the code under test. A host with
//! the pair runs the two PHP legs for real; a host without it must **refuse**
//! with the package sentence `critical_php_embed` pins
//! ([`refused_without_a_host`]). Neither branch skips: a PHP leg whose host has
//! no usable pair asserts that documented refusal first and only then returns,
//! so no run of this file is a no-op and no test here is marked ignored.
//!
//! The WSGI leg runs on every `make check` (`rs-test-critical` passes
//! `--features python` for this crate). The PHP legs run there too — a CI
//! `check` host has no PHP, so they assert the refusal — and against the real
//! pair through `make rs-test-php` (`PHP_SUITE_ARGS` now names this file). The
//! `php-sapi` **image** leg still builds only `critical_php_backend` and
//! `critical_php_embed` (`e2e/php-sapi/Dockerfile` hardcodes them and the
//! Makefile refuses anything but two recorded executables), so getting this
//! suite into the image is a recorded follow-up in `MB.24.4/RESULT.md`, not a
//! claim of this file.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// `stage0.rs`'s discovery order, as a copy this suite's probe walks on its own
/// (`MB.3.62`): a drift check keeps it honest.
#[path = "common/libphp_probes.rs"]
mod libphp_probes;

/// How many requests a wall-clock leg sends at once: `ADR-0015`'s promise is
/// about four, and the PHP warm-up is four wide for the same reason.
const WIDTH: usize = 4;

/// The serialized floor: four 200 ms requests, one at a time.
const CONTROL_FLOOR: Duration = Duration::from_millis(800);

/// The measured promise: four concurrent 200 ms requests on one warm route.
const PARALLEL_BUDGET: Duration = Duration::from_millis(450);

/// How much faster than the serialized control the parallel leg must be, so a
/// deleted pool cannot pass on a fast host (`ADR-0015`, the measured promise).
const CONTROL_MARGIN: Duration = Duration::from_millis(400);

/// How far apart the WSGI app's own per-request `t0` stamps may be for the four
/// requests to count as concurrent, in seconds (`docs/design/parallel-views.md`
/// §7): the app stamps itself, so a leg that quietly staged its requests one
/// after another cannot pass on a fast machine.
#[cfg(feature = "python")]
const CONTENTION_SPAN: f64 = 0.100;

/// How long a boot may take to bind, refuse, or answer.
const BOOT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long one request may take once the port accepts connections.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// How long init's event stream must be quiet before a `process.spawned` set is
/// read as complete: the event travels on init's own stdout pipe, so "no new
/// process" is a settled set, not whatever happened to have arrived.
const QUIET: Duration = Duration::from_millis(300);

/// How long the pool may take to give its idle workers back.
const RETIRE_TIMEOUT: Duration = Duration::from_secs(15);

/// `ADR-0010` §4.1's package half: the sentence every refusal carries
/// (`critical_php_embed`'s `PACKAGES`).
const PACKAGES: &str = "install the PHP embed package";

/// The route name both fixtures declare.
const ROUTE: &str = "app";

/// The PHP front controller, as `entrypoint php` spells it.
const ENTRY: &str = "public/index.php";

/// A WSGI view: one process, `BLASTIT_VIEW_MAX_WORKERS` requests at once
/// (`ADR-0015` decision 9 — unchanged).
#[cfg(feature = "python")]
const WSGI_MANIFEST: &str = "\
app \"app\"
entrypoint python \"app:app\"

view app regex '.*' {}
";

/// The WSGI fixture: 200 ms in `time.sleep` (which releases the GIL, so a
/// threaded process really overlaps them), the last path segment back as its own
/// marker, and the request's own `t0` (`docs/design/parallel-views.md` §7) so the
/// suite can assert the four requests really left together instead of trusting
/// the barrier alone.
#[cfg(feature = "python")]
const WSGI_APP: &str = "\
import time

def app(environ, start_response):
    t0 = time.monotonic()
    time.sleep(0.2)
    marker = environ.get(\"PATH_INFO\", \"/\").rsplit(\"/\", 1)[-1] or \"root\"
    body = (\"wsgi:%s:%.6f\" % (marker, t0)).encode()
    start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\"),
                              (\"Content-Length\", str(len(body)))])
    return [body]
";

/// A PHP route that warms one worker at boot (`prefork`): the measured legs
/// send a four-wide burst first, because `prefork` warms exactly one.
const PHP_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {
    prefork
}
";

/// The same route with no warm worker: the cap leg's burst must pay its own
/// startups inside the window (which is what makes its lower bound meaningful).
const PHP_MANIFEST_COLD: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

/// The PHP fixture: 200 ms in `usleep`, then this request's own marker and the
/// pid that served it — so a swapped response and a shared process are both
/// detectable from the body alone.
const PHP_PAGE: &str = "\
<?php
$m = $_GET['m'] ?? 'none';
usleep(200000);
header('Content-Type: text/plain');
echo \"php:$m:\" . getmypid() . \"\\n\";
";

// ------------------------------------------------------------- assertions

/// A response that is not a `200` fails with what the server actually said.
fn assert_200(what: &str, body: &str) {
    assert!(
        body.starts_with("HTTP/1.1 200"),
        "{what}: expected a `200`, got:\n{body}"
    );
}

/// The marker and the request's own `t0` a WSGI response carries
/// (`wsgi:<marker>:<t0>`).
#[cfg(feature = "python")]
fn wsgi_answer(response: &str) -> Option<(&str, f64)> {
    let rest = response.split("wsgi:").nth(1)?;
    let mut fields = rest.split_whitespace().next()?.split(':');
    let marker = fields.next()?;
    let t0 = fields.next()?.parse().ok()?;
    Some((marker, t0))
}

/// The marker and the pid a PHP response carries (`php:<marker>:<pid>`).
fn php_answer(response: &str) -> Option<(&str, u32)> {
    let rest = response.split("php:").nth(1)?;
    let (marker, rest) = rest.split_once(':')?;
    Some((marker, rest.split_whitespace().next()?.parse().ok()?))
}

/// Asserts four `200`s, each carrying its **own** request's marker and `t0`, and
/// returns the four `t0`s: a swapped or shared response is a failure, not a
/// coincidence, and the `t0`s are what prove the four left together.
#[cfg(feature = "python")]
fn assert_wsgi_burst(what: &str, bodies: &[String]) -> Vec<f64> {
    assert_eq!(bodies.len(), WIDTH, "{what}: one response per request");
    let mut starts = Vec::with_capacity(bodies.len());
    for (index, body) in bodies.iter().enumerate() {
        assert_200(what, body);
        let expected = format!("w{index}");
        let (marker, t0) = wsgi_answer(body)
            .unwrap_or_else(|| panic!("{what}: request {index} carried no app body:\n{body}"));
        assert_eq!(
            marker, expected,
            "{what}: request {index} answered for someone else:\n{body}"
        );
        starts.push(t0);
    }
    starts
}

/// Asserts `WIDTH` `200`s with their own markers and returns the pids that
/// served them, in request order.
fn assert_php_burst(what: &str, bodies: &[String]) -> Vec<u32> {
    assert_eq!(bodies.len(), WIDTH, "{what}: one response per request");
    let mut pids = Vec::with_capacity(bodies.len());
    for (index, body) in bodies.iter().enumerate() {
        assert_200(what, body);
        let (marker, pid) = php_answer(body)
            .unwrap_or_else(|| panic!("{what}: request {index} carried no page body:\n{body}"));
        let expected = format!("p{index}");
        assert_eq!(
            marker, expected,
            "{what}: request {index} answered for someone else:\n{body}"
        );
        pids.push(pid);
    }
    pids
}

/// The distinct pids in `pids`, sorted: a burst's worker set.
fn worker_set(pids: &[u32]) -> Vec<u32> {
    let mut set = pids.to_vec();
    set.sort_unstable();
    set.dedup();
    set
}

// ------------------------------------------------------------- host probe

/// The host's NTS `libphp` and the shim for its tuple, resolved **without** the
/// code under test (`critical_php_embed`'s probe, copied).
struct HostPhp {
    /// The real file a boot is handed (`canonicalize`d).
    libphp: PathBuf,
    /// The shim for its `(major, minor)`.
    shim: PathBuf,
}

/// The library and shim this host can embed, or `None`.
fn host_php() -> Option<HostPhp> {
    let libphp = libphp_path()?;
    // A `.dynsym` name is a plain string in the file; reading the bytes beats
    // duplicating an ELF walk, and it is still independent of `blastit`.
    let bytes = std::fs::read(&libphp).ok()?;
    if !contains(&bytes, b"executor_globals") || contains(&bytes, b"tsrm_startup") {
        return None;
    }
    // An object that exports no `php_version` — the symbol the worker resolves
    // before `php_module_startup` — is refused by stage 0 (`MB.3.59`). The exact
    // name needs its NUL: `php_version_compare` exists in every version and must
    // not count as the symbol.
    if !contains(&bytes, b"php_version\0") {
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

/// The discovery order this probe walks: a file name, and the per-minor
/// directory it is looked for in (`None`: every directory of [`libphp_path`]'s
/// list, in its order).
///
/// Version-major, then spelling (`MB.3.61`): the *minor* is preferred and the
/// name only breaks a tie within it. It is a **copy** of `stage0.rs`'s
/// `LIBPHP_PROBES` — this suite may not lean on the code under test — and
/// [`the_probe_table_matches_stage0`] keeps the copy honest (`MB.3.62`).
const PROBES: [(&str, Option<&str>); 13] = [
    ("libphp8.5.so", None),
    ("libphp-8.5.so", None),
    ("libphp85.so", None),
    ("libphp.so", Some("php85")),
    ("libphp8.4.so", None),
    ("libphp-8.4.so", None),
    ("libphp84.so", None),
    ("libphp.so", Some("php84")),
    ("libphp8.3.so", None),
    ("libphp-8.3.so", None),
    ("libphp83.so", None),
    ("libphp.so", Some("php83")),
    ("libphp.so", None),
];

/// `MB.3.62`: [`PROBES`] is `stage0.rs`'s discovery order, read from that source
/// at compile time and compared entry by entry — the copy this suite probes a
/// host with cannot drift from the boot's own list.
#[test]
fn the_probe_table_matches_stage0() {
    if let Err(drift) = libphp_probes::assert_matches(
        &PROBES,
        include_str!("../../blastit-view-php/src/discovery.rs"),
    ) {
        panic!("{drift}");
    }
}

/// `BLASTIT_LIBPHP` when it is a file, else the version-major probe order in the
/// library directories (`ADR-0010` §3) — the resolved path.
fn libphp_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BLASTIT_LIBPHP") {
        let path = PathBuf::from(path);
        return path.is_file().then(|| path.canonicalize().ok()).flatten();
    }
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    let triplet = format!("{}-linux-{libc}", std::env::consts::ARCH);
    let dirs = [
        format!("/usr/lib/{triplet}"),
        "/usr/lib64".to_string(),
        "/usr/lib".to_string(),
        "/usr/lib/php85".to_string(),
        "/usr/lib/php84".to_string(),
        "/usr/lib/php83".to_string(),
        format!("/usr/local/lib/{triplet}"),
        "/usr/local/lib".to_string(),
        "/usr/local/lib/php85".to_string(),
        "/usr/local/lib/php84".to_string(),
        "/usr/local/lib/php83".to_string(),
    ];
    for (name, minor) in PROBES {
        let found = dirs
            .iter()
            .filter(|dir| match minor {
                Some(minor) => {
                    Path::new(dir).file_name().and_then(|name| name.to_str()) == Some(minor)
                }
                None => true,
            })
            .map(|dir| Path::new(dir).join(name))
            .find(|path| path.is_file());
        if let Some(path) = found {
            return path.canonicalize().ok();
        }
    }
    None
}

/// The `(major, minor)` the path states: a dotted name (`libphp8.4.so`,
/// `libphp-8.4.so`), a packed name (`libphp83.so`), or a per-minor directory
/// (`/usr/lib/php84`, `MB.3.59`).
fn minor_of(path: &Path) -> Option<(u32, u32)> {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(dotted) = dotted_of(name)
    {
        return Some(dotted);
    }
    let packed = |component: &str| -> Option<(u32, u32)> {
        let rest = component
            .strip_prefix("libphp")
            .or_else(|| component.strip_prefix("php"))?;
        let stem = rest.split('.').next()?;
        let bytes = stem.as_bytes();
        if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some((
            u32::from(bytes.first()? - b'0'),
            u32::from(bytes.get(1)? - b'0'),
        ))
    };
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(packed) = packed(name)
    {
        return Some(packed);
    }
    path.ancestors()
        .skip(1)
        .filter_map(Path::file_name)
        .filter_map(|component| component.to_str())
        .find_map(packed)
}

/// The dotted spelling: `libphp8.4.so`, `libphp-8.4.so`, `libphp8.4.so.0`.
fn dotted_of(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("libphp")?;
    let mut rest = rest.strip_prefix('-').unwrap_or(rest);
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
    let name = shim_name(major * 10 + minor);
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

/// The shim's file name for a packed minor (`84`), spelled by the target tuple.
fn shim_name(minor: u32) -> String {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    };
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    format!("libblastit_php{minor}-{arch}-{libc}.so")
}

// ---------------------------------------------------------------- fixture

/// A temp directory holding one case's `Blastfile` and its app, removed on drop.
struct Scratch {
    /// The directory `blastit dev` is started in.
    dir: PathBuf,
}

impl Scratch {
    /// Creates `dir` with `Blastfile` written.
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "blastit-parallel-views-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("public")).expect("a fixture directory");
        Self { dir }
    }

    /// Writes `contents` to `relative`, creating its parent.
    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a parent directory");
        }
        std::fs::write(&path, contents).expect("a fixture file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ----------------------------------------------------------------- a boot

/// The process-wide lock every boot takes, poisoning-proof: a failing test must
/// not turn every later test into a lock failure.
///
/// Booting one at a time is also what keeps a wall-clock leg honest: no other
/// boot of this binary is forking, starting PHP or answering requests while one
/// leg's clock runs.
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The client half of a boot: only the port, so the request threads need no lock
/// on the process the main thread owns.
#[derive(Clone, Copy)]
struct Client {
    /// The port `blastit dev` bound.
    port: u16,
}

impl Client {
    /// `GET <path>` over a fresh connection, as the raw response.
    fn get(&self, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connecting");
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .expect("a read timeout");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .expect("writing the request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("the response");
        String::from_utf8_lossy(&response).into_owned()
    }
}

/// A `blastit dev` boot under test: its process, both streams, and its port.
struct Boot {
    child: Child,
    /// Every stdout line, in order (JSON events under `BLASTIT_LOG_FORMAT=json`).
    stdout: Arc<Mutex<Vec<String>>>,
    /// Everything stderr has said.
    stderr: Arc<Mutex<Vec<String>>>,
    readers: Vec<JoinHandle<()>>,
    /// One boot at a time: an ephemeral port handed back by one test must not be
    /// bound by the next boot before this one has bound it.
    #[allow(dead_code)]
    serial: std::sync::MutexGuard<'static, ()>,
    port: u16,
}

impl Boot {
    /// Starts a boot in `dir` with `env` on top of an empty environment.
    ///
    /// `env_clear` is what a developer's shell is not: every `BLASTIT_*` a test
    /// needs is named here, so nothing ambient — in particular no
    /// `BLASTIT_LIBPHP` — can make the probe and the boot disagree. The loader
    /// and interpreter variables are the two the embedded CPython needs, exactly
    /// as `make check` sets them.
    fn start(dir: &Path, env: &[(&str, &str)]) -> Self {
        let serial = boot_lock();
        // A port nobody listens on: `dev` binds it, and a refusal never gets
        // that far.
        let held = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let port = held.local_addr().expect("the port").port();
        drop(held);

        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .env_clear()
            // A loaded host must not turn this into the memory-watermark
            // refusal. The cap leg is about `BLASTIT_MAX_WORKERS`, not about
            // how much free memory this machine happens to have, so the
            // derived live-process cap is pinned to a width the legs fit in.
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .env("BLASTIT_MAX_VIEW_PROCESSES", "8")
            .env("BLASTIT_LOG_FORMAT", "json")
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawning `blastit dev`");

        let stdout_lines = Arc::new(Mutex::new(Vec::new()));
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));

        let collected = Arc::clone(&stdout_lines);
        let stdout = child.stdout.take().expect("stdout");
        let mut readers = vec![std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                collected.lock().expect("the stdout buffer").push(line);
            }
        })];

        let collected = Arc::clone(&stderr_lines);
        let stderr = child.stderr.take().expect("stderr");
        readers.push(std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                collected
                    .lock()
                    .expect("the stderr buffer")
                    .push(line.clone());
            }
        }));

        Self {
            child,
            stdout: stdout_lines,
            stderr: stderr_lines,
            readers,
            serial,
            port,
        }
    }

    /// The boot's own pid (it `execve`s into stage 1 in place, so it is init's).
    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The client half, for request threads.
    fn client(&self) -> Client {
        Client { port: self.port }
    }

    /// Waits for the boot to either announce `ready` or exit.
    ///
    /// A gate that refuses never binds — every `ADR-0010` §1 gate runs before
    /// the listener — so "the boot says it is serving" and "the process is gone"
    /// is a complete partition of the two outcomes. `ready` is init's own
    /// statement, never a connect to a port this harness picked by binding and
    /// releasing it (`MB.3.40` follow-up 4).
    fn await_boot(&mut self) -> Option<Option<i32>> {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("waiting for the boot") {
                return Some(status.code());
            }
            if self.ready() {
                return None;
            }
            assert!(
                Instant::now() < deadline,
                "`blastit dev` neither served nor refused within {BOOT_TIMEOUT:?}:\n{}\n{}",
                self.stderr_text(),
                self.stdout_text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Whether init has announced `ready`.
    fn ready(&self) -> bool {
        self.events().iter().any(|event| event["event"] == "ready")
    }

    /// Every stdout line parsed as an event, ignoring anything else.
    fn events(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lock()
            .expect("the stdout buffer")
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect()
    }

    /// The pids init has announced for this suite's one route.
    fn spawned_pids(&self) -> Vec<u32> {
        let mut pids: Vec<u32> = self
            .events()
            .iter()
            .filter(|event| event["event"] == "process.spawned" && event["route"] == ROUTE)
            .filter_map(|event| event["pid"].as_u64())
            .filter_map(|pid| u32::try_from(pid).ok())
            .collect();
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// The pids retired with `reason`, for this suite's one route.
    fn retired_pids(&self, reason: &str) -> Vec<u32> {
        let mut pids: Vec<u32> = self
            .events()
            .iter()
            .filter(|event| {
                event["event"] == "process.retired"
                    && event["route"] == ROUTE
                    && event["reason"] == reason
            })
            .filter_map(|event| event["pid"].as_u64())
            .filter_map(|pid| u32::try_from(pid).ok())
            .collect();
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// The spawned pid set once the event stream has stopped growing, so "no new
    /// `process.spawned`" is not a race against init's stdout pipe.
    fn settled_spawned(&self) -> Vec<u32> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut last = self.spawned_pids();
        let mut since = Instant::now();
        loop {
            if Instant::now() > deadline {
                return last;
            }
            std::thread::sleep(Duration::from_millis(20));
            let now = self.spawned_pids();
            if now == last {
                if since.elapsed() >= QUIET {
                    return now;
                }
            } else {
                last = now;
                since = Instant::now();
            }
        }
    }

    fn stdout_text(&self) -> String {
        self.stdout.lock().expect("the stdout buffer").join("\n")
    }

    fn stderr_text(&self) -> String {
        self.stderr.lock().expect("the stderr buffer").join("\n")
    }

    /// Waits for the readers to reach end of file (the process is gone).
    fn join_readers(&mut self) {
        for reader in std::mem::take(&mut self.readers) {
            let _ = reader.join();
        }
    }

    /// Stops the boot: `SIGTERM` first (so `dev` removes its runtime root and
    /// drains its children), then `SIGKILL`, then the readers are joined.
    fn stop(&mut self) {
        if self.child.try_wait().expect("the boot's state").is_none() {
            let _ = Command::new("kill")
                .args(["-TERM", &self.pid().to_string()])
                .status();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if self.child.try_wait().expect("the boot's state").is_some() {
                    break;
                }
                if Instant::now() > deadline {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.join_readers();
    }
}

impl Drop for Boot {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// ---------------------------------------------------------------- helpers

/// Sends one `GET` per path **released together**: a barrier of `paths.len() + 1`
/// so the clock starts before the first request can leave.
///
/// Returns the wall time from the release to the last body, and the raw
/// responses in path order.
fn burst(client: Client, paths: &[String]) -> (Duration, Vec<String>) {
    let barrier = Arc::new(Barrier::new(paths.len() + 1));
    let mut handles = Vec::with_capacity(paths.len());
    for path in paths {
        let barrier = Arc::clone(&barrier);
        let path = path.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            client.get(&path)
        }));
    }
    let started = Instant::now();
    barrier.wait();
    let bodies: Vec<String> = handles
        .into_iter()
        .map(|handle| handle.join().expect("a request thread"))
        .collect();
    (started.elapsed(), bodies)
}

/// Waits until `check` says the world has moved, failing with `what` if it never
/// does. Polling, never a fixed sleep: the bound is on the pool, not on a guess
/// about how fast a pump runs.
fn wait_for(what: &str, within: Duration, check: impl Fn() -> bool) {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{what} did not happen within {within:?}");
}

/// The other branch of the two-outcome rule: no usable pair on this host, so the
/// boot must refuse with `ADR-0010` §4.1's sentence `critical_php_embed` pins —
/// exit `1`, nothing on stdout, the route, its `entry`, the distro package and
/// `BLASTIT_LIBPHP` named, no fallback recipe, and never a listener.
///
/// The boot is handed **the same override the probe used**, so probe and boot
/// agree about *why* this host has no pair: with no override, discovery finds
/// nothing (`install the PHP embed package`); with an override this host cannot
/// use, the boot states that override (`… cannot be opened or is not a regular
/// file …`) instead of quietly finding some other library the probe would have
/// accepted. Both shapes name the package and the override, which is what makes
/// this one assertion.
fn refused_without_a_host(what: &str, scratch: &Scratch) {
    let override_path = std::env::var_os("BLASTIT_LIBPHP");
    let override_text = override_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let mut env = Vec::new();
    if let Some(path) = override_text.as_deref() {
        env.push(("BLASTIT_LIBPHP", path));
    }
    let mut boot = Boot::start(&scratch.dir, &env);
    let code = boot
        .await_boot()
        .expect("a host with no usable libphp cannot serve an entrypoint php file");
    boot.stop();
    let stdout = boot.stdout_text();
    let stderr = boot.stderr_text();
    assert_eq!(code, Some(1), "{what}: stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "{what}: a startup error writes nothing on stdout (§1.1)"
    );
    for needle in [
        format!("route {ROUTE:?}"),
        format!("entrypoint php {ENTRY:?}"),
        PACKAGES.to_string(),
        "BLASTIT_LIBPHP".to_string(),
    ] {
        assert!(
            stderr.contains(&needle),
            "{what}: the refusal must carry {needle:?}: {stderr}"
        );
    }
    let lower = stderr.to_ascii_lowercase();
    for banned in ["php-fpm", "fastcgi", "route.fastcgi", "wsgi"] {
        assert!(
            !lower.contains(banned),
            "{what}: the refusal names a fallback recipe ({banned}): {stderr}"
        );
    }
    println!("{what}: refused (no usable libphp + shim on this host): {stderr}");
}

// ------------------------------------------------------------------ WSGI

/// One WSGI route: the serialized control, then the parallel leg on a fresh boot
/// (`ADR-0015` decision 9 — the Python path is unchanged, and this is its
/// witness).
///
/// Both numbers come from this one function, so no `static` or env var crosses
/// tests, and the parallel boot is new: the control's `BLASTIT_MAX_WORKERS=1`
/// cannot make the parallel leg pass by accident.
#[cfg(feature = "python")]
#[test]
fn serialized_then_parallel_wsgi() {
    let scratch = Scratch::new("wsgi");
    scratch.write("Blastfile", WSGI_MANIFEST);
    scratch.write("app.py", WSGI_APP);
    let paths: Vec<String> = (0..WIDTH).map(|index| format!("/m/w{index}")).collect();

    // The control: one live request slot, four requests one at a time. The floor
    // is the fixture's own 4 × 200 ms, so a fast host cannot below it.
    let mut control = Boot::start(&scratch.dir, &[("BLASTIT_MAX_WORKERS", "1")]);
    assert!(
        control.await_boot().is_none(),
        "a WSGI route must serve: {}",
        control.stderr_text()
    );
    let started = Instant::now();
    let mut control_bodies = Vec::with_capacity(WIDTH);
    for path in &paths {
        control_bodies.push(control.client().get(path));
    }
    let control_wall = started.elapsed();
    assert_wsgi_burst("wsgi control", &control_bodies);
    assert!(
        control_wall >= CONTROL_FLOOR,
        "the serialized control took {control_wall:?}, below the {CONTROL_FLOOR:?} floor the \
         fixture's own sleeps set — the fixture does not sleep, so the parallel leg would prove \
         nothing"
    );
    control.stop();
    drop(control);

    // The parallel leg: a fresh boot at the scheduler's own width.
    let mut boot = Boot::start(&scratch.dir, &[]);
    assert!(
        boot.await_boot().is_none(),
        "a WSGI route must serve: {}",
        boot.stderr_text()
    );
    let spawned_before = boot.settled_spawned();
    assert!(
        spawned_before.is_empty(),
        "a lazy boot forks nothing before the first request, saw {spawned_before:?}"
    );
    let (wall, bodies) = burst(boot.client(), &paths);
    let starts = assert_wsgi_burst("wsgi parallel", &bodies);
    assert!(
        wall < PARALLEL_BUDGET,
        "four concurrent 200 ms requests to one WSGI route took {wall:?}, which is not under \
         {PARALLEL_BUDGET:?}: the route is serializing them"
    );
    assert!(
        wall + CONTROL_MARGIN <= control_wall,
        "the parallel leg took {wall:?} against a serialized control of {control_wall:?}: it must \
         be at least {CONTROL_MARGIN:?} faster, or this host's own speed is doing the work"
    );
    // The app's own `t0`s: the four requests were really in flight together, not
    // staged one behind another by this suite (`docs/design/parallel-views.md`
    // §7).
    let span = starts.iter().copied().fold(f64::MIN, f64::max)
        - starts.iter().copied().fold(f64::MAX, f64::min);
    assert!(
        span <= CONTENTION_SPAN,
        "the four requests entered the app within {:.1} ms of each other, more than the {:.0} ms \
         this leg allows: it is measuring how fast the client can send, not the pool",
        span * 1000.0,
        CONTENTION_SPAN * 1000.0
    );
    // `ADR-0015` decision 9: the Python path is unchanged — one process, however
    // many requests it runs at once.
    let spawned_after = boot.settled_spawned();
    assert_eq!(
        spawned_after.len(),
        1,
        "a threaded WSGI route owns exactly one process however many requests it runs at once, \
         saw {spawned_after:?}"
    );
    boot.stop();
    println!(
        "wsgi: serialized control {control_wall:?} (BLASTIT_MAX_WORKERS=1), parallel {wall:?} \
         over {} spawned process(es), four t0s spanning {:.1} ms",
        spawned_after.len(),
        span * 1000.0
    );
}

// ------------------------------------------------------------------ PHP

/// One PHP route: the serialized control, the four-wide **warm-up** burst, then
/// the measured four (`ADR-0015`'s measured promise, warm by construction).
///
/// The control runs the same four 200 ms requests one at a time with
/// `BLASTIT_MAX_WORKERS=1` (a cold worker only inflates it, so the lower bound
/// is safe). The measured leg asserts the pid set is unchanged and
/// `process.spawned` does not grow across the window, so no fork, `dlopen` or
/// `php_module_startup` hides inside the 450 ms.
#[test]
fn serialized_then_parallel_php() {
    let scratch = Scratch::new("php");
    scratch.write("Blastfile", PHP_MANIFEST);
    scratch.write(ENTRY, PHP_PAGE);
    let paths: Vec<String> = (0..WIDTH).map(|index| format!("/php?m=p{index}")).collect();

    let Some(host) = host_php() else {
        // No usable pair on this host: the documented refusal **is** this leg's
        // assertion, and the return happens only after it has run — never a
        // skip.
        refused_without_a_host("php parallel", &scratch);
        return;
    };
    let libphp = host.libphp.to_string_lossy();
    let shim_dir = host
        .shim
        .parent()
        .expect("a shim directory")
        .to_string_lossy();
    let pair = [
        ("BLASTIT_LIBPHP", libphp.as_ref()),
        ("BLASTIT_PHP_SHIM_DIR", shim_dir.as_ref()),
    ];

    // The control: one live request slot, so one worker, four requests serially.
    let mut control_env = Vec::from(pair);
    control_env.push(("BLASTIT_MAX_WORKERS", "1"));
    let mut control = Boot::start(&scratch.dir, &control_env);
    assert!(
        control.await_boot().is_none(),
        "this host has {} and {}, so the route must be served: {}",
        host.libphp.display(),
        host.shim.display(),
        control.stderr_text()
    );
    let started = Instant::now();
    let mut control_bodies = Vec::with_capacity(WIDTH);
    for path in &paths {
        control_bodies.push(control.client().get(path));
    }
    let control_wall = started.elapsed();
    let control_pids = assert_php_burst("php control", &control_bodies);
    assert_eq!(
        worker_set(&control_pids).len(),
        1,
        "with BLASTIT_MAX_WORKERS=1 one worker serves all four serially, saw {control_pids:?}"
    );
    assert!(
        control_wall >= CONTROL_FLOOR,
        "the serialized control took {control_wall:?}, below the {CONTROL_FLOOR:?} floor the \
         fixture's own sleeps set — the fixture does not sleep, so the parallel leg would prove \
         nothing"
    );
    control.stop();
    drop(control);

    // The measured boot: a budget wide enough for four workers, and `prefork`
    // so one is already warm.
    let mut env = Vec::from(pair);
    env.push(("BLASTIT_MAX_WORKERS", "4"));
    let mut boot = Boot::start(&scratch.dir, &env);
    assert!(
        boot.await_boot().is_none(),
        "this host has {} and {}, so the route must be served: {}",
        host.libphp.display(),
        host.shim.display(),
        boot.stderr_text()
    );
    let spawned_at_boot = boot.settled_spawned();

    // The warm-up burst: the same four requests, one worker each. Its duration
    // includes every new worker's `dlopen` + `php_module_startup` and is not
    // part of the 450 ms (`ADR-0015`).
    let (warm_wall, warm_bodies) = burst(boot.client(), &paths);
    let warm_pids = assert_php_burst("php warm-up", &warm_bodies);
    let warm_workers = worker_set(&warm_pids);
    assert_eq!(
        warm_workers.len(),
        WIDTH,
        "the warm-up burst is four wide: it must have used four processes, saw {warm_pids:?} \
         (a narrower warm-up would leave a cold worker inside the measured window)"
    );
    let spawned_before = boot.settled_spawned();
    assert_eq!(
        spawned_before.len(),
        WIDTH.max(spawned_at_boot.len()),
        "the warm-up burst must have forked one worker per request: spawned {spawned_before:?} \
         for pids {warm_pids:?}"
    );

    // The measured four: the same workers, no new fork.
    let (wall, bodies) = burst(boot.client(), &paths);
    let pids = assert_php_burst("php measured", &bodies);
    assert_eq!(
        worker_set(&pids),
        warm_workers,
        "the measured window must reuse the four warm workers {warm_workers:?}, saw {pids:?}"
    );
    let spawned_after = boot.settled_spawned();
    assert_eq!(
        spawned_after, spawned_before,
        "no `process.spawned` may appear inside the measured window: {spawned_before:?} -> \
         {spawned_after:?}"
    );
    assert!(
        wall < PARALLEL_BUDGET,
        "four concurrent 200 ms requests to one warm PHP route took {wall:?}, which is not under \
         {PARALLEL_BUDGET:?}: the route is running one request per process but not in parallel"
    );
    assert!(
        wall + CONTROL_MARGIN <= control_wall,
        "the parallel leg took {wall:?} against a serialized control of {control_wall:?}: it must \
         be at least {CONTROL_MARGIN:?} faster, or this host's own speed is doing the work"
    );
    boot.stop();
    println!(
        "php: serialized control {control_wall:?}, warm-up burst {warm_wall:?} over pids \
         {warm_workers:?}, measured {wall:?} over the same pids with {} spawned process(es)",
        spawned_before.len()
    );
}

/// The other half of decision 5: `BLASTIT_MAX_WORKERS` strictly caps total
/// concurrency, and the pool gives its extras back.
///
/// Eight concurrent 200 ms requests with a budget of 4 are two waves over
/// exactly four pids — serialized would be ~1600 ms, eight parallel ~200 ms — and
/// then every burst pid retires with reason `idle` once
/// `BLASTIT_VIEW_IDLE_TIMEOUT` expires, after which the next request gets a
/// fresh worker.
///
/// The clock's floor is the fixture's own **two waves** (`2 × 200 ms`), not
/// `docs/design/parallel-views.md` §7's 600 ms: that number priced four PHP
/// startups into the window, and on this host a fork + `dlopen` +
/// `php_module_startup` measures ~30 ms (`MB.24.4/RESULT.md` records the
/// measurement), so the window is ~480 ms and 600 ms is unreachable by
/// construction. Two waves still separate a 4-wide pool (~480 ms here) from an
/// 8-wide one (~280 ms) by a wide margin, and the pid set — exactly four pids
/// for eight requests, each its own request's marker — is the host-independent
/// oracle for the cap.
#[test]
fn the_cap_holds_and_the_pool_shrinks() {
    const CAP: u32 = 4;
    const CONCURRENT: usize = 8;
    /// The fixture's own sleep per request: 200 ms, so no part of the pool can
    /// answer two of them sooner than two waves.
    const FIXTURE_SLEEP: Duration = Duration::from_millis(200);
    /// Well under the ~1600 ms a serialized route would need.
    const TWO_WAVES_CEILING: Duration = Duration::from_millis(1000);

    let scratch = Scratch::new("php-cap");
    scratch.write("Blastfile", PHP_MANIFEST_COLD);
    scratch.write(ENTRY, PHP_PAGE);
    let paths: Vec<String> = (0..CONCURRENT)
        .map(|index| format!("/php?m=c{index}"))
        .collect();

    let Some(host) = host_php() else {
        // No usable pair on this host: the documented refusal **is** this leg's
        // assertion, and the return happens only after it has run — never a
        // skip.
        refused_without_a_host("php cap", &scratch);
        return;
    };
    let libphp = host.libphp.to_string_lossy();
    let shim_dir = host
        .shim
        .parent()
        .expect("a shim directory")
        .to_string_lossy();
    let env = [
        ("BLASTIT_LIBPHP", libphp.as_ref()),
        ("BLASTIT_PHP_SHIM_DIR", shim_dir.as_ref()),
        ("BLASTIT_MAX_WORKERS", "4"),
        // Short enough to watch the pool shrink, long enough that a worker
        // waiting for the second wave is never retired between its two requests.
        ("BLASTIT_VIEW_IDLE_TIMEOUT", "500ms"),
    ];

    let mut boot = Boot::start(&scratch.dir, &env);
    assert!(
        boot.await_boot().is_none(),
        "this host has {} and {}, so the route must be served: {}",
        host.libphp.display(),
        host.shim.display(),
        boot.stderr_text()
    );

    let (wall, bodies) = burst(boot.client(), &paths);
    // Every marker exactly once, each its own request's.
    assert_eq!(bodies.len(), CONCURRENT, "one response per request");
    let mut markers = Vec::with_capacity(CONCURRENT);
    let mut pids = Vec::with_capacity(CONCURRENT);
    for (index, body) in bodies.iter().enumerate() {
        assert_200("php cap", body);
        let (marker, pid) = php_answer(body)
            .unwrap_or_else(|| panic!("php cap: request {index} carried no page body:\n{body}"));
        markers.push(marker.to_string());
        pids.push(pid);
    }
    markers.sort();
    markers.dedup();
    assert_eq!(
        markers.len(),
        CONCURRENT,
        "every request must answer for itself, saw {markers:?}"
    );
    let workers = worker_set(&pids);
    assert_eq!(
        workers.len(),
        CAP as usize,
        "{CONCURRENT} concurrent requests under BLASTIT_MAX_WORKERS={CAP} must run on exactly \
         {CAP} processes, saw {pids:?}"
    );
    // The host-independent half of the same rule: init forked exactly those
    // workers and no more, so no fifth process was hidden by the fan-in.
    assert_eq!(
        boot.settled_spawned(),
        workers,
        "init must have forked exactly the {CAP} workers that answered, no more"
    );
    let rounds = u32::try_from(CONCURRENT).expect("a small burst width") / CAP;
    let two_waves = FIXTURE_SLEEP * rounds;
    let serialized = FIXTURE_SLEEP * CONCURRENT as u32;
    assert!(
        wall >= two_waves && wall < TWO_WAVES_CEILING,
        "{CONCURRENT} concurrent 200 ms requests under BLASTIT_MAX_WORKERS={CAP} took {wall:?}, \
         which is outside {two_waves:?}..{TWO_WAVES_CEILING:?}: {CONCURRENT} parallel requests \
         would be ~{FIXTURE_SLEEP:?} and a serialized route ~{serialized:?}"
    );

    // The pool shrinks: every burst pid retires for idleness, and a follow-up
    // request is served by a worker that was not in the burst.
    let expected = workers.clone();
    wait_for(
        "every burst pid retires for idleness",
        RETIRE_TIMEOUT,
        || {
            let idle = boot.retired_pids("idle");
            expected.iter().all(|pid| idle.contains(pid))
        },
    );
    let follow_up = boot.client().get("/php?m=follow");
    assert_200("php shrink", &follow_up);
    let (marker, fresh) = php_answer(&follow_up)
        .unwrap_or_else(|| panic!("php shrink: the follow-up carried no page body:\n{follow_up}"));
    assert_eq!(marker, "follow", "the follow-up is its own request");
    assert!(
        !workers.contains(&fresh),
        "after the idle timeout every burst pid is gone, so the follow-up must get a fresh \
         process: {fresh} is one of {workers:?}"
    );
    boot.stop();
    println!(
        "php cap: {CONCURRENT} concurrent took {wall:?} over pids {workers:?}; all retired \
         (idle); the follow-up got pid {fresh}"
    );
}
