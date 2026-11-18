//! Validation: every cross-reference check, warning and the uid cap (§6).
//!
//! A `Blastfile` cannot write some of the shapes these rules judge — a view name
//! is a `Word` the parser checks, a pattern is compiled by the parser, an
//! `unrestricted` block takes nothing — so those arms are exercised through the
//! model's `#[doc(hidden)]` builder instead of a document. Design §20 R8 makes
//! that builder the integration tests' own: the validator still runs on every
//! manifest, and every rule keeps a test rather than becoming a dead letter. Each
//! such test says in one line why a document cannot reach it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;

use blastit_manifest::{
    Columns, Grant, Group, Location, Manifest, ManifestError, ManifestErrors, ManifestWarning,
    PgGrant, Position, ProxyName, Route, TableGrant, TableGrants,
};
use blastit_sqlpolicy::TemplateKind;

use common::{
    SECTION_5_1, SECTION_6, parse_errors, section_6, texts, validate_errors, validate_warnings,
};

/// A route built through the model builder, in its own sandbox.
fn built_route(
    sandbox: u32,
    name: &str,
    path: &[&str],
    methods: Option<&[&str]>,
    groups: &[&str],
    grants: BTreeMap<ProxyName, Grant>,
) -> Route {
    Route::new(
        name.to_string(),
        path.iter().map(|rule| (*rule).to_string()).collect(),
        methods.map(|methods| methods.iter().map(|method| (*method).to_string()).collect()),
        groups.iter().map(|group| (*group).to_string()).collect(),
        false,
        false,
        grants,
    )
    .with_sandbox(sandbox)
}

/// Validates one route built through the builder and returns its problems.
fn built_errors(route: Route) -> ManifestErrors {
    Manifest::from_parts(1, Vec::new(), BTreeMap::new(), BTreeMap::new(), vec![route])
        .validate(20_000)
        .expect_err("the route must not validate")
}

/// A plain route: no groups, no grants, one pattern.
fn plain_route(sandbox: u32, name: &str) -> Route {
    built_route(sandbox, name, &[".*"], None, &[], BTreeMap::new())
}

/// One postgresql grant, unrestricted and optionally carrying the content the
/// rule forbids next to it.
fn postgres_grant(unrestricted: bool, queries: &[&str], tables: TableGrants) -> Grant {
    Grant::Postgresql(
        PgGrant::new(
            queries.iter().map(|query| (*query).to_string()).collect(),
            false,
        )
        .with_tables(tables)
        .with_unrestricted(unrestricted),
    )
}

/// The stores map for one grant on `db`.
fn on_db(grant: Grant) -> BTreeMap<ProxyName, Grant> {
    [(ProxyName::from("db"), grant)].into_iter().collect()
}

#[test]
fn the_section_6_example_validates_with_its_documented_warnings() {
    // §7.3: the checkout view declares `egress any`, and `R11` says every egress
    // route is a `manifest.warning` at boot.
    let egress = vec![ManifestWarning::EgressRoute {
        route: "checkout".to_string(),
    }];
    let (manifest, warnings) = Manifest::parse_blastfile(SECTION_6, 20_000).expect("valid");
    assert_eq!(warnings, egress);
    assert_eq!(manifest.validate(20_000).expect("valid"), egress);
    assert_eq!(manifest.validate_default().expect("valid"), egress);
    assert_eq!(section_6().validate(20_000).expect("valid"), egress);
}

#[test]
fn an_unknown_use_is_refused_at_the_view_that_writes_it() {
    // A `use` is resolved by lowering (§11.2), so the file never validates: the
    // retired format's "unknown group" finding is a positioned refusal now.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait demo {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  view about \"/about/\" {\n    use demo, alsonope\n}\n";
    let errors = parse_errors(source);
    assert_eq!(errors.len(), 1, "{errors}");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::UnknownTrait { view, name, .. } if view == "about" && name == "alsonope"
        ),
        "{errors}"
    );
    assert!(errors.to_string().contains("names no trait"), "{errors}");
}

/// The **global** group list still has a validator rule, and no `Blastfile` can
/// fill it (§7.1, `ADR-0012`): `lower` always leaves `global_groups` empty, so
/// the only way to reach this arm is to build the model. Kept so the rule the
/// scheduler's `effective_grants` still honours has a test.
#[test]
fn a_global_group_reference_is_still_reported_by_validate() {
    let manifest = Manifest::from_parts(
        1,
        vec!["nope".to_string()],
        BTreeMap::new(),
        BTreeMap::new(),
        Vec::new(),
    );
    assert_eq!(
        manifest.validate(20_000).expect_err("the group is unknown"),
        ManifestErrors::from(ManifestError::UnknownGroup {
            location: Location::Global,
            group: "nope".to_string(),
        })
    );
}

