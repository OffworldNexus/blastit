//! `TESTING_PLAN.md` §7 — the `Blastfile` grammar and its positioned
//! diagnostics.
//!
//! Every case goes through the crate's public reader
//! ([`Manifest::from_blastfile_str`], [`Manifest::parse_blastfile`]), so the
//! suite proves what `blastit dev`, `blastit check` and the LSP read, never a
//! private helper.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{
    BodyCodec, BodyOwner, Columns, Grant, IngressPolicy, Manifest, ManifestWarning, PgGrant,
    Protocol, ProxyName, Route, UidLayout,
};

const BASE: u32 = UidLayout::DEFAULT_BASE;
/// The documented surface of `docs/blastfile.md` and
/// `docs/reference/blastfile.md` in one file: `app`, one `entrypoint`, two
/// stores in both spellings, a trait, a mount with every directive, views with
/// methods and patterns (single- and multi-endpoint), `use`, `body`, `egress`,
/// `prefork`, and every grant row.
const DOCUMENT: &str = r#"# The shop's routing table and permissions.
app "shop"

entrypoint python "shop.wsgi:application" {
    preload "shop.urls", "shop.signals", "shop.urls"
}

postgres $DATABASE_URL

postgres analytics {
    host $ANALYTICS_HOST
    port $ANALYTICS_PORT
    user $ANALYTICS_USER
    password $ANALYTICS_PASSWORD
    database $ANALYTICS_DB
    sslmode $ANALYTICS_SSLMODE
    ca_cert $ANALYTICS_CA
}

trait session {
    postgres {
        transactions
        query "SET TIME ZONE 'UTC'"
    }
}

static "/assets" {
    root "public/assets"
    index "index.html"
    immutable regex '/assets/.+\.[0-9a-f]{12}\.[^/]+'
    fallback "index.html"
}

view products GET, HEAD "/products", "/products/{id:int}" {
    body none
    use session, session
    postgres {
        functions st_distance, pg_catalog.unaccent
        select products(id, name, price), categories
        query 'SELECT count(*) FROM products WHERE price > $?'
    }
    postgres analytics {
        select events(id)
    }
}

view cart {
    GET "/cart"
    POST "/cart/items" { body json }
    PUT "/cart/items/{id:uuid}" { body form }
    DELETE "/cart/items/{id}" { body raw unvalidated }
    use session
    postgres {
        insert cart_items(product_id, quantity)
        update cart_items(quantity)
        delete archived_carts
    }
}

view orders regex '/v[0-9]+/orders' {}

view fallback "/*" {
    egress any
    prefork
}
"#;

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

/// The view names of one manifest, in document order.
fn view_names(manifest: &Manifest) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for route in manifest.routes() {
        if names.last().copied() != Some(route.name()) {
            names.push(route.name());
        }
    }
    names
}

