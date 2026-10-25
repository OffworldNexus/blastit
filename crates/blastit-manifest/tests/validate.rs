//! Validation: every cross-reference check, warning and the uid cap (§6).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{
    Location, Manifest, ManifestError, ManifestErrors, ManifestWarning, PgGrant, ProxyName,
};

use common::{SECTION_5_1, SECTION_6, section_6, texts, validate_errors, validate_warnings};

#[test]
fn the_section_6_example_validates_without_warnings() {
    let (manifest, warnings) = Manifest::parse_and_validate(SECTION_6, 20_000).expect("valid");
    assert_eq!(warnings, vec![]);
    assert_eq!(manifest.validate(20_000).expect("valid"), vec![]);
    assert_eq!(manifest.validate_default().expect("valid"), vec![]);
    assert_eq!(section_6().validate(20_000).expect("valid"), vec![]);
}

#[test]
fn unknown_groups_are_reported_wherever_they_are_referenced() {
    let source = "version = 1\n\
                  groups = [\"nope\"]\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.demo.db]\nqueries = []\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\ngroups = [\"alsonope\"]\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::new(vec![
            ManifestError::UnknownGroup {
                location: Location::Global,
                group: "nope".to_string(),
            },
            ManifestError::UnknownGroup {
                location: Location::Route {
                    index: 0,
                    name: "about".to_string(),
                },
                group: "alsonope".to_string(),
            },
        ])
    );
}

#[test]
fn a_group_that_exists_is_fine_wherever_it_is_referenced() {
    let source = "version = 1\n\
                  groups = [\"session\"]\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.session.db]\nqueries = []\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\ngroups = [\"session\"]\n";
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn the_section_5_1_proxies_validate_with_ungranted_proxy_warnings() {
    let warnings = validate_warnings(SECTION_5_1);
    assert_eq!(
        warnings,
        vec![
            ManifestWarning::UngrantedProxy {
                proxy: ProxyName::from("analytics"),
            },
            ManifestWarning::UngrantedProxy {
                proxy: ProxyName::from("db"),
            },
        ],
        "§5.1 declares proxies but no grants: nothing is allowed to use them yet"
    );
}

#[test]
fn grants_on_unknown_proxies_are_reported_with_their_place() {
    let route = "version = 1\n\
                 [[route]]\nname = \"about\"\npath = ['/about/']\n\
                 [route.missing]\nqueries = []\n";
    assert_eq!(
        validate_errors(route, 20_000),
        ManifestErrors::from(ManifestError::GrantOnUnknownProxy {
            location: Location::Route {
                index: 0,
                name: "about".to_string(),
            },
            proxy: ProxyName::from("missing"),
        })
    );

    let group = "version = 1\n[group.session.nocache]\nqueries = []\n";
    assert_eq!(
        validate_errors(group, 20_000),
        ManifestErrors::from(ManifestError::GrantOnUnknownProxy {
            location: Location::Group("session".to_string()),
            proxy: ProxyName::from("nocache"),
        })
    );
}

#[test]
fn a_declared_proxy_needs_no_grant_to_be_known() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n\
                  [route.db]\nqueries = []\n";
    // An empty grant is still a grant: no validation error, no warning.
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn duplicate_route_names_are_reported() {
    let source = "version = 1\n\
                  [[route]]\nname = \"about\"\npath = ['/a']\n\
                  [[route]]\nname = \"checkout\"\npath = ['/b']\n\
                  [[route]]\nname = \"about\"\npath = ['/c']\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::from(ManifestError::DuplicateRouteName {
            name: "about".to_string(),
            first: 0,
            second: 2,
        })
    );
}

