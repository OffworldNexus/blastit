//! `MB.42.2.1.4`: the universal binary serves Python through the pinned shim,
//! and nothing about a PHP/static boot can reach an interpreter.
//!
//! Four proofs, all against **real** processes and **real** sockets — no
//! `Mock*Server`, no `StubPolicy`, no `#[ignore]`:
//!
//! 1. **one binary, both minors:** `blastit dev` boots the same
//!    `target/debug/blastit` against a fixture whose `.venv` is CPython 3.13 and
//!    then CPython 3.14, and each leg asserts an HTTP `200` and the app's body
//!    over a loopback socket. The shim each leg loads is the one `make py-sapi`
//!    wrote for that minor (`target/py-sapi/libblastit_py3<d>-<arch>-<libc>.so`).
//! 2. **a static-only boot never opens `libpython`:** the fixture's venv points
//!    at a prefix whose `libpython3.<m>.so.1.0` is mode `000`, so an accidental
//!    open is a hard failure rather than a quiet pass. The static mount still
//!    answers `200`.
//! 3. **a PHP-only boot never opens `libpython`:** the same unreadable prefix
//!    and the same volume of Python ingredients; the PHP worker serves (or the
//!    documented PHP refusal names PHP and nothing about an interpreter).
//! 4. **stage 1 refuses a pinned descriptor whose identity moved:** a real
//!    `(st_dev, st_ino)` swap on a real inherited descriptor (`dup2` of a second
//!    file onto the number stage 0 recorded, under a real tempdir), driven
//!    through `--stage=1 --config-fd` written by the real `Config::to_json`.
//!
//! # Skipping loudly
//!
//! `make py-sapi` needs two CPython interpreters. A host that has neither the
//! artifacts nor one of the interpreters gets one printed line per missing leg —
//! never a silent pass, never an `#[ignore]`: a suite that quietly skips is a
//! suite that cannot fail.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::CString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(feature = "python")]
use blastit::config::{Config, LogFormat, PythonHandoff};

/// How long one boot may take to answer, or to be refused.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The WSGI fixture: the running minor is in the body, so a leg cannot pass on
/// the other's interpreter.
const MANIFEST: &str = "\
app \"app\"
entrypoint python \"app:app\"

view app regex '.*' {}
";

/// The application, PEP 3333 and nothing else.
const APP: &str = "\
import sys

def app(environ, start_response):
    body = (\"served by cpython %d.%d\" % sys.version_info[:2]).encode()
    start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\"),
                              (\"Content-Length\", str(len(body)))])
    return [body]
";

/// A temporary project, removed when dropped.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("blastit-py-shim-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory");
        Self { dir }
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the parent");
        std::fs::write(&path, bytes).expect("a fixture file");
        path
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Whether a leg that cannot run must **fail** instead of skipping (`CI` sets `1`).
fn require_runtime() -> bool {
    std::env::var_os("BLASTIT_PY_SAPI_REQUIRE").is_some_and(|value| value == "1" || value == "true")
}

/// One loud skip: the reason, both commands that fix it, and a failure when the
/// gate asked for the runtime (`BLASTIT_PY_SAPI_REQUIRE=1`).
fn skip(reason: &str) {
    eprintln!(
        "SKIP {reason}: build the shims with `make py-sapi` and install the interpreters \
         with `uv python install 3.13 3.14`"
    );
    assert!(
        !require_runtime(),
        "BLASTIT_PY_SAPI_REQUIRE=1 and {reason}: build the shims with `make py-sapi` and \
         install the interpreters with `uv python install 3.13 3.14`"
    );
}

/// `target/py-sapi`, where `make py-sapi` writes the per-minor shims.
fn shim_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/py-sapi")
}

/// The shim `make py-sapi` built for `digits`, or a printed reason it is absent.
fn shim_for(digits: &str) -> Option<PathBuf> {
    let dir = shim_dir();
    let prefix = format!("libblastit_py{digits}-");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
                && path.extension().is_some_and(|ext| ext == "so")
        })
        .collect();
    found.sort();
    match found.into_iter().next() {
        Some(shim) => Some(shim),
        None => {
            skip(&format!(
                "no `{prefix}<arch>-<libc>.so` in {} for CPython {digits}",
                dir.display()
            ));
            None
        }
    }
}

