//! The protected rewrite suite of design §7.2 (`R6`, `R8`, `R12`): the total
//! rewrite order of §4.3 and the method/body gate of §4.4, driven through the
//! production `head_segments` → `resolve` → `decide` composition over the §5.2
//! fixture tree.
//!
//! This is the **pure** half: `decide` is a total function of the resolution,
//! the method, the declared body and the manifest's `app`, so every precedence
//! pair and every status is asserted without a socket or a clock. `MB.2.5.2`
//! appends the flow half (slots, spawns, pump, timeout) to this same file.
//!
//! Every refusal table carries its accepted twin in the same file — an exact
//! file, a directory `index`, the SPA fallback and a `.well-known` token are all
//! served by the suite whose refusals are asserted here — so neither a `decide`
//! that always forwards nor one that always serves can pass.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
#[path = "common/static_fixture.rs"]
mod static_fixture;

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use blastit_events::Outcome;
use blastit_frame::{Frame, INITIAL_WINDOW, ReqHead};
use blastit_manifest::{Manifest, RoutingTable, StaticPolicy, UidLayout};
use blastit_scheduler::rewrite::{Rewrite, STATIC_ALLOW, decide, declares_body};
use blastit_scheduler::routes_from_manifest;
use blastit_scheduler::static_asset::{
    AnswerKind, Backend, STATIC_CHUNK_BYTES, StaticOutcome, StaticRefusal, StaticRoot,
    head_segments,
};
use common::{Options, Script, World, aborted, body_of, ended, head, status_of};
use static_fixture::{
    APP_JS_BYTES, CAFE_BYTES, DOCS_HTML, Fixture, INDEX_HTML, StaticTree, TOKEN_BYTES,
};

/// Every method shape the gate has to separate: the two it answers, the four
/// §4.4 names, and three that prove the comparison is exact bytes (`get` is
/// just another method).
const METHODS: [&str; 9] = [
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "get", "",
];

/// The seven ways a decoded path can be malformed: all of them are a `400`
/// (design §5.2 rows 1–13, `R7`).
const REFUSALS: [StaticRefusal; 7] = [
    StaticRefusal::Traversal,
    StaticRefusal::Absolute,
    StaticRefusal::Backslash,
    StaticRefusal::Control,
    StaticRefusal::Encoding,
    StaticRefusal::TooLong,
    StaticRefusal::SegmentTooLong,
];

/// A `Serve`, with the served file actually read: a `200` that is not this file
/// cannot pass.
type Answer = (String, AnswerKind, u64, Vec<u8>);

fn answer(url_path: &str, kind: AnswerKind, bytes: &[u8]) -> Answer {
    (
        url_path.to_string(),
        kind,
        bytes.len() as u64,
        bytes.to_vec(),
    )
}

/// Design §4.3 steps 3–11 for one raw request target: the production head
/// (`head_segments`: query cut, one decode, `segments`, the dotfile rule) and
/// then the filesystem half, so every refusal is produced the way the scheduler
/// produces it — never hand-built.
fn resolve(root: &StaticRoot, target: &str) -> StaticOutcome {
    let owned = match head_segments(target) {
        Ok(parts) => parts,
        Err(outcome) => return outcome,
    };
    let parts: Vec<&str> = owned.iter().map(String::as_str).collect();
    root.resolve(&parts)
}

/// The shape of a resolution, for the outcomes that carry no payload.
fn shape(outcome: &StaticOutcome) -> &'static str {
    match outcome {
        StaticOutcome::File(_) => "file",
        StaticOutcome::Missing => "missing",
        StaticOutcome::SpaLost => "spa-lost",
        StaticOutcome::Hidden => "hidden",
        StaticOutcome::Refused(_) => "refused",
    }
}

fn describe(rewrite: &Rewrite) -> String {
    match rewrite {
        Rewrite::Serve(file) => format!("serve:{:?}:{}", file.kind, file.url_path),
        Rewrite::Forward => "forward".to_string(),
        Rewrite::Reject { status, allow } => format!("reject:{status}:{allow:?}"),
    }
}

fn served(rewrite: Rewrite) -> Answer {
    match rewrite {
        Rewrite::Serve(file) => {
            let mut body = Vec::new();
            std::fs::File::from(file.file)
                .read_to_end(&mut body)
                .expect("reading the served file");
            (file.url_path, file.kind, file.len, body)
        }
        other => panic!("expected a Serve, got {}", describe(&other)),
    }
}

fn forwarded(rewrite: Rewrite) {
    assert!(
        matches!(rewrite, Rewrite::Forward),
        "expected a Forward, got {}",
        describe(&rewrite)
    );
}

/// A rejection of `status`, with exactly `Allow: GET, HEAD` on the `405` and no
/// `Allow` on any other status (design §4.4).
fn rejected(rewrite: Rewrite, status: u16) {
    match rewrite {
        Rewrite::Reject {
            status: seen,
            allow,
        } => {
            assert_eq!(seen, status, "status");
            if status == 405 {
                assert_eq!(allow, Some(&b"GET, HEAD"[..]), "the one Allow value");
            } else {
                assert_eq!(allow, None, "only the 405 carries Allow");
            }
        }
        other => panic!("expected a {status} rejection, got {}", describe(&other)),
    }
}

// ---------------------------------------------------------------------------
// §4.3, first match wins: a file, a directory `index`, then the fallback.
// ---------------------------------------------------------------------------

#[test]
fn an_exact_file_is_served_for_every_alias_and_never_forwarded() {
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);
    let exact = answer(
        "/assets/app.4f1c2a9b7d3e.js",
        AnswerKind::Exact,
        APP_JS_BYTES,
    );

    // The query is not part of resolution, a trailing empty segment is dropped
    // and an interior `//` collapses: one file, one key (rows 32, 33, 40).
    for target in [
        "/assets/app.4f1c2a9b7d3e.js",
        "/assets/app.4f1c2a9b7d3e.js/",
        "/assets/app.4f1c2a9b7d3e.js?v=1",
        "/assets//app.4f1c2a9b7d3e.js",
    ] {
        // The manifest's `app` never turns a file into a forward: a static
        // answer outranks the front controller.
        for has_app in [false, true] {
            assert_eq!(
                served(decide(resolve(&root, target), b"GET", false, has_app)),
                exact,
                "{target} with has_app={has_app}"
            );
        }
        // `HEAD` still selects the file; omitting the body is `respond`'s job.
        assert_eq!(
            served(decide(resolve(&root, target), b"HEAD", false, true)),
            exact,
            "{target} as HEAD"
        );
    }

    // The `.well-known` exemption is the other exact file that proves the
    // refusals below are not a blanket 404 (row 18).
    assert_eq!(
        served(decide(
            resolve(&root, "/.well-known/acme-challenge/token"),
            b"GET",
            false,
            true
        )),
        answer(
            "/.well-known/acme-challenge/token",
            AnswerKind::Exact,
            TOKEN_BYTES
        )
    );
}

#[test]
fn a_directory_answers_its_index_and_both_spellings_are_one_file() {
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);

    // No redirect: `/docs` and `/docs/` are the same answer, byte for byte
    // (row 28).
    for target in ["/docs", "/docs/"] {
        assert_eq!(
            served(decide(resolve(&root, target), b"GET", false, true)),
            answer("/docs/index.html", AnswerKind::DirectoryIndex, DOCS_HTML),
            "{target}"
        );
    }
    // Naming the file makes the same bytes an exact answer instead.
    assert_eq!(
        served(decide(
            resolve(&root, "/docs/index.html"),
            b"GET",
            false,
            true
        )),
        answer("/docs/index.html", AnswerKind::Exact, DOCS_HTML)
    );
    // The root directory is the root `index`, and `/index.html` names it.
    assert_eq!(
        served(decide(resolve(&root, "/"), b"GET", false, true)),
        answer("/index.html", AnswerKind::DirectoryIndex, INDEX_HTML)
    );
    assert_eq!(
        served(decide(resolve(&root, "/index.html"), b"GET", false, true)),
        answer("/index.html", AnswerKind::Exact, INDEX_HTML)
    );
}

#[test]
fn a_miss_on_a_spa_route_answers_index_and_never_the_near_miss_file() {
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);

    // Rows 14, 19, 31 and 38 are misses; with `spa = true` and an `app`
    // declared the fallback still answers first (SPA outranks the front
    // controller).
    for target in [
        "/dashboard/settings",
        "/DOCS",
        "/cafe%CC%81.js",
        "/%252e%252e/%252e%252e/etc/passwd",
        "/.well-known/missing",
    ] {
        assert_eq!(
            served(decide(resolve(&root, target), b"GET", false, true)),
            answer("/index.html", AnswerKind::SpaFallback, INDEX_HTML),
            "{target}"
        );
    }
    // The NFC file is served only under its own name (row 37): the near miss
    // above is the index, never these bytes.
    assert_eq!(
        served(decide(resolve(&root, "/caf%C3%A9.js"), b"GET", false, true)),
        answer("/café.js", AnswerKind::Exact, CAFE_BYTES)
    );
}

#[test]
fn a_miss_forwards_only_with_an_app_and_keeps_every_method_and_body() {
    let tree = StaticTree::build(Fixture::F);
    let root = tree.open(Fixture::F, Backend::Openat2);

    // `spa` is unset, so a miss is the app's: every method, and a declared body
    // too, because the route's own `[route.ingress]` decides there (§4.4).
    for method in METHODS {
        forwarded(decide(
            resolve(&root, "/missing"),
            method.as_bytes(),
            false,
            true,
        ));
    }
    for method in ["GET", "HEAD"] {
        forwarded(decide(
            resolve(&root, "/missing"),
            method.as_bytes(),
            true,
            true,
        ));
    }

    // Without an `app` the same miss is a 404 — the pair that proves `Forward`
    // is not the fallback answer.
    for method in METHODS {
        for has_body in [false, true] {
            rejected(
                decide(
                    resolve(&root, "/missing"),
                    method.as_bytes(),
                    has_body,
                    false,
                ),
                404,
            );
        }
    }
}

