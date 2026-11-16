//! Standard-tier coverage for the manifest model's accessors and for the
//! one-line refusal messages `blastit` prints.
//!
//! The `critical_*` suites prove the promises (a manifest boots, an edit is
//! surgical, a CVE shape is refused). This file walks the model accessors, the
//! flags the lowering derives and the error rendering, so each one's real
//! behaviour is pinned by a test instead of being assumed from the code.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::borrow::Borrow;
use std::collections::BTreeMap;

use blastit_manifest::{
    BodyCodec, Columns, Grant, Location, Manifest, ManifestError, ManifestErrors, ManifestWarning,
    PgGrant, Position, Protocol, ProxyName, Route, TableGrant, TableGrants, UidLayout,
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
// Names, protocols and the raw store declaration.
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
fn a_store_keeps_its_input_exactly_as_written() {
    let manifest = section_5_1();
    assert_eq!(manifest.proxies().len(), 2);

    let db = manifest.proxy("db").expect("store db");
    assert_eq!(db.name(), &ProxyName::from("db"));
    assert_eq!(db.protocol(), Protocol::Postgresql);
    assert_eq!(text_map(db.input()), [("url", "DATABASE_URL")]);

    let analytics = manifest.proxy("analytics").expect("store analytics");
    assert_eq!(text_map(analytics.input()), [("url", "ANALYTICS_URL")]);

    // §7.5: the retired format could rename the variables the app receives, and
    // this language cannot. Lowering writes the empty map, which is the
    // protocol's default templates — the same behaviour as a store that
    // declared no `output` at all. `ADR-0012` records the drop.
    assert!(db.output().is_empty(), "an `output` map is not writable");
    assert!(analytics.output().is_empty(), "nor for a named store");

    assert!(manifest.proxy("warehouse").is_none());
}

#[test]
fn the_block_form_names_every_slot_variable() {
    // §7.4: a slot's value is the variable's *name*, never its value, and the
    // seven slots are the set `blastit-env` documents.
    let source = "postgres {\n\
                  \x20   host $DB_HOST\n\
                  \x20   port $DB_PORT\n\
                  \x20   user $DB_USERNAME\n\
                  \x20   password $DB_PASSWORD\n\
                  \x20   database $DB_DATABASE\n\
                  \x20   sslmode $DB_SSLMODE\n\
                  \x20   ca_cert $DB_CA_CERT\n\
                  }\n\
                  \n\
                  view v \"/v\" {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("parses");
    let store = manifest.proxy("db").expect("the unnamed store is db");
    assert_eq!(
        text_map(store.input()),
        [
            ("ca_cert", "DB_CA_CERT"),
            ("database", "DB_DATABASE"),
            ("host", "DB_HOST"),
            ("password", "DB_PASSWORD"),
            ("port", "DB_PORT"),
            ("sslmode", "DB_SSLMODE"),
            ("user", "DB_USERNAME"),
        ]
    );
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
// The lowered document: views, traits and the top-level keys.
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
    assert_eq!(texts(product.groups()), ["session", "catalog"]);
    assert!(!product.egress());
    assert!(product.prefork());
    assert!(product.grant(&ProxyName::from("db")).is_none());
    assert!(product.ingress().is_none(), "the view declared none");

    let checkout = manifest.route(1).expect("route 1");
    assert_eq!(checkout.name(), "checkout");
    assert_eq!(checkout.methods(), None, "no method means any method");
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
        .expect("the view's inline grant");
    assert!(!inline.is_empty());
    assert!(checkout.grant(&ProxyName::from("warehouse")).is_none());

    let fallback = manifest.route(3).expect("route 3");
    assert_eq!(texts(fallback.path()), [".*"]);
    assert!(fallback.grants().is_empty());
    assert!(!fallback.egress());
    assert!(!fallback.prefork());
    assert!(fallback.groups().is_empty(), "no `use`, no trait");

    assert!(manifest.route(4).is_none());

    // §7.3: a view's sandbox is its own index, and both endpoints of one view
    // share it — here every view has one.
    for (index, route) in manifest.routes().iter().enumerate() {
        assert_eq!(route.sandbox(), index as u32, "{}", route.name());
    }
}

#[test]
fn a_use_attaches_the_trait_and_no_global_list_exists() {
    let manifest = section_6();
    assert!(
        manifest.global_groups().is_empty(),
        "no document can attach a trait to every view (§7.1)"
    );
    // The written `use` is what reaches the route, in written order.
    assert_eq!(
        manifest
            .route(0)
            .expect("route 0")
            .groups()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["session", "catalog"]
    );
}

#[test]
fn a_route_keeps_the_ingress_policy_it_declared() {
    let source = "view upload \"/upload/\" {\n    body json\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("parses");
    let ingress = manifest
        .route(0)
        .expect("route 0")
        .ingress()
        .expect("the view declared one");
    assert_eq!(ingress.body, BodyCodec::Json);
    assert!(!ingress.allow_raw);
}

#[test]
fn a_group_reports_its_name_and_grants() {
    let manifest = section_6();
    assert_eq!(manifest.groups().len(), 2);

    let session = manifest.group("session").expect("trait session");
    assert_eq!(session.name(), "session");
    assert_eq!(session.grants().len(), 1);
    assert!(session.grant(&ProxyName::from("db")).is_some());
    assert!(session.grant(&ProxyName::from("warehouse")).is_none());

    assert!(manifest.group("ghost").is_none());
}

#[test]
fn the_top_level_keys_are_the_document() {
    let manifest = Manifest::from_blastfile_str(
        "app \"shop\"\n\
         \n\
         entrypoint python \"shop.wsgi:application\" {\n    preload \"b\", \"a\"\n}\n",
    )
    .expect("parses");
    assert_eq!(manifest.version(), 1);
    assert_eq!(manifest.name(), Some("shop"));
    assert_eq!(manifest.app(), Some("shop.wsgi:application"));
    assert_eq!(texts(manifest.preload()), ["b", "a"]);
    assert_eq!(manifest.route_count(), 0);
    assert!(manifest.route(0).is_none());
    assert!(manifest.routes().is_empty());
    assert!(manifest.proxies().is_empty());
    assert!(manifest.groups().is_empty());
    assert!(manifest.global_groups().is_empty());
    assert!(manifest.mounts().is_empty());
    assert!(manifest.php().is_none());

    let bare = Manifest::from_blastfile_str("").expect("parses");
    assert_eq!(bare.app(), None, "`BLASTIT_APP` has to name the callable");
    assert_eq!(bare.name(), None);
    assert!(bare.preload().is_empty());
}

#[test]
fn every_entry_point_returns_the_same_warnings() {
    // The trait is used by no view: it grants nothing.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait lonely {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  view r \"/\" {\n    postgres {\n        query 'SELECT 2'\n    }\n}\n";
    let expected = vec![ManifestWarning::UnattachedGroup {
        group: "lonely".to_string(),
    }];
    assert_eq!(validate_warnings(source), expected);

    let manifest = Manifest::from_blastfile_str(source).expect("parses");
    assert_eq!(
        manifest
            .validate(UidLayout::DEFAULT_BASE)
            .expect("validates"),
        expected
    );
    assert_eq!(manifest.validate_default().expect("validates"), expected);

    let (parsed, warnings) =
        Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE).expect("validates");
    assert_eq!(warnings, expected);
    assert_eq!(parsed.route_count(), 1);
}

