//! `TESTING_PLAN.md` §5 — the CI completeness gate (`blastit check`) and the
//! formatter (`blastit fmt`), driven through the real binary.
//!
//! Every case boots the real `blastit`, the real dbproxy role and — where a
//! store is involved — a real PostgreSQL v3 peer (`common/pg_upstream.rs`). The
//! suite `check` spawns is `common/check_suite.rs`'s scriptable `pytest` shim:
//! its observations, statements and exit code are the fixture, and the gate,
//! the audit, the learn editor and the exit table are the product's.
//!
//! Covered: the `0/1/2/3` exit table and stdout purity; human ⇄ `--json` parity
//! (all §5 finding kinds, `view`/`trait` nouns, proxy counts); `--learn` (write,
//! dry-run, JSON, failing suite); the installed gate, venv and `PYTHONPATH`; the
//! `0600` Django handshake; the static-root rules and all-static refusal; the
//! `libphp` preflight; `fmt` (idempotent, comment-preserving, diff on
//! `--check`, `PATH`); and one-file `Blastfile` discovery.
#![cfg(all(feature = "static", feature = "python"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

#[path = "common/check_learn.rs"]
#[allow(dead_code)]
mod check_learn;
#[path = "common/check_suite.rs"]
mod check_suite;
#[path = "common/pg_upstream.rs"]
#[allow(dead_code)]
mod pg_upstream;
#[cfg(feature = "php")]
#[path = "common/php_host.rs"]
#[allow(dead_code)]
mod php_host;
#[path = "common/project.rs"]
mod project;

use pg_upstream::PgUpstream;
use project::Project;

#[path = "common/check_cli.rs"]
mod check_cli;

use check_cli::{
    BINARY, DEV_ENV, dev_serves, document, env_of, env_pg, pg_url, project, run, run_bounded,
    stderr, suite, text, write_static_root,
};

// ---------------------------------------------------------------------------
// §5: the clean runs — WSGI + Postgres, database-free, all-static
// ---------------------------------------------------------------------------

/// A full suite: a routed request, a granted parameterized statement through the
/// real dbproxy, and a clean report in both forms.
#[test]
fn a_clean_wsgi_and_postgres_suite_exits_zero() {
    let project = project("clean-pg");
    let server = PgUpstream::start();
    let manifest = "\
postgres $DATABASE_URL

view shop GET \"/shop/*\" {
    postgres {
        query 'SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $?'
    }
}
";
    let shim = suite(
        &project,
        manifest,
        json!({
            "http": [{"method": "GET", "path": "/shop/7"}],
            "push": [{"op": "push_routed", "route": "shop"}],
            "sql": ["SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $1"],
        }),
    );
    let url = pg_url(&server);
    let tmp = project.path("tmp");
    let env = env_pg(&shim, &tmp, &url);

    let output = run(&project.dir, &["check"], &env);
    let human = text(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr:\n{}",
        stderr(&output)
    );
    assert_eq!(
        human,
        "blastit check: 1 request, 1 statement, 1/1 view, clean\n",
        "stderr:\n{}",
        stderr(&output)
    );
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));

    let output = run(&project.dir, &["check", "--json"], &env);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report = document(&output);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["requests"], 1, "{report}");
    assert_eq!(report["statements"], 1, "{report}");
    assert_eq!(
        report["routes"],
        json!({"total": 1, "exercised": 1}),
        "{report}"
    );
    assert_eq!(
        report["proxies"],
        json!({"db": {"statements": 1, "denied": 0}})
    );
    assert_eq!(report["findings"], json!([]));
}

/// A store-less app declares no proxy, so `check` starts no dbproxy and audits
/// the HTTP observations alone — the 0-proxy case.
#[test]
fn a_database_free_suite_is_clean_and_passes_pytest_args() {
    let project = project("clean-modern");
    let record = project.path("record.json");
    let shim = suite(
        &project,
        "view home GET \"/\" {}\n",
        json!({
            "http": [{"method": "GET", "path": "/"}],
            "record": record.to_str().expect("a UTF-8 record path"),
        }),
    );
    let tmp = project.path("tmp");
    let output = run(
        &project.dir,
        &["check", "--json", "--", "-k", "products"],
        &env_of(&shim, &tmp),
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report = document(&output);
    assert_eq!(report["proxies"], json!({}), "{report}");
    assert_eq!(report["findings"], json!([]), "{report}");
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));

    // The suite's own argv ends with the caller's `-- ARGS`.
    let facts: Value = serde_json::from_str(&std::fs::read_to_string(&record).expect("the record"))
        .expect("the record is JSON");
    let argv = facts["argv"].as_array().expect("the recorded argv");
    assert_eq!(
        argv.get(argv.len() - 2).and_then(Value::as_str),
        Some("-k"),
        "{facts}"
    );
    assert_eq!(
        argv.last().and_then(Value::as_str),
        Some("products"),
        "{facts}"
    );
}

/// An all-static file is audited without pytest: every root exists, the human
/// form prints nothing and `--json` is the unchanged zero-count document.
#[test]
fn a_static_only_blastfile_audits_its_roots_and_exits_zero() {
    let project = project("clean-static");
    project.write("Blastfile", "static \"/\" {\n    root \"public\"\n}\n");
    write_static_root(&project.dir);

    // No PATH, so a run that reached the suite preflight would exit `2`.
    let output = run(&project.dir, &["check", "--json"], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report = document(&output);
    assert_eq!(report["schema_version"], 1, "{report}");
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["requests"], 0, "{report}");
    assert_eq!(report["statements"], 0, "{report}");
    assert_eq!(
        report["routes"],
        json!({"total": 0, "exercised": 0}),
        "{report}"
    );
    assert_eq!(report["proxies"], json!({}), "{report}");
    assert_eq!(report["findings"], json!([]), "{report}");
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));

    let output = run(&project.dir, &["check"], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
}

const PARITY_MANIFEST: &str = "\
postgres $DATABASE_URL

postgres analytics $ANALYTICS_URL

trait shared {
    postgres {
        query 'SELECT \"s\".\"id\" FROM \"s\"'
    }
}

view shop \"/shop/*\" {
    use shared
    postgres {
        query 'SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $?'
    }
    postgres analytics {
        query 'SELECT 2'
    }
}

view other \"/other/*\" {}

view upload \"/upload/*\" {}

view form \"/form/*\" {
    body json
}

view raw \"/raw/*\" {
    body raw
}
";