/// The methods of one route as written, with the implied `HEAD`.
fn methods(route: &Route) -> Option<Vec<&str>> {
    route
        .methods()
        .map(|listed| listed.iter().map(String::as_str).collect())
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

#[test]
fn the_documented_surface_parses_and_lowers() {
    let manifest = lowered(DOCUMENT);
    assert_eq!(manifest.name(), Some("shop"));
    assert_eq!(manifest.app(), Some("shop.wsgi:application"));
    assert_eq!(
        manifest
            .preload()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["shop.urls", "shop.signals"],
        "preload keeps written order and drops the repeat"
    );

    let db = manifest.proxy("db").expect("the default store");
    assert_eq!(db.protocol(), Protocol::Postgresql);
    assert_eq!(
        db.input().get("url").map(String::as_str),
        Some("DATABASE_URL"),
        "a store holds the variable's name, never its value"
    );
    assert!(db.output().is_empty(), "the protocol's defaults apply");
    let analytics = manifest.proxy("analytics").expect("the named store");
    assert_eq!(
        analytics.input().get("host").map(String::as_str),
        Some("ANALYTICS_HOST")
    );
    assert_eq!(manifest.mounts().len(), 1);
    let mount = &manifest.mounts()[0];
    assert_eq!(mount.prefix, "/assets");
    assert_eq!(mount.policy.root, "public/assets");
    assert_eq!(mount.policy.index, "index.html");
    assert_eq!(mount.policy.fallback.as_deref(), Some("index.html"));
    let [pattern] = mount.policy.immutable.patterns() else {
        panic!("one `regex` immutable pattern");
    };
    assert!(pattern.is_regex(), "`regex '…'` records its spelling");
    assert!(pattern.matches("/assets/app.0123456789ab.js"));
    assert!(!pattern.matches("/assets/app.js"), "the hash is required");

    assert_eq!(
        view_names(&manifest),
        ["products", "cart", "orders", "fallback"]
    );
    let products = manifest.route(0).expect("the products view");
    assert_eq!(products.sandbox(), 0);
    assert_eq!(methods(products), Some(vec!["GET", "HEAD"]));
    assert_eq!(products.groups(), ["session"], "`use` deduplicates");
    assert_eq!(
        products.ingress(),
        Some(IngressPolicy {
            body: BodyCodec::None,
            allow_raw: false
        })
    );

    let postgres = grant(&manifest, "products", "db");
    assert_eq!(
        postgres.queries(),
        ["SELECT count(*) FROM products WHERE price > $?"]
    );
    assert_eq!(
        postgres.tables().functions().collect::<Vec<_>>(),
        ["st_distance", "unaccent"],
        "a leading `pg_catalog.` is dropped"
    );
    let table = postgres.tables().table("products").expect("products");
    assert_eq!(table.select, Some(Columns::list(["id", "name", "price"])));
    let table = grant(&manifest, "products", "analytics")
        .tables()
        .table("events")
        .expect("events");
    assert_eq!(
        table.select,
        Some(Columns::list(["id"])),
        "a named store's grant"
    );

    // The multi-endpoint form is one sandbox with one route per endpoint.
    let cart: Vec<&Route> = manifest
        .routes()
        .iter()
        .filter(|route| route.name() == "cart")
        .collect();
    assert_eq!(cart.len(), 4, "four endpoints, four routes");
    assert!(cart.iter().all(|route| route.sandbox() == 1));
    assert_eq!(methods(cart[0]), Some(vec!["GET", "HEAD"]));
    assert_eq!(cart[0].body_owner(), BodyOwner::View, "no body was written");
    assert_eq!(
        cart[1].ingress(),
        Some(IngressPolicy {
            body: BodyCodec::Json,
            allow_raw: false
        })
    );
    assert_eq!(cart[1].body_owner(), BodyOwner::Endpoint);
    assert_eq!(
        cart[2].ingress().map(|policy| policy.body),
        Some(BodyCodec::Form)
    );
    assert_eq!(
        cart[3].ingress(),
        Some(IngressPolicy {
            body: BodyCodec::Raw,
            allow_raw: true
        })
    );
    let fallback = manifest.route(6).expect("the catch-all");
    assert!(!products.egress() && !products.prefork());
    assert!(fallback.egress() && fallback.prefork() && fallback.groups().is_empty());
}

#[test]
fn the_grammar_refusal_matrix_is_positioned() {
    let cases: &[(&str, &str)] = &[
        (
            "version = 1\n",
            "1:1: `version` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, `trait`, `view`, `static`",
        ),
        (
            "view v \"/x\" {\n  body = \"json\"\n}\n",
            "2:8: `=` is not Blastfile syntax; write the value after the keyword: `body json`",
        ),
        (
            "postgres \"postgres://x\"\n",
            "1:10: a quoted string is not a variable name; a store reads `$IDENT`: `postgres $DATABASE_URL`",
        ),
        (
            "view v {\n  query 'SELECT 1'\n}\n",
            "2:3: `query` is not a directive here; a view takes `body`, `use`, `egress`, `prefork`, `postgres` and `METHOD \"pattern\"`",
        ),
        (
            "view v {\n  body json\n",
            "3:1: the block opened on line 1 is not closed before the end of the file",
        ),
        (
            "view v \"/x\" { }\nview v \"/y\" { }\n",
            "2:1: `view \"v\"` is declared twice; the first is on line 1",
        ),
        (
            "app \"a\"\napp \"b\"\n",
            "2:1: a file declares one `app`; the first is on line 1",
        ),
        (
            "trait t {\n}\ntrait t {\n}\n",
            "3:1: `trait \"t\"` is declared twice; the first is on line 1",
        ),
        (
            "postgres db $A\npostgres db $B\n",
            "2:1: `store \"db\"` is declared twice; the first is on line 1",
        ),
        (
            "view v {\n  body\n}\n",
            "2:7: `body` needs a codec: `json`, `form`, `none` or `raw`",
        ),
        (
            "view v {\n  body json unvalidated\n}\n",
            "2:13: `unvalidated` acknowledges `raw` only; `body json` already validates the body",
        ),
        (
            "entrypoint python \"x\" {\n  body json\n}\n",
            "2:3: the `entrypoint python` block holds `preload` only",
        ),
        (
            "entrypoint php \"index.phtml\"\n",
            "1:1: entrypoint php: must end in .php",
        ),
    ];
    for (source, expected) in cases {
        let rendered = refusal(source);
        assert_eq!(rendered, *expected, "{source:?}");
        assert!(
            !rendered.contains('\n'),
            "one problem is one line: {source:?}"
        );
    }
}

#[test]
fn a_refusal_never_echoes_a_pasted_secret() {
    for source in [
        "postgres \"postgres://user:hunter2@host/db\"\n",
        "postgres \"$DATABASE_URL/password=hunter2\"\n",
        "app \"shop\" \"hunter2\"\n",
    ] {
        let errors = Manifest::from_blastfile_str(source).expect_err("the document is refused");
        assert!(!errors.is_empty(), "{source:?}: no refusal");
        for error in errors.errors() {
            let rendered = error.to_string();
            assert!(!rendered.contains("hunter2"), "{source:?}: {rendered}");
        }
    }
}

#[test]
fn a_duplicate_mount_and_the_boot_warnings_are_reported() {
    // Two mounts sharing a prefix: the second could never answer.
    let errors = Manifest::parse_blastfile(
        "static \"/x\" {\n    root \"a\"\n}\nstatic \"/x\" {\n    root \"b\"\n}\n",
        BASE,
    )
    .expect_err("the second mount cannot answer");
    assert_eq!(
        errors.errors()[0].to_string(),
        "duplicate static mount \"/x\" (mounts 0 and 1)"
    );

    // `egress any` and a store no view grants both warn at boot.
    let (_, warnings) = Manifest::parse_blastfile(
        "postgres $DATABASE_URL\n\nview app GET \"/*\" {\n    egress any\n}\n",
        BASE,
    )
    .expect("it boots");
    assert!(
        warnings.iter().any(|warning| matches!(
            warning,
            ManifestWarning::EgressRoute { route } if route == "app"
        )),
        "{warnings:?}"
    );
    assert!(
        warnings.iter().any(|warning| matches!(
            warning,
            ManifestWarning::UngrantedProxy { proxy } if proxy.as_str() == "db"
        )),
        "{warnings:?}"
    );
}
