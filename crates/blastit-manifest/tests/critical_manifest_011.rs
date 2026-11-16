//! Protected critical suite for the 0.1.1 schema (`R7`, `R8`, `R11`, `R24`).
//!
//! Every pair here has the two halves a policy test needs: the documented
//! spelling that must parse and validate, and the mistake that must be refused
//! with the message the documentation promises. The negative half is the point
//! — a view that inlines a value, or a word written where the file has no
//! directive, has to fail the boot rather than reach an allowlist.
//!
//! The subject is the `Blastfile` language: a key the retired schema wrote as
//! `key = value` is now a word where a section belongs, and its refusal is the
//! ordinary `<line>:<col>` syntax error §15.1 promises.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{
    BodyCodec, IngressPolicy, Location, Manifest, ManifestError, ManifestErrors, ManifestWarning,
    Position, ProxyName, UidLayout,
};

const BASE: u32 = UidLayout::DEFAULT_BASE;

/// The top level's own list, as the `NotADirectiveHere` sentence renders it
/// (§6.7).
const TOP_LEVEL_TAKES: &str = "`app`, `entrypoint`, `postgres`, `trait`, `view` and `static`";

/// A grant block's own list, as the parser renders it (§6.7).
const GRANT_TAKES: &str =
    "`transactions`, `functions`, `select`, `insert`, `update`, `delete` and `query`";

fn parse(source: &str) -> Manifest {
    Manifest::from_blastfile_str(source).expect("manifest should parse")
}

fn parse_errors(source: &str) -> ManifestErrors {
    Manifest::from_blastfile_str(source).expect_err("manifest should not parse")
}

fn validate_errors(source: &str) -> ManifestErrors {
    Manifest::parse_blastfile(source, BASE).expect_err("manifest should not validate")
}

fn warnings(source: &str) -> Vec<ManifestWarning> {
    Manifest::parse_blastfile(source, BASE)
        .expect("manifest should validate")
        .1
}

/// One view whose `body` directive is exactly `ingress`.
fn route_with_ingress(ingress: &str) -> String {
    format!("view checkout \"/checkout/\" {{\n  {ingress}}}\n")
}

/// One view with one `postgres` grant whose lines are `grants`.
fn route_with_queries(grants: &str) -> String {
    format!(
        "postgres $DATABASE_URL\n\n\
         view notes \"/notes/\" {{\n    postgres {{\n        {grants}\n    }}\n}}\n"
    )
}

/// The `query` directive for `statement`, in whichever string spelling holds it.
///
/// A statement with a `'` is an inline literal the editor refuses, so it is
/// written as a `"…"` string (the only spelling that can hold a quote); the
/// rest go in the raw form.
fn query_line(statement: &str) -> String {
    if statement.contains('\'') || statement.contains('\\') {
        format!(
            "query \"{}\"",
            statement.replace('\\', "\\\\").replace('"', "\\\"")
        )
    } else {
        format!("query '{statement}'")
    }
}

/// The location and proxy every query test is written against.
fn route_grant() -> (Location, ProxyName) {
    (
        Location::Route {
            index: 0,
            name: "notes".to_string(),
        },
        ProxyName::from("db"),
    )
}

//
// R7: the callable, its preload list and the app name.
//

#[test]
fn app_and_preload_parse_in_both_spellings() {
    let manifest =
        parse("entrypoint python \"myshop.wsgi:application\" {\n    preload \"myshop.urls\"\n}\n");
    assert_eq!(manifest.app(), Some("myshop.wsgi:application"));
    assert_eq!(manifest.preload(), ["myshop.urls"]);
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);

    // An array keeps its order; a module named twice is imported once, so the
    // duplicate keeps its first position.
    let manifest = parse(
        "entrypoint python \"myshop.wsgi:application\" {\n    preload \"b\", \"a\", \"b\"\n}\n",
    );
    assert_eq!(manifest.preload(), ["b", "a"]);
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);

    // The entrypoint is optional: a file may carry only the app name, and
    // `BLASTIT_APP` may supply the callable.
    let manifest = parse("app \"shop\"\n");
    assert_eq!(
        manifest.app(),
        None,
        "app is optional; BLASTIT_APP may supply it"
    );
    assert_eq!(manifest.name(), Some("shop"));
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);
}