#[test]
fn a_route_without_spa_answers_404_instead_of_falling_back() {
    let tree = StaticTree::build(Fixture::N);
    let root = tree.open(Fixture::N, Backend::Openat2);

    for target in [
        "/missing",
        "/dashboard",
        "/DOCS",
        "/.well-known/missing",
        "/assets",
    ] {
        for method in ["GET", "POST"] {
            rejected(
                decide(resolve(&root, target), method.as_bytes(), false, false),
                404,
            );
        }
    }
    // It is not a blanket 404: the route still answers what it has.
    assert_eq!(
        served(decide(resolve(&root, "/"), b"GET", false, false)),
        answer("/index.html", AnswerKind::DirectoryIndex, INDEX_HTML)
    );
    assert_eq!(
        served(decide(resolve(&root, "/docs"), b"GET", false, false)),
        answer("/docs/index.html", AnswerKind::DirectoryIndex, DOCS_HTML)
    );
}

#[test]
fn a_refusal_is_final_even_with_a_spa_fallback_and_an_app_armed() {
    // Fixture S has `spa = true`, so both traps — the fallback and the app —
    // are armed for every row below.
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);
    let long_path = format!("/{}.js", "a".repeat(4096));
    let long_segment = format!("/{}", "b".repeat(256));

    // Rows 1–13: a malformed path is a `400`, whatever the refusal was.
    let malformed = [
        ("/../etc/passwd", StaticRefusal::Traversal),
        ("/%2e%2e/%2e%2e/etc/passwd", StaticRefusal::Traversal),
        ("/..%2f..%2fetc%2fpasswd", StaticRefusal::Traversal),
        (
            "/assets/app.4f1c2a9b7d3e.js%2f..%2f..%2fetc%2fpasswd",
            StaticRefusal::Traversal,
        ),
        ("/%2Fetc/passwd", StaticRefusal::Absolute),
        ("//etc/passwd", StaticRefusal::Absolute),
        ("/foo%5c..%5cbar", StaticRefusal::Backslash),
        ("/%c0%ae%c0%ae/etc/passwd", StaticRefusal::Encoding),
        ("/%00.js", StaticRefusal::Control),
        ("/%1f.js", StaticRefusal::Control),
        ("/%ff.js", StaticRefusal::Encoding),
        ("/%zz", StaticRefusal::Encoding),
        (long_path.as_str(), StaticRefusal::TooLong),
        (long_segment.as_str(), StaticRefusal::SegmentTooLong),
    ];
    for (target, refusal) in malformed {
        match resolve(&root, target) {
            StaticOutcome::Refused(seen) => assert_eq!(seen, refusal, "{target}"),
            other => panic!(
                "{target}: expected Refused({refusal:?}), got {}",
                shape(&other)
            ),
        }
        for method in ["GET", "HEAD", "POST"] {
            rejected(
                decide(resolve(&root, target), method.as_bytes(), false, true),
                400,
            );
        }
    }

    // Rows 15–17, 20–26 and 39: an entry that exists but is not servable is a
    // `404` — never the fallback, never the app.
    for target in [
        "/.env",
        "/.git/config",
        "/.git/",
        "/.well-known/.env",
        "/link",
        "/link2/x",
        "/fifo",
        "/sock",
        "/mode000",
        "/big.bin",
        "/proclink/self/fd/0",
    ] {
        assert_eq!(shape(&resolve(&root, target)), "hidden", "{target}");
        for method in ["GET", "POST"] {
            rejected(
                decide(resolve(&root, target), method.as_bytes(), false, true),
                404,
            );
        }
    }
    // A `..` outranks the `.well-known` exemption: the refusal is the `400`,
    // not the hidden `404`.
    assert_eq!(shape(&resolve(&root, "/.well-known/../.env")), "refused");
    rejected(
        decide(resolve(&root, "/.well-known/../.env"), b"GET", false, true),
        400,
    );
}

