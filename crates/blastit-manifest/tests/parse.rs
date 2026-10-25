//! Parsing: the §6 and §5.1 examples, and every structural error.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{
    Columns, Manifest, ManifestError, ManifestErrors, PgGrant, Protocol, ProxyName,
};

use common::{SECTION_5_1, SECTION_6, parse_errors, section_5_1, section_6, text_map, texts};

#[test]
fn section_6_example_parses() {
    let manifest = section_6();

    assert_eq!(manifest.version(), 1);
    assert_eq!(texts(manifest.global_groups()), ["session"]);
    assert_eq!(manifest.route_count(), 4);

    // [proxy.db]
    assert_eq!(manifest.proxies().len(), 1);
    let db = manifest.proxy("db").expect("proxy db");
    assert_eq!(db.name(), &ProxyName::from("db"));
    assert_eq!(db.protocol(), Protocol::Postgresql);
    assert_eq!(
        text_map(db.input()),
        [("ca_cert", "DATABASE_CA_CERT"), ("url", "DATABASE_URL")]
    );
    assert!(db.output().is_empty(), "no output table: defaults apply");

    // [group.session.db]
    let session = manifest.group("session").expect("group session");
    assert_eq!(session.name(), "session");
    assert_eq!(session.grants().len(), 1);
    let session_db = session
        .grant(&ProxyName::from("db"))
        .expect("session grants db");
    let session_db = session_db.as_postgresql().expect("postgresql grant");
    assert!(session_db.transaction_control());
    assert_eq!(
        texts(session_db.queries()),
        [
            "SET TIME ZONE 'UTC'",
            r#"SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $1 AND "django_session"."session_key" = $2) LIMIT 21"#,
        ]
    );

    // [group.catalog.db]
    let catalog = manifest.group("catalog").expect("group catalog");
    let catalog_db = catalog
        .grant(&ProxyName::from("db"))
        .expect("catalog grants db")
        .as_postgresql()
        .expect("postgresql grant");
    assert!(!catalog_db.transaction_control());
    assert_eq!(catalog_db.queries().len(), 1);
    assert!(catalog_db.queries()[0].starts_with("SELECT \"shop_product\""));

    // [[route]] in manifest order.
    let names: Vec<&str> = manifest.routes().iter().map(|r| r.name()).collect();
    assert_eq!(
        names,
        ["product-detail", "checkout", "about", "fallback"],
        "routes keep their manifest order; routing follows it"
    );

    let product_detail = manifest.route(0).expect("route 0");
    assert_eq!(texts(product_detail.path()), ["/products/[0-9]+/"]);
    assert_eq!(
        product_detail.methods().map(texts),
        Some(vec!["GET", "HEAD"])
    );
    assert_eq!(texts(product_detail.groups()), ["catalog"]);
    assert!(product_detail.prefork());
    assert!(!product_detail.egress());
    assert!(
        product_detail.grants().is_empty(),
        "grants come from catalog"
    );

    let checkout = manifest.route(1).expect("route 1");
    assert!(checkout.egress());
    assert!(!checkout.prefork());
    assert!(checkout.methods().is_none(), "no methods: any method");
    assert_eq!(texts(checkout.groups()), ["catalog"]);
    let checkout_db = checkout
        .grant(&ProxyName::from("db"))
        .expect("checkout grants db inline")
        .as_postgresql()
        .expect("postgresql grant");
    assert_eq!(checkout_db.queries().len(), 1);
    assert!(checkout_db.queries()[0].starts_with("INSERT INTO \"shop_order\""));
    assert!(!checkout_db.transaction_control());

    let about = manifest.route(2).expect("route 2");
    assert!(about.groups().is_empty());
    assert!(about.grant(&ProxyName::from("db")).is_some());

    let fallback = manifest.route(3).expect("route 3");
    assert_eq!(texts(fallback.path()), [".*"]);
    assert!(fallback.grants().is_empty(), "global groups only");
    assert!(fallback.groups().is_empty());
    assert!(!fallback.egress());
    assert!(!fallback.prefork());
}

