//! `MB.3.28` interim: a `[route.php]` route is refused at boot until the
//! composition root can spawn an embedded-PHP worker (`MB.3.13.3`).
//!
//! `PhpEmbedRuntime` and `fork_php_view` exist (`MB.3.13.1`), but `Ctl::Spawn`
//! has no PHP arm. Without the stage-0 guard a `[route.php]` manifest is
//! accepted and then either takes the container down at the first spawn (the
//! standalone build) or forks a **WSGI** view for the PHP route (the wheel
//! build). This suite holds down the refusal and, just as important, the
//! *absence* of the WSGI surface: a test that only checked exit `1` would pass
//! on the old boot too, because the held port is what fails it.
//!
//! `blastit check` is the second entry point into the same preflight: it must
//! refuse the manifest as a configuration error (exit `2`) before pytest can
//! exercise a route this build cannot serve.
//!
//! The allowed twins are asserted beside it: a static-only manifest is not
//! refused by this gate (its boot reaches the bind the held port controls), and
//! a `[route.static]` + `[route.php]` page — the documented shape — is refused
//! by the same sentence as a bare `[route.php]`.
//!
//! **Interim.** `MB.3.13.3` wires `SpawnSpec::Php`, deletes the refusal in
//! `stage0.rs`, and rewrites this file to assert a PHP route boots and serves
//! `200` through `view.chan` (authorized with `critical_guard --node
//! MB.3.13.3`). Until then `make rs-test-critical` runs it in the
//! `python` configuration and `make rs-no-python`'s suites cover the
//! standalone one; the guard is compiled in both builds.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The sentence that names the missing wiring, not a missing package.
const SENTENCE: &str = "embedded PHP backend is not available in this build";

/// One `[route.php]` route and no app: refused until the worker is wired.
const PHP_ONLY: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"
";

/// The documented page shape: files from disk, the rest to the front
/// controller. Also refused, by the same sentence.
const STATIC_AND_PHP: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.static]
root = \"public\"

[route.php]
entry = \"public/index.php\"
";

/// The allowed twin: no PHP route, so this gate has nothing to say.
const STATIC_ONLY: &str = "\
version = 1

[[route]]
name = \"docs\"
path = ['.*']

[route.static]
root = \"site\"
";

/// A temp directory holding one case's `blastit.toml`, removed on drop.
struct Scratch {
    /// The directory `blastit dev` is started in.
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str, manifest: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("blastit-mb328-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut file = std::fs::File::create(dir.join("blastit.toml")).unwrap();
        file.write_all(manifest.as_bytes()).unwrap();
        Self { dir }
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

/// Runs `blastit dev` in `dir` and returns `(exit code, stdout, stderr)`.
///
/// The port is one this test holds open: a boot that got past the gate cannot
/// bind it, so a guard that went missing fails here instead of serving forever
/// and hanging the suite. The wait is bounded either way.
fn dev(dir: &Path) -> (Option<i32>, String, String) {
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port().to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .args(["dev", "--port", &port])
        .env_clear()
        // A loaded host must not turn this into the memory-watermark refusal.
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "`blastit dev` did not refuse within 15 s: the boot gate never ran"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Runs `blastit check` in `dir` and returns `(exit code, stdout, stderr)`.
///
/// The wait is bounded: the guard must answer before `check` reaches pytest,
/// so a regression that lets the manifest through fails here instead of hanging
/// the suite.
fn check(dir: &Path) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_blastit"))
        .arg("check")
        .env_clear()
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "`blastit check` did not refuse within 15 s: the guard never ran"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// One PHP backend, one sentence (`ADR-0010` §1): the refusal must not name an
/// alternative transport — even to deny it — because that reads as a fallback
/// recipe to an agent.
fn assert_no_fallback_recipe(text: &str) {
    let lower = text.to_ascii_lowercase();
    for banned in [
        "php-fpm",
        "fastcgi",
        "route.fastcgi",
        "wsgi",
        "meanwhile",
        "libphp",
    ] {
        assert!(
            !lower.contains(banned),
            "the refusal names a fallback recipe ({banned}): {text}"
        );
    }
}

/// A boot that served the PHP route would print the dev banner and, at the
/// first spawn, a `view …` line. Neither may exist: the refusal is before the
/// listener and before any fork.
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

#[test]
fn a_php_route_is_refused_at_boot_and_never_reaches_a_wsgi_view() {
    let scratch = Scratch::new("php", PHP_ONLY);
    let (code, stdout, stderr) = dev(&scratch.dir);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert!(stderr.contains("route \"app\""), "{stderr}");
    assert!(
        stderr.contains("[route.php] entry \"public/index.php\""),
        "{stderr}"
    );
    assert!(stderr.contains(SENTENCE), "{stderr}");
    assert!(
        stderr.contains("MB.3.13.3"),
        "the sentence names where the wiring lands: {stderr}"
    );
    assert_no_fallback_recipe(&stderr);
    assert_no_wsgi_surface(&stdout, &stderr);
}

/// `blastit check` is a second entry point into the same preflight: it must
/// refuse the manifest as a configuration error (exit `2`, empty stdout) before
/// pytest can exercise a route this build cannot serve. A check that reached
/// the suite would mis-route exactly like a boot would.
#[test]
fn check_refuses_a_php_route_before_anything_is_served() {
    let scratch = Scratch::new("check", PHP_ONLY);
    let (code, stdout, stderr) = check(&scratch.dir);
    assert_eq!(code, Some(2), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a configuration error writes nothing on stdout (§1.1)"
    );
    assert!(stderr.contains("blastit check: "), "{stderr}");
    assert!(stderr.contains("route \"app\""), "{stderr}");
    assert!(
        stderr.contains("[route.php] entry \"public/index.php\""),
        "{stderr}"
    );
    assert!(stderr.contains(SENTENCE), "{stderr}");
    assert!(
        stderr.contains("MB.3.13.3"),
        "the sentence names where the wiring lands: {stderr}"
    );
    assert_no_fallback_recipe(&stderr);
    assert_no_wsgi_surface(&stdout, &stderr);
}

#[test]
fn a_static_and_php_manifest_is_refused_by_the_same_sentence() {
    let scratch = Scratch::new("page", STATIC_AND_PHP).with_static_root("public");
    let (code, stdout, stderr) = dev(&scratch.dir);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert!(stderr.contains("route \"app\""), "{stderr}");
    assert!(
        stderr.contains("[route.php] entry \"public/index.php\""),
        "{stderr}"
    );
    assert!(stderr.contains(SENTENCE), "{stderr}");
    assert!(
        stderr.contains("MB.3.13.3"),
        "the sentence names where the wiring lands: {stderr}"
    );
    assert_no_fallback_recipe(&stderr);
    assert_no_wsgi_surface(&stdout, &stderr);
}

/// The allowed twin: no PHP route, so the gate stays silent and the boot runs
/// on to the bind — which the held port makes fail for a reason this test
/// controls. The banner proves it got past the gate; the sentence must not be
/// there.
#[test]
fn a_static_only_manifest_is_not_refused_by_this_gate() {
    let scratch = Scratch::new("static", STATIC_ONLY).with_static_root("site");
    let (code, stdout, stderr) = dev(&scratch.dir);
    assert_eq!(
        code,
        Some(1),
        "the held port is what refuses this boot: stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(
        stdout.contains("1 route"),
        "the boot reached its banner with the static route counted: {stdout}"
    );
    let all = format!("{stdout}{stderr}");
    assert!(
        !all.contains(SENTENCE),
        "a static-only manifest has no PHP route to refuse: {all}"
    );
}
