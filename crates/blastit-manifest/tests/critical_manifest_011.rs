//! Protected critical suite for the `blastit.toml` 0.1.1 schema (`R7`, `R8`,
//! `R11`, `R24`).
//!
//! Every pair here has the two halves a policy test needs: the documented
//! spelling that must parse and validate, and the mistake that must be refused
//! with the message the documentation promises. The negative half is the point
//! — a manifest that inlines a value, or a top-level key TOML silently attached
//! to a table, has to fail the boot rather than reach an allowlist.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{
    BodyCodec, IngressPolicy, Location, Manifest, ManifestError, ManifestErrors, ManifestWarning,
    ProxyName, UidLayout,
};

const BASE: u32 = UidLayout::DEFAULT_BASE;

/// The documented message of `R8`, for one key.
fn not_top_level(key: &str) -> String {
    format!("{key} after a [table] is not top-level; move it above the first table")
}

fn parse(source: &str) -> Manifest {
    Manifest::from_toml_str(source).expect("manifest should parse")
}

fn parse_errors(source: &str) -> ManifestErrors {
    Manifest::from_toml_str(source).expect_err("manifest should not parse")
}

fn validate_errors(source: &str) -> ManifestErrors {
    Manifest::parse_and_validate(source, BASE).expect_err("manifest should not validate")
}

fn warnings(source: &str) -> Vec<ManifestWarning> {
    Manifest::parse_and_validate(source, BASE)
        .expect("manifest should validate")
        .1
}

/// One route whose `[route.ingress]` is exactly `ingress`.
fn route_with_ingress(ingress: &str) -> String {
    format!(
        "version = 1\n\
         [[route]]\nname = \"checkout\"\npath = ['/checkout/']\n\
         [route.ingress]\n{ingress}"
    )
}

/// One route with one `postgresql` grant whose `queries` list is `queries`.
fn route_with_queries(queries: &str) -> String {
    format!(
        "version = 1\n\
         [proxy.db]\nprotocol = \"postgresql\"\n\
         [[route]]\nname = \"notes\"\npath = ['/notes/']\n\
         [route.db]\n{queries}"
    )
}

/// The location and proxy every query test is written against.
fn route_grant() -> (Location, ProxyName) {
    (
        Location::Route {
            index: 0,
            name: "notes".to_string(),
        },
        ProxyName::from("db"),
    )
}

//
// R7: top-level `app` and `preload`.
//

#[test]
fn app_and_preload_parse_in_both_spellings() {
    let manifest =
        parse("version = 1\napp = \"myshop.wsgi:application\"\npreload = \"myshop.urls\"\n");
    assert_eq!(manifest.app(), Some("myshop.wsgi:application"));
    assert_eq!(manifest.preload(), ["myshop.urls"]);
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);

    // An array keeps its order; a module named twice is imported once, so the
    // duplicate keeps its first position.
    let manifest = parse("version = 1\npreload = [\"b\", \"a\", \"b\"]\n");
    assert_eq!(
        manifest.app(),
        None,
        "app is optional; BLASTIT_APP may supply it"
    );
    assert_eq!(manifest.preload(), ["b", "a"]);
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);
}

#[test]
fn app_must_be_module_attribute() {
    // Django's dotted path looks right and is the documented trap.
    let errors = validate_errors("version = 1\napp = \"myshop.wsgi.application\"\n");
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::InvalidApp {
            app: "myshop.wsgi.application".to_string(),
        })
    );
    assert!(
        errors.to_string().contains("must be `module:attribute`"),
        "{errors}"
    );

    // The allowed twin: a colon.
    assert!(
        Manifest::parse_and_validate("version = 1\napp = \"app:app\"\n", BASE).is_ok(),
        "a module:attribute app validates"
    );

    // An empty app names nothing, so it is a parse error and never a silent
    // fallback to BLASTIT_APP.
    assert_eq!(
        parse_errors("version = 1\napp = \"\"\n"),
        ManifestErrors::from(ManifestError::Empty {
            path: "app".to_string(),
        })
    );
}

