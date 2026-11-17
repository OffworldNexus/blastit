//! The model-equality oracle: one fixture per row of §7.1/§7.2, lowered and
//! compared with an expected `Manifest` built **in this file** (§20 R8).
//!
//! There is no TOML on either side: the expected model is assembled field by
//! field with the crate's builders, so a reviewer reads the Blastfile, the row
//! it proves and the model it must produce without leaving the file. Every row
//! the design **drops** — the `version` header, `workers`, a per-view `static`
//! or `php` block, the global `groups` attach — is asserted **absent**, not
//! merely uncompared.
//!
//! It also carries the capability map's four grant rows (§11.1, §13.1), because
//! `Manifest::effective_grants` is this crate's and the scheduler carries no
//! grant at all. What a test pins of the runtime half is the per-route
//! *selection*: `crates/blastit/src/check.rs::tests::the_route_policies_are_the_effective_grants`
//! (the `RoutePolicySpec` list `blastit check` hands dbproxy from a `Blastfile`)
//! and `stage0::tests::an_explicit_empty_grant_still_names_the_proxy_for_its_route`
//! (the per-route socket rule); dbproxy's `critical_learn_and_redaction.rs`
//! pins that a `RoutePolicySpec` is the allowlist the listener enforces. The
//! production handoff itself, `crates/blastit/src/stage0.rs::bind_view_sockets`,
//! is private, needs stage 0's privileges, and has no test — this leaf records
//! that as a follow-up instead of claiming it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;

use blastit_ingress::{BodyCodec, IngressPolicy};
use blastit_manifest::{
    BodyOwner, Columns, Grant, Group, Manifest, ManifestWarning, PgGrant, PhpPolicy, Protocol,
    Proxy, ProxyName, Route, StaticMount, StaticPolicy, TableGrant, TableGrants, UidLayout,
    blastfile::{StaticImmutable, StaticPattern},
};

// ---- builders ----------------------------------------------------------

/// A column list, as `select products(id, name)` spells it.
fn columns(names: &[&str]) -> Columns {
    Columns::List(names.iter().map(|name| (*name).to_owned()).collect())
}

/// One table's grant: `select`, `insert`, `update` and `delete` as written.
fn table(
    select: Option<Columns>,
    insert: Option<Columns>,
    update: Option<Columns>,
    delete: bool,
) -> TableGrant {
    TableGrant {
        select,
        insert,
        update,
        lock_only: false,
        delete,
    }
}

/// A `postgresql` grant.
fn grant(
    queries: &[&str],
    transaction_control: bool,
    tables: Vec<(&str, TableGrant)>,
    functions: &[&str],
) -> Grant {
    let tables = tables
        .into_iter()
        .map(|(name, grant)| (name.to_owned(), grant));
    let functions = functions.iter().map(|name| (*name).to_owned());
    Grant::Postgresql(
        PgGrant::new(
            queries.iter().map(|query| (*query).to_owned()).collect(),
            transaction_control,
        )
        .with_tables(TableGrants::new(tables, functions)),
    )
}

/// A `postgres` store: its input slots, and the protocol's default outputs.
fn proxy(name: &str, input: &[(&str, &str)]) -> (ProxyName, Proxy) {
    let name = ProxyName::from(name);
    let input = input
        .iter()
        .map(|(slot, variable)| ((*slot).to_owned(), (*variable).to_owned()))
        .collect();
    (
        name.clone(),
        Proxy::new(name, Protocol::Postgresql, input, BTreeMap::new()),
    )
}

/// A trait: `Group { name, grants }`.
fn group(name: &str, grants: Vec<(ProxyName, Grant)>) -> (String, Group) {
    (
        name.to_owned(),
        Group::new(name.to_owned(), grants.into_iter().collect()),
    )
}

/// One lowered route, built field by field.
///
/// A builder rather than a nine-argument function: §7.1's rows differ in one or
/// two fields, and a named setter is what makes the difference readable.
#[derive(Default)]
struct Spec<'a> {
    /// The view's name.
    name: &'a str,
    /// The view's index in file order.
    sandbox: u32,
    /// The compiled path rules.
    path: &'a [&'a str],
    /// The declared methods, or `None` for any.
    methods: Option<&'a [&'a str]>,
    /// The traits the view `use`s.
    groups: &'a [&'a str],
    /// `egress any`.
    egress: bool,
    /// `prefork`.
    prefork: bool,
    /// The view's inline grants.
    grants: Vec<(ProxyName, Grant)>,
    /// The `body` policy, when the view or the endpoint declares one.
    ingress: Option<IngressPolicy>,
    /// Who declared it (§10.3): the view, or one of its endpoints.
    body_owner: BodyOwner,
}

