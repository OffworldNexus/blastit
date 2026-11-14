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
//! gate the WSGI manifest below still exits `1` — `app_rule` refuses it with
//! "BLASTIT_APP: the WSGI callable (`module:attribute`) is required" — and that
//! is advice no operator of this artifact can act on. Each case therefore
//! asserts the gate's own sentence *and* the absence of that one, so deleting
//! either call site fails here instead of passing on an exit code both paths
//! happen to return.
//!
//! The retired external-PHP key (`MB.3.17`) is the second case: this build must
//! still refuse it at exit `1`, but the sentence is now the **parser's** — the
//! key stops existing, so the manifest is refused as an unknown key that names
//! it, before the capability gate is consulted at all.
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

/// One route that still asks for the external `php-fpm` pool Blastit deleted
/// (`MB.3.17`): the key is retired, so no build — this one included — parses it.
const RETIRED_EXTERNAL_PHP: &str = "\
version = 1

[[route]]
name = \"php\"
path = ['.*']

[route.fastcgi]
entry = \"public/index.php\"
";

/// One `[route.php]` route and no app: an embedded-PHP route is its own
/// executor (`ADR-0010` §2), so this build carries everything it needs and the
/// capability gate must not fire. The fixture became expressible in
/// `MB.3.5.4`, which is exactly why the refusal it used to get went unnoticed.
const PHP_ONLY: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"
";

/// The sentence the capability gate speaks when it refuses: this build has no
/// interpreter, so it must never say it about a PHP route.
const CAPABILITY_GATE: &str = "this build has no Python support (built without feature \"python\")";

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
fn a_manifest_asking_for_the_retired_external_php_pool_is_refused_by_name() {
    let scratch = Scratch::new("fastcgi", RETIRED_EXTERNAL_PHP);
    let (code, stdout, stderr) = dev(&scratch.dir);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert!(
        stderr.contains("route[0].fastcgi: unknown key"),
        "the refusal names the retired key: {stderr}"
    );
    assert!(
        !stderr.contains(INSTALL),
        "installing the wheel cannot bring the key back: {stderr}"
    );
    assert!(
        !stderr.contains(APP_RULE),
        "the parser answered, not `app_rule`: {stderr}"
    );
}

/// A PHP-only manifest is **not** a WSGI manifest: this build serves it, so the
/// capability gate must not fire and nothing may suggest installing Python
/// (`MB.3.5.4`, `ADR-0010` §2).
///
/// The boot still fails here, and for a reason this file controls: the test
/// holds the port open, so a boot that got past both gates cannot bind it. That
/// is the point — what is asserted is *which* sentence was **not** printed, so a
/// gate that came back fails instead of passing on an exit code every path
/// happens to return. The WSGI case above stays the denied twin: same harness,
/// same held port, gate's sentence.
#[test]
fn a_php_only_manifest_is_not_refused_by_the_capability_gate() {
    let scratch = Scratch::new("php", PHP_ONLY);
    let (code, stdout, stderr) = dev(&scratch.dir);
    assert_eq!(
        code,
        Some(1),
        "the held port is what refuses this boot: stdout={stdout:?} stderr={stderr:?}"
    );
    // Unlike the two refusals above, this boot gets **past** the banner: the
    // route is counted (`1 route`) and the failure is the bind, so the events
    // land on stdout. That is the proof the gates let it through.
    assert!(
        stdout.contains("1 route"),
        "the boot reached its banner with the PHP route counted: {stdout}"
    );
    let all = format!("{stdout}{stderr}");
    assert!(
        !all.contains(CAPABILITY_GATE),
        "a PHP route needs no interpreter, so the gate must not fire: {all}"
    );
    assert!(
        !all.contains(INSTALL),
        "no Python package can serve a [route.php] route: {all}"
    );
    assert!(
        !all.contains(APP_RULE),
        "a PHP route is its own executor: {all}"
    );
}