//
// R8: reserved top-level keys inside a table.
//

#[test]
fn a_reserved_key_after_a_table_names_the_key_and_the_fix() {
    // Each source writes a top-level key below the first `[table]`, which TOML
    // attaches to that table instead of refusing.
    let cases = [
        (
            "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\nversion = 1\n",
            "version",
        ),
        (
            "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\napp = \"app:app\"\n",
            "app",
        ),
        (
            "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\npreload = [\"mod\"]\n",
            "preload",
        ),
        (
            "version = 1\n[[route]]\nname = \"about\"\npath = ['/about/']\n\
             [route.ingress]\nbody = \"none\"\ngroups = [\"session\"]\n",
            "groups",
        ),
        (
            "version = 1\n[[route]]\nname = \"about\"\npath = ['/about/']\nversion = 1\n",
            "version",
        ),
    ];

    for (source, key) in cases {
        let errors = parse_errors(source);
        assert_eq!(
            errors.errors(),
            [ManifestError::NotTopLevel {
                key: key.to_string()
            }],
            "for {key:?} in {source:?}"
        );
        assert_eq!(errors.errors()[0].to_string(), not_top_level(key));
    }
}

#[test]
fn groups_stays_top_level_at_the_top_and_inside_a_route() {
    // The allowed twins: the same key in the two places it belongs.
    let source = "version = 1\n\
                  groups = [\"session\"]\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.session.db]\nqueries = []\n\
                  [[route]]\nname = \"account\"\npath = ['/account/']\ngroups = [\"session\"]\n\
                  [route.db]\nqueries = []\n";
    let manifest = parse(source);
    assert_eq!(manifest.global_groups(), ["session"]);
    assert_eq!(manifest.route(0).expect("route").groups(), ["session"]);
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);
}

#[test]
fn author_chosen_names_may_repeat_a_top_level_key() {
    // Every name below belongs to the author: a proxy name, a group name, an
    // output variable and a table. None of them is a misplaced top-level key,
    // so all of them must boot.
    let source = r#"version = 1
app = "myshop.wsgi:application"

[proxy.app]
protocol = "postgresql"
input = { url = "APP_DB_URL" }

[proxy.db]
protocol = "postgresql"
input = { url = "DB_URL" }
[proxy.db.output]
app = "{{ url }}"

[proxy.groups]
protocol = "postgresql"

[proxy.version]
protocol = "postgresql"

[group.app.db]
queries = ['SELECT "orders"."id" FROM "orders"']

[group.app.groups]
queries = ['SELECT "sites"."id" FROM "sites"']

[group.app.version]
queries = ['SELECT "audit"."id" FROM "audit"']

[[route]]
name = "catalog"
path = ["/products/"]
groups = ["app"]
[route.app]
queries = ['SELECT "notes"."id" FROM "notes"']
[route.db.tables]
groups = { select = ["id"] }
"#;
    let manifest = parse(source);
    assert_eq!(manifest.app(), Some("myshop.wsgi:application"));

    // `[proxy.app]` is a proxy named `app`, and `[proxy.db.output]` names one of
    // its variables `app`.
    let proxy = manifest.proxy("app").expect("a proxy may be named app");
    assert_eq!(
        proxy.input().get("url").map(String::as_str),
        Some("APP_DB_URL")
    );
    assert_eq!(
        manifest
            .proxy("db")
            .expect("proxy db")
            .output()
            .get("app")
            .map(String::as_str),
        Some("{{ url }}"),
        "an output variable may be named app"
    );
    assert!(manifest.proxy("groups").is_some());
    assert!(manifest.proxy("version").is_some());
    // The group named `app` is there, with a grant on every awkward name.
    let group = manifest.group("app").expect("a group may be named app");
    assert_eq!(
        group
            .grants()
            .keys()
            .map(ProxyName::as_str)
            .collect::<Vec<_>>(),
        ["db", "groups", "version"]
    );
    // `[route.app]` is an inline grant on the proxy named `app`.
    assert!(
        manifest
            .route(0)
            .expect("route")
            .grant(&ProxyName::from("app"))
            .is_some()
    );
    // Django's own `groups` table is a table like any other.
    let tables = manifest
        .route(0)
        .expect("route")
        .grant(&ProxyName::from("db"))
        .expect("grant on db")
        .as_postgresql()
        .expect("postgresql")
        .tables();
    assert!(
        tables.table("groups").is_some(),
        "a table may be named groups"
    );
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);

    // The denied twin: `app` is still a mistake where nothing is called that,
    // so the guard is narrowed to names, not removed.
    let errors = parse_errors(
        "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\n\
         [[route]]\nname = \"a\"\npath = ['/a/']\napp = \"app:app\"\n",
    );
    assert_eq!(
        errors.errors(),
        [ManifestError::NotTopLevel {
            key: "app".to_string()
        }],
        "a route key that names no declared proxy is a misplaced top-level key"
    );
    assert_eq!(errors.errors()[0].to_string(), not_top_level("app"));
}