#[test]
fn duplicate_path_rules_and_patterns_are_fine() {
    let source = "version = 1\n[[route]]\nname = \"about\"\npath = ['/about/', '/about/']\n";
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn route_names_must_match_the_documented_pattern() {
    for name in [
        "Product",
        "-leading",
        "_leading",
        "",
        "with space",
        "with/slash",
        "Ünicode",
    ] {
        let source = format!("version = 1\n[[route]]\nname = \"{name}\"\npath = ['.*']\n");
        assert_eq!(
            validate_errors(&source, 20_000),
            ManifestErrors::from(ManifestError::InvalidRouteName {
                index: 0,
                name: name.to_string(),
            }),
            "route name {name:?} should be rejected"
        );
    }

    for name in ["about", "product-detail", "v2_api", "9lives", "a"] {
        let source = format!("version = 1\n[[route]]\nname = \"{name}\"\npath = ['.*']\n");
        assert_eq!(
            validate_warnings(&source),
            vec![],
            "route name {name:?} should be accepted"
        );
    }
}

#[test]
fn a_route_without_usable_patterns_is_reported() {
    let source = "version = 1\n\
                  [[route]]\nname = \"empty\"\npath = []\n\
                  [[route]]\nname = \"blank\"\npath = [\"\", '/real']\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::new(vec![
            ManifestError::EmptyPath {
                route: "empty".to_string(),
            },
            ManifestError::EmptyPattern {
                route: "blank".to_string(),
                rule: 0,
            },
        ])
    );
}

#[test]
fn invalid_regexes_are_reported_with_their_rule() {
    let source = "version = 1\n[[route]]\nname = \"about\"\npath = ['/ok', '[']\n";
    let errors = validate_errors(source, 20_000);
    assert_eq!(errors.len(), 1, "{errors:?}");
    match &errors.errors()[0] {
        ManifestError::InvalidRegex {
            route,
            rule,
            pattern,
            message,
        } => {
            assert_eq!(route, "about");
            assert_eq!(*rule, 1);
            assert_eq!(pattern, "[");
            assert!(!message.is_empty(), "the regex error is carried through");
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn methods_must_be_uppercase_http_tokens() {
    let source = "version = 1\n\
                  [[route]]\nname = \"about\"\npath = ['.*']\nmethods = [\"get\", \"GE T\", \"\"]\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::new(vec![
            ManifestError::InvalidMethod {
                route: "about".to_string(),
                method: "get".to_string(),
            },
            ManifestError::InvalidMethod {
                route: "about".to_string(),
                method: "GE T".to_string(),
            },
            ManifestError::InvalidMethod {
                route: "about".to_string(),
                method: String::new(),
            },
        ])
    );
}

#[test]
fn methods_must_not_be_an_empty_list() {
    let source = "version = 1\n\
                  [[route]]\nname = \"about\"\npath = ['.*']\nmethods = []\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::from(ManifestError::EmptyMethods {
            route: "about".to_string(),
        }),
        "an explicit empty filter would make the route unreachable"
    );
}