/// One run with every §5 diagnostic: connection setup, a missing per-view grant,
/// a dead per-view and trait grant, an unexercised view, an undeclared body, a
/// codec mismatch, an unacknowledged `raw` body, an unrouted request — and the
/// `2 proxies` header — with identical facts in both forms.
#[test]
fn human_and_json_report_the_same_findings_and_fixes() {
    let project = project("parity");
    let server = PgUpstream::start();
    let shim = suite(
        &project,
        PARITY_MANIFEST,
        json!({
            "http": [
                {"method": "GET", "path": "/shop/1"},
                {"method": "POST", "path": "/upload/x", "content_type": "text/plain", "body_len": 4},
                {
                    "method": "POST",
                    "path": "/form/x",
                    "content_type": "application/x-www-form-urlencoded",
                    "body_len": 15,
                    "sample": {
                        "mime": "application/x-www-form-urlencoded",
                        "truncated": false,
                        "valid_json": false,
                        "valid_form": true,
                    },
                },
                {"method": "GET", "path": "/raw/x"},
                {"method": "GET", "path": "/nope"},
            ],
            "push": [{"op": "push_routed", "route": "shop"}],
            "sql": [
                "DELETE FROM \"p\" WHERE \"p\".\"id\" = $1",
                "SELECT set_config('TimeZone', $1, false)",
            ],
            "analytics_sql": ["SELECT 2"],
        }),
    );
    let url = pg_url(&server);
    let tmp = project.path("tmp");
    let env = [
        ("BLASTIT_PYTHON", shim.to_str().expect("a UTF-8 shim path")),
        ("DATABASE_URL", url.as_str()),
        ("ANALYTICS_URL", url.as_str()),
        ("TMPDIR", tmp.to_str().expect("a UTF-8 fixture path")),
    ];

    let output = run(&project.dir, &["check"], &env);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert_eq!(
        text(&output.stdout),
        "\
blastit check: 5 requests, 3 statements, 4/5 views, 2 proxies
  view shop: 2 statements not granted on proxy db
    DELETE FROM \"p\" WHERE \"p\".\"id\" = $?
      fix: add it to view shop postgres
    SELECT set_config('TimeZone', $?, false)
      fix: add it to trait connection
  view shop: 1 grant never used on proxy db
    SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $?
      fix: remove it from view shop postgres
  trait shared: 1 grant never used on proxy db
    SELECT \"s\".\"id\" FROM \"s\"
      fix: remove it from trait shared
  view other: never exercised
      fix: add a test that reaches it
  view upload: takes a body and declares no ingress validation; observed text/plain
      fix: write `body raw unvalidated`
  view form: declared body json does not match observed application/x-www-form-urlencoded
      fix: write `body form`
  view raw: declares body raw without `unvalidated`
      fix: write `body raw unvalidated` on view raw body
  request GET /nope: no view matches
      fix: add a `view` whose pattern matches this request
blastit check: 9 problems · `--json` for the machine-readable list
"
    );

    let output = run(&project.dir, &["check", "--json"], &env);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let report = document(&output);
    assert_eq!(report["ok"], false, "{report}");
    assert_eq!(report["requests"], 5, "{report}");
    assert_eq!(report["statements"], 3, "{report}");
    assert_eq!(
        report["routes"],
        json!({"total": 5, "exercised": 4}),
        "{report}"
    );
    assert_eq!(
        report["proxies"],
        json!({
            "analytics": {"statements": 1, "denied": 0},
            "db": {"statements": 2, "denied": 2},
        }),
        "{report}"
    );
    assert_eq!(
        report["findings"],
        json!([
            {
                "kind": "missing_grant",
                "severity": "error",
                "route": "shop",
                "proxy": "db",
                "sql": "DELETE FROM \"p\" WHERE \"p\".\"id\" = $?",
                "fix": {"action": "add_query", "to": "view shop postgres"}
            },
            {
                "kind": "missing_grant",
                "severity": "error",
                "route": "shop",
                "proxy": "db",
                "sql": "SELECT set_config('TimeZone', $?, false)",
                "fix": {"action": "add_group", "to": "trait connection"}
            },
            {
                "kind": "dead_grant",
                "severity": "error",
                "route": "shop",
                "proxy": "db",
                "sql": "SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $?",
                "fix": {"action": "remove_query", "to": "view shop postgres", "unsafe": true}
            },
            {
                "kind": "dead_grant",
                "severity": "error",
                "route": "shop",
                "group": "shared",
                "proxy": "db",
                "sql": "SELECT \"s\".\"id\" FROM \"s\"",
                "fix": {"action": "remove_query", "to": "trait shared", "unsafe": true}
            },
            {
                "kind": "unexercised_route",
                "severity": "error",
                "route": "other",
                "fix": {"action": "add_test"}
            },
            {
                "kind": "missing_ingress",
                "severity": "error",
                "route": "upload",
                "observed": ["text/plain"],
                "fix": {"action": "declare_ingress", "codecs": ["raw"], "allow_raw": true}
            },
            {
                "kind": "ingress_mismatch",
                "severity": "error",
                "route": "form",
                "declared": "json",
                "observed": ["application/x-www-form-urlencoded"],
                "fix": {"action": "fix_ingress", "codecs": ["form"]}
            },
            {
                "kind": "raw_ingress",
                "severity": "error",
                "route": "raw",
                "fix": {"action": "fix_ingress", "to": "view raw body", "allow_raw": true}
            },
            {
                "kind": "unrouted_request",
                "severity": "error",
                "method": "GET",
                "path": "/nope",
                "fix": {"action": "add_route"}
            }
        ])
    );
}

/// `no_observations` and `proxy_unused`, plus the R35 suppression that keeps an
/// empty HTTP stream from inventing one `unexercised_route` per view.
#[test]
fn an_unobserved_run_names_no_observations_and_proxy_unused() {
    let server = PgUpstream::start();
    let url = pg_url(&server);
    for (tag, manifest, spec, exercised, human, findings) in [
        (
            "empty",
            "view a GET \"/a/*\" {}\nview b GET \"/b/*\" {}\n",
            json!({"http": []}),
            0,
            "blastit check: 0 requests, 0 statements, 0/2 views\n  suite exercised 0 views or 0 proxy statements\n      fix: run the suite against this app (does DATABASE_URL point at blastit?)\nblastit check: 1 problem · `--json` for the machine-readable list\n",
            json!([{"kind": "no_observations", "severity": "error", "fix": {"action": "observe"}}]),
        ),
        (
            "proxy",
            "postgres $DATABASE_URL\n\nview home GET \"/\" {}\n",
            json!({"http": [{"method": "GET", "path": "/"}]}),
            1,
            "blastit check: 1 request, 0 statements, 1/1 view\n  suite exercised 0 views or 0 proxy statements\n      fix: run the suite against this app (does DATABASE_URL point at blastit?)\n  proxy db: declared, 0 statements observed\n      fix: remove the store, or add the call that uses it\nblastit check: 2 problems · `--json` for the machine-readable list\n",
            json!([
                {"kind": "no_observations", "severity": "error", "fix": {"action": "observe"}},
                {"kind": "proxy_unused", "severity": "error", "proxy": "db", "fix": {"action": "observe"}}
            ]),
        ),
    ] {
        let project = project(&format!("un-{tag}"));
        let shim = suite(&project, manifest, spec);
        let env = [
            ("BLASTIT_PYTHON", shim.to_str().expect("a UTF-8 shim path")),
            ("DATABASE_URL", url.as_str()),
        ];
        let output = run(&project.dir, &["check"], &env);
        assert_eq!(output.status.code(), Some(1), "{tag}: {}", stderr(&output));
        assert_eq!(text(&output.stdout), human, "{tag}");
        let output = run(&project.dir, &["check", "--json"], &env);
        assert_eq!(output.status.code(), Some(1), "{tag}: {}", stderr(&output));
        let report = document(&output);
        assert_eq!(report["findings"], findings, "{tag}: {report}");
        assert_eq!(report["routes"]["exercised"], exercised, "{tag}: {report}");
    }
}