#[test]
fn a_reserved_key_inside_a_grant_body_is_not_an_unknown_field() {
    // A grant body is reached through `[group.<name>.<proxy>]` and
    // `[route.<proxy>]` too. The same scoping mistake there has to come back
    // as `R8`'s message, not as the `UnknownKey` a grant field nobody declared
    // would produce: the reader wrote a top-level key after a table, and that
    // is the fix to name.
    let group = |body: &str| {
        format!(
            "version = 1\n\
             [proxy.db]\nprotocol = \"postgresql\"\n\
             [group.session.db]\n{body}"
        )
    };
    let route = |body: &str| {
        format!(
            "version = 1\n\
             [proxy.db]\nprotocol = \"postgresql\"\n\
             [[route]]\nname = \"notes\"\npath = ['/notes/']\n\
             [route.db]\n{body}"
        )
    };

    let cases = [
        (group("version = 1\n"), "version"),
        (group("app = \"a:b\"\n"), "app"),
        (group("preload = [\"m\"]\n"), "preload"),
        (route("version = 1\n"), "version"),
        (route("app = \"a:b\"\n"), "app"),
        (route("preload = [\"m\"]\n"), "preload"),
    ];

    for (source, key) in cases {
        let errors = parse_errors(&source);
        assert_eq!(
            errors.errors(),
            [ManifestError::NotTopLevel {
                key: key.to_string()
            }],
            "for {key:?} in {source:?} (not UnknownKey)"
        );
        assert_eq!(errors.errors()[0].to_string(), not_top_level(key));
    }

    // The allowed twin: the same two grant bodies with a field the schema does
    // declare boot, so the test above is not refusing every grant body.
    let allowed = "version = 1\n\
                   [proxy.db]\nprotocol = \"postgresql\"\n\
                   [group.session.db]\nqueries = []\n\
                   [[route]]\nname = \"notes\"\npath = ['/notes/']\ngroups = [\"session\"]\n\
                   [route.db]\nqueries = []\n";
    let (manifest, warnings) =
        Manifest::parse_and_validate(allowed, BASE).expect("the grant bodies boot");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        manifest
            .group("session")
            .expect("group")
            .grants()
            .keys()
            .map(ProxyName::as_str)
            .collect::<Vec<_>>(),
        ["db"]
    );
    assert!(
        manifest
            .route(0)
            .expect("route")
            .grant(&ProxyName::from("db"))
            .is_some()
    );
}

//
// R11: `[route.ingress]` and the two boot warnings.
//