//
// Refusals: the location, the name and the fix, on one line.
//

#[test]
fn a_grant_on_an_unknown_proxy_names_where_it_was_written() {
    let route = "postgres $DATABASE_URL\n\
                 \n\
                 view orders \"/\" {\n    postgres warehouse {\n        query 'SELECT 1'\n    }\n}\n";
    let errors = validate_errors(route, UidLayout::DEFAULT_BASE);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors.errors()[0],
        ManifestError::GrantOnUnknownProxy { .. }
    ));
    assert_eq!(
        errors.errors()[0].to_string(),
        "view \"orders\": grant on unknown proxy \"warehouse\"",
        "§18: a view is named, not numbered"
    );

    let trait_body = "postgres $DATABASE_URL\n\
                      \n\
                      trait catalog {\n    postgres warehouse {\n        query 'SELECT 1'\n    }\n}\n\
                      \n\
                      view r \"/\" {\n    postgres {\n        query 'SELECT 2'\n    }\n}\n";
    let errors = validate_errors(trait_body, UidLayout::DEFAULT_BASE);
    assert_eq!(
        errors.errors()[0].to_string(),
        "trait \"catalog\": grant on unknown proxy \"warehouse\""
    );

    // The global list has its own spelling: it is not a trait and not a view.
    // No document can fill it (§7.1), so the rendering is asserted directly.
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
    // A value the sentence is allowed to quote: the codec a view named.
    // (`InvalidApp` no longer quotes its value — §18, `a_refusal_never_carries`
    // below — so the escaping and cutting rule is pinned on a variant that does.)
    let bell = ManifestError::UnknownBodyCodec {
        path: "view upload: body".to_string(),
        found: "shop\u{7}wsgi".to_string(),
    };
    assert_eq!(
        bell.to_string(),
        "view upload: body: expected \"json\", \"form\", \"none\" or \"raw\", found \"shop\\u{7}wsgi\"",
        "a control character never reaches the event stream raw"
    );
    assert!(!bell.to_string().chars().any(char::is_control));

    // Up to 120 characters are quoted whole...
    let short = ManifestError::UnknownBodyCodec {
        path: "view upload: body".to_string(),
        found: "a".repeat(120),
    };
    let rendered = short.to_string();
    assert!(rendered.ends_with(&format!("found \"{}\"", "a".repeat(120))));
    assert!(!rendered.contains('…'));

    // ...and the 121st is replaced by an ellipsis, so the line stays readable.
    let long = ManifestError::UnknownBodyCodec {
        path: "view upload: body".to_string(),
        found: "a".repeat(121),
    };
    let rendered = long.to_string();
    assert!(rendered.contains(&format!("\"{}\u{2026}\"", "a".repeat(120))));

    // The cut counts characters, not bytes: a name of two-byte characters is
    // never split mid-character.
    let accented = ManifestError::UnknownBodyCodec {
        path: "view upload: body".to_string(),
        found: "é".repeat(130),
    };
    assert!(
        accented
            .to_string()
            .contains(&format!("\"{}\u{2026}\"", "é".repeat(120)))
    );
}

