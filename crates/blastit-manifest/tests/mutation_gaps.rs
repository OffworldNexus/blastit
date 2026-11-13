//! Standard-tier coverage for the manifest model's accessors and for the
//! one-line refusal messages `blastit` prints.
//!
//! The `critical_*` suites prove the promises (a manifest boots, an edit is
//! surgical, a CVE shape is refused). This file walks the model accessors, the
//! flags the parser derives and the error rendering, so each one's real
//! behaviour is pinned by a test instead of being assumed from the code.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::borrow::Borrow;

use blastit_manifest::{
    BodyCodec, Columns, Grant, Location, Manifest, ManifestError, ManifestErrors, ManifestWarning,
    PgGrant, Protocol, ProxyName, TableGrant, TableGrants, UidLayout,
};

use common::{
    parse_errors, section_5_1, section_6, text_map, texts, validate_errors, validate_warnings,
};

/// One table grant on `t`: every column may be read.
fn all_columns_of_t() -> TableGrants {
    TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    )
}

//
// Names, protocols and the raw proxy declaration.
//

#[test]
fn a_proxy_name_borrows_as_its_own_string() {
    let name = ProxyName::from("db");
    assert_eq!(name.as_str(), "db");
    assert_eq!(name.to_string(), "db");
    assert_eq!(<ProxyName as Borrow<str>>::borrow(&name), "db");
    assert_eq!(<ProxyName as AsRef<str>>::as_ref(&name), "db");
    assert_eq!(ProxyName::from(String::from("db")), name);
    assert_eq!(ProxyName::new("db"), name);

    // The `Borrow<str>` impl is what lets a `&str` address the map.
    let manifest = section_6();
    assert_eq!(
        manifest.proxies().get("db").map(|proxy| proxy.name()),
        Some(&name)
    );
}

#[test]
fn a_protocol_has_exactly_one_spelling() {
    assert_eq!(Protocol::Postgresql.as_str(), "postgresql");
    assert_eq!(Protocol::Postgresql.to_string(), "postgresql");
    assert_eq!(Protocol::ALL, [Protocol::Postgresql]);
    assert_eq!(Protocol::parse("postgresql"), Some(Protocol::Postgresql));
    assert_eq!(Protocol::parse("PostgreSQL"), None, "case matters");
    assert_eq!(Protocol::parse("mysql"), None, "not in this build");
    assert_eq!(Protocol::parse(""), None);
}

#[test]
fn a_proxy_keeps_input_and_output_exactly_as_written() {
    let manifest = section_5_1();
    assert_eq!(manifest.proxies().len(), 2);

    let db = manifest.proxy("db").expect("proxy db");
    assert_eq!(db.name(), &ProxyName::from("db"));
    assert_eq!(db.protocol(), Protocol::Postgresql);
    assert_eq!(
        text_map(db.input()),
        [("ca_cert", "DATABASE_CA_CERT"), ("url", "DATABASE_URL")]
    );
    assert!(
        db.output().is_empty(),
        "no output table: rewritten in place"
    );

    let analytics = manifest.proxy("analytics").expect("proxy analytics");
    assert_eq!(text_map(analytics.input()), [("url", "ANALYTICS_URL")]);
    assert_eq!(
        text_map(analytics.output()),
        [
            ("ANALYTICS_DB_HOST", "{{ socket_dir }}"),
            ("ANALYTICS_DB_NAME", "{{ database }}"),
            ("ANALYTICS_DB_PORT", "{{ port }}"),
            ("ANALYTICS_DB_USER", "{{ user }}"),
        ]
    );

    assert!(manifest.proxy("warehouse").is_none());
}

//
// `PgGrant`: what a grant holds and what each flag means.
//