#[test]
fn a_trait_that_exists_is_fine_where_it_is_used() {
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait session {\n    postgres {\n        transactions\n    }\n}\n\
                  \n\
                  view about \"/about/\" {\n    use session\n}\n";
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
        "§5.1 declares stores but no grants: nothing is allowed to use them yet"
    );
}

#[test]
fn grants_on_unknown_stores_are_reported_with_their_place() {
    // A grant on a store nothing declares lowers to an unresolved grant, so the
    // whole-file check reports it against the view that wrote it (§13.1).
    let route = "postgres $DATABASE_URL\n\
                 \n\
                 view about \"/about/\" {\n    postgres missing {\n        query 'SELECT 1'\n    }\n}\n";
    assert_eq!(
        validate_errors(route, 20_000),
        ManifestErrors::from(ManifestError::GrantOnUnknownProxy {
            location: Location::View {
                index: 0,
                name: "about".to_string(),
            },
            proxy: ProxyName::from("missing"),
        })
    );

    let trait_body = "postgres $DATABASE_URL\n\
                      \n\
                      trait session {\n    postgres nocache {\n        query 'SELECT 1'\n    }\n}\n\
                      \n\
                      view about \"/about/\" {\n    use session\n}\n";
    assert_eq!(
        validate_errors(trait_body, 20_000),
        ManifestErrors::from(ManifestError::GrantOnUnknownProxy {
            location: Location::Trait("session".to_string()),
            proxy: ProxyName::from("nocache"),
        })
    );
}

#[test]
fn a_declared_store_needs_no_grant_to_be_known() {
    // An empty grant block is still a grant: no validation error, no warning.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  view about \"/about/\" {\n    postgres {\n    }\n}\n";
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn duplicate_view_names_are_refused_by_the_parser() {
    // The document is refused where the second name is written, so it never
    // reaches the validator (§6.7): `view` declares the name once for all of its
    // endpoints.
    let errors = parse_errors("view about \"/a\" {\n}\n\nview about \"/b\" {\n}\n");
    assert_eq!(errors.len(), 1, "{errors}");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::DuplicateName { kind, name, .. } if *kind == "view" && name == "about"
        ),
        "{errors}"
    );
    assert!(errors.to_string().contains("declared twice"), "{errors}");

    // The validator's own arm compares **sandboxes**, not positions (§10.3), so
    // two routes named `about` in two sandboxes are a duplicate. No `Blastfile`
    // can write it (its two names are refused above), so the model is built.
    let manifest = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        vec![
            plain_route(0, "about"),
            plain_route(1, "checkout"),
            plain_route(2, "about"),
        ],
    );
    assert_eq!(
        manifest.validate(20_000).expect_err("the name repeats"),
        ManifestErrors::from(ManifestError::DuplicateRouteName {
            name: "about".to_string(),
            first: 0,
            second: 2,
        })
    );
}

#[test]
fn duplicate_path_rules_and_patterns_are_fine() {
    let source = "view about \"/about/\", \"/about/\" {\n}\n";
    assert_eq!(validate_warnings(source), vec![]);
    assert_eq!(
        section_6()
            .route(0)
            .map(|route| route.path().len())
            .expect("one view"),
        1
    );
}

#[test]
fn route_names_must_match_the_documented_pattern() {
    // The refused names are refused twice, on purpose: a document cannot even
    // write them (`view` takes a `Word`, §6.3), and the validator refuses the
    // model an editor could construct from anything else.
    for name in [
        "Product",
        "-leading",
        "_leading",
        "",
        "with space",
        "with/slash",
        "Ünicode",
    ] {
        let errors = parse_errors(&format!("view {name} regex '.*' {{\n}}\n"));
        assert_eq!(errors.len(), 1, "name {name:?}: {errors}");
        let rendered = errors.to_string();
        assert!(
            rendered.starts_with("1:"),
            "name {name:?} is refused at its own line: {rendered}"
        );
        assert!(!rendered.contains('\n'), "one line: {rendered:?}");

        assert_eq!(
            built_errors(built_route(0, name, &[".*"], None, &[], BTreeMap::new())),
            ManifestErrors::from(ManifestError::InvalidRouteName {
                index: 0,
                name: name.to_string(),
            }),
            "route name {name:?} should be rejected"
        );
    }

    for name in ["about", "product-detail", "v2_api", "9lives", "a"] {
        assert_eq!(
            validate_warnings(&format!("view {name} regex '.*' {{\n}}\n")),
            vec![],
            "route name {name:?} should be accepted"
        );
        assert_eq!(
            Manifest::from_parts(
                1,
                Vec::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                vec![plain_route(0, name)]
            )
            .validate(20_000)
            .expect("validates"),
            vec![],
            "route name {name:?} should pass the builder's route too"
        );
    }
}