#[test]
fn a_lost_spa_fallback_is_a_404_and_never_the_app() {
    let tree = StaticTree::build(Fixture::S);
    // `spa = true` and `root/index` missing: step 8, final ("a fallback whose
    // file is missing is a `404`").
    std::fs::remove_file(tree.path("index.html")).expect("remove the fallback");
    let root = tree.open(Fixture::S, Backend::Openat2);

    for target in ["/missing", "/", "/dashboard"] {
        assert_eq!(shape(&resolve(&root, target)), "spa-lost", "{target}");
        for method in ["GET", "POST"] {
            for has_body in [false, true] {
                rejected(
                    decide(resolve(&root, target), method.as_bytes(), has_body, true),
                    404,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// §4.4: the method gate first, then the declared body.
// ---------------------------------------------------------------------------

#[test]
fn the_method_gate_is_405_with_exactly_allow_get_head() {
    assert_eq!(STATIC_ALLOW, &b"GET, HEAD"[..]);
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);

    // A file, a directory index, the root index, the SPA fallback and the
    // `.well-known` token are all paths this responder answers — including
    // `POST /dashboard` on a `spa = true` route.
    for target in [
        "/assets/app.4f1c2a9b7d3e.js",
        "/docs",
        "/",
        "/dashboard",
        "/.well-known/acme-challenge/token",
    ] {
        for method in METHODS {
            if method == "GET" || method == "HEAD" {
                assert!(
                    matches!(
                        decide(resolve(&root, target), method.as_bytes(), false, true),
                        Rewrite::Serve(_)
                    ),
                    "{target} as {method}"
                );
                continue;
            }
            rejected(
                decide(resolve(&root, target), method.as_bytes(), false, true),
                405,
            );
        }
    }
}

#[test]
fn a_get_or_head_that_declares_a_body_is_413_and_the_method_gate_runs_first() {
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);

    for target in ["/assets/app.4f1c2a9b7d3e.js", "/docs", "/dashboard"] {
        for method in ["GET", "HEAD"] {
            for has_app in [false, true] {
                rejected(
                    decide(resolve(&root, target), method.as_bytes(), true, has_app),
                    413,
                );
                assert!(
                    matches!(
                        decide(resolve(&root, target), method.as_bytes(), false, has_app),
                        Rewrite::Serve(_)
                    ),
                    "{target} as {method} without a body"
                );
            }
        }
        // The method gate outranks the body gate: a body on a `POST` is a 405.
        rejected(decide(resolve(&root, target), b"POST", true, true), 405);
    }

    // A `Forward` keeps the app's own plan: the body is `[route.ingress]`'s
    // business, and a miss without an app is still the 404.
    forwarded(decide(StaticOutcome::Missing, b"GET", true, true));
    rejected(decide(StaticOutcome::Missing, b"GET", true, false), 404);
}

#[test]
fn declares_body_reads_the_head_and_never_a_body() {
    fn head(pairs: &[(&str, &str)]) -> Vec<(Vec<u8>, Vec<u8>)> {
        pairs
            .iter()
            .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
            .collect()
    }

    assert!(!declares_body(&[]));
    assert!(!declares_body(&head(&[("Host", "example.com")])));
    assert!(!declares_body(&head(&[("Content-Length", "0")])));
    assert!(!declares_body(&head(&[("content-length", "00")])));
    assert!(!declares_body(&head(&[("X-Content-Length", "5")])));
    assert!(declares_body(&head(&[("Content-Length", "1")])));
    assert!(declares_body(&head(&[("Content-Length", "1024")])));
    // Any `Transfer-Encoding` declares a body, whatever its value.
    assert!(declares_body(&head(&[("Transfer-Encoding", "chunked")])));
    assert!(declares_body(&head(&[("transfer-encoding", "gzip")])));
    assert!(declares_body(&head(&[
        ("Content-Length", "0"),
        ("Transfer-Encoding", "chunked")
    ])));
    // A length that is not a number is not a zero either: fail closed.
    assert!(declares_body(&head(&[("Content-Length", "not-a-number")])));
}

#[test]
fn the_status_mapping_is_total_over_every_outcome_and_every_request() {
    let tree = StaticTree::build(Fixture::S);
    let root = tree.open(Fixture::S, Backend::Openat2);
    let asset = || resolve(&root, "/assets/app.4f1c2a9b7d3e.js");

    for method in METHODS {
        for has_body in [false, true] {
            for has_app in [false, true] {
                // A refusal is final for every method and body, with or
                // without an app.
                for refusal in REFUSALS {
                    rejected(
                        decide(
                            StaticOutcome::Refused(refusal),
                            method.as_bytes(),
                            has_body,
                            has_app,
                        ),
                        400,
                    );
                }
                rejected(
                    decide(StaticOutcome::Hidden, method.as_bytes(), has_body, has_app),
                    404,
                );
                rejected(
                    decide(StaticOutcome::SpaLost, method.as_bytes(), has_body, has_app),
                    404,
                );

                // A miss is the app's, or a `404`: never a static answer.
                let miss = decide(StaticOutcome::Missing, method.as_bytes(), has_body, has_app);
                if has_app {
                    forwarded(miss);
                } else {
                    rejected(miss, 404);
                }

                // A file is served, refused by method, or refused by body —
                // never forwarded, whatever the method and the app.
                let decision = decide(asset(), method.as_bytes(), has_body, has_app);
                if method != "GET" && method != "HEAD" {
                    rejected(decision, 405);
                } else if has_body {
                    rejected(decision, 413);
                } else {
                    assert!(
                        matches!(decision, Rewrite::Serve(_)),
                        "{method} without a body is a Serve, got {}",
                        describe(&decision)
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// §4.3 steps 1–2 and §4.6: which route matched, and what may forward.
// ---------------------------------------------------------------------------

#[test]
fn an_app_route_matches_before_a_later_static_route() {
    let front_first = "version = 1\napp = \"app:app\"\n\n\
         [[route]]\nname = \"app\"\npath = ['.*']\n\n\
         [[route]]\nname = \"site\"\npath = ['.*']\n\n\
         [route.static]\nroot = \"public\"\n";
    let static_first = "version = 1\napp = \"app:app\"\n\n\
         [[route]]\nname = \"site\"\npath = ['.*']\n\n\
         [route.static]\nroot = \"public\"\n\n\
         [[route]]\nname = \"app\"\npath = ['.*']\n";

    // First route in manifest order wins, and `[static]` adds no matching rule:
    // a static route placed after an app catch-all is unreachable (§4.3).
    let manifest = Manifest::from_toml_str(front_first).unwrap();
    let table = RoutingTable::new(&manifest).unwrap();
    let routes = routes_from_manifest(&manifest, &UidLayout::new(20_000));
    assert_eq!(
        table.route("GET", "/assets/app.js").map(|hit| hit.route),
        Some(0)
    );
    assert_eq!(routes[0].static_policy, None);
    assert!(
        !routes[0].static_forward,
        "a route with no policy never forwards statically"
    );
    assert_eq!(
        routes[1].static_policy,
        Some(StaticPolicy {
            root: "public".to_string(),
            index: "index.html".to_string(),
            spa: false,
            immutable: Vec::new(),
        })
    );
    assert!(
        routes[1].static_forward,
        "no spa, and the manifest declares an app"
    );

    // Swapping the two routes swaps which one wins, so the order — not the
    // presence of `[static]` — decides. `decide` is reachable only for the
    // route that matched.
    let manifest = Manifest::from_toml_str(static_first).unwrap();
    let table = RoutingTable::new(&manifest).unwrap();
    let routes = routes_from_manifest(&manifest, &UidLayout::new(20_000));
    assert_eq!(
        table.route("GET", "/assets/app.js").map(|hit| hit.route),
        Some(0)
    );
    assert!(routes[0].static_policy.is_some());
    assert_eq!(routes[1].static_policy, None);
}

#[test]
fn a_static_policy_can_forward_only_without_spa_and_with_an_app() {
    fn policy(spa: bool) -> StaticPolicy {
        StaticPolicy {
            root: "public".to_string(),
            index: "index.html".to_string(),
            spa,
            immutable: Vec::new(),
        }
    }

    // `spa = true` answers every miss itself, so the route never spawns a view.
    let spa_route = routes_from_manifest(
        &Manifest::from_toml_str(
            "version = 1\napp = \"app:app\"\n[[route]]\nname = \"site\"\npath = ['.*']\n\
             [route.static]\nroot = \"public\"\nspa = true\n",
        )
        .unwrap(),
        &UidLayout::new(20_000),
    );
    assert_eq!(spa_route[0].static_policy, Some(policy(true)));
    assert!(!spa_route[0].static_forward);

    // No `app` in the manifest: nothing to forward to either.
    let no_app = routes_from_manifest(
        &Manifest::from_toml_str(
            "version = 1\n[[route]]\nname = \"site\"\npath = ['.*']\n\
             [route.static]\nroot = \"public\"\n",
        )
        .unwrap(),
        &UidLayout::new(20_000),
    );
    assert_eq!(no_app[0].static_policy, Some(policy(false)));
    assert!(!no_app[0].static_forward, "no app: nothing to forward to");
}

// ---------------------------------------------------------------------------
// The flow half (`MB.2.5.2`, design §4.6, §7.2): admission, the pump, the slot.
//
// Everything below runs the real `run` loop against a fake init and a fake
// client (`tests/common`), because what has to be proven here is not what
// `decide` says but when the scheduler says it: that a static answer takes one
// slot, keeps it until its read result, and gives it back exactly once.
// ---------------------------------------------------------------------------

/// The page's route over the §5.2 fixture tree, with an `app` declared so the
/// fallback and the front controller are both armed: nothing a refusal does may
/// be masked by either (design §4.4).
const SITE_MANIFEST: &str = "version = 1\napp = \"app:app\"\n\n\
     [[route]]\nname = \"site\"\npath = ['.*']\n\n\
     [route.static]\nroot = \"root\"\nspa = true\nimmutable = [\"/assets/*\"]\n";

/// The same tree with `spa` unset and no `app`: an all-static route that must
/// never fork a view (design §4.6).
const PUMP_MANIFEST: &str = "version = 1\n\n\
     [[route]]\nname = \"site\"\npath = ['.*']\n\n\
     [route.static]\nroot = \"root\"\n";

/// The front controller of design §4.4: a miss forwards with the route's own
/// `[route.ingress]`, so the static slot has to be handed back for the view to
/// be spawned.
const FRONT_MANIFEST: &str = "version = 1\napp = \"app:app\"\n\n\
     [[route]]\nname = \"site\"\npath = ['.*']\n\n\
     [route.static]\nroot = \"root\"\n\n\
     [route.ingress]\nbody = \"raw\"\nallow_raw = true\n";

/// The same front controller with a structured body: its `plan_body` error must
/// be stored, not answered, while a `Serve` still answers the page's own rule.
const JSON_FRONT_MANIFEST: &str = "version = 1\napp = \"app:app\"\n\n\
     [[route]]\nname = \"site\"\npath = ['.*']\n\n\
     [route.static]\nroot = \"root\"\n\n\
     [route.ingress]\nbody = \"json\"\n";

/// A request head carrying `headers` on top of the shared one.
fn head_with(method: &str, target: &str, headers: &[(&str, &str)]) -> ReqHead {
    let mut head = head(method, target);
    for (name, value) in headers {
        head.headers
            .push((name.as_bytes().to_vec(), value.as_bytes().to_vec()));
    }
    head
}

/// The value of the first header named `name`, compared ASCII case-insensitively.
fn header_value<'a>(headers: &'a [(Vec<u8>, Vec<u8>)], name: &[u8]) -> Option<&'a [u8]> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_slice())
}

/// The head frame of a response.
fn response_head(frames: &[Frame]) -> &blastit_frame::RespHead {
    match frames.first() {
        Some(Frame::RespHead(head)) => head,
        other => panic!("no response head: {other:?}"),
    }
}

/// The streams whose `RESP_HEAD` arrived and whose response has not ended: the
/// static responses that are *in flight* right now.
fn in_flight(world: &mut World) -> Vec<u32> {
    let mut heads: Vec<u32> = Vec::new();
    let mut finished: Vec<u32> = Vec::new();
    for (id, frame) in world.http.leftover() {
        match frame {
            Frame::RespHead(_) => {
                if !heads.contains(&id) {
                    heads.push(id);
                }
            }
            Frame::RespEnd | Frame::RespEndCost { .. } | Frame::RespAbort { .. } => {
                finished.push(id);
            }
            _ => {}
        }
    }
    heads.retain(|id| !finished.contains(id));
    heads
}

/// Every stream that has sent the client *any* frame.
fn streams_with_frames(world: &mut World) -> Vec<u32> {
    let mut seen: Vec<u32> = world
        .http
        .leftover()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    seen.sort_unstable();
    seen.dedup();
    seen
}

/// How many body frames the client is holding, across every stream.
///
/// A static stream that has spent its credit window has pushed exactly
/// `INITIAL_WINDOW / 64 KiB` of them, so this is how a test knows the pump has
/// parked rather than merely started.
fn body_frames(world: &mut World) -> usize {
    world
        .http
        .leftover()
        .iter()
        .filter(|(_, frame)| matches!(frame, Frame::RespBody(_)))
        .count()
}

/// The body frames of one credit window (`STATIC_CHUNK_BYTES` each).
fn window_chunks() -> usize {
    (INITIAL_WINDOW / 64 / 1024) as usize
}

/// Spins until `condition` holds, polling the client each turn and never
/// letting the paused clock move (`World::until` cannot poll the client).
///
/// The turn count is deliberately small: inspecting the client clones the body
/// bytes it holds, so a condition that never holds has to fail quickly rather
/// than spend minutes cloning megabytes. A working pump needs a few dozen turns
/// per read, and a stream is at most two windows long here.
async fn until_pumped(world: &mut World, mut condition: impl FnMut(&mut World) -> bool) {
    for _ in 0..4_000 {
        world.http.poll();
        if condition(world) {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!(
        "condition never became true; heads in flight: {:?}, body frames: {}",
        in_flight(world),
        body_frames(world)
    );
}

/// A tree of the sizes the pump needs, in `std::env::temp_dir()`, removed on
/// drop.
///
/// The §5.2 fixture is another leaf's file and every file in it fits one credit
/// window, so it cannot park a read: these files are what make "the slot is held
/// until the read result" observable, and they are sparse, so a `pread` of them
/// costs no disk.
struct PumpTree {
    base: PathBuf,
}

impl PumpTree {
    /// `<base>/root/<name>`, `size` bytes of zeros, for every `(name, size)`.
    fn build(files: &[(&str, u64)]) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let base =
            std::env::temp_dir().join(format!("blastit-pump-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(base.join("root")).expect("pump root directory");
        for (name, size) in files {
            let path = base.join("root").join(name);
            let file = std::fs::File::create(&path)
                .unwrap_or_else(|error| panic!("pump {}: {error}", path.display()));
            file.set_len(*size)
                .unwrap_or_else(|error| panic!("pump {} length: {error}", path.display()));
        }
        // The descriptor oracle compares a target with this path, so it has to
        // be the resolved one whatever `TMPDIR` points at.
        let base = std::fs::canonicalize(&base).expect("canonical pump base");
        Self { base }
    }

    /// `<base>/root/<rel>`.
    fn path(&self, rel: &str) -> PathBuf {
        self.base.join("root").join(rel)
    }

    /// How many open descriptors point inside this tree: one for the root's
    /// dirfd, plus one per static body in flight or parked (design §7.1's
    /// reader bound).
    fn open_in_tree(&self) -> usize {
        let Ok(entries) = std::fs::read_dir("/proc/self/fd") else {
            return 0;
        };
        entries
            .flatten()
            .filter(|entry| {
                std::fs::read_link(entry.path())
                    .map(|target| target.starts_with(&self.base))
                    .unwrap_or(false)
            })
            .count()
    }

    /// Truncates a file the scheduler already holds open: the next `pread` of it
    /// comes up short, which is the abort path of design §4.6.
    fn truncate(&self, rel: &str) {
        std::fs::OpenOptions::new()
            .write(true)
            .open(self.path(rel))
            .expect("open the pump file")
            .set_len(0)
            .expect("truncate the pump file");
    }
}

impl Drop for PumpTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A world serving `PUMP_MANIFEST` over `tree`, with `max_workers` slots.
fn pump_world(tree: &PumpTree, max_workers: u32) -> World {
    let mut options = Options::new().manifest(PUMP_MANIFEST);
    options.cfg.manifest_dir = tree.base.clone();
    options.cfg.max_workers = max_workers;
    options.cfg.queue_size = 8;
    options.cfg.queue_timeout = Duration::from_secs(5);
    options.cfg.request_timeout = Duration::from_secs(30);
    World::start(options)
}

/// The bytes of a response body, checked to be the sparse file's zeros.
fn zeros(frames: &[Frame], expected: u64) -> Vec<u8> {
    let body = body_of(frames);
    assert_eq!(body.len() as u64, expected, "every byte of the file");
    assert!(
        body.iter().all(|byte| *byte == 0),
        "the sparse file's zeros"
    );
    body
}

#[tokio::test]
async fn a_serve_reaches_the_wire_with_the_responders_own_bytes_and_headers() {
    tokio::time::pause();
    let tree = StaticTree::build(Fixture::S);
    let mut options = Options::new().manifest(SITE_MANIFEST);
    options.cfg.manifest_dir = tree.base.clone();
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head("GET", "/assets/app.4f1c2a9b7d3e.js"), b"")
        .await;
    let frames = world.http.response(1).await;

    assert_eq!(status_of(&frames), 200);
    assert!(ended(&frames), "{frames:?}");
    assert_eq!(
        body_of(&frames),
        APP_JS_BYTES,
        "the exact bytes of the file the route serves"
    );

    // The head is the responder's, header for header. It is compared against a
    // fresh `respond` of the same file — the `ETag` is size and mtime, so it is
    // not something this test may restate — and the four documented values are
    // also read by name so a responder that dropped one cannot pass.
    let root = tree.open(Fixture::S, Backend::Openat2);
    let StaticOutcome::File(file) = resolve(&root, "/assets/app.4f1c2a9b7d3e.js") else {
        panic!("the fixture asset resolves");
    };
    let expected = root.respond(file, b"GET", None);
    assert_eq!(expected.status, 200);
    let wire = response_head(&frames);
    assert_eq!(wire.status, expected.status);
    assert_eq!(wire.reason, expected.reason.as_bytes());
    assert_eq!(wire.headers, expected.headers, "the responder's own head");
    let length = APP_JS_BYTES.len().to_string();
    assert_eq!(
        header_value(&wire.headers, b"Content-Type"),
        Some(&b"text/javascript"[..]),
        "the closed table's row for .js"
    );
    assert_eq!(
        header_value(&wire.headers, b"Cache-Control"),
        Some(&b"public, max-age=31536000, immutable"[..]),
        "an exact `Exact` answer that matches `/assets/*`"
    );
    assert_eq!(
        header_value(&wire.headers, b"X-Content-Type-Options"),
        Some(&b"nosniff"[..])
    );
    assert_eq!(
        header_value(&wire.headers, b"Content-Length"),
        Some(length.as_bytes())
    );

    // One `request` event, with the route, the rule and `ok`; the responder is
    // the answer, so no view was forked and no view saw a `REQ_HEAD` (design
    // §7.2's `R12`).
    world.until(|world| !world.requests().is_empty()).await;
    let events = world.requests();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    assert_eq!(events[0].rule(), Some(0));
    assert_eq!(events[0].outcome(), Outcome::Ok);
    assert_eq!(events[0].status(), 200);
    assert_eq!(events[0].bytes_out(), APP_JS_BYTES.len() as u64);
    assert_eq!(world.request_routes(), vec![Some("site".to_string())]);
    assert!(
        world.log.lock().unwrap().spawns.is_empty(),
        "a pure static route never forks a view"
    );
    assert!(
        world.log.lock().unwrap().received.is_empty(),
        "no view ever saw this request"
    );
}

#[tokio::test]
async fn a_head_answers_the_get_head_and_never_a_body_frame() {
    tokio::time::pause();
    let tree = StaticTree::build(Fixture::S);
    let mut options = Options::new().manifest(SITE_MANIFEST);
    options.cfg.manifest_dir = tree.base.clone();
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head("HEAD", "/assets/app.4f1c2a9b7d3e.js"), b"")
        .await;
    let frames = world.http.response(1).await;

    assert_eq!(status_of(&frames), 200);
    assert!(ended(&frames), "{frames:?}");
    assert_eq!(
        frames.len(),
        2,
        "a head and an end, no body frame: {frames:?}"
    );
    assert!(matches!(frames[1], Frame::RespEnd));
    assert_eq!(body_of(&frames), b"", "HEAD is GET without a body");
    // The `Content-Length` of the body it did not write is still promised.
    let length = APP_JS_BYTES.len().to_string();
    assert_eq!(
        header_value(&response_head(&frames).headers, b"Content-Length"),
        Some(length.as_bytes())
    );

    world.until(|world| !world.requests().is_empty()).await;
    let events = world.requests();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    assert_eq!(events[0].status(), 200);
    assert_eq!(events[0].bytes_out(), 0, "not one byte was read out");
}

#[tokio::test]
async fn a_static_refusal_is_one_scheduler_answer_with_no_store() {
    tokio::time::pause();
    let tree = StaticTree::build(Fixture::S);
    let mut options = Options::new().manifest(SITE_MANIFEST);
    options.cfg.manifest_dir = tree.base.clone();
    let mut world = World::start(options);

    // `(request, status, body)`: a malformed path, a dotfile, a servable path
    // with the wrong method and one that declares a body (§4.4). The route has
    // `spa = true` and an `app`, so a refusal that leaked into either the
    // fallback or the app would answer 200 instead.
    let rows: [(ReqHead, u16, &[u8]); 4] = [
        (head("GET", "/../etc/passwd"), 400, b"bad request\n"),
        (head("GET", "/.env"), 404, b"not found\n"),
        (
            head("POST", "/assets/app.4f1c2a9b7d3e.js"),
            405,
            b"method not allowed\n",
        ),
        (
            head_with(
                "GET",
                "/assets/app.4f1c2a9b7d3e.js",
                &[("Content-Length", "1")],
            ),
            413,
            b"content too large\n",
        ),
    ];
    for (index, (head, status, body)) in rows.into_iter().enumerate() {
        let stream = index as u32 + 1;
        world.http.send_request(stream, head, b"").await;
        let frames = world.http.response(stream).await;
        assert_eq!(status_of(&frames), status, "{frames:?}");
        assert_eq!(body_of(&frames), body, "the canned body of {status}");
        let wire = response_head(&frames);
        let headers = &wire.headers;
        if status == 405 {
            // The real answer to a `POST` on a static path: a reason phrase and
            // a body of its own, and exactly one `Allow` value — never the
            // generic `Error`/`error\n` fallback.
            assert_eq!(wire.reason, b"Method Not Allowed");
            assert_eq!(
                header_value(headers, b"Allow"),
                Some(STATIC_ALLOW),
                "the one Allow value"
            );
        } else {
            assert_eq!(
                header_value(headers, b"Allow"),
                None,
                "only the 405 carries Allow"
            );
        }
        assert_eq!(
            header_value(headers, b"Cache-Control"),
            Some(&b"no-store"[..]),
            "every scheduler-written static refusal is no-store"
        );
    }

    // Each is one event, `unmatched`, and none of them reached a view.
    world.until(|world| world.requests().len() == 4).await;
    for event in world.requests() {
        assert_eq!(event.outcome(), Outcome::Unmatched);
        assert_eq!(
            event.rule(),
            Some(0),
            "the refusal keeps its route and rule"
        );
    }
    assert!(
        world.log.lock().unwrap().spawns.is_empty(),
        "a refusal never forks the app"
    );
}

#[tokio::test]
async fn more_than_max_workers_static_streams_complete_in_waves_inside_the_slot_bound() {
    // Design §4.6, and the reason the slot is released at the read result: a
    // pump that handed the slot back at `RESP_HEAD` would dispatch all three
    // streams at once, so the third would have a head while the first two are
    // parked — and the tree would hold three descriptors, not `max_workers`.
    tokio::time::pause();
    let window = u64::from(INITIAL_WINDOW);
    let size = window + 64 * 1024;
    let tree = PumpTree::build(&[
        ("a.bin", size),
        ("b.bin", size),
        ("c.bin", size),
        ("small.js", 24),
    ]);
    let mut world = pump_world(&tree, 2);

    until_pumped(&mut world, |_| tree.open_in_tree() >= 1).await;
    let baseline = tree.open_in_tree();
    assert_eq!(baseline, 1, "one dirfd per declared static root");

    // Three streams that need more than one credit window, and one that needs
    // none: with two slots the last one can only answer when a slot comes back.
    world.http.send_request(1, head("GET", "/a.bin"), b"").await;
    world.http.send_request(2, head("GET", "/b.bin"), b"").await;
    world.http.send_request(3, head("GET", "/c.bin"), b"").await;
    world
        .http
        .send_request(4, head("GET", "/small.js"), b"")
        .await;

    // Exactly two are in flight, both have pumped their whole first window, and
    // neither has ended: the two streams progress together on one slot each.
    until_pumped(&mut world, |world| {
        in_flight(world).len() == 2 && body_frames(world) == 2 * window_chunks()
    })
    .await;
    assert_eq!(in_flight(&mut world), vec![1, 2]);
    assert_eq!(
        tree.open_in_tree(),
        baseline + 2,
        "at most `max_workers` static bodies hold a descriptor"
    );
    assert_eq!(
        streams_with_frames(&mut world),
        vec![1, 2],
        "the queued streams own no descriptor and answer nothing"
    );

    // Ending one stream frees its slot, and the next queued stream takes it in
    // the wave behind it — not a new descriptor for the one still waiting.
    world.http.grant(1, INITIAL_WINDOW).await;
    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 200);
    assert!(ended(&first), "the last chunk ends the response: {first:?}");
    assert_eq!(zeros(&first, size).len() as u64, size);
    until_pumped(&mut world, |world| in_flight(world) == vec![2, 3]).await;
    assert_eq!(tree.open_in_tree(), baseline + 2);
    assert_eq!(
        streams_with_frames(&mut world),
        vec![2, 3],
        "the fourth stream is still waiting for a slot"
    );

    // Draining the parked streams lets the fourth — which needs no grant at all
    // — answer, and every descriptor goes back.
    world.http.grant(2, INITIAL_WINDOW).await;
    let second = world.http.response(2).await;
    assert!(ended(&second), "{second:?}");
    zeros(&second, size);
    world.http.grant(3, INITIAL_WINDOW).await;
    let third = world.http.response(3).await;
    assert!(ended(&third), "{third:?}");
    zeros(&third, size);
    let fourth = world.http.response(4).await;
    assert_eq!(status_of(&fourth), 200);
    assert_eq!(body_of(&fourth), vec![0u8; 24]);

    world.until(|world| world.requests().len() == 4).await;
    assert!(
        world.log.lock().unwrap().spawns.is_empty(),
        "an all-static route never forks a view"
    );
    for event in world.requests() {
        assert_eq!(event.outcome(), Outcome::Ok);
        assert_eq!(event.status(), 200);
    }
}

#[tokio::test]
async fn a_short_read_aborts_the_response_and_frees_the_slot_exactly_once() {
    // Design §4.6: a read error or a short read is `RespAbort`, never a body
    // shorter than the promised `Content-Length`. The slot is handed back by
    // that result and by nothing else — a second release would let one more
    // stream in than `max_workers` allows, and the fourth request below would
    // answer while two are still parked.
    tokio::time::pause();
    let window = u64::from(INITIAL_WINDOW);
    let size = window + 64 * 1024;
    let tree = PumpTree::build(&[
        ("a.bin", size),
        ("b.bin", size),
        ("c.bin", size),
        ("d.bin", size),
    ]);
    let mut world = pump_world(&tree, 2);

    for (stream, name) in [(1, "/a.bin"), (2, "/b.bin"), (3, "/c.bin"), (4, "/d.bin")] {
        world
            .http
            .send_request(stream, head("GET", name), b"")
            .await;
    }
    until_pumped(&mut world, |world| {
        in_flight(world).len() == 2 && body_frames(world) == 2 * window_chunks()
    })
    .await;
    assert_eq!(in_flight(&mut world), vec![1, 2]);

    // The file shrinks under the read the scheduler already opened: the next
    // `pread` returns zero bytes, which is the short read.
    tree.truncate("a.bin");
    world.http.grant(1, INITIAL_WINDOW).await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200, "the head was promised: {frames:?}");
    assert!(aborted(&frames), "the response never ends: {frames:?}");
    match frames.last() {
        Some(Frame::RespAbort { reason }) => assert_eq!(reason, "static: read failed"),
        other => panic!("expected a read-failure abort, got {other:?}"),
    }
    // Not a truncated body under the promised length.
    assert!(body_of(&frames).len() < size as usize);

    // Exactly one slot came back: the third request takes it, the fourth waits.
    until_pumped(&mut world, |world| in_flight(world) == vec![2, 3]).await;
    assert_eq!(
        streams_with_frames(&mut world),
        vec![2, 3],
        "the fourth request still has no slot — the abort freed exactly one"
    );
    assert_eq!(
        tree.open_in_tree(),
        3,
        "the root's dirfd and exactly two bodies"
    );

    world.until(|world| !world.requests().is_empty()).await;
    assert_eq!(world.requests()[0].status(), 200);
    assert_eq!(
        world.requests()[0].outcome(),
        Outcome::ViewDied,
        "the response died after its head, the closed outcome for that"
    );

    // The reply of a dead stream still frees what it held: the rest of the wave
    // completes.
    for stream in [2, 3, 4] {
        world.http.grant(stream, INITIAL_WINDOW).await;
        let frames = world.http.response(stream).await;
        assert_eq!(status_of(&frames), 200);
        assert!(ended(&frames), "{frames:?}");
        zeros(&frames, size);
    }
}

#[tokio::test]
async fn a_client_that_stops_granting_credit_is_aborted_at_request_timeout() {
    // Design §4.6: "a parked response holds its slot"; the request timeout ends
    // the response, the slot comes back because no read is outstanding, and
    // another request can be served afterwards.
    tokio::time::pause();
    let window = u64::from(INITIAL_WINDOW);
    let tree = PumpTree::build(&[("a.bin", window + 64 * 1024), ("small.js", 24)]);
    let mut options = Options::new().manifest(PUMP_MANIFEST);
    options.cfg.manifest_dir = tree.base.clone();
    options.cfg.max_workers = 1;
    options.cfg.queue_timeout = Duration::from_secs(5);
    options.cfg.request_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    // The client never grants a second window, so the pump parks on credit.
    world.http.send_request(1, head("GET", "/a.bin"), b"").await;
    until_pumped(&mut world, |world| {
        in_flight(world) == vec![1] && body_frames(world) == window_chunks()
    })
    .await;
    let pushed: usize = world
        .http
        .leftover()
        .into_iter()
        .filter_map(|(id, frame)| match frame {
            Frame::RespBody(body) if id == 1 => Some(body.len()),
            _ => None,
        })
        .sum();
    assert_eq!(
        pushed, window as usize,
        "a full initial window and not one byte more"
    );

    // `awaited_response` parks, so the paused clock reaches the deadline.
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 200, "the head was already out");
    assert!(aborted(&frames), "{frames:?}");
    match frames.last() {
        Some(Frame::RespAbort { reason }) => assert_eq!(reason, "request timeout"),
        other => panic!("expected the request timeout abort, got {other:?}"),
    }
    world.until(|world| !world.requests().is_empty()).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::Timeout);
    assert_eq!(
        world.requests()[0].bytes_out(),
        window,
        "one window, and not one byte more"
    );

    // The slot came back: the next request is answered, not queued to a 503.
    world
        .http
        .send_request(2, head("GET", "/small.js"), b"")
        .await;
    let after = world.http.awaited_response(2).await;
    assert_eq!(status_of(&after), 200, "{after:?}");
    assert_eq!(body_of(&after), vec![0u8; 24]);
}

#[tokio::test]
async fn a_forward_hands_the_slot_back_and_re_enters_the_view_path() {
    // Design §4.6, observably at `max_workers = 1`: a `Serve`, then two
    // `Forward`s. A static slot that was not handed back, or one counted twice,
    // leaves the second forward queued until `queue_timeout` and answers `503`.
    tokio::time::pause();
    let tree = StaticTree::build(Fixture::F);
    let mut options = Options::new().manifest(FRONT_MANIFEST).script(
        0,
        Script::Answer {
            body: b"hello".to_vec(),
        },
    );
    options.cfg.manifest_dir = tree.base.clone();
    options.cfg.max_workers = 1;
    options.cfg.queue_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    // The file answers without a view, so nothing is forked yet.
    world
        .http
        .send_request(1, head("GET", "/index.html"), b"")
        .await;
    let served = world.http.awaited_response(1).await;
    assert_eq!(status_of(&served), 200);
    assert_eq!(body_of(&served), INDEX_HTML);
    assert!(world.log.lock().unwrap().spawns.is_empty());

    // A miss forwards: the slot goes back, the front controller is spawned, and
    // the request reaches it with the route's own ingress.
    world
        .http
        .send_request(2, head("GET", "/missing"), b"")
        .await;
    let forwarded = world.http.awaited_response(2).await;
    assert_eq!(status_of(&forwarded), 200, "{forwarded:?}");
    assert_eq!(body_of(&forwarded), b"hello");
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 1)
        .await;

    // A second forward with the view already `READY`: the slot was given back
    // for the first one, so this one is dispatched too, not left in the queue.
    world
        .http
        .send_request(3, head("GET", "/missing-again"), b"")
        .await;
    let second = world.http.awaited_response(3).await;
    assert_eq!(status_of(&second), 200, "{second:?}");
    assert_eq!(body_of(&second), b"hello");

    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0],
        "one view for the route, forked by the first handback"
    );
    // The path the view saw is the routed one: a forward keeps the app's own
    // `PATH_INFO` and never carries the static resolution with it.
    let events = world.requests();
    assert_eq!(events.len(), 3);
    assert_eq!(world.request_routes(), vec![Some("site".to_string()); 3]);
    for event in events {
        assert_eq!(event.outcome(), Outcome::Ok);
    }
}

#[tokio::test]
async fn a_serve_outranks_the_codec_error_a_forward_stores() {
    // Design §4.4: `plan_body` is still computed for a route with a policy, but
    // its error is *stored* — `Serve` answers the page's method/body rule
    // instead, and `Forward` is the only path that answers the stored status.
    // A build that answered the codec error at the head would give `400` for
    // the third row's twin (`POST` on a servable path), where §4.4 says `405`.
    tokio::time::pause();
    let tree = StaticTree::build(Fixture::F);
    let mut options = Options::new().manifest(JSON_FRONT_MANIFEST);
    options.cfg.manifest_dir = tree.base.clone();
    options.cfg.max_workers = 1;
    options.cfg.queue_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    let rows: [(ReqHead, u16, &[u8]); 4] = [
        // `Serve`: the method gate first, whatever the codec would say.
        (
            head_with(
                "POST",
                "/assets/app.4f1c2a9b7d3e.js",
                &[("Content-Type", "text/plain")],
            ),
            405,
            b"method not allowed\n",
        ),
        // `Forward`: the stored codec `400`, exactly as the app path answers it.
        (
            head_with("POST", "/missing", &[("Content-Type", "text/plain")]),
            400,
            b"bad request\n",
        ),
        // `Forward`: a bodyless-safe method that declares a body is the stored
        // `413`, answered before any view exists.
        (
            head_with("GET", "/missing", &[("Content-Length", "1")]),
            413,
            b"content too large\n",
        ),
        // `Serve`: the page's `413`, and the codec error is never reached.
        (
            head_with(
                "GET",
                "/assets/app.4f1c2a9b7d3e.js",
                &[("Content-Length", "1")],
            ),
            413,
            b"content too large\n",
        ),
    ];
    for (index, (head, status, body)) in rows.into_iter().enumerate() {
        let stream = index as u32 + 1;
        world.http.send_request(stream, head, b"").await;
        let frames = world.http.response(stream).await;
        assert_eq!(status_of(&frames), status, "{frames:?}");
        assert_eq!(body_of(&frames), body, "the {status} body");
        if status == 405 {
            assert_eq!(
                header_value(&response_head(&frames).headers, b"Allow"),
                Some(STATIC_ALLOW)
            );
        }
    }

    // None of these reached the app: the stored error is answered here, so no
    // view was forked and no `REQ_HEAD` was forwarded.
    world.until(|world| world.requests().len() == 4).await;
    assert!(
        world.log.lock().unwrap().spawns.is_empty(),
        "the stored body-plan error is the scheduler's answer, not the app's"
    );
    assert!(world.log.lock().unwrap().received.is_empty());
    for event in world.requests() {
        assert_eq!(event.outcome(), Outcome::Unmatched);
    }
}

#[tokio::test]
async fn an_if_none_match_from_the_head_answers_304_without_a_body() {
    // Design §4.5: the responder's `304` needs the request's `If-None-Match`,
    // and `Request` keeps no header list — so `begin_raw` takes that one value
    // off the borrowed head. A build that dropped it would answer a second
    // `200` here, with the same bytes, which is the whole point of the header.
    tokio::time::pause();
    let tree = StaticTree::build(Fixture::S);
    let mut options = Options::new().manifest(SITE_MANIFEST);
    options.cfg.manifest_dir = tree.base.clone();
    let mut world = World::start(options);

    let asset = "/assets/app.4f1c2a9b7d3e.js";
    world.http.send_request(1, head("GET", asset), b"").await;
    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 200);
    let etag = header_value(&response_head(&first).headers, b"ETag")
        .expect("a 200 carries an ETag")
        .to_vec();
    let cache = header_value(&response_head(&first).headers, b"Cache-Control")
        .expect("a 200 carries a Cache-Control")
        .to_vec();
    let etag = std::str::from_utf8(&etag)
        .expect("the ETag is ASCII")
        .to_string();

    world
        .http
        .send_request(2, head_with("GET", asset, &[("If-None-Match", &etag)]), b"")
        .await;
    let second = world.http.response(2).await;

    assert_eq!(status_of(&second), 304);
    assert!(ended(&second), "{second:?}");
    assert_eq!(
        second.len(),
        2,
        "a head and an end, no body frame: {second:?}"
    );
    assert_eq!(body_of(&second), b"");
    let wire = response_head(&second);
    assert_eq!(
        header_value(&wire.headers, b"Content-Length"),
        None,
        "a 304 carries no Content-Length (design §4.5)"
    );
    assert_eq!(
        header_value(&wire.headers, b"Cache-Control"),
        Some(cache.as_slice()),
        "a 304 repeats the cache header the 200 would have carried"
    );
    assert_eq!(header_value(&wire.headers, b"ETag"), Some(etag.as_bytes()));
}