// ---------------------------------------------------------------------------
// §5: the exit table and suite failures
// ---------------------------------------------------------------------------

/// A failed suite is exit `3` for every shape pytest can fail with, and stdout
/// stays empty even under `--json`.
#[test]
fn a_failed_suite_is_exit_three_for_every_pytest_failure_shape() {
    for (tag, exit) in [
        ("assertion", json!(1)),
        ("interrupted", json!(2)),
        ("internal", json!(3)),
        ("usage", json!(4)),
        ("collected-nothing", json!(5)),
        ("signalled", json!("signal")),
    ] {
        let project = project(&format!("suite-{tag}"));
        let shim = suite(
            &project,
            "view home GET \"/\" {}\n",
            json!({"http": [{"method": "GET", "path": "/"}], "exit": exit}),
        );
        let tmp = project.path("tmp");
        let env = env_of(&shim, &tmp);
        for arguments in [&["check"][..], &["check", "--json"][..]] {
            let output = run(&project.dir, arguments, &env);
            assert_eq!(
                output.status.code(),
                Some(3),
                "{arguments:?} ({tag}): {}",
                stderr(&output)
            );
            assert!(
                output.stdout.is_empty(),
                "{arguments:?} ({tag}) wrote to stdout: {:?}",
                text(&output.stdout)
            );
        }
    }
}

/// A start failure is exit `2`, empties stdout under `--json`, and names the
/// thing that could not start.
#[test]
fn a_start_failure_is_exit_two_with_empty_stdout() {
    let project = project("start-failure");
    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = run(&project.dir, arguments, &[]);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert!(
            stderr(&output).starts_with("blastit check: ./Blastfile: no such file or directory"),
            "{arguments:?}: {}",
            stderr(&output)
        );
    }
}

