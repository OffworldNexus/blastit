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
//! The `[route.php]` case (`MB.3.13.3`) is the third: the capability gate must
//! stay silent (a PHP route needs no interpreter), and the boot is refused by
//! the `[route.php]` discovery gate for the library this build does not bundle
//! — never by `app_rule`, and never by installing Python.
#![cfg(not(feature = "python"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One view, no `static` mount, no app: a WSGI view, which this build cannot
/// fork.
const WSGI_ONLY: &str = "\
view web regex '.*' {}
";

/// A file that still asks for the external `php-fpm` pool Blastit deleted
/// (`MB.3.17`): `fastcgi` has no Blastfile spelling, so it is an ordinary
/// unknown keyword and no build — this one included — boots the file.
const RETIRED_EXTERNAL_PHP: &str = "\
fastcgi \"public/index.php\"
";

/// One `entrypoint php` and no app: a PHP file is its own executor (`ADR-0010`
/// §2), so **this** gate must not fire. The boot is refused by the separate PHP
/// discovery gate when the host has no usable NTS `libphp` (the case below pins
/// one that cannot be a library); `dev` therefore says that sentence, never the
/// capability gate's.
const PHP_ONLY: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

/// The sentence the capability gate speaks when it refuses: this build has no
/// interpreter, so it must never say it about a PHP route.
const CAPABILITY_GATE: &str = "this build has no Python support (built without feature \"python\")";

/// The sentence that names the artifact which *can* serve a WSGI manifest.
const INSTALL: &str = "install the Python package: uv add blastit";

/// `app_rule`'s pre-existing refusal, which the capability gate must get in
/// front of: on this build "set `BLASTIT_APP`" is not a fix.
const APP_RULE: &str = "BLASTIT_APP: the WSGI callable (`module:attribute`) is required";

/// A temp directory holding one case's `Blastfile`, removed on drop.
struct Scratch {
    /// The directory `blastit dev` is started in.
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str, manifest: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("blastit-mb311-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut file = std::fs::File::create(dir.join("Blastfile")).unwrap();
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
///
/// `env` is applied on top of an empty environment, which is how the PHP case
/// pins a `BLASTIT_LIBPHP` that cannot be a library — the boot must refuse it
/// before the held port or a fork is reached.
fn dev(dir: &Path, env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port().to_string();
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(["dev", "--port", &port])
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
    let (code, stdout, stderr) = dev(&scratch.dir, &[]);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert!(stderr.contains("view \"web\""), "{stderr}");
    assert!(
        stderr.contains("this build has no Python support (built without feature \"python\")"),
        "{stderr}"
    );
    assert!(stderr.contains(INSTALL), "{stderr}");
    assert!(
        stderr.contains("`static` mount") && stderr.contains("`entrypoint php`"),
        "the fix names the file's own executors: {stderr}"
    );
    assert!(
        !stderr.contains(APP_RULE),
        "the capability gate answered, not `app_rule`: {stderr}"
    );
}

#[test]
fn a_manifest_asking_for_the_retired_external_php_pool_is_refused_by_name() {
    let scratch = Scratch::new("fastcgi", RETIRED_EXTERNAL_PHP);
    let (code, stdout, stderr) = dev(&scratch.dir, &[]);
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert!(
        stderr.contains("`fastcgi` is not a keyword"),
        "the refusal is the ordinary unknown-keyword one, and it names the key: {stderr}"
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
/// This build does embed PHP (`--features php`), so the PHP file is not refused
/// for the missing interpreter — it is refused by the PHP discovery gate,
/// because `BLASTIT_LIBPHP` here is a text file and no NTS `libphp` is bundled
/// (`ADR-0010` §1: one backend, no fallback). What is asserted is
/// *which* sentence was printed and which was not: a capability gate that came
/// back would name the wrong artifact, and `app_rule` would send a PHP developer
/// to set `BLASTIT_APP`. The WSGI case above stays the denied twin: same
/// harness, same held port, capability gate's sentence. The served case — a host
/// with a usable library — is `critical_php_backend.rs`'s.
#[test]
fn a_php_only_manifest_is_refused_by_the_php_discovery_gate() {
    let scratch = Scratch::new("php", PHP_ONLY);
    let not_a_library = scratch.dir.join("libphp.so");
    std::fs::write(&not_a_library, b"not an ELF object").unwrap();
    let (code, stdout, stderr) = dev(
        &scratch.dir,
        &[("BLASTIT_LIBPHP", &not_a_library.to_string_lossy())],
    );
    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert!(stderr.contains("route \"app\""), "{stderr}");
    assert!(
        stderr.contains("entrypoint php \"public/index.php\""),
        "{stderr}"
    );
    assert!(
        stderr.contains("install the PHP embed package"),
        "the discovery gate named the package: {stderr}"
    );
    let all = format!("{stdout}{stderr}");
    assert!(
        !all.contains(CAPABILITY_GATE),
        "a PHP route needs no interpreter, so the capability gate must not fire: {all}"
    );
    assert!(
        !all.contains(INSTALL),
        "no Python package can serve an entrypoint php: {all}"
    );
    assert!(
        !all.contains(APP_RULE),
        "a PHP route is its own executor: {all}"
    );
}