#[tokio::test]
async fn a_zero_length_file_is_an_ordinary_200_with_no_body_frame() {
    // Design §4.6 step 1: a file with nothing left to read ends the response
    // *before* any read is spawned — an ordinary `200` with `Content-Length: 0`
    // and not one body frame.
    tokio::time::pause();
    let tree = PumpTree::build(&[("empty.js", 0)]);
    let mut world = pump_world(&tree, 1);

    world
        .http
        .send_request(1, head("GET", "/empty.js"), b"")
        .await;
    let frames = world.http.response(1).await;

    assert_eq!(status_of(&frames), 200);
    assert_eq!(frames.len(), 2, "a head and an end: {frames:?}");
    assert_eq!(body_of(&frames), b"");
    let wire = response_head(&frames);
    assert_eq!(
        header_value(&wire.headers, b"Content-Length"),
        Some(&b"0"[..])
    );
    assert_eq!(
        header_value(&wire.headers, b"Cache-Control"),
        Some(&b"no-cache"[..]),
        "an exact answer that matches no pattern"
    );

    world.until(|world| !world.requests().is_empty()).await;
    let events = world.requests();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].outcome(), Outcome::Ok);
    assert_eq!(events[0].bytes_out(), 0);
}

#[tokio::test]
async fn a_buffered_forward_that_ends_while_queued_is_still_answered() {
    // Design §4.4/§4.6, and the regression this row exists for: a `json` front
    // controller's buffered `Forward` whose `REQ_END` lands while the request is
    // still waiting for a slot has already been transcoded, so it must not be
    // re-admitted (which would list it twice in the FIFO) and must not be armed
    // to wait for a body that has arrived (nothing re-drives that, and the queue
    // timeout answers a request that was ready). With one slot and one parked
    // static stream, this request can only be queued when its body ends.
    tokio::time::pause();
    let window = u64::from(INITIAL_WINDOW);
    let tree = PumpTree::build(&[("big.bin", window + 64 * 1024)]);
    let mut options = Options::new().manifest(JSON_FRONT_MANIFEST).script(
        0,
        Script::Answer {
            body: b"hello".to_vec(),
        },
    );
    options.cfg.manifest_dir = tree.base.clone();
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 4;
    options.cfg.queue_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    // The parked stream holds the only slot.
    world
        .http
        .send_request(1, head("GET", "/big.bin"), b"")
        .await;
    until_pumped(&mut world, |world| {
        in_flight(world) == vec![1] && body_frames(world) == window_chunks()
    })
    .await;

    // A `json` body and its end, both while the request waits behind it.
    let body = br#"{"a":1}"#;
    let length = body.len().to_string();
    world
        .http
        .send_head(
            2,
            head_with(
                "POST",
                "/missing",
                &[
                    ("Content-Type", "application/json"),
                    ("Content-Length", &length),
                ],
            ),
        )
        .await;
    world.http.send_body(2, body).await;
    world.http.send(2, &Frame::ReqEnd).await;
    // Nothing may answer it while the slot is held: a 503 here would be the
    // duplicate-enqueue/never-re-driven pair.
    world.advance(Duration::from_millis(1)).await;
    assert!(
        !streams_with_frames(&mut world).contains(&2),
        "the queued forward has not been answered: {:?}",
        world.http.leftover()
    );

    // Freeing the slot lets the transcoded body reach the app.
    world.http.grant(1, INITIAL_WINDOW).await;
    let parked = world.http.response(1).await;
    assert!(ended(&parked), "{parked:?}");
    let forwarded = world.http.awaited_response(2).await;
    assert_eq!(status_of(&forwarded), 200, "{forwarded:?}");
    assert_eq!(body_of(&forwarded), b"hello");

    // Exactly one head reached the view, and the body it saw is the canonical
    // JSON of the one that was sent.
    world
        .until(|world| !world.log.lock().unwrap().request_ids(0).is_empty())
        .await;
    let received = world.log.lock().unwrap().received_by(0);
    assert_eq!(
        received
            .iter()
            .filter(|(_, frame)| matches!(frame, Frame::ReqHead(_)))
            .count(),
        1,
        "one forwarded head: {received:?}"
    );
    let forwarded_body: Vec<u8> = received
        .iter()
        .filter_map(|(_, frame)| match frame {
            Frame::ReqBody(chunk) => Some(chunk.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&forwarded_body).expect("the view got JSON"),
        serde_json::json!({"a": 1}),
        "the transcoded body the view received"
    );
}

#[tokio::test]
async fn a_full_static_queue_answers_503_with_retry_after() {
    // Design §4.6: the same bounded queue, `queue_timeout` and `503` +
    // `Retry-After` apply to a static answer. With one slot and `queue_size = 1`
    // the second waiter is the last one admitted and the third is turned away.
    tokio::time::pause();
    let window = u64::from(INITIAL_WINDOW);
    let tree = PumpTree::build(&[
        ("a.bin", window + 64 * 1024),
        ("b.bin", window + 64 * 1024),
        ("c.bin", window + 64 * 1024),
    ]);
    let mut options = Options::new().manifest(PUMP_MANIFEST);
    options.cfg.manifest_dir = tree.base.clone();
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 1;
    options.cfg.queue_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    // The parked stream holds the only slot, so the next request is the one
    // waiter `queue_size = 1` admits and the one after it is turned away.
    world.http.send_request(1, head("GET", "/a.bin"), b"").await;
    until_pumped(&mut world, |world| {
        in_flight(world) == vec![1] && body_frames(world) == window_chunks()
    })
    .await;
    world.http.send_request(2, head("GET", "/b.bin"), b"").await;
    world.http.send_request(3, head("GET", "/c.bin"), b"").await;

    let refused = world.http.response(3).await;
    assert_eq!(status_of(&refused), 503, "{refused:?}");
    assert_eq!(body_of(&refused), b"queue full\n");
    let headers = &response_head(&refused).headers;
    assert_eq!(
        header_value(headers, b"Retry-After"),
        Some(&b"5"[..]),
        "the queue timeout, in seconds"
    );
    assert!(
        !streams_with_frames(&mut world).contains(&2),
        "the admitted waiter is still waiting, not answered: {:?}",
        world.http.leftover()
    );

    world.until(|world| !world.requests().is_empty()).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::QueueFull);
    assert!(
        world.log.lock().unwrap().spawns.is_empty(),
        "a turned-away static request forks nothing"
    );
}

