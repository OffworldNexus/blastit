//! The runtime parity suite: one named case per row of `MB.21`'s capability
//! map **that the scheduler can assert**, on the `Blastfile` a user writes and
//! the scheduler the product runs.
//!
//! Every case in this file boots a real [`common::World`] through
//! [`common::Options::blastfile`], so the manifest is
//! `Manifest::from_blastfile_str`'s (the one parser), the route table is
//! `RoutingTable`'s, and the mounts are handed over beside it exactly as
//! `stage0` hands them over. Nothing here lowers by hand or compares models: the
//! model-equality oracle is `crates/blastit-manifest/tests/
//! critical_blastfile_parity.rs`, and `mount_precedence.rs` is the narrative
//! half of the static row. What this file adds is the **outcome table**: a
//! request → a status, over the wire.
//!
//! Four capability-map rows are **not** here, and this file does not claim
//! them: §11.1's trait grants and §13.1's `unrestricted`, table/column grants
//! and normalised `query`. The scheduler carries no grant at all (`RouteInfo`
//! holds the name, the prefork flag, the uid and the ingress policy), so a
//! `World` boot cannot assert one without inventing the handoff; those rows are
//! `Manifest::effective_grants`' and live in the manifest crate's parity suite
//! (`row_a_trait_grants_what_the_view_receives`,
//! `row_unrestricted_on_the_default_and_a_named_store`,
//! `row_table_and_column_grants`, `row_query_is_stored_normalised` there).
//!
//! What the tests do pin of the runtime half, so no citation here is a wish:
//! `crates/blastit/src/check.rs::tests::the_route_policies_are_the_effective_grants`
//! asserts the per-route `RoutePolicySpec` list `blastit check` hands dbproxy
//! from a `Blastfile` (the granted view carries its own queries, the ungranted
//! one no policy at all); `stage0::tests::an_explicit_empty_grant_still_names_the_proxy_for_its_route`
//! asserts the per-route socket rule; and dbproxy's
//! `critical_learn_and_redaction.rs` asserts a `RoutePolicySpec` is the
//! allowlist the listener enforces. The production handoff itself
//! (`stage0::bind_view_sockets`) is private and needs stage 0's privileges and
//! has **no test**; `RESULT.md` records that as a follow-up rather than
//! claiming it here.
//!
//! | Capability-map row | Test | New assertion here |
//! |---|---|---|
//! | §9.2 static precedence, four steps | [`row_static_precedence_is_the_outcome_table`] | the whole request → status table in one fixture: nested mount, `Missing` → view, `Hidden`/`Refused` final, `fallback` on `GET`/`HEAD` only and only from the longest mount, `405` after every view |
//! | §10.1 `body json` | [`row_body_json_accepts_json_and_refuses_a_form_body`] | the codec from a `Blastfile`: JSON 200 with its bytes, a form body 400 before the view |
//! | §10.1 `body form` | [`row_body_form_accepts_a_form_and_refuses_json`] | the same two sides for `form` |
//! | §10.1 `body none` | [`row_body_none_refuses_any_body`] | 413 with a body, 200 with none, neither reaching the view's bytes |
//! | §10.1 `body raw` | [`row_body_raw_is_unacknowledged_and_streams`] | `(Raw, false)` from the file, `RawIngress` warning, a non-empty production refusal list, and the bytes streaming through |
//! | §10.1 `body raw unvalidated` | [`row_body_raw_unvalidated_boots_with_the_same_warning`] | `(Raw, true)`, the **same** warning, an empty refusal list, the same bytes |
//! | §12.4 `prefork` | [`row_prefork_reaches_the_route_the_scheduler_spawns`] | a view exists before any request, and only the preforked one |
//! | §12.4 `egress any` | [`row_egress_any_and_its_warning_arrive_together`] | `Route.egress`, the `EgressRoute` warning, and a request still answered |
//! | §12.2 `entrypoint php` | [`row_entrypoint_php_answers_every_view`] | one file-level policy, no per-view one, and a booted `World` in which **each** view answers its own request while a path no view claims is `404` |
//! | §8.4 `HEAD` implied by `GET` | [`row_head_is_implied_by_get_through_the_matcher`] | `RoutingTable::route("HEAD", …)` matches, and a real `HEAD` is dispatched |
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};

