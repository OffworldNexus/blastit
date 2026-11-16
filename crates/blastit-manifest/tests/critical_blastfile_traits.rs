//! §11.3's refusals and warnings, one trait attached to every view (`R12`).
//!
//! A trait is the retired format's shared grant set, so this is the suite that
//! proves a shared set can only be attached deliberately: `use` in each view,
//! never a global list, and an `unrestricted` inside a trait can never be
//! attached by accident.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Manifest, ManifestWarning, ProxyName};

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// One trait, attached to every view, gives every view its grants.
#[test]
fn one_trait_attached_to_every_view() {
    let source = "postgres $DATABASE_URL\n\
                  trait session {\n  postgres { transactions }\n}\n\
                  view a \"/a\" {\n  use session\n}\n\
                  view b \"/b\" {\n  use session\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(manifest.route_count(), 2);
    for index in 0..2 {
        let route = manifest.route(index).expect("a route");
        assert_eq!(route.groups(), ["session"]);
        let effective = manifest.effective_grants(index);
        let grant = effective
            .get(&ProxyName::from("db"))
            .expect("the trait's grant")
            .as_postgresql()
            .expect("a postgresql grant");
        assert!(grant.transaction_control());
    }
    // The trait is attached twice and warned about zero times.
    let (_, warnings) = Manifest::parse_blastfile(source, 20_000).expect("validates");
    assert!(
        !warnings
            .iter()
            .any(|warning| matches!(warning, ManifestWarning::UnattachedGroup { .. })),
        "{warnings:?}"
    );
}

/// `unrestricted` inside a trait is refused, so a shared set can never hand out
/// every statement (§11.3).
#[test]
fn unrestricted_inside_a_trait_is_refused() {
    for source in [
        "postgres $DATABASE_URL\ntrait t {\n  postgres unrestricted\n}\nview v \"/v\" {\n  use t\n}\n",
        "postgres $DATABASE_URL\ntrait t {\n  postgres db unrestricted\n}\nview v \"/v\" {\n  use t\n}\n",
    ] {
        let rendered = refusal(source);
        assert!(
            rendered.contains("unrestricted is only allowed inline on a view"),
            "{rendered}"
        );
        assert!(rendered.contains("trait \"t\""), "{rendered}");
    }
}

/// A trait that grants a store nothing declares is refused, naming the trait and
/// the store (§11.3).
#[test]
fn a_trait_grant_on_an_undeclared_store_is_refused() {
    let rendered = refusal(
        "postgres $DATABASE_URL\n\
         trait t {\n  postgres analytics { select events }\n}\n\
         view v \"/v\" {\n  use t\n}\n",
    );
    assert!(rendered.contains("trait \"t\""), "{rendered}");
    assert!(
        rendered.contains("grant on unknown proxy \"analytics\""),
        "{rendered}"
    );
}

/// `use` of no trait is refused, at the name's own position (§11.3).
#[test]
fn an_unknown_trait_is_refused() {
    let rendered = refusal("view v \"/v\" {\n  use ghost\n}\n");
    assert_eq!(
        rendered,
        "2:7: view \"v\": `use ghost` names no trait; a trait is declared at the top level: \
         `trait ghost { … }`"
    );
}

/// A directive that is not `postgres` inside a trait is a parse refusal naming
/// the trait's rule (§11.3).
#[test]
fn only_postgres_belongs_in_a_trait() {
    for (source, expected) in [
        (
            "trait t {\n  view cart \"/c\"\n}\n",
            "2:3: a trait holds `postgres` blocks only; `view` belongs at the top level",
        ),
        (
            "trait t {\n  egress any\n}\n",
            "2:3: a trait holds `postgres` blocks only; `egress any` belongs on a view",
        ),
        (
            "trait t {\n  body json\n}\n",
            "2:3: a trait holds `postgres` blocks only; `body` belongs on a view",
        ),
    ] {
        assert_eq!(refusal(source), expected);
    }
}

/// A trait no view uses warns, and a trait with no `postgres` block is accepted
/// (§11.3).
#[test]
fn an_unused_trait_warns() {
    let source = "postgres $DATABASE_URL\ntrait unused {\n  postgres { select products }\n}\n";
    let (_, warnings) = Manifest::parse_blastfile(source, 20_000).expect("validates");
    assert_eq!(
        warnings
            .iter()
            .filter(|warning| matches!(warning, ManifestWarning::UnattachedGroup { .. }))
            .count(),
        1,
        "{warnings:?}"
    );
    // The warning names the trait, so an operator can find it.
    let rendered = warnings
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("\"unused\""), "{rendered}");
}

/// A trait declared twice is refused, naming the first line (§11.3).
#[test]
fn a_duplicate_trait_is_refused() {
    assert_eq!(
        refusal("trait t {\n}\ntrait t {\n}\n"),
        "3:1: `trait \"t\"` is declared twice; the first is on line 1"
    );
}