// ---------------------------------------------------------------------------
// §4.6, the cancel and the queue: the client that goes away, the forwarded
// body that ends while it waits, and the three bounds those two share.
// ---------------------------------------------------------------------------

/// A `POST` with a small `json` body on the static front controller, head to
/// `REQ_END`: the buffered `Forward` the queue rows below are about.
async fn buffered_forward(world: &mut World, stream: u32, body: &[u8]) {
    let length = body.len().to_string();
    world
        .http
        .send_head(
            stream,
            head_with(
                "POST",
                "/missing",
                &[
                    ("Content-Type", "application/json"),
                    ("Content-Length", length.as_str()),
                ],
            ),
        )
        .await;
    world.http.send_body(stream, body).await;
    world.http.send(stream, &Frame::ReqEnd).await;
}

/// The same `json` ingress with no `[route.static]`: the app path's own
/// half-body bound, which the static path has to meet (design §4.6).
const JSON_APP_MANIFEST: &str = "version = 1\napp = \"app:app\"\n\n\
     [[route]]\nname = \"app\"\npath = ['.*']\n\n\
     [route.ingress]\nbody = \"json\"\n";

/// The response of one buffered `json` `POST` and its one `request` event,
/// through `manifest` with `queue_size` half-body slots.
async fn buffered_forward_answer(
    manifest: &str,
    manifest_dir: PathBuf,
    queue_size: usize,
) -> (Vec<Frame>, Outcome) {
    let mut options = Options::new().manifest(manifest).script(
        0,
        Script::Answer {
            body: b"hello".to_vec(),
        },
    );
    options.cfg.manifest_dir = manifest_dir;
    options.cfg.max_workers = 4;
    options.cfg.queue_size = queue_size;
    options.cfg.queue_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    buffered_forward(&mut world, 1, br#"{"a":1}"#).await;
    let frames = world.http.response(1).await;
    world.until(|world| !world.requests().is_empty()).await;
    let outcome = world.requests()[0].outcome();
    (frames, outcome)
}

/// A runtime with **one** thread in its blocking pool, and a way for a test to
/// occupy it.
///
/// A static body's `pread` cannot be made slow and cannot be cancelled (design
/// §4.6), so the only deterministic way to observe one still in flight is to
/// stop it from ever starting: with one blocking thread, a task the test
/// submits first owns the pool, and the scheduler's `spawn_blocking` read waits
/// behind it until the test lets go. That is the state a read parked on a
/// stalled mount is in, without a mount.
fn one_blocking_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .expect("a current-thread runtime with one blocking thread")
}