/// An observation line the schema does not read is a start failure naming the
/// file and the line — never a half-read report.
#[test]
fn a_malformed_observation_is_a_start_failure() {
    let project = project("bad-observation");
    let shim = suite(
        &project,
        "view home GET \"/\" {}\n",
        json!({"raw_http": ["{\"method\": \"GET\", \"path\":"]}),
    );
    let tmp = project.path("tmp");
    let env = env_of(&shim, &tmp);
    let output = run(&project.dir, &["check", "--json"], &env);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    let stderr = stderr(&output);
    assert!(
        stderr.contains("http_obs.jsonl: line 1: "),
        "the refusal names the file and the line: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// §5: --learn
// ---------------------------------------------------------------------------

/// `--learn --dry-run` prints the exact proposal and leaves the file
/// byte-identical.
#[test]
fn a_dry_run_learn_prints_the_proposal_and_writes_nothing() {
    let project = project("learn-dry-run");
    let server = PgUpstream::start();
    let url = pg_url(&server);
    let python = check_suite::host_python();
    let shim = check_learn::write_fixture(&project.dir, &python);
    let manifest = project.path("Blastfile");
    let before = std::fs::read(&manifest).expect("the manifest bytes");

    // A short scratch `TMPDIR`: the check socket lives under it, and a
    // fixture-named directory would push the Unix socket path past `SUN_LEN`.
    let scratch = std::env::temp_dir().join(format!("mb5633-dry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("the dry-run TMPDIR");
    let env = [
        ("BLASTIT_PYTHON", shim.to_str().expect("a UTF-8 shim path")),
        ("DATABASE_URL", url.as_str()),
        ("TMPDIR", scratch.to_str().expect("a UTF-8 scratch path")),
    ];
    let output = run(&project.dir, &["check", "--learn", "--dry-run"], &env);
    let stdout = text(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr:\n{}",
        stderr(&output)
    );
    let absolute = std::path::absolute(&manifest).expect("an absolute manifest path");
    let expected = format!(
        "learn: 2 routes, 3 queries proposed (dry-run) → {}",
        absolute.display()
    );
    assert!(
        stdout.contains(&expected),
        "the dry-run line is `{expected}`:\n{stdout}"
    );
    assert!(
        stdout.contains("learn: proposed Blastfile (nothing written):"),
        "{stdout}"
    );
    assert!(
        !stdout.contains(" added "),
        "a dry run must never say `added`: {stdout}"
    );
    assert_eq!(
        std::fs::read(&manifest).expect("the manifest bytes"),
        before,
        "a dry run must leave the file byte-identical"
    );
    assert_eq!(
        std::fs::read_dir(&scratch)
            .expect("the dry-run TMPDIR")
            .count(),
        0,
        "a dry run left its runtime directory behind"
    );

    // The same dry run as JSON: the proposal, no `findings`, `wrote:false`.
    let output = run(
        &project.dir,
        &["check", "--learn", "--dry-run", "--json"],
        &env,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let plan = document(&output);
    assert_eq!(plan["wrote"], false, "{plan}");
    assert!(plan.get("proposed").is_some(), "{plan}");
    assert!(plan.get("findings").is_none(), "{plan}");
    assert_eq!(
        std::fs::read(&manifest).expect("the manifest bytes"),
        before
    );
    assert_eq!(
        std::fs::read_dir(&scratch)
            .expect("the dry-run TMPDIR")
            .count(),
        0,
        "the JSON dry run left its runtime directory behind"
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

/// `--learn --json` writes a grant into the view that lacks it, never widens a
/// neighbour, puts the `learn:` line on stderr, and keys the document `routes`.
#[test]
fn learn_writes_one_grant_without_widening_a_view() {
    let project = project("learn-write");
    let server = PgUpstream::start();
    let manifest = "\
postgres $DATABASE_URL

view shop \"/shop/*\" {
    postgres {
        query 'SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $?'
    }
}

view house \"/house/*\" {
    postgres {
        query 'SELECT \"h\".\"id\" FROM \"h\"'
    }
}
";
    let shim = suite(
        &project,
        manifest,
        json!({
            "http": [{"method": "GET", "path": "/shop/1"}],
            "push": [{"op": "push_routed", "route": "shop"}],
            "sql": ["DELETE FROM \"p\" WHERE \"p\".\"id\" = $1"],
        }),
    );
    let url = pg_url(&server);
    let tmp = project.path("tmp");
    let env = env_pg(&shim, &tmp, &url);
    let path = project.path("Blastfile");
    let before = std::fs::read_to_string(&path).expect("the manifest");
    let output = run(&project.dir, &["check", "--learn", "--json"], &env);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let plan = document(&output);
    assert_eq!(plan["proposed"]["routes"], 0, "{plan}");
    assert_eq!(plan["proposed"]["queries"], 1, "{plan}");
    assert!(
        plan["proposed"].get("route").is_none(),
        "never `route`: {plan}"
    );
    assert_eq!(plan["wrote"], true, "{plan}");
    assert!(plan.get("schema_version").is_none(), "{plan}");
    let absolute = std::path::absolute(&path).expect("an absolute manifest path");
    assert!(
        stderr(&output).contains(&format!(
            "learn: 0 routes, 1 query added → {}",
            absolute.display()
        )),
        "{}",
        stderr(&output)
    );

    let after = std::fs::read_to_string(&path).expect("the manifest");
    assert!(
        after.contains("DELETE FROM \"p\" WHERE \"p\".\"id\" = $?"),
        "{after}"
    );
    assert!(
        after.contains("SELECT \"p\".\"id\" FROM \"p\" WHERE \"p\".\"id\" = $?"),
        "the existing grant survived: {after}"
    );
    assert_eq!(
        after.matches("view shop").count(),
        1,
        "the view was duplicated: {after}"
    );
    let house = &after[after.find("view house").expect("the house view")..];
    assert_eq!(
        house,
        &before[before.find("view house").expect("the house view")..],
        "a learn run widened or rewrote a view it did not own"
    );

    // Idempotent: a second learn of the same run changes nothing.
    let output = run(&project.dir, &["check", "--learn", "--json"], &env);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(
        std::fs::read_to_string(&path).expect("the manifest"),
        after,
        "a second learn rewrote the file"
    );

    // A failing suite writes nothing, even with a grant to learn.
    std::fs::write(
        project.path("spec.json"),
        json!({
            "http": [{"method": "GET", "path": "/shop/1"}],
            "push": [{"op": "push_routed", "route": "shop"}],
            "sql": ["INSERT INTO \"p\" VALUES ($1)"],
            "exit": 1,
        })
        .to_string(),
    )
    .expect("the failing spec");
    let output = run(&project.dir, &["check", "--learn"], &env);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert_eq!(
        std::fs::read_to_string(&path).expect("the manifest"),
        after,
        "a failed suite rewrote the manifest"
    );
    assert!(
        !stderr(&output).contains(" added "),
        "a failed suite printed a learn line: {}",
        stderr(&output)
    );
}

// ---------------------------------------------------------------------------
// §5: the installed gate, the project venv and PYTHONPATH
// ---------------------------------------------------------------------------

/// The installed bundle carries `_check_pytest.py` and `__main__.py` beside the
/// binary; a cargo tree does not. The project's `.venv` wins over `PATH`, and
/// the caller's `PYTHONPATH` stays first.
#[test]
fn the_installed_bundle_carries_the_gate_and_the_venv_wins() {
    let project = project("gate-root");
    let marker = project.path("marker");
    let record_path = project.path("record.json");
    let shim = suite(
        &project,
        "view home GET \"/\" {}\n",
        json!({
            "http": [{"method": "GET", "path": "/"}],
            "record": record_path.to_str().expect("a UTF-8 record path"),
        }),
    );
    let caller = project.path("caller");
    std::fs::create_dir_all(&caller).expect("the caller PYTHONPATH entry");
    let venv = project.path(".venv/bin");
    std::fs::create_dir_all(&venv).expect(".venv/bin");
    let path_dir = project.path("bin");
    std::fs::create_dir_all(&path_dir).expect("a PATH directory");
    let python = check_suite::host_python();
    check_suite::write_shim(
        &venv.join("python3"),
        &python,
        &project.path("suite.py"),
        Some((&marker, "venv")),
        true,
        false,
    );
    check_suite::write_shim(
        &path_dir.join("python3"),
        &python,
        &project.path("suite.py"),
        Some((&marker, "path")),
        true,
        false,
    );
    let _ = shim;
    let tmp = project.path("tmp");
    let path_value = path_dir.to_str().expect("a UTF-8 fixture path");
    let caller_value = caller.to_str().expect("a UTF-8 fixture path");
    let env = [
        ("PATH", path_value),
        ("PYTHONPATH", caller_value),
        ("TMPDIR", tmp.to_str().expect("a UTF-8 fixture path")),
    ];

    // A cargo tree: no bundle beside `target/debug/blastit`, so PYTHONPATH is
    // the caller's alone and the project's `.venv` still wins.
    let output = check_cli::command(Path::new(BINARY), &project.dir, &["check", "--json"], &env)
        .output()
        .expect("run the cargo binary");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("the record"))
            .expect("the record is JSON");
    assert_eq!(record["pythonpath"], caller_value, "{record}");
    let used = std::fs::read_to_string(&marker).expect("the interpreter marker");
    assert!(
        used.lines().all(|line| line == "venv"),
        "the project venv must win over PATH: {used:?}"
    );

    // The installed layout: `<exe_dir>/../python/blastit` holds the gate, so
    // the bundle directory follows the caller's entry.
    let installed = installed_binary(&project);
    std::fs::remove_file(&marker).expect("clear the marker");
    let output = check_cli::command(&installed, &project.dir, &["check", "--json"], &env)
        .output()
        .expect("run the installed binary");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("the record"))
            .expect("the record is JSON");
    let bundle = project.path("install/share/blastit/lib/../python");
    assert_eq!(
        record["pythonpath"],
        format!("{caller_value}:{}", bundle.display()),
        "the bundle root follows the caller's PYTHONPATH"
    );
    assert_eq!(
        std::fs::read_to_string(&marker)
            .expect("the interpreter marker")
            .lines()
            .filter(|line| *line != "venv")
            .count(),
        0,
        "the installed run still starts from the project venv"
    );
}

/// Copies the binary into a `make install`-shaped tree with the staged gate,
/// and returns the copy a user would type.
fn installed_binary(project: &Project) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let lib = project.path("install/share/blastit/lib");
    std::fs::create_dir_all(&lib).expect("the installed lib directory");
    let exe = lib.join("blastit");
    std::fs::copy(BINARY, &exe).expect("copy the binary");
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
        .expect("an executable copy");
    let package = project.path("install/share/blastit/python/blastit");
    std::fs::create_dir_all(&package).expect("the staged package");
    std::fs::write(package.join("_check_pytest.py"), "").expect("the gate marker");
    std::fs::write(package.join("__main__.py"), "").expect("the package marker");
    exe
}

// ---------------------------------------------------------------------------
// §5: the pytest-django handshake
// ---------------------------------------------------------------------------

/// The handshake is `0600`, names `test_<database>` and the real inputs, is
/// created fresh for each run and is gone when the run ends.
#[test]
fn the_django_handshake_is_private_and_removed() {
    let project = project("django");
    let server = PgUpstream::start();
    let url = pg_url(&server);
    let record_path = project.path("record.json");
    let manifest = "\
postgres $DATABASE_URL

view home \"/\" {
    postgres {
        query 'SELECT \"p\".\"id\" FROM \"p\"'
    }
}
";
    let spec = json!({
        "http": [{"method": "GET", "path": "/"}],
        "push": [{"op": "push_routed", "route": "home"}],
        "sql": ["SELECT \"p\".\"id\" FROM \"p\""],
        "record": record_path.to_str().expect("a UTF-8 record path"),
        "django": true,
    });
    let shim = check_suite::django_fixture(&project.dir, manifest, &spec);
    let tmp = project.path("tmp");
    let env = [
        ("BLASTIT_PYTHON", shim.to_str().expect("a UTF-8 shim path")),
        ("DATABASE_URL", url.as_str()),
        ("DJANGO_SETTINGS_MODULE", "demo.settings"),
        ("TMPDIR", tmp.to_str().expect("a UTF-8 fixture path")),
    ];

    let mut handshakes = Vec::new();
    for _ in 0..2 {
        let output = run(&project.dir, &["check", "--json"], &env);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        let record: Value =
            serde_json::from_str(&std::fs::read_to_string(&record_path).expect("the record"))
                .expect("the record is JSON");
        let handshake = &record["django"];
        assert_eq!(handshake["mode"], 0o600, "{record}");
        assert_eq!(handshake["content"]["proxy"], "db", "{record}");
        assert_eq!(handshake["content"]["database"], "test_app", "{record}");
        assert_eq!(
            handshake["content"]["real"]["DATABASE_URL"], url,
            "{record}"
        );
        assert_eq!(handshake["content"]["user"], "blastit", "{record}");
        let swapped = handshake["content"]["swapped"]["DATABASE_URL"]
            .as_str()
            .expect("the swapped URL");
        let socket_dir = handshake["content"]["socket_dir"]
            .as_str()
            .expect("the socket directory");
        assert!(
            swapped.starts_with("postgresql://blastit@") && swapped.contains(socket_dir),
            "the swapped URL points at the check sockets: {record}"
        );
        let path = PathBuf::from(
            handshake["path"]
                .as_str()
                .expect("the recorded handshake path"),
        );
        assert!(
            !path.exists(),
            "the handshake outlived the run: {}",
            path.display()
        );
        handshakes.push(path);

        // `manage.py` alone makes the project a Django project too.
        std::fs::write(project.path("manage.py"), "").expect("manage.py");
        let env = env_pg(&shim, &tmp, &url);
        let output = run(&project.dir, &["check", "--json"], &env);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        let record: Value =
            serde_json::from_str(&std::fs::read_to_string(&record_path).expect("the record"))
                .expect("the record is JSON");
        handshakes.push(PathBuf::from(
            record["django"]["path"]
                .as_str()
                .expect("the recorded handshake path"),
        ));
    }
    assert_ne!(
        handshakes[0], handshakes[2],
        "each run gets its own handshake file"
    );
    for path in &handshakes {
        assert!(
            !path.exists(),
            "a handshake was reused or left behind: {path:?}"
        );
    }

    // A failed suite still removes it: the `Drop` is on every return path.
    let failing = json!({
        "http": [{"method": "GET", "path": "/"}],
        "record": record_path.to_str().expect("a UTF-8 record path"),
        "django": true,
        "exit": 1,
    });
    let shim = check_suite::django_fixture(&project.dir, manifest, &failing);
    let env = [
        ("BLASTIT_PYTHON", shim.to_str().expect("a UTF-8 shim path")),
        ("DATABASE_URL", url.as_str()),
        ("DJANGO_SETTINGS_MODULE", "demo.settings"),
        ("TMPDIR", tmp.to_str().expect("a UTF-8 fixture path")),
    ];
    let output = run(&project.dir, &["check", "--json"], &env);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("the record"))
            .expect("the record is JSON");
    let path = PathBuf::from(
        record["django"]["path"]
            .as_str()
            .expect("the recorded handshake path"),
    );
    assert!(
        !path.exists(),
        "a failed suite left the handshake behind: {path:?}"
    );
}