/// The interpreter for `minor`, from the test override, `uv`, then `PATH`.
fn python_for(minor: &str) -> Option<PathBuf> {
    let digits = minor.replace('.', "");
    if let Some(path) = std::env::var_os(format!("BLASTIT_TEST_PYTHON_{digits}")) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let uv = Command::new("uv")
        .args(["python", "find", "--no-project", minor])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_owned()))
        .filter(|path| path.is_file());
    uv.or_else(|| {
        ["python", &format!("python{minor}")]
            .into_iter()
            .map(Path::new)
            .find(|candidate| candidate.is_file())
            .map(Path::to_path_buf)
    })
}

/// A `.venv` beside the manifest whose interpreter is `interp`.
///
/// Stage 0's dev order reads `<manifest_dir>/.venv/bin/{python3,python}` with a
/// `pyvenv.cfg` present, and takes `home`'s parent as the interpreter prefix.
fn write_venv(scratch: &Scratch, interp: &Path, minor: &str) {
    let bin = scratch.path(".venv/bin");
    std::fs::create_dir_all(&bin).expect("the venv bin directory");
    std::os::unix::fs::symlink(interp, bin.join("python3")).expect("the venv interpreter");
    let parent = interp.parent().expect("the interpreter's bin directory");
    std::fs::write(
        scratch.path(".venv/pyvenv.cfg"),
        format!(
            "home = {}\nversion = {minor}.0\ninclude-system-site-packages = false\n",
            parent.display()
        ),
    )
    .expect("pyvenv.cfg");
}

/// A port nothing listens on right now.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a loopback port");
    listener.local_addr().expect("its address").port()
}

/// One `GET`, or `None` while the boot does not answer.
fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).ok()?;
    // An accepted connection that carries no bytes is the pre-fork listener
    // (or a component that died): not an answer yet.
    if response.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&response).into_owned())
}

