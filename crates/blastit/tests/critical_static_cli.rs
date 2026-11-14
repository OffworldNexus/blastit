#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Protected critical suite for the static-surface CLI and boot contract
//! (`R13`, `R14`; `docs/frameworks/static-and-spa.md`, `docs/design/static-and-spa.md`
//! §4.7).
//!
//! Every case pairs the promise with its negative twin, because a suite that
//! only proves the happy path cannot fail:
//!
//! * an all-static manifest with **no** `app` boots under `blastit dev` and
//!   answers a file **and** an SPA fallback, against a mixed manifest without
//!   `app` that still names `BLASTIT_APP`;
//! * a missing static root is named with the manifest and the route before
//!   anything reaches stdout, against a root that is present;
//! * `blastit check` on an all-static manifest audits the root, spawns no
//!   pytest and exits `0` with the unchanged `--json` document, against the
//!   same manifest with the root removed, which exits `2` — so a `check` that
//!   always exited `0` fails here;
//! * a static root the suite never requested is a stderr warning and not an
//!   `unexercised_route` finding, against a plain route that still is one.
//!
//! No case mutates the process environment or its working directory: every
//! child gets an explicit environment and `current_dir`, so the file's cases
//! run in parallel.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use blastit::audit_v1::{self, CheckObsV1, HttpObsLine};
use blastit_manifest::{Manifest, RoutingTable};

/// The one-line marker the fixture's `index.html` carries.
const MARKER: &str = "blastit-static-fixture";

/// How long a `blastit dev` gets to serve its first request.
const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a signalled `blastit dev` gets to leave.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);

/// The manifest the page promises for a site with no app at all.
const ALL_STATIC: &str = r#"version = 1

[[route]]
name = "site"
path = ['.*']

[route.static]
root = "public"
spa = true
"#;

/// The same manifest with one route that can only reach an app: no `app` is
/// declared, so this tree is not all-static and `BLASTIT_APP` is still required.
const MIXED: &str = r#"version = 1

[[route]]
name = "api"
path = ['/api/.*']

[[route]]
name = "site"
path = ['.*']

[route.static]
root = "public"
spa = true
"#;