#[test]
fn app_must_be_module_attribute() {
    // Django's dotted path looks right and is the documented trap.
    let errors = validate_errors("entrypoint python \"myshop.wsgi.application\"\n");
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::InvalidApp {
            app: "myshop.wsgi.application".to_string(),
        })
    );
    assert!(
        errors.to_string().contains("must be `module:attribute`"),
        "{errors}"
    );

    // The allowed twin: a colon.
    assert!(
        Manifest::parse_blastfile("entrypoint python \"app:app\"\n", BASE).is_ok(),
        "a module:attribute app validates"
    );

    // An empty callable names nothing, so it is carried and refused rather than
    // dropped as "absent", where it would silently fall back to BLASTIT_APP.
    let empty = parse("entrypoint python \"\"\n");
    assert_eq!(
        empty.app(),
        Some(""),
        "the empty value is carried, not dropped"
    );
    assert_eq!(
        validate_errors("entrypoint python \"\"\n"),
        ManifestErrors::from(ManifestError::InvalidApp { app: String::new() })
    );
}

//
// R8: a reserved top-level key has no spelling in the file.
//

#[test]
fn a_reserved_key_after_a_table_is_an_ordinary_syntax_error() {
    // The retired schema wrote these keys as `key = value` below a `[table]`,
    // which its reader had to rescue (`R8`). The file has no tables and no
    // `=`: a key that is not a section is a word where a section belongs, and
    // `app = …` is refused at the `=` itself. Each refusal is the ordinary
    // positioned syntax error §15.1 promises, naming the fix.
    let cases = [
        (
            "version = 1\n",
            "1:1: `version` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, \
             `trait`, `view`, `static`",
        ),
        (
            "app = \"app:app\"\n",
            "1:5: `=` is not Blastfile syntax; write the value after the keyword: `body json`",
        ),
        (
            "preload = [\"mod\"]\n",
            "1:1: `preload` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, \
             `trait`, `view`, `static`",
        ),
        (
            "groups = [\"session\"]\n",
            "1:1: `groups` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, \
             `trait`, `view`, `static`",
        ),
        // The same key on a later line is refused at that line, not at the
        // first one: position is part of the contract.
        (
            "view about \"/about/\" {\n}\nversion = 1\n",
            "3:1: `version` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, \
             `trait`, `view`, `static`",
        ),
    ];

    for (source, expected) in cases {
        let errors = parse_errors(source);
        assert_eq!(errors.errors()[0].to_string(), expected, "for {source:?}");
        // The variant is positioned, and names the token's own line and column.
        match &errors.errors()[0] {
            ManifestError::UnknownKeyword { at, .. } | ManifestError::Equals { at } => {
                assert_ne!(*at, Position::NONE, "for {source:?}");
            }
            other => panic!("unexpected refusal for {source:?}: {other:?}"),
        }
    }
}

#[test]
fn groups_stays_top_level_at_the_top_and_inside_a_route() {
    // The allowed twins: a trait defined once and attached to a view. The file
    // has no file-wide group list, so `global_groups` is empty by construction
    // and the attachment is the view's own `use`.
    let source = "postgres $DATABASE_URL\n\
                  trait session {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\
                  view account \"/account/\" {\n    use session\n    postgres {\n        query 'SELECT 2'\n    }\n}\n";
    let manifest = parse(source);
    assert!(manifest.global_groups().is_empty());
    assert_eq!(manifest.route(0).expect("route").groups(), ["session"]);
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);

    // The denied twin: the attachment at the top is not a spelling the file
    // has. A top-level `groups` is not even a keyword, and `use` is a
    // directive of a view, so each names its own mistake.
    assert_eq!(
        parse_errors("groups = [\"session\"]\n").errors(),
        [ManifestError::UnknownKeyword {
            at: Position::at(1, 1),
            word: "groups".to_string(),
        }]
    );
    assert_eq!(
        parse_errors("use session\n").errors(),
        [ManifestError::NotADirectiveHere {
            at: Position::at(1, 1),
            word: "use".to_string(),
            block: "the top level",
            takes: TOP_LEVEL_TAKES,
        }]
    );
}