#[test]
fn a_grant_reports_its_queries_flags_and_tables() {
    let plain = PgGrant::new(vec!["SELECT 1".to_string()], false);
    assert_eq!(texts(plain.queries()), ["SELECT 1"]);
    assert!(plain.tables().is_empty());
    assert!(!plain.transaction_control());
    assert!(!plain.is_unrestricted());
    assert!(!plain.is_empty());

    let control_only = PgGrant::new(vec![], true);
    assert!(control_only.transaction_control());
    assert!(!control_only.is_unrestricted());
    assert!(
        !control_only.is_empty(),
        "a grant that only allows BEGIN still needs a listener"
    );

    let tables_only = PgGrant::new(vec![], false).with_tables(all_columns_of_t());
    assert!(!tables_only.tables().is_empty());
    assert_eq!(
        tables_only
            .tables()
            .table("t")
            .and_then(|table| table.select.as_ref()),
        Some(&Columns::All)
    );
    assert!(!tables_only.transaction_control());
    assert!(
        !tables_only.is_empty(),
        "tables are a capability of their own"
    );

    let everything = PgGrant::unrestricted();
    assert!(everything.is_unrestricted());
    assert!(!everything.transaction_control(), "it needs no such flag");
    assert!(everything.queries().is_empty(), "it allows by not listing");
    assert!(everything.tables().is_empty());
    assert!(
        !everything.is_empty(),
        "unrestricted is the opposite of empty"
    );

    assert!(PgGrant::default().is_empty());
    assert!(PgGrant::new(vec![], false).is_empty());
}

#[test]
fn merging_grants_unions_every_field() {
    // Deduplicates queries in insertion order: `SELECT 1` stays once.
    let mut left = PgGrant::new(vec!["SELECT 1".to_string()], false);
    let right = PgGrant::new(vec!["SELECT 1".to_string(), "SELECT 2".to_string()], true)
        .with_tables(all_columns_of_t());
    left.merge_from(&right);
    assert_eq!(texts(left.queries()), ["SELECT 1", "SELECT 2"]);
    assert!(left.transaction_control(), "transaction control is OR-ed");
    assert!(!left.is_unrestricted());
    assert_eq!(
        left.tables()
            .table("t")
            .and_then(|table| table.select.as_ref()),
        Some(&Columns::All),
        "table grants are unioned"
    );

    // `unrestricted` is OR-ed too, and the merged grant then allows anything.
    let mut plain = PgGrant::new(vec!["SELECT 1".to_string()], false);
    plain.merge_from(&PgGrant::unrestricted());
    assert!(plain.is_unrestricted());
    assert_eq!(texts(plain.queries()), ["SELECT 1"], "the list is kept raw");
}

#[test]
fn a_grant_narrows_to_its_protocol() {
    let postgresql = Grant::Postgresql(PgGrant::new(vec!["SELECT 1".to_string()], false));
    assert!(!postgresql.is_empty());
    assert_eq!(
        postgresql
            .as_postgresql()
            .map(|grant| grant.queries().len()),
        Some(1)
    );
    assert_eq!(
        postgresql
            .clone()
            .into_postgresql()
            .expect("a postgresql grant")
            .queries()
            .len(),
        1
    );

    let empty = Grant::Postgresql(PgGrant::default());
    assert!(empty.is_empty());
    assert!(empty.as_postgresql().is_some());
    assert_eq!(
        empty.into_postgresql().map(|grant| grant.queries().len()),
        Some(0)
    );
}

//
// The parsed document: routes, groups and the top-level keys.
//

#[test]
fn a_route_reports_what_was_written() {
    let manifest = section_6();
    assert_eq!(manifest.route_count(), 4);
    assert_eq!(manifest.routes().len(), 4);

    let product = manifest.route(0).expect("route 0");
    assert_eq!(product.name(), "product-detail");
    assert_eq!(texts(product.path()), ["/products/[0-9]+/"]);
    assert_eq!(product.methods().map(texts), Some(vec!["GET", "HEAD"]));
    assert_eq!(texts(product.groups()), ["catalog"]);
    assert!(!product.egress());
    assert!(product.prefork());
    assert!(product.grant(&ProxyName::from("db")).is_none());
    assert!(product.ingress().is_none(), "the route declared none");

    let checkout = manifest.route(1).expect("route 1");
    assert_eq!(checkout.name(), "checkout");
    assert_eq!(checkout.methods(), None, "no `methods` means any method");
    assert!(checkout.egress());
    assert!(!checkout.prefork());
    assert!(checkout.ingress().is_none());
    assert_eq!(
        checkout
            .grants()
            .keys()
            .map(ProxyName::as_str)
            .collect::<Vec<_>>(),
        ["db"]
    );
    let inline = checkout
        .grant(&ProxyName::from("db"))
        .expect("the route's inline grant");
    assert!(!inline.is_empty());
    assert!(checkout.grant(&ProxyName::from("warehouse")).is_none());

    let fallback = manifest.route(3).expect("route 3");
    assert_eq!(texts(fallback.path()), [".*"]);
    assert!(fallback.grants().is_empty());
    assert!(!fallback.egress());
    assert!(!fallback.prefork());

    assert!(manifest.route(4).is_none());
}