/// The held blocking thread: the reads queued behind it run when this is
/// released — or dropped, so a failing test can never leave it parked.
struct BlockingPool {
    release: Option<std::sync::mpsc::Sender<()>>,
}

impl BlockingPool {
    /// Occupies the only blocking thread, and returns once it is really busy.
    async fn occupy() -> Self {
        let (release, held) = std::sync::mpsc::channel();
        let (started, running) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            let _ = started.send(());
            let _ = held.recv();
        });
        running.await.expect("the blocking thread started");
        Self {
            release: Some(release),
        }
    }

    /// Lets the queued reads run.
    fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

impl Drop for BlockingPool {
    fn drop(&mut self) {
        self.release();
    }
}

#[tokio::test]
async fn a_client_that_goes_away_mid_body_releases_its_slot_once() {
    // Design §4.6/§8.1: a static response the client abandons is `client_gone`,
    // and the slot it still holds comes back exactly once. With one slot, a
    // release that never happened leaves the next request queued until the queue
    // timeout, and a release too many would let two bodies stream at once — the
    // two requests after the cancel prove both directions.
    tokio::time::pause();
    let window = u64::from(INITIAL_WINDOW);
    let size = window + 64 * 1024;
    let tree = PumpTree::build(&[("a.bin", size), ("b.bin", size), ("c.bin", size)]);
    let mut world = pump_world(&tree, 1);

    world.http.send_request(1, head("GET", "/a.bin"), b"").await;
    until_pumped(&mut world, |world| {
        in_flight(world) == vec![1] && body_frames(world) == window_chunks()
    })
    .await;
    assert_eq!(
        tree.open_in_tree(),
        2,
        "the root's dirfd and the parked body"
    );

    // The client goes away while the response is parked on credit: no read is
    // outstanding, so the slot is the client's to give back.
    world.http.send(1, &Frame::Cancel).await;
    world.until(|world| !world.requests().is_empty()).await;
    let events = world.requests();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    assert_eq!(events[0].outcome(), Outcome::ClientGone);
    assert_eq!(events[0].status(), 200, "the head was already out");
    assert_eq!(events[0].bytes_out(), window, "the one window it saw");
    assert_eq!(
        tree.open_in_tree(),
        1,
        "the slot and the descriptor went back with the cancel"
    );

    // The dead client is told nothing more: the frames it already has are all.
    let pushed = world.http.take_all();
    let heads = pushed
        .iter()
        .filter(|(_, frame)| matches!(frame, Frame::RespHead(_)))
        .count();
    let bodies = pushed
        .iter()
        .filter(|(_, frame)| matches!(frame, Frame::RespBody(_)))
        .count();
    assert_eq!(
        (heads, bodies, pushed.len()),
        (1, window_chunks(), 1 + window_chunks()),
        "one head and one window of body, and no second status or abort"
    );

    // Exactly one slot came back: the two requests behind it run in waves of one.
    world.http.send_request(2, head("GET", "/b.bin"), b"").await;
    world.http.send_request(3, head("GET", "/c.bin"), b"").await;
    until_pumped(&mut world, |world| {
        in_flight(world) == vec![2] && body_frames(world) == window_chunks()
    })
    .await;
    assert_eq!(
        streams_with_frames(&mut world),
        vec![2],
        "the third request has no slot: the cancel released one, not two"
    );
    assert_eq!(tree.open_in_tree(), 2, "one body at a time");

    world.http.grant(2, INITIAL_WINDOW).await;
    let second = world.http.response(2).await;
    assert!(ended(&second), "{second:?}");
    zeros(&second, size);
    until_pumped(&mut world, |world| {
        in_flight(world) == vec![3] && body_frames(world) == window_chunks()
    })
    .await;
    world.http.grant(3, INITIAL_WINDOW).await;
    let third = world.http.response(3).await;
    assert!(ended(&third), "{third:?}");
    zeros(&third, size);
    assert_eq!(tree.open_in_tree(), 1, "every descriptor went back");

    world.until(|world| world.requests().len() == 3).await;
    let outcomes: Vec<Outcome> = world
        .requests()
        .iter()
        .map(|event| event.outcome())
        .collect();
    assert_eq!(
        outcomes,
        vec![Outcome::ClientGone, Outcome::Ok, Outcome::Ok],
        "the abandoned response is the only one the client caused"
    );
}

