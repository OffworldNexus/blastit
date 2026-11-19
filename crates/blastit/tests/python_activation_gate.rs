//! Design §2.1: a PHP-only or static-only boot never searches for — or opens —
//! `libpython`.
//!
//! The gate is structural: [`blastit::stage0`]'s Python half returns before any
//! search, `open` or `dlopen` when every request is answered by the file's own
//! executors and no Python app is declared. This leg drives a **real** stage 0
//! against an interpreter path that cannot be opened and asserts the boot is
//! unaffected, then the adversarial twin: a Python app with the same bad path is
//! refused with the discovery's own sentence.
//!
//! No `Mock*Server`, no environment seam in production code: a real process, a
//! real temporary project, and the refusal a host would print.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long a boot may take to answer or refuse.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A project directory, removed when dropped.
struct Project {
    path: PathBuf,
}

impl Project {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("blastit-py-gate-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the project directory");
        Self { path }
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.path.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the parent");
        std::fs::write(&path, bytes).expect("a fixture file");
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
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
    Some(String::from_utf8_lossy(&response).into_owned())
}

/// Waits for the boot to answer `path`.
fn wait_for(port: u16, path: &str, child: &mut Child) -> String {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Some(response) = get(port, path) {
            return response;
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!("blastit dev exited before it answered: {status}");
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

#[test]
fn a_php_only_boot_never_reaches_the_python_discovery() {
    // The gate order: the `entrypoint php` library is resolved (and refused)
    // before the Python half runs, so a bad `BLASTIT_PYTHON` must not appear in
    // the sentence at all.
    let project = Project::new("php-gate");
    project.write(
        "Blastfile",
        b"entrypoint php \"public/index.php\"\nview app regex '.*' {}\n",
    );
    project.write("public/index.php", b"<?php echo 'ok';\n");
    let port = free_port();
    let output = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .args(["dev", "--no-reload", "--port", &port.to_string()])
        .current_dir(&project.path)
        .env("BLASTIT_PYTHON", UNREADABLE)
        .env("BLASTIT_LIBPHP", "/definitely/not/a/libphp.so")
        .env_remove("VIRTUAL_ENV")
        .stdin(Stdio::null())
        .output()
        .expect("run blastit dev");
    assert!(
        !output.status.success(),
        "a bad libphp must refuse the boot"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_ascii_lowercase().contains("php"),
        "the PHP gate owns this refusal:\n{stderr}"
    );
    assert!(
        !stderr.contains(UNREADABLE) && !stderr.contains("BLASTIT_PYTHON"),
        "a PHP-only boot never names the Python variable:\n{stderr}"
    );
}

/// The one path that would refuse a Python boot, if the gate did not hold.
const UNREADABLE: &str = "/definitely/not/a/python";

#[test]
fn a_static_only_boot_never_opens_libpython() {
    let project = Project::new("static");
    project.write("Blastfile", b"static \"/\" {\n    root \"public\"\n}\n");
    project.write("public/index.html", b"static and no interpreter\n");
    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .args(["dev", "--no-reload", "--port", &port.to_string()])
        .current_dir(&project.path)
        .env("BLASTIT_PYTHON", UNREADABLE)
        .env("VIRTUAL_ENV", "/nonexistent/venv")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn blastit dev");
    let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for(port, "/index.html", &mut child)
    }));
    stop(&mut child);
    let response = response.expect("the static boot answers");
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "a static boot must not search for an interpreter:\n{response}"
    );
    assert!(response.contains("static and no interpreter"), "{response}");
}

#[cfg(feature = "python")]
#[test]
fn a_python_boot_with_the_same_unreadable_interpreter_is_refused() {
    // The adversarial twin: with a Python app the gate is active, and the
    // unreadable `BLASTIT_PYTHON` is the discovery's refusal (stage 0, before a
    // listener).
    let project = Project::new("python");
    project.write(
        "Blastfile",
        b"entrypoint python \"app:application\"\nview hello regex '/hello/.*' {}\n",
    );
    project.write(
        "app.py",
        b"def application(environ, start_response):\n    return []\n",
    );
    let port = free_port();
    let output = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .args(["dev", "--no-reload", "--port", &port.to_string()])
        .current_dir(&project.path)
        .env("BLASTIT_PYTHON", UNREADABLE)
        .env_remove("VIRTUAL_ENV")
        .stdin(Stdio::null())
        .output()
        .expect("run blastit dev");
    assert!(!output.status.success(), "the boot must be refused");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is not an executable file"),
        "the discovery sentence names the variable and the fix:\n{stderr}"
    );
    assert!(
        stderr.contains(UNREADABLE),
        "the refusal names the path the operator set:\n{stderr}"
    );
}