#[test]
fn section_5_1_two_proxies_parse() {
    let manifest = section_5_1();

    assert_eq!(manifest.proxies().len(), 2);
    assert!(manifest.routes().is_empty());

    let db = manifest.proxy("db").expect("proxy db");
    assert_eq!(db.protocol(), Protocol::Postgresql);
    assert_eq!(
        text_map(db.input()),
        [("ca_cert", "DATABASE_CA_CERT"), ("url", "DATABASE_URL")]
    );
    assert!(db.output().is_empty());

    let analytics = manifest.proxy("analytics").expect("proxy analytics");
    assert_eq!(text_map(analytics.input()), [("url", "ANALYTICS_URL")]);
    assert_eq!(
        text_map(analytics.output()),
        [
            ("ANALYTICS_DB_HOST", "{{ socket_dir }}"),
            ("ANALYTICS_DB_NAME", "{{ database }}"),
            ("ANALYTICS_DB_PORT", "{{ port }}"),
            ("ANALYTICS_DB_USER", "{{ user }}"),
        ],
        "output templates stay raw: rendering is the environment builder's job (§5.3)"
    );
}

#[test]
fn unknown_keys_are_errors() {
    assert_eq!(
        parse_errors("version = 1\nprocotol = 1\n"),
        ManifestErrors::from(ManifestError::UnknownKey {
            path: "procotol".to_string(),
        })
    );

    assert_eq!(
        parse_errors(
            "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\nurl = \"DATABASE_URL\"\n"
        ),
        ManifestErrors::from(ManifestError::UnknownKey {
            path: "proxy.db.url".to_string(),
        })
    );

    let declared = Manifest::from_toml_str(
        "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = { url = \"DATABASE_URL\" }\n",
    )
    .expect("an input-only proxy is fine");
    assert!(declared.proxy("db").is_some());
}

#[test]
fn unknown_grant_keys_are_errors() {
    let group = "version = 1\n\
                 [proxy.db]\nprotocol = \"postgresql\"\n\
                 [group.session.db]\nqueries = []\nallow_everything = true\n";
    assert_eq!(
        parse_errors(group),
        ManifestErrors::from(ManifestError::UnknownKey {
            path: "group.session.db.allow_everything".to_string(),
        })
    );

    let inline = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n\
                  [route.db]\nqueries = []\ntransaction_control = true\negress = true\n";
    assert_eq!(
        parse_errors(inline),
        ManifestErrors::from(ManifestError::UnknownKey {
            path: "route[0].db.egress".to_string(),
        }),
        "egress belongs to the route, not to the grant"
    );
}

#[test]
fn grant_contents_are_typed() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.session.db]\nqueries = \"SELECT 1\"\ntransaction_control = \"yes\"\n";
    assert_eq!(
        parse_errors(source),
        ManifestErrors::new(vec![
            ManifestError::WrongType {
                path: "group.session.db.queries".to_string(),
                expected: "an array",
                found: "a string",
            },
            ManifestError::WrongType {
                path: "group.session.db.transaction_control".to_string(),
                expected: "a boolean",
                found: "a string",
            },
        ])
    );

    let defaults = Manifest::from_toml_str(
        "version = 1\n\
         [proxy.db]\nprotocol = \"postgresql\"\n\
         [[route]]\nname = \"about\"\npath = ['/about/']\n\
         [route.db]\n",
    )
    .expect("an empty grant uses the defaults");
    let grant = defaults
        .route(0)
        .expect("route 0")
        .grant(&ProxyName::from("db"))
        .expect("grant")
        .as_postgresql()
        .expect("postgresql");
    assert!(grant.queries().is_empty());
    assert!(!grant.transaction_control());
}

#[test]
fn version_is_required_and_must_be_one() {
    assert_eq!(
        parse_errors("[proxy.db]\nprotocol = \"postgresql\"\n"),
        ManifestErrors::from(ManifestError::MissingVersion)
    );
    assert_eq!(
        parse_errors("version = 2\n"),
        ManifestErrors::from(ManifestError::UnsupportedVersion {
            found: "2".to_string(),
        })
    );
    assert_eq!(
        parse_errors("version = \"1\"\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "version".to_string(),
            expected: "the integer 1",
            found: "a string",
        })
    );
    assert_eq!(
        Manifest::from_toml_str("version = 1\n").unwrap().version(),
        1
    );
}

