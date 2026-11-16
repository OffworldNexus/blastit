//! §13.1's grant table, through the public entry point (`R14`).
//!
//! Every row is written as the Blastfile spelling and read back as the
//! `PgGrant` / `TableGrants` the model already had, so the mapping is proved on
//! the model a boot uses rather than on an intermediate.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use blastit_manifest::{Columns, Grant, Manifest, PgGrant, ProxyName};

/// The one store every document here declares.
const STORE: &str = "db";

/// A document with one store and one view, whose body is `body`.
fn document(body: &str) -> String {
    format!("postgres $DATABASE_URL\n\nview v \"/v\" {{\n{body}\n}}\n")
}

/// The lowered manifest of `body`.
fn lowered(body: &str) -> Manifest {
    Manifest::from_blastfile_str(&document(body))
        .unwrap_or_else(|error| panic!("{body:?} lowers: {error}"))
}

/// The view's grant on the default store.
fn grant(body: &str) -> PgGrant {
    grant_on(body, STORE)
}

/// The view's grant on `store`.
fn grant_on(body: &str, store: &str) -> PgGrant {
    let manifest = lowered(body);
    let route = manifest.route(0).expect("one route");
    let grant = route
        .grant(&ProxyName::from(store))
        .unwrap_or_else(|| panic!("{body:?} grants {store}"));
    grant.as_postgresql().expect("a postgresql grant").clone()
}