#[test]
fn a_route_keeps_the_ingress_policy_it_declared() {
    let source = "version = 1\n\
                  [[route]]\nname = \"upload\"\npath = ['/upload/']\n\
                  [route.ingress]\nbody = \"json\"\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let ingress = manifest
        .route(0)
        .expect("route 0")
        .ingress()
        .expect("the route declared one");
    assert_eq!(ingress.body, BodyCodec::Json);
    assert!(!ingress.allow_raw);
}

#[test]
fn a_group_reports_its_name_and_grants() {
    let manifest = section_6();
    assert_eq!(manifest.groups().len(), 2);
    assert_eq!(texts(manifest.global_groups()), ["session"]);

    let session = manifest.group("session").expect("group session");
    assert_eq!(session.name(), "session");
    assert_eq!(session.grants().len(), 1);
    assert!(session.grant(&ProxyName::from("db")).is_some());
    assert!(session.grant(&ProxyName::from("warehouse")).is_none());

    assert!(manifest.group("ghost").is_none());
}

#[test]
fn the_top_level_keys_are_the_document() {
    let manifest = Manifest::from_toml_str(
        "version = 1\napp = \"shop.wsgi:application\"\npreload = [\"b\", \"a\"]\n",
    )
    .expect("parses");
    assert_eq!(manifest.version(), 1);
    assert_eq!(manifest.app(), Some("shop.wsgi:application"));
    assert_eq!(texts(manifest.preload()), ["b", "a"]);
    assert_eq!(manifest.route_count(), 0);
    assert!(manifest.route(0).is_none());
    assert!(manifest.routes().is_empty());
    assert!(manifest.proxies().is_empty());
    assert!(manifest.groups().is_empty());
    assert!(manifest.global_groups().is_empty());

    let bare = Manifest::from_toml_str("version = 1\n").expect("parses");
    assert_eq!(bare.app(), None, "`BLASTIT_APP` has to name the callable");
    assert!(bare.preload().is_empty());
}

#[test]
fn every_entry_point_returns_the_same_warnings() {
    // The group is attached to nothing: it grants nothing.
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.lonely.db]\nqueries = ['SELECT 1']\n\
                  [[route]]\nname = \"r\"\npath = ['/']\n\
                  [route.db]\nqueries = ['SELECT 2']\n";
    let expected = vec![ManifestWarning::UnattachedGroup {
        group: "lonely".to_string(),
    }];
    assert_eq!(validate_warnings(source), expected);

    let manifest = Manifest::from_toml_str(source).expect("parses");
    assert_eq!(
        manifest
            .validate(UidLayout::DEFAULT_BASE)
            .expect("validates"),
        expected
    );
    assert_eq!(manifest.validate_default().expect("validates"), expected);

    let (parsed, warnings) =
        Manifest::parse_and_validate(source, UidLayout::DEFAULT_BASE).expect("validates");
    assert_eq!(warnings, expected);
    assert_eq!(parsed.route_count(), 1);
}

//
// Refusals: the location, the name and the fix, on one line.
//