#[test]
fn protocol_is_required_and_known() {
    assert_eq!(
        parse_errors("version = 1\n[proxy.db]\ninput = { url = \"DATABASE_URL\" }\n"),
        ManifestErrors::from(ManifestError::MissingKey {
            path: "proxy.db.protocol".to_string(),
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[proxy.cache]\nprotocol = \"redis\"\n"),
        ManifestErrors::from(ManifestError::UnknownProtocol {
            proxy: "cache".to_string(),
            protocol: "redis".to_string(),
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[proxy.db]\nprotocol = 1\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "proxy.db.protocol".to_string(),
            expected: "a string",
            found: "an integer",
        })
    );
}

#[test]
fn a_broken_proxy_declaration_is_not_reported_again_as_an_unknown_proxy() {
    let source = "version = 1\n\
                  [proxy.cache]\nprotocol = \"redis\"\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n\
                  [route.cache]\nwhatever = 1\n";
    assert_eq!(
        parse_errors(source),
        ManifestErrors::from(ManifestError::UnknownProtocol {
            proxy: "cache".to_string(),
            protocol: "redis".to_string(),
        }),
        "the declaration error is enough; the grant is not checked further"
    );
}

#[test]
fn input_names_must_not_be_empty() {
    assert_eq!(
        parse_errors(
            "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = { url = \"\" }\n"
        ),
        ManifestErrors::from(ManifestError::Empty {
            path: "proxy.db.input.url (input variable name)".to_string(),
        })
    );
    assert_eq!(
        parse_errors(
            "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = { \"\" = \"DATABASE_URL\" }\n"
        ),
        ManifestErrors::from(ManifestError::Empty {
            path: "input slot".to_string(),
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = \"url\"\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "proxy.db.input".to_string(),
            expected: "a table",
            found: "a string",
        })
    );
}

#[test]
fn route_fields_are_required_and_typed() {
    assert_eq!(
        parse_errors("version = 1\n[[route]]\npath = ['/about/']\n"),
        ManifestErrors::from(ManifestError::MissingKey {
            path: "route[0].name".to_string(),
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[[route]]\nname = \"about\"\n"),
        ManifestErrors::from(ManifestError::MissingKey {
            path: "route[0].path".to_string(),
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[[route]]\nname = \"about\"\npath = \"/about/\"\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route[0].path".to_string(),
            expected: "an array",
            found: "a string",
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[[route]]\nname = \"about\"\npath = [1]\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route[0].path[0]".to_string(),
            expected: "a string",
            found: "an integer",
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[[route]]\nname = \"about\"\npath = []\negress = \"yes\"\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route[0].egress".to_string(),
            expected: "a boolean",
            found: "a string",
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[[route]]\nname = \"about\"\npath = []\ngroups = 1\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route[0].groups".to_string(),
            expected: "an array",
            found: "an integer",
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[[route]]\nname = \"about\"\npath = []\nprefork = 1\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route[0].prefork".to_string(),
            expected: "a boolean",
            found: "an integer",
        })
    );
}

#[test]
fn an_unknown_route_key_that_is_not_a_table_is_an_error() {
    // "egres" is neither a route key nor a proxy name; a typo must not be read
    // as an inline grant.
    assert_eq!(
        parse_errors(
            "version = 1\n[[route]]\nname = \"about\"\npath = ['/about/']\negres = true\n"
        ),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route[0].egres".to_string(),
            expected: "one of the route keys (`name`, `path`, `methods`, `groups`, `egress`, \
                       `prefork`) or a table holding an inline grant",
            found: "a boolean",
        })
    );
}