#[test]
fn a_route_without_usable_patterns_is_reported() {
    // `view empty {}` has no endpoint at all: the parser refuses it before a
    // route exists (§6.7).
    let errors = parse_errors("view empty {\n}\n");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::ViewWithoutEndpoint { name, .. } if name == "empty"
        ),
        "{errors}"
    );
    // An empty pattern cannot be written either: every pattern has to start with
    // `/` (§8.3).
    assert!(matches!(
        &parse_errors("view blank \"\", \"/real\" {\n}\n").errors()[0],
        ManifestError::PatternNotAnchored { .. }
    ));

    // The validator's two arms judge a model an editor could still hand over, so
    // they keep their exact assertions through the builder.
    assert_eq!(
        built_errors(built_route(0, "empty", &[], None, &[], BTreeMap::new())),
        ManifestErrors::from(ManifestError::EmptyPath {
            route: "empty".to_string(),
        })
    );
    assert_eq!(
        built_errors(built_route(
            0,
            "blank",
            &["", "/real"],
            None,
            &[],
            BTreeMap::new()
        )),
        ManifestErrors::from(ManifestError::EmptyPattern {
            route: "blank".to_string(),
            rule: 0,
        })
    );
}

#[test]
fn invalid_regexes_are_reported_with_their_rule() {
    // A document's `regex '…'` is compiled by the parser (§8.1), so the
    // validator only sees a path an editor's model carries — asserted there.
    let errors = built_errors(built_route(
        0,
        "about",
        &["/ok", "["],
        None,
        &[],
        BTreeMap::new(),
    ));
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

    // The document's own spelling of the same mistake is refused at the quote.
    let errors = parse_errors("view about regex '/ok', regex '[' {\n}\n");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::PatternRegex { pattern, .. } if pattern == "["
        ),
        "{errors}"
    );
}

#[test]
fn methods_must_be_uppercase_http_tokens() {
    // A `Blastfile` header takes `Method` tokens, so a lowercase word is a name
    // where a method or a pattern belongs, and the parser says so.
    let errors = parse_errors("view about get \"/about/\" {\n}\n");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::NotAMethod { word, .. } if word == "get"
        ),
        "{errors}"
    );
    // The upper-case methods a view may not declare are refused by lowering
    // (§8.4), at the method's own position.
    let errors = parse_errors("view about CONNECT \"/about/\" {\n}\n");
    assert_eq!(
        errors.to_string(),
        "1:12: `CONNECT` cannot reach a view; Blastit answers `GET`/`HEAD` static files and \
         forwards the methods you declare"
    );

    // The validator's token rule judges a model, so it keeps its exact list.
    assert_eq!(
        built_errors(built_route(
            0,
            "about",
            &[".*"],
            Some(&["get", "GE T", ""]),
            &[],
            BTreeMap::new()
        )),
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
    // A view header's method list is non-empty by grammar, so an empty one only
    // arrives through the model (an editor must not write it, §14.5).
    assert_eq!(
        built_errors(built_route(
            0,
            "about",
            &[".*"],
            Some(&[]),
            &[],
            BTreeMap::new()
        )),
        ManifestErrors::from(ManifestError::EmptyMethods {
            route: "about".to_string(),
        }),
        "an explicit empty filter would make the route unreachable"
    );
}

#[test]
fn valid_methods_are_accepted() {
    let source = "view about GET, MKCOL, M-SEARCH \"/about/\" {\n}\n";
    assert_eq!(validate_warnings(source), vec![]);
    let manifest = Manifest::from_blastfile_str(source).expect("parses");
    assert_eq!(
        texts(
            manifest
                .route(0)
                .expect("one route")
                .methods()
                .expect("the header lists them")
        ),
        ["GET", "HEAD", "MKCOL", "M-SEARCH"],
        "`GET` implies `HEAD`; the written order is kept (§8.4)"
    );
}