#[test]
fn author_chosen_names_may_repeat_a_top_level_key() {
    // Every name below belongs to the author: store names, a trait name, a
    // view name and a table. None of them is a misplaced top-level key, so all
    // of them must boot.
    let source = r#"app "shop"

postgres app $APP_DB_URL
postgres db $DB_URL
postgres groups $GROUPS_URL
postgres version $VERSION_URL

trait app {
    postgres {
        select t(id)
    }
    postgres groups {
        select t(id)
    }
    postgres version {
        select t(id)
    }
}

view catalog "/products/" {
    use app
    postgres app {
        select t(id)
    }
    postgres db {
        select groups(id)
    }
}
"#;
    let manifest = parse(source);
    assert_eq!(manifest.name(), Some("shop"));

    // `postgres app` is a store named `app`, and its input variable is the
    // author's.
    let proxy = manifest.proxy("app").expect("a store may be named app");
    assert_eq!(
        proxy.input().get("url").map(String::as_str),
        Some("APP_DB_URL")
    );
    // The retired `output` map has no spelling in the file, so the protocol's
    // default templates are the only ones: nothing is renamed.
    assert!(
        manifest.proxy("db").expect("store db").output().is_empty(),
        "the file cannot rename an output; the defaults apply"
    );
    assert!(manifest.proxy("groups").is_some());
    assert!(manifest.proxy("version").is_some());
    // The trait named `app` grants every awkward store name.
    let group = manifest.group("app").expect("a trait may be named app");
    assert_eq!(
        group
            .grants()
            .keys()
            .map(ProxyName::as_str)
            .collect::<Vec<_>>(),
        ["db", "groups", "version"]
    );
    // `postgres app { … }` is an inline grant on the store named `app`.
    assert!(
        manifest
            .route(0)
            .expect("route")
            .grant(&ProxyName::from("app"))
            .is_some()
    );
    // A table may be named `groups`.
    let tables = manifest
        .route(0)
        .expect("route")
        .grant(&ProxyName::from("db"))
        .expect("grant on db")
        .as_postgresql()
        .expect("postgresql")
        .tables();
    assert!(
        tables.table("groups").is_some(),
        "a table may be named groups"
    );
    assert_eq!(manifest.validate(BASE).expect("valid"), vec![]);

    // The denied twin: the names stay ordinary names, so a grant on a store
    // nothing declared is still refused, and the message names the view and
    // the store.
    let errors = validate_errors(
        "postgres db $DB_URL\n\nview a \"/a/\" {\n    postgres app { select t }\n}\n",
    );
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::GrantOnUnknownProxy {
            location: Location::View {
                index: 0,
                name: "a".to_string(),
            },
            proxy: ProxyName::from("app"),
        })
    );
    assert!(
        errors
            .to_string()
            .contains("grant on unknown proxy \"app\""),
        "{errors}"
    );
}

#[test]
fn a_reserved_key_inside_a_grant_body_is_not_an_unknown_field() {
    // A grant body is reached through `[group.<name>.<proxy>]` and
    // `[route.<proxy>]` in the retired schema; the file spells them
    // `trait <name> { postgres { … } }` and a view's `postgres [name] { … }`.
    // The same scoping mistake there comes back as the directive sentence,
    // naming the word and what a grant block takes — never as an "unknown
    // key", which would read as a field the schema quietly added.
    let group = |word: &str| {
        format!("postgres $DATABASE_URL\ntrait session {{\n    postgres {{ {word} }}\n}}\n")
    };
    let route = |word: &str| {
        format!(
            "postgres $DATABASE_URL\nview notes \"/notes/\" {{\n    postgres {{ {word} }}\n}}\n"
        )
    };

    let cases = [
        (group("version"), "version"),
        (group("app"), "app"),
        (group("preload"), "preload"),
        (group("allow_everything"), "allow_everything"),
        (route("version"), "version"),
        (route("app"), "app"),
        (route("preload"), "preload"),
        // A directive of another block, and an operation the grant grammar
        // does not have, are the same sentence: the contract is the place,
        // not a field list.
        (route("egress"), "egress"),
        (route("upsert"), "upsert"),
    ];

    for (source, word) in cases {
        let errors = parse_errors(&source);
        assert_eq!(
            errors.errors(),
            [ManifestError::NotADirectiveHere {
                at: Position::at(3, 16),
                word: word.to_string(),
                block: "a grant block",
                takes: GRANT_TAKES,
            }],
            "for {word:?} in {source:?} (not an unknown key)"
        );
        assert!(
            !errors.to_string().contains("unknown key"),
            "{source:?}: {errors}"
        );
    }

    // The allowed twin: the same two grant bodies with a directive the schema
    // does declare boot, so the test above is not refusing every grant body.
    let allowed = "postgres $DATABASE_URL\n\
                   trait session {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n\n\
                   view notes \"/notes/\" {\n    use session\n    postgres {\n        select t(id)\n    }\n}\n";
    let (manifest, warnings) =
        Manifest::parse_blastfile(allowed, BASE).expect("the grant bodies boot");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        manifest
            .group("session")
            .expect("trait")
            .grants()
            .keys()
            .map(ProxyName::as_str)
            .collect::<Vec<_>>(),
        ["db"]
    );
    assert!(
        manifest
            .route(0)
            .expect("route")
            .grant(&ProxyName::from("db"))
            .is_some()
    );
}