#[test]
fn a_grant_on_an_unknown_proxy_names_where_it_was_written() {
    let route = "version = 1\n\
                 [proxy.db]\nprotocol = \"postgresql\"\n\
                 [[route]]\nname = \"orders\"\npath = ['/']\n\
                 [route.warehouse]\nqueries = ['SELECT 1']\n";
    let errors = validate_errors(route, UidLayout::DEFAULT_BASE);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors.errors()[0],
        ManifestError::GrantOnUnknownProxy { .. }
    ));
    assert_eq!(
        errors.errors()[0].to_string(),
        "route \"orders\" (index 0): grant on unknown proxy \"warehouse\""
    );

    let group = "version = 1\n\
                 [proxy.db]\nprotocol = \"postgresql\"\n\
                 [group.catalog.warehouse]\nqueries = ['SELECT 1']\n\
                 [[route]]\nname = \"r\"\npath = ['/']\n\
                 [route.db]\nqueries = ['SELECT 2']\n";
    let errors = validate_errors(group, UidLayout::DEFAULT_BASE);
    assert_eq!(
        errors.errors()[0].to_string(),
        "group \"catalog\": grant on unknown proxy \"warehouse\""
    );

    // The global list has its own spelling: it is not a group and not a route.
    assert_eq!(
        ManifestError::UnknownGroup {
            location: Location::Global,
            group: "ghost".to_string(),
        }
        .to_string(),
        "global groups: unknown group \"ghost\""
    );
}

#[test]
fn a_message_escapes_control_characters_and_cuts_long_values() {
    let bell = ManifestError::InvalidApp {
        app: "shop\u{7}wsgi".to_string(),
    };
    assert_eq!(
        bell.to_string(),
        "app \"shop\\u{7}wsgi\": must be `module:attribute`",
        "a control character never reaches the event stream raw"
    );
    assert!(!bell.to_string().chars().any(char::is_control));

    // Up to 120 characters are quoted whole...
    let short = ManifestError::InvalidApp {
        app: "a".repeat(120),
    };
    let rendered = short.to_string();
    assert!(rendered.starts_with(&format!("app \"{}\": ", "a".repeat(120))));
    assert!(!rendered.contains('…'));

    // ...and the 121st is replaced by an ellipsis, so the line stays readable.
    let long = ManifestError::InvalidApp {
        app: "a".repeat(121),
    };
    let rendered = long.to_string();
    assert!(rendered.starts_with(&format!("app \"{}\u{2026}\": ", "a".repeat(120))));
    assert!(rendered.ends_with(": must be `module:attribute`"));

    // The cut counts characters, not bytes: a name of two-byte characters is
    // never split mid-character.
    let accented = ManifestError::InvalidApp {
        app: "é".repeat(130),
    };
    assert!(
        accented
            .to_string()
            .contains(&format!("\"{}\u{2026}\"", "é".repeat(120)))
    );
}

#[test]
fn an_invalid_regex_reports_the_engines_last_line() {
    let source = "version = 1\n[[route]]\nname = \"r\"\npath = ['[']\n";
    let errors = validate_errors(source, UidLayout::DEFAULT_BASE);
    let message = errors.errors()[0].to_string();
    assert_eq!(
        message, "route \"r\" rule 0: invalid regex \"[\": error: unclosed character class",
        "the regex engine's excerpt is dropped; only the problem is kept"
    );
    assert!(!message.contains('\n'), "one error, one line");
}

//
// The collected-error value itself.
//

#[test]
fn the_error_list_counts_iterates_and_renders() {
    let one: ManifestErrors = ManifestError::MissingVersion.into();
    assert_eq!(one.len(), 1);
    assert!(!one.is_empty());
    assert_eq!(one.errors().len(), 1);
    assert_eq!(
        one.to_string(),
        "manifest has 1 error(s)\n  - version: missing `version = 1`"
    );

    let two = ManifestErrors::from(vec![
        ManifestError::MissingVersion,
        ManifestError::MissingKey {
            path: "route[0].name".to_string(),
        },
    ]);
    assert_eq!(
        two.len(),
        2,
        "both problems are collected, not just the first"
    );
    assert!(!two.is_empty());
    assert_eq!(two.errors().len(), 2);
    let rendered = two.to_string();
    assert!(
        rendered.starts_with("manifest has 2 error(s)"),
        "{rendered}"
    );
    assert!(
        rendered.contains("\n  - route[0].name: missing required key"),
        "{rendered}"
    );

    // Iterating by reference, then by value.
    let mut seen = 0;
    for error in &two {
        assert!(!error.to_string().is_empty());
        seen += 1;
    }
    assert_eq!(seen, 2);
    let mut owned = Vec::new();
    for error in two.clone() {
        owned.push(error);
    }
    assert_eq!(owned, two.errors());

    assert_eq!(two.clone().into_errors(), owned);

    let none = ManifestErrors::new(Vec::new());
    assert!(none.is_empty());
    assert_eq!(none.len(), 0);
    assert_eq!(none.to_string(), "manifest has 0 error(s)");
    assert!(none.into_errors().is_empty());
}