#[test]
fn every_body_codec_parses() {
    for (spelling, codec) in [
        ("json", BodyCodec::Json),
        ("form", BodyCodec::Form),
        ("none", BodyCodec::None),
        ("raw", BodyCodec::Raw),
    ] {
        let manifest = parse(&route_with_ingress(&format!("body = \"{spelling}\"\n")));
        assert_eq!(
            manifest.route(0).expect("route").ingress(),
            Some(IngressPolicy {
                body: codec,
                allow_raw: false,
            }),
            "for body = {spelling:?}"
        );
        manifest.validate(BASE).expect("a declared body validates");
    }

    // `allow_raw` is meaningful only on `raw`, and defaults to false there.
    let manifest = parse(&route_with_ingress("body = \"raw\"\nallow_raw = true\n"));
    assert_eq!(
        manifest.route(0).expect("route").ingress(),
        Some(IngressPolicy {
            body: BodyCodec::Raw,
            allow_raw: true,
        })
    );

    // No `[route.ingress]` at all: fail-closed, `None`.
    let manifest = parse("version = 1\n[[route]]\nname = \"about\"\npath = ['/about/']\n");
    assert_eq!(manifest.route(0).expect("route").ingress(), None);
}

#[test]
fn allow_raw_is_an_unknown_key_on_every_other_codec() {
    for codec in ["json", "form", "none"] {
        assert_eq!(
            parse_errors(&route_with_ingress(&format!(
                "body = \"{codec}\"\nallow_raw = true\n"
            ))),
            ManifestErrors::from(ManifestError::UnknownKey {
                path: "route[0].ingress.allow_raw".to_string(),
            }),
            "allow_raw says nothing on {codec:?}, so it must not look like it does"
        );
    }
}

#[test]
fn an_ingress_table_needs_a_known_body() {
    assert_eq!(
        parse_errors(&route_with_ingress("allow_raw = true\n")),
        ManifestErrors::from(ManifestError::MissingKey {
            path: "route[0].ingress.body".to_string(),
        })
    );
    assert_eq!(
        parse_errors(&route_with_ingress("body = \"xml\"\n")),
        ManifestErrors::from(ManifestError::UnknownBodyCodec {
            path: "route[0].ingress.body".to_string(),
            found: "xml".to_string(),
        })
    );
    // 0.1.1 has no multipart codec: the name is refused, not silently widened.
    assert!(
        parse_errors(&route_with_ingress("body = \"multipart\"\n")).errors()[0]
            .to_string()
            .contains("\"json\", \"form\", \"none\" or \"raw\"")
    );
}

#[test]
fn raw_ingress_and_egress_are_boot_warnings() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [[route]]\nname = \"upload\"\npath = ['/upload/']\negress = true\n\
                  [route.db]\nqueries = []\n\
                  [route.ingress]\nbody = \"raw\"\nallow_raw = true\n";
    assert_eq!(
        warnings(source),
        vec![
            ManifestWarning::RawIngress {
                route: "upload".to_string()
            },
            ManifestWarning::EgressRoute {
                route: "upload".to_string()
            },
        ]
    );

    // The denied twin: no `raw` and no `egress` means neither warning, so the
    // test above is proving something rather than matching a constant list.
    let quiet = "version = 1\n\
                 [proxy.db]\nprotocol = \"postgresql\"\n\
                 [[route]]\nname = \"upload\"\npath = ['/upload/']\n\
                 [route.db]\nqueries = []\n\
                 [route.ingress]\nbody = \"json\"\n";
    assert_eq!(warnings(quiet), vec![]);
}

//
// R28 half: both catch-all spellings.
//