#[test]
fn broken_toml_is_a_syntax_error_with_a_position() {
    let errors = parse_errors("version = 1\nversion = 1\n");
    assert_eq!(errors.len(), 1);
    let rendered = errors.to_string();
    assert!(rendered.contains("invalid TOML"), "{rendered}");
    assert!(rendered.contains("line 2"), "{rendered}");
    assert!(matches!(errors.errors()[0], ManifestError::Syntax { .. }));
}

#[test]
fn syntax_errors_after_non_ascii_text_stay_on_one_line() {
    for source in [
        "version = 1\n# café ☕\nversion = 1\n",
        "version = 1\n\"café\" = \"☕\"\n\"café\" = \"☕\"\n",
        "version = 1\ncafé = ☕\n",
        "version = 1\nname = \"ok\"\n\u{0}\n",
    ] {
        let errors = parse_errors(source);
        let rendered = errors.to_string();
        assert!(
            rendered.starts_with("manifest has 1 error(s)"),
            "{source:?} -> {rendered}"
        );
        for line in rendered.lines() {
            assert!(
                line.starts_with("manifest has 1 error(s)") || line.starts_with("  - "),
                "{source:?} -> {rendered}"
            );
        }
        let single = errors.errors()[0].to_string();
        assert!(!single.contains('\n'), "{source:?} -> {single}");
    }
}

#[test]
fn control_characters_in_names_and_keys_are_escaped() {
    // A quoted TOML key may hold a newline; the error must not carry it.
    let source = "version = 1\n[[route]]\nname = \"bad\\nname\"\npath = ['.*']\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let errors = manifest.validate(20_000).expect_err("invalid route name");
    let rendered = errors.errors()[0].to_string();
    assert!(!rendered.contains('\n'), "{rendered:?}");
    assert!(
        rendered.contains(r#"invalid name "bad\nname""#),
        "{rendered}"
    );

    let source = "version = 1\n[proxy.\"a\\nb\"]\nprotocol = \"postgresql\"\noops = 1\n";
    let errors = parse_errors(source);
    let rendered = errors.errors()[0].to_string();
    assert!(
        rendered.contains(r#"proxy.a\nb.oops: unknown key"#),
        "{rendered}"
    );
    assert!(!rendered.contains('\n'));
}

#[test]
fn long_patterns_are_cut_short_in_messages_but_kept_in_the_error() {
    let pattern = format!("[{}", "a".repeat(400));
    let source = format!("version = 1\n[[route]]\nname = \"about\"\npath = ['{pattern}']\n");
    let manifest = Manifest::from_toml_str(&source).expect("parses");
    let errors = manifest.validate(20_000).expect_err("not a regex");
    match &errors.errors()[0] {
        ManifestError::InvalidRegex {
            pattern: reported, ..
        } => assert_eq!(reported, &pattern, "the error keeps the pattern as written"),
        other => panic!("unexpected error {other:?}"),
    }

    let rendered = errors.errors()[0].to_string();
    assert!(rendered.contains('…'), "{rendered}");
    assert!(!rendered.contains('\n'), "{rendered}");
    assert!(rendered.len() < 220, "still short: {rendered}");
    assert!(
        !rendered.contains(&"a".repeat(200)),
        "the pattern is cut in the message"
    );
}

#[test]
fn every_problem_is_collected_and_listed() {
    let source = "version = 1\n\
                  first = 1\n\
                  second = 2\n\
                  [proxy.cache]\nprotocol = \"redis\"\n";
    let errors = parse_errors(source);
    assert_eq!(errors.len(), 3, "{errors:?}");
    for expected in [
        ManifestError::UnknownProtocol {
            proxy: "cache".to_string(),
            protocol: "redis".to_string(),
        },
        ManifestError::UnknownKey {
            path: "first".to_string(),
        },
        ManifestError::UnknownKey {
            path: "second".to_string(),
        },
    ] {
        assert!(errors.errors().contains(&expected), "missing {expected}");
    }
    assert!(!errors.is_empty());

    let rendered = errors.to_string();
    assert!(
        rendered.starts_with("manifest has 3 error(s)"),
        "{rendered}"
    );
    assert!(rendered.contains("\n  - first: unknown key"), "{rendered}");
    assert!(rendered.contains("\n  - second: unknown key"), "{rendered}");

    let collected: Vec<ManifestError> = errors.into_iter().collect();
    assert_eq!(collected.len(), 3);
}

#[test]
fn tables_that_are_not_tables_are_reported() {
    assert_eq!(
        parse_errors("version = 1\nproxy = \"db\"\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "proxy".to_string(),
            expected: "a table",
            found: "a string",
        })
    );
    assert_eq!(
        parse_errors("version = 1\nroute = 1\n"),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route".to_string(),
            expected: "an array",
            found: "an integer",
        })
    );
}