impl<'a> Spec<'a> {
    /// A route with no methods, no traits, no grants and no body.
    fn new(name: &'a str, path: &'a [&'a str]) -> Self {
        Self {
            name,
            path,
            ..Self::default()
        }
    }

    /// The view's sandbox.
    fn sandbox(mut self, sandbox: u32) -> Self {
        self.sandbox = sandbox;
        self
    }

    /// The declared methods.
    fn methods(mut self, methods: &'a [&'a str]) -> Self {
        self.methods = Some(methods);
        self
    }

    /// The traits the view `use`s.
    fn groups(mut self, groups: &'a [&'a str]) -> Self {
        self.groups = groups;
        self
    }

    /// `egress any` and `prefork`.
    fn flags(mut self, egress: bool, prefork: bool) -> Self {
        self.egress = egress;
        self.prefork = prefork;
        self
    }

    /// The view's inline grants.
    fn grants(mut self, grants: Vec<(ProxyName, Grant)>) -> Self {
        self.grants = grants;
        self
    }

    /// The `body` policy.
    fn body(mut self, ingress: IngressPolicy) -> Self {
        self.ingress = Some(ingress);
        self
    }

    /// The route with an **endpoint's** `body` (§10.3): the locator names the
    /// endpoint (`view app POST body`), not the view.
    fn endpoint_body(mut self, ingress: IngressPolicy) -> Self {
        self.ingress = Some(ingress);
        self.body_owner = BodyOwner::Endpoint;
        self
    }

    /// The route itself.
    fn build(self) -> Route {
        Route::new(
            self.name.to_owned(),
            self.path.iter().map(|rule| (*rule).to_owned()).collect(),
            self.methods
                .map(|methods| methods.iter().map(|method| (*method).to_owned()).collect()),
            self.groups.iter().map(|name| (*name).to_owned()).collect(),
            self.egress,
            self.prefork,
            self.grants.into_iter().collect(),
        )
        .with_ingress(self.ingress)
        .with_body_owner(self.body_owner)
        .with_sandbox(self.sandbox)
    }
}

/// One mount.
fn mount(
    prefix: &str,
    root: &str,
    index: &str,
    fallback: Option<&str>,
    immutable: &[&str],
) -> StaticMount {
    StaticMount {
        prefix: prefix.to_owned(),
        policy: StaticPolicy {
            root: root.to_owned(),
            index: index.to_owned(),
            fallback: fallback.map(str::to_owned),
            immutable: if immutable.is_empty() {
                StaticImmutable::None
            } else {
                StaticImmutable::Patterns(
                    immutable
                        .iter()
                        .map(|rule| StaticPattern::quoted(rule).expect("a legal fixture pattern"))
                        .collect(),
                )
            },
        },
    }
}

/// A `body` policy.
fn ingress(body: BodyCodec) -> IngressPolicy {
    IngressPolicy {
        body,
        allow_raw: false,
    }
}

/// The manifest a fixture lowers to, or a failure that names the fixture.
fn lowered(name: &str) -> Manifest {
    Manifest::from_blastfile_str(&common::fixture(name))
        .unwrap_or_else(|error| panic!("{name} lowers: {error}"))
}

/// The manifest a source string lowers to, for the grant rows whose subject is
/// a document rather than a corpus fixture.
fn lower_source(source: &str) -> Manifest {
    Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE)
        .expect("the Blastfile parses and validates")
        .0
}

/// The warnings the same parse produced, in `validate`'s order.
fn warnings_of(source: &str) -> Vec<ManifestWarning> {
    Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE)
        .expect("the Blastfile parses and validates")
        .1
}

/// True when one warning matches `pick`.
fn has_warning(warnings: &[ManifestWarning], pick: impl Fn(&ManifestWarning) -> bool) -> bool {
    warnings.iter().any(pick)
}

/// Asserts the oracle's one comparison, printing both sides on a mismatch.
fn assert_equivalent(actual: &Manifest, expected: &Manifest) {
    assert!(
        actual.equivalent(expected),
        "the lowered manifest differs from the expected model\nactual: {actual:#?}\nexpected: {expected:#?}"
    );
}