#[test]
fn valid_methods_are_accepted() {
    let source = "version = 1\n\
                  [[route]]\nname = \"about\"\npath = ['.*']\nmethods = [\"GET\", \"HEAD\", \"MKCOL\", \"M-SEARCH\"]\n";
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn the_last_route_uid_must_stay_below_65536() {
    let manifest = |routes: usize| {
        let mut source = String::from("version = 1\n");
        for index in 0..routes {
            source.push_str(&format!("[[route]]\nname = \"r{index}\"\npath = ['.*']\n"));
        }
        source
    };

    // uid = base + 1000 + index, and the §6 example has four routes.
    assert_eq!(
        section_6().validate(64_536).unwrap_err(),
        ManifestErrors::from(ManifestError::TooManyRoutes {
            index: 3,
            uid: 65_539, // 64536 + 1000 + 3
            base: 64_536,
            max: 65_536,
        }),
        "the last route of the §6 example overflows at this base"
    );
    assert_eq!(
        section_6().validate(64_533).unwrap_err(),
        ManifestErrors::from(ManifestError::TooManyRoutes {
            index: 3,
            uid: 65_536,
            base: 64_533,
            max: 65_536,
        }),
        "65536 is out of range, not the last usable uid"
    );

    // 64532 + 1000 + 3 = 65535: exactly the last usable uid.
    assert_eq!(section_6().validate(64_532).expect("fits"), vec![]);

    // A base that is already too high for dbproxy.
    assert_eq!(
        Manifest::from_toml_str("version = 1\n")
            .unwrap()
            .validate(65_533)
            .unwrap_err(),
        ManifestErrors::from(ManifestError::UidBaseTooHigh {
            base: 65_533,
            uid: 65_536,
            max: 65_536,
        })
    );

    // No routes: nothing can overflow.
    assert_eq!(
        Manifest::from_toml_str("version = 1\n")
            .unwrap()
            .validate(64_536)
            .expect("no routes"),
        vec![]
    );

    // The boundary of the count, not just the sample manifests.
    assert_eq!(
        Manifest::from_toml_str(&manifest(1))
            .unwrap()
            .validate(64_536)
            .unwrap_err(),
        ManifestErrors::from(ManifestError::TooManyRoutes {
            index: 0,
            uid: 65_536,
            base: 64_536,
            max: 65_536,
        })
    );
    Manifest::from_toml_str(&manifest(1))
        .unwrap()
        .validate(64_535)
        .expect("one route fits just below the cap");
}

#[test]
fn warnings_report_unattached_groups_and_ungranted_proxies() {
    let source = "version = 1\n\
                  [proxy.used]\nprotocol = \"postgresql\"\n\
                  [proxy.unused]\nprotocol = \"postgresql\"\n\
                  [group.orphan.used]\nqueries = []\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n";
    assert_eq!(
        validate_warnings(source),
        vec![
            ManifestWarning::UnattachedGroup {
                group: "orphan".to_string(),
            },
            ManifestWarning::UngrantedProxy {
                proxy: ProxyName::from("unused"),
            },
        ]
    );
}

#[test]
fn attaching_a_group_and_granting_a_proxy_silences_its_warning() {
    let source = "version = 1\n\
                  groups = [\"orphan\"]\n\
                  [proxy.used]\nprotocol = \"postgresql\"\n\
                  [proxy.unused]\nprotocol = \"postgresql\"\n\
                  [group.orphan.used]\nqueries = []\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n\
                  [route.unused]\nqueries = []\n";
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn every_validation_problem_is_collected() {
    let source = "version = 1\n\
                  groups = [\"ghost\"]\n\
                  [[route]]\nname = \"Product\"\npath = ['[']\nmethods = [\"get\"]\n\
                  [route.missing]\nqueries = []\n";
    let errors = validate_errors(source, 20_000);
    assert_eq!(errors.len(), 5, "{errors:?}");
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        ManifestError::UnknownGroup {
            location: Location::Global,
            ..
        }
    )));
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, ManifestError::InvalidRouteName { .. }))
    );
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, ManifestError::InvalidRegex { .. }))
    );
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, ManifestError::InvalidMethod { .. }))
    );
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, ManifestError::GrantOnUnknownProxy { .. }))
    );

    let rendered = errors.to_string();
    assert!(rendered.contains("unknown group \"ghost\""), "{rendered}");
    assert!(rendered.contains("invalid name \"Product\""), "{rendered}");
    assert!(
        rendered.contains("grant on unknown proxy \"missing\""),
        "{rendered}"
    );
}

#[test]
fn parse_errors_come_back_before_validation() {
    let errors = Manifest::parse_and_validate("version = 2\n", 20_000).unwrap_err();
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::UnsupportedVersion {
            found: "2".to_string(),
        })
    );
}

#[test]
fn locations_read_well_in_errors() {
    assert_eq!(Location::Global.to_string(), "global groups");
    assert_eq!(
        Location::Group("session".to_string()).to_string(),
        "group \"session\""
    );
    assert_eq!(
        Location::Route {
            index: 2,
            name: "checkout".to_string(),
        }
        .to_string(),
        "route \"checkout\" (index 2)"
    );
}

/// A manifest with one unrestricted inline grant, and a place to put more.
const UNRESTRICTED: &str = "version = 1\n\
                             [proxy.db]\nprotocol = \"postgresql\"\n\
                             [proxy.analytics]\nprotocol = \"postgresql\"\n\
                             [[route]]\nname = \"admin\"\npath = ['/admin/.*']\n\
                             [route.db]\nunrestricted = true\n\
                             [route.analytics]\nunrestricted = true\n\
                             [[route]]\nname = \"about\"\npath = ['/about/']\n\
                             [route.db]\nqueries = []\n";

#[test]
fn an_unrestricted_route_warns_once_per_route_and_proxy() {
    assert_eq!(
        validate_warnings(UNRESTRICTED),
        vec![
            ManifestWarning::UnrestrictedRoute {
                route: "admin".to_string(),
                proxy: ProxyName::from("analytics"),
            },
            ManifestWarning::UnrestrictedRoute {
                route: "admin".to_string(),
                proxy: ProxyName::from("db"),
            },
        ],
        "one warning per (route, proxy), in route then proxy order"
    );
}