/// Django refuses to start before pytest can seed a database: no store at all,
/// or a store whose input variable is absent. Both name the block.
#[test]
fn django_start_failures_name_the_missing_store() {
    for (manifest, facts) in [
        (
            "view home GET \"/\" {}\n",
            ["declares no postgresql proxy", "declare a `postgres` store"],
        ),
        (
            "postgres $DATABASE_URL\n\nview home GET \"/\" {}\n",
            ["proxy \"db\"", "DATABASE_URL"],
        ),
    ] {
        let project = project("django-start");
        let shim = check_suite::django_fixture(
            &project.dir,
            manifest,
            &json!({"http": [{"method": "GET", "path": "/"}]}),
        );
        let tmp = project.path("tmp");
        let env = [
            ("BLASTIT_PYTHON", shim.to_str().expect("a UTF-8 shim path")),
            ("DJANGO_SETTINGS_MODULE", "demo.settings"),
            ("TMPDIR", tmp.to_str().expect("a UTF-8 fixture path")),
        ];
        let output = run(&project.dir, &["check", "--json"], &env);
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
        for fact in facts {
            assert!(stderr(&output).contains(fact), "{}", stderr(&output));
        }
    }
}

// ---------------------------------------------------------------------------
// §5: the static-root rules
// ---------------------------------------------------------------------------

/// A readable root under a `0700` directory the caller owns is the ordinary
/// local shape: both `check` and `dev` accept it.
#[test]
fn a_readable_root_under_an_owned_0700_ancestor_is_accepted() {
    use std::os::unix::fs::PermissionsExt;

    let project = project("static-owned");
    let home = project.path("home");
    let app = home.join("app");
    std::fs::create_dir_all(&app).expect("the project");
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))
        .expect("chmod the owned ancestor");
    std::fs::write(
        app.join("Blastfile"),
        "static \"/\" {\n    root \"public\"\n}\n",
    )
    .expect("the manifest");
    write_static_root(&app);

    let output = run(&app, &["check", "--json"], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
    dev_serves(&app);
}

/// A root the caller cannot list, and a root that is a file, are refused before
/// anything is printed on stdout.
#[test]
fn an_unlistable_root_and_a_root_that_is_a_file_are_refused() {
    use std::os::unix::fs::PermissionsExt;

    let project = project("static-negatives");
    project.write("Blastfile", "static \"/\" {\n    root \"public\"\n}\n");
    write_static_root(&project.dir);
    let public = project.path("public");

    if rustix::process::geteuid().as_raw() == 0 {
        println!("SKIP the unlistable static root: root can list a 0000 directory");
    } else {
        std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o000))
            .expect("chmod the root");

        let check = run(&project.dir, &["check", "--json"], &[]);
        let dev = run(&project.dir, &["dev"], DEV_ENV);
        std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o755))
            .expect("restore the root");

        assert_eq!(check.status.code(), Some(2), "{}", stderr(&check));
        assert!(check.stdout.is_empty(), "{}", text(&check.stdout));
        let refusal = stderr(&check);
        for expected in [
            "mount \"/\" (index 0)",
            "static root \"public\"",
            "permission denied",
        ] {
            assert!(
                refusal.contains(expected),
                "{expected} missing from: {refusal}"
            );
        }
        assert_eq!(dev.status.code(), Some(1), "{}", stderr(&dev));
        assert!(dev.stdout.is_empty(), "{}", text(&dev.stdout));
        assert!(
            stderr(&dev).contains("static root \"public\""),
            "{}",
            stderr(&dev)
        );
    }

    // The same rule for a path that is not a directory.
    std::fs::remove_dir_all(&public).expect("remove the root");
    std::fs::write(&public, "not a directory\n").expect("a file at the root");
    let check = run(&project.dir, &["check", "--json"], &[]);
    assert_eq!(check.status.code(), Some(2), "{}", stderr(&check));
    assert!(check.stdout.is_empty(), "{}", text(&check.stdout));
    for expected in [
        "mount \"/\" (index 0)",
        "static root \"public\"",
        "not a directory",
    ] {
        assert!(stderr(&check).contains(expected), "{}", stderr(&check));
    }
    let dev = run(&project.dir, &["dev"], DEV_ENV);
    assert_eq!(dev.status.code(), Some(1), "{}", stderr(&dev));
    assert!(dev.stdout.is_empty(), "{}", text(&dev.stdout));
}

