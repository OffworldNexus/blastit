//! §8.1's pattern table and §8.3's refusals, through the public entry point
//! (`R9`).
//!
//! The table is proved the way a request is: the lowered `Route.path` goes
//! through the **existing** [`RoutingTable`], so a fragment that is wrong in a
//! way the compiler accepts still fails to match the path it should.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Manifest, RoutingTable};

/// The routing table one view header lowers to.
fn table(header: &str) -> RoutingTable {
    let source = format!("view v {header} {{\n}}\n");
    let manifest = Manifest::from_blastfile_str(&source)
        .unwrap_or_else(|error| panic!("{header} lowers: {error}"));
    RoutingTable::new(&manifest).unwrap_or_else(|error| panic!("{header} compiles: {error}"))
}

/// Whether `path` reaches the view whose header is `header`.
fn matches(header: &str, path: &str) -> bool {
    table(header).route("GET", path).is_some()
}

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::from_blastfile_str(source).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// A literal path matches exactly that path (§8.1 row 1).
#[test]
fn a_literal_path_matches_exactly() {
    assert!(matches("\"/products\"", "/products"));
    assert!(!matches("\"/products\"", "/product"));
    assert!(!matches("\"/products\"", "/products/1"));
    assert!(!matches("\"/products\"", "/products/"));
}

/// A `.` in a literal path is a literal dot (§8.1 row 2).
#[test]
fn a_dot_is_a_literal_dot() {
    assert!(matches("\"/v1.0/x\"", "/v1.0/x"));
    assert!(!matches("\"/v1.0/x\"", "/v1x0/x"));
    assert!(!matches("\"/v1.0/x\"", "/v1.00/x"));
}

/// `{id}` is one non-empty segment (§8.1 row 3).
#[test]
fn a_parameter_is_one_segment() {
    assert!(matches("\"/products/{id}\"", "/products/7"));
    assert!(matches("\"/products/{id}\"", "/products/abc"));
    assert!(!matches("\"/products/{id}\"", "/products/"));
    assert!(!matches("\"/products/{id}\"", "/products/7/x"));
}

/// `{id:int}` is digits only (§8.1 row 4).
#[test]
fn an_int_parameter_is_digits() {
    assert!(matches("\"/products/{id:int}\"", "/products/12"));
    assert!(!matches("\"/products/{id:int}\"", "/products/12a"));
    assert!(!matches("\"/products/{id:int}\"", "/products/a12"));
    assert!(!matches("\"/products/{id:int}\"", "/products/4.2"));
}

/// `{id:uuid}` is the canonical UUID form (§8.1 row 5).
#[test]
fn a_uuid_parameter_is_a_uuid() {
    assert!(matches(
        "\"/orders/{id:uuid}\"",
        "/orders/123e4567-e89b-12d3-a456-426614174000"
    ));
    for near_miss in [
        "/orders/123e4567-e89b-12d3-a456-42661417400",
        "/orders/123e4567e89b12d3a456426614174000",
        "/orders/123e4567-e89b-12d3-a456-4266141740000",
        "/orders/not-a-uuid",
    ] {
        assert!(!matches("\"/orders/{id:uuid}\"", near_miss), "{near_miss}");
    }
}

/// A trailing `*` matches the prefix and everything after it, and never a
/// sibling that only shares the characters (§8.1 row 6).
#[test]
fn a_trailing_star_matches_a_nested_path() {
    assert!(matches("\"/assets/*\"", "/assets/a/b.css"));
    assert!(matches("\"/assets/*\"", "/assets/"));
    assert!(!matches("\"/assets/*\"", "/assets"));
    assert!(!matches("\"/assets/*\"", "/assets-secret/key.pem"));
}

/// `/*` matches every path (§8.1 row 7).
#[test]
fn a_star_alone_matches_everything() {
    assert!(matches("\"/*\"", "/"));
    assert!(matches("\"/*\"", "/anything/at/all"));
}

/// A bare `/x` is not a prefix (§8.1 row 9).
#[test]
fn a_bare_path_is_not_a_prefix() {
    assert!(matches("\"/x\"", "/x"));
    assert!(!matches("\"/x\"", "/xy"));
    assert!(!matches("\"/x\"", "/x/y"));
}

/// A `regex` is kept verbatim and anchored whole (§8.1 row 8).
#[test]
fn a_regex_is_verbatim_and_anchored() {
    assert!(matches("regex '/v[0-9]+/items'", "/v12/items"));
    assert!(!matches("regex '/v[0-9]+/items'", "/v12/items/1"));
    assert!(!matches("regex '/v[0-9]+/items'", "/v/items"));
    // The escape hatch keeps what the quoted form cannot express.
    assert!(matches("regex '/a\\{b\\}'", "/a{b}"));
}

/// `GET` implies `HEAD` before the table sees the route (§8.4).
#[test]
fn get_answers_head() {
    let table = table("GET \"/products\"");
    assert!(table.route("GET", "/products").is_some());
    assert!(table.route("HEAD", "/products").is_some());
    assert!(table.route("POST", "/products").is_none());
}

/// §8.3's rows, each at the pattern's own `line:col`.
#[test]
fn the_pattern_refusals() {
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
            "view v \"/x/{Id}\" {\n}\n",
            "1:8: `{Id}` is not a parameter name; names start with a lower-case letter and are `[a-z][a-z0-9_]*`",
        ),
        (
            "view v \"/x/{id:str}\" {\n}\n",
            "1:8: `{id:str}` is not a pattern type; the types are `int` and `uuid`",
        ),
        (
            "view v \"/x/*/y\" {\n}\n",
            "1:8: `*` is only valid at the end of a pattern; write `/x/*`",
        ),
        (
            "view v \"/x/{a}/{a:int}\" {\n}\n",
            "1:8: the parameter `{a}` appears twice; a pattern names each parameter once",
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
}

/// A pattern over 256 bytes is refused in both spellings (§4.5, §8.3).
#[test]
fn a_pattern_is_capped_at_256_bytes() {
    // The cap counts the pattern's bytes, without its quotes.
    let legal = format!("\"/{}\"", "a".repeat(255));
    assert_eq!(legal.len() - 2, 256, "256 bytes of pattern");
    assert!(Manifest::from_blastfile_str(&format!("view v {legal} {{\n}}\n")).is_ok());

    let long = format!("\"/{}\"", "a".repeat(256));
    assert_eq!(long.len() - 2, 257, "257 bytes of pattern");
    assert_eq!(
        refusal(&format!("view v {long} {{\n}}\n")),
        "1:8: a pattern holds at most 256 bytes"
    );

    let long = format!("regex '/{}'", "a".repeat(256));
    assert_eq!(
        refusal(&format!("view v {long} {{\n}}\n")),
        "1:14: a pattern holds at most 256 bytes"
    );
}

/// A `regex` the crate refuses is a clean refusal with the crate's own message
/// (§8.3, §17.4).
#[test]
fn a_regex_the_crate_refuses_is_a_clean_refusal() {
    let rendered = refusal("view v regex '/a(?=b)/' {\n}\n");
    assert!(
        rendered.starts_with("1:14: regex `/a(?=b)/` does not compile: "),
        "{rendered}"
    );
    assert!(rendered.contains("look-around"), "{rendered}");
    // An unbalanced `)` cannot be rescued by the wrapper into a catch-all.
    let rendered = refusal("view v regex '/x)|(.*' {\n}\n");
    assert!(
        rendered.starts_with("1:14: regex `/x)|(.*` does not compile: "),
        "{rendered}"
    );
    assert!(Manifest::from_blastfile_str("view v regex '/x)|(.*' {\n}\n").is_err());
}