#[test]
fn a_parse_error_is_reported_as_a_list_of_one() {
    let errors = parse_errors("version = 1\nunknown = true\n");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].to_string(), "unknown: unknown key");
}

//
// `is_query_covered_by_tables_or_groups`: which contributor may cover a
// `queries` entry, and which may not.
//

/// One route per contributor: its own table grants, its own
/// `transaction_control`, its own `unrestricted`, and a group's queries,
/// `transaction_control` and `unrestricted`.
const CONTRIBUTORS: &str = "version = 1\n\
    [proxy.db]\nprotocol = \"postgresql\"\n\
    [group.control.db]\ntransaction_control = true\n\
    [group.open.db]\nunrestricted = true\n\
    [group.listed.db]\nqueries = ['SELECT 1']\n\
    [[route]]\nname = \"tables\"\npath = ['/tables/']\n\
    [route.db]\nunrestricted = false\n\
    [route.db.tables]\nt = { select = [\"a\", \"b\"] }\n\
    [[route]]\nname = \"control\"\npath = ['/control/']\n\
    [route.db]\ntransaction_control = true\n\
    [[route]]\nname = \"any\"\npath = ['/any/']\n\
    [route.db]\nunrestricted = true\n\
    [[route]]\nname = \"grouped\"\npath = ['/grouped/']\ngroups = [\"control\", \"open\"]\n\
    [[route]]\nname = \"listed\"\npath = ['/listed/']\ngroups = [\"listed\"]\n\
    [[route]]\nname = \"controlled\"\npath = ['/controlled/']\ngroups = [\"control\"]\n";

#[test]
fn only_tables_and_groups_cover_a_query() {
    let manifest = Manifest::from_toml_str(CONTRIBUTORS).expect("parses");
    let db = ProxyName::from("db");

    // The route's own table grants cover a statement the analysis can parse,
    // including one whose literal holds a doubled quote and a `$?`.
    assert!(manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "t"."a" FROM "t" WHERE "t"."b" = 'it''s $?'"#,
    ));
    assert!(!manifest.is_query_covered_by_tables_or_groups(0, &db, r#"SELECT "u"."a" FROM "u""#,));

    // `transaction_control = true` inline covers BEGIN/COMMIT, and nothing else.
    assert!(manifest.is_query_covered_by_tables_or_groups(1, &db, "BEGIN"));
    assert!(!manifest.is_query_covered_by_tables_or_groups(1, &db, "SELECT 1"));

    // `unrestricted = true` inline covers everything.
    assert!(manifest.is_query_covered_by_tables_or_groups(2, &db, "DROP TABLE everything"));

    // So does a group's, and a group's transaction control.
    assert!(manifest.is_query_covered_by_tables_or_groups(3, &db, "COMMIT"));
    assert!(manifest.is_query_covered_by_tables_or_groups(3, &db, "DROP TABLE everything"));

    // A group's exact query covers it.
    assert!(manifest.is_query_covered_by_tables_or_groups(4, &db, "SELECT 1"));

    // A group's transaction control covers COMMIT even when no group grants
    // anything else.
    assert!(manifest.is_query_covered_by_tables_or_groups(5, &db, "COMMIT"));
    assert!(!manifest.is_query_covered_by_tables_or_groups(5, &db, "SELECT 1"));

    // Neither a proxy no grant names nor an index no route has covers anything.
    assert!(!manifest.is_query_covered_by_tables_or_groups(9, &db, "SELECT 1"));
    assert!(!manifest.is_query_covered_by_tables_or_groups(
        4,
        &ProxyName::from("warehouse"),
        "SELECT 1"
    ));
}
