//! The routing table: full match, manifest order, method filters, rule indices
//! (§2.4, §6).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{Manifest, ManifestError, Match, RoutingTable};

use common::{SECTION_6, section_6};

/// Compiles a document that is assumed to be valid.
fn routing_table(source: &str) -> RoutingTable {
    let manifest = Manifest::from_toml_str(source).expect("parses");
    RoutingTable::try_new(&manifest).expect("compiles")
}

#[test]
fn the_section_6_table_routes_paths_to_routes() {
    let manifest = section_6();
    let table = RoutingTable::try_new(&manifest).expect("compiles");

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
    let table = RoutingTable::try_new(&manifest).expect("compiles");
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
    let table = RoutingTable::try_new(&manifest).expect("compiles");

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

    let lowercase = "version = 1\n\
                     [[route]]\nname = \"upper\"\npath = ['.*']\nmethods = [\"GET\"]\n";
    let table = routing_table(lowercase);
    assert_eq!(table.route("get", "/x"), None, "methods are case-sensitive");
}

#[test]
fn an_empty_method_list_matches_nothing() {
    let source = "version = 1\n\
                  [[route]]\nname = \"none\"\npath = ['.*']\nmethods = []\n\
                  [[route]]\nname = \"any\"\npath = ['.*']\n";
    let table = routing_table(source);
    assert_eq!(
        table.route("GET", "/x"),
        Some(Match { route: 1, rule: 0 }),
        "an explicit empty filter is not the same as no filter"
    );
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

    let empty = RoutingTable::new(&Manifest::from_toml_str("version = 1\n").expect("parses"));
    assert_eq!(empty.route("GET", "/"), None);
}

#[test]
fn a_manifest_that_does_not_validate_still_compiles() {
    let source = "version = 1\n\
                  [[route]]\nname = \"broken\"\npath = ['[', '/ok']\n\
                  [[route]]\nname = \"about\"\npath = ['.*']\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");

    // `new` skips the pattern that does not compile and keeps rule indices.
    let table = RoutingTable::new(&manifest);
    assert_eq!(table.route("GET", "/ok"), Some(Match { route: 0, rule: 1 }));
    assert_eq!(
        table.route("GET", "/anything"),
        Some(Match { route: 1, rule: 0 }),
        "the broken rule is skipped; the next route still matches"
    );

    // `try_new` reports it instead of guessing.
    let errors = RoutingTable::try_new(&manifest).expect_err("pattern does not compile");
    assert_eq!(errors.len(), 1);
    match &errors.errors()[0] {
        ManifestError::InvalidRegex {
            route,
            rule,
            pattern,
            ..
        } => {
            assert_eq!(route, "broken");
            assert_eq!(*rule, 0);
            assert_eq!(pattern, "[");
        }
        other => panic!("unexpected error {other:?}"),
    }

    let manifest = Manifest::from_toml_str(SECTION_6).expect("parses");
    assert!(RoutingTable::try_new(&manifest).is_ok());
}

#[test]
fn a_broken_first_pattern_does_not_hide_a_later_route() {
    let source = "version = 1\n\
                  [[route]]\nname = \"broken\"\npath = ['(']\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    let table = RoutingTable::new(&manifest);
    assert_eq!(
        table.route("GET", "/about/"),
        Some(Match { route: 1, rule: 0 })
    );
}

#[test]
fn regexes_that_come_from_the_manifest_are_bounded() {
    // A pattern whose automaton is far bigger than the compile budget: it is
    // rejected rather than compiled. (The regex crate's own default budget
    // would have accepted this one.)
    let bomb = "a".repeat(100_000);
    let source = format!("version = 1\n[[route]]\nname = \"huge\"\npath = ['{bomb}']\n");
    let manifest = Manifest::from_toml_str(&source).expect("parses");

    let errors = RoutingTable::try_new(&manifest).expect_err("the size limit is enforced");
    match &errors.errors()[0] {
        ManifestError::InvalidRegex { route, message, .. } => {
            assert_eq!(route, "huge");
            assert!(message.contains("size limit"), "{message}");
        }
        other => panic!("unexpected error {other:?}"),
    }
    assert_eq!(RoutingTable::new(&manifest).route("GET", "/a"), None);

    // Validation reports the same problem before anything is compiled.
    let errors = manifest.validate(20_000).expect_err("too big to compile");
    assert!(matches!(
        errors.errors()[0],
        ManifestError::InvalidRegex { .. }
    ));
}