#[test]
fn both_catch_all_spellings_are_unrestricted() {
    // `path = ".*"` is a bare string; TOML hands the parser a list of one.
    for path in ["\".*\"", "['.*']", "['^.*$']"] {
        let source = format!("version = 1\n[[route]]\nname = \"fallback\"\npath = {path}\n");
        assert!(
            parse(&source)
                .route(0)
                .expect("route")
                .is_unrestricted_catch_all(),
            "for path = {path}"
        );
    }

    for path in ["['/products/.*']", "['/']", "[]"] {
        let source = format!("version = 1\n[[route]]\nname = \"fallback\"\npath = {path}\n");
        assert!(
            !parse(&source)
                .route(0)
                .expect("route")
                .is_unrestricted_catch_all(),
            "for path = {path}"
        );
    }

    // A catch-all restricted to some methods is not unrestricted: another
    // method still falls through to the route below it.
    let source = "version = 1\n\
                  [[route]]\nname = \"fallback\"\npath = ['.*']\nmethods = [\"GET\", \"HEAD\"]\n";
    assert!(
        !parse(source)
            .route(0)
            .expect("route")
            .is_unrestricted_catch_all()
    );
}

//
// R24: no value-bearing literal reaches an allowlist.
//

#[test]
fn the_fixed_protocol_statements_may_keep_their_literal() {
    for query in [
        "SET TIME ZONE 'UTC'",
        "SELECT set_config('TimeZone', $?, false)",
    ] {
        let source = route_with_queries(&format!("queries = [{query:?}]\n"));
        assert_eq!(
            Manifest::parse_and_validate(&source, BASE)
                .expect("known setup SQL is grantable")
                .0
                .route(0)
                .expect("route")
                .grant(&ProxyName::from("db"))
                .expect("grant")
                .as_postgresql()
                .expect("postgresql")
                .queries(),
            [query],
            "the two protocol constants are the only literal-bearing grants"
        );
    }
}

#[test]
fn the_wire_spelling_of_the_fixed_setup_statement_is_exempt() {
    // The wire carries `$1`; `--learn` writes the normalised `$?` form, but a
    // hand-written or older manifest may spell the placeholder. Validation
    // normalises only to compare, and the model keeps the author's text.
    let source = route_with_queries("queries = [\"SELECT set_config('TimeZone', $1, false)\"]\n");
    let (manifest, warnings) =
        Manifest::parse_and_validate(&source, BASE).expect("the $1 spelling is the same statement");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        manifest
            .route(0)
            .expect("route")
            .grant(&ProxyName::from("db"))
            .expect("grant")
            .as_postgresql()
            .expect("postgresql")
            .queries(),
        ["SELECT set_config('TimeZone', $1, false)"],
        "normalisation is for comparison and must not rewrite the model"
    );

    // The exemption is that exact statement, not every `set_config` call: a
    // different literal is still a value-bearing grant.
    let sibling =
        route_with_queries("queries = [\"SELECT set_config('application_name', $1, false)\"]\n");
    let (location, proxy) = route_grant();
    assert_eq!(
        validate_errors(&sibling),
        ManifestErrors::from(ManifestError::LiteralQuery {
            location,
            proxy,
            rule: 0,
        })
    );
}

#[test]
fn a_redacted_placeholder_cannot_be_granted() {
    let source = route_with_queries(
        "queries = ['SELECT \"notes\".\"body\" FROM \"notes\" WHERE \"notes\".\"id\" = …']\n",
    );
    let (location, proxy) = route_grant();
    assert_eq!(
        validate_errors(&source),
        ManifestErrors::from(ManifestError::RedactedQueryPlaceholder {
            location,
            proxy,
            rule: 0,
        })
    );

    // The allowed twin: the same shape with the value bound.
    let parameterised = route_with_queries(
        "queries = ['SELECT \"notes\".\"body\" FROM \"notes\" WHERE \"notes\".\"id\" = $?']\n",
    );
    assert!(Manifest::parse_and_validate(&parameterised, BASE).is_ok());
}