use blastit_frame::{Frame, ReqHead};
use blastit_ingress::BodyCodec;
use blastit_manifest::{Manifest, ManifestWarning, MountTable, RoutingTable, UidLayout};
use common::{Options, Script, World, body_of, head, status_of};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// The manifest the product's parser reads: parse, lower and validate.
fn parsed(source: &str) -> Manifest {
    Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE)
        .expect("the Blastfile parses and validates")
        .0
}

/// The warnings the same parse produced, in the boot's order.
fn warnings(source: &str) -> Vec<ManifestWarning> {
    Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE)
        .expect("the Blastfile parses and validates")
        .1
}

/// A request head carrying `Content-Type` and `Transfer-Encoding: chunked`, the
/// shape a body arrives in without a `Content-Length`.
fn chunked(method: &str, target: &str, content_type: &str) -> ReqHead {
    let mut request = head(method, target);
    request
        .headers
        .push((b"Content-Type".to_vec(), content_type.as_bytes().to_vec()));
    request
        .headers
        .push((b"Transfer-Encoding".to_vec(), b"chunked".to_vec()));
    request
}

/// One request with a chunked body, and its finished frames.
async fn round_trip(world: &mut World, id: u32, request: ReqHead, body: &[u8]) -> Vec<Frame> {
    world.http.send_head(id, request).await;
    world.http.send_body(id, body).await;
    world.http.send(id, &Frame::ReqEnd).await;
    world.http.response(id).await
}

/// A response header's value, matched case-insensitively.
fn header_value<'a>(frames: &'a [Frame], name: &[u8]) -> Option<&'a [u8]> {
    match frames.first() {
        Some(Frame::RespHead(head)) => head
            .headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_slice()),
        other => panic!("no response head: {other:?}"),
    }
}

/// True when one warning in `warnings` matches `pick`.
fn has_warning(warnings: &[ManifestWarning], pick: impl Fn(&ManifestWarning) -> bool) -> bool {
    warnings.iter().any(pick)
}

// ---------------------------------------------------------------------------
// §9.2 the static precedence, as an outcome table
// ---------------------------------------------------------------------------

/// The tree the mounts serve, removed on drop.
struct Site {
    base: PathBuf,
}

