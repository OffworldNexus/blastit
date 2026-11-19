//! `MB.40.8`: a standalone `entrypoint python` boot imports the project's
//! `.venv` `site-packages`, and a module that exists **only** there is served.
//!
//! Stage 1 embeds the binary's own CPython, which knows nothing about a project:
//! stage 0 discovers `<manifest_dir>/.venv`, puts its `site-packages` into the
//! configuration's `sys_path`, and stage 1's `Embed` branch has to restore those
//! entries *in front of* the base interpreter's purelib. This leg proves the
//! last hop for real over a live `blastit dev`: `venv_only_mod.py` lives nowhere
//! but inside the venv, so a boot that never restored the path cannot answer
//! `200`.

#![cfg(feature = "python")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long a `dev` boot may take to answer before the leg gives up.
const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a `SIGTERM` may take to end the boot before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// The CPython `major.minor` this build embeds.
///
/// The build script records it (`BLASTIT_PYTHON_MM`); a test crate compiled
/// without it asks the interpreter `PYO3_PYTHON` names, which is the same one.
fn embedded_mm() -> Option<String> {
    if let Some(mm) = option_env!("BLASTIT_PYTHON_MM") {
        return Some(mm.to_string());
    }
    let python = std::env::var_os("PYO3_PYTHON")?;
    let output = Command::new(python)
        .arg("-c")
        .arg("import sys; print(f'{sys.version_info[0]}.{sys.version_info[1]}')")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mm = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!mm.is_empty()).then_some(mm)
}

/// A directory with a name of its own, removed when dropped.
struct Project {
    path: std::path::PathBuf,
}

impl Project {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("blastit-embed-venv-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the project directory");
        Self { path }
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.path.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the parent directory");
        std::fs::write(&path, bytes).expect("a fixture file");
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A port nothing listens on right now.
fn free_port() -> Option<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(port)
}

/// One `GET` once the boot answers, or `None` while it does not.
fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let request = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = Vec::new();
    std::io::Read::read_to_end(&mut stream, &mut response).ok()?;
    Some(String::from_utf8_lossy(&response).into_owned())
}

/// Waits for the boot to answer a health request, then fetches `path`.
fn wait_for(port: u16, path: &str, child: &mut Child) -> String {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(response) = get(port, path) {
            return response;
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!("blastit dev exited before it answered: {status}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("blastit dev did not answer 127.0.0.1:{port} within {READY_TIMEOUT:?}");
}

/// Ends the boot with `SIGTERM`, then with `SIGKILL` if it does not leave.
fn stop(child: &mut Child) {
    let pid = i32::try_from(child.id()).unwrap_or_default();
    // SAFETY: `pid` is this test's own child, and `SIGTERM` is a valid signal.
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let deadline = Instant::now() + STOP_TIMEOUT;
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
fn a_module_that_exists_only_in_the_project_venv_is_served() {
    let Some(mm) = embedded_mm() else {
        // A `python`-less build has no venv to match; the boot gate refuses the
        // manifest before this leg could run, and `rs-no-python` never compiles
        // this target.
        return;
    };
    // Design §2.1: a `dev` boot now resolves and pins the interpreter, the
    // shared `libpython` and the per-minor shim before it serves. This leg needs
    // the same three things `make py-sapi` builds; without them the refusal is
    // the discovery's own sentence, not this test's subject.
    let Some(interpreter) = std::env::var_os("PYO3_PYTHON").map(std::path::PathBuf::from) else {
        eprintln!(
            "SKIPPED a_module_that_exists_only_in_the_project_venv_is_served: PYO3_PYTHON is \
             unset, so no project interpreter can be linked; run it through make (RUST_ENV)"
        );
        return;
    };
    let interpreter = std::fs::canonicalize(&interpreter).expect("the PYO3_PYTHON interpreter");
    let shims = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/py-sapi")
        .canonicalize()
        .ok()
        .filter(|dir| dir.is_dir());
    let Some(_shims) = shims else {
        eprintln!(
            "SKIPPED a_module_that_exists_only_in_the_project_venv_is_served: target/py-sapi has \
             no shim; run make py-sapi"
        );
        return;
    };
    let home = interpreter
        .parent()
        .expect("the interpreter's bin directory");
    let project = Project::new("only-in-venv");
    let site = format!(".venv/lib/python{mm}/site-packages");
    project.write(
        "Blastfile",
        b"entrypoint python \"app:application\"\n\nview hello regex '/hello/.*' {}\n",
    );
    project.write(
        "app.py",
        b"from venv_only_mod import VALUE\n\n\ndef application(environ, start_response):\n    start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\")])\n    return [VALUE]\n",
    );
    project.write(
        ".venv/pyvenv.cfg",
        format!("home = {}\nversion = {mm}.0\n", home.display()).as_bytes(),
    );
    // The interpreter inside the project's venv: discovery resolves the minor
    // from this name and the prefix from `pyvenv.cfg`'s `home`.
    std::fs::create_dir_all(project.path.join(".venv/bin")).expect("the venv bin directory");
    std::os::unix::fs::symlink(&interpreter, project.path.join(".venv/bin/python3"))
        .expect("the venv interpreter link");
    project.write(
        &format!("{site}/venv_only_mod.py"),
        b"VALUE = b\"hello from the venv site-packages\\n\"\n",
    );
    // The proof the import cannot come from anywhere else: the working
    // directory has no copy, so only `.venv`'s `site-packages` can answer.
    assert!(
        !project.path.join("venv_only_mod.py").exists(),
        "the module must live only inside the venv"
    );

    let binary = env!("CARGO_BIN_EXE_blastit");
    let mut last = None;
    for _ in 0..3 {
        let Some(port) = free_port() else {
            panic!("no free loopback port");
        };
        let mut child = Command::new(binary)
            .args(["dev", "--no-reload", "--port", &port.to_string()])
            .current_dir(&project.path)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn blastit dev");
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            wait_for(port, "/hello/", &mut child)
        })) {
            Ok(response) => {
                stop(&mut child);
                assert!(
                    response.starts_with("HTTP/1.1 200"),
                    "the venv-only module was not imported:\n{response}"
                );
                assert!(
                    response.contains("hello from the venv site-packages"),
                    "{response}"
                );
                return;
            }
            Err(panic) => {
                stop(&mut child);
                last = Some(panic);
            }
        }
    }
    if let Some(panic) = last {
        std::panic::resume_unwind(panic);
    }
    panic!("no dev boot answered");
}
