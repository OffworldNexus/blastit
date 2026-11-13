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

#[path = "common/static_fixture.rs"]
mod static_fixture;

use std::io::Read;

use blastit_manifest::{Manifest, RoutingTable, StaticPolicy, UidLayout};
use blastit_scheduler::rewrite::{Rewrite, STATIC_ALLOW, decide, declares_body};
use blastit_scheduler::routes_from_manifest;
use blastit_scheduler::static_asset::{
    AnswerKind, Backend, StaticOutcome, StaticRefusal, StaticRoot, head_segments,
};
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