#[test]
fn a_cancel_while_a_read_is_parked_keeps_the_slot_until_the_read_returns() {
    // Design §4.6: "the result releases both, and never the timeout". A `pread`
    // cannot be cancelled, so a cancelled request whose read is still
    // outstanding must keep its slot *and* its descriptor until that result
    // arrives. The blocking pool is held by the test, so the read is in flight
    // and unable to finish: releasing the slot here (instead of deferring to
    // the result) is visible as a second request answered while a read is
    // parked, and a descriptor charged for nobody.
    one_blocking_thread().block_on(async {
        tokio::time::pause();
        let window = u64::from(INITIAL_WINDOW);
        let tree = PumpTree::build(&[("a.bin", window + 64 * 1024), ("small.js", 24)]);
        let mut world = pump_world(&tree, 1);

        world.http.send_request(1, head("GET", "/a.bin"), b"").await;
        until_pumped(&mut world, |world| {
            in_flight(world) == vec![1] && body_frames(world) == window_chunks()
        })
        .await;

        // The grant wakes the pump, which spawns the next read; the pool is
        // held, so that read can never return.
        let mut pool = BlockingPool::occupy().await;
        world.http.grant(1, INITIAL_WINDOW).await;
        world.http.send(1, &Frame::Cancel).await;
        world.until(|world| !world.requests().is_empty()).await;

        let events = world.requests();
        assert_eq!(events.len(), 1, "one event: {events:?}");
        assert_eq!(events[0].outcome(), Outcome::ClientGone);
        assert_eq!(events[0].status(), 200, "the head was already out");
        assert_eq!(events[0].bytes_out(), window, "the one window it saw");

        // A request behind it waits: the slot is still charged, and so is the
        // descriptor the parked read holds.
        world
            .http
            .send_request(2, head("GET", "/small.js"), b"")
            .await;
        world.advance(Duration::from_millis(1)).await;
        assert!(
            !streams_with_frames(&mut world).contains(&2),
            "the slot is still the parked read's: {:?}",
            streams_with_frames(&mut world)
        );
        assert_eq!(
            tree.open_in_tree(),
            2,
            "the root's dirfd and the parked read's body"
        );

        // The result releases both, exactly once, and the waiter is served.
        pool.release();
        let second = world.http.response(2).await;
        assert_eq!(status_of(&second), 200, "{second:?}");
        assert_eq!(body_of(&second), vec![0u8; 24]);
        assert_eq!(
            tree.open_in_tree(),
            1,
            "the read released the slot and the descriptor"
        );
        world.until(|world| world.requests().len() == 2).await;
        assert_eq!(world.requests()[1].outcome(), Outcome::Ok);
    });
}