impl Site {
    /// `public/assets/{app.js,index.html,shell.php,v2/app.js}` and
    /// `public/v2/app.js`.
    ///
    /// `shell.php` exists so the mount's `Hidden` rule has a real file to hide,
    /// and `v2/app.js` exists under **both** mounts with different bytes, so
    /// "the longest prefix answered" is a body comparison.
    fn build() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!(
            "blastit-parity-static-{}-{nanos}",
            std::process::id()
        ));
        for (relative, body) in [
            ("public/assets/app.js", "the-outer-mount"),
            ("public/assets/index.html", "the-declared-fallback"),
            ("public/assets/shell.php", "<?php echo 'never';"),
            ("public/assets/v2/app.js", "the-outer-mount-relative"),
            ("public/v2/app.js", "the-nested-mount"),
        ] {
            let path = base.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
        }
        Self {
            base: std::fs::canonicalize(&base).expect("canonical site base"),
        }
    }

    fn dir(&self) -> &Path {
        &self.base
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Two mounts (one nested, only the outer with a `fallback`) and three views, so
/// every step of §9.2's precedence is reachable in one fixture.
///
/// * `dyn` claims a path below `/assets` that has no file: step 1 misses, step 2
///   answers the view.
/// * `shell` claims the `.php` path that **does** exist: step 1 must be `Hidden`
///   and final, never this view.
/// * `api` declares `GET` only: a `POST` to it is step 4's `405`.
const SITE: &str = concat!(
    "static \"/assets\" {\n",
    "    root \"public/assets\"\n",
    "    fallback \"index.html\"\n",
    "}\n",
    "\n",
    "static \"/assets/v2\" {\n",
    "    root \"public/v2\"\n",
    "}\n",
    "\n",
    "view dyn GET \"/assets/dyn\" {}\n",
    "view shell GET \"/assets/shell.php\" {}\n",
    "view secret GET \"/assets/.env\" {}\n",
    "view api GET \"/api/*\" {}\n",
);

fn site_world(site: &Site) -> World {
    let mut options = Options::new()
        .blastfile(SITE)
        .script(
            0,
            Script::Answer {
                body: b"the-view".to_vec(),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"the-shell".to_vec(),
            },
        )
        .script(
            2,
            Script::Answer {
                body: b"the-dotfile".to_vec(),
            },
        )
        .script(
            3,
            Script::Answer {
                body: b"the-api".to_vec(),
            },
        );
    options.cfg.manifest_dir = site.dir().to_path_buf();
    World::start(options)
}

async fn ask(world: &mut World, id: u32, method: &str, target: &str) -> Vec<Frame> {
    world.http.send_request(id, head(method, target), b"").await;
    world.http.response(id).await
}

/// §9.2: the four steps, one request at a time, as an outcome table.
#[tokio::test]
async fn row_static_precedence_is_the_outcome_table() {
    tokio::time::pause();
    let site = Site::build();
    let mut world = site_world(&site);

    // Step 1, longest prefix: `/assets/v2/app.js` exists under both mounts and
    // the nested one answers.
    let frames = ask(&mut world, 1, "GET", "/assets/v2/app.js").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-nested-mount");

    // Step 1, the outer mount alone.
    let frames = ask(&mut world, 2, "GET", "/assets/app.js").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-outer-mount");

    // Step 2: the mount misses (`/assets/dyn` is no file) and the view answers —
    // the mount's own `fallback` must not shadow it.
    let frames = ask(&mut world, 3, "GET", "/assets/dyn").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-view");

    // Step 2's deny side: the same path with the declared method's sibling is
    // `405` from the view table, not the fallback and not a `404`.
    let frames = ask(&mut world, 4, "POST", "/assets/dyn").await;
    assert_eq!(status_of(&frames), 405, "{frames:?}");
    assert_eq!(
        header_value(&frames, b"Allow"),
        Some(&b"GET, HEAD"[..]),
        "the view's declared methods, with HEAD implied"
    );

    // Step 3: no view claimed `/assets/gone.js`, so the matching mount's
    // declared `fallback` answers `GET` …
    let frames = ask(&mut world, 5, "GET", "/assets/gone.js").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-declared-fallback");
    // … and `HEAD`, without a body, with the length it promised.
    let frames = ask(&mut world, 6, "HEAD", "/assets/gone.js").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert!(body_of(&frames).is_empty(), "HEAD is GET without a body");
    assert_eq!(
        header_value(&frames, b"Content-Length"),
        Some(b"the-declared-fallback".len().to_string().as_bytes())
    );
    // … and nothing else: a `POST` never reaches the fallback, and no view
    // matched, so it is step 4's `404` (not a `405`).
    let frames = ask(&mut world, 7, "POST", "/assets/gone.js").await;
    assert_eq!(status_of(&frames), 404, "{frames:?}");

    // Step 3, the longest prefix owns the fallback: the nested mount declares
    // none, so its miss is a `404` even though the outer mount has one.
    let frames = ask(&mut world, 8, "GET", "/assets/v2/gone.js").await;
    assert_eq!(status_of(&frames), 404, "{frames:?}");
    assert_ne!(body_of(&frames).as_slice(), b"the-declared-fallback");

    // The deny side of step 1: a `.php` file beneath a mount is `Hidden` (`404`)
    // and final — the view that claims the exact same path never runs.
    let frames = ask(&mut world, 9, "GET", "/assets/shell.php").await;
    assert_eq!(status_of(&frames), 404, "{frames:?}");
    assert!(
        !world
            .log
            .lock()
            .unwrap()
            .saw(1, |frame| matches!(frame, Frame::ReqHead(_))),
        "a hidden PHP source fell through to the view"
    );

    // The deny side of step 0: a dotfile beneath a mount is `Hidden` (`404`)
    // and final — the view that claims that exact path never runs.
    let frames = ask(&mut world, 10, "GET", "/assets/.env").await;
    assert_eq!(status_of(&frames), 404, "{frames:?}");
    assert!(
        !world
            .log
            .lock()
            .unwrap()
            .saw(2, |frame| matches!(frame, Frame::ReqHead(_))),
        "a dotfile fell through to the view"
    );

    // The deny side of step 0: `..` beneath a mount is a `400` before any view.
    let frames = ask(&mut world, 11, "GET", "/assets/../secret").await;
    assert_eq!(status_of(&frames), 400, "{frames:?}");

    // The allow side of the same shape: a path the view claims and no mount
    // claims is answered by the view.
    let frames = ask(&mut world, 12, "GET", "/api/notes").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"the-api");

    // A request that matches no mount and no view is step 4's `404`.
    let frames = ask(&mut world, 13, "GET", "/nowhere").await;
    assert_eq!(status_of(&frames), 404, "{frames:?}");

    // And the two mounts are the file's, in file order.
    assert_eq!(
        world
            .cfg
            .mounts
            .iter()
            .map(|mount| mount.prefix.as_str())
            .collect::<Vec<_>>(),
        ["/assets", "/assets/v2"]
    );
}