/// Waits for the boot to answer `path`, or panics naming what it said instead.
fn wait_for(port: u16, path: &str, child: &mut Child, stderr: &Path) -> String {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Some(response) = get(port, path) {
            return response;
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!(
                "blastit dev exited before it answered ({status}):\n{}",
                std::fs::read_to_string(stderr).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("blastit dev did not answer 127.0.0.1:{port} within {TIMEOUT:?}");
}

/// Ends the boot with `SIGTERM`, then `SIGKILL`.
fn stop(child: &mut Child) {
    let pid = i32::try_from(child.id()).unwrap_or_default();
    // SAFETY: `pid` is this test's own child and `SIGTERM` is a valid signal.
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The same boot with live reload **on** (`dev`'s default), for the cycle leg.
#[cfg(feature = "python")]
fn reload_command(scratch: &Scratch, port: u16, stderr: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(["dev", "--port", &port.to_string()])
        .current_dir(&scratch.dir)
        .env_clear()
        .env("BLASTIT_PY_SHIM_DIR", shim_dir())
        .stdin(Stdio::null())
        // `dev`'s log (the reload warning is an event on stdout) and its
        // stderr are both watched by the cycle leg.
        .stdout(std::fs::File::create(scratch.path("dev.stdout")).expect("the stdout capture"))
        .stderr(std::fs::File::create(stderr).expect("the stderr capture"));
    for name in ["LD_LIBRARY_PATH", "HOME", "TMPDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

/// One `app.py` whose body names `version`, so a reload is observable.
#[cfg(feature = "python")]
fn app_body(version: &str) -> Vec<u8> {
    format!(
        "def app(environ, start_response):\n    body = b\"body {version}\"\n    \
         start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\"), \
         (\"Content-Length\", str(len(body)))])\n    return [body]\n"
    )
    .into_bytes()
}

/// Polls `/` until the body carries `needle` (a cycle is asynchronous).
#[cfg(feature = "python")]
fn wait_for_body(port: u16, needle: &str, child: &mut Child, stderr: &Path) -> String {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Some(response) = get(port, "/").filter(|response| response.contains(needle)) {
            return response;
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!(
                "blastit dev exited before serving {needle:?} ({status}):\n{}",
                std::fs::read_to_string(stderr).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "blastit dev never served {needle:?} within {TIMEOUT:?}:\n{}",
        std::fs::read_to_string(stderr).unwrap_or_default()
    );
}

/// Polls the boot's whole log (stdout events and stderr) until it carries
/// `needle`.
#[cfg(feature = "python")]
fn wait_for_log(logs: &[&Path], needle: &str, child: &mut Child) {
    let read = || {
        logs.iter()
            .map(|path| std::fs::read_to_string(path).unwrap_or_default())
            .collect::<String>()
    };
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if read().contains(needle) {
            return;
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!(
                "blastit dev exited before printing {needle:?} ({status}):\n{}",
                read()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("blastit dev never printed {needle:?}:\n{}", read());
}

/// The environment every leg runs with: only what this suite names.
fn dev_command(scratch: &Scratch, port: u16, stderr: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(["dev", "--no-reload", "--port", &port.to_string()])
        .current_dir(&scratch.dir)
        .env_clear()
        .env("BLASTIT_PY_SHIM_DIR", shim_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(stderr).expect("the stderr capture"));
    for name in ["LD_LIBRARY_PATH", "HOME", "TMPDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

/// One WSGI leg: boot, answer `/`, assert the body, stop. `false` is a skip.
#[cfg(feature = "python")]
fn serve_wsgi(minor: &str, digits: &str) -> bool {
    let Some(shim) = shim_for(digits) else {
        return false;
    };
    let Some(interp) = python_for(minor) else {
        skip(&format!(
            "no CPython {minor} interpreter (set BLASTIT_TEST_PYTHON_{digits})"
        ));
        return false;
    };
    let scratch = Scratch::new(&format!("wsgi-{digits}"));
    scratch.write("Blastfile", MANIFEST.as_bytes());
    scratch.write("app.py", APP.as_bytes());
    write_venv(&scratch, &interp, minor);
    let port = free_port();
    let stderr = scratch.path("dev.stderr");
    let mut child = dev_command(&scratch, port, &stderr)
        .env("BLASTIT_PYTHON", &interp)
        .spawn()
        .expect("spawn blastit dev");
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for(port, "/", &mut child, &stderr)
    }));
    stop(&mut child);
    let answer = answer.expect("the WSGI boot answers");
    assert!(
        answer.starts_with("HTTP/1.1 200") || answer.starts_with("HTTP/1.0 200"),
        "CPython {minor} must serve 200:\n{answer}\n{}",
        std::fs::read_to_string(&stderr).unwrap_or_default()
    );
    let body = format!("served by cpython {minor}");
    assert!(
        answer.contains(&body),
        "the shim for CPython {minor} must be the one that served:\n{answer}"
    );
    println!(
        "CPython {minor}: 200 {body:?} via {} (shim {})",
        interp.display(),
        shim.display()
    );
    true
}

#[cfg(feature = "python")]
#[test]
fn the_one_binary_serves_wsgi_under_both_supported_minors() {
    let mut served = 0;
    for (minor, digits) in [("3.13", "313"), ("3.14", "314")] {
        if serve_wsgi(minor, digits) {
            served += 1;
        }
    }
    assert!(
        served >= 1 || !require_runtime(),
        "BLASTIT_PY_SAPI_REQUIRE=1 and no CPython leg ran: build the shims with `make py-sapi` \
         and install the interpreters with `uv python install 3.13 3.14`"
    );
    if served == 0 {
        skip("no CPython leg could run, so the one binary's Python half was not exercised");
    }
}

/// A prefix whose `libpython` cannot be opened, plus a venv pointing at it.
///
/// Everything a Python boot would search for is present — interpreter, venv,
/// shim directory — **except** the library, whose mode is `000`: an accidental
/// open is `EACCES`, which is a loud refusal rather than a quiet success.
fn unreadable_prefix(scratch: &Scratch, interp: &Path, minor: &str) -> PathBuf {
    let prefix = scratch.path("unreadable");
    std::fs::create_dir_all(prefix.join("bin")).expect("the prefix bin");
    std::fs::create_dir_all(prefix.join("lib")).expect("the prefix lib");
    std::os::unix::fs::symlink(interp, prefix.join(format!("bin/python{minor}")))
        .expect("the prefix interpreter");
    let library = prefix.join(format!("lib/libpython{minor}.so.1.0"));
    std::fs::write(&library, b"an interpreter nobody can open").expect("the library");
    std::fs::set_permissions(
        &library,
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .expect("its mode");
    let parent = prefix.join("bin");
    std::fs::create_dir_all(scratch.path(".venv/bin")).expect("the venv bin");
    std::os::unix::fs::symlink(
        parent.join(format!("python{minor}")),
        scratch.path(".venv/bin/python3"),
    )
    .expect("the venv interpreter");
    std::fs::write(
        scratch.path(".venv/pyvenv.cfg"),
        format!("home = {}\nversion = {minor}.0\n", parent.display()),
    )
    .expect("pyvenv.cfg");
    prefix
}

/// The interpreter a PHP-only/static-only leg builds its unreadable prefix from.
fn unreadable_fixture(tag: &str, manifest: &[u8]) -> Option<(Scratch, PathBuf)> {
    let Some(interp) = python_for("3.14") else {
        skip(&format!(
            "{tag}: no CPython 3.14 to build an unreadable prefix around (set \
             BLASTIT_TEST_PYTHON_314)"
        ));
        return None;
    };
    let scratch = Scratch::new(tag);
    scratch.write("Blastfile", manifest);
    let prefix = unreadable_prefix(&scratch, &interp, "3.14");
    Some((scratch, prefix))
}

#[test]
fn a_static_only_boot_never_opens_libpython() {
    let Some((scratch, prefix)) =
        unreadable_fixture("static", b"static \"/\" {\n    root \"public\"\n}\n")
    else {
        return;
    };
    scratch.write("public/index.html", b"no interpreter was opened");
    let port = free_port();
    let stderr = scratch.path("dev.stderr");
    let mut child = dev_command(&scratch, port, &stderr)
        .env("BLASTIT_PYTHON", prefix.join("bin/python3.14"))
        .spawn()
        .expect("spawn blastit dev");
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for(port, "/index.html", &mut child, &stderr)
    }));
    stop(&mut child);
    let answer = answer.expect("the static boot answers");
    assert!(
        answer.contains("no interpreter was opened"),
        "a static boot must not search for an interpreter:\n{answer}"
    );
    let log = std::fs::read_to_string(&stderr).unwrap_or_default();
    for banned in ["libpython", "CPython", "BLASTIT_PYTHON"] {
        assert!(
            !log.contains(banned),
            "a static-only boot named {banned}: {log}"
        );
    }
    println!("static: 200 without opening {}", prefix.display());
}

/// `target/php-sapi`, where `make php-sapi` writes the per-minor PHP shims.
fn php_shim_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/php-sapi")
}

/// The PHP shim `make php-sapi` writes, when there is one.
fn php_shim() -> Option<PathBuf> {
    let dir = php_shim_dir();
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("libblastit_php"))
                && path.extension().is_some_and(|ext| ext == "so")
        })
}

#[cfg(feature = "php")]
#[test]
fn a_php_only_boot_never_opens_libpython() {
    let Some((scratch, prefix)) = unreadable_fixture(
        "php",
        b"entrypoint php \"public/index.php\"\nview app regex '.*' {}\n",
    ) else {
        return;
    };
    scratch.write(
        "public/index.php",
        b"<?php header('Content-Type: text/plain'); echo 'php served this';\n",
    );
    let port = free_port();
    let stderr = scratch.path("dev.stderr");
    let mut child = dev_command(&scratch, port, &stderr)
        .env("BLASTIT_PYTHON", prefix.join("bin/python3.14"))
        .env("BLASTIT_PHP_SHIM_DIR", php_shim_dir())
        .spawn()
        .expect("spawn blastit dev");
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for(port, "/", &mut child, &stderr)
    }));
    stop(&mut child);
    let log = std::fs::read_to_string(&stderr).unwrap_or_default();
    for banned in ["libpython", "CPython", "BLASTIT_PYTHON"] {
        assert!(
            !log.contains(banned),
            "a PHP-only boot named {banned}: {log}"
        );
    }
    match (php_shim(), answer) {
        (Some(shim), Ok(answer)) => {
            assert!(
                answer.starts_with("HTTP/1.1 200") || answer.starts_with("HTTP/1.0 200"),
                "the PHP worker must serve 200:\n{answer}"
            );
            assert!(answer.contains("php served this"), "{answer}");
            println!(
                "php: 200 via {} without opening {}",
                shim.display(),
                prefix.display()
            );
        }
        (None, _) => {
            // No PHP runtime on this host: the documented refusal is this leg's
            // outcome, and it must still be about PHP only.
            assert!(
                log.contains("php") || log.contains("PHP"),
                "a PHP-only boot must refuse in PHP's terms: {log}"
            );
            println!("php: no `make php-sapi` artifact here; refused in PHP's terms");
        }
        (Some(_), Err(_)) => panic!("a PHP boot with a shim must serve: {log}"),
    }
}

