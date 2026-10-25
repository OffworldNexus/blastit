//! Effective grants: the union of global groups, route groups and inline
//! grants, per proxy (§6).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;

use blastit_manifest::{Grant, Manifest, PgGrant, ProxyName};

use common::{section_6, texts};

const SET_TIME_ZONE: &str = "SET TIME ZONE 'UTC'";
const SESSION_SELECT: &str = r#"SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $1 AND "django_session"."session_key" = $2) LIMIT 21"#;
const CATALOG_SELECT: &str = r#"SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $1 LIMIT 21"#;
const CHECKOUT_INSERT: &str =
    r#"INSERT INTO "shop_order" ("product_id", "qty") VALUES ($1, $2) RETURNING "shop_order"."id""#;
const ABOUT_SELECT: &str =
    r#"SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $1 LIMIT 21"#;

/// The one grant in a single-proxy result, narrowed to `postgresql`.
fn db_grant(grants: BTreeMap<ProxyName, Grant>) -> PgGrant {
    assert_eq!(grants.len(), 1, "only proxy \"db\": {grants:?}");
    let (name, grant) = grants.into_iter().next().expect("proxy db");
    assert_eq!(name, ProxyName::from("db"));
    grant.into_postgresql().expect("a postgresql grant")
}

#[test]
fn product_detail_unions_the_global_group_and_its_own_group() {
    let manifest = section_6();
    let grants = manifest.effective_grants(0);
    let db = db_grant(grants);
    assert_eq!(
        texts(db.queries()),
        [SET_TIME_ZONE, SESSION_SELECT, CATALOG_SELECT],
        "global groups first, then the route's groups"
    );
    assert!(
        db.transaction_control(),
        "session turns transaction control on"
    );
}

#[test]
fn checkout_adds_its_inline_grant_last() {
    let manifest = section_6();
    let db = db_grant(manifest.effective_grants(1));
    assert_eq!(
        texts(db.queries()),
        [
            SET_TIME_ZONE,
            SESSION_SELECT,
            CATALOG_SELECT,
            CHECKOUT_INSERT
        ],
        "inline grants come after the groups"
    );
    assert!(db.transaction_control());
}

#[test]
fn about_has_only_the_global_group_and_its_inline_grant() {
    let manifest = section_6();
    let db = db_grant(manifest.effective_grants(2));
    assert_eq!(
        texts(db.queries()),
        [SET_TIME_ZONE, SESSION_SELECT, ABOUT_SELECT]
    );
    assert!(db.transaction_control());
}

#[test]
fn fallback_gets_the_global_groups_only() {
    let manifest = section_6();
    let db = db_grant(manifest.effective_grants(3));
    assert_eq!(
        texts(db.queries()),
        [SET_TIME_ZONE, SESSION_SELECT],
        "the top-level groups reach every route, fallback included"
    );
    assert!(db.transaction_control());
}

#[test]
fn an_out_of_range_route_has_no_grants() {
    let manifest = section_6();
    assert!(manifest.effective_grants(4).is_empty());
    assert!(manifest.effective_grants(usize::MAX).is_empty());
}

#[test]
fn queries_are_deduplicated_across_contributors() {
    let source = "version = 1\n\
                  groups = [\"g1\"]\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.g1.db]\nqueries = [\"SELECT 1\"]\n\
                  [group.g2.db]\nqueries = [\"SELECT 1\", \"SELECT 2\"]\n\
                  [[route]]\nname = \"r\"\npath = ['.*']\ngroups = [\"g2\"]\n\
                  [route.db]\nqueries = [\"SELECT 2\", \"SELECT 3\"]\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let db = db_grant(manifest.effective_grants(0));
    assert_eq!(
        texts(db.queries()),
        ["SELECT 1", "SELECT 2", "SELECT 3"],
        "the first occurrence wins, later duplicates are dropped"
    );
    assert!(!db.transaction_control());
}

#[test]
fn transaction_control_is_or_ed() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.session.db]\ntransaction_control = true\n\
                  [[route]]\nname = \"from-group\"\npath = ['/a']\ngroups = [\"session\"]\n\
                  [route.db]\ntransaction_control = false\n\
                  [[route]]\nname = \"from-inline\"\npath = ['/b']\n\
                  [route.db]\ntransaction_control = true\nqueries = [\"SELECT 1\"]\n\
                  [[route]]\nname = \"from-nothing\"\npath = ['/c']\n\
                  [route.db]\ntransaction_control = false\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");

    let db = db_grant(manifest.effective_grants(0));
    assert!(
        db.transaction_control(),
        "the group wins over the inline false"
    );
    assert!(db.queries().is_empty());

    let db = db_grant(manifest.effective_grants(1));
    assert!(db.transaction_control());
    assert_eq!(texts(db.queries()), ["SELECT 1"]);

    assert!(
        manifest.effective_grants(2).is_empty(),
        "no queries and no transaction control: the proxy is absent, so no listener (§2.2)"
    );
}