/// The rows the design drops, asserted absent on every fixture (§7.1, §12.3).
fn assert_dropped_rows(manifest: &Manifest) {
    // `version` has no header: the field's invariant survives, the file has no
    // spelling for it.
    assert_eq!(manifest.version(), 1, "the model's version is always 1");
    // A global attach of a grant set has no keyword.
    assert!(
        manifest.global_groups().is_empty(),
        "a lowered manifest never attaches a group globally"
    );
    // Blastfile has no per-view `static` or `php` declaration.
    for route in manifest.routes() {
        assert!(
            route.static_policy().is_none(),
            "route {:?} carries a static block",
            route.name()
        );
        assert!(
            route.php().is_none(),
            "route {:?} carries a php block",
            route.name()
        );
    }
    // `workers` is always `None`: the scheduler owns parallelism (§12.3).
    if let Some(php) = manifest.php() {
        assert_eq!(php.workers, None, "a front controller never caps workers");
    }
}

// ---- one test per §7.1/§7.2 row ----------------------------------------

/// §7.1 rows 2–4: the callable, the application's name and the preload list.
#[test]
fn row_the_callable_the_name_and_the_preload_list() {
    let actual = lowered("01-app-and-entrypoint.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            Spec::new("about", &["/about"])
                .body(ingress(BodyCodec::None))
                .build(),
        ],
    )
    .with_name(Some("shop".to_owned()))
    .with_app(Some("shop.wsgi:application".to_owned()))
    .with_preload(vec!["shop.urls".to_owned(), "shop.signals".to_owned()]);
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    assert_eq!(actual.name(), Some("shop"));
    assert_eq!(actual.app(), Some("shop.wsgi:application"));
    assert_eq!(actual.preload(), ["shop.urls", "shop.signals"]);
}

