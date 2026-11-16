//! Fixtures shared by the integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Manifest, ManifestErrors, ManifestWarning, UidLayout};

/// Borrows a list of manifest strings for comparison with literal arrays.
pub fn texts(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

/// Borrows a string map for comparison with literal arrays.
pub fn text_map(map: &std::collections::BTreeMap<String, String>) -> Vec<(&str, &str)> {
    map.iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

/// The application of ARCHITECTURE.md §6, written as a `Blastfile` (§7.2).
///
/// It is the same four views and the same grants as the section's example: one
/// store, the shared `session` grant set, the shared `catalog` one, one inline
/// grant per view, `prefork` on the hot view and `egress any` on checkout.
///
/// Two rows of the example cannot be carried over, and neither is a way of
/// writing this file (§7.1): the example attaches `session` to every route with
/// a top-level `groups` list, but a `Blastfile` has no spelling for "attach this
/// trait to every view", so each view that wants `session` writes `use session`
/// and `fallback` — the view the example leaves without grants — writes none.
/// The other dropped row is the store's `ca_cert` slot, because `url` is the
/// one-line form only (§7.4).
pub const SECTION_6: &str = r#"app "shop"

postgres $DATABASE_URL

# Connection setup, plus what middleware does on every request.
trait session {
    postgres {
        transactions
        query "SET TIME ZONE 'UTC'"
        query 'SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $1 AND "django_session"."session_key" = $2) LIMIT 21'
    }
}

trait catalog {
    postgres {
        query 'SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $1 LIMIT 21'
    }
}

# Full match against PATH_INFO; the regex form is the one the section writes.
view product-detail GET regex '/products/[0-9]+/' {
    use session, catalog
    prefork
}

view checkout regex '/checkout/.*' {
    use session, catalog
    egress any
    postgres {
        query 'INSERT INTO "shop_order" ("product_id", "qty") VALUES ($1, $2) RETURNING "shop_order"."id"'
    }
}

view about "/about/" {
    use session
    postgres {
        query 'SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $1 LIMIT 21'
    }
}

# Catch-all: the app renders its own 404s. It writes no `use`: a view that does
# not ask for a trait inherits nothing (§7.1).
view fallback regex '.*' {
}
"#;

/// The two stores of ARCHITECTURE.md §5.1, as a `Blastfile` (§7.1, §7.4, §7.5).
///
/// §5.1's `db` also carried `ca_cert` and its `analytics` carried an `output`
/// map. Neither has a spelling in this language: `url` is the one-line store
/// form (§7.4), so a store cannot hold it beside a second slot, and the `output`
/// map is dropped outright (§7.5, `ADR-0012`). The fixture therefore carries the
/// one-line form the language has, and the model's `output` maps are asserted
/// empty rather than absent.
pub const SECTION_5_1: &str = r#"postgres $DATABASE_URL

postgres analytics $ANALYTICS_URL
"#;

/// The §6 application, parsed.
pub fn section_6() -> Manifest {
    Manifest::from_blastfile_str(SECTION_6).expect("§6 example should parse")
}

/// The §5.1 stores, parsed.
pub fn section_5_1() -> Manifest {
    Manifest::from_blastfile_str(SECTION_5_1).expect("§5.1 example should parse")
}

/// Parses a document that must be refused at parse or lower time, and returns
/// its problems.
pub fn parse_errors(source: &str) -> ManifestErrors {
    Manifest::from_blastfile_str(source).expect_err("manifest should not parse")
}

/// Validates a document that parses but must not validate.
pub fn validate_errors(source: &str, base: u32) -> ManifestErrors {
    let manifest = Manifest::from_blastfile_str(source).expect("manifest should parse");
    manifest
        .validate(base)
        .expect_err("manifest should not validate")
}

/// Validates a document and returns its warnings.
pub fn validate_warnings(source: &str) -> Vec<ManifestWarning> {
    let manifest = Manifest::from_blastfile_str(source).expect("manifest should parse");
    manifest
        .validate(UidLayout::DEFAULT_BASE)
        .expect("manifest should validate")
}

// ---- the Blastfile fixture corpus (§20 R8) ------------------------------

/// The fixture directory: one flat directory of `*.blastfile` documents, one
/// per row of §7.1/§7.2.
///
/// It is flat on purpose. §20 makes it the round-trip corpus `MB.21.3.4`
/// (P1–P3), `MB.21.3.6` (fence counts) and `MB.21.5.1` (docs) read, and each of
/// those globs `fixtures/*` and asserts a count — a directory per row would make
/// every one of them recurse and would lose the "15 fixtures" number §20 pins.
///
/// The expected model for a fixture is **not** a file: each test builds it field
/// by field, so both sides of the parity oracle are readable in one place and
/// neither comes from a document.
pub fn fixture_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture, as `(file name, source)`, in file-name order.
pub fn corpus() -> Vec<(String, String)> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(fixture_directory())
        .expect("the fixture directory is readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "blastfile")
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
            (name, source)
        })
        .collect()
}

/// One fixture's source, by file name.
pub fn fixture(name: &str) -> String {
    let path = fixture_directory().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()))
}

/// How many fixtures the corpus holds.
///
/// §20 pins the number: the corpus is the 15 rows of §7.1/§7.2, and a later
/// leaf that adds a file has to say so here as well.
pub const CORPUS_SIZE: usize = 15;
