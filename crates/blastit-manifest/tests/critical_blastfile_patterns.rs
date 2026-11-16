//! §8's pattern guarantees under a hostile corpus (`R9`).
//!
//! A pattern is policy, not input, but a pattern that matches more than its
//! author wrote is a capability nobody declared: `/assets/*` must never answer
//! `/assets-secret`, a literal must never become a prefix, and a `regex` the
//! crate refuses must never be rescued by the anchoring wrapper into a
//! catch-all (§8.2, §17.3, §17.4).
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

/// A pattern never widens into a sibling that only shares its characters.
#[test]
fn a_pattern_cannot_widen_into_a_sibling() {
    let corpus: &[(&str, &str)] = &[
        ("\"/assets/*\"", "/assets-secret/key.pem"),
        ("\"/assets/*\"", "/assetsX/a.css"),
        ("\"/x\"", "/xy"),
        ("\"/x\"", "/x/y"),
        ("\"/products\"", "/products/1"),
        ("\"/v1.0/x\"", "/v1x0/x"),
        (
            "\"/orders/{id:uuid}\"",
            "/orders/123e4567-e89b-12d3-a456-426614174000/x",
        ),
    ];
    for (header, path) in corpus {
        assert!(!matches(header, path), "{header} matched {path}");
    }
}

/// A literal pattern never matches a traversal variant of itself: the resolver
/// refuses `..` before routing (§9.2), and the pattern cannot be talked into
/// matching one either.
#[test]
fn a_literal_pattern_does_not_match_a_traversal() {
    for (header, path) in [
        ("\"/assets/app.js\"", "/assets/../app.js"),
        ("\"/assets/app.js\"", "/assets/./app.js"),
        ("\"/assets/app.js\"", "/assets//app.js"),
        ("\"/admin\"", "/admin/../public"),
        ("\"/admin\"", "/public/../admin"),
    ] {
        assert!(!matches(header, path), "{header} matched {path}");
    }
    // The catch-all does match them, which is exactly why the resolver's
    // `..`/`//` refusal runs before any view is consulted (§9.2).
    assert!(matches("\"/*\"", "/admin/../public"));
}

/// A parameter emits one fixed fragment, so it can never become a character
/// class or a quantifier of its own (§8.2, §17.3).
#[test]
fn a_parameter_emits_only_its_fixed_fragment() {
    // `{id}` is `[^/]+`: one non-empty segment, never a `/`.
    assert!(!matches("\"/p/{id}\"", "/p/a/b"));
    assert!(!matches("\"/p/{id}\"", "/p/"));
    // `{id:int}` is `[0-9]+`: digits only, never a sign or a decimal point.
    assert!(!matches("\"/p/{id:int}\"", "/p/-1"));
    assert!(!matches("\"/p/{id:int}\"", "/p/1.5"));
    // A literal `{` cannot be written in the quoted form at all, so it cannot
    // smuggle a group in.
    assert_eq!(
        refusal("view v \"/a{b\" {\n}\n"),
        "1:8: the parameter `{b` is not closed; write `{id:int}`"
    );
}

/// A pattern one byte over the 4 KiB cap is refused in both spellings (§4.5,
/// §8.3).
#[test]
fn a_4097_byte_pattern_is_refused() {
    let long = format!("\"/{}\"", "a".repeat(4_096));
    assert_eq!(
        refusal(&format!("view v {long} {{\n}}\n")),
        "1:8: a pattern holds at most 4096 bytes"
    );
    let long = format!("regex '/{}'", "a".repeat(4_096));
    assert_eq!(
        refusal(&format!("view v {long} {{\n}}\n")),
        "1:14: a pattern holds at most 4096 bytes"
    );
}

/// An unbalanced `)` is refused on its own, never rescued by the wrapper into a
/// catch-all (§8.1, §17.4).
#[test]
fn an_unbalanced_close_paren_is_refused() {
    for pattern in ["/x)|(.*", "/(a", "/(a))", "/a{2,1}"] {
        let source = format!("view v regex '{pattern}' {{\n}}\n");
        let rendered = refusal(&source);
        assert!(
            rendered.starts_with(&format!("1:14: regex `{pattern}` does not compile: ")),
            "{rendered}"
        );
        assert!(!rendered.contains('\n'), "{rendered}");
    }
}

/// A `GET` view answers `HEAD`, and the implication is in `methods` before the
/// table sees the route (§8.4).
#[test]
fn head_is_in_the_methods_of_a_get_view() {
    let manifest = Manifest::from_blastfile_str("view v GET \"/x\" {\n}\n").expect("lowers");
    let route = manifest.route(0).expect("one route");
    assert_eq!(
        route.methods(),
        Some(["GET".to_owned(), "HEAD".to_owned()].as_slice())
    );
    let table = RoutingTable::new(&manifest).expect("compiles");
    assert!(table.route("GET", "/x").is_some());
    assert!(table.route("HEAD", "/x").is_some());
    assert!(table.route("POST", "/x").is_none());
}

/// A method-less view answers every method the front end lets through, and
/// `CONNECT`/`TRACE` can never be declared (§8.4).
#[test]
fn a_method_less_view_has_no_list() {
    let manifest = Manifest::from_blastfile_str("view v \"/x\" {\n}\n").expect("lowers");
    assert_eq!(manifest.route(0).and_then(|route| route.methods()), None);
    for method in ["CONNECT", "TRACE"] {
        let source = format!("view v {method} \"/x\" {{\n}}\n");
        assert_eq!(
            refusal(&source),
            format!(
                "1:8: `{method}` cannot reach a view; Blastit answers `GET`/`HEAD` static files and forwards the methods you declare"
            )
        );
    }
}
