//! Effective grants: the union of the traits a view `use`s and its own inline
//! grants, per store (§6, §11.1).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;

use blastit_manifest::{Grant, Manifest, ManifestError, PgGrant, ProxyName};

use common::{parse_errors, section_6, texts};

// A `query` is stored through `blastit_sqlpolicy::normalize`, which writes every
// bind placeholder as `$?` (§13.1), so the expected text below is the normalised
// spelling and not the `$1` a document writes.
const SET_TIME_ZONE: &str = "SET TIME ZONE 'UTC'";
const SESSION_SELECT: &str = r#"SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $? AND "django_session"."session_key" = $?) LIMIT 21"#;
const CATALOG_SELECT: &str = r#"SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $? LIMIT 21"#;
const CHECKOUT_INSERT: &str =
    r#"INSERT INTO "shop_order" ("product_id", "qty") VALUES ($?, $?) RETURNING "shop_order"."id""#;
const ABOUT_SELECT: &str =
    r#"SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $? LIMIT 21"#;

/// The one grant in a single-proxy result, narrowed to `postgresql`.
fn db_grant(grants: BTreeMap<ProxyName, Grant>) -> PgGrant {
    assert_eq!(grants.len(), 1, "only proxy \"db\": {grants:?}");
    let (name, grant) = grants.into_iter().next().expect("proxy db");
    assert_eq!(name, ProxyName::from("db"));
    grant.into_postgresql().expect("a postgresql grant")
}

#[test]
fn product_detail_unions_the_traits_it_uses() {
    let manifest = section_6();
    let db = db_grant(manifest.effective_grants(0));
    assert_eq!(
        texts(db.queries()),
        [SET_TIME_ZONE, SESSION_SELECT, CATALOG_SELECT],
        "`use session, catalog` contributes in written order"
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
        "inline grants come after the traits"
    );
    assert!(db.transaction_control());
}

#[test]
fn about_has_only_its_trait_and_its_inline_grant() {
    let manifest = section_6();
    let db = db_grant(manifest.effective_grants(2));
    assert_eq!(
        texts(db.queries()),
        [SET_TIME_ZONE, SESSION_SELECT, ABOUT_SELECT]
    );
    assert!(db.transaction_control());
}

#[test]
fn a_view_that_writes_no_use_inherits_no_trait() {
    // The retired format could attach a group to every route at once. A
    // `Blastfile` cannot (§7.1, `ADR-0012`): the only attach is `use <trait>`,
    // so the catch-all — which asks for nothing — receives nothing, even though
    // the same file's other views use `session` and `catalog`.
    let manifest = section_6();
    assert!(
        manifest.effective_grants(3).is_empty(),
        "a view without `use` inherits nothing: {:?}",
        manifest.effective_grants(3)
    );
    assert!(
        manifest.global_groups().is_empty(),
        "no document can fill the global list"
    );
}

#[test]
fn an_out_of_range_route_has_no_grants() {
    let manifest = section_6();
    assert!(manifest.effective_grants(4).is_empty());
    assert!(manifest.effective_grants(usize::MAX).is_empty());
}

#[test]
fn queries_are_deduplicated_across_contributors() {
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait g1 {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  trait g2 {\n    postgres {\n        query 'SELECT 1'\n        query 'SELECT 2'\n    }\n}\n\
                  \n\
                  view r regex '.*' {\n    use g1, g2\n    postgres {\n        query 'SELECT 2'\n        query 'SELECT 3'\n    }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("parses");
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
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait session {\n    postgres {\n        transactions\n    }\n}\n\
                  \n\
                  view from-trait \"/a\" {\n    use session\n}\n\
                  \n\
                  view from-inline \"/b\" {\n    postgres {\n        transactions\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  view from-nothing \"/c\" {\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("parses");

    let db = db_grant(manifest.effective_grants(0));
    assert!(db.transaction_control(), "the trait turns it on");
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
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  view quiet regex '.*' {\n    postgres {\n    }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("parses");
    assert!(
        manifest
            .route(0)
            .is_some_and(|route| !route.grants().is_empty()),
        "the empty block is still a grant entry"
    );
    assert!(manifest.effective_grants(0).is_empty());
}

#[test]
fn a_use_that_names_no_trait_is_refused() {
    // The retired format's unknown group was a validation finding that merging
    // silently skipped. A `use` is resolved while lowering (§11.2), so the file
    // is refused before any merge can skip it.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait real {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  view r regex '.*' {\n    use ghost, real\n}\n";
    let errors = parse_errors(source);
    assert_eq!(errors.len(), 1, "{errors}");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::UnknownTrait { view, name, .. } if view == "r" && name == "ghost"
        ),
        "{errors}"
    );
    assert!(errors.to_string().contains("names no trait"), "{errors}");

    // The allowed twin: the same view naming only the trait that exists.
    let manifest = Manifest::from_blastfile_str(&source.replace("use ghost, real", "use real"))
        .expect("lowers");
    let db = db_grant(manifest.effective_grants(0));
    assert_eq!(texts(db.queries()), ["SELECT 1"]);
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
fn table_grants_merge_across_a_trait_and_the_view() {
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait catalog {\n    postgres {\n        select shop_product(id, name)\n        functions unaccent\n    }\n}\n\
                  \n\
                  view product-detail regex '/products/.*' {\n    use catalog\n    postgres {\n        select shop_product(slug)\n        insert shop_order\n    }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("parses");
    let db = db_grant(manifest.effective_grants(0));
    let tables = db.tables();
    assert_eq!(
        tables.table("shop_product").unwrap().select,
        Some(Columns::list(["id", "name", "slug"])),
        "the view's columns are added to the trait's"
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