//
// R11: `body` and the two boot warnings.
//

#[test]
fn every_body_codec_parses() {
    for (spelling, codec) in [
        ("json", BodyCodec::Json),
        ("form", BodyCodec::Form),
        ("none", BodyCodec::None),
        ("raw", BodyCodec::Raw),
    ] {
        let manifest = parse(&route_with_ingress(&format!("body {spelling}\n")));
        assert_eq!(
            manifest.route(0).expect("route").ingress(),
            Some(IngressPolicy {
                body: codec,
                allow_raw: false,
            }),
            "for body {spelling:?}"
        );
        manifest.validate(BASE).expect("a declared body validates");
    }

    // `unvalidated` is meaningful only on `raw`, and defaults to false there.
    let manifest = parse(&route_with_ingress("body raw unvalidated\n"));
    assert_eq!(
        manifest.route(0).expect("route").ingress(),
        Some(IngressPolicy {
            body: BodyCodec::Raw,
            allow_raw: true,
        })
    );

    // No `body` at all: fail-closed, `None`.
    let manifest = parse("view about \"/about/\" {\n}\n");
    assert_eq!(manifest.route(0).expect("route").ingress(), None);
}

#[test]
fn unvalidated_is_refused_on_every_other_codec() {
    for codec in ["json", "form", "none"] {
        assert_eq!(
            parse_errors(&route_with_ingress(&format!("body {codec} unvalidated\n"))).to_string(),
            format!(
                "2:13: `unvalidated` acknowledges `raw` only; `body {codec}` already validates the body"
            ),
            "unvalidated says nothing on {codec:?}, so it must not look like it does"
        );
    }
}

#[test]
fn a_body_needs_a_known_codec() {
    assert_eq!(
        parse_errors(&route_with_ingress("body\n")).to_string(),
        "2:7: `body` needs a codec: `json`, `form`, `none` or `raw`"
    );
    assert_eq!(
        parse_errors(&route_with_ingress("body xml\n")).to_string(),
        "2:8: `xml` is not a codec; the codecs are `json`, `form`, `none`, `raw`"
    );
    // 0.1.1 has no multipart codec: the name is refused, not silently widened.
    assert!(
        parse_errors(&route_with_ingress("body multipart\n"))
            .to_string()
            .contains("`json`, `form`, `none`, `raw`")
    );
}

#[test]
fn raw_ingress_and_egress_are_boot_warnings() {
    let source = "view upload \"/upload/\" {\n    egress any\n    body raw unvalidated\n}\n";
    assert_eq!(
        warnings(source),
        vec![
            ManifestWarning::RawIngress {
                route: "upload".to_string()
            },
            ManifestWarning::EgressRoute {
                route: "upload".to_string()
            },
        ]
    );

    // The denied twin: no `raw` and no `egress` means neither warning, so the
    // test above is proving something rather than matching a constant list.
    let quiet = "view upload \"/upload/\" {\n    body json\n}\n";
    assert_eq!(warnings(quiet), vec![]);
}