#[test]
fn empty_proxy_and_group_names_are_reported() {
    assert_eq!(
        parse_errors("version = 1\n[proxy.\"\"]\nprotocol = \"postgresql\"\n"),
        ManifestErrors::from(ManifestError::Empty {
            path: "proxy name".to_string(),
        })
    );
    assert_eq!(
        parse_errors("version = 1\n[group.\"\".db]\nqueries = []\n"),
        ManifestErrors::from(ManifestError::Empty {
            path: "group name".to_string(),
        })
    );
}

#[test]
fn a_manifest_needs_nothing_but_a_version() {
    let manifest = Manifest::from_toml_str("version = 1\n").expect("parses");
    assert_eq!(manifest.route_count(), 0);
    assert!(manifest.proxies().is_empty());
    assert!(manifest.groups().is_empty());
    assert!(manifest.global_groups().is_empty());
    assert_eq!(manifest.validate(20_000).expect("validates"), vec![]);
}

#[test]
fn the_fixtures_are_the_documented_text() {
    assert!(SECTION_6.starts_with("version = 1"));
    assert!(SECTION_5_1.contains("[proxy.analytics.output]"));
}

#[test]
fn an_inline_unrestricted_grant_parses() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [[route]]\nname = \"admin\"\npath = ['/admin/']\n\
                  [route.db]\nunrestricted = true\ntransaction_control = true\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let route = manifest.route(0).expect("route 0");
    let grant = route
        .grant(&ProxyName::from("db"))
        .expect("an inline grant")
        .as_postgresql()
        .expect("a postgresql grant");
    assert!(grant.is_unrestricted());
    assert!(grant.transaction_control());
    assert!(grant.queries().is_empty());
    assert!(!grant.is_empty(), "it allows everything, so it grants");

    // Without the key, the same grant is an ordinary, empty one.
    let source = source.replace("unrestricted = true\n", "");
    let manifest = Manifest::from_toml_str(&source).expect("parses");
    let grant = manifest
        .route(0)
        .and_then(|route| route.grant(&ProxyName::from("db")))
        .and_then(|grant| grant.as_postgresql())
        .expect("the grant");
    assert!(!grant.is_unrestricted());

    // `unrestricted = false` is the same as leaving it out.
    let source = source.replace(
        "transaction_control = true",
        "transaction_control = true\nunrestricted = false",
    );
    let manifest = Manifest::from_toml_str(&source).expect("parses");
    let grant = manifest
        .route(0)
        .and_then(|route| route.grant(&ProxyName::from("db")))
        .and_then(|grant| grant.as_postgresql())
        .expect("the grant");
    assert!(!grant.is_unrestricted());
}

#[test]
fn a_group_may_be_parsed_with_unrestricted_because_validation_refuses_it() {
    // The parser only reads the document; the rule that a group must not carry
    // it is a cross-reference check (see tests/validate.rs).
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.backoffice.db]\nunrestricted = true\n\
                  [[route]]\nname = \"admin\"\npath = ['/admin/']\ngroups = [\"backoffice\"]\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let grant = manifest
        .group("backoffice")
        .and_then(|group| group.grant(&ProxyName::from("db")))
        .and_then(|grant| grant.as_postgresql())
        .expect("the group's grant");
    assert!(grant.is_unrestricted());
}

