//! The routing table: full match, manifest order, method filters, rule indices
//! (§2.4, §6).
//!
//! A `Blastfile`'s pattern is compiled by the parser (a `"/x"` through the
//! pattern compiler, a `regex '…'` by the `regex` crate itself, §8.1), so a
//! document that reaches `RoutingTable` always holds rules that compile. The
//! refusals the retired format reported from the table's constructor therefore
//! live at the document's own position now — and they are asserted here, next to
//! the matching they protect, rather than left to the parser's own suite.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{Manifest, ManifestError, Match, RoutingTable};

use common::{SECTION_6, section_6};

/// Compiles a document that is assumed to be valid.
fn routing_table(source: &str) -> RoutingTable {
    let manifest = Manifest::from_blastfile_str(source).expect("parses");
    RoutingTable::new(&manifest).expect("compiles")
}

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> blastit_manifest::ManifestErrors {
    Manifest::from_blastfile_str(source).expect_err("the document is refused")
}

#[test]
fn the_section_6_table_routes_paths_to_routes() {
    let manifest = section_6();
    let table = RoutingTable::new(&manifest).expect("compiles");

    let cases = [
        ("GET", "/products/42/", Some(Match { route: 0, rule: 0 })),
        ("GET", "/products/42", Some(Match { route: 3, rule: 0 })),
        ("GET", "/products/abc/", Some(Match { route: 3, rule: 0 })),
        (
            "GET",
            "/checkout/anything/at/all",
            Some(Match { route: 1, rule: 0 }),
        ),
        ("POST", "/checkout/", Some(Match { route: 1, rule: 0 })),
        ("GET", "/about/", Some(Match { route: 2, rule: 0 })),
        ("GET", "/about/x", Some(Match { route: 3, rule: 0 })),
        ("GET", "/", Some(Match { route: 3, rule: 0 })),
        ("GET", "", Some(Match { route: 3, rule: 0 })),
    ];
    for (method, path, expected) in cases {
        assert_eq!(
            table.route(method, path),
            expected,
            "{method} {path} should route to {expected:?}"
        );
    }
}

#[test]
fn patterns_match_the_whole_path_info() {
    let manifest = section_6();
    let table = RoutingTable::new(&manifest).expect("compiles");
    // '/about/' is anchored, so it neither matches a longer path nor a prefix.
    assert_eq!(
        table.route("GET", "/about/x"),
        Some(Match { route: 3, rule: 0 })
    );
    assert_eq!(
        table.route("GET", "/x/about/"),
        Some(Match { route: 3, rule: 0 })
    );
    // The catch-all is last, so nothing reaches the end without a match.
    assert!(table.route("GET", "/anything").is_some());
}

#[test]
fn methods_are_filtered_and_head_is_implied_by_get() {
    let manifest = section_6();
    let table = RoutingTable::new(&manifest).expect("compiles");

    assert_eq!(
        table.route("GET", "/products/42/"),
        Some(Match { route: 0, rule: 0 })
    );
    assert_eq!(
        table.route("HEAD", "/products/42/"),
        Some(Match { route: 0, rule: 0 }),
        "`GET` in a view header implies `HEAD` when the file is lowered (§8.4)"
    );
    assert_eq!(
        table.route("POST", "/products/42/"),
        Some(Match { route: 3, rule: 0 }),
        "a method the view does not list falls through to the next one"
    );

    // The implication is a lower-time decision, not a table one: a `GET` view
    // answers `HEAD` itself, so a later `HEAD`-only view never sees one.
    let implied = "view get GET regex '.*' {\n}\n\
                   \n\
                   view head HEAD regex '.*' {\n}\n\
                   \n\
                   view rest regex '.*' {\n}\n";
    let table = routing_table(implied);
    assert_eq!(table.route("GET", "/x"), Some(Match { route: 0, rule: 0 }));
    assert_eq!(
        table.route("HEAD", "/x"),
        Some(Match { route: 0, rule: 0 }),
        "the GET view implies HEAD, so the HEAD-only view is never reached"
    );
    assert_eq!(table.route("PUT", "/x"), Some(Match { route: 2, rule: 0 }));

    let uppercase_only = "view upper GET regex '.*' {\n}\n";
    let table = routing_table(uppercase_only);
    assert_eq!(table.route("get", "/x"), None, "methods are case-sensitive");
}

#[test]
fn the_first_matching_route_wins() {
    let source = "view specific regex '/a/.*' {\n}\n\
                  \n\
                  view general regex '/.*' {\n}\n\
                  \n\
                  view everything regex '.*' {\n}\n";
    let table = routing_table(source);
    assert_eq!(
        table.route("GET", "/a/b"),
        Some(Match { route: 0, rule: 0 })
    );
    assert_eq!(table.route("GET", "/b"), Some(Match { route: 1, rule: 0 }));
    assert_eq!(table.route("GET", "b"), Some(Match { route: 2, rule: 0 }));
}

#[test]
fn a_route_reports_which_of_its_rules_matched() {
    let source = "view many \"/a\", regex '/b/[0-9]+', \"/c\" {\n}\n";
    let table = routing_table(source);
    assert_eq!(table.route("GET", "/a"), Some(Match { route: 0, rule: 0 }));
    assert_eq!(
        table.route("GET", "/b/7"),
        Some(Match { route: 0, rule: 1 })
    );
    assert_eq!(table.route("GET", "/c"), Some(Match { route: 0, rule: 2 }));
    assert_eq!(table.route("GET", "/d"), None);
}