#[test]
fn the_last_route_uid_must_stay_below_65536() {
    let manifest = |routes: usize| {
        let mut source = String::new();
        for index in 0..routes {
            source.push_str(&format!("view r{index} regex '.*' {{\n}}\n\n"));
        }
        source
    };

    // uid = base + 1000 + index, and the §6 example has four views.
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
    assert_eq!(
        section_6().validate(64_532).expect("fits"),
        vec![ManifestWarning::EgressRoute {
            route: "checkout".to_string(),
        }]
    );

    // A base that is already too high for dbproxy.
    assert_eq!(
        Manifest::from_blastfile_str("")
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
        Manifest::from_blastfile_str("")
            .unwrap()
            .validate(64_536)
            .expect("no routes"),
        vec![]
    );

    // The boundary of the count, not just the sample manifests.
    assert_eq!(
        Manifest::from_blastfile_str(&manifest(1))
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
    Manifest::from_blastfile_str(&manifest(1))
        .unwrap()
        .validate(64_535)
        .expect("one route fits just below the cap");
}

#[test]
fn warnings_report_unattached_traits_and_ungranted_stores() {
    let source = "postgres $DATABASE_URL\n\
                  postgres unused $UNUSED_URL\n\
                  \n\
                  trait orphan {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  view about \"/about/\" {\n}\n";
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
fn attaching_a_trait_and_granting_a_store_silences_its_warning() {
    let source = "postgres $DATABASE_URL\n\
                  postgres unused $UNUSED_URL\n\
                  \n\
                  trait orphan {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  view about \"/about/\" {\n    use orphan\n    postgres unused {\n        query 'SELECT 2'\n    }\n}\n";
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn every_validation_problem_is_collected() {
    // Three rules of `validate` at once: a trait's `unrestricted`, a redacted
    // `query`, and two mounts sharing a prefix. Each is refused on its own, so
    // the list has to carry all three.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait wide {\n    postgres unrestricted\n}\n\
                  \n\
                  static \"/assets\" {\n    root \"public/assets\"\n}\n\
                  \n\
                  static \"/assets\" {\n    root \"public/assets2\"\n}\n\
                  \n\
                  view about \"/about/\" {\n    use wide\n    postgres {\n        query 'SELECT … FROM cms_page'\n    }\n}\n";
    let errors = validate_errors(source, 20_000);
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        ManifestError::UnrestrictedGroup {
            location: Location::Group(group),
            ..
        } if group == "wide"
    )));
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        ManifestError::RedactedQueryPlaceholder {
            location: Location::Route { name, .. },
            ..
        } if name == "about"
    )));
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        ManifestError::DuplicateMount { prefix, .. } if prefix == "/assets"
    )));

    let rendered = errors.to_string();
    assert!(
        rendered.contains("unrestricted is only allowed inline on a view"),
        "{rendered}"
    );
    assert!(
        rendered.contains("holds a redacted `…` placeholder"),
        "{rendered}"
    );
    assert!(
        rendered.contains("duplicate static mount \"/assets\""),
        "{rendered}"
    );
    assert_eq!(
        rendered.lines().count(),
        3,
        "one line per problem: {rendered}"
    );
}

#[test]
fn a_refusal_comes_back_before_validation_runs() {
    // `CONNECT` cannot reach a view (`§8.4`), and the file also declares a trait
    // nothing may use (`§11.3`). Lowering fails, so validation never runs and its
    // finding is not reported: no partial manifest, no second subject (R21).
    let errors = Manifest::parse_blastfile(
        "postgres $DATABASE_URL\n\
         \n\
         trait wide {\n    postgres unrestricted\n}\n\
         \n\
         view about CONNECT \"/about/\" {\n}\n",
        20_000,
    )
    .expect_err("the file is refused");
    assert_eq!(errors.len(), 1, "{errors}");
    assert!(
        matches!(
            &errors.errors()[0],
            ManifestError::MethodCannotReachView { method, .. } if method == "CONNECT"
        ),
        "{errors}"
    );
    assert!(
        errors.to_string().starts_with("7:12: "),
        "the refusal points at the method: {errors}"
    );
    assert!(
        !errors.to_string().contains("unrestricted"),
        "validation did not run: {errors}"
    );
}

#[test]
fn locations_read_well_in_errors() {
    assert_eq!(Location::Global.to_string(), "global groups");
    assert_eq!(
        Location::Group("session".to_string()).to_string(),
        "trait \"session\"",
        "a `[group.x]` and a `trait x` are one node, and it is spelled `trait` (§11.3)"
    );
    // A trait's grant is written in the trait, and both spellings of the
    // location render as the noun a developer edits.
    assert_eq!(
        Location::Trait("session".to_string()).to_string(),
        "trait \"session\""
    );
    assert_eq!(
        Location::Route {
            index: 2,
            name: "checkout".to_string(),
        }
        .to_string(),
        "route \"checkout\" (index 2)"
    );
    // §18: a view is named, not numbered — `(index 0)` would send a reader
    // looking for a numbering the file does not have.
    assert_eq!(
        Location::View {
            index: 0,
            name: "catalog".to_string(),
        }
        .to_string(),
        "view \"catalog\""
    );
}