// ---------------------------------------------------------------------------
// §10.1 ingress: json, form, none, raw
// ---------------------------------------------------------------------------

/// `body json`: the allowed body reaches the view; a form body is a `400` before
/// it does.
#[tokio::test]
async fn row_body_json_accepts_json_and_refuses_a_form_body() {
    tokio::time::pause();
    let source = "view upload POST \"/json/\" {\n    body json\n}\n";
    let manifest = parsed(source);
    assert_eq!(
        manifest.route(0).unwrap().ingress().unwrap().body,
        BodyCodec::Json
    );
    assert!(!manifest.route(0).unwrap().ingress().unwrap().allow_raw);

    let mut world = World::start(Options::new().blastfile(source).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));

    let frames = round_trip(
        &mut world,
        1,
        chunked("POST", "/json/", "application/json"),
        b"{\"a\":1}",
    )
    .await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"ok");

    // Denied: a form body on a JSON route is a `400` and the view is never
    // dispatched.
    let frames = round_trip(
        &mut world,
        2,
        chunked("POST", "/json/", "application/x-www-form-urlencoded"),
        b"a=1",
    )
    .await;
    assert_eq!(status_of(&frames), 400, "{frames:?}");
    assert_eq!(
        world.log.lock().unwrap().request_ids(0),
        [1],
        "only the JSON request reached the view"
    );
}

/// `body form`: the allowed body reaches the view; a JSON body is a `400`.
#[tokio::test]
async fn row_body_form_accepts_a_form_and_refuses_json() {
    tokio::time::pause();
    let source = "view upload POST \"/form/\" {\n    body form\n}\n";
    assert_eq!(
        parsed(source).route(0).unwrap().ingress().unwrap().body,
        BodyCodec::Form
    );

    let mut world = World::start(Options::new().blastfile(source).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));

    let frames = round_trip(
        &mut world,
        1,
        chunked("POST", "/form/", "application/x-www-form-urlencoded"),
        b"a=1&b=2",
    )
    .await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"ok");

    let frames = round_trip(
        &mut world,
        2,
        chunked("POST", "/form/", "application/json"),
        b"{}",
    )
    .await;
    assert_eq!(status_of(&frames), 400, "{frames:?}");
    assert_eq!(world.log.lock().unwrap().request_ids(0), [1]);
}

