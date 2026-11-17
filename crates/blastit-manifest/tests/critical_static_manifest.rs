//! The `static "<prefix>" { … }` mount's grammar, its fatal errors and the root
//! audit (`R1`–`R5`, `R13`; design §7.2, §9.1).
//!
//! Protected (critical tier). A mount's `root`, `index`, `fallback` and
//! `immutable` decide which files the responder serves, so a regression here
//! would serve a directory the operator never declared — or publish a dotfile
//! with every miss. Every accepted form has its denied twin, and each family
//! asserts the exact boot message the design pins, not merely "an error".
//!
//! The subject moved and the promises did not: a static block is a top-level
//! `static "<prefix>"` mount and no longer a route's table (§9.1), so every
//! table-driven case asserts `manifest.mounts()[i].policy` and every refusal
//! names `mount "<prefix>" (index <i>)`. A mount owns no uid, starts no process
//! and reaches no app, so the four declarations a static route could never run
//! are refused one step earlier, by the parser's own directive list.
//!
//! Existence is a separate call by design: [`Manifest::validate`] never touches
//! the filesystem (the documentation fences are parsed from the repository root,
//! where no `public/` exists), so `check_static_roots` is proven on both sides.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use blastit_manifest::{
    Location, Manifest, ManifestError, ManifestWarning, Position, ProxyName, StaticPolicy,
    UidLayout,
    blastfile::{StaticImmutable, StaticPattern},
};

/// The accepted page's complete `Blastfile` (`docs/frameworks/static-and-spa.md`
/// still carries the retired spelling until `MB.21.5.1` rewrites it to this
/// file; the four-step precedence walk is `critical_blastfile_parity.rs`'s, so
/// this suite keeps the per-key grammar).
const PAGE: &str = r#"static "/" {
    root "public"
    index "index.html"
    fallback "index.html"
    immutable "/assets/*"
}
"#;

/// The one mount every table-driven case declares.
fn mount_location() -> Location {
    Location::Mount {
        index: 0,
        prefix: "/".to_string(),
    }
}

/// A file with the one mount, holding `body` exactly as written.
fn static_file(body: &str) -> String {
    format!("static \"/\" {{\n{body}}}\n")
}

/// Parses a file that must parse, and returns what validation refuses.
fn errors_of(source: &str) -> Vec<ManifestError> {
    let manifest = Manifest::from_blastfile_str(source)
        .unwrap_or_else(|errors| panic!("should parse:\n{errors}\n{source}"));
    manifest
        .validate(UidLayout::DEFAULT_BASE)
        .expect_err("should not validate")
        .into_errors()
}

/// Parses a file that must not parse, and returns its problems.
fn parse_errors(source: &str) -> Vec<ManifestError> {
    Manifest::from_blastfile_str(source)
        .expect_err("should not parse")
        .into_errors()
}

/// Everything a mount body is refused for, at whichever stage refuses it: the
/// `root` and `index` rules belong to lowering, and `immutable`, `fallback` and
/// the prefix to validation. Both stages render one line (§18), so the families
/// below can assert the sentence without pinning the stage by accident.
fn refusals(body: &str) -> Vec<ManifestError> {
    let source = static_file(body);
    match Manifest::from_blastfile_str(&source) {
        Err(errors) => errors.into_errors(),
        Ok(manifest) => manifest
            .validate(UidLayout::DEFAULT_BASE)
            .expect_err("should not validate")
            .into_errors(),
    }
}

/// The refusals of a mount body, rendered exactly as the boot prints them.
fn lines(body: &str) -> Vec<String> {
    refusals(body).iter().map(ToString::to_string).collect()
}

/// Parses and validates a mount body that must be accepted; one mount alone has
/// no view, group or store to warn about.
fn accepted(body: &str) -> Manifest {
    let source = static_file(body);
    let (manifest, warnings) = Manifest::parse_blastfile(&source, UidLayout::DEFAULT_BASE)
        .unwrap_or_else(|errors| panic!("should boot:\n{errors}\n{source}"));
    assert_eq!(warnings, vec![], "a mount on its own warns about nothing");
    assert_eq!(
        manifest.mounts().len(),
        1,
        "one `static` block is one mount"
    );
    assert!(manifest.routes().is_empty(), "a mount is not a route");
    manifest
}

/// The policy of the one mount the file declares.
fn policy(manifest: &Manifest) -> &StaticPolicy {
    &manifest.mounts().first().expect("a mount").policy
}

