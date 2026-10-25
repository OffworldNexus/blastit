//! Parsing: the §6 and §5.1 examples, and every structural error.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{Manifest, ManifestError, ManifestErrors, Protocol, ProxyName};

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
