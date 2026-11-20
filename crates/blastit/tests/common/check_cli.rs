//! The CLI harness `e2e_check_fmt` drives: one clean environment, one run, and
//! the one `dev` boot the static case needs.
//!
//! It lives here so the suite file is the §5 cases and their fixtures: every
//! helper is a real process invocation or a real loopback request.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::check_suite;
use crate::pg_upstream::PgUpstream;
use crate::project::Project;

/// The binary cargo built for this suite.
pub const BINARY: &str = env!("CARGO_BIN_EXE_blastit");

/// The `blastit dev` environment that keeps a host's free memory out of a
/// static case.
pub const DEV_ENV: &[(&str, &str)] = &[("BLASTIT_MEM_HIGH_WATERMARK", "100")];

/// A command for the real binary in `dir`, with exactly `env` added to a clean
/// environment plus the loader variables the embedded build needs.
pub fn command(binary: &Path, dir: &Path, arguments: &[&str], env: &[(&str, &str)]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .env_clear()
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for (name, value) in env {
        command.env(name, value);
    }
    command
}

/// One run of the suite's own binary.
pub fn run(dir: &Path, arguments: &[&str], env: &[(&str, &str)]) -> Output {
    command(Path::new(BINARY), dir, arguments, env)
        .output()
        .expect("run the blastit binary")
}

/// Runs `blastit`, failing instead of hanging when it does not exit.
pub fn run_bounded(dir: &Path, arguments: &[&str], env: &[(&str, &str)]) -> Output {
    let mut child = command(Path::new(BINARY), dir, arguments, env)
        .spawn()
        .expect("spawn the blastit binary");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().expect("poll the blastit binary").is_some() {
            return child
                .wait_with_output()
                .expect("collect the blastit output");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("`blastit {}` did not exit within 20s", arguments.join(" "));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn stderr(output: &Output) -> String {
    text(&output.stderr)
}

pub fn document(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not one JSON document ({error}): {:?} / stderr: {}",
            text(&output.stdout),
            stderr(output)
        )
    })
}

/// A fresh project with a `tmp` directory `check` uses as its `TMPDIR`.
pub fn project(tag: &str) -> Project {
    let project = Project::new(tag);
    std::fs::create_dir_all(project.path("tmp")).expect("the fixture TMPDIR");
    project
}

/// A scriptable suite in `project`, returning the shim `BLASTIT_PYTHON` names.
pub fn suite(project: &Project, manifest: &str, spec: Value) -> PathBuf {
    check_suite::fixture(&project.dir, manifest, &spec)
}

/// The PostgreSQL URL `check` hands dbproxy and the suite.
pub fn pg_url(server: &PgUpstream) -> String {
    format!(
        "postgresql://real-user:s3cr3t@127.0.0.1:{}/app?sslmode=disable",
        server.port()
    )
}

/// The child environment of a store-less `check` fixture.
pub fn env_of<'a>(shim: &'a Path, tmp: &'a Path) -> [(&'a str, &'a str); 2] {
    [
        ("BLASTIT_PYTHON", shim.to_str().expect("a UTF-8 shim path")),
        ("TMPDIR", tmp.to_str().expect("a UTF-8 fixture path")),
    ]
}

/// The child environment of a `check` fixture with a PostgreSQL peer.
pub fn env_pg<'a>(shim: &'a Path, tmp: &'a Path, url: &'a str) -> [(&'a str, &'a str); 3] {
    [
        ("BLASTIT_PYTHON", shim.to_str().expect("a UTF-8 shim path")),
        ("DATABASE_URL", url),
        ("TMPDIR", tmp.to_str().expect("a UTF-8 fixture path")),
    ]
}

/// A `static` file whose `public/` holds one page.
pub fn write_static_root(dir: &Path) {
    std::fs::create_dir_all(dir.join("public")).expect("the static root");
    std::fs::write(dir.join("public/index.html"), "<h1>static</h1>\n").expect("the index");
}

/// One `GET` that returns the status as soon as the boot answers.
fn get(port: u16, path: &str) -> Option<u16> {
    use std::io::{Read, Write};

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    String::from_utf8_lossy(&raw)
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// A port the kernel just handed out, released before the child binds it.
fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("a free loopback port")
        .local_addr()
        .expect("the bound address")
        .port()
}

/// Starts `blastit dev` in `dir`, waits for `GET /` to answer `200`, and stops
/// it; a port another process took between [`free_port`] and the child's bind is
/// retried, like every other dev fixture.
pub fn dev_serves(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let tmp = dir.join("tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    let mut last = None;
    for attempt in 1..=3 {
        let port = free_port();
        let mut child = command(
            Path::new(BINARY),
            dir,
            &["dev", "--no-reload", "--port", &port.to_string()],
            &[
                DEV_ENV[0],
                ("TMPDIR", tmp.to_str().expect("a UTF-8 fixture path")),
            ],
        )
        .spawn()
        .expect("spawn `blastit dev`");
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut served = false;
        while Instant::now() < deadline {
            if get(port, "/") == Some(200) {
                served = true;
                break;
            }
            if child.try_wait().expect("poll `blastit dev`").is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let output = child.wait_with_output().expect("collect `blastit dev`");
        if served {
            return;
        }
        eprintln!(
            "blastit dev[{dir:?}]: attempt {attempt}/3 on port {port} did not serve; retrying"
        );
        last = Some(output);
    }
    let output = last.expect("three attempts ran");
    panic!(
        "blastit dev never served `GET /` in {dir:?}\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}