#[test]
fn unrestricted_must_be_a_boolean() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [[route]]\nname = \"admin\"\npath = ['/admin/']\n\
                  [route.db]\nunrestricted = \"yes\"\n";
    assert_eq!(
        parse_errors(source),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route[0].db.unrestricted".to_string(),
            expected: "a boolean",
            found: "a string",
        })
    );
    // ... and an unknown key next to it is still an error.
    let source = source.replace("unrestricted = \"yes\"", "unrestrictedt = true");
    assert_eq!(
        parse_errors(&source),
        ManifestErrors::from(ManifestError::UnknownKey {
            path: "route[0].db.unrestrictedt".to_string(),
        })
    );
}

//
// Table and column grants (§6, D7).
//

/// A manifest with one route whose grant is exactly `grant`.
fn manifest_with(grant: &str) -> String {
    format!(
        "version = 1\n\
         [proxy.db]\nprotocol = \"postgresql\"\n\
         [[route]]\nname = \"notes\"\npath = ['/notes/']\n\
         [route.db]\n{grant}"
    )
}

/// The route's inline `postgresql` grant.
fn inline(source: &str) -> PgGrant {
    Manifest::from_toml_str(source)
        .expect("parses")
        .route(0)
        .and_then(|route| route.grant(&ProxyName::from("db")))
        .and_then(|grant| grant.as_postgresql())
        .cloned()
        .expect("the route's grant")
}

#[test]
fn table_grants_parse() {
    let grant = inline(&manifest_with(
        "functions = [\"st_distance\", \"unaccent\"]\n\n\
         [route.db.tables]\n\
         restaurants_restaurant = { select = [\"id\", \"slug\", \"name\"] }\n\
         restaurants_cuisinetype = { select = \"*\" }\n\
         orders_order = { select = \"*\", insert = \"*\", update = [\"status\", \"accepted_at\"], delete = true }\n",
    ));
    assert!(grant.queries().is_empty());
    let tables = grant.tables();
    assert_eq!(
        tables.table("restaurants_restaurant").unwrap().select,
        Some(Columns::list(["id", "slug", "name"]))
    );
    assert_eq!(
        tables.table("restaurants_cuisinetype").unwrap().select,
        Some(Columns::All)
    );
    let orders = tables.table("orders_order").unwrap();
    assert_eq!(orders.select, Some(Columns::All));
    assert_eq!(orders.insert, Some(Columns::All));
    assert_eq!(
        orders.update,
        Some(Columns::list(["status", "accepted_at"]))
    );
    assert!(orders.delete);
    assert_eq!(
        texts(&tables.functions().map(str::to_string).collect::<Vec<_>>()),
        ["st_distance", "unaccent"]
    );

    assert!(
        !grant.is_empty(),
        "table grants alone make a grant non-empty"
    );
    assert!(!grant.is_unrestricted());
    assert!(!grant.transaction_control());
}

#[test]
fn a_table_grant_may_name_another_schema() {
    let grant = inline(&manifest_with(
        "[route.db.tables.\"audit.users\"]\nselect = [\"id\"]\n",
    ));
    assert!(grant.tables().table("audit.users").is_some());
    // `public.` is the same table as the unqualified name.
    let grant = inline(&manifest_with(
        "[route.db.tables.\"public.notes\"]\nselect = \"*\"\n",
    ));
    assert!(grant.tables().table("notes").is_some());
    // ... and `pg_catalog.lower` is `lower`.
    let grant = inline(&manifest_with(
        "functions = [\"pg_catalog.lower\"]\n\n[route.db.tables]\nt = { select = \"*\" }\n",
    ));
    assert_eq!(Vec::from_iter(grant.tables().functions()), ["lower"]);
}

#[test]
fn an_unknown_table_operation_is_refused() {
    assert_eq!(
        parse_errors(&manifest_with(
            "[route.db.tables]\nt = { select = \"*\", upsert = true }\n"
        )),
        ManifestErrors::from(ManifestError::UnknownTableOperation {
            path: "route[0].db.tables.t.upsert".to_string(),
        })
    );
}