#[test]
fn a_refusal_never_carries_the_app_it_refused() {
    // §18: a pasted token can land in the `app` slot, and the sentence names the
    // shape instead of quoting what is there — so nothing a log or CI run prints
    // can carry it. (`PhpEntryInvalid`, the other echoing variant §18 names,
    // belongs to `critical_php_manifest.rs`, outside this leaf's paths.)
    for value in ["$BLASTIT_TOKEN", "ghp_s3cr3tToken"] {
        let source = format!("entrypoint python \"{value}\"\n\nview home \"/\" {{}}\n");
        let errors = Manifest::parse_blastfile(&source, 20_000).expect_err("refused");
        assert_eq!(
            errors.errors(),
            [ManifestError::InvalidApp {
                app: value.to_string(),
            }],
            "{errors}"
        );
        let rendered = errors.errors()[0].to_string();
        assert_eq!(rendered, "app: must be `module:attribute`", "{rendered}");
        let joined = errors.to_string();
        assert!(!joined.contains(value), "the value is echoed: {joined}");
        for fragment in ["$BLASTIT", "ghp_"] {
            assert!(
                !joined.contains(fragment),
                "`{fragment}` reached the message: {joined}"
            );
        }
    }
    // The allowed twin: one colon is the whole rule.
    assert!(
        Manifest::parse_blastfile(
            "entrypoint python \"shop.wsgi:application\"\n\nview home \"/\" {}\n",
            20_000
        )
        .is_ok()
    );
}

#[test]
fn an_invalid_regex_reports_the_engines_last_line() {
    // A document's `regex '…'` is compiled by the parser (§8.1), so the
    // validator only sees the regex of a path an editor's model carries — which
    // is where this rendering is asserted.
    let route = Route::new(
        "r".to_string(),
        vec!["[".to_string()],
        None,
        Vec::new(),
        false,
        false,
        BTreeMap::new(),
    );
    let errors = Manifest::from_parts(1, Vec::new(), BTreeMap::new(), BTreeMap::new(), vec![route])
        .validate(UidLayout::DEFAULT_BASE)
        .expect_err("the regex does not compile");
    let message = errors.errors()[0].to_string();
    assert_eq!(
        message, "route \"r\" rule 0: invalid regex \"[\": error: unclosed character class",
        "the regex engine's excerpt is dropped; only the problem is kept"
    );
    assert!(!message.contains('\n'), "one error, one line");

    // The document's spelling of the same mistake is refused at the quote.
    let errors = parse_errors("view r regex '[' {\n}\n");
    assert!(
        errors
            .to_string()
            .starts_with("1:14: regex `[` does not compile:"),
        "{errors}"
    );
}