fn root_invalid(root: &str, rule: &'static str) -> ManifestError {
    ManifestError::StaticRootInvalid {
        location: mount_location(),
        root: root.to_string(),
        rule,
    }
}

fn index_invalid(index: &str, rule: &'static str) -> ManifestError {
    ManifestError::StaticIndexInvalid {
        location: mount_location(),
        index: index.to_string(),
        rule,
    }
}

fn fallback_invalid(fallback: &str, rule: &'static str) -> ManifestError {
    ManifestError::StaticFallbackInvalid {
        location: mount_location(),
        fallback: fallback.to_string(),
        rule,
    }
}

#[test]
fn the_pages_mount_parses_with_the_defaults_it_omits() {
    let (manifest, warnings) =
        Manifest::parse_blastfile(PAGE, UidLayout::DEFAULT_BASE).expect("the page boots");
    assert_eq!(warnings, vec![]);
    assert_eq!(manifest.app(), None, "the page's mount needs no app");
    assert!(manifest.is_all_static());
    assert!(manifest.routes().is_empty(), "a mount is not a route");
    assert_eq!(manifest.mounts().len(), 1);
    assert_eq!(manifest.mounts()[0].prefix, "/");
    assert_eq!(
        policy(&manifest),
        &StaticPolicy {
            root: "public".to_string(),
            // Omitted by the page, so the default has to appear here.
            index: "index.html".to_string(),
            // The page answers a miss with the file it answers a directory
            // with (§9.3).
            fallback: Some("index.html".to_string()),
            immutable: StaticImmutable::Patterns(vec![
                StaticPattern::quoted("/assets/*").expect("the page's pattern"),
            ]),
        }
    );

    // The accepted twin of every default: the one required key, and the three
    // omitted ones filled in. Omitting `fallback` is the retired `spa = false`
    // (§9.3).
    assert_eq!(
        policy(&accepted("root \"public\"\n")),
        &StaticPolicy {
            root: "public".to_string(),
            index: "index.html".to_string(),
            fallback: None,
            immutable: StaticImmutable::None,
        }
    );
    let explicit = accepted(
        "root \"public\"\nindex \"shell.html\"\nimmutable \"/assets/*\", \"/index.html\"\n",
    );
    assert_eq!(policy(&explicit).index, "shell.html");
    assert_eq!(
        policy(&explicit).immutable.texts().collect::<Vec<_>>(),
        ["/assets/*", "/index.html"],
        "the list keeps written order"
    );
}