#[tokio::test]
async fn a_buffered_forward_that_ends_while_queued_keeps_one_queue_entry() {
    // Design §4.6: a `REQ_END` that lands while the request is still waiting for
    // a slot must not re-admit it. `enqueue` lists the id a second time (the
    // FIFO does not dedupe) and refreshes `queued_since`, so with `queue_size =
    // 2` one queued forward would fill the queue and the request behind it would
    // be turned away — although only one request is waiting.
    tokio::time::pause();
    let window = u64::from(INITIAL_WINDOW);
    let tree = PumpTree::build(&[("big.bin", window + 64 * 1024), ("small.js", 24)]);
    let mut options = Options::new().manifest(JSON_FRONT_MANIFEST).script(
        0,
        Script::Answer {
            body: b"hello".to_vec(),
        },
    );
    options.cfg.manifest_dir = tree.base.clone();
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 2;
    options.cfg.queue_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    // The parked stream holds the only slot.
    world
        .http
        .send_request(1, head("GET", "/big.bin"), b"")
        .await;
    until_pumped(&mut world, |world| {
        in_flight(world) == vec![1] && body_frames(world) == window_chunks()
    })
    .await;

    // One buffered forward, queued, whose end lands while it waits.
    buffered_forward(&mut world, 2, br#"{"a":1}"#).await;
    world.advance(Duration::from_millis(1)).await;
    assert!(
        !streams_with_frames(&mut world).contains(&2),
        "the queued forward is not answered while it waits: {:?}",
        streams_with_frames(&mut world)
    );

    // A third request: one queue entry leaves room for it, a duplicate does not.
    world
        .http
        .send_request(3, head("GET", "/small.js"), b"")
        .await;

    // Freeing the slot answers the one queued forward, then the request behind
    // it — in that order, and both `200`.
    world.http.grant(1, INITIAL_WINDOW).await;
    let parked = world.http.response(1).await;
    assert!(ended(&parked), "{parked:?}");
    let forwarded = world.http.awaited_response(2).await;
    assert_eq!(
        status_of(&forwarded),
        200,
        "the queued forward, not re-queued: {forwarded:?}"
    );
    assert_eq!(body_of(&forwarded), b"hello");
    let waiting = world.http.awaited_response(3).await;
    assert_eq!(
        status_of(&waiting),
        200,
        "the request behind it was admitted: {waiting:?}"
    );
    assert_eq!(body_of(&waiting), vec![0u8; 24]);
}

#[tokio::test]
async fn a_buffered_forward_that_ends_while_queued_still_expires_on_queue_timeout() {
    // The other half of the same transition (design §4.6): keeping one queue
    // entry keeps the request's `queued_since`. A request that cleared it while
    // queued has no deadline `expire_queue` can find, so it is never answered —
    // not even when the slot is still held and the queue timeout has long
    // passed. The slot holder below is untouched by that timeout.
    tokio::time::pause();
    let window = u64::from(INITIAL_WINDOW);
    let tree = PumpTree::build(&[("big.bin", window + 64 * 1024)]);
    let mut options = Options::new().manifest(JSON_FRONT_MANIFEST).script(
        0,
        Script::Answer {
            body: b"hello".to_vec(),
        },
    );
    options.cfg.manifest_dir = tree.base.clone();
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 2;
    options.cfg.queue_timeout = Duration::from_secs(5);
    options.cfg.request_timeout = Duration::from_secs(30);
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head("GET", "/big.bin"), b"")
        .await;
    until_pumped(&mut world, |world| {
        in_flight(world) == vec![1] && body_frames(world) == window_chunks()
    })
    .await;
    buffered_forward(&mut world, 2, br#"{"a":1}"#).await;

    // The slot is never given back, so the queue's own deadline is the only one
    // that can answer the waiter.
    world.advance(Duration::from_secs(5)).await;
    let refused = world.http.response(2).await;
    assert_eq!(status_of(&refused), 503, "{refused:?}");
    assert_eq!(body_of(&refused), b"queue timeout\n");
    assert_eq!(
        header_value(&response_head(&refused).headers, b"Retry-After"),
        Some(&b"5"[..]),
        "the queue timeout, in seconds"
    );
    assert_eq!(
        in_flight(&mut world),
        vec![1],
        "the slot holder is still streaming: only the waiter timed out"
    );
    world.until(|world| !world.requests().is_empty()).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::QueueTimeout);
    assert_eq!(world.requests()[0].status(), 503);

    // And the slot holder still completes: the timeout freed nobody else's slot.
    world.http.grant(1, INITIAL_WINDOW).await;
    let parked = world.http.response(1).await;
    assert!(ended(&parked), "{parked:?}");
    assert_eq!(status_of(&parked), 200);
    assert_eq!(tree.open_in_tree(), 1, "one slot, one descriptor");
}

#[tokio::test]
async fn a_buffered_forward_counts_itself_against_the_app_paths_bound() {
    // Design §4.6: "the count of half-bodies a route may hold is the app path's
    // own bound". `begin_raw` inserts the request with `body_ready = false`
    // *before* it asks `buffering_count()`, so the request counts itself and
    // `queue_size = 1` admits none. A static front controller that asked first
    // and cleared `body_ready` after would admit one more — and with
    // `queue_size = 2` it would admit two where an app route admits one.
    tokio::time::pause();
    let tree = StaticTree::build(Fixture::F);
    for (queue_size, status) in [(1usize, 503u16), (2, 200)] {
        let (static_frames, static_outcome) =
            buffered_forward_answer(JSON_FRONT_MANIFEST, tree.base.clone(), queue_size).await;
        let (app_frames, app_outcome) =
            buffered_forward_answer(JSON_APP_MANIFEST, tree.base.clone(), queue_size).await;

        assert_eq!(
            status_of(&static_frames),
            status,
            "the static path at queue_size = {queue_size}: {static_frames:?}"
        );
        assert_eq!(
            status_of(&app_frames),
            status,
            "the app path at queue_size = {queue_size}: {app_frames:?}"
        );
        assert_eq!(
            (status_of(&static_frames), static_outcome),
            (status_of(&app_frames), app_outcome),
            "the same bound, and the same outcome, on both paths"
        );
        if status == 503 {
            assert_eq!(body_of(&static_frames), b"queue full\n");
            assert_eq!(
                header_value(&response_head(&static_frames).headers, b"Retry-After"),
                Some(&b"5"[..])
            );
        } else {
            assert_eq!(
                body_of(&static_frames),
                b"hello",
                "the view answered the admitted body"
            );
        }
    }
}

#[tokio::test]
async fn the_pump_reads_the_documented_chunk() {
    // `R10`/§4.6 step 2: one read is at most `STATIC_CHUNK_BYTES`, and the body
    // frames are that read — the only place the bound is observable. A chunk of
    // zero would ask for a zero-byte read, which can never advance the offset:
    // the pump would push empty frames until `request_timeout` answered a stream
    // that had already failed. Pinning the frame size makes such a chunk a fast
    // failure rather than a hang.
    tokio::time::pause();
    let chunk = STATIC_CHUNK_BYTES as u64;
    let size = 2 * chunk + 7;
    let tree = PumpTree::build(&[("two.bin", size)]);
    let mut world = pump_world(&tree, 1);

    world
        .http
        .send_request(1, head("GET", "/two.bin"), b"")
        .await;
    let frames = world.http.response(1).await;
    let chunks: Vec<usize> = frames
        .iter()
        .filter_map(|frame| match frame {
            Frame::RespBody(body) => Some(body.len()),
            _ => None,
        })
        .collect();
    assert!(
        ended(&frames),
        "the response ends after {chunks:?} body frames"
    );
    assert_eq!(
        chunks,
        vec![STATIC_CHUNK_BYTES, STATIC_CHUNK_BYTES, 7],
        "two reads of one chunk each, then the rest"
    );
    assert_eq!(
        STATIC_CHUNK_BYTES,
        64 * 1024,
        "the chunk the frames above just proved is `R10`'s 64 KiB"
    );
    assert_eq!(body_of(&frames).len() as u64, size);
    world.until(|world| !world.requests().is_empty()).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::Ok);
    assert_eq!(world.requests()[0].bytes_out(), size);
}

#[test]
fn a_zero_byte_read_aborts_at_once_and_never_waits_for_the_timeout() {
    // Design §4.6: a read error or a short read aborts the response, and the
    // scheduler never sends a truncated body under a `Content-Length` it
    // promised. A read that returns nothing is that short read — not a silent
    // park until `request_timeout` answers a stream that had already failed.
    // The paused clock is never advanced, so an abort only the request timeout
    // could produce cannot appear here.
    one_blocking_thread().block_on(async {
        tokio::time::pause();
        let window = u64::from(INITIAL_WINDOW);
        let size = window + 64 * 1024;
        let tree = PumpTree::build(&[("a.bin", size), ("small.js", 24)]);
        let mut world = pump_world(&tree, 1);

        // The read is spawned at dispatch and cannot run while the pool is
        // held, so the head is out and no byte of the body is.
        let mut pool = BlockingPool::occupy().await;
        world.http.send_request(1, head("GET", "/a.bin"), b"").await;
        until_pumped(&mut world, |world| in_flight(world) == vec![1]).await;
        tree.truncate("a.bin");
        pool.release();

        let frames = world.http.response(1).await;
        assert_eq!(status_of(&frames), 200, "the head was promised: {frames:?}");
        assert!(
            aborted(&frames),
            "a zero-byte read is an abort, not an end: {frames:?}"
        );
        assert_eq!(
            body_of(&frames),
            b"",
            "not one byte under the length promised"
        );
        match frames.last() {
            Some(Frame::RespAbort { reason }) => assert_eq!(reason, "static: read failed"),
            other => panic!("expected a read-failure abort, got {other:?}"),
        }

        world.until(|world| !world.requests().is_empty()).await;
        assert_eq!(world.requests()[0].outcome(), Outcome::ViewDied);
        assert_eq!(world.requests()[0].bytes_out(), 0);
        assert_eq!(
            tree.open_in_tree(),
            1,
            "the slot and the descriptor went back with the abort"
        );

        // Exactly one release: the next request is served rather than queued.
        world
            .http
            .send_request(2, head("GET", "/small.js"), b"")
            .await;
        let after = world.http.response(2).await;
        assert_eq!(status_of(&after), 200, "{after:?}");
        assert_eq!(body_of(&after), vec![0u8; 24]);
    });
}
