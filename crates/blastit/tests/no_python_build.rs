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
//!
//! The `[route.php]` case (`MB.3.28`) is the third: the capability gate must
//! stay silent (a PHP route needs no interpreter), but the boot is refused by
//! the separate interim guard that names the missing worker wiring — not by
//! `app_rule`, and not by installing Python.
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
/// (`MB.3.17`): the key has no alias, so it is an ordinary unknown route key and
/// no build — this one included — boots the manifest.
const RETIRED_EXTERNAL_PHP: &str = "\
version = 1

[[route]]
name = \"php\"
path = ['.*']

[route.fastcgi]
entry = \"public/index.php\"
";

/// One `[route.php]` route and no app: an embedded-PHP route is its own
/// executor (`ADR-0010` §2), so **this** gate must not fire. The boot itself is
/// refused by stage 0's separate interim guard (`MB.3.28`) until `MB.3.13.3`
/// wires `SpawnSpec::Php`; `dev` therefore says the PHP sentence, never the
/// capability gate's.
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

/// `MB.3.28`'s interim refusal: `[route.php]` has no worker in this build yet
/// (`MB.3.13.3` wires it), so the boot fails closed rather than reaching
/// `Ctl::Spawn`.
const PHP_GATE: &str = "embedded PHP backend is not available in this build";

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
        stderr.contains("grant on unknown proxy \"fastcgi\""),
        "the refusal is the ordinary unknown-route-key one, and it names the key: {stderr}"
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

/// A PHP-only manifest is **not** a WSGI manifest: the capability gate must not
/// fire and nothing may suggest installing Python (`MB.3.5.4`, `ADR-0010` §2).
///
/// The boot is still refused, now by `MB.3.28`'s interim stage-0 guard: this
/// build has no PHP worker to spawn, so refusing before a listener beats
/// reaching `Ctl::Spawn` and taking the container down. What is asserted is
/// *which* sentence was printed and which was not — a capability gate that came
/// back would name the wrong artifact, and `app_rule` would send a PHP
/// developer to set `BLASTIT_APP`. The WSGI case above stays the denied twin:
/// same harness, same held port, capability gate's sentence.
#[test]
fn a_php_only_manifest_is_refused_by_the_interim_php_gate() {
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
    assert!(stderr.contains(PHP_GATE), "{stderr}");
    let all = format!("{stdout}{stderr}");
    assert!(
        !all.contains(CAPABILITY_GATE),
        "a PHP route needs no interpreter, so the capability gate must not fire: {all}"
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
