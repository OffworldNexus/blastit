//! The routing table: full match, manifest order, method filters, rule indices
//! (§2.4, §6).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{Manifest, ManifestError, Match, RoutingTable};

use common::{SECTION_6, section_6};

/// Compiles a document that is assumed to be valid.
fn routing_table(source: &str) -> RoutingTable {
    let manifest = Manifest::from_toml_str(source).expect("parses");
    RoutingTable::new(&manifest).expect("compiles")
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
fn methods_are_filtered_and_head_is_not_implied_by_get() {
    let manifest = section_6();
    let table = RoutingTable::new(&manifest).expect("compiles");

    assert_eq!(
        table.route("GET", "/products/42/"),
        Some(Match { route: 0, rule: 0 })
    );
    assert_eq!(
        table.route("HEAD", "/products/42/"),
        Some(Match { route: 0, rule: 0 }),
        "HEAD is listed explicitly in the §6 example"
    );
    assert_eq!(
        table.route("POST", "/products/42/"),
        Some(Match { route: 3, rule: 0 }),
        "a method the route does not list falls through to the next route"
    );

    let head_only = "version = 1\n\
                     [[route]]\nname = \"get\"\npath = ['.*']\nmethods = [\"GET\"]\n\
                     [[route]]\nname = \"head\"\npath = ['.*']\nmethods = [\"HEAD\"]\n\
                     [[route]]\nname = \"rest\"\npath = ['.*']\n";
    let table = routing_table(head_only);
    assert_eq!(table.route("GET", "/x"), Some(Match { route: 0, rule: 0 }));
    assert_eq!(table.route("HEAD", "/x"), Some(Match { route: 1, rule: 0 }));
    assert_eq!(table.route("PUT", "/x"), Some(Match { route: 2, rule: 0 }));

    let uppercase_only = "version = 1\n\
                          [[route]]\nname = \"upper\"\npath = ['.*']\nmethods = [\"GET\"]\n";
    let table = routing_table(uppercase_only);
    assert_eq!(table.route("get", "/x"), None, "methods are case-sensitive");
}

#[test]
fn the_first_matching_route_wins() {
    let source = "version = 1\n\
                  [[route]]\nname = \"specific\"\npath = ['/a/.*']\n\
                  [[route]]\nname = \"general\"\npath = ['/.*']\n\
                  [[route]]\nname = \"everything\"\npath = ['.*']\n";
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
    let source = "version = 1\n\
                  [[route]]\nname = \"many\"\npath = ['/a', '/b/[0-9]+', '/c']\n";
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
    let source = "version = 1\n[[route]]\nname = \"about\"\npath = ['/about/']\n";
    let table = routing_table(source);
    assert_eq!(
        table.route("GET", "/about/"),
        Some(Match { route: 0, rule: 0 })
    );
    assert_eq!(table.route("GET", "/about/x"), None);
    assert_eq!(table.route("GET", "/"), None);

    let manifest = Manifest::from_toml_str("version = 1\n").expect("parses");
    let empty = RoutingTable::new(&manifest).expect("compiles");
    assert_eq!(empty.route("GET", "/"), None);
}

#[test]
fn alternatives_stay_inside_the_anchor() {
    // A bare `a|ab` would match the prefix `a`; the wrapper turns a whole
    // pattern into one alternative group, so both branches are anchored.
    let source = "version = 1\n\
                  [[route]]\nname = \"either\"\npath = ['a|ab']\n\
                  [[route]]\nname = \"fallback\"\npath = ['.*']\n";
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
fn a_pattern_cannot_escape_the_anchor() {
    // `x)|(.*` compiles to `^(?:x)|(.*)$` if it is wrapped blindly: a
    // catch-all, which would hand every request to that route. `a)|(b` is the
    // same trick with a prefix and a suffix. Both are malformed on their own.
    for pattern in ["x)|(.*", "a)|(b"] {
        let source = format!(
            "version = 1\n\
             [[route]]\nname = \"hostile\"\npath = ['{pattern}']\n\
             [[route]]\nname = \"fallback\"\npath = ['.*']\n"
        );
        let manifest = Manifest::from_toml_str(&source).expect("parses");

        let errors = RoutingTable::new(&manifest).expect_err("must not compile");
        assert_eq!(errors.len(), 1, "{pattern:?}");
        match &errors.errors()[0] {
            ManifestError::InvalidRegex {
                route,
                rule,
                pattern: reported,
                ..
            } => {
                assert_eq!(route, "hostile");
                assert_eq!(*rule, 0);
                assert_eq!(reported, pattern);
            }
            other => panic!("unexpected error {other:?}"),
        }

        let errors = manifest
            .validate(20_000)
            .expect_err("validation reports it too");
        assert!(matches!(
            errors.errors()[0],
            ManifestError::InvalidRegex { .. }
        ));
    }
}

#[test]
fn every_broken_pattern_is_reported() {
    let source = "version = 1\n\
                  [[route]]\nname = \"first\"\npath = ['[', '/ok']\n\
                  [[route]]\nname = \"second\"\npath = ['/fine', '(']\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let errors = RoutingTable::new(&manifest).expect_err("both are broken");
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(matches!(
        &errors.errors()[0],
        ManifestError::InvalidRegex { route, rule: 0, .. } if route == "first"
    ));
    assert!(matches!(
        &errors.errors()[1],
        ManifestError::InvalidRegex { route, rule: 1, .. } if route == "second"
    ));

    let manifest = Manifest::from_toml_str(SECTION_6).expect("parses");
    assert!(RoutingTable::new(&manifest).is_ok());
}

#[test]
fn regexes_that_come_from_the_manifest_are_bounded() {
    // A pattern whose automaton is far bigger than the compile budget: it is
    // rejected rather than compiled. (The regex crate's own default budget
    // would have accepted this one.)
    let bomb = "a".repeat(100_000);
    let source = format!("version = 1\n[[route]]\nname = \"huge\"\npath = ['{bomb}']\n");
    let manifest = Manifest::from_toml_str(&source).expect("parses");

    let errors = RoutingTable::new(&manifest).expect_err("the size limit is enforced");
    match &errors.errors()[0] {
        ManifestError::InvalidRegex { route, message, .. } => {
            assert_eq!(route, "huge");
            assert!(message.contains("size limit"), "{message}");
        }
        other => panic!("unexpected error {other:?}"),
    }

    // Validation reports the same problem before anything is compiled.
    let errors = manifest.validate(20_000).expect_err("too big to compile");
    assert!(matches!(
        errors.errors()[0],
        ManifestError::InvalidRegex { .. }
    ));
}

#[test]
fn deeply_nested_patterns_are_rejected() {
    let deep = format!("{}a{}", "(?:".repeat(200), ")".repeat(200));
    let source = format!("version = 1\n[[route]]\nname = \"deep\"\npath = ['{deep}']\n");
    let manifest = Manifest::from_toml_str(&source).expect("parses");
    let errors = RoutingTable::new(&manifest).expect_err("nesting is bounded");
    match &errors.errors()[0] {
        ManifestError::InvalidRegex { message, .. } => {
            assert!(message.contains("nested parentheses"), "{message}");
        }
        other => panic!("unexpected error {other:?}"),
    }
}