//
// R9: both catch-all spellings.
//

#[test]
fn both_catch_all_spellings_are_unrestricted() {
    // `"/*"` is the file's catch-all; the anchored regexes are the same route.
    for path in ["\"/*\"", "regex '/.*'", "regex '^.*$'"] {
        let source = format!("view fallback {path} {{\n}}\n");
        assert!(
            parse(&source)
                .route(0)
                .expect("route")
                .is_unrestricted_catch_all(),
            "for path = {path}"
        );
    }

    for path in ["\"/products/*\"", "\"/\"", "regex '/products/[0-9]+'"] {
        let source = format!("view fallback {path} {{\n}}\n");
        assert!(
            !parse(&source)
                .route(0)
                .expect("route")
                .is_unrestricted_catch_all(),
            "for path = {path}"
        );
    }

    // An empty pattern list has no spelling: a view with no endpoint is
    // refused, so there is no unrestricted route to ask about.
    assert_eq!(
        parse_errors("view fallback {\n}\n").to_string(),
        "1:15: view \"fallback\" has no endpoint: declare at least `GET \"/products\"`"
    );

    // A catch-all restricted to some methods is not unrestricted: another
    // method still falls through to the route below it.
    let source = "view fallback GET \"/*\" {\n}\n";
    assert!(
        !parse(source)
            .route(0)
            .expect("route")
            .is_unrestricted_catch_all()
    );
}

//
// R24: no value-bearing literal reaches an allowlist.
//

#[test]
fn the_fixed_protocol_statements_may_keep_their_literal() {
    for query in [
        "SET TIME ZONE 'UTC'",
        "SELECT set_config('TimeZone', $?, false)",
    ] {
        let source = route_with_queries(&query_line(query));
        let (manifest, warnings) =
            Manifest::parse_blastfile(&source, BASE).expect("known setup SQL is grantable");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            manifest
                .route(0)
                .expect("route")
                .grant(&ProxyName::from("db"))
                .expect("grant")
                .as_postgresql()
                .expect("postgresql")
                .queries(),
            [query],
            "the two protocol constants are the only literal-bearing grants"
        );
    }
}

#[test]
fn the_wire_spelling_of_the_fixed_setup_statement_is_exempt() {
    // The wire carries `$1`; a hand-written file may spell the placeholder
    // that way. Lowering normalises it to `$?` (the editor's form), and
    // validation normalises again only to compare, so both spellings are the
    // same statement.
    let source = route_with_queries(&query_line("SELECT set_config('TimeZone', $1, false)"));
    let (manifest, warnings) =
        Manifest::parse_blastfile(&source, BASE).expect("the $1 spelling is the same statement");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        manifest
            .route(0)
            .expect("route")
            .grant(&ProxyName::from("db"))
            .expect("grant")
            .as_postgresql()
            .expect("postgresql")
            .queries(),
        ["SELECT set_config('TimeZone', $?, false)"],
        "the file normalises the wire spelling to the editor's `$?`"
    );

    // The exemption is that exact statement, not every `set_config` call: a
    // different literal is still a value-bearing grant.
    let sibling = route_with_queries(&query_line(
        "SELECT set_config('application_name', $1, false)",
    ));
    let (location, proxy) = route_grant();
    assert_eq!(
        validate_errors(&sibling),
        ManifestErrors::from(ManifestError::LiteralQuery {
            location,
            proxy,
            rule: 0,
        })
    );
}