#[test]
fn an_empty_grant_leaves_the_proxy_out() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [[route]]\nname = \"quiet\"\npath = ['.*']\n\
                  [route.db]\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    assert!(manifest.effective_grants(0).is_empty());
}

#[test]
fn a_group_that_does_not_exist_contributes_nothing() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.real.db]\nqueries = [\"SELECT 1\"]\n\
                  [[route]]\nname = \"r\"\npath = ['.*']\ngroups = [\"ghost\", \"real\"]\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let db = db_grant(manifest.effective_grants(0));
    assert_eq!(
        texts(db.queries()),
        ["SELECT 1"],
        "validation reports the unknown group; merging just skips it"
    );
}

#[test]
fn merge_from_is_the_union() {
    let mut left = PgGrant::new(vec!["SELECT 1".to_string()], false);
    let right = PgGrant::new(vec!["SELECT 1".to_string(), "SELECT 2".to_string()], true);
    left.merge_from(&right);
    assert_eq!(texts(left.queries()), ["SELECT 1", "SELECT 2"]);
    assert!(left.transaction_control());
    assert!(!left.is_empty());

    assert!(PgGrant::default().is_empty());
    assert!(PgGrant::new(vec![], false).is_empty());
    assert!(!PgGrant::new(vec!["SELECT 1".to_string()], false).is_empty());
    assert!(!PgGrant::new(vec![], true).is_empty());
}

//
// Table and column grants (§6, D7).
//

use blastit_manifest::{Columns, TableGrant, TableGrants};

fn table(select: Option<Columns>) -> TableGrant {
    TableGrant {
        select,
        ..TableGrant::default()
    }
}

#[test]
fn table_grants_merge_across_groups_and_the_route() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.catalog.db.tables]\n\
                  shop_product = { select = [\"id\", \"name\"] }\n\
                  [group.catalog.db]\nfunctions = [\"unaccent\"]\n\
                  [[route]]\nname = \"product-detail\"\npath = ['/products/.*']\n\
                  groups = [\"catalog\"]\n\
                  [route.db.tables.shop_product]\nselect = [\"slug\"]\n\
                  [route.db.tables.shop_order]\ninsert = \"*\"\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let db = db_grant(manifest.effective_grants(0));
    let tables = db.tables();
    assert_eq!(
        tables.table("shop_product").unwrap().select,
        Some(Columns::list(["id", "name", "slug"])),
        "the route's columns are added to the group's"
    );
    assert_eq!(
        tables.table("shop_order").unwrap().insert,
        Some(Columns::All)
    );
    assert_eq!(Vec::from_iter(tables.functions()), ["unaccent"]);
}

#[test]
fn a_star_absorbs_a_column_list_when_grants_merge() {
    let mut left = PgGrant::new(vec![], false).with_tables(TableGrants::new(
        [("t".to_string(), table(Some(Columns::list(["id"]))))],
        [],
    ));
    let right = PgGrant::new(vec![], false).with_tables(TableGrants::new(
        [("t".to_string(), table(Some(Columns::All)))],
        ["f".to_string()],
    ));
    left.merge_from(&right);
    assert_eq!(left.tables().table("t").unwrap().select, Some(Columns::All));
    assert_eq!(Vec::from_iter(left.tables().functions()), ["f"]);

    let mut left = PgGrant::default();
    left.merge_from(&right);
    assert_eq!(left.tables().table("t").unwrap().select, Some(Columns::All));
}

#[test]
fn a_grant_of_nothing_but_tables_is_not_empty() {
    // A route with table grants needs a dbproxy listener, or its statements
    // would never reach the analysis at all (§2.2).
    let tables = PgGrant::new(vec![], false).with_tables(TableGrants::new(
        [("t".to_string(), table(Some(Columns::All)))],
        [],
    ));
    assert!(!tables.is_empty());

    let functions = PgGrant::new(vec![], false).with_tables(TableGrants::new(
        std::iter::empty(),
        ["unaccent".to_string()],
    ));
    assert!(!functions.is_empty());

    // An entry that grants no operation of a table grants nothing.
    let nothing = PgGrant::new(vec![], false).with_tables(TableGrants::new(
        [("t".to_string(), TableGrant::default())],
        [],
    ));
    assert!(nothing.is_empty());
}