//
// `R8`: where a reserved word is a name and where it is a directive.
//
// The retired format answered this by walking tables, which no longer exist. The
// language answers it by position: a grant clause's argument is always a name,
// a store's name may be any word but the one that starts the grant form, and a
// directive is a directive only where the grammar has one.
//

#[test]
fn a_reserved_word_is_a_name_inside_a_grant() {
    // `select groups, app` names two tables the author chose: a table name is a
    // `Word`, and no keyword is consulted for it (§13.1).
    let named = "postgres $DATABASE_URL\n\
                 \n\
                 view v \"/v\" {\n    postgres {\n        select groups, app\n    }\n}\n";
    let manifest = Manifest::from_blastfile_str(named).expect("a table named after a keyword");
    let grant = manifest
        .route(0)
        .expect("route 0")
        .grant(&ProxyName::from("db"))
        .and_then(Grant::as_postgresql)
        .expect("the inline grant on the declared store");
    assert!(
        grant.tables().table("groups").is_some(),
        "the table named `groups` was dropped"
    );
    assert!(
        grant.tables().table("app").is_some(),
        "the table named `app` was dropped"
    );

    // A *store* named `app` is a store too, and a grant on it is a grant.
    let store = "postgres app $DATABASE_URL\n\
                 \n\
                 view v \"/v\" {\n    postgres app {\n        select t\n    }\n}\n";
    let manifest = Manifest::from_blastfile_str(store).expect("a store named app");
    assert!(
        manifest
            .route(0)
            .expect("route 0")
            .grant(&ProxyName::from("app"))
            .is_some(),
        "a declared store makes `app` a grant"
    );
}

#[test]
fn a_keyword_is_only_a_directive_where_the_grammar_has_one() {
    // `tables` is the retired format's key and no directive at all: a grant
    // block's clauses are the seven §13.1 names, and the refusal lists them.
    let errors = parse_errors(
        "postgres $DATABASE_URL\n\nview v \"/v\" {\n    postgres {\n        tables t\n    }\n}\n",
    );
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::NotADirectiveHere { word, block, .. } if word == "tables" && *block == "a grant block"
        ),
        "{errors}"
    );
    assert!(
        errors
            .to_string()
            .contains("`transactions`, `functions`, `select`"),
        "{errors}"
    );

    // `unrestricted` is reserved in the store slot: `postgres unrestricted` is
    // the grant form, so a store cannot be named it (§7.4).
    let errors = parse_errors("postgres unrestricted $DATABASE_URL\n");
    assert!(
        matches!(&errors.errors()[0], ManifestError::StoreNameReserved { .. }),
        "{errors}"
    );

    // The allowed twin: every one of the seven clauses, in one grant.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  view v \"/v\" {\n    postgres {\n        transactions\n        functions unaccent\n        select t(a)\n        insert t\n        update t(b)\n        delete t\n        query 'SELECT 1'\n    }\n}\n";
    assert!(Manifest::from_blastfile_str(source).is_ok());
}

//
// The collected-error value itself.
//