/// `body none`: any body is a `413`, and an empty body is answered.
#[tokio::test]
async fn row_body_none_refuses_any_body() {
    tokio::time::pause();
    let source = "view plain POST \"/none/\" {\n    body none\n}\n";
    assert_eq!(
        parsed(source).route(0).unwrap().ingress().unwrap().body,
        BodyCodec::None
    );

    let mut world = World::start(Options::new().blastfile(source).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));

    // Allowed: no body at all.
    let frames = round_trip(&mut world, 1, head("POST", "/none/"), b"").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");

    // Denied: one byte is a `413`, and no body byte reaches the view.
    let mut request = head("POST", "/none/");
    request
        .headers
        .push((b"Content-Length".to_vec(), b"1".to_vec()));
    let frames = round_trip(&mut world, 2, request, b"x").await;
    assert_eq!(status_of(&frames), 413, "{frames:?}");
    assert!(
        !world
            .log
            .lock()
            .unwrap()
            .saw(0, |frame| matches!(frame, Frame::ReqBody(_))),
        "a body byte reached a `none` route"
    );
}

/// `body raw`, unacknowledged: the lowered policy, the boot warning, the
/// production refusal list — and the bytes still streaming through under `dev`.
#[tokio::test]
async fn row_body_raw_is_unacknowledged_and_streams() {
    tokio::time::pause();
    let source = "view upload POST \"/raw/\" {\n    body raw\n}\n";
    let manifest = parsed(source);
    let policy = manifest.route(0).unwrap().ingress().unwrap();
    assert_eq!(policy.body, BodyCodec::Raw);
    assert!(!policy.allow_raw, "`body raw` is unacknowledged");

    // §10.2: the `RawIngress` warning is the dev half, the refusal list the
    // production gate's input. `crates/blastit/tests/critical_raw_ingress.rs`
    // pins the two process behaviours (production exit `1`, `dev` serves + warns);
    // this is the same list that suite's boot consults.
    assert!(
        has_warning(&warnings(source), |warning| matches!(
            warning,
            ManifestWarning::RawIngress { .. }
        )),
        "an unacknowledged raw body warns"
    );
    // §10.2's exact wording is `MB.21.4.3`'s re-spelling, so this asserts the
    // gate's *shape*: one refusal, for this view, naming it. The process
    // behaviours are `critical_raw_ingress.rs`'s.
    let refusals = manifest.raw_ingress_refusals();
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(
        refusals[0].contains("upload"),
        "the refusal names the view it is about: {refusals:?}"
    );

    // `dev` serves it: the bytes reach the view unchanged.
    let mut world = World::start(Options::new().blastfile(source).script(
        0,
        Script::Consume {
            body: b"stored".to_vec(),
        },
    ));
    let frames = round_trip(
        &mut world,
        1,
        chunked("POST", "/raw/", "application/octet-stream"),
        b"raw-bytes",
    )
    .await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    let seen: Vec<u8> = world
        .log
        .lock()
        .unwrap()
        .received_by(0)
        .into_iter()
        .filter_map(|(_, frame)| match frame {
            Frame::ReqBody(body) => Some(body),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(seen, b"raw-bytes", "raw passes the bytes through");
}

/// `body raw unvalidated`: the same warning, no refusal, the same bytes.
#[tokio::test]
async fn row_body_raw_unvalidated_boots_with_the_same_warning() {
    tokio::time::pause();
    let source = "view upload POST \"/raw/\" {\n    body raw unvalidated\n}\n";
    let manifest = parsed(source);
    let policy = manifest.route(0).unwrap().ingress().unwrap();
    assert_eq!(policy.body, BodyCodec::Raw);
    assert!(policy.allow_raw, "`unvalidated` is the acknowledgement");

    assert!(
        has_warning(&warnings(source), |warning| matches!(
            warning,
            ManifestWarning::RawIngress { .. }
        )),
        "the acknowledgement keeps the warning: the bytes are still unvalidated"
    );
    assert!(
        manifest.raw_ingress_refusals().is_empty(),
        "acknowledged raw is not refused by production"
    );

    let mut world = World::start(Options::new().blastfile(source).script(
        0,
        Script::Consume {
            body: b"stored".to_vec(),
        },
    ));
    let frames = round_trip(
        &mut world,
        1,
        chunked("POST", "/raw/", "application/octet-stream"),
        b"raw-bytes",
    )
    .await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
}

// ---------------------------------------------------------------------------
// §12.4 prefork and egress
// ---------------------------------------------------------------------------

/// `prefork` exists at boot: the scheduler spawns that view before any request,
/// and only that one.
#[tokio::test]
async fn row_prefork_reaches_the_route_the_scheduler_spawns() {
    tokio::time::pause();
    let source = "view hot GET \"/hot\" {\n    prefork\n}\nview cold GET \"/cold\" {}\n";
    let manifest = parsed(source);
    assert!(manifest.route(0).unwrap().prefork());
    assert!(!manifest.route(1).unwrap().prefork());

    let mut world = World::start(Options::new().blastfile(source));
    world
        .until(|world| world.log.lock().unwrap().spawns.contains(&0))
        .await;
    assert_eq!(
        world.log.lock().unwrap().spawns,
        [0],
        "only the preforked view exists before a request"
    );

    // The cold view is spawned on first contact, which is what makes `prefork`
    // the difference.
    let frames = ask(&mut world, 1, "GET", "/cold").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert!(world.log.lock().unwrap().spawns.contains(&1));
}

/// `egress any`: the flag, the `EgressRoute` warning and a served request all
/// arrive together.
#[tokio::test]
async fn row_egress_any_and_its_warning_arrive_together() {
    tokio::time::pause();
    let source = "view out GET \"/out\" {\n    egress any\n}\n";
    let manifest = parsed(source);
    assert!(manifest.route(0).unwrap().egress());
    assert!(
        has_warning(&warnings(source), |warning| matches!(
            warning,
            ManifestWarning::EgressRoute { .. }
        )),
        "the warning is the whole gate: egress is never silent"
    );

    let mut world = World::start(Options::new().blastfile(source).script(
        0,
        Script::Answer {
            body: b"sent".to_vec(),
        },
    ));
    let frames = ask(&mut world, 1, "GET", "/out").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(body_of(&frames), b"sent");
}

// ---------------------------------------------------------------------------
// §12.2 the file-level entrypoint
// ---------------------------------------------------------------------------

/// `entrypoint php` is one policy for the file, every view is in the table the
/// entry is bound to, and a booted `World` answers **each** view's own request
/// from it.
#[tokio::test]
async fn row_entrypoint_php_answers_every_view() {
    tokio::time::pause();
    let source = concat!(
        "entrypoint php \"public/index.php\"\n",
        "\n",
        "view home GET \"/\" {}\n",
        "view api GET \"/api/*\" {}\n",
    );
    let manifest = parsed(source);

    // One entry, and no view carries its own.
    let php = manifest.php().expect("the front controller");
    assert_eq!(php.entry, "public/index.php");
    assert!(
        manifest.routes().iter().all(|route| route.php().is_none()),
        "a `Blastfile` view has no php block to carry"
    );

    // Every view is a route the one entry answers: the binding loops this exact
    // list with this exact policy (`crates/blastit/src/php_view.rs::boot_bindings`).
    let routes = blastit_scheduler::routes_from_manifest(&manifest, &UidLayout::default());
    assert_eq!(routes.len(), manifest.routes().len());
    assert_eq!(
        routes
            .iter()
            .map(|route| route.name.as_str())
            .collect::<Vec<_>>(),
        ["home", "api"]
    );

    // And it needs no `BLASTIT_APP`: the file is its own executor (§9.5).
    assert!(manifest.can_boot_without_app());

    // The booted product: one policy for the file means every view is served,
    // so each of them answers its own request, and the entry does not become a
    // catch-all — a path no view claims is still the `404`.
    let mut world = World::start(
        Options::new()
            .blastfile(source)
            .script(
                0,
                Script::Answer {
                    body: b"the-home-view".to_vec(),
                },
            )
            .script(
                1,
                Script::Answer {
                    body: b"the-api-view".to_vec(),
                },
            ),
    );
    let home = ask(&mut world, 1, "GET", "/").await;
    assert_eq!(status_of(&home), 200, "{home:?}");
    assert_eq!(body_of(&home), b"the-home-view");
    assert_eq!(
        world.log.lock().unwrap().request_ids(0).len(),
        1,
        "the first view answered its own request"
    );
    let api = ask(&mut world, 2, "GET", "/api/orders").await;
    assert_eq!(status_of(&api), 200, "{api:?}");
    assert_eq!(body_of(&api), b"the-api-view");
    assert_eq!(
        world.log.lock().unwrap().request_ids(1).len(),
        1,
        "the second view answered its own request"
    );
    assert_eq!(
        world.log.lock().unwrap().request_ids(0).len(),
        1,
        "the second view's request did not reach the first: {api:?}"
    );
    let unmatched = ask(&mut world, 3, "GET", "/no-view-claims-this").await;
    assert_eq!(
        status_of(&unmatched),
        404,
        "the one entry is bound to the views, not to the working directory: {unmatched:?}"
    );

    // Static mounts are still tried first: the entry does not swallow a file.
    let with_mount = concat!(
        "entrypoint php \"public/index.php\"\n",
        "\n",
        "static \"/assets\" {\n",
        "    root \"public/assets\"\n",
        "}\n",
        "\n",
        "view home GET \"/\" {}\n",
    );
    assert_eq!(
        MountTable::new(&parsed(with_mount))
            .lookup("/assets/app.js")
            .map(|found| found.mount),
        Some(0)
    );
}

// ---------------------------------------------------------------------------
// §8.4 HEAD implied by GET
// ---------------------------------------------------------------------------

/// `HEAD` is implied by `GET` before the table is built, and the real matcher
/// and a real request both see it.
#[tokio::test]
async fn row_head_is_implied_by_get_through_the_matcher() {
    tokio::time::pause();
    let source = "view page GET \"/page\" {}\nview any \"/any\" {}\n";
    let manifest = parsed(source);
    assert_eq!(
        manifest.route(0).unwrap().methods(),
        Some(["GET".to_string(), "HEAD".to_string()].as_slice()),
        "lowering adds HEAD"
    );

    // The matcher, not only the parse.
    let table = RoutingTable::new(&manifest).expect("the table builds");
    assert!(table.route("GET", "/page").is_some());
    assert!(
        table.route("HEAD", "/page").is_some(),
        "HEAD matched the GET view"
    );
    assert!(table.route("POST", "/page").is_none());
    assert_eq!(
        table.methods_for("/page"),
        Some(vec!["GET".to_string(), "HEAD".to_string()])
    );

    // Through the wire: a HEAD is dispatched to the GET view at all. Without the
    // implication this is a `405` from `methods_for`, so the dispatch is the
    // assertion. (Stripping the body is `blastit-http`'s job at the outer layer:
    // the scheduler forwards exactly what the view sends.)
    let mut world = World::start(Options::new().blastfile(source).script(
        0,
        Script::Answer {
            body: b"page-body".to_vec(),
        },
    ));
    let frames = ask(&mut world, 1, "HEAD", "/page").await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(
        world.log.lock().unwrap().request_ids(0),
        [1],
        "the GET view ran for a HEAD"
    );
}