#[test]
fn a_column_grant_is_a_star_or_a_list() {
    // `select = "*"` is the string form ...
    assert!(
        Manifest::from_toml_str(&manifest_with(
            "[route.db.tables]\nt = { select = \"*\" }\n"
        ))
        .is_ok()
    );
    // ... a list of names the other.
    assert!(
        Manifest::from_toml_str(&manifest_with(
            "[route.db.tables]\nt = { select = [\"id\"] }\n"
        ))
        .is_ok()
    );

    for (bad, found) in [
        ("select = \"id\"", "a string"),
        ("select = 1", "an integer"),
        ("select = true", "a boolean"),
    ] {
        let errors = parse_errors(&manifest_with(&format!(
            "[route.db.tables]\nt = {{ {bad} }}\n"
        )));
        let error = &errors.errors()[0];
        assert!(
            matches!(error, ManifestError::WrongType { path, .. } if path == "route[0].db.tables.t.select"),
            "{bad}: {error:?}"
        );
        assert!(error.to_string().contains(found), "{bad}: {error}");
    }
}

#[test]
fn a_star_inside_a_column_list_is_refused() {
    assert_eq!(
        parse_errors(&manifest_with(
            "[route.db.tables]\nt = { select = [\"*\", \"id\"] }\n"
        )),
        ManifestErrors::from(ManifestError::StarredColumn {
            path: "route[0].db.tables.t.select".to_string(),
        })
    );
}

#[test]
fn an_empty_table_or_column_name_is_refused() {
    assert!(matches!(
        parse_errors(&manifest_with(
            "[route.db.tables]\n\"\" = { select = \"*\" }\n"
        ))
        .errors()[0],
        ManifestError::Empty { .. }
    ));
    assert!(matches!(
        parse_errors(&manifest_with(
            "[route.db.tables]\nt = { select = [\"\"] }\n"
        ))
        .errors()[0],
        ManifestError::Empty { .. }
    ));
    assert!(matches!(
        parse_errors(&manifest_with(
            "functions = [\"\"]\n\n[route.db.tables]\nt = { select = \"*\" }\n"
        ))
        .errors()[0],
        ManifestError::Empty { .. }
    ));
}

#[test]
fn delete_must_be_a_boolean() {
    assert_eq!(
        parse_errors(&manifest_with(
            "[route.db.tables]\nt = { delete = \"yes\" }\n"
        )),
        ManifestErrors::from(ManifestError::WrongType {
            path: "route[0].db.tables.t.delete".to_string(),
            expected: "a boolean",
            found: "a string",
        })
    );
    // `delete = false` grants nothing, and is not an error.
    let grant = inline(&manifest_with(
        "[route.db.tables]\nt = { delete = false }\n",
    ));
    assert!(!grant.tables().table("t").unwrap().delete);
    assert!(grant.tables().is_empty(), "nothing is granted");
}

#[test]
fn a_table_grant_must_be_a_table_of_operations() {
    assert!(matches!(
        parse_errors(&manifest_with("[route.db.tables]\nt = \"*\"\n")).errors()[0],
        ManifestError::WrongType { .. }
    ));
    assert!(matches!(
        parse_errors(&manifest_with("tables = \"*\"\n")).errors()[0],
        ManifestError::WrongType { .. }
    ));
    // An unknown key next to `tables` is still an error.
    assert_eq!(
        parse_errors(&manifest_with("tablesx = {}\n")),
        ManifestErrors::from(ManifestError::UnknownKey {
            path: "route[0].db.tablesx".to_string(),
        })
    );
}

#[test]
fn functions_must_be_a_list_of_names() {
    assert!(matches!(
        parse_errors(&manifest_with("functions = \"lower\"\n")).errors()[0],
        ManifestError::WrongType { .. }
    ));
    assert!(matches!(
        parse_errors(&manifest_with("functions = [1]\n")).errors()[0],
        ManifestError::WrongType { .. }
    ));
    let grant = inline(&manifest_with("functions = [\"unaccent\", \"unaccent\"]\n"));
    assert_eq!(Vec::from_iter(grant.tables().functions()), ["unaccent"]);
}