#[test]
fn unmatched_requests_have_no_route() {
    let source = "view about \"/about/\" {\n}\n";
    let table = routing_table(source);
    assert_eq!(
        table.route("GET", "/about/"),
        Some(Match { route: 0, rule: 0 })
    );
    assert_eq!(table.route("GET", "/about/x"), None);
    assert_eq!(table.route("GET", "/"), None);

    let manifest = Manifest::from_blastfile_str("").expect("parses");
    let empty = RoutingTable::new(&manifest).expect("compiles");
    assert_eq!(empty.route("GET", "/"), None);
}

#[test]
fn alternatives_stay_inside_the_anchor() {
    // A bare `a|ab` would match the prefix `a`; the wrapper turns a whole
    // pattern into one alternative group, so both branches are anchored.
    let source = "view either regex 'a|ab' {\n}\n\
                  \n\
                  view fallback regex '.*' {\n}\n";
    let table = routing_table(source);
    assert_eq!(table.route("GET", "ab"), Some(Match { route: 0, rule: 0 }));
    assert_eq!(table.route("GET", "a"), Some(Match { route: 0, rule: 0 }));
    assert_eq!(
        table.route("GET", "aba"),
        Some(Match { route: 1, rule: 0 }),
        "the alternative is not a prefix match"
    );
    assert_eq!(table.route("GET", "b"), Some(Match { route: 1, rule: 0 }));
}

#[test]
fn a_regex_pattern_cannot_escape_the_anchor() {
    // `x)|(.*` compiles to `^(?:x)|(.*)$` if it is wrapped blindly: a
    // catch-all, which would hand every request to that route. `a)|(b` is the
    // same trick with a prefix and a suffix. Both are malformed on their own,
    // and §8.1 pre-compiles a `regex` before it is wrapped — at the pattern's
    // own quote, so the file never lowers and no table is ever built (R21).
    for pattern in ["x)|(.*", "a)|(b"] {
        let errors = refusal(&format!("view hostile regex '{pattern}' {{\n}}\n"));
        assert_eq!(errors.len(), 1, "{pattern:?}: {errors}");
        match &errors.errors()[0] {
            ManifestError::PatternRegex {
                pattern: reported,
                message,
                ..
            } => {
                assert_eq!(reported, pattern);
                assert!(message.contains("group"), "{pattern:?}: {message}");
            }
            other => panic!("unexpected error {other:?}"),
        }
        assert!(!errors.to_string().contains('\n'), "one line: {errors:?}");

        // The allowed twin: the same shape with every group closed.
        assert!(Manifest::from_blastfile_str("view safe regex '(?:x)|(?:.*)' {\n}\n").is_ok());
        assert!(Manifest::from_blastfile_str("view safe regex '/x/.*' {\n}\n").is_ok());
    }
}

#[test]
fn a_broken_pattern_is_refused_before_a_manifest_exists() {
    // Two broken patterns in one file: the parser refuses the first and yields
    // no manifest at all (R21), so nothing can report the second. The second
    // statement is still checked the same way when it is the first one.
    let source = "view first regex '[' {\n}\n\
                  \n\
                  view second regex '(' {\n}\n";
    let errors = refusal(source);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::PatternRegex { pattern, .. } if pattern == "["
        ),
        "{errors:?}"
    );

    assert!(matches!(
        &refusal("view second regex '(' {\n}\n").errors()[0],
        ManifestError::PatternRegex { pattern, .. } if pattern == "("
    ));

    // The allowed twin: the §6 application, whose every pattern compiles.
    let manifest = Manifest::from_blastfile_str(SECTION_6).expect("parses");
    assert!(RoutingTable::new(&manifest).is_ok());
}

#[test]
fn a_pattern_is_bounded_before_it_is_compiled() {
    // §8.1: both pattern forms are capped at 4096 bytes of source, so a `regex`
    // cannot hide a megabyte pattern the quoted form would refuse.
    let bomb = "a".repeat(100_000);
    let errors = refusal(&format!("view huge regex '{bomb}' {{\n}}\n"));
    assert!(
        matches!(&errors.errors()[0], ManifestError::PatternTooLong { .. }),
        "{errors:?}"
    );
    assert!(
        errors.to_string().contains("at most 4096 bytes"),
        "{errors}"
    );

    // A short source can still build a program over the compile budget: the
    // `regex` crate's own limit — `REGEX_SIZE_LIMIT`, unchanged — refuses it at
    // the pattern's position rather than letting it balloon (§17.4).
    let errors = refusal("view huge regex 'a{1000000}' {\n}\n");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::PatternRegex { message, .. } if message.contains("size limit")
        ),
        "{errors:?}"
    );
}

#[test]
fn deeply_nested_patterns_are_rejected() {
    let deep = format!("{}a{}", "(?:".repeat(200), ")".repeat(200));
    let errors = refusal(&format!("view deep regex '{deep}' {{\n}}\n"));
    match &errors.errors()[0] {
        ManifestError::PatternRegex { message, .. } => {
            assert!(message.contains("nested parentheses"), "{message}");
        }
        other => panic!("unexpected error {other:?}"),
    }
}
