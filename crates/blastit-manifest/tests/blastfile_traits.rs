//! Traits and `use` (§11), through the public entry point (`R12`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Grant, Manifest, ManifestWarning, ProxyName};

/// The one store every document here declares.
const STORE: &str = "db";

/// A document with one store, one trait and one view that uses it.
fn document() -> &'static str {
    "postgres $DATABASE_URL\n\
     \n\
     trait session {\n  postgres {\n    transactions\n    query \"SET TIME ZONE 'UTC'\"\n  }\n}\n\
     \n\
     view account \"/account\" {\n  use session\n  body json\n}\n"
}

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// A trait lowers to a `Group` with the same grants (§11.1).
#[test]
fn a_trait_is_a_group() {
    let manifest = Manifest::from_blastfile_str(document()).expect("lowers");
    let group = manifest.group("session").expect("the trait");
    assert_eq!(group.name(), "session");
    let grant = group
        .grant(&ProxyName::from(STORE))
        .expect("the trait's grant")
        .as_postgresql()
        .expect("a postgresql grant");
    assert!(grant.transaction_control());
    assert_eq!(grant.queries(), ["SET TIME ZONE 'UTC'"]);
    // The trait is a group, not a route: it owns no sandbox and no path.
    assert_eq!(manifest.route_count(), 1);
}

/// `use session` attaches the trait to the view, and only by name (§11.1).
#[test]
fn use_attaches_the_trait() {
    let manifest = Manifest::from_blastfile_str(document()).expect("lowers");
    let route = manifest.route(0).expect("one route");
    assert_eq!(route.groups(), ["session"]);
    // There is no global attach: the view's own list is the only one.
    assert!(manifest.global_groups().is_empty());
}

/// `use session, catalog` keeps written order and drops a repeated name.
#[test]
fn use_keeps_written_order_and_deduplicates() {
    let source = "postgres $DATABASE_URL\n\
                  trait session {\n  postgres { transactions }\n}\n\
                  trait catalog {\n  postgres { select products }\n}\n\
                  view v \"/v\" {\n  use session, catalog, session\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(
        manifest.route(0).expect("one route").groups(),
        ["session", "catalog"]
    );
}

/// A trait may be written after the view that uses it (§11.2).
#[test]
fn use_resolves_after_the_trait_map_exists() {
    let source = "postgres $DATABASE_URL\n\
                  view v \"/v\" {\n  use session\n}\n\
                  trait session {\n  postgres { transactions }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(manifest.route(0).expect("one route").groups(), ["session"]);
    assert!(manifest.group("session").is_some());
}

/// `use <name>` of no trait is `UnknownTrait`, at the name's own `line:col`,
/// with §11.2's sentence (§11.3).
#[test]
fn an_unknown_trait_is_refused_at_its_own_position() {
    let source = "postgres $DATABASE_URL\n\
                  view products \"/products\" {\n  use catalog\n}\n";
    let rendered = refusal(source);
    assert_eq!(
        rendered,
        "3:7: view \"products\": `use catalog` names no trait; a trait is declared at the top level: \
         `trait catalog { … }`"
    );
    // The second name of a list is refused at its own column.
    let source = "postgres $DATABASE_URL\n\
                  trait session {\n  postgres { transactions }\n}\n\
                  view v \"/v\" {\n  use session, catalog\n}\n";
    assert!(
        refusal(source).starts_with("6:16: view \"v\": `use catalog` names no trait"),
        "{}",
        refusal(source)
    );
}

/// `unrestricted` inside a trait is `UnrestrictedGroup`, naming the trait
/// (§11.3).
#[test]
fn unrestricted_inside_a_trait_is_refused() {
    let source = "postgres $DATABASE_URL\n\
                  trait session {\n  postgres unrestricted\n}\n\
                  view v \"/v\" {\n  use session\n}\n";
    let rendered = refusal(source);
    // `validate` names the section by the group it lowers to; re-spelling that
    // sentence is `MB.21.3.0`'s (§13.2), not this leaf's.
    assert!(rendered.contains("group \"session\""), "{rendered}");
    assert!(
        rendered.contains("unrestricted is only allowed inline on a route"),
        "{rendered}"
    );
}

/// A trait declared twice is refused by the parser, naming the first line
/// (§11.3).
#[test]
fn a_trait_declared_twice_is_refused() {
    let source = "trait session {\n  postgres { transactions }\n}\n\
                  trait session {\n  postgres { select products }\n}\n";
    assert_eq!(
        refusal(source),
        "4:1: `trait \"session\"` is declared twice; the first is on line 1"
    );
}

/// A trait no view uses is `UnattachedGroup`, and a trait with no `postgres`
/// block is accepted (§11.3).
#[test]
fn an_unused_trait_warns_and_an_empty_trait_is_legal() {
    let source = "postgres $DATABASE_URL\n\
                  trait unused {\n  postgres { transactions }\n}\n\
                  trait empty { }\n\
                  view v \"/v\" {\n  use empty\n}\n";
    let (manifest, warnings) = Manifest::parse_blastfile(source, 20_000).expect("validates");
    assert!(
        manifest
            .group("empty")
            .expect("the empty trait")
            .grants()
            .is_empty()
    );
    let unattached: Vec<&str> = warnings
        .iter()
        .filter_map(|warning| match warning {
            ManifestWarning::UnattachedGroup { group } => Some(group.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(unattached, ["unused"]);
}

/// A store a trait grants but nothing declares is `GrantOnUnknownProxy`, naming
/// the trait and the store (§11.3).
#[test]
fn a_trait_grant_on_an_undeclared_store_is_refused() {
    let source = "postgres $DATABASE_URL\n\
                  trait reporting {\n  postgres analytics { select events }\n}\n\
                  view v \"/v\" {\n  use reporting\n}\n";
    let rendered = refusal(source);
    assert!(rendered.contains("trait \"reporting\""), "{rendered}");
    assert!(
        rendered.contains("grant on unknown proxy \"analytics\""),
        "{rendered}"
    );
}

/// `egress any` inside a trait is a parse refusal naming the trait and the fix
/// (§11.3).
#[test]
fn egress_inside_a_trait_is_refused() {
    let source = "trait session {\n  egress any\n}\n";
    assert_eq!(
        refusal(source),
        "2:3: a trait holds `postgres` blocks only; `egress any` belongs on a view"
    );
}

/// Effective grants are the traits in `use` order, then the view's inline
/// grants: transaction control OR-ed and queries deduplicated in insertion
/// order (§11.1).
#[test]
fn effective_grants_chain_traits_then_the_view() {
    let source = "postgres $DATABASE_URL\n\
                  trait first {\n  postgres { query \"SELECT 1\" }\n}\n\
                  trait second {\n  postgres { transactions; query \"SELECT 1\" }\n}\n\
                  view v \"/v\" {\n  use first, second\n  postgres { query \"SELECT 2\" }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    let effective = manifest.effective_grants(0);
    let grant: &Grant = effective.get(&ProxyName::from(STORE)).expect("the grant");
    let grant = grant.as_postgresql().expect("a postgresql grant");
    assert!(grant.transaction_control(), "OR-ed across the traits");
    assert_eq!(
        grant.queries(),
        ["SELECT 1", "SELECT 2"],
        "deduplicated in order"
    );
}