#[test]
fn a_view_refusal_names_the_view_and_no_index() {
    // The rendering a `Blastfile` grant error reaches a developer with, end to
    // end: the store is unknown, so lowering reports it against the view (§13.1).
    let source = "postgres $DATABASE_URL\n\n\
                  view catalog \"/catalog/\" {\n    postgres analytics {\n        select products\n    }\n}\n";
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("unknown store");
    assert_eq!(
        errors.errors()[0].to_string(),
        "view \"catalog\": grant on unknown proxy \"analytics\"",
        "{errors}"
    );
    assert!(!errors.to_string().contains("(index"), "{errors}");
}

/// A manifest with one unrestricted inline grant per store, and a place to put
/// more.
const UNRESTRICTED: &str = "postgres $DATABASE_URL\n\
                            postgres analytics $ANALYTICS_URL\n\
                            \n\
                            view admin regex '/admin/.*' {\n    postgres unrestricted\n    postgres analytics unrestricted\n}\n\
                            \n\
                            view about \"/about/\" {\n    postgres {\n    }\n}\n";

#[test]
fn an_unrestricted_view_warns_once_per_view_and_store() {
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
        "one warning per (view, store), in view then store order"
    );
}

#[test]
fn an_unrestricted_grant_in_a_trait_is_refused_at_boot() {
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait backoffice {\n    postgres unrestricted\n}\n\
                  \n\
                  view admin regex '/admin/.*' {\n    use backoffice\n}\n";
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
        rendered.contains("unrestricted is only allowed inline on a view"),
        "{rendered}"
    );
    assert!(rendered.contains("trait \"backoffice\""), "{rendered}");

    // A trait attached nowhere is refused too: it could be attached to every
    // view at the next deploy, which is what the rule exists for.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait backoffice {\n    postgres unrestricted\n}\n\
                  \n\
                  view about \"/about/\" {\n}\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::from(ManifestError::UnrestrictedGroup {
            location: Location::Group("backoffice".to_string()),
            proxy: ProxyName::from("db"),
        })
    );

    // Attached by two views: same refusal. `use` is the only attach a file has
    // (§7.1), so "attached to every view at once" cannot be written at all.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait backoffice {\n    postgres unrestricted\n}\n\
                  \n\
                  view admin regex '/admin/.*' {\n    use backoffice\n}\n\
                  \n\
                  view help \"/help/\" {\n    use backoffice\n}\n";
    assert_eq!(
        validate_errors(source, 20_000),
        ManifestErrors::from(ManifestError::UnrestrictedGroup {
            location: Location::Group("backoffice".to_string()),
            proxy: ProxyName::from("db"),
        })
    );

    // An ordinary trait grant is an ordinary grant.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait backoffice {\n    postgres {\n        transactions\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  view admin regex '/admin/.*' {\n    use backoffice\n}\n";
    assert_eq!(validate_warnings(source), vec![]);
}

#[test]
fn an_unrestricted_grant_takes_no_queries() {
    // `postgres unrestricted` is a whole block and takes no directives (§13.1),
    // so a file cannot write the contradiction; the model can, and an editor
    // could propose it.
    let errors = built_errors(built_route(
        0,
        "admin",
        &["/admin/.*"],
        None,
        &[],
        on_db(postgres_grant(true, &["SELECT 1"], TableGrants::default())),
    ));
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::UnrestrictedWithQueries {
            location: Location::Route {
                index: 0,
                name: "admin".to_string(),
            },
            proxy: ProxyName::from("db"),
        })
    );
    let rendered = errors.to_string();
    assert!(
        rendered.contains("unrestricted grants take no queries"),
        "{rendered}"
    );
    assert!(rendered.contains("route \"admin\""), "{rendered}");

    // In a trait both rules are reported, and in the validator's fixed order.
    let groups: BTreeMap<String, Group> = [(
        "backoffice".to_string(),
        Group::new(
            "backoffice".to_string(),
            on_db(postgres_grant(true, &["SELECT 1"], TableGrants::default())),
        ),
    )]
    .into_iter()
    .collect();
    let manifest = Manifest::from_parts(
        1,
        Vec::new(),
        BTreeMap::new(),
        groups,
        vec![built_route(
            0,
            "admin",
            &["/admin/.*"],
            None,
            &["backoffice"],
            BTreeMap::new(),
        )],
    );
    assert_eq!(
        manifest.validate(20_000).expect_err("refused"),
        ManifestErrors::new(vec![
            ManifestError::UnrestrictedGroup {
                location: Location::Group("backoffice".to_string()),
                proxy: ProxyName::from("db"),
            },
            ManifestError::UnrestrictedWithQueries {
                location: Location::Group("backoffice".to_string()),
                proxy: ProxyName::from("db"),
            },
        ])
    );
}

