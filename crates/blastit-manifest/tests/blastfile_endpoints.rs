//! `TESTING_PLAN.md` §7 — the `body` codecs, the PHP entrypoint rules, the
//! list budget, and the boot warnings a documented file produces.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{BodyCodec, Manifest, ManifestWarning, UidLayout};

const BASE: u32 = UidLayout::DEFAULT_BASE;

/// The lowered manifest of `source`, through the non-validating entry point.
fn lowered(source: &str) -> Manifest {
    Manifest::from_blastfile_str(source).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

/// The one refusal `source` produces, rendered as the developer sees it.
fn refusal(source: &str) -> String {
    Manifest::from_blastfile_str(source)
        .expect_err("the document is refused")
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}
#[test]
fn body_codecs_lower_and_raw_warns_at_boot() {
    for (written, expected) in [
        ("json", BodyCodec::Json),
        ("form", BodyCodec::Form),
        ("none", BodyCodec::None),
        ("raw unvalidated", BodyCodec::Raw),
    ] {
        let source = format!("view v POST \"/x\" {{\n    body {written}\n}}\n");
        let route = lowered(&source).route(0).cloned().expect("one view");
        let policy = route.ingress().expect("a body was declared");
        assert_eq!(policy.body, expected, "{written}");
        assert_eq!(policy.allow_raw, written.starts_with("raw"), "{written}");
    }
    // The unacknowledged form is a boot warning in `dev`, not a parse failure.
    let (_, warnings) = Manifest::parse_blastfile("view v POST \"/x\" {\n    body raw\n}\n", BASE)
        .expect("`body raw` parses");
    assert!(
        warnings.iter().any(|warning| matches!(
            warning,
            ManifestWarning::RawIngress { route } if route == "v"
        )),
        "{warnings:?}"
    );
    // Denied: a second body at either level, and `unvalidated` beside a codec.
    assert_eq!(
        refusal("view v POST \"/x\" {\n    body json\n    body form\n}\n"),
        "3:5: `body` is declared once per view; the one for a single method goes inside that endpoint"
    );
    assert_eq!(
        refusal("view v {\n    POST \"/x\" {\n        body json\n        body form\n    }\n}\n"),
        "4:9: `body` is declared once per endpoint"
    );
    assert_eq!(
        refusal("view v {\n    body json\n    body form\n}\n"),
        "3:5: `body` is declared once per view; the one for a single method goes inside that endpoint"
    );
    assert!(Manifest::from_blastfile_str("view v {\n    body form unvalidated\n}\n").is_err());
}

#[test]
fn the_php_entry_rules_keep_a_relative_php_path() {
    for (source, rule) in [
        ("entrypoint php \"../index.php\"\n", "must not contain .."),
        (
            "entrypoint php \"/srv/index.php\"\n",
            "must be a relative path",
        ),
        ("entrypoint php \"index.phtml\"\n", "must end in .php"),
        (
            "entrypoint php \"public\\\\index.php\"\n",
            "must not contain \\",
        ),
    ] {
        let errors = Manifest::from_blastfile_str(source).expect_err("the entry is refused");
        let rendered = errors.errors()[0].to_string();
        assert!(
            rendered.starts_with("1:1: entrypoint php: "),
            "{source:?}: {rendered}"
        );
        assert!(rendered.contains(rule), "{source:?}: {rendered}");
        assert!(
            !rendered.contains("index"),
            "the entry is never echoed: {rendered}"
        );
    }
    // A front controller reached by no view is legal, and says so.
    let (manifest, warnings) =
        Manifest::parse_blastfile("entrypoint php \"public/index.php\"\n", BASE).expect("it boots");
    assert!(manifest.php().is_some());
    assert!(
        warnings
            .iter()
            .any(|warning| matches!(warning, ManifestWarning::EntrypointNotReached { .. })),
        "{warnings:?}"
    );
    // `workers` is gone: the scheduler owns parallelism.
    assert_eq!(
        refusal("view v \"/x\" {\n    workers 4\n}\n"),
        "2:5: `workers` was removed in 0.2.0: the scheduler owns parallelism, so delete the line and add nothing"
    );
    // `fastcgi` is not a keyword, so it is refused like any other.
    assert!(Manifest::from_blastfile_str("entrypoint fastcgi \"x\"\n").is_err());
}