/// A `BLASTIT_APP` on a file that serves only static mounts is refused, naming
/// the variable and never its value.
#[test]
fn an_app_on_an_all_static_manifest_is_refused() {
    let project = project("static-app");
    project.write("Blastfile", "static \"/\" {\n    root \"public\"\n}\n");
    write_static_root(&project.dir);
    let app = "demo:application";

    let dev = run(&project.dir, &["dev"], &[DEV_ENV[0], ("BLASTIT_APP", app)]);
    assert_eq!(dev.status.code(), Some(1), "{}", stderr(&dev));
    assert!(dev.stdout.is_empty(), "{}", text(&dev.stdout));
    for expected in ["BLASTIT_APP", "the file serves static mounts and no views"] {
        assert!(stderr(&dev).contains(expected), "{}", stderr(&dev));
    }
    assert!(
        !stderr(&dev).contains(app),
        "the value leaked: {}",
        stderr(&dev)
    );

    let check = run(&project.dir, &["check", "--json"], &[("BLASTIT_APP", app)]);
    assert_eq!(check.status.code(), Some(2), "{}", stderr(&check));
    assert!(check.stdout.is_empty(), "{}", text(&check.stdout));
    assert!(
        stderr(&check).contains("BLASTIT_APP: the file serves static mounts and no views"),
        "{}",
        stderr(&check)
    );
    assert!(!stderr(&check).contains(app), "{}", stderr(&check));
}

/// An all-static `check --learn` refuses (nothing to learn), and a missing root
/// is named with its prefix before any output.
#[test]
fn an_all_static_check_refuses_learn_and_names_a_missing_root() {
    let project = project("static-learn");
    project.write("Blastfile", "static \"/\" {\n    root \"public\"\n}\n");
    write_static_root(&project.dir);
    let output = run(&project.dir, &["check", "--learn"], &[]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output)
            .contains("nothing to learn: the file answers every request from static mounts"),
        "{}",
        stderr(&output)
    );

    std::fs::remove_dir_all(project.path("public")).expect("remove the root");
    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = run(&project.dir, arguments, &[]);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        for expected in [
            "Blastfile",
            "mount \"/\" (index 0)",
            "static root \"public\"",
            "no such file or directory",
        ] {
            assert!(
                stderr(&output).contains(expected),
                "{arguments:?}: {expected} missing from: {}",
                stderr(&output)
            );
        }
    }
}

/// A mount the suite never requested is a warning on stderr, never a finding;
/// the twin is a view nothing reached, which is a finding.
#[test]
fn an_unrequested_static_mount_is_a_warning_not_a_finding() {
    let project = project("static-warning");
    project.write(
        "Blastfile",
        "entrypoint python \"demo:application\"\n\nview api GET \"/api/*\" {}\n\n\
         static \"/site\" {\n    root \"public\"\n}\n",
    );
    write_static_root(&project.dir);
    let shim = suite(
        &project,
        &std::fs::read_to_string(project.path("Blastfile")).expect("the manifest"),
        json!({"http": [{"method": "GET", "path": "/api/notes"}]}),
    );
    let tmp = project.path("tmp");
    let env = env_of(&shim, &tmp);
    let output = run(&project.dir, &["check", "--json"], &env);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let report = document(&output);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["findings"], json!([]), "{report}");
    let hint = stderr(&output);
    for fact in ["static \"/site\"", "root \"public\"", "was never requested"] {
        assert!(hint.contains(fact), "{fact} missing from: {hint}");
    }

    // The twin: a **view** nothing reached is a finding, not a warning.
    let shim = suite(
        &project,
        "entrypoint python \"demo:application\"\n\nview api GET \"/api/*\" {}\n\n\
         view site GET \"/site/*\" {}\n",
        json!({"http": [{"method": "GET", "path": "/api/notes"}]}),
    );
    let output = run(&project.dir, &["check", "--json"], &env_of(&shim, &tmp));
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let report = document(&output);
    assert!(
        report["findings"]
            .as_array()
            .is_some_and(|findings| findings.iter().any(|finding| finding["kind"]
                == "unexercised_route"
                && finding["route"] == "site")),
        "{report}"
    );
}

/// `check` never demanded an app; `dev` still does for a mixed tree.
#[test]
fn check_without_an_app_runs_the_suite_and_dev_names_blastit_app() {
    let project = project("mixed-no-app");
    project.write(
        "Blastfile",
        "view api GET \"/api/*\" {}\n\nstatic \"/site\" {\n    root \"public\"\n}\n",
    );
    write_static_root(&project.dir);

    // `check` brings its own callable: no PATH stops at the suite preflight,
    // never at `BLASTIT_APP`.
    let check = run(&project.dir, &["check"], &[]);
    assert_eq!(check.status.code(), Some(2), "{}", stderr(&check));
    assert!(check.stdout.is_empty(), "{}", text(&check.stdout));
    assert!(
        !stderr(&check).contains("BLASTIT_APP"),
        "{}",
        stderr(&check)
    );
    assert!(
        stderr(&check).contains("no interpreter to run pytest on"),
        "{}",
        stderr(&check)
    );

    let dev = run(&project.dir, &["dev"], DEV_ENV);
    assert_eq!(dev.status.code(), Some(1), "{}", stderr(&dev));
    assert!(dev.stdout.is_empty(), "{}", text(&dev.stdout));
    assert!(
        stderr(&dev).contains("BLASTIT_APP: the WSGI callable (`module:attribute`) is required"),
        "{}",
        stderr(&dev)
    );
    assert!(!stderr(&dev).contains("static root"), "{}", stderr(&dev));
}

// ---------------------------------------------------------------------------
// §5: the PHP preflight
// ---------------------------------------------------------------------------