/// The refusal `body` produces, rendered as the user sees it.
fn refusal(body: &str) -> String {
    let errors =
        Manifest::parse_blastfile(&document(body), 20_000).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// `transactions` turns transaction control on, and nothing else.
#[test]
fn transactions_is_transaction_control() {
    let grant = grant("postgres { transactions }");
    assert!(grant.transaction_control());
    assert!(grant.queries().is_empty());
    assert!(grant.tables().is_empty());
    assert!(!grant.is_unrestricted());
}

/// `functions` is a set, and `pg_catalog.` is dropped so the two spellings are
/// one entry.
#[test]
fn functions_are_canonicalised() {
    let first = grant("postgres { functions st_distance, pg_catalog.unaccent }");
    let functions: BTreeSet<&str> = first.tables().functions().collect();
    assert_eq!(
        functions,
        BTreeSet::from(["st_distance", "unaccent"]),
        "a leading `pg_catalog.` is dropped"
    );
    // The same name written twice is one entry, whichever spelling is used.
    let twice = grant("postgres { functions pg_catalog.lower, lower }");
    assert_eq!(twice.tables().functions().count(), 1);
}

/// `select` takes a column list or the whole table.
#[test]
fn select_takes_a_list_or_every_column() {
    let grant = grant("postgres { select products(id, name), categories }");
    let products = grant.tables().tables().get("products").expect("products");
    assert_eq!(
        products.select,
        Some(Columns::List(
            ["id", "name"]
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        ))
    );
    let categories = grant
        .tables()
        .tables()
        .get("categories")
        .expect("categories");
    assert_eq!(categories.select, Some(Columns::All));
    assert_eq!(categories.insert, None);
    assert_eq!(categories.update, None);
    assert!(!categories.delete);
}

/// `insert` and `update` take the same two spellings.
#[test]
fn insert_and_update_take_a_list_or_every_column() {
    let grant =
        grant("postgres { insert orders(id, total), audit; update products(price), stock }");
    let orders = grant.tables().tables().get("orders").expect("orders");
    assert_eq!(
        orders.insert,
        Some(Columns::List(
            ["id", "total"]
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        ))
    );
    let audit = grant.tables().tables().get("audit").expect("audit");
    assert_eq!(audit.insert, Some(Columns::All));
    let products = grant.tables().tables().get("products").expect("products");
    assert_eq!(
        products.update,
        Some(Columns::List(
            ["price"].iter().map(|name| (*name).to_owned()).collect()
        ))
    );
    let stock = grant.tables().tables().get("stock").expect("stock");
    assert_eq!(stock.update, Some(Columns::All));
}

/// `delete` is a whole-table flag.
#[test]
fn delete_is_a_whole_table_flag() {
    let grant = grant("postgres { delete archived_products, sessions }");
    for table in ["archived_products", "sessions"] {
        let entry = grant.tables().tables().get(table).expect(table);
        assert!(entry.delete, "{table}");
        assert_eq!(entry.select, None);
    }
}

/// One table's `select` and `update` in one block are one entry, not two.
#[test]
fn two_operations_on_one_table_are_one_entry() {
    let grant = grant("postgres { select products(id, name); update products(price) }");
    let products = grant.tables().tables().get("products").expect("products");
    assert_eq!(
        products.select,
        Some(Columns::List(
            ["id", "name"]
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        ))
    );
    assert_eq!(
        products.update,
        Some(Columns::List(
            ["price"].iter().map(|name| (*name).to_owned()).collect()
        ))
    );
}

/// The same operation written twice for one table unions its columns instead of
/// dropping the first list.
#[test]
fn a_repeated_operation_unions_its_columns() {
    let grant = grant("postgres { select products(id); select products(name) }");
    let products = grant.tables().tables().get("products").expect("products");
    assert_eq!(
        products.select,
        Some(Columns::List(
            ["id", "name"]
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        ))
    );
}

/// A `query` is stored normalised and without its quotes, in either spelling.
#[test]
fn queries_are_normalised_in_both_spellings() {
    let grant = grant(
        "postgres {\n  query 'SELECT count(*) FROM products WHERE price > $?'\n  \
         query \"SELECT 1 FROM products WHERE name = $1\"\n}",
    );
    assert_eq!(
        grant.queries(),
        [
            "SELECT count(*) FROM products WHERE price > $?",
            "SELECT 1 FROM products WHERE name = $?",
        ],
        "`$1` is normalised to `$?` and the quotes are stripped"
    );
}

/// `postgres unrestricted` is one grant on the default store, and the named
/// form is the same grant on another.
#[test]
fn unrestricted_is_a_view_level_grant() {
    let grant = grant("postgres unrestricted");
    assert!(grant.is_unrestricted());
    assert!(grant.queries().is_empty());
    assert!(grant.tables().is_empty());

    let document = "postgres $DATABASE_URL\npostgres analytics $ANALYTICS_URL\n\n\
                    view v \"/v\" {\n  postgres analytics unrestricted\n}\n";
    let manifest = Manifest::from_blastfile_str(document).expect("lowers");
    let route = manifest.route(0).expect("one route");
    assert!(route.grant(&ProxyName::from(STORE)).is_none());
    let grant = route
        .grant(&ProxyName::from("analytics"))
        .expect("the named store")
        .as_postgresql()
        .expect("a postgresql grant");
    assert!(grant.is_unrestricted());
}

/// A grant that names a store nothing declares is `GrantOnUnknownProxy`, naming
/// the view and the store (§13.1).
#[test]
fn a_grant_on_an_undeclared_store_is_refused() {
    let rendered = refusal("postgres analytics { select products }");
    assert!(
        rendered.contains("grant on unknown proxy \"analytics\""),
        "{rendered}"
    );
    assert!(rendered.contains("view \"v\""), "{rendered}");
    // The same row on the default store, when the file declares no store.
    let errors = Manifest::parse_blastfile("view v \"/v\" {\n  postgres { select a }\n}\n", 20_000)
        .expect_err("the document is refused");
    let rendered = errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(
        rendered.contains("grant on unknown proxy \"db\""),
        "{rendered}"
    );
}

/// A grant on a store declared *after* the view still resolves: step 2 builds
/// the store index before step 4 reads the views (§7.3).
#[test]
fn a_grant_may_name_a_store_declared_later() {
    let document = "view v \"/v\" {\n  postgres { select products }\n}\n\n\
                    postgres $DATABASE_URL\n";
    let manifest = Manifest::from_blastfile_str(document).expect("lowers");
    let route = manifest.route(0).expect("one route");
    let grant = route
        .grant(&ProxyName::from(STORE))
        .expect("the store declared below the view");
    assert!(
        grant
            .as_postgresql()
            .expect("a postgresql grant")
            .tables()
            .tables()
            .contains_key("products")
    );
}

/// Two `postgres` blocks for one store in one view are refused by the parser
/// (§6.7), so a view never silently keeps only the second one.
#[test]
fn two_grants_for_one_store_are_refused() {
    let rendered = refusal("postgres { select a }\n  postgres { select b }");
    assert!(
        rendered.contains("`postgres` is declared once per store in a view"),
        "{rendered}"
    );
}

/// A `postgres unrestricted` block is not in the grammar: the refusal is the
/// statement-terminator row, and nothing permissive is produced (§13.1).
#[test]
fn unrestricted_takes_no_block() {
    let rendered = refusal("postgres unrestricted { select products }");
    assert!(
        rendered.contains("expected a newline, `;` or `}`"),
        "{rendered}"
    );
    assert!(
        Manifest::from_blastfile_str(&document("postgres unrestricted { select products }"))
            .is_err()
    );
}

/// The grant survives `effective_grants` unchanged: one proxy, one grant.
#[test]
fn effective_grants_read_the_same_strings() {
    let manifest = lowered("postgres { transactions; select products(id) }");
    let effective = manifest.effective_grants(0);
    let grant: &Grant = effective.get(&ProxyName::from(STORE)).expect("the grant");
    let grant = grant.as_postgresql().expect("a postgresql grant");
    assert!(grant.transaction_control());
    assert!(grant.tables().tables().contains_key("products"));
}
