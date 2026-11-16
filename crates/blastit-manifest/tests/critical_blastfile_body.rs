//! The four codecs, the two `raw` forms and the acknowledgement (§10, `R11`).
//!
//! `raw` is the one ingress declaration the runtime does not gate, so the
//! manifest's acknowledgement is the whole of the decision: this suite proves
//! both halves — the warning every `raw` route emits, and the production
//! refusal only the unacknowledged one earns.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{BodyCodec, IngressPolicy, Manifest, ManifestWarning};

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// The one route's ingress policy.
fn ingress(source: &str) -> Option<IngressPolicy> {
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    manifest.route(0).expect("one route").ingress()
}

/// The four codecs and the two `raw` forms, exactly as §10.1's table.
#[test]
fn the_four_codecs_and_the_two_raw_forms() {
    let rows: &[(&str, BodyCodec, bool)] = &[
        ("json", BodyCodec::Json, false),
        ("form", BodyCodec::Form, false),
        ("none", BodyCodec::None, false),
        ("raw", BodyCodec::Raw, false),
        ("raw unvalidated", BodyCodec::Raw, true),
    ];
    for (written, body, allow_raw) in rows {
        let source = format!("view v \"/v\" {{\n  body {written}\n}}\n");
        assert_eq!(
            ingress(&source),
            Some(IngressPolicy {
                body: *body,
                allow_raw: *allow_raw
            }),
            "{written}"
        );
    }
}

/// A route that declares no `body` has `None`, which is fail-closed: any
/// non-empty body is refused (`R12`).
#[test]
fn no_body_is_fail_closed() {
    assert_eq!(ingress("view v \"/v\" {\n}\n"), None);
    let manifest = Manifest::from_blastfile_str("view v \"/v\" {\n}\n").expect("lowers");
    assert_eq!(manifest.route(0).and_then(|route| route.ingress()), None);
}

/// `body raw` warns and is refused on the production path; `body raw
/// unvalidated` warns and boots.
#[test]
fn the_two_raw_forms_differ_only_by_the_acknowledgement() {
    let unacknowledged = "view upload \"/upload\" {\n  body raw\n}\n";
    let (manifest, warnings) =
        Manifest::parse_blastfile(unacknowledged, 20_000).expect("validates");
    assert_eq!(
        warnings,
        [ManifestWarning::RawIngress {
            route: "upload".to_owned()
        }]
    );
    let refusals = manifest.raw_ingress_refusals();
    assert_eq!(refusals.len(), 1, "the production path refuses it");
    // §10.2 re-spells this sentence and it lives in `validate.rs`, which this
    // leaf does not own: the assertion is that it names the view, not the words.
    assert!(refusals[0].contains("\"upload\""), "{}", refusals[0]);

    let acknowledged = "view upload \"/upload\" {\n  body raw unvalidated\n}\n";
    let (manifest, warnings) = Manifest::parse_blastfile(acknowledged, 20_000).expect("validates");
    assert_eq!(
        warnings,
        [ManifestWarning::RawIngress {
            route: "upload".to_owned()
        }],
        "the warning is the same either way"
    );
    assert!(
        manifest.raw_ingress_refusals().is_empty(),
        "the acknowledgement is the whole of the decision"
    );
}

/// `unvalidated` acknowledges `raw` only, so it can never be written beside a
/// codec that already validates (§6.7, §10.1).
#[test]
fn unvalidated_is_refused_beside_a_validating_codec() {
    for codec in ["json", "form", "none"] {
        let source = format!("view v \"/v\" {{\n  body {codec} unvalidated\n}}\n");
        assert_eq!(
            refusal(&source),
            format!(
                "2:13: `unvalidated` acknowledges `raw` only; `body {codec}` already validates the body"
            )
        );
    }
}

/// The acknowledgement has no second spelling: not `=`, not a second directive
/// (§10.1, §19.6).
#[test]
fn the_acknowledgement_has_one_spelling() {
    assert_eq!(
        refusal("view v \"/v\" {\n  body raw = unvalidated\n}\n"),
        "2:12: `=` is not Blastfile syntax; write the value after the keyword: `body json`"
    );
    // `allow_raw` is not a keyword at all.
    assert_eq!(
        refusal("view v \"/v\" {\n  allow_raw true\n}\n"),
        "2:3: `allow_raw` is not a method: methods are upper-case, `ALLOW_RAW`"
    );
}

/// A `body` inside an endpoint belongs to that endpoint, and one outside to all
/// of them (§10.3).
#[test]
fn a_body_belongs_where_it_is_written() {
    let source = "view cart {\n  body json\n  GET \"/cart\"\n  POST \"/cart/items\" { body raw unvalidated }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(
        manifest.route(0).and_then(|route| route.ingress()),
        Some(IngressPolicy {
            body: BodyCodec::Json,
            allow_raw: false
        })
    );
    assert_eq!(
        manifest.route(1).and_then(|route| route.ingress()),
        Some(IngressPolicy {
            body: BodyCodec::Raw,
            allow_raw: true
        })
    );
    // Both routes are one sandbox, so the two policies share one worker.
    assert_eq!(manifest.route(0).map(|route| route.sandbox()), Some(0));
    assert_eq!(manifest.route(1).map(|route| route.sandbox()), Some(0));
}