/// Deliverable 6's last leg: a `dev` `exec_cycle` keeps serving **through the
/// pinned descriptors** (`Adoption.py_fds`), and a half-written save is the
/// preflight's sentence with no reload.
///
/// The replacement image's environment has no `BLASTIT_PYTHON` (stage 0 strips
/// every `BLASTIT_*` at the barrier), so the only way it can still serve is the
/// `(libpython, shim)` pair it inherits as numbers — exactly what this asserts.
#[cfg(feature = "python")]
#[test]
fn a_dev_cycle_keeps_serving_through_the_pinned_descriptors() {
    if shim_for("314").is_none() {
        return;
    }
    let Some(interp) = python_for("3.14") else {
        skip("the reload cycle leg needs CPython 3.14 (set BLASTIT_TEST_PYTHON_314)");
        return;
    };
    let scratch = Scratch::new("reload");
    scratch.write("Blastfile", MANIFEST.as_bytes());
    scratch.write("app.py", &app_body("v1"));
    write_venv(&scratch, &interp, "3.14");
    let port = free_port();
    let stderr = scratch.path("dev.stderr");
    let mut child = reload_command(&scratch, port, &stderr)
        .env("BLASTIT_PYTHON", &interp)
        .spawn()
        .expect("spawn blastit dev");
    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for(port, "/", &mut child, &stderr)
    }));
    let result = match first {
        Ok(first) => {
            assert!(first.contains("body v1"), "{first}");
            // A save to the module every view's heap holds: `exec_cycle`.
            scratch.write("app.py", &app_body("v2"));
            let second = wait_for_body(port, "body v2", &mut child, &stderr);
            assert!(
                second.starts_with("HTTP/1.1 200") || second.starts_with("HTTP/1.0 200"),
                "the replacement image must serve the new body:\n{second}"
            );
            // A half-written file: the preflight refuses it, names it, and the
            // old image keeps serving.
            scratch.write(
                "app.py",
                b"def app(environ, start_response):\n    return [\n",
            );
            let stdout = scratch.path("dev.stdout");
            wait_for_log(&[&stdout, &stderr], "app.py", &mut child);
            let still = get(port, "/").expect("the previous image still answers");
            assert!(
                still.contains("body v2"),
                "a rejected save must not reload:\n{still}"
            );
            let log = format!(
                "{}{}",
                std::fs::read_to_string(&stdout).unwrap_or_default(),
                std::fs::read_to_string(&stderr).unwrap_or_default()
            );
            let lower = log.to_ascii_lowercase();
            assert!(
                lower.contains("syntaxerror") || lower.contains("was never closed"),
                "the preflight prints the interpreter's own sentence:\n{log}"
            );
            Ok(())
        }
        Err(payload) => Err(payload),
    };
    stop(&mut child);
    result.expect("the reload cycle leg");
    println!("reload: v1 -> v2 through the pinned descriptors, then a rejected save kept v2");
}

