//! `TESTING_PLAN.md` §7 — route precedence, path patterns, trait composition
//! and the grant rows, against the real [`RoutingTable`] and lowered model.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{
    Columns, Grant, Manifest, ManifestError, ManifestWarning, Match, PgGrant, ProxyName,
    RoutingTable, UidLayout,
};

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

/// One view's lowered grant on one store.
fn grant<'a>(manifest: &'a Manifest, view: &str, store: &str) -> &'a PgGrant {
    manifest
        .routes()
        .iter()
        .find(|route| route.name() == view)
        .and_then(|route| route.grant(&ProxyName::from(store)))
        .and_then(Grant::as_postgresql)
        .unwrap_or_else(|| panic!("view {view} has a postgres grant on {store}"))
}
/// The routing table of a document holding only views.
fn routing(source: &str) -> RoutingTable {
    RoutingTable::new(&lowered(source)).unwrap_or_else(|error| panic!("{error}"))
}

/// Whether `path` reaches the one view `header` describes.
fn matches(header: &str, path: &str) -> bool {
    routing(&format!("view v {header} {{\n}}\n"))
        .route("GET", path)
        .is_some()
}
#[test]
fn route_precedence_is_written_order_exact_then_prefix_then_regex() {
    let routes = routing(
        "view products GET \"/products\" {\n}\n\
         view files GET \"/assets/*\" {\n}\n\
         view api GET regex '/v[0-9]+/items' {\n}\n\
         view catch_all \"/*\" {\n}\n",
    );
    let hit = |path: &str| routes.route("GET", path);
    assert_eq!(hit("/products"), Some(Match { route: 0, rule: 0 }));
    assert_eq!(
        hit("/products/1"),
        Some(Match { route: 3, rule: 0 }),
        "an exact path is not a prefix"
    );
    assert_eq!(hit("/assets/a/b.css"), Some(Match { route: 1, rule: 0 }));
    assert_eq!(
        hit("/assets-secret/key.pem"),
        Some(Match { route: 3, rule: 0 }),
        "a `/*` tail never widens to a sibling"
    );
    assert_eq!(hit("/v12/items"), Some(Match { route: 2, rule: 0 }));
    assert_eq!(
        hit("/v12/items/1"),
        Some(Match { route: 3, rule: 0 }),
        "a `regex` is anchored to the whole path"
    );
    // The first route written wins: reordering the same two views flips it.
    let specific = routing("view admin GET \"/admin/*\" {\n}\nview broad GET \"/*\" {\n}\n");
    assert_eq!(
        specific.route("GET", "/admin/x"),
        Some(Match { route: 0, rule: 0 })
    );
    let broad = routing("view broad GET \"/*\" {\n}\nview admin GET \"/admin/*\" {\n}\n");
    assert_eq!(
        broad.route("GET", "/admin/x"),
        Some(Match { route: 0, rule: 0 }),
        "the catch-all shadows the view written below it"
    );
    // A match names the rule, not just the route.
    let rules = routing("view two GET \"/a\", \"/b\" {\n}\n");
    assert_eq!(rules.route("GET", "/b"), Some(Match { route: 0, rule: 1 }));
    assert_eq!(rules.route("GET", "/c"), None);
}

#[test]
fn the_pattern_table_lowers_to_the_documented_matchers() {
    // Allowed and denied twins for every row of the §8.1 table.
    let cases: &[(&str, &str, bool)] = &[
        ("\"/products\"", "/products", true),
        ("\"/products\"", "/product", false),
        ("\"/products\"", "/products/", false),
        ("\"/v1.0/x\"", "/v1.0/x", true),
        ("\"/v1.0/x\"", "/v1x0/x", false),
        ("\"/products/{id}\"", "/products/7", true),
        ("\"/products/{id}\"", "/products/", false),
        ("\"/products/{id}\"", "/products/7/x", false),
        ("\"/products/{id:int}\"", "/products/12", true),
        ("\"/products/{id:int}\"", "/products/12a", false),
        (
            "\"/orders/{id:uuid}\"",
            "/orders/123e4567-e89b-12d3-a456-426614174000",
            true,
        ),
        ("\"/orders/{id:uuid}\"", "/orders/not-a-uuid", false),
        ("\"/assets/*\"", "/assets/a/b.css", true),
        ("\"/assets/*\"", "/assets", false),
        ("\"/*\"", "/anything/at/all", true),
        ("regex '/v[0-9]+/items'", "/v12/items", true),
        ("regex '/v[0-9]+/items'", "/v12/items/1", false),
        ("regex '/a\\{b\\}'", "/a{b}", true),
        ("regex '/a\\{b\\}'", "/aab", false),
    ];
    for (header, path, expected) in cases {
        assert_eq!(matches(header, path), *expected, "{header} vs {path}");
    }
}