#[test]
fn an_unrestricted_grant_in_a_group_is_refused_at_boot() {
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.backoffice.db]\nunrestricted = true\n\
                  [[route]]\nname = \"admin\"\npath = ['/admin/.*']\ngroups = [\"backoffice\"]\n";
    let errors = validate_errors(source, 20_000);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::UnrestrictedGroup {
            location: Location::Group("backoffice".to_string()),
            proxy: ProxyName::from("db"),
        })
    );
    let rendered = errors.to_string();
    assert!(
        rendered.contains("unrestricted is only allowed inline on a route"),
        "{rendered}"
    );
    assert!(rendered.contains("group \"backoffice\""), "{rendered}");

    // A group attached nowhere is refused too: it could be attached to every
    // route at the next deploy, which is what the rule exists for.
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.backoffice.db]\nunrestricted = true\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::from(ManifestError::UnrestrictedGroup {
            location: Location::Group("backoffice".to_string()),
            proxy: ProxyName::from("db"),
        })
    );

    // Attached globally: same refusal, and the global `groups` line does not
    // hide it.
    let source = "version = 1\n\
                  groups = [\"backoffice\"]\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.backoffice.db]\nunrestricted = true\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::from(ManifestError::UnrestrictedGroup {
            location: Location::Group("backoffice".to_string()),
            proxy: ProxyName::from("db"),
        })
    );

    // `unrestricted = false` in a group is an ordinary grant.
    let source = "version = 1\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.backoffice.db]\nunrestricted = false\nqueries = [\"SELECT 1\"]\n\
                  [[route]]\nname = \"admin\"\npath = ['/admin/.*']\ngroups = [\"backoffice\"]\n";
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn an_unrestricted_route_needs_no_query_and_gets_a_grant() {
    let manifest = Manifest::from_toml_str(UNRESTRICTED).expect("parses");
    manifest
        .validate(20_000)
        .expect("validates despite the warning");
    // The route gets a grant on both proxies, so init binds both listeners
    // (§2.2, §6), and the second route's empty grant on `db` disappears.
    let admin = manifest.effective_grants(0);
    assert_eq!(admin.len(), 2);
    for (proxy, grant) in &admin {
        let grant = grant.as_postgresql().expect("a postgresql grant");
        assert!(grant.is_unrestricted(), "{proxy:?}");
        assert!(!grant.is_empty(), "{proxy:?}");
    }
    assert_eq!(
        manifest.effective_grants(1).len(),
        0,
        "an empty inline grant is no grant at all"
    );
}

#[test]
fn a_group_can_never_make_a_route_unrestricted() {
    // The check is about groups, not about the union: nothing a group holds can
    // turn a route into one that may run anything.
    let source = "version = 1\n\
                  groups = [\"wide\"]\n\
                  [proxy.db]\nprotocol = \"postgresql\"\n\
                  [group.wide.db]\nqueries = [\"SELECT 1\"]\ntransaction_control = true\n\
                  [[route]]\nname = \"about\"\npath = ['/about/']\n\
                  [route.db]\nqueries = [\"SELECT 2\"]\n";
    let manifest = Manifest::from_toml_str(source).expect("parses");
    assert_eq!(manifest.validate(20_000).expect("validates"), vec![]);
    let mut grants = manifest.effective_grants(0);
    let grant = grants
        .remove(&ProxyName::from("db"))
        .expect("the union")
        .into_postgresql()
        .expect("a postgresql grant");
    assert!(!grant.is_unrestricted());
    assert_eq!(texts(grant.queries()), ["SELECT 1", "SELECT 2"]);
    assert!(grant.transaction_control());
}

#[test]
fn merging_keeps_unrestricted_once_it_is_on() {
    let mut grant = PgGrant::new(vec!["SELECT 1".to_string()], true);
    assert!(!grant.is_unrestricted());
    grant.merge_from(&PgGrant::unrestricted());
    assert!(grant.is_unrestricted());
    assert_eq!(texts(grant.queries()), ["SELECT 1"], "the queries stay");
    assert!(!grant.is_empty());

    let mut grant = PgGrant::unrestricted();
    grant.merge_from(&PgGrant::new(vec!["SELECT 2".to_string()], false));
    assert!(grant.is_unrestricted());
    assert_eq!(texts(grant.queries()), ["SELECT 2"]);
}