#[test]
fn an_unrestricted_view_needs_no_query_and_gets_a_grant() {
    let manifest = Manifest::from_blastfile_str(UNRESTRICTED).expect("parses");
    manifest
        .validate(20_000)
        .expect("validates despite the warning");
    // The view gets a grant on both stores, so init binds both listeners
    // (§2.2, §6), and the second view's empty grant on `db` disappears.
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
fn a_trait_can_never_make_a_view_unrestricted() {
    // The check is about traits, not about the union: nothing a trait holds can
    // turn a view into one that may run anything.
    let source = "postgres $DATABASE_URL\n\
                  \n\
                  trait wide {\n    postgres {\n        transactions\n        query 'SELECT 1'\n    }\n}\n\
                  \n\
                  view about \"/about/\" {\n    use wide\n    postgres {\n        query 'SELECT 2'\n    }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("parses");
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

//
// Table and column grants (§6, D7).
//

/// One table grant naming every column of `table`.
fn all_columns_of(table: &str) -> TableGrants {
    TableGrants::new(
        [(
            table.to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    )
}

#[test]
fn an_unrestricted_grant_takes_no_tables() {
    // Same shape as the queries rule, and the same reason a file cannot write
    // it: `postgres unrestricted` has no block to hold a directive (§13.1).
    let built = |tables: TableGrants, what: &'static str| {
        built_errors(built_route(
            0,
            "notes",
            &["/notes/"],
            None,
            &[],
            on_db(postgres_grant(true, &[], tables)),
        ))
        .errors()
        .iter()
        .any(|error| {
            matches!(
                error,
                ManifestError::UnrestrictedWithContent { what: carried, .. } if *carried == what
            )
        })
    };
    assert!(built(all_columns_of("t"), "tables"), "tables are content");

    let functions = TableGrants::new(std::iter::empty(), ["unaccent".to_string()]);
    assert!(built(functions, "functions"), "functions are content");

    // ... and both at once, next to a query, is reported once per problem.
    let both = TableGrants::new(
        [(
            "t".to_string(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        ["f".to_string()],
    );
    let errors = built_errors(built_route(
        0,
        "notes",
        &["/notes/"],
        None,
        &[],
        on_db(postgres_grant(true, &["SELECT 1"], both)),
    ));
    assert_eq!(errors.len(), 3, "{errors}");
}

#[test]
fn a_system_catalogue_table_grant_is_refused() {
    // A table name is a `Word` in a grant (§13.1), so the unqualified names of
    // both catalogues are the ones a file can write.
    for table in [
        "pg_catalog",
        "information_schema",
        // An unqualified `pg_*` name resolves into `pg_catalog` (which is
        // always first on the search path), so a grant for it is refused too.
        "pg_class",
        "pg_type",
        // ... and a name that is one of the `information_schema` views.
        "tables",
        "columns",
    ] {
        let source = format!(
            "postgres $DATABASE_URL\n\nview notes \"/notes/\" {{\n    postgres {{\n        select {table}\n    }}\n}}\n"
        );
        let errors = validate_errors(&source, 20_000);
        assert_eq!(errors.len(), 1, "{table}: {errors}");
        assert!(
            errors.errors()[0].to_string().contains("system catalogue"),
            "{table}: {errors}"
        );
    }

    // A trait's table grants are checked too.
    let errors = validate_errors(
        "postgres $DATABASE_URL\n\
         \n\
         trait all {\n    postgres {\n        select pg_catalog\n    }\n}\n\
         \n\
         view notes \"/notes/\" {\n    use all\n}\n",
        20_000,
    );
    assert_eq!(errors.len(), 1, "{errors}");
    assert!(errors.to_string().contains("system catalogue"), "{errors}");

    // A schema-qualified name is not a `Word`, so neither the refused
    // `pg_catalog.pg_class` nor the accepted `audit.pg_class` can be written in
    // a grant (§13.1 records the loss); the rule keeps both halves on the model.
    let qualified = |table: &str| {
        built_route(
            0,
            "notes",
            &["/notes/"],
            None,
            &[],
            on_db(postgres_grant(false, &[], all_columns_of(table))),
        )
    };
    let errors = built_errors(qualified("pg_catalog.pg_class"));
    assert!(errors.to_string().contains("system catalogue"), "{errors}");
    assert_eq!(
        Manifest::from_parts(
            1,
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            vec![qualified("audit.pg_class")]
        )
        .validate(20_000)
        .expect("a schema of the app's own is a table like any other"),
        vec![]
    );
}

#[test]
fn table_grants_may_be_attached_to_a_trait() {
    // Only `unrestricted` is restricted to an inline grant: anything a trait
    // grants reaches every view that uses it, which is what traits are for.
    let warnings = validate_warnings(
        "postgres $DATABASE_URL\n\
         \n\
         trait catalog {\n    postgres {\n        select shop_product(id, name)\n        functions unaccent\n    }\n}\n\
         \n\
         view product-detail regex '/products/.*' {\n    use catalog\n}\n",
    );
    assert_eq!(warnings, vec![]);
}

//
// §13.4.5 / §18.1: a `query`'s own span, and the template refusals.
//

/// One view with one `postgres` grant whose lines are `grants`.
///
/// The grant's first `query` is on line 5 with eight spaces of indent, so the
/// first character inside its opening quote is column 16 — the hand-computed
/// origin every case below starts from.
fn query_grant(grants: &str) -> String {
    format!(
        "postgres $DATABASE_URL\n\n\
         view notes \"/notes/\" {{\n    postgres {{\n        {grants}\n    }}\n}}\n"
    )
}

#[test]
fn a_typo_in_a_one_line_query_is_refused_at_its_first_character() {
    // `SELCT` is the statement's first character, which is what `Syntax`'s
    // pinned offset is: the origin, never `Position::NONE`.
    let source = query_grant("query 'SELCT id FROM products WHERE price > $?'");
    let errors = validate_errors(&source, 20_000);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(5, 16),
            kind: TemplateKind::Syntax,
        }),
        "one line, one statement, one sentence at the statement's own first character"
    );
    // The sentence is the page's, fixed and whole, with the site in its own
    // column — and it never quotes the statement.
    let rendered = errors.to_string();
    assert_eq!(
        rendered,
        format!(
            "{}: {}",
            Position::at(5, 16),
            TemplateKind::Syntax.sentence()
        )
    );
    assert!(!rendered.contains("SELCT"), "{rendered}");
}

#[test]
fn a_defect_on_the_third_line_of_a_query_follows_the_newline() {
    // A raw newline inside the string moves the walk to the next line: the
    // `...` is 9 + 1 + 13 + 1 + 14 characters in, from (5, 16), so it is
    // (7, 15) — not the token's (5, 16).
    let source = query_grant("query 'SELECT id\nFROM products\nWHERE price > ...'");
    let errors = validate_errors(&source, 20_000);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(7, 15),
            kind: TemplateKind::StrayEllipsis,
        }),
        "the position follows the newline, not the token start"
    );

    // And a statement that *opens* with trivia: `Syntax` skips it, so the site
    // is the `S` on the line below — offset 3 from an origin at (5, 16).
    let leading = query_grant("query '\n  SELECT id FROM'");
    assert_eq!(
        validate_errors(&leading, 20_000),
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(6, 3),
            kind: TemplateKind::Syntax,
        }),
        "trivia is skipped, and the skipped newline still moves the line"
    );
}