#[test]
fn every_bad_root_names_the_rule_it_breaks() {
    let cases = [
        ("root \"\"\n", root_invalid("", "must be a relative path")),
        (
            "root \"/srv/public\"\n",
            root_invalid("/srv/public", "must be a relative path"),
        ),
        ("root \"/\"\n", root_invalid("/", "must be a relative path")),
        (
            "root \"../public\"\n",
            root_invalid("../public", "must not contain .."),
        ),
        (
            "root \"current/../../public\"\n",
            root_invalid("current/../../public", "must not contain .."),
        ),
        (
            "root \"pub\\\\lic\"\n",
            root_invalid(r"pub\lic", r"must not contain \"),
        ),
        // `blastfile/lower.rs` `root_rule`'s inline copy is `byte < 0x20`; the
        // lexer copies every byte that is neither a quote nor a backslash
        // verbatim, so a raw NUL and a raw `0x1f` pin both ends of that range
        // (and a predicate that only knew `\n`/`\t` fails on either).
        (
            "root \"pub\u{0}lic\"\n",
            root_invalid("pub\u{0}lic", "must not contain a control byte"),
        ),
        (
            "root \"pub\u{1f}lic\"\n",
            root_invalid("pub\u{1f}lic", "must not contain a control byte"),
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(refusals(body), vec![expected], "{body}");
    }

    // The message the operator reads: the mount, the value and the fix.
    assert_eq!(
        lines("root \"/srv/public\"\n")[0],
        r#"mount "/" (index 0): static root "/srv/public": must be a relative path"#
    );

    // Every relative spelling is fine, a space included.
    for root in ["public", "./public", "a/b", "current/public", "pub lic"] {
        assert_eq!(policy(&accepted(&format!("root \"{root}\"\n"))).root, root);
    }
}

#[test]
fn a_mount_without_a_root_and_a_mount_without_a_prefix_are_refused() {
    // §9.1: `root` is required — a default would answer nothing and make the
    // omission invisible.
    assert_eq!(
        parse_errors(&static_file("")),
        vec![ManifestError::StaticRootMissing {
            at: Position::at(1, 1),
        }]
    );
    assert_eq!(
        parse_errors(&static_file(""))
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec![r#"1:1: a `static` block needs `root`: write `root "public"`"#]
    );

    // A mount that names no prefix can never be reached — the retired suite's
    // "a static route with no `path`", on the construct that replaced it.
    assert_eq!(
        errors_of("static \"\" {\n    root \"public\"\n}\n"),
        vec![ManifestError::StaticMountPrefixInvalid {
            location: Location::Mount {
                index: 0,
                prefix: String::new(),
            },
            rule: "must start with /",
        }]
    );
    assert_eq!(
        errors_of("static \"\" {\n    root \"public\"\n}\n")[0].to_string(),
        r#"mount "" (index 0): must start with /"#
    );

    // Neither half of the header may be omitted: the prefix is a string and
    // the block is a block.
    assert_eq!(
        parse_errors("static {\n    root \"public\"\n}\n")
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec![r#"1:8: expected a `"…"` prefix, found `{`"#]
    );
    assert_eq!(
        parse_errors("static \"/\" \"public\"\n")
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["1:12: expected `{`, found a string"]
    );
}

#[test]
fn an_unknown_static_directive_is_refused_not_dropped() {
    // The design names this one: a `static` block is never a grant table, and
    // its four directives are the whole list.
    for (body, word) in [
        ("queries \"SELECT 1\"\n", "queries"),
        ("tables\n", "tables"),
        ("listen \"run.sock\"\n", "listen"),
    ] {
        assert_eq!(
            lines(body),
            vec![format!(
                "2:1: `{word}` is not a directive here; a `static` block takes \
                 `root`, `index`, `fallback` and `immutable`"
            )],
            "{body}"
        );
    }
}

#[test]
fn every_static_directive_takes_one_string() {
    // A value that is not a `"…"` string is named by its class, never coerced
    // and never echoed (§18).
    let cases = [
        ("root 1\n", "2:6", "a `\"…\"` path", "`1`"),
        ("root 'public'\n", "2:6", "a `\"…\"` path", "a raw string"),
        (
            "root \"public\"\nindex true\n",
            "3:7",
            "a `\"…\"` path",
            "`true`",
        ),
        (
            "root \"public\"\nfallback 1\n",
            "3:10",
            "a `\"…\"` path",
            "`1`",
        ),
        ("root \"public\"\nimmutable 1\n", "3:11", "a pattern", "`1`"),
    ];
    for (body, at, expected, found) in cases {
        assert_eq!(
            lines(body),
            vec![format!("{at}: expected {expected}, found {found}")],
            "{body}"
        );
    }

    // A second, non-string item is the list rule, and the sentence names the
    // comma to delete rather than inventing a type for the value.
    assert_eq!(
        lines("root \"public\"\nimmutable \"/a\", 2\n"),
        vec!["3:15: a list has no trailing comma; delete the `,`"]
    );

    // One item is a list, so the single-pattern spelling is legal.
    assert_eq!(
        policy(&accepted("root \"public\"\nimmutable \"/assets/*\"\n"))
            .immutable
            .texts()
            .collect::<Vec<_>>(),
        ["/assets/*"]
    );
}

#[test]
fn every_bad_index_names_the_rule_it_breaks() {
    let cases = [
        (
            "index \"\"\n",
            index_invalid("", "must be one or more segments"),
        ),
        (
            "index \"a//b\"\n",
            index_invalid("a//b", "must be one or more segments"),
        ),
        (
            "index \"a/\"\n",
            index_invalid("a/", "must be one or more segments"),
        ),
        (
            "index \"/index.html\"\n",
            index_invalid("/index.html", "must be a relative path"),
        ),
        (
            "index \"../index.html\"\n",
            index_invalid("../index.html", "must not contain .."),
        ),
        (
            "index \".env\"\n",
            index_invalid(".env", "must not contain a dotfile segment"),
        ),
        (
            "index \"docs/.env\"\n",
            index_invalid("docs/.env", "must not contain a dotfile segment"),
        ),
        (
            "index \"a\\\\b\"\n",
            index_invalid(r"a\b", r"must not contain \"),
        ),
        (
            "index \"a\u{0}b\"\n",
            index_invalid("a\u{0}b", "must not contain a control byte"),
        ),
        (
            "index \"a\u{1f}b\"\n",
            index_invalid("a\u{1f}b", "must not contain a control byte"),
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(
            refusals(&format!("root \"public\"\n{body}")),
            vec![expected],
            "{body}"
        );
    }
    assert_eq!(
        lines("root \"public\"\nindex \".env\"\n")[0],
        r#"mount "/" (index 0): static index ".env": must not contain a dotfile segment"#
    );

    for index in ["index.html", "shell.html", "docs/index.html", "a b.html"] {
        let manifest = accepted(&format!("root \"public\"\nindex \"{index}\"\n"));
        assert_eq!(policy(&manifest).index, index);
    }
}

#[test]
fn every_bad_immutable_pattern_gets_the_view_pattern_sentence() {
    // `immutable` takes the one pattern grammar (§8.1), so each refusal below
    // is the sentence **and** the `line:col` a `view` pattern gets, at the
    // pattern's own opening quote — never a `static immutable "…"` second
    // vocabulary, and never the mount as the subject.
    let over_cap = format!("/{}", "a".repeat(4096));
    let cases: Vec<(String, String)> = vec![
        (
            "immutable \"assets/*\"\n".to_owned(),
            "3:11: a pattern starts with `/`: write `/x`".to_owned(),
        ),
        (
            "immutable \"\"\n".to_owned(),
            "3:11: a pattern starts with `/`: write `/x`".to_owned(),
        ),
        (
            "immutable \"/a*b\"\n".to_owned(),
            "3:11: `*` is only valid at the end of a pattern; write `/x/*`".to_owned(),
        ),
        (
            "immutable \"/a/*/b\"\n".to_owned(),
            "3:11: `*` is only valid at the end of a pattern; write `/x/*`".to_owned(),
        ),
        (
            "immutable \"/x/{Id}\"\n".to_owned(),
            "3:11: `{Id}` is not a parameter name; names start with a lower-case letter and \
             are `[a-z][a-z0-9_]*`"
                .to_owned(),
        ),
        (
            "immutable \"/x/{id:str}\"\n".to_owned(),
            "3:11: `{id:str}` is not a pattern type; the types are `int` and `uuid`".to_owned(),
        ),
        (
            "immutable \"/x/{a}/{a:int}\"\n".to_owned(),
            "3:11: the parameter `{a}` appears twice; a pattern names each parameter once"
                .to_owned(),
        ),
        (
            format!("immutable \"{over_cap}\"\n"),
            "3:11: a pattern holds at most 4096 bytes".to_owned(),
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(
            lines(&format!("root \"public\"\n{body}")),
            vec![expected.clone()],
            "{body}"
        );
    }
    // The values are the pattern's own variants, so a caller never parses a
    // message back into an error (`view`'s refusal set, reused).
    assert_eq!(
        refusals("root \"public\"\nimmutable \"assets/*\"\n"),
        vec![ManifestError::PatternNotAnchored {
            at: Position::at(3, 11),
        }]
    );
    assert_eq!(
        refusals("root \"public\"\nimmutable \"/a/*/b\"\n"),
        vec![ManifestError::PatternStarPosition {
            at: Position::at(3, 11),
        }]
    );
    assert_eq!(
        refusals("root \"public\"\nimmutable \"/x/{a}/{a:int}\"\n"),
        vec![ManifestError::PatternParameterDuplicate {
            at: Position::at(3, 11),
            name: "a".to_owned(),
        }]
    );

    // An expression the crate refuses is refused at the raw string, with the
    // crate's own message — not a `regex` that silently caches nothing.
    let rendered = lines("root \"public\"\nimmutable regex '('\n");
    assert_eq!(rendered.len(), 1);
    assert!(
        rendered[0].starts_with("3:17: regex `(` does not compile: "),
        "{rendered:?}"
    );
    assert!(
        matches!(
            refusals("root \"public\"\nimmutable regex '('\n").as_slice(),
            [ManifestError::PatternRegex { at, .. }] if *at == Position::at(3, 17)
        ),
        "the refusal points at the expression"
    );
}

#[test]
fn a_33rd_immutable_pattern_is_refused_at_its_own_pattern() {
    // §9.4's list cap, refused where a developer can jump to it: the 33rd
    // pattern, not the mount, and not after the fact.
    let items: Vec<String> = (0..33).map(|at| format!("\"/a{at}\"")).collect();
    let line = format!("immutable {}", items.join(", "));
    // The 33rd item as written, quote included: its quote is where the refusal
    // points, exactly as a `view` pattern's does.
    let quote = line
        .find(items.get(32).expect("33 items").as_str())
        .expect("the 33rd item is in the list");
    let body = format!("{line}\n");
    let rendered = lines(&format!("root \"public\"\n{body}"));
    assert_eq!(
        rendered,
        vec![format!(
            "3:{}: a mount holds at most 32 `immutable` patterns; one pattern whose trailing \
             `/*` crosses `/` reaches every depth, and `regex '…'` reaches a hashed name",
            quote + 1
        )],
        "{line}"
    );

    // Exactly 32 is legal, and every one of them survives in written order.
    let boundary: Vec<String> = (0..32).map(|at| format!("\"/a{at}\"")).collect();
    let manifest = accepted(&format!(
        "root \"public\"\nimmutable {}\n",
        boundary.join(", ")
    ));
    assert_eq!(
        policy(&manifest)
            .immutable
            .texts()
            .collect::<Vec<_>>()
            .len(),
        32
    );
}

#[test]
fn the_three_immutable_shapes_and_their_denied_twins() {
    // Bare `immutable` is `All`, a value of its own — not "no patterns", and
    // not a `"/*"` someone could mistake for a written catch-all.
    let bare = accepted("root \"public\"\nimmutable\n");
    assert_eq!(policy(&bare).immutable, StaticImmutable::All);
    assert!(policy(&bare).immutable.is_all());
    assert_eq!(policy(&bare).immutable.patterns().len(), 0);
    for path in ["/", "/a", "/assets/app.js", "/static/admin/js/vendor/x.js"] {
        assert!(policy(&bare).immutable.matches(path), "{path}");
    }

    // No directive is `None`, and it caches nothing.
    let none = accepted("root \"public\"\n");
    assert_eq!(policy(&none).immutable, StaticImmutable::None);
    assert!(!policy(&none).immutable.is_all());
    for path in ["/", "/assets/app.js"] {
        assert!(!policy(&none).immutable.matches(path), "{path}");
    }

    // A trailing `/*` crosses `/` at any depth: the Django page's one line.
    let deep = accepted("root \"public\"\nimmutable \"/static/*\"\n");
    let immutable = &policy(&deep).immutable;
    for path in [
        "/static/app.js",
        "/static/admin/js/vendor/select2/i18n/de.js",
        "/static/",
    ] {
        assert!(immutable.matches(path), "{path} must match `/static/*`");
    }
    for path in ["/static", "/staticfiles/app.js", "/assets/app.js"] {
        assert!(
            !immutable.matches(path),
            "{path} must not match `/static/*`"
        );
    }

    // A literal dot and the two typed segments, each with its near miss.
    let typed = accepted(
        "root \"public\"\nimmutable \"/v1.0/app.css\", \"/orders/{id:int}/invoice.pdf\", \
         \"/users/{id:uuid}.json\"\n",
    );
    let immutable = &policy(&typed).immutable;
    assert_eq!(
        immutable.texts().collect::<Vec<_>>(),
        [
            "/v1.0/app.css",
            "/orders/{id:int}/invoice.pdf",
            "/users/{id:uuid}.json"
        ],
        "written order, text as compiled"
    );
    assert!(immutable.matches("/v1.0/app.css"));
    assert!(!immutable.matches("/v1x0/app.css"));
    assert!(immutable.matches("/orders/42/invoice.pdf"));
    assert!(!immutable.matches("/orders/abc/invoice.pdf"));
    assert!(immutable.matches("/users/6ba7b810-9dad-11d1-80b4-00c04fd430c8.json"));
    assert!(!immutable.matches("/users/not-a-uuid.json"));

    // The escape hatch: one expression for a hashed name at any depth.
    let regex = accepted("root \"public\"\nimmutable regex '/static/.+\\.[0-9a-f]{12}\\.[^/]+'\n");
    let immutable = &policy(&regex).immutable;
    assert_eq!(
        immutable.texts().collect::<Vec<_>>(),
        ["/static/.+\\.[0-9a-f]{12}\\.[^/]+"],
        "the raw form is kept verbatim"
    );
    assert!(
        immutable.matches("/static/admin/js/vendor/select2/i18n/de.0123456789ab.js"),
        "one expression reaches Django's deepest hashed file"
    );
    assert!(
        !immutable.matches("/static/admin/js/vendor/select2/i18n/de.js"),
        "an unhashed name is not matched"
    );
}

/// The spelling travels with the pattern (`MB.31.2` review): a `regex '…'` entry
/// is not a glob, so a consumer that only knows the compiled grammar can skip it
/// instead of re-reading its text.
#[test]
fn an_immutable_pattern_records_its_spelling() {
    let manifest = accepted("root \"public\"\nimmutable \"/a/*\", regex '/b/*'\n");
    let patterns = policy(&manifest).immutable.patterns();
    assert_eq!(patterns.len(), 2, "one entry per item, bare excluded");
    assert_eq!(patterns[0].text(), "/a/*");
    assert!(!patterns[0].is_regex(), "`\"/a/*\"` is the quoted grammar");
    assert_eq!(patterns[1].text(), "/b/*");
    assert!(patterns[1].is_regex(), "`regex '…'` keeps its spelling");

    // `regex '/b/*'` is `/b` then zero or more `/`: it denies `/b/app.js`, which
    // a stop-at-`/` glob reading the same text would accept.
    assert!(patterns[1].matches("/b"));
    assert!(!patterns[1].matches("/b/app.js"));
    // And through the policy, both directions: the quoted entry caches, the
    // regex entry does not.
    assert!(policy(&manifest).immutable.matches("/a/app.js"));
    assert!(!policy(&manifest).immutable.matches("/b/app.js"));
}

#[test]
fn a_fallback_is_its_own_file_and_a_dotfile_one_is_refused() {
    // The `spa` key's replacement, both directions (§9.3): the file that
    // answers a miss is a different file from the directory's index …
    let deep = accepted("root \"public\"\nindex \"index.html\"\nfallback \"shell.html\"\n");
    assert_eq!(policy(&deep).index, "index.html");
    assert_eq!(policy(&deep).fallback.as_deref(), Some("shell.html"));

    // … and it is a *file name*, so it carries the index's rules plus the one
    // that matters: a dotfile segment is refused, because the fallback answers
    // a request the request-path check never sees.
    for (fallback, expected) in [
        ("", fallback_invalid("", "must be one or more segments")),
        (
            "a//b",
            fallback_invalid("a//b", "must be one or more segments"),
        ),
        (
            ".env",
            fallback_invalid(".env", "must not contain a dotfile segment"),
        ),
        (
            "docs/.env",
            fallback_invalid("docs/.env", "must not contain a dotfile segment"),
        ),
        (
            "/shell.html",
            fallback_invalid("/shell.html", "must be a relative path"),
        ),
        (
            "../shell.html",
            fallback_invalid("../shell.html", "must not contain .."),
        ),
        ("a\\\\b", fallback_invalid(r"a\b", r"must not contain \")),
        (
            "a\u{0}b",
            fallback_invalid("a\u{0}b", "must not contain a control byte"),
        ),
        (
            "a\u{1f}b",
            fallback_invalid("a\u{1f}b", "must not contain a control byte"),
        ),
    ] {
        assert_eq!(
            refusals(&format!("root \".\"\nfallback \"{fallback}\"\n")),
            vec![expected],
            "{fallback}"
        );
    }
    assert_eq!(
        lines("root \".\"\nfallback \".env\"\n")[0],
        r#"mount "/" (index 0): static fallback ".env": must not contain a dotfile segment"#
    );

    // A dot-directory `root` stays legal — the author chose that directory, and
    // the resolver still checks the request path (§9.1) — while the same
    // spelling as an `index` is refused.
    let dotted = accepted("root \".output\"\nfallback \"index.html\"\n");
    assert_eq!(policy(&dotted).root, ".output");
    assert_eq!(
        refusals("root \".output\"\nindex \".env\"\n"),
        vec![index_invalid(".env", "must not contain a dotfile segment")]
    );

    // A fallback that *is* the index is the page's own shape.
    let page = accepted("root \"public\"\nindex \"index.html\"\nfallback \"index.html\"\n");
    assert_eq!(
        policy(&page).fallback.as_deref(),
        Some(policy(&page).index.as_str())
    );
}

#[test]
fn a_mount_that_never_reaches_the_app_may_not_declare_them() {
    // The retired suite refused an ingress, a `prefork`, an inline group and a
    // grant on a static route. A mount has no route, so the same four
    // declarations are refused one step earlier, by the parser's own list.
    for (body, word) in [
        ("body json\n", "body"),
        ("prefork\n", "prefork"),
        ("use session\n", "use"),
        ("postgres {\n", "postgres"),
    ] {
        assert_eq!(
            lines(body),
            vec![format!(
                "2:1: `{word}` is not a directive here; a `static` block takes \
                 `root`, `index`, `fallback` and `immutable`"
            )],
            "{body}"
        );
    }

    // The other side of the same rule: on the view that does reach the app,
    // every one of them is live and the whole file validates.
    let source = "postgres $DATABASE_URL\n\n\
                  trait session {\n    postgres {\n        select products\n    }\n}\n\n\
                  static \"/\" {\n    root \"public\"\n}\n\n\
                  view site \"/*\" {\n    use session\n    body none\n    prefork\n    \
                  postgres {\n        select products\n    }\n}\n";
    let (manifest, warnings) = Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE)
        .expect("the deployed file boots");
    assert_eq!(warnings, vec![]);
    assert!(policy(&manifest).fallback.is_none());
    let route = manifest.route(0).expect("route 0");
    assert!(route.ingress().is_some());
    assert!(route.prefork());
    assert_eq!(route.groups().len(), 1);
    assert_eq!(route.grants().len(), 1);
    assert!(!manifest.is_all_static());
}

#[test]
fn only_a_manifest_whose_every_route_is_static_needs_no_app() {
    let page = Manifest::parse_blastfile(PAGE, UidLayout::DEFAULT_BASE)
        .expect("the page boots")
        .0;
    assert!(page.is_all_static());

    // A callable makes it not all-static even when every mount serves files:
    // the app still answers what the mounts do not. `app "shop"` is only the
    // application's *name* (§4.3); the callable is `entrypoint python`.
    let with_app = Manifest::parse_blastfile(
        &format!("entrypoint python \"shop.wsgi:application\"\n\n{PAGE}"),
        UidLayout::DEFAULT_BASE,
    )
    .expect("the page plus a callable boots")
    .0;
    assert_eq!(with_app.app(), Some("shop.wsgi:application"));
    assert!(!with_app.is_all_static());

    // A mount next to a forwarding view, no `app`: the manifest is not
    // all-static, so the boot still needs `BLASTIT_APP` (that check is the
    // CLI's, `MB.2.6`), and the mount cannot reach an app either.
    let mixed = "static \"/assets\" {\n    root \"public/assets\"\n}\n\n\
                 view app \"/*\" {\n    body none\n}\n";
    let (manifest, _) = Manifest::parse_blastfile(mixed, UidLayout::DEFAULT_BASE)
        .expect("the mixed manifest validates");
    assert_eq!(manifest.app(), None);
    assert_eq!(manifest.mounts().len(), 1);
    assert_eq!(manifest.routes().len(), 1);
    assert!(!manifest.is_all_static(), "one view is not static");

    // Neither an empty file nor a name-only one is a static site.
    assert!(
        !Manifest::from_blastfile_str("")
            .expect("an empty file parses")
            .is_all_static()
    );
    assert!(
        !Manifest::from_blastfile_str("app \"shop\"\n")
            .expect("a name-only file parses")
            .is_all_static()
    );
}

#[test]
fn a_store_named_static_can_never_be_granted_by_a_mount() {
    // `postgres static $STATIC_URL` stays legal to declare, but the `static`
    // keyword is a mount, so the store can never be granted — and the boot says
    // so once, as an ungranted store.
    let source = "postgres static $STATIC_URL\n\n\
                  static \"/\" {\n    root \"public\"\n}\n\n\
                  view site \"/*\" {\n    body none\n    prefork\n}\n";
    let (manifest, warnings) =
        Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE).expect("the manifest boots");
    assert!(manifest.proxy("static").is_some(), "the store is declared");
    assert_eq!(manifest.mounts().len(), 1, "the keyword is a mount");
    assert_eq!(policy(&manifest).root, "public");
    assert!(manifest.route(0).expect("route 0").grants().is_empty());
    assert_eq!(
        warnings,
        vec![ManifestWarning::UngrantedProxy {
            proxy: ProxyName::from("static"),
        }]
    );
}

#[test]
fn validation_never_touches_the_filesystem() {
    // The split the design requires: the page's mount validates from a
    // directory with no `public/`, and the explicit probe then refuses it.
    let dir = Scratch::new("split");
    let (manifest, _) =
        Manifest::parse_blastfile(PAGE, UidLayout::DEFAULT_BASE).expect("the page validates");
    let errors = manifest
        .check_static_roots(&dir.path)
        .expect_err("no public/ here");
    assert_eq!(
        errors.errors(),
        [ManifestError::StaticRootUnusable {
            location: mount_location(),
            root: "public".to_string(),
            reason: "no such file or directory",
        }]
    );
    assert_eq!(
        errors.errors()[0].to_string(),
        r#"mount "/" (index 0): static root "public": no such file or directory"#
    );
}

#[test]
fn a_listable_root_is_accepted_while_its_index_need_not_exist_yet() {
    let dir = Scratch::new("accept");
    std::fs::create_dir(dir.join("public")).expect("mkdir");

    // The design audits the root only: a missing `index.html` is a `404` at
    // request time, never a boot error.
    let manifest = accepted("root \"public\"\nindex \"shell.html\"\n");
    assert_eq!(manifest.check_static_roots(&dir.path), Ok(()));
    assert!(!dir.join("public/shell.html").exists());

    // A symlinked root is what a `current` deployment looks like, and symlinks
    // are followed.
    std::os::unix::fs::symlink(dir.join("public"), dir.join("current")).expect("symlink");
    let manifest = accepted("root \"current\"\n");
    assert_eq!(manifest.check_static_roots(&dir.path), Ok(()));
    assert_eq!(
        manifest.static_roots(&dir.path),
        vec![(0, dir.join("current"))]
    );
}

#[test]
fn every_unusable_root_is_named_and_none_stops_the_list() {
    let dir = Scratch::new("many");
    std::fs::create_dir(dir.join("ok")).expect("mkdir");
    std::fs::write(dir.join("afile"), b"not a directory").expect("write");
    // Three mounts: a root belongs to a mount now (§9.6), so three declarations
    // are three roots to audit, in file order.
    let source = "static \"/a\" {\n    root \"ok\"\n}\n\n\
                  static \"/b\" {\n    root \"afile\"\n}\n\n\
                  static \"/c\" {\n    root \"gone\"\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("the file parses");
    assert_eq!(
        manifest.static_roots(&dir.path),
        vec![
            (0, dir.join("ok")),
            (1, dir.join("afile")),
            (2, dir.join("gone")),
        ]
    );

    let unusable = |index: usize, prefix: &str, root: &str, reason: &'static str| {
        ManifestError::StaticRootUnusable {
            location: Location::Mount {
                index,
                prefix: prefix.to_string(),
            },
            root: root.to_string(),
            reason,
        }
    };
    assert_eq!(
        manifest
            .check_static_roots(&dir.path)
            .expect_err("a file and a missing directory")
            .errors(),
        [
            unusable(1, "/b", "afile", "not a directory"),
            unusable(2, "/c", "gone", "no such file or directory"),
        ],
        "every unusable root, in manifest order, and only those"
    );
}

#[test]
fn an_unlistable_root_is_permission_denied() {
    // A `000` directory only hides itself from a non-root runner, so a root
    // runner is a hard failure rather than a skip: the row would prove nothing.
    assert_ne!(
        effective_uid(),
        0,
        "the unlistable-root row needs an unprivileged runner"
    );

    let dir = Scratch::new("locked");
    let root = dir.join("locked");
    std::fs::create_dir(&root).expect("mkdir");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    let manifest = accepted("root \"locked\"\n");
    let errors = manifest
        .check_static_roots(&dir.path)
        .expect_err("an unlistable root cannot be served");
    assert_eq!(
        errors.errors(),
        [ManifestError::StaticRootUnusable {
            location: mount_location(),
            root: "locked".to_string(),
            reason: "permission denied",
        }]
    );

    // Let the scratch directory be removed.
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).expect("chmod back");
}

/// A unique directory under the system temporary directory, removed on drop.
///
/// No `tempfile` dependency: the manifest crate has none and `MB.2.8` owns the
/// workspace manifest.
struct Scratch {
    path: PathBuf,
}

/// Distinguishes two scratches a parallel test creates in the same nanosecond.
static SCRATCHES: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    fn new(label: &str) -> Self {
        let unique = SCRATCHES.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "blastit-static-manifest-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The effective uid, through `/proc`, so the crate needs no `libc` dependency.
fn effective_uid() -> u32 {
    use std::os::unix::fs::MetadataExt as _;

    std::fs::metadata("/proc/self")
        .unwrap_or_else(|error| panic!("/proc/self: {error}"))
        .uid()
}