/// §7.1 row 5 and §7.4: a store declared by its URL variable.
#[test]
fn row_a_store_declared_by_its_url_variable() {
    let actual = lowered("02-store-url.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        [proxy("db", &[("url", "DATABASE_URL")])]
            .into_iter()
            .collect(),
        BTreeMap::new(),
        vec![
            Spec::new("products", &["/products"])
                .grants(vec![(
                    ProxyName::from("db"),
                    grant(
                        &[],
                        false,
                        vec![("products", table(Some(Columns::All), None, None, false))],
                        &[],
                    ),
                )])
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    // §7.4: the value is the variable's *name*, and the environment is never
    // read; §7.5: `output` is empty, so the protocol's defaults apply.
    let store = actual.proxy("db").expect("the default store");
    assert_eq!(
        store.input().get("url").map(String::as_str),
        Some("DATABASE_URL")
    );
    assert!(store.output().is_empty());
}

/// §7.1 row 5 and §7.4: the block form, and a named store beside it.
#[test]
fn row_a_store_declared_by_its_slots() {
    let actual = lowered("03-store-block.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        [
            proxy(
                "db",
                &[
                    ("host", "DB_HOST"),
                    ("port", "DB_PORT"),
                    ("user", "DB_USERNAME"),
                    ("password", "DB_PASSWORD"),
                    ("database", "DB_DATABASE"),
                    ("sslmode", "DB_SSLMODE"),
                    ("ca_cert", "DB_CA_CERT"),
                ],
            ),
            proxy("analytics", &[("url", "ANALYTICS_URL")]),
        ]
        .into_iter()
        .collect(),
        BTreeMap::new(),
        vec![
            Spec::new("reports", &["/reports"])
                .grants(vec![(
                    ProxyName::from("analytics"),
                    grant(
                        &[],
                        false,
                        vec![("events", table(Some(Columns::All), None, None, false))],
                        &[],
                    ),
                )])
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    assert_eq!(actual.proxies().len(), 2, "two stores, two proxies");
    assert_eq!(
        actual
            .proxy("db")
            .and_then(|store| store.input().get("host"))
            .map(String::as_str),
        Some("DB_HOST")
    );
}

/// §7.1 row 6 and §11.1: a trait is the shared grant set, and `use` attaches it.
#[test]
fn row_a_shared_grant_set_attached_by_use() {
    let actual = lowered("04-trait-and-use.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        [proxy("db", &[("url", "DATABASE_URL")])]
            .into_iter()
            .collect(),
        [group(
            "session",
            vec![(
                ProxyName::from("db"),
                grant(&["SET TIME ZONE 'UTC'"], true, Vec::new(), &[]),
            )],
        )]
        .into_iter()
        .collect(),
        vec![
            Spec::new("account", &["/account"])
                .groups(&["session"])
                .body(ingress(BodyCodec::Json))
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    // §11.1: the effective grants are `Manifest::effective_grants`', unchanged.
    let effective = actual.effective_grants(0);
    let db = effective
        .get(&ProxyName::from("db"))
        .expect("the trait's grant");
    let db = db.as_postgresql().expect("a postgresql grant");
    assert!(db.transaction_control());
    assert_eq!(db.queries(), ["SET TIME ZONE 'UTC'"]);
}

/// §7.1 row 15 and §13.1: every grant row in one view.
#[test]
fn row_the_grant_table() {
    let actual = lowered("05-grants.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        [proxy("db", &[("url", "DATABASE_URL")])]
            .into_iter()
            .collect(),
        BTreeMap::new(),
        vec![
            Spec::new("catalog", &["/catalog", "/catalog/[0-9]+"])
                .methods(&["GET", "HEAD"])
                .grants(vec![(
                    ProxyName::from("db"),
                    grant(
                        &[
                            "SELECT count(*) FROM products WHERE price > $?",
                            "SELECT 1 FROM products WHERE name = $?",
                        ],
                        true,
                        vec![
                            (
                                "products",
                                table(
                                    Some(columns(&["id", "name"])),
                                    None,
                                    Some(columns(&["price"])),
                                    false,
                                ),
                            ),
                            ("categories", table(Some(Columns::All), None, None, false)),
                            (
                                "orders",
                                table(None, Some(columns(&["id", "total"])), None, false),
                            ),
                            ("archived_products", table(None, None, None, true)),
                        ],
                        &["st_distance", "unaccent"],
                    ),
                )])
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
}

/// §7.1 row 15 and §13.1: `unrestricted` on the default store and a named one.
#[test]
fn row_an_unrestricted_grant() {
    let actual = lowered("06-unrestricted.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        [
            proxy("db", &[("url", "DATABASE_URL")]),
            proxy("analytics", &[("url", "ANALYTICS_URL")]),
        ]
        .into_iter()
        .collect(),
        BTreeMap::new(),
        vec![
            Spec::new("admin", &["/admin"])
                .grants(vec![(
                    ProxyName::from("db"),
                    Grant::Postgresql(PgGrant::unrestricted()),
                )])
                .build(),
            Spec::new("metrics", &["/metrics"])
                .sandbox(1)
                .grants(vec![(
                    ProxyName::from("analytics"),
                    Grant::Postgresql(PgGrant::unrestricted()),
                )])
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
}

// ---- the capability map's grant rows, which are this crate's ------------

/// §11.1: a trait's grants are exactly what `effective_grants` hands the view:
/// `use` order first, then the inline block, `transactions` OR-ed, queries
/// deduplicated.
///
/// The scheduler cannot assert this row: it carries no grant at all
/// (`RouteInfo` is the name, the prefork flag, the uid and the ingress policy).
/// The seam that turns this map into a socket is
/// `crates/blastit/src/stage0.rs::bind_view_sockets`, and
/// `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` pins what the
/// listener then refuses.
#[test]
fn row_a_trait_grants_what_the_view_receives() {
    let source = concat!(
        "postgres $DATABASE_URL\n",
        "\n",
        "trait first {\n",
        "    postgres {\n",
        "        transactions\n",
        "        query 'SELECT 1'\n",
        "    }\n",
        "}\n",
        "\n",
        "trait second {\n",
        "    postgres {\n",
        "        query 'SELECT 2'\n",
        "    }\n",
        "}\n",
        "\n",
        "view catalog GET \"/c\" {\n",
        "    use second, first\n",
        "    postgres {\n",
        "        query 'SELECT 3'\n",
        "        query 'SELECT 1'\n",
        "    }\n",
        "}\n",
    );
    let manifest = lower_source(source);
    let route = manifest.route(0).expect("one view");
    assert_eq!(route.groups(), ["second", "first"], "written order");

    let grants = manifest.effective_grants(0);
    let db = grants
        .values()
        .next()
        .and_then(|grant| grant.as_postgresql())
        .expect("the `db` grant");
    assert_eq!(
        db.queries(),
        ["SELECT 2", "SELECT 1", "SELECT 3"],
        "`use` order, then inline; the inline duplicate of `SELECT 1` is dropped"
    );
    assert!(
        db.transaction_control(),
        "the trait's `transactions` is OR-ed"
    );

    // There is no invisible global attach: a trait nobody `use`s warns instead.
    let unused = "postgres $DATABASE_URL\ntrait idle {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\nview v \"/v\" {}\n";
    assert!(
        has_warning(&warnings_of(unused), |warning| matches!(
            warning,
            ManifestWarning::UnattachedGroup { .. }
        )),
        "a trait no view uses is a warning, not a silent global grant"
    );
    assert!(
        lower_source(unused).effective_grants(0).is_empty(),
        "the unused trait granted the view nothing"
    );
}

/// §13.1: `postgres … unrestricted` on the default and a named store: inline
/// only, each one warns, and a trait cannot carry it.
#[test]
fn row_unrestricted_on_the_default_and_a_named_store() {
    let source = concat!(
        "postgres $DATABASE_URL\n",
        "postgres analytics $ANALYTICS_URL\n",
        "\n",
        "view a GET \"/a\" {\n",
        "    postgres unrestricted\n",
        "}\n",
        "\n",
        "view b GET \"/b\" {\n",
        "    postgres analytics unrestricted\n",
        "}\n",
    );
    let manifest = lower_source(source);

    let a = manifest.effective_grants(0);
    assert!(
        a.values()
            .next()
            .and_then(|grant| grant.as_postgresql())
            .is_some_and(PgGrant::is_unrestricted),
        "the default store is unrestricted"
    );
    let b = manifest.effective_grants(1);
    let analytics = b
        .iter()
        .find(|(name, _)| name.as_str() == "analytics")
        .and_then(|(_, grant)| grant.as_postgresql())
        .expect("the named store's grant");
    assert!(analytics.is_unrestricted());

    let warned = warnings_of(source);
    assert!(
        has_warning(&warned, |warning| matches!(
            warning,
            ManifestWarning::UnrestrictedRoute { route, .. } if route == "a"
        )),
        "{warned:?}"
    );
    assert!(
        has_warning(&warned, |warning| matches!(
            warning,
            ManifestWarning::UnrestrictedRoute { route, proxy } if route == "b" && proxy.as_str() == "analytics"
        )),
        "{warned:?}"
    );

    // The deny side: `unrestricted` in a trait is refused, because a trait
    // could attach it to every view by accident.
    let in_a_trait = "postgres $DATABASE_URL\ntrait t {\n    postgres unrestricted\n}\nview v \"/v\" {\n    use t\n}\n";
    let error = Manifest::parse_blastfile(in_a_trait, UidLayout::DEFAULT_BASE)
        .expect_err("a trait cannot carry unrestricted");
    assert!(
        error
            .to_string()
            .contains("unrestricted is only allowed inline"),
        "{error}"
    );
}

/// §13.1: `select products(id, name)` vs `select products` — the column list
/// and the whole table, through the same accessor the policy reads.
#[test]
fn row_table_and_column_grants() {
    let source = concat!(
        "postgres $DATABASE_URL\n",
        "\n",
        "view narrow \"/n\" {\n",
        "    postgres {\n",
        "        select products(id, name)\n",
        "        insert orders\n",
        "        delete archived\n",
        "    }\n",
        "}\n",
        "\n",
        "view whole \"/w\" {\n",
        "    postgres {\n",
        "        select products\n",
        "    }\n",
        "}\n",
    );
    let manifest = lower_source(source);

    let narrow = manifest.effective_grants(0);
    let narrow = narrow
        .values()
        .next()
        .and_then(|grant| grant.as_postgresql())
        .expect("the narrow grant");
    let products = narrow.tables().table("products").expect("products");
    let select = products.select.as_ref().expect("a select grant");
    assert_eq!(
        select,
        &columns(&["id", "name"]),
        "`select products(id, name)` names two columns"
    );
    assert!(select.contains("id") && select.contains("name"));
    assert!(
        !select.contains("password"),
        "a column grant does not cover the columns it does not name"
    );
    assert!(products.insert.is_none());
    assert!(
        narrow
            .tables()
            .table("orders")
            .is_some_and(|orders| orders.insert.is_some()),
        "`insert orders` names the whole table"
    );
    assert!(
        narrow
            .tables()
            .table("archived")
            .is_some_and(|archived| archived.delete),
        "`delete archived` is a row grant"
    );

    let whole = manifest.effective_grants(1);
    let whole = whole
        .values()
        .next()
        .and_then(|grant| grant.as_postgresql())
        .expect("the whole-table grant");
    let select = whole
        .tables()
        .table("products")
        .and_then(|grant| grant.select.as_ref())
        .expect("a select grant");
    assert_eq!(select, &Columns::All, "`select products` is every column");
    assert!(
        select.contains("password"),
        "`All` covers a column added later"
    );
}

/// §13.1: `query '…'` is stored after `normalize` — `$1` becomes `$?` — and an
/// inlined literal is refused rather than granted.
#[test]
fn row_query_is_stored_normalised() {
    let source = "postgres $DATABASE_URL\n\nview v \"/v\" {\n    postgres {\n        query 'SELECT 1 FROM products WHERE name = $1'\n    }\n}\n";
    let manifest = lower_source(source);
    let grant = manifest
        .effective_grants(0)
        .values()
        .next()
        .and_then(|grant| grant.as_postgresql())
        .cloned()
        .expect("the grant");
    assert_eq!(
        grant.queries(),
        ["SELECT 1 FROM products WHERE name = $?"],
        "the statement is stored normalised, without its quotes"
    );

    // Denied: an inlined literal is a value committed to the repository, so the
    // grant is refused before it can be run.
    let literal = "postgres $DATABASE_URL\n\nview v \"/v\" {\n    postgres {\n        query \"SELECT 1 FROM products WHERE name = 'bob'\"\n    }\n}\n";
    let error = Manifest::parse_blastfile(literal, UidLayout::DEFAULT_BASE)
        .expect_err("an inlined literal is refused");
    assert!(
        error.to_string().contains("inlines a quoted literal"),
        "{error}"
    );
}

/// §7.1 row 9, §10.1 and §10.3: the five `body` rows, one per endpoint.
#[test]
fn row_the_ingress_policies() {
    let actual = lowered("07-body.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            Spec::new("app", &["/a"])
                .methods(&["GET", "HEAD"])
                .endpoint_body(ingress(BodyCodec::Json))
                .build(),
            Spec::new("app", &["/b"])
                .methods(&["POST"])
                .endpoint_body(ingress(BodyCodec::Form))
                .build(),
            Spec::new("app", &["/c"])
                .methods(&["PUT"])
                .endpoint_body(ingress(BodyCodec::None))
                .build(),
            Spec::new("app", &["/d"])
                .methods(&["DELETE"])
                .endpoint_body(ingress(BodyCodec::Raw))
                .build(),
            Spec::new("app", &["/e"])
                .methods(&["PATCH"])
                .endpoint_body(IngressPolicy {
                    body: BodyCodec::Raw,
                    allow_raw: true,
                })
                .build(),
            Spec::new("app", &["/f"]).methods(&["OPTIONS"]).build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    // §10.3: one view, one sandbox — six routes, one uid.
    let sandboxes: Vec<u32> = actual.routes().iter().map(Route::sandbox).collect();
    assert_eq!(sandboxes, [0, 0, 0, 0, 0, 0]);
}

/// §7.1 row 10, §7.2 row 1 and §9.5: a static-only file.
#[test]
fn row_a_static_only_file() {
    let actual = lowered("08-static-only.blastfile");
    let expected =
        Manifest::from_parts(1, Vec::new(), BTreeMap::new(), BTreeMap::new(), Vec::new())
            .with_mounts(vec![mount(
                "/",
                "public",
                "index.html",
                Some("index.html"),
                &["/assets/*", "/fonts/*"],
            )]);
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    assert!(actual.routes().is_empty(), "a static-only file has no view");
    assert_eq!(actual.mounts().len(), 1);
}

/// §7.2 row 2: a mount above a forwarding view.
#[test]
fn row_a_mount_above_a_view() {
    let actual = lowered("09-static-and-view.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            Spec::new("about", &["/about"])
                .body(ingress(BodyCodec::None))
                .build(),
        ],
    )
    .with_mounts(vec![mount(
        "/assets",
        "public/assets",
        "index.html",
        None,
        &["/assets/*"],
    )]);
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
}

/// §7.1 row 11, §7.2 row 3 and §12.2: a mount beside a front controller.
#[test]
fn row_a_mount_beside_a_front_controller() {
    let actual = lowered("10-static-and-php.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            Spec::new("app", &["/.*"])
                .body(ingress(BodyCodec::None))
                .build(),
        ],
    )
    .with_mounts(vec![mount(
        "/assets",
        "public/assets",
        "index.html",
        None,
        &[],
    )])
    .with_php(Some(PhpPolicy {
        entry: "public/index.php".to_owned(),
        workers: None,
    }));
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    // §12.2: one front controller for the whole file, and no route carries one.
    assert_eq!(
        actual.php().map(|php| php.entry.as_str()),
        Some("public/index.php")
    );
}

/// §7.1 rows 13–14 and §7.2 row 4: a catch-all view with both opt-outs.
#[test]
fn row_a_catch_all_with_prefork_and_egress() {
    let actual = lowered("11-catch-all.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            Spec::new("app", &["/.*"])
                .flags(true, true)
                .body(ingress(BodyCodec::None))
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    let route = actual.route(0).expect("one route");
    assert!(route.egress(), "`egress any` sets the route's flag");
    assert!(route.prefork(), "`prefork` sets the route's flag");
}

/// §7.2 row 5 and §8.1: a raw regex path is kept verbatim.
#[test]
fn row_a_regex_path() {
    let actual = lowered("12-regex-path.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            Spec::new("items", &["/v[0-9]+/items"])
                .body(ingress(BodyCodec::Json))
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
}

/// §7.2 row 6: one view, several patterns.
#[test]
fn row_several_patterns() {
    let actual = lowered("13-several-paths.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            Spec::new(
                "products",
                &[
                    "/products",
                    "/products/[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
                ],
            )
            .methods(&["GET", "HEAD"])
            .body(ingress(BodyCodec::Json))
            .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    assert_eq!(actual.route(0).map(|route| route.path().len()), Some(2));
}

/// §7.2 row 7 and §8.4: `GET` implies `HEAD`.
#[test]
fn row_get_implies_head() {
    let actual = lowered("14-get-implies-head.blastfile");
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            Spec::new("home", &["/"])
                .methods(&["GET", "HEAD"])
                .body(ingress(BodyCodec::None))
                .build(),
            Spec::new("search", &["/search"])
                .sandbox(1)
                .methods(&["GET", "HEAD", "POST"])
                .body(ingress(BodyCodec::Json))
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
}

/// §7.2 row 8, §7.3 and §10.3: one view, two endpoints, one sandbox.
#[test]
fn row_a_multi_endpoint_view() {
    let actual = lowered("15-multi-endpoint.blastfile");
    let cart_items = || {
        grant(
            &[],
            false,
            vec![(
                "cart_items",
                table(Some(columns(&["id", "product_id"])), None, None, false),
            )],
            &[],
        )
    };
    let expected = Manifest::from_parts(
        1,
        Vec::new(),
        [proxy("db", &[("url", "DATABASE_URL")])]
            .into_iter()
            .collect(),
        [group(
            "session",
            vec![(ProxyName::from("db"), grant(&[], true, Vec::new(), &[]))],
        )]
        .into_iter()
        .collect(),
        vec![
            Spec::new("cart", &["/cart"])
                .methods(&["GET", "HEAD"])
                .groups(&["session"])
                .grants(vec![(ProxyName::from("db"), cart_items())])
                .build(),
            Spec::new("cart", &["/cart/items"])
                .methods(&["POST"])
                .groups(&["session"])
                .grants(vec![(ProxyName::from("db"), cart_items())])
                .endpoint_body(ingress(BodyCodec::Json))
                .build(),
        ],
    );
    assert_equivalent(&actual, &expected);
    assert_dropped_rows(&actual);
    // §10.3: two routes, one name, one sandbox — and `validate` agrees they are
    // not a duplicate (`check_routes` compares sandboxes).
    assert_eq!(actual.route_count(), 2);
    assert_eq!(actual.route(0).map(Route::sandbox), Some(0));
    assert_eq!(actual.route(1).map(Route::sandbox), Some(0));
    assert_eq!(actual.route(0).map(Route::name), Some("cart"));
    assert_eq!(actual.route(1).map(Route::name), Some("cart"));
}

// ---- the corpus, and the rows that are dropped -------------------------

/// The corpus is the 15 rows of §7.1/§7.2, and every one of them lowers.
///
/// `MB.21.3.4` (P1–P3), `MB.21.3.6` (fence counts) and `MB.21.5.1` (docs) read
/// this same directory, so the count is asserted here as well as there.
#[test]
fn the_corpus_is_the_fifteen_rows_and_every_one_lowers() {
    let corpus = common::corpus();
    assert_eq!(
        corpus.len(),
        common::CORPUS_SIZE,
        "the corpus holds 15 files"
    );
    for (name, source) in corpus {
        let manifest = Manifest::from_blastfile_str(&source)
            .unwrap_or_else(|error| panic!("{name} lowers: {error}"));
        assert_dropped_rows(&manifest);
        // Every fixture also validates: the corpus is the round-trip corpus of
        // §20, so a document that cannot boot would poison three later leaves.
        manifest
            .validate(20_000)
            .unwrap_or_else(|error| panic!("{name} validates: {error}"));
    }
}

/// §7.1's dropped rows, proven absent rather than merely uncompared.
#[test]
fn the_dropped_rows_are_absent() {
    let manifest = lowered("15-multi-endpoint.blastfile");
    // No `version` header: the model's field survives with its invariant.
    assert_eq!(manifest.version(), 1);
    // No global attach: `use` in each view is the only way (§11.1).
    assert!(manifest.global_groups().is_empty());
    // No per-view `static` or `php` declaration (§7.1, §12.3).
    for route in manifest.routes() {
        assert!(route.static_policy().is_none());
        assert!(route.php().is_none());
    }
    // No `workers` cap, on a front controller or anywhere else.
    assert!(manifest.php().is_none(), "this fixture declares no php");
}

/// §7.3 step 6: `route_lines` carries the `view` header's line, once per route.
#[test]
fn route_lines_carry_the_view_header_line() {
    let source = common::fixture("15-multi-endpoint.blastfile");
    let manifest = Manifest::from_blastfile_str(&source).expect("lowers");
    let lines = manifest.route_lines();
    assert_eq!(lines.len(), 2, "one line per route");
    assert_eq!(lines[0], lines[1], "both routes of one view share its line");
    // The view's header is the line `view cart` is written on.
    let expected = source
        .lines()
        .position(|line| line.starts_with("view cart"))
        .map(|index| index as u32 + 1)
        .expect("the fixture declares `view cart`");
    assert_eq!(lines, [expected, expected]);
}

/// A `view` cannot carry a `static` or a `php` declaration: neither is a view
/// directive, and no lowering path can set either field (§7.1, §12.3).
#[test]
fn a_view_cannot_carry_a_static_or_php_declaration() {
    let takes =
        "a view takes `body`, `use`, `egress`, `prefork`, `postgres` and `METHOD \"pattern\"`";
    for (directive, word) in [
        ("static \"/assets\" { root \"public/assets\" }", "static"),
        ("php \"public/index.php\"", "php"),
        ("entrypoint php \"public/index.php\"", "entrypoint"),
    ] {
        let source = format!("view v \"/v\" {{\n  {directive}\n}}\n");
        let rendered = Manifest::from_blastfile_str(&source)
            .expect_err("the document is refused")
            .errors()
            .first()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert_eq!(
            rendered,
            format!("2:3: `{word}` is not a directive here; {takes}"),
            "{directive}"
        );
    }
    // A file that declares a mount and a front controller lowers to a manifest
    // whose *routes* still carry neither: the two are the file's, not a view's.
    let manifest = lowered("10-static-and-php.blastfile");
    assert_eq!(manifest.mounts().len(), 1);
    assert!(manifest.php().is_some());
    for route in manifest.routes() {
        assert!(route.static_policy().is_none());
        assert!(route.php().is_none());
    }
}

/// `parse_blastfile` surfaces `validate`'s warnings unchanged, in one document
/// (§13.2, acceptance item 4) — one sentence per kind, verbatim, in the order
/// the five fire, and the sixth (`EntrypointNotReached`) from its own file.
#[test]
fn the_validate_warnings_are_surfaced_unchanged() {
    use blastit_manifest::ManifestWarning;

    fn kind(warning: &ManifestWarning) -> &'static str {
        match warning {
            ManifestWarning::UnattachedGroup { .. } => "UnattachedGroup",
            ManifestWarning::UngrantedProxy { .. } => "UngrantedProxy",
            ManifestWarning::UnrestrictedRoute { .. } => "UnrestrictedRoute",
            ManifestWarning::RawIngress { .. } => "RawIngress",
            ManifestWarning::EgressRoute { .. } => "EgressRoute",
            ManifestWarning::EntrypointNotReached { .. } => "EntrypointNotReached",
        }
    }

    let source = "postgres $DATABASE_URL\n\
                  postgres analytics $ANALYTICS_URL\n\
                  trait unused {\n  postgres { select events }\n}\n\
                  view admin \"/admin\" {\n  postgres unrestricted\n  body raw\n  egress any\n}\n";
    let (_, warnings) = Manifest::parse_blastfile(source, 20_000).expect("validates");
    let kinds: Vec<&str> = warnings.iter().map(kind).collect();
    assert_eq!(
        kinds,
        [
            "UnattachedGroup",
            "UngrantedProxy",
            "UnrestrictedRoute",
            "RawIngress",
            "EgressRoute",
        ],
        "{warnings:?}"
    );
    // §13.2: each sentence names the Blastfile node a developer edits.
    assert_eq!(
        warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
        [
            "trait \"unused\" is attached to no view and no global traits: it grants nothing",
            "store \"analytics\" has no grant in any view or trait: it is unusable",
            "view \"admin\" may run any statement on store \"db\" (unrestricted)",
            "view \"admin\": body raw passes the request body through unvalidated",
            "view \"admin\": egress any opens any outbound address and port",
        ],
        "{warnings:?}"
    );

    // §12.2: a front controller no view forwards to is the sixth, and its
    // sentence names the entry and the fix.
    let (_, warnings) = Manifest::parse_blastfile("entrypoint php \"public/index.php\"\n", 20_000)
        .expect("a php entry with no view boots");
    assert_eq!(
        warnings.iter().map(kind).collect::<Vec<_>>(),
        ["EntrypointNotReached"],
        "{warnings:?}"
    );
    assert_eq!(
        warnings[0].to_string(),
        "entrypoint php \"public/index.php\" is reached by no view: declare a `view` that forwards to it",
        "{warnings:?}"
    );
}