#[test]
fn an_escaped_double_quoted_query_points_at_the_sql_token() {
    // `\"` costs two source columns for one decoded character, so the `...` is
    // at column 16 + 31 + 2 = 49; an identity map would say 47, and the
    // backslash before the first `\"` is at 23, not 49.
    let source = query_grant("query \"SELECT \\\"name\\\" FROM t WHERE a > ...\"");
    let errors = validate_errors(&source, 20_000);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(5, 49),
            kind: TemplateKind::StrayEllipsis,
        }),
        "the position is the SQL token, not the backslash"
    );

    // A second statement in the same string is refused on its own first
    // character: `SELECT 1; ` is ten decoded characters.
    let two = query_grant("query 'SELECT 1; SELECT 2'");
    assert_eq!(
        validate_errors(&two, 20_000),
        ManifestErrors::from(ManifestError::QueryTemplate {
            at: Position::at(5, 26),
            kind: TemplateKind::MultipleStatements,
        })
    );
}

#[test]
fn a_redacted_placeholder_and_an_inline_literal_carry_their_own_site() {
    // The `…` is the seventh character of `SELECT … FROM cms_page`.
    let redacted = query_grant("query 'SELECT … FROM cms_page'");
    let errors = validate_errors(&redacted, 20_000);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::RedactedQueryPlaceholder {
            location: Location::Route {
                index: 0,
                name: "notes".to_string(),
            },
            proxy: ProxyName::from("db"),
            rule: 0,
            at: Position::at(5, 23),
        })
    );
    assert!(!errors.errors()[0].position().is_none(), "{errors}");

    // The literal's quote, not the statement's origin: `SELECT "id" FROM t
    // WHERE a = ` is 29 decoded characters, and the two `\"` pairs before it
    // add two more source columns each — so the `'` is at column 16 + 29 + 2.
    let literal = query_grant("query \"SELECT \\\"id\\\" FROM t WHERE a = 'x'\"");
    let errors = validate_errors(&literal, 20_000);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::LiteralQuery {
            location: Location::Route {
                index: 0,
                name: "notes".to_string(),
            },
            proxy: ProxyName::from("db"),
            rule: 0,
            at: Position::at(5, 47),
        })
    );
    assert!(!errors.to_string().contains("'x'"), "{errors}");
}