/// Opens `path` read-only **without** `O_CLOEXEC`, so the child inherits it.
fn open_inherited(path: &Path) -> i32 {
    let name = CString::new(path.as_os_str().as_bytes()).expect("a path without a NUL byte");
    // SAFETY: `name` is NUL-terminated and outlives the call; the descriptor is
    // closed by the caller.
    let fd = unsafe { libc::open(name.as_ptr(), libc::O_RDONLY) };
    assert!(
        fd >= 0,
        "open {}: {}",
        path.display(),
        std::io::Error::last_os_error()
    );
    fd
}

/// Stage 1 refuses a descriptor whose `(st_dev, st_ino)` moved after stage 0
/// recorded it.
///
/// The descriptor number is real and inherited; the **file** it names was
/// swapped under it with `dup2`, which is exactly the state stage 0's record and
/// stage 1's `fstat` disagree about. The document is written by the real
/// `Config::to_json`, so stage 1 parses what it would parse from stage 0.
#[cfg(feature = "python")]
#[test]
fn stage_1_refuses_a_python_descriptor_whose_identity_moved() {
    let scratch = Scratch::new("swap");
    scratch.write("Blastfile", b"static \"/\" {\n    root \"public\"\n}\n");
    scratch.write("public/index.html", b"unused");
    let inspected = scratch.write("libpython3.14.so.1.0", b"the inspected object");
    let replacement = scratch.write("replacement", b"a different object");
    let (dev, ino) = {
        let meta = std::fs::metadata(&inspected).expect("the inspected object");
        (meta.dev(), meta.ino())
    };

    // The descriptor stage 0 would have pinned, and a second file swapped onto
    // the very same number.
    let pinned = open_inherited(&inspected);
    let swapped = open_inherited(&replacement);
    // SAFETY: both descriptors are open and this process owns them; `dup2`
    // atomically closes `pinned` and makes it name the replacement.
    let rc = unsafe { libc::dup2(swapped, pinned) };
    assert!(rc >= 0, "dup2: {}", std::io::Error::last_os_error());
    // SAFETY: `swapped` is a second descriptor on the same open file description.
    unsafe { libc::close(swapped) };

    let config = Config {
        manifest: scratch.path("Blastfile"),
        manifest_dir: scratch.dir.clone(),
        log_format: LogFormat::Pretty,
        python: Some(PythonHandoff {
            py_minor: 314,
            python_home: scratch.dir.clone(),
            sys_path: Vec::new(),
            lib: (pinned, dev, ino),
            shim: (pinned, dev, ino),
        }),
        ..Config::default()
    };
    let document = scratch.path("config.json");
    std::fs::write(
        &document,
        serde_json::to_vec(&config.to_json()).expect("the document serialises"),
    )
    .expect("the document");
    let config_fd = open_inherited(&document);

    let output = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .args([
            "--stage=1",
            &format!("--config-fd={config_fd}"),
            &format!("--py-lib-fd={pinned}"),
            &format!("--py-shim-fd={pinned}"),
        ])
        .current_dir(&scratch.dir)
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .expect("run stage 1");
    // SAFETY: both are this process's descriptors and the child has exited.
    unsafe {
        libc::close(config_fd);
        libc::close(pinned);
    }

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(
        !output.status.success(),
        "stage 1 must refuse a swapped descriptor:\n{text}"
    );
    assert!(
        text.contains("was replaced after blastit inspected it"),
        "the refusal names the swap:\n{text}"
    );
    assert!(
        text.contains(&format!("inode {ino} is now")),
        "the refusal names both identities:\n{text}"
    );
    assert!(
        text.contains("restore the file and restart blastit"),
        "the refusal names the fix:\n{text}"
    );
    assert!(
        !text.contains("stage 0"),
        "a user-facing refusal never names an internal stage:\n{text}"
    );
    println!(
        "swap: refused (descriptor {pinned} names {}, recorded inode {ino})",
        replacement.display()
    );
}