/// A fixture directory plus the environment every child inherits.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "blastit-mb226-{}-{tag}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("a fresh fixture directory");
        Self { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn write_manifest(&self, content: &str) {
        fs::write(self.dir.join("blastit.toml"), content).expect("write blastit.toml");
    }

    /// The built front end: `public/index.html`, the one file `spa` falls back
    /// to and `/` answers as a directory index.
    fn write_site(&self) {
        write_site(self.path());
    }

    /// A project at `dir` inside this fixture, with `dir`'s mode set.
    ///
    /// Used for the ordinary local shape — a checkout under a `0700` directory
    /// the caller owns — which the cases below must accept.
    fn project(&self, dir: &Path, mode: u32) -> PathBuf {
        fs::create_dir_all(dir).expect("create the project directory");
        set_mode(dir, mode);
        dir.to_path_buf()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A case that deliberately removes a directory's permission puts it back
        // before it asserts, but a failed assertion must not leave an
        // undelatable tree behind.
        for dir in [self.dir.clone(), self.dir.join("home")] {
            if dir.is_dir() {
                set_mode(&dir, 0o755);
            }
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// The built front end in `dir`: `public/index.html` with [`MARKER`] in it.
fn write_site(dir: &Path) {
    let public = dir.join("public");
    fs::create_dir_all(&public).expect("create the static root");
    fs::write(
        public.join("index.html"),
        format!("<!doctype html><h1>{MARKER}</h1>\n"),
    )
    .expect("write the index");
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod a fixture path");
}

/// One `blastit` invocation, in `dir`, with exactly `env` in its environment.
///
/// `env_clear` keeps the suite's own `BLASTIT_*` out of a case that is about
/// what the child reads; the loader variables the embedded interpreter needs
/// travel through [`runtime_env`] because the binary cannot start without them.
fn run(dir: &Path, env: &[(&str, &str)], arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(arguments)
        .env_clear()
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    runtime_env(&mut command);
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().expect("run the blastit binary")
}

/// The loader and interpreter variables a child needs to reach CPython.
fn runtime_env(command: &mut Command) {
    for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

/// Starts `blastit dev` in `dir`, proves it answers, and stops it.
///
/// Returns the port it served on and the child's whole output. A port that
/// another process took between [`free_port`] and the child's `bind` is retried
/// with a fresh one: that race is a property of the host, never of the case.
fn dev_serves(dir: &Path) -> (u16, Output) {
    let mut last = None;
    for _ in 0..3 {
        let (served, port, output) = dev_once(dir);
        if served {
            return (port, output);
        }
        last = Some(output);
    }
    let output = last.expect("three attempts ran");
    panic!(
        "blastit dev never answered GET / within {READY_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn dev_once(dir: &Path) -> (bool, u16, Output) {
    let port = free_port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(["dev", "--port", &port.to_string()])
        .env_clear()
        // The host's free memory must not turn a boot case into the
        // `BLASTIT_MEM_HIGH_WATERMARK` refusal; both are `BLASTIT_*`, so the
        // `execve` barrier strips them again.
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .env("BLASTIT_MAX_VIEW_PROCESSES", "8")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    runtime_env(&mut command);
    let mut child = command.spawn().expect("the blastit binary starts");

    let deadline = Instant::now() + READY_TIMEOUT;
    let mut served = false;
    while Instant::now() < deadline {
        if let Some(response) = get(port, "/")
            && response.starts_with("HTTP/1.1 200")
            && response.contains(MARKER)
        {
            // The same live process must also answer a deep path from `spa`:
            // an index served by the responder, not a bound socket that says
            // nothing.
            served = get(port, "/dashboard/settings").is_some_and(|fallback| {
                fallback.starts_with("HTTP/1.1 200") && fallback.contains(MARKER)
            });
            break;
        }
        if child.try_wait().expect("the child's exit status").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    stop(&mut child);
    (
        served,
        port,
        child
            .wait_with_output()
            .expect("collect the child's output"),
    )
}

/// `SIGTERM`, then a bounded wait; `SIGKILL` if it did not leave.
fn stop(child: &mut Child) {
    // SAFETY: the pid came from `spawn`; `kill` takes two integers.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let deadline = Instant::now() + EXIT_TIMEOUT;
    while Instant::now() < deadline {
        if child.try_wait().expect("the child's exit status").is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    listener.local_addr().expect("a bound address").port()
}

/// One HTTP/1.1 request answered in full, or `None` while the server is not up.
fn get(port: u16, target: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

// ---------------------------------------------------------------------------
// `blastit dev`: the app-optional rule (§4.7 steps 2–5)
// ---------------------------------------------------------------------------

#[test]
fn an_all_static_site_boots_under_dev_with_no_app() {
    let fixture = Fixture::new("dev-static");
    fixture.write_manifest(ALL_STATIC);
    fixture.write_site();

    let (port, output) = dev_serves(fixture.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("BLASTIT_APP"),
        "an all-static manifest needs no app: {stderr}"
    );
    // The documented banner, unchanged and complete: a static route is a route
    // and is counted (`docs/design/static-and-spa.md` §1.2,
    // `docs/frameworks/static-and-spa.md` §Run it).
    let lines = [
        format!("  blastit dev · single-uid simulation · 127.0.0.1:{port}"),
        "  Not a sandbox. Production `blastit` refuses to run like this.".to_string(),
        "  http · scheduler · dbproxy · 1 route".to_string(),
    ];
    for line in &lines {
        assert!(
            stdout.contains(line.as_str()),
            "{line} missing from: {stdout}"
        );
    }
}

#[test]
fn a_mixed_tree_without_an_app_still_names_blastit_app() {
    let fixture = Fixture::new("dev-mixed");
    fixture.write_manifest(MIXED);
    // Deliberately no `public/`: the missing-app refusal is step 3 and the root
    // audit is step 4, so the app is named first even when both would fail.
    let output = run(
        fixture.path(),
        &[("BLASTIT_MEM_HIGH_WATERMARK", "100")],
        &["dev"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "a startup error writes nothing to stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BLASTIT_APP: the WSGI callable (`module:attribute`) is required"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("static root"),
        "the app rule comes before the root audit: {stderr}"
    );
}

#[test]
fn a_missing_static_root_is_named_with_its_route_before_any_output() {
    let fixture = Fixture::new("dev-missing-root");
    fixture.write_manifest(ALL_STATIC);

    let output = run(
        fixture.path(),
        &[("BLASTIT_MEM_HIGH_WATERMARK", "100")],
        &["dev"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "the banner is printed only once the boot is known-good: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in [
        "blastit.toml",
        "route \"site\"",
        "static root \"public\"",
        "no such file or directory",
    ] {
        assert!(
            stderr.contains(expected),
            "{expected} missing from: {stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// `blastit check`: the all-static path (§4.7 step 3, `R14`)
// ---------------------------------------------------------------------------

#[test]
fn an_all_static_check_audits_the_root_and_runs_no_pytest() {
    let fixture = Fixture::new("check-static");
    fixture.write_manifest(ALL_STATIC);
    fixture.write_site();

    // No `PATH`, so a run that reached `python_program`/`require_pytest` would
    // fail with exit `2` instead of auditing the root.
    let output = run(fixture.path(), &[], &["check", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("pytest"),
        "an all-static manifest runs no suite: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("one JSON document");
    let mut keys: Vec<&str> = document
        .as_object()
        .expect("the report is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "findings",
            "ok",
            "proxies",
            "requests",
            "routes",
            "schema_version",
            "statements"
        ],
        "the v1 document keeps its exact shape: {document}"
    );
    assert_eq!(document["schema_version"], 1, "{document}");
    assert_eq!(document["ok"], true, "{document}");
    assert_eq!(document["requests"], 0, "{document}");
    assert_eq!(document["statements"], 0, "{document}");
    assert_eq!(document["proxies"], serde_json::json!({}), "{document}");
    assert_eq!(document["findings"], serde_json::json!([]), "{document}");
    assert_eq!(document["routes"]["total"], 1, "{document}");
    assert_eq!(document["routes"]["exercised"], 0, "{document}");

    // The human form prints nothing at all when every root is audited.
    let human = run(fixture.path(), &[], &["check"]);
    assert_eq!(human.status.code(), Some(0), "{human:?}");
    assert!(
        human.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&human.stdout)
    );
}

#[test]
fn an_all_static_check_exits_two_on_a_missing_root() {
    let fixture = Fixture::new("check-missing-root");
    fixture.write_manifest(ALL_STATIC);

    for arguments in [&["check", "--json"][..], &["check"][..]] {
        let output = run(fixture.path(), &[], arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        assert!(
            output.stdout.is_empty(),
            "{arguments:?}: stdout is reserved for the one document: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Design §7.2: the path, the route and the root, in that order of
        // specificity.
        for expected in ["blastit.toml", "route \"site\"", "static root \"public\""] {
            assert!(
                stderr.contains(expected),
                "{arguments:?}: {expected} missing from: {stderr}"
            );
        }
    }
}

/// `blastit check` never demanded an `app` and must not start now: the suite
/// brings its own WSGI callable, so a non-all-static manifest with no `app` in
/// the file and no `BLASTIT_APP` in the environment reaches the suite path.
///
/// With no `PATH` the run stops at "no interpreter to run pytest on" — which is
/// the proof it got past the app rule — instead of naming `BLASTIT_APP`.
#[test]
fn check_without_an_app_still_runs_the_suite() {
    let fixture = Fixture::new("check-no-app");
    fixture.write_manifest(MIXED);
    fixture.write_site();

    let output = run(fixture.path(), &[], &["check"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("BLASTIT_APP"),
        "`check` does not require an app: {stderr}"
    );
    assert!(
        stderr.contains("no interpreter to run pytest on"),
        "the run must reach the suite preflight: {stderr}"
    );
}

#[test]
fn an_all_static_check_refuses_learn() {
    let fixture = Fixture::new("check-learn");
    fixture.write_manifest(ALL_STATIC);
    fixture.write_site();

    let output = run(fixture.path(), &[], &["check", "--learn"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nothing to learn: every route is static"),
        "{stderr}"
    );
}

// ---------------------------------------------------------------------------
// §7.2's root negatives, and the ordinary local shape they hid
// ---------------------------------------------------------------------------

/// A checkout under a `0700` directory the caller owns — `/home/<user>/app` on
/// most hosts — is the default local case, so a readable root in it is accepted.
///
/// The `R13` step-5 probe cannot answer that question by dropping to a uid: an
/// unprivileged process may not call `setgroups(2)`, and the mode-bit fallback
/// asks whether a *foreign* uid could pass, which an owned `0700` parent never
/// lets it. Asked in-process instead, the root is readable and the site serves.
#[test]
fn a_readable_root_under_an_owned_0700_ancestor_is_accepted() {
    let fixture = Fixture::new("owned-0700");
    let home = fixture.project(&fixture.path().join("home"), 0o700);
    let app = fixture.project(&home.join("app"), 0o755);
    fs::write(app.join("blastit.toml"), ALL_STATIC).expect("write blastit.toml");
    write_site(&app);

    let check = run(&app, &[], &["check", "--json"]);
    assert_eq!(check.status.code(), Some(0), "{check:?}");
    assert!(
        String::from_utf8_lossy(&check.stderr).is_empty(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );

    // And the same tree serves under `blastit dev`: this is the page's headline
    // local flow, not only the CI one.
    let (_port, output) = dev_serves(&app);
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("permission denied"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_root_the_caller_cannot_list_is_refused() {
    // `chmod 000` answers `permission denied` for an unprivileged caller and
    // nothing at all for root, which reads any directory: this case is about
    // the caller's own identity, so it states that assumption instead of
    // passing silently as uid 0.
    assert_ne!(
        rustix::process::geteuid().as_raw(),
        0,
        "this case must run as the calling user: root can list a 0000 directory"
    );
    let fixture = Fixture::new("root-unreadable");
    fixture.write_manifest(ALL_STATIC);
    fixture.write_site();
    let public = fixture.path().join("public");
    set_mode(&public, 0o000);

    let check = run(fixture.path(), &[], &["check", "--json"]);
    let dev = run(
        fixture.path(),
        &[("BLASTIT_MEM_HIGH_WATERMARK", "100")],
        &["dev"],
    );
    // Put the mode back before asserting, so a failure still leaves a
    // removable fixture behind.
    set_mode(&public, 0o755);

    assert_eq!(check.status.code(), Some(2), "{check:?}");
    assert!(check.stdout.is_empty(), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    for expected in [
        "route \"site\"",
        "static root \"public\"",
        "permission denied",
    ] {
        assert!(
            stderr.contains(expected),
            "{expected} missing from: {stderr}"
        );
    }

    assert_eq!(dev.status.code(), Some(1), "{dev:?}");
    assert!(dev.stdout.is_empty(), "{dev:?}");
    assert!(
        String::from_utf8_lossy(&dev.stderr).contains("static root \"public\""),
        "{}",
        String::from_utf8_lossy(&dev.stderr)
    );
}

#[test]
fn a_root_that_is_a_file_is_refused() {
    let fixture = Fixture::new("root-is-file");
    fixture.write_manifest(ALL_STATIC);
    fs::write(fixture.path().join("public"), "not a directory\n").expect("write the file");

    let check = run(fixture.path(), &[], &["check", "--json"]);
    assert_eq!(check.status.code(), Some(2), "{check:?}");
    assert!(check.stdout.is_empty(), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    for expected in [
        "route \"site\"",
        "static root \"public\"",
        "not a directory",
    ] {
        assert!(
            stderr.contains(expected),
            "{expected} missing from: {stderr}"
        );
    }

    let dev = run(
        fixture.path(),
        &[("BLASTIT_MEM_HIGH_WATERMARK", "100")],
        &["dev"],
    );
    assert_eq!(dev.status.code(), Some(1), "{dev:?}");
    assert_eq!(dev.stdout.len(), 0, "{dev:?}");
}

/// A `BLASTIT_APP` that no route could ever reach is refused, not ignored.
///
/// `Manifest::is_all_static` is the manifest-level answer, so a setting that
/// names an app for a site whose every route answers from the filesystem is a
/// setting that can never take effect: `check` would skip the suite and `dev`
/// would never import it. The refusal names the variable and never its value.
#[test]
fn a_blastit_app_on_an_all_static_manifest_is_refused() {
    let fixture = Fixture::new("app-on-static");
    fixture.write_manifest(ALL_STATIC);
    fixture.write_site();

    let app = "demo:application";
    let dev = run(
        fixture.path(),
        &[("BLASTIT_MEM_HIGH_WATERMARK", "100"), ("BLASTIT_APP", app)],
        &["dev"],
    );
    assert_eq!(dev.status.code(), Some(1), "{dev:?}");
    assert!(dev.stdout.is_empty(), "{dev:?}");
    let stderr = String::from_utf8_lossy(&dev.stderr);
    for expected in ["BLASTIT_APP", "every route is static"] {
        assert!(
            stderr.contains(expected),
            "{expected} missing from: {stderr}"
        );
    }
    assert!(
        !stderr.contains(app),
        "the message names the variable, never its value: {stderr}"
    );

    let check = run(
        fixture.path(),
        &[("BLASTIT_APP", app)],
        &["check", "--json"],
    );
    assert_eq!(check.status.code(), Some(2), "{check:?}");
    assert!(check.stdout.is_empty(), "{check:?}");
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("BLASTIT_APP: every route is static"),
        "{stderr}"
    );
    assert!(!stderr.contains(app), "{stderr}");
}

// ---------------------------------------------------------------------------
// The completeness warning is a warning (§4.7 last paragraph)
// ---------------------------------------------------------------------------

/// A manifest with an app, one app route and one static route.
fn mixed_with_app(static_route: bool) -> Manifest {
    let static_block = if static_route {
        "\n[route.static]\nroot = \"public\"\nspa = true\n"
    } else {
        ""
    };
    let source = format!(
        "version = 1\napp = \"demo:application\"\n\n\
         [[route]]\nname = \"api\"\npath = ['/api/.*']\n\n\
         [[route]]\nname = \"site\"\npath = ['.*']\n{static_block}"
    );
    Manifest::from_toml_str(&source).expect("the fixture manifest parses")
}

/// One `GET /api/notes`, which only the app route can match.
fn api_request() -> CheckObsV1 {
    CheckObsV1::new(
        1,
        vec![HttpObsLine {
            method: "GET".to_string(),
            path: "/api/notes".to_string(),
            content_type: None,
            body_len: 0,
            sample: None,
        }],
        Vec::new(),
    )
    .expect("v1 observations")
}

#[test]
fn a_static_root_the_suite_never_requested_is_a_warning_not_a_finding() {
    let manifest = mixed_with_app(true);
    let observations = api_request();
    let table = RoutingTable::new(&manifest).expect("the table builds");

    let report = audit_v1::evaluate(&manifest, &observations).expect("the audit runs");
    assert!(
        report.ok,
        "a static route the suite cannot exercise is not a finding: {report:?}"
    );
    assert!(
        !report.findings.iter().any(|finding| matches!(
            finding,
            audit_v1::Finding::UnexercisedRoute { route, .. } if route == "site"
        )),
        "{:?}",
        report.findings
    );
    assert_eq!(report.routes.total, 2, "{report:?}");
    assert_eq!(report.routes.exercised, 1, "{report:?}");

    assert_eq!(
        audit_v1::unrequested_static_roots(&manifest, &table, &observations),
        [("site".to_string(), "public".to_string())]
    );

    // The negative twin: the same route without `[route.static]` is an
    // `unexercised_route` finding again, so the skip is about the static
    // surface and not a blanket exemption.
    let plain = mixed_with_app(false);
    let plain_table = RoutingTable::new(&plain).expect("the table builds");
    let plain_report = audit_v1::evaluate(&plain, &observations).expect("the audit runs");
    assert!(
        plain_report.findings.iter().any(|finding| matches!(
            finding,
            audit_v1::Finding::UnexercisedRoute { route, .. } if route == "site"
        )),
        "{:?}",
        plain_report.findings
    );
    assert!(audit_v1::unrequested_static_roots(&plain, &plain_table, &observations).is_empty());
}