#[test]
fn a_duplicate_query_keeps_the_first_site_and_one_sentence() {
    // Whitespace is not part of a statement, so these are one entry: one site,
    // one sentence, and the line 6 spelling is never mentioned. The statement
    // also fails to parse, so the one-sentence rule is under test here too.
    let source = query_grant(
        "query 'SELECT … FROM'\n\
                 query '  SELECT   …   FROM  '",
    );
    let manifest = Manifest::from_blastfile_str(&source).expect("parses");
    let sites = manifest
        .route(0)
        .expect("route")
        .grant(&ProxyName::from("db"))
        .expect("grant")
        .as_postgresql()
        .expect("postgresql")
        .query_sites();
    assert_eq!(sites.len(), 1, "one entry, one site");
    assert_eq!(sites[0].at, Position::at(5, 16));

    let errors = validate_errors(&source, 20_000);
    assert_eq!(errors.len(), 1, "{errors}");
    assert_eq!(errors.errors()[0].position(), Position::at(5, 23));
    assert!(
        !errors.to_string().contains("6:"),
        "the duplicate gets no diagnostic: {errors}"
    );
}

#[test]
fn a_file_past_the_parse_cap_is_refused_at_the_entry_past_it() {
    // §13.4.2: 4096 distinct entries are parsed; the 4097th is refused at its
    // own origin instead, which is what keeps a huge file from costing one
    // parse per entry on every publish.
    let mut grants = String::new();
    for index in 0..=4096 {
        grants.push_str(&format!("query 'SELECT {index}'\n        "));
    }
    let source = query_grant(grants.trim_end());
    let errors = validate_errors(&source, 20_000);
    assert_eq!(errors.len(), 1, "{errors}");
    assert_eq!(
        errors.errors()[0],
        ManifestError::QueryTemplate {
            // The 4097th query line: line 5 + 4096, still eight spaces in.
            at: Position::at(4101, 16),
            kind: TemplateKind::TooManyQueries,
        }
    );
}

#[test]
fn a_query_site_never_renders_the_statement_it_carries() {
    // §13.4.5: the site keeps the raw text so an offset can be computed, and
    // `Debug` is hand-written to redact it — `PgGrant` derives `Debug`, so a
    // `{:?}` in a log line must not print the value a `query` may hold.
    let source = query_grant("query \"SELECT 's3cr3t' FROM t\"");
    let manifest = Manifest::from_blastfile_str(&source).expect("parses");
    let grant = manifest
        .route(0)
        .expect("route")
        .grant(&ProxyName::from("db"))
        .expect("grant")
        .as_postgresql()
        .expect("postgresql");
    assert!(
        grant.query_sites()[0].raw.contains("s3cr3t"),
        "the value is what the site carries"
    );
    let rendered = format!("{:?}", grant.query_sites());
    assert!(
        !rendered.contains("s3cr3t"),
        "Debug leaked a value: {rendered}"
    );
    assert!(rendered.contains("SELECT … FROM t"), "{rendered}");
}

#[test]
fn a_site_points_at_the_origin_when_the_offset_is_past_the_text() {
    // §13.4.5's fallback: a walk that cannot be built — here an offset no
    // caller can produce from a document, so the model's only builder cannot
    // reach it — falls back to the string's own origin rather than inventing a
    // column. The fields are public, so the state is constructible on purpose.
    let site = blastit_manifest::QuerySite {
        at: Position::at(5, 16),
        interior: 0..0,
        escapes: Vec::new(),
        raw: "SELECT".to_string(),
    };
    assert_eq!(site.position_at(0), Position::at(5, 16));
    assert_eq!(
        site.position_at(6),
        Position::at(5, 22),
        "the end is a real offset"
    );
    assert_eq!(
        site.position_at(7),
        Position::at(5, 16),
        "past the end: the origin"
    );
    assert_eq!(site.position_at(u32::MAX), Position::at(5, 16));
}