/// `check` runs the same `libphp` discovery gate the boot does, before pytest
/// can exercise a route with no worker behind it, and audits a missing root
/// before the suite preflight too.
#[cfg(feature = "php")]
#[test]
fn the_php_preflight_validates_the_libphp_gate() {
    use std::os::unix::fs::PermissionsExt;

    let project = project("php-preflight");
    project.write(
        "Blastfile",
        "entrypoint php \"public/index.php\"\n\nview app \"/\" {}\n",
    );
    project.write("public/index.php", "<?php\n");
    let marker = project.path("pytest-ran");
    let shim = project.write(
        "python3",
        format!(
            "#!/bin/sh\nprintf 'ran\\n' >> '{marker}'\nexit 0\n",
            marker = marker.display()
        ),
    );
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
        .expect("an executable shim");
    let shim = shim.to_str().expect("a UTF-8 shim path");
    let not_a_library = project.write("libphp.so", "this is not an ELF shared object\n");
    let output = run(
        &project.dir,
        &["check", "--json"],
        &[
            (
                "BLASTIT_LIBPHP",
                not_a_library.to_str().expect("a UTF-8 path"),
            ),
            ("BLASTIT_PYTHON", shim),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    let refusal = stderr(&output);
    assert!(
        refusal.contains("route \"app\": entrypoint php \"public/index.php\""),
        "{refusal}"
    );
    assert!(refusal.contains("not an ELF shared object"), "{refusal}");
    assert!(refusal.contains("BLASTIT_LIBPHP"), "{refusal}");
    assert!(!refusal.contains("pytest is not installed"), "{refusal}");
    assert!(
        !marker.exists(),
        "pytest ran before the libphp gate refused"
    );

    // With a usable host libphp, a missing static root is named before pytest.
    let Some(host) = php_host::host_php() else {
        println!(
            "SKIP the missing static root behind a valid libphp: this host has no NTS libphp + shim"
        );
        return;
    };
    project.write(
        "Blastfile",
        "entrypoint php \"public/index.php\"\n\nview app \"/\" {}\n\n\
         static \"/assets\" {\n    root \"nowhere\"\n}\n",
    );
    let output = run(
        &project.dir,
        &["check", "--json"],
        &[
            (
                "BLASTIT_LIBPHP",
                host.libphp.to_str().expect("a UTF-8 libphp path"),
            ),
            (
                "BLASTIT_PHP_SHIM_DIR",
                host.shim
                    .parent()
                    .expect("the shim's directory")
                    .to_str()
                    .expect("a UTF-8 shim directory"),
            ),
            ("BLASTIT_PYTHON", shim),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    assert!(
        stderr(&output).contains("static root \"nowhere\""),
        "the missing root is named first: {}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("pytest is not installed"));
    assert!(!marker.exists(), "pytest ran before the static root audit");
}

// ---------------------------------------------------------------------------
// §5 / §7: blastit fmt
// ---------------------------------------------------------------------------

/// A messy file with comments, and its canonical form.
const MESSY: &str = "\
# the shop front
app \"shop\"   # the callable

view products GET \"/products\" { body json ; prefork }
";
const CANONICAL: &str = "\
# the shop front
app \"shop\" # the callable

view products GET \"/products\" {
    body json
    prefork
}
";

/// `fmt` rewrites in place, keeps comments, keeps the mode, leaves no temporary
/// file and is idempotent.
#[test]
fn fmt_rewrites_in_place_preserving_comments_and_is_idempotent() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let project = project("fmt-rewrite");
    let file = project.write("Blastfile", MESSY);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).expect("chmod");
    let output = run(&project.dir, &["fmt"], &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{:?}", text(&output.stdout));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(std::fs::read_to_string(&file).expect("the file"), CANONICAL);
    assert_eq!(
        std::fs::read_dir(&project.dir)
            .expect("the directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "tmp")
            .collect::<Vec<_>>(),
        ["Blastfile"],
        "a temporary file was left behind"
    );
    let mode = std::fs::metadata(&file).expect("the metadata").mode();
    assert_eq!(mode & 0o777, 0o640, "the rewrite changed the mode");

    let settled = identity(&file);
    let output = run(&project.dir, &["fmt"], &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        identity(&file),
        settled,
        "a second fmt rewrote a canonical file"
    );

    // `fmt PATH` names the file, and the default stays byte-identical.
    let other = project.write("tmp/other.Blastfile", MESSY);
    let output = run(&project.dir, &["fmt", "tmp/other.Blastfile"], &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        std::fs::read_to_string(&other).expect("the named file"),
        CANONICAL
    );
    assert_eq!(
        std::fs::read_to_string(&file).expect("the default manifest"),
        CANONICAL,
        "`fmt PATH` rewrote ./Blastfile"
    );
    let output = run(
        &project.dir,
        &["fmt", "--check", "tmp/other.Blastfile"],
        &[],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{:?}", text(&output.stdout));
}

/// `fmt --check` prints a unified diff naming the file, exits `1` and writes
/// nothing — including for CRLF and a missing final newline.
#[test]
fn fmt_check_prints_a_unified_diff_and_writes_nothing() {
    use std::os::unix::fs::PermissionsExt;

    let project = project("fmt-check");
    let file = project.write("Blastfile", MESSY);
    let before = identity(&file);
    std::fs::set_permissions(&project.dir, std::fs::Permissions::from_mode(0o555))
        .expect("chmod the directory read-only");
    let _restore = Restore(project.dir.clone());
    let output = run(&project.dir, &["fmt", "--check"], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with(
            "./Blastfile: not in canonical layout\n--- ./Blastfile\n+++ ./Blastfile\n@@ "
        ),
        "{stdout}"
    );
    assert!(stdout.contains("@@ -1,4 +1,7 @@\n"), "{stdout}");
    assert!(
        stdout.contains("\n-view products GET \"/products\" { body json ; prefork }\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("\n+    body json\n+    prefork\n"),
        "{stdout}"
    );
    assert!(output.stderr.is_empty(), "{}", stderr(&output));
    assert_eq!(identity(&file), before, "--check touched the file");
    assert_eq!(std::fs::read_to_string(&file).expect("the file"), MESSY);
    std::fs::set_permissions(&project.dir, std::fs::Permissions::from_mode(0o755))
        .expect("restore the directory");

    // A canonical file exits `0` with nothing on either stream.
    project.write("Blastfile", CANONICAL);
    let output = run(&project.dir, &["fmt", "--check"], &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{:?}", text(&output.stdout));
    assert!(output.stderr.is_empty(), "{}", stderr(&output));

    // CRLF is a real diff, and the removed line keeps its carriage return.
    project.write("Blastfile", CANONICAL.replace('\n', "\r\n"));
    let output = run(&project.dir, &["fmt", "--check"], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("-app \"shop\" # the callable\r\n"),
        "{stdout:?}"
    );
    assert!(
        stdout.contains("+app \"shop\" # the callable\n"),
        "{stdout:?}"
    );

    // A missing final newline is the line-unchanged, terminator-changed diff.
    project.write("Blastfile", CANONICAL.trim_end_matches('\n'));
    let output = run(&project.dir, &["fmt", "--check"], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        text(&output.stdout).contains("\\ No newline at end of file"),
        "{:?}",
        text(&output.stdout)
    );
}

/// `fmt` writes nothing for a broken, missing or empty-named file, and refuses
/// a second path or an unknown argument.
#[test]
fn fmt_refusals_name_the_file_and_write_nothing() {
    let project = project("fmt-refusals");
    let broken = "view v \"/x\" {\n    body xml\n}\n";
    let file = project.write("Blastfile", broken);
    let before = identity(&file);
    let output = run(&project.dir, &["fmt"], &[]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(output.stdout.is_empty(), "{:?}", text(&output.stdout));
    assert!(
        stderr(&output).starts_with("./Blastfile:2:10: ")
            && stderr(&output).contains("not a codec"),
        "{}",
        stderr(&output)
    );
    assert_eq!(identity(&file), before, "a refused file was rewritten");
    assert_eq!(std::fs::read_to_string(&file).expect("the file"), broken);

    std::fs::remove_file(&file).expect("remove the manifest");
    let output = run(&project.dir, &["fmt"], &[]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        stderr(&output).trim_end(),
        "./Blastfile: no such file or directory (put a Blastfile in the working directory, or \
         set BLASTIT_MANIFEST)"
    );

    project.write("Blastfile", CANONICAL);
    let empty = run(&project.dir, &["fmt"], &[("BLASTIT_MANIFEST", "")]);
    assert_eq!(empty.status.code(), Some(2), "{}", stderr(&empty));
    assert!(
        stderr(&empty).contains("BLASTIT_MANIFEST must not be empty"),
        "{}",
        stderr(&empty)
    );

    for (arguments, expected) in [
        (vec!["fmt", "a", "b"], "is a second path"),
        (vec!["fmt", "--write"], "unknown argument `--write`"),
    ] {
        let output = run(&project.dir, &arguments, &[]);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(stderr(&output).contains(expected), "{}", stderr(&output));
    }
}

/// A `--check` on a CRLF file must not rewrite it: the diff case above proves
/// the bytes; this guard restores a directory mode however the case ends.
struct Restore(PathBuf);

impl Drop for Restore {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// The identity a file keeps when nothing writes it: inode and mtime, to the
/// nanosecond.
fn identity(path: &Path) -> (u64, i64, i64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).expect("the file exists");
    (metadata.ino(), metadata.mtime(), metadata.mtime_nsec())
}

// ---------------------------------------------------------------------------
// §5: discovery — one file, never upward, never a foreign body
// ---------------------------------------------------------------------------

const MISSING: &str = "./Blastfile: no such file or directory (put a Blastfile in the working \
                       directory, or set BLASTIT_MANIFEST)";
const EMPTY: &str = "BLASTIT_MANIFEST must not be empty: put ./Blastfile in the working \
                     directory or name a file";
const FOREIGN: &str = "version = 1\n\n[[route]]\nname = \"one\"\npath = ['.*']\n";

/// `BLASTIT_MANIFEST` wins, the default is `./Blastfile` only, nothing is read
/// above the working directory, an empty variable is refused, a foreign body is
/// an ordinary syntax error, a control byte in a path prints one line, and a
/// FIFO is refused without blocking.
#[test]
fn manifest_discovery_is_one_file_and_never_walks_up() {
    let project = project("discovery");
    write_static_root(&project.dir);
    project.write("Blastfile", "static \"/\" {\n    root \"nowhere\"\n}\n");
    project.write("Other", "static \"/\" {\n    root \"public\"\n}\n");

    let named = run(
        &project.dir,
        &["check", "--json"],
        &[("BLASTIT_MANIFEST", "Other")],
    );
    assert_eq!(named.status.code(), Some(0), "{}", stderr(&named));
    let defaulted = run(&project.dir, &["check", "--json"], &[]);
    assert_eq!(defaulted.status.code(), Some(2), "{}", stderr(&defaulted));
    assert!(
        stderr(&defaulted).contains("static root \"nowhere\""),
        "{}",
        stderr(&defaulted)
    );
    let fmt_named = run(
        &project.dir,
        &["fmt", "--check"],
        &[("BLASTIT_MANIFEST", "Other")],
    );
    assert_eq!(fmt_named.status.code(), Some(0), "{}", stderr(&fmt_named));

    // A parent's file is never read.
    let child = project.path("app");
    std::fs::create_dir_all(&child).expect("the child directory");
    project.write("Blastfile", "static \"/\" {\n    root \"public\"\n}\n");
    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = run(&child, arguments, &[]);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert_eq!(
            stderr(&output).trim_end(),
            format!("blastit check: {MISSING}"),
            "{arguments:?}"
        );
    }
    let fmt = run(&child, &["fmt"], &[]);
    assert_eq!(fmt.status.code(), Some(2), "{}", stderr(&fmt));
    assert_eq!(stderr(&fmt).trim_end(), MISSING);

    // A set-but-empty variable never falls back to the default.
    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = run(&child, arguments, &[("BLASTIT_MANIFEST", "")]);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert_eq!(
            stderr(&output).trim_end(),
            format!("blastit check: {EMPTY}"),
            "{arguments:?}"
        );
    }
    let fmt = run(&child, &["fmt"], &[("BLASTIT_MANIFEST", "")]);
    assert_eq!(stderr(&fmt).trim_end(), EMPTY);

    // A foreign body is the parser's positioned refusal and nothing beside it.
    let foreign = child.join("Foreign");
    std::fs::write(&foreign, FOREIGN).expect("the foreign file");
    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = run(&child, arguments, &[("BLASTIT_MANIFEST", "Foreign")]);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert_foreign(&stderr(&output), "blastit check: ");
    }
    let output = run(&child, &["fmt"], &[("BLASTIT_MANIFEST", "Foreign")]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("Foreign:1:1: "),
        "{}",
        stderr(&output)
    );
    assert_foreign(&stderr(&output), "");
    assert_eq!(
        std::fs::read_to_string(&foreign).expect("the file"),
        FOREIGN
    );

    // An operator's newline stays one escaped line.
    let named = "./a\nb";
    let output = run(&child, &["check", "--json"], &[("BLASTIT_MANIFEST", named)]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stderr(&output).lines().count(), 1, "{:?}", stderr(&output));
    assert!(
        stderr(&output).contains("./a\\nb: no such file or directory"),
        "{}",
        stderr(&output)
    );

    // A FIFO is refused from its metadata, never opened.
    let fifo = child.join("Blastfile");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs")
            .success()
    );
    let output = run_bounded(&child, &["check"], &[]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(stderr(&output).lines().count(), 1, "{:?}", stderr(&output));
    assert!(
        stderr(&output).contains("Blastfile: ") && stderr(&output).contains("not a regular file"),
        "{}",
        stderr(&output)
    );
    let output = run_bounded(&child, &["fmt"], &[]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("not a regular file"),
        "{}",
        stderr(&output)
    );
}

/// The ordinary syntax error, and nothing beside it.
fn assert_foreign(stderr: &str, prefix: &str) {
    let body = stderr
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("expected {prefix:?}: {stderr}"));
    assert!(body.contains("1:1:"), "{stderr}");
    assert!(body.contains("`version` is not a keyword"), "{stderr}");
    assert!(
        body.contains("Foreign") || body.contains("Blastfile"),
        "{stderr}"
    );
    for banned in ["TOML", "migrat", "convert", "detected", "did you mean"] {
        assert!(!body.contains(banned), "{banned:?} leaked: {stderr}");
    }
}