#[test]
fn a_redacted_placeholder_cannot_be_granted() {
    let source =
        route_with_queries(r#"query 'SELECT "notes"."body" FROM "notes" WHERE "notes"."id" = …'"#);
    let (location, proxy) = route_grant();
    assert_eq!(
        validate_errors(&source),
        ManifestErrors::from(ManifestError::RedactedQueryPlaceholder {
            location,
            proxy,
            rule: 0,
        })
    );

    // The allowed twin: the same shape with the value bound.
    let parameterised =
        route_with_queries(r#"query 'SELECT "notes"."body" FROM "notes" WHERE "notes"."id" = $?'"#);
    assert!(Manifest::parse_blastfile(&parameterised, BASE).is_ok());
}

#[test]
fn an_inline_literal_is_refused_and_never_echoed() {
    let source = route_with_queries(
        "query \"SELECT \\\"notes\\\".\\\"name\\\" FROM \\\"notes\\\" \
         WHERE \\\"notes\\\".\\\"name\\\" = 'secret'\"",
    );
    let (location, proxy) = route_grant();
    let errors = validate_errors(&source);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::LiteralQuery {
            location: location.clone(),
            proxy: proxy.clone(),
            rule: 0,
        })
    );
    // The message names the position and the fix, never the value: an error is
    // logged, and a log line is not a place for it.
    let rendered = errors.to_string();
    assert!(!rendered.contains("secret"), "{rendered}");
    assert!(rendered.contains("queries[0]"), "{rendered}");
    assert!(rendered.contains("bind the value"), "{rendered}");

    // A trait grant is refused too: it may attach to many views at once.
    let group = "postgres $DATABASE_URL\n\
                 trait session {\n    postgres {\n        query \"SELECT 'secret'\"\n    }\n}\n\n\
                 view account \"/account/\" {\n    use session\n}\n";
    let errors = validate_errors(group);
    assert_eq!(
        errors,
        ManifestErrors::from(ManifestError::LiteralQuery {
            location: Location::Group("session".to_string()),
            proxy: ProxyName::from("db"),
            rule: 0,
        })
    );
    assert!(!errors.to_string().contains("secret"));
}

#[test]
fn every_literal_form_in_a_grant_is_refused() {
    // The forms the lexer knows, each written in the string spelling that can
    // hold it.
    for query in [
        "SELECT E'x'",
        "SELECT B'1'",
        "SELECT X'ff'",
        "SELECT N'x'",
        "SELECT $$x$$",
        "SELECT $tag$x$tag$",
        "SELECT U&'x'",
    ] {
        let source = route_with_queries(&query_line(query));
        let (location, proxy) = route_grant();
        assert_eq!(
            validate_errors(&source),
            ManifestErrors::from(ManifestError::LiteralQuery {
                location,
                proxy,
                rule: 0,
            }),
            "a literal-bearing grant slipped through: {query:?}"
        );
    }
}

//
// The plumbing shared with `blastit check`.
//

#[test]
fn a_statement_is_covered_by_tables_or_by_groups_but_not_by_itself() {
    let source = r#"postgres $DATABASE_URL

trait session {
    postgres {
        query 'SELECT "orders"."id" FROM "orders"'
    }
}

view catalog "/products/" {
    use session
    postgres {
        query 'SELECT "secrets"."token" FROM "secrets"'
        select products(id)
    }
}
"#;
    let manifest = Manifest::parse_blastfile(source, BASE).expect("valid").0;
    let db = ProxyName::from("db");

    // Covered by the view's own table grant.
    assert!(manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "products"."id" FROM "products" WHERE "products"."id" = $?"#,
    ));
    // Covered by the attached trait's exact query.
    assert!(manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "orders"."id" FROM "orders""#,
    ));
    // The view's own `query` entry is the one being judged: it cannot be its
    // own coverage, or every `dead_grant` would hide itself.
    assert!(!manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "secrets"."token" FROM "secrets""#,
    ));
    // Neither the trait nor the table grants mention this.
    assert!(!manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "audit"."id" FROM "audit""#,
    ));
    // An index no view has covers nothing.
    assert!(!manifest.is_query_covered_by_tables_or_groups(9, &db, "SELECT 1"));

    // A quoted identifier holding `$?` is an identifier, not a placeholder:
    // rewriting it would ask the analysis about a table the grant never named.
    let quoted = r#"postgres $DATABASE_URL

view odd "/odd/" {
    postgres {
        select a(b), notes(id)
    }
}
"#;
    let manifest = Manifest::parse_blastfile(quoted, BASE).expect("valid").0;
    assert!(manifest.is_query_covered_by_tables_or_groups(
        0,
        &db,
        r#"SELECT "notes"."id" FROM "notes""#,
    ));
    assert!(
        !manifest.is_query_covered_by_tables_or_groups(0, &db, r#"SELECT "a$?"."b" FROM "a$?""#,),
        "the `$?` inside the identifier must not be erased into the granted table `a`"
    );
}