#[test]
fn the_pattern_refusals_and_their_caps_are_positioned() {
    let cases: &[(&str, &str)] = &[
        (
            "view v \"/x/{id:int\" {\n}\n",
            "1:8: the parameter `{id:int` is not closed; write `{id:int}`",
        ),
        (
            "view v \"/x/{}\" {\n}\n",
            "1:8: `{}` has no name; write `{id}`, `{id:int}` or `{id:uuid}`",
        ),
        (
            "view v \"/x/*/y\" {\n}\n",
            "1:8: `*` is only valid at the end of a pattern; write `/x/*`",
        ),
        (
            "view v \"x\" {\n}\n",
            "1:8: a pattern starts with `/`: write `/x`",
        ),
        (
            "view v regex \"x\" {\n}\n",
            "1:14: `regex` takes a raw string: write `regex '/v[0-9]+/items'`",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(refusal(source), *expected, "{source:?}");
    }
    // A 4 KiB pattern is legal; one byte more is refused in both spellings.
    let legal = format!("\"/{}\"", "a".repeat(4_095));
    assert!(Manifest::from_blastfile_str(&format!("view v {legal} {{\n}}\n")).is_ok());
    for header in [
        format!("\"/{}\"", "a".repeat(4_096)),
        format!("regex '/{}'", "a".repeat(4_096)),
    ] {
        let rendered = refusal(&format!("view v {header} {{\n}}\n"));
        assert!(
            rendered.ends_with("a pattern holds at most 4096 bytes"),
            "{rendered}"
        );
    }
    // A `regex` the engine refuses is a clean refusal, and an unbalanced `)`
    // is never rescued into a catch-all by the anchor wrapper.
    let rendered = refusal("view v regex '/a(?=b)/' {\n}\n");
    assert!(
        rendered.starts_with("1:14: regex `/a(?=b)/` does not compile: ")
            && rendered.contains("look-around"),
        "{rendered}"
    );
    assert!(Manifest::from_blastfile_str("view v regex '/x)|(.*' {\n}\n").is_err());
}

#[test]
fn methods_are_upper_case_http_tokens_and_get_answers_head() {
    let manifest = lowered("view products GET \"/products\" {\n}\n");
    let table = RoutingTable::new(&manifest).expect("it routes");
    for method in ["GET", "HEAD"] {
        assert_eq!(
            table.route(method, "/products"),
            Some(Match { route: 0, rule: 0 })
        );
    }
    assert_eq!(
        table.route("POST", "/products"),
        None,
        "an undeclared method is a 405"
    );
    assert_eq!(
        table.methods_for("/products"),
        Some(vec!["GET".to_string(), "HEAD".to_string()]),
        "the 405 names what the view does answer"
    );
    assert_eq!(
        table.methods_for("/elsewhere"),
        None,
        "no view matched at all"
    );

    // A method-less view answers every method, and contributes no 405 list.
    let manifest = lowered("view any \"/any\" {\n}\n");
    let table = RoutingTable::new(&manifest).expect("it routes");
    assert_eq!(
        table.route("PATCH", "/any"),
        Some(Match { route: 0, rule: 0 })
    );
    assert_eq!(table.methods_for("/any"), None);
    for method in ["CONNECT", "TRACE"] {
        let rendered = refusal(&format!("view v {method} \"/x\" {{\n}}\n"));
        assert!(
            rendered.starts_with("1:8: ")
                && rendered.contains(&format!("`{method}` cannot reach a view")),
            "{rendered}"
        );
    }
}

#[test]
fn trait_composition_unions_grants_in_written_order() {
    let (manifest, warnings) = Manifest::parse_blastfile(
        "postgres $DATABASE_URL\n\
         \n\
         trait session {\n    postgres {\n        transactions\n        query \"SET TIME ZONE 'UTC'\"\n    }\n}\n\
         \n\
         trait catalog {\n    postgres {\n        select products(id, name)\n    }\n}\n\
         \n\
         view products GET \"/products\" {\n\
             use session, catalog, session\n\
             postgres {\n\
                 select products(price)\n\
                 query 'SELECT count(*) FROM products WHERE price > $?'\n\
             }\n\
         }\n",
        BASE,
    )
    .expect("the trait document boots");
    assert!(warnings.is_empty(), "{warnings:?}");
    let route = manifest.route(0).expect("one view");
    assert_eq!(
        route.groups(),
        ["session", "catalog"],
        "written order, deduplicated"
    );

    let effective = manifest.effective_grants(0);
    let db = effective
        .get(&ProxyName::from("db"))
        .and_then(Grant::as_postgresql)
        .expect("the union on db");
    assert!(db.transaction_control(), "any contributor turns it on");
    assert_eq!(
        db.queries(),
        [
            "SET TIME ZONE 'UTC'",
            "SELECT count(*) FROM products WHERE price > $?"
        ],
        "traits first, the view's own grant last"
    );
    let products = db.tables().table("products").expect("products");
    assert_eq!(
        products.select,
        Some(Columns::list(["id", "name", "price"])),
        "a trait and the view union their columns"
    );
    // The view's own block carries neither the trait's transaction flag nor its
    // table scope: only the effective union does.
    assert!(!grant(&manifest, "products", "db").transaction_control());

    let rendered = refusal("postgres $DATABASE_URL\nview v \"/x\" {\n    use ghost\n}\n");
    assert_eq!(
        rendered,
        "3:9: view \"v\": `use ghost` names no trait; a trait is declared at the top level: `trait ghost { … }`"
    );
    let errors = Manifest::parse_blastfile(
        "postgres $DATABASE_URL\n\ntrait t {\n    postgres unrestricted\n}\n\nview v \"/x\" {\n    use t\n}\n",
        BASE,
    )
    .expect_err("a trait may not make a view unrestricted");
    assert!(
        errors
            .to_string()
            .contains("unrestricted is only allowed inline on a view"),
        "{errors}"
    );
}
#[test]
fn grants_lower_to_the_documented_shapes() {
    let manifest = lowered(
        "postgres $DATABASE_URL\n\
         \n\
         view admin GET \"/admin\" {\n    postgres unrestricted\n}\n\
         \n\
         view all_columns GET \"/all\" {\n    postgres {\n        select products\n    }\n}\n\
         \n\
         view one GET \"/one\" {\n    postgres {\n        insert orders(id, total)\n        update orders(status)\n        delete archived_orders\n    }\n}\n",
    );
    assert!(grant(&manifest, "admin", "db").is_unrestricted());
    assert!(grant(&manifest, "admin", "db").queries().is_empty());
    let all = grant(&manifest, "all_columns", "db");
    assert_eq!(
        all.tables()
            .table("products")
            .and_then(|table| table.select.clone()),
        Some(Columns::All),
        "a bare table grants every column"
    );
    let one = grant(&manifest, "one", "db");
    let orders = one.tables().table("orders").expect("orders");
    assert_eq!(orders.insert, Some(Columns::list(["id", "total"])));
    assert_eq!(orders.update, Some(Columns::list(["status"])));
    assert!(!orders.delete);
    assert!(
        one.tables()
            .table("archived_orders")
            .is_some_and(|table| table.delete)
    );

    // `$1` and `$?` are one grant: the wire spelling is folded.
    let manifest = lowered(
        "postgres $DATABASE_URL\nview v GET \"/v\" {\n    postgres {\n        query 'SELECT id FROM products WHERE id = $1'\n    }\n}\n",
    );
    assert_eq!(
        grant(&manifest, "v", "db").queries(),
        ["SELECT id FROM products WHERE id = $?"]
    );

    // An unrestricted grant is reported once per view and store at boot.
    let (_, warnings) = Manifest::parse_blastfile(
        "postgres $DATABASE_URL\nview admin GET \"/admin\" {\n    postgres unrestricted\n}\n",
        BASE,
    )
    .expect("it boots");
    assert!(
        warnings.iter().any(|warning| matches!(
            warning,
            ManifestWarning::UnrestrictedRoute { route, .. } if route == "admin"
        )),
        "{warnings:?}"
    );

    // Denied: `unrestricted` takes no block, two grants share no store, and a
    // grant may not name a store the file never declares.
    assert!(Manifest::from_blastfile_str(
        "postgres $DATABASE_URL\nview v \"/x\" {\n    postgres unrestricted {\n        query 'SELECT 1'\n    }\n}\n"
    )
    .is_err());
    assert_eq!(
        refusal(
            "postgres $DATABASE_URL\nview v \"/x\" {\n    postgres {\n        select a\n    }\n    postgres {\n        select b\n    }\n}\n"
        ),
        "6:5: `postgres` is declared once per store in a view; put both grants in one block"
    );
    let errors = Manifest::parse_blastfile(
        "view v \"/x\" {\n    postgres ghost {\n        select a\n    }\n}\n",
        BASE,
    )
    .expect_err("the store is undeclared");
    assert!(
        matches!(
            errors.errors().first(),
            Some(ManifestError::GrantOnUnknownProxy { proxy, .. }) if proxy.as_str() == "ghost"
        ),
        "{errors}"
    );
}