#[test]
fn an_inline_literal_is_refused_and_never_echoed() {
    let source = route_with_queries(
        "queries = [\"SELECT \\\"notes\\\".\\\"name\\\" FROM \\\"notes\\\" \
         WHERE \\\"notes\\\".\\\"name\\\" = 'secret'\"]\n",
    );
    let (location, proxy) = route_grant();
    let errors = validate_errors(&source);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::LiteralQuery {
            location: location.clone(),
            proxy: proxy.clone(),
            rule: 0,
        })
    );
    // The message names the position and the fix, never the value: an error is
    // logged, and a log line is not a place for it.
    let rendered = errors.to_string();
    assert!(!rendered.contains("secret"), "{rendered}");
    assert!(rendered.contains("queries[0]"), "{rendered}");
    assert!(rendered.contains("bind the value"), "{rendered}");

    // A group grant is refused too: it may attach to many routes at once.
    let group = "version = 1\n\
                 [proxy.db]\nprotocol = \"postgresql\"\n\
                 [group.session.db]\nqueries = [\"SELECT 'secret'\"]\n\
                 [[route]]\nname = \"account\"\npath = ['/account/']\ngroups = [\"session\"]\n";
    let errors = validate_errors(group);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::LiteralQuery {
            location: Location::Group("session".to_string()),
            proxy: ProxyName::from("db"),
            rule: 0,
        })
    );
    assert!(!errors.to_string().contains("secret"));
}

#[test]
fn every_literal_form_in_a_grant_is_refused() {
    // The forms the lexer knows, each written inside a TOML basic string.
    for query in [
        "SELECT E'x'",
        "SELECT B'1'",
        "SELECT X'ff'",
        "SELECT N'x'",
        "SELECT $$x$$",
        "SELECT $tag$x$tag$",
        "SELECT U&'x'",
    ] {
        let source = route_with_queries(&format!("queries = [{query:?}]\n"));
        let (location, proxy) = route_grant();
        assert_eq!(
            validate_errors(&source),
            ManifestErrors::from(ManifestError::LiteralQuery {
                location,
                proxy,
                rule: 0,
            }),
            "a literal-bearing grant slipped through: {query:?}"
        );
    }
}

//
// R34 plumbing shared with `blastit check`.
//

#[test]
fn a_statement_is_covered_by_tables_or_by_groups_but_not_by_itself() {
    let source = r#"version = 1
groups = ["session"]

[proxy.db]
protocol = "postgresql"

[group.session.db]
queries = ['SELECT "orders"."id" FROM "orders"']

[[route]]
name = "catalog"
path = ['/products/']
groups = ["session"]
[route.db]
queries = ['SELECT "secrets"."token" FROM "secrets"']
[route.db.tables]
products = { select = ["id"] }
"#;
    let manifest = Manifest::parse_and_validate(source, BASE).expect("valid").0;
    let db = ProxyName::from("db");

    // Covered by `[route.db.tables]`.
    assert!(manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "products"."id" FROM "products" WHERE "products"."id" = $?"#,
    ));
    // Covered by the attached group's exact query.
    assert!(manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "orders"."id" FROM "orders""#,
    ));
    // The route's own `queries` entry is the one being judged: it cannot be its
    // own coverage, or every `dead_grant` would hide itself.
    assert!(!manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "secrets"."token" FROM "secrets""#,
    ));
    // Neither the groups nor the table grants mention this.
    assert!(!manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "audit"."id" FROM "audit""#,
    ));
    // An index no route has covers nothing.
    assert!(!manifest.is_query_covered_by_tables_or_groups(9, &db, "SELECT 1"));

    // A quoted identifier holding `$?` is an identifier, not a placeholder:
    // rewriting it would ask the analysis about a table the grant never named.
    let quoted = r#"version = 1
[proxy.db]
protocol = "postgresql"

[[route]]
name = "odd"
path = ['/odd/']
[route.db]
queries = ['SELECT "a$?"."b" FROM "a$?"']
[route.db.tables]
"a$?" = { select = ["b"] }
"#;
    let manifest = Manifest::parse_and_validate(quoted, BASE).expect("valid").0;
    assert!(manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "a$?"."b" FROM "a$?""#,
    ));
}
