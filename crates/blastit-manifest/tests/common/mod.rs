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

/// The example manifest from ARCHITECTURE.md §6, verbatim.
pub const SECTION_6: &str = r#"version = 1
groups = ["session"]                # attached to every route (top-level keys come before any [table])

[proxy.db]                          # see §5 for proxies and their variables
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }

# Connection setup, plus what middleware does on every request.
[group.session.db]                  # grant on proxy "db" (§5)
transaction_control = true          # BEGIN/COMMIT/ROLLBACK, SAVEPOINT/RELEASE sp_*
queries = [
  "SET TIME ZONE 'UTC'",
  'SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $1 AND "django_session"."session_key" = $2) LIMIT 21',
]

[group.catalog.db]
queries = [
  'SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $1 LIMIT 21',
]

[[route]]
name = "product-detail"
path = ['/products/[0-9]+/']        # full match against PATH_INFO
methods = ["GET", "HEAD"]           # optional; default: any method
groups = ["catalog"]                # shared with checkout; nothing else needed here
prefork = true                      # hot route: fork at startup, skip first-hit fork latency

[[route]]
name = "checkout"
path = ['/checkout/.*']
groups = ["catalog"]
egress = true                       # may open TCP connections (payment API); see §7.3
[route.db]                          # inline grants, only for this route
queries = [
  'INSERT INTO "shop_order" ("product_id", "qty") VALUES ($1, $2) RETURNING "shop_order"."id"',
]

[[route]]
name = "about"
path = ['/about/']
[route.db]                          # inline only: no group involved
queries = ['SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $1 LIMIT 21']

[[route]]
name = "fallback"                   # catch-all: the app renders its own 404s
path = ['.*']                       # without it, unmatched paths get blastit's own 404
                                    # only the global groups; add grants if error pages need the DB
"#;

/// The two-proxy example from ARCHITECTURE.md §5.1, plus the `version` line a
/// whole manifest needs; §5.1 only shows the proxy sections.
pub const SECTION_5_1: &str = r#"version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }
# No output table: DATABASE_URL is rewritten in place, DATABASE_CA_CERT disappears.

[proxy.analytics]
protocol = "postgresql"
input = { url = "ANALYTICS_URL" }
[proxy.analytics.output]                # the app wants split variables for this one
ANALYTICS_DB_HOST = "{{ socket_dir }}"
ANALYTICS_DB_PORT = "{{ port }}"
ANALYTICS_DB_NAME = "{{ database }}"
ANALYTICS_DB_USER = "{{ user }}"
"#;

/// The §6 example, parsed.
pub fn section_6() -> Manifest {
    Manifest::from_toml_str(SECTION_6).expect("§6 example should parse")
}

/// The §5.1 example, parsed.
pub fn section_5_1() -> Manifest {
    Manifest::from_toml_str(SECTION_5_1).expect("§5.1 example should parse")
}

/// Parses a document that must not parse, and returns its problems.
pub fn parse_errors(source: &str) -> ManifestErrors {
    Manifest::from_toml_str(source).expect_err("manifest should not parse")
}

/// Validates a document that parses but must not validate.
pub fn validate_errors(source: &str, base: u32) -> ManifestErrors {
    let manifest = Manifest::from_toml_str(source).expect("manifest should parse");
    manifest
        .validate(base)
        .expect_err("manifest should not validate")
}

/// Validates a document and returns its warnings.
pub fn validate_warnings(source: &str) -> Vec<ManifestWarning> {
    let manifest = Manifest::from_toml_str(source).expect("manifest should parse");
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
