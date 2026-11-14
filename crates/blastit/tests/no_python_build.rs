//! `MB.3.11`: the interpreter-free build refuses a manifest that needs a runtime
//! it does not carry, in stage 0, before a listener is bound (`ADR-0009` §5).
//!
//! This file is compiled **only** in the configuration `make rs-no-python`
//! builds (`--no-default-features --features php`); there `CARGO_BIN_EXE_blastit`
//! *is* the python-less binary, so the suite runs the artifact rather than a
//! function. In the wheel build it is empty on purpose: a WSGI boot is that
//! build's normal path and has its own suites.
//!
//! What it holds down is the **wiring**, not only the words. Without the stage-0
//! gate both manifests below still exit `1` — `app_rule` refuses them with
//! "BLASTIT_APP: the WSGI callable (`module:attribute`) is required" — and that
//! is advice no operator of this artifact can act on. Each case therefore
//! asserts the gate's own sentence *and* the absence of that one, so deleting
//! either call site fails here instead of passing on an exit code both paths
//! happen to return.
#![cfg(not(feature = "python"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One route, no `[route.static]` block, no app: a WSGI view, which this build
/// cannot fork.
const WSGI_ONLY: &str = "\
version = 1

[[route]]
name = \"web\"
path = ['.*']
";

/// One route served by an operator-managed `php-fpm` pool: an external executor
/// this build does not carry either.
const EXTERNAL_FASTCGI: &str = "\
version = 1

[[route]]
name = \"php\"
path = ['.*']

[route.fastcgi]
entry = \"public/index.php\"
";

/// The sentence that names the artifact which *can* serve a WSGI manifest.
const INSTALL: &str = "install the Python package: uv add blastit";

/// `app_rule`'s pre-existing refusal, which the capability gate must get in
/// front of: on this build "set `BLASTIT_APP`" is not a fix.
const APP_RULE: &str = "BLASTIT_APP: the WSGI callable (`module:attribute`) is required";

/// A temp directory holding one case's `blastit.toml`, removed on drop.
struct Scratch {
    /// The directory `blastit dev` is started in.
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str, manifest: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("blastit-mb311-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut file = std::fs::File::create(dir.join("blastit.toml")).unwrap();
        file.write_all(manifest.as_bytes()).unwrap();
        Self { dir }
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
/// bind it, so a gate that went missing fails the assertion instead of serving
/// forever and hanging the suite. The wait is bounded either way.
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

#[test]
fn a_wsgi_manifest_is_refused_by_the_capability_gate() {
    let scratch = Scratch::new("wsgi", WSGI_ONLY);
    let (code, stdout, stderr) = dev(&scratch.dir);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert!(stderr.contains("route \"web\""), "{stderr}");
    assert!(
        stderr.contains("this build has no Python support (built without feature \"python\")"),
        "{stderr}"
    );
    assert!(stderr.contains(INSTALL), "{stderr}");
    assert!(
        !stderr.contains(APP_RULE),
        "the capability gate answered, not `app_rule`: {stderr}"
    );
}

#[test]
fn an_external_fastcgi_manifest_is_refused_on_its_own_terms() {
    let scratch = Scratch::new("fastcgi", EXTERNAL_FASTCGI);
    let (code, stdout, stderr) = dev(&scratch.dir);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert!(
        stderr.contains("route \"php\": this build cannot serve an external [route.fastcgi] route"),
        "{stderr}"
    );
    assert!(stderr.contains("MB.3.13's wiring"), "{stderr}");
    assert!(
        stderr.contains("BLASTIT_ALLOW_EXTERNAL_FASTCGI is the gate it will arrive behind"),
        "{stderr}"
    );
    assert!(
        !stderr.contains(INSTALL),
        "installing the wheel cannot reach an operator-managed pool: {stderr}"
    );
    assert!(
        !stderr.contains(APP_RULE),
        "the capability gate answered, not `app_rule`: {stderr}"
    );
}
