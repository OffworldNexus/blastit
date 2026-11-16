//! `body` and the two `raw` forms (§10), through the public entry point
//! (`R11`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{BodyCodec, IngressPolicy, Manifest, ManifestWarning, Route};

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// The lowered route's ingress policy.
fn ingress(source: &str) -> Option<IngressPolicy> {
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    manifest.route(0).expect("one route").ingress()
}

/// §10.1's five rows, one document each.
#[test]
fn the_five_body_rows() {
    assert_eq!(
        ingress("view v \"/v\" {\n  body json\n}\n"),
        Some(IngressPolicy {
            body: BodyCodec::Json,
            allow_raw: false
        })
    );
    assert_eq!(
        ingress("view v \"/v\" {\n  body form\n}\n"),
        Some(IngressPolicy {
            body: BodyCodec::Form,
            allow_raw: false
        })
    );
    assert_eq!(
        ingress("view v \"/v\" {\n  body none\n}\n"),
        Some(IngressPolicy {
            body: BodyCodec::None,
            allow_raw: false
        })
    );
    assert_eq!(
        ingress("view v \"/v\" {\n  body raw\n}\n"),
        Some(IngressPolicy {
            body: BodyCodec::Raw,
            allow_raw: false
        })
    );
    assert_eq!(
        ingress("view v \"/v\" {\n  body raw unvalidated\n}\n"),
        Some(IngressPolicy {
            body: BodyCodec::Raw,
            allow_raw: true
        }),
        "the acknowledgement is a sub-token of `body`"
    );
    // No `body` at all is `None`, which behaves like `none` and is fail-closed.
    assert_eq!(ingress("view v \"/v\" {\n}\n"), None);
}

/// `body` outside an endpoint applies to every endpoint of the view (§10.3).
#[test]
fn a_view_body_applies_to_every_endpoint() {
    let source = "view cart {\n  body json\n  GET \"/cart\"\n  POST \"/cart/items\"\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    let body = IngressPolicy {
        body: BodyCodec::Json,
        allow_raw: false,
    };
    assert_eq!(manifest.route(0).and_then(Route::ingress), Some(body));
    assert_eq!(manifest.route(1).and_then(Route::ingress), Some(body));
}

/// `body` inside an endpoint applies to that endpoint only, and overrides the
/// view's (§10.3).
#[test]
fn an_endpoint_body_wins_over_the_view_body() {
    let source =
        "view cart {\n  body none\n  GET \"/cart\"\n  POST \"/cart/items\" { body json }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(
        manifest.route(0).and_then(Route::ingress),
        Some(IngressPolicy {
            body: BodyCodec::None,
            allow_raw: false
        })
    );
    assert_eq!(
        manifest.route(1).and_then(Route::ingress),
        Some(IngressPolicy {
            body: BodyCodec::Json,
            allow_raw: false
        })
    );
    // One view, one sandbox, whatever the endpoints declare.
    assert_eq!(manifest.route(0).map(Route::sandbox), Some(0));
    assert_eq!(manifest.route(1).map(Route::sandbox), Some(0));
}

/// `unvalidated` acknowledges `raw` only (§6.7, §10.1).
#[test]
fn unvalidated_acknowledges_raw_only() {
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

/// `body` with no codec, and a codec that is not one of the four, are refused
/// (§6.7).
#[test]
fn a_body_needs_one_of_the_four_codecs() {
    assert_eq!(
        refusal("view v \"/v\" {\n  body\n}\n"),
        "2:7: `body` needs a codec: `json`, `form`, `none` or `raw`"
    );
    assert_eq!(
        refusal("view v \"/v\" {\n  body xml\n}\n"),
        "2:8: `xml` is not a codec; the codecs are `json`, `form`, `none`, `raw`"
    );
}

/// Both `raw` forms warn at boot, acknowledged or not (§10.1, §13.2).
#[test]
fn both_raw_forms_warn() {
    for source in [
        "view upload \"/upload\" {\n  body raw\n}\n",
        "view upload \"/upload\" {\n  body raw unvalidated\n}\n",
    ] {
        let (_, warnings) = Manifest::parse_blastfile(source, 20_000).expect("validates");
        let raw: Vec<&str> = warnings
            .iter()
            .filter_map(|warning| match warning {
                ManifestWarning::RawIngress { route } => Some(route.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(raw, ["upload"], "{source:?}");
    }
}

/// The production gate refuses only the unacknowledged form, and its sentence
/// is `validate`'s (§10.2).
#[test]
fn the_production_gate_refuses_only_the_unacknowledged_form() {
    let refused =
        Manifest::from_blastfile_str("view upload \"/upload\" {\n  body raw\n}\n").expect("lowers");
    assert_eq!(refused.raw_ingress_refusals().len(), 1);

    let acknowledged =
        Manifest::from_blastfile_str("view upload \"/upload\" {\n  body raw unvalidated\n}\n")
            .expect("lowers");
    assert!(
        acknowledged.raw_ingress_refusals().is_empty(),
        "the acknowledgement is the whole of the decision"
    );
}

/// A body declared twice in one view, and twice in one endpoint, are refused
/// (§6.7).
#[test]
fn a_second_body_is_refused() {
    assert_eq!(
        refusal("view v \"/v\" {\n  body json\n  body form\n}\n"),
        "3:3: `body` is declared once per view; the one for a single method goes inside that endpoint"
    );
    assert_eq!(
        refusal("view v {\n  GET \"/v\" {\n    body json\n    body form\n  }\n}\n"),
        "4:5: `body` is declared once per endpoint"
    );
}