#[test]
fn the_error_list_counts_iterates_and_renders() {
    let unknown = ManifestError::UnknownKeyword {
        at: Position::at(2, 1),
        word: "not_a_directive".to_string(),
    };
    let one: ManifestErrors = unknown.clone().into();
    assert_eq!(one.len(), 1);
    assert!(!one.is_empty());
    assert_eq!(one.errors().len(), 1);
    assert_eq!(
        one.to_string(),
        "2:1: `not_a_directive` is not a keyword; the sections are `app`, `entrypoint`, \
         `postgres`, `trait`, `view`, `static`",
        "§18: one line per problem, no header and no `  - ` prefix"
    );

    let named = ManifestError::InvalidName {
        at: Position::at(1, 6),
        name: "_leading".to_string(),
    };
    let two = ManifestErrors::from(vec![unknown, named]);
    assert_eq!(
        two.len(),
        2,
        "both problems are collected, not just the first"
    );
    assert!(!two.is_empty());
    assert_eq!(two.errors().len(), 2);
    let rendered = two.to_string();
    assert!(
        !rendered.contains("manifest has "),
        "no header (§18): {rendered}"
    );
    assert_eq!(rendered.lines().count(), 2, "{rendered}");
    assert_eq!(
        rendered,
        "2:1: `not_a_directive` is not a keyword; the sections are `app`, `entrypoint`, \
         `postgres`, `trait`, `view`, `static`\n\
         1:6: `_leading` is not a name; a view, trait or store name is `[a-z0-9][a-z0-9_-]*`",
        "{rendered}"
    );
    assert!(!rendered.ends_with('\n'), "{rendered:?}");

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
    assert_eq!(none.to_string(), "", "no problems, no lines (§18)");
    assert!(none.into_errors().is_empty());
}

#[test]
fn a_parse_error_is_reported_as_a_list_of_one() {
    let errors = parse_errors("app \"shop\"\nnot_a_directive\n");
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors.errors()[0].to_string(),
        "2:1: `not_a_directive` is not a keyword; the sections are `app`, `entrypoint`, \
         `postgres`, `trait`, `view`, `static`"
    );
}

//
// `is_query_covered_by_tables_or_groups`: which contributor may cover a
// `queries` entry, and which may not.
//

/// One view per contributor: its own table grants, its own `transactions`, its
/// own `unrestricted`, and a trait's `transactions`, `unrestricted` and exact
/// queries.
const CONTRIBUTORS: &str = "postgres $DATABASE_URL\n\
    trait control {\n    postgres {\n        transactions\n    }\n}\n\
    trait open {\n    postgres unrestricted\n}\n\
    trait listed {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\
    view tables \"/tables/\" {\n    postgres {\n        select t(a, b)\n    }\n}\n\
    view control \"/control/\" {\n    postgres {\n        transactions\n    }\n}\n\
    view any \"/any/\" {\n    postgres unrestricted\n}\n\
    view grouped \"/grouped/\" {\n    use control, open\n}\n\
    view listed \"/listed/\" {\n    use listed\n}\n\
    view controlled \"/controlled/\" {\n    use control\n}\n";

#[test]
fn only_tables_and_traits_cover_a_query() {
    let manifest = Manifest::from_blastfile_str(CONTRIBUTORS).expect("parses");
    let db = ProxyName::from("db");

    // The view's own table grants cover a statement the analysis can parse,
    // including one whose literal holds a doubled quote and a `$?`.
    assert!(manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "t"."a" FROM "t" WHERE "t"."b" = 'it''s $?'"#,
    ));
    assert!(!manifest.is_query_covered_by_tables_or_groups(0, &db, r#"SELECT "u"."a" FROM "u""#,));

    // `transactions` inline covers BEGIN/COMMIT, and nothing else.
    assert!(manifest.is_query_covered_by_tables_or_groups(1, &db, "BEGIN"));
    assert!(!manifest.is_query_covered_by_tables_or_groups(1, &db, "SELECT 1"));

    // `unrestricted` inline covers everything.
    assert!(manifest.is_query_covered_by_tables_or_groups(2, &db, "DROP TABLE everything"));

    // So does a trait's, and a trait's transaction control.
    assert!(manifest.is_query_covered_by_tables_or_groups(3, &db, "COMMIT"));
    assert!(manifest.is_query_covered_by_tables_or_groups(3, &db, "DROP TABLE everything"));

    // A trait's exact query covers it.
    assert!(manifest.is_query_covered_by_tables_or_groups(4, &db, "SELECT 1"));

    // A trait's transaction control covers COMMIT even when no trait grants
    // anything else.
    assert!(manifest.is_query_covered_by_tables_or_groups(5, &db, "COMMIT"));
    assert!(!manifest.is_query_covered_by_tables_or_groups(5, &db, "SELECT 1"));

    // Neither a store no grant names nor an index no view has covers anything.
    assert!(!manifest.is_query_covered_by_tables_or_groups(9, &db, "SELECT 1"));
    assert!(!manifest.is_query_covered_by_tables_or_groups(
        4,
        &ProxyName::from("warehouse"),
        "SELECT 1"
    ));
}
