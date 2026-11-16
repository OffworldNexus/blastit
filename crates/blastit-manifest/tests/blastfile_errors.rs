//! §18's error contract, through the public entry points (`R2`, `R26`).
//!
//! Every refusal is one line: control bytes escaped, values cut at 120 `char`s
//! with `…`, and no value that could be a credential ever quoted. The path
//! prefix is the caller's (§18), so this suite proves the value half and the
//! `ManifestErrors` list shape.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Manifest, ManifestError};

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::from_blastfile_str(source).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// A refusal is exactly one line, whatever the document holds.
#[test]
fn every_refusal_is_one_line() {
    for source in [
        "View products \"/x\" { }\n",
        "view v \"/x/{id:int\" {\n}\n",
        "view v {\n  body = \"json\"\n}\n",
        "postgres \"postgres://user:pw@host/db\"\n",
        "view v \"/x\" {\n  body json unvalidated\n}\n",
        "entrypoint php \"../index.php\"\n",
    ] {
        let rendered = refusal(source);
        assert!(!rendered.contains('\n'), "{source:?}: {rendered:?}");
        assert!(
            rendered.starts_with(char::is_numeric),
            "{source:?}: {rendered:?}"
        );
        assert!(rendered.contains(": "), "{source:?}: {rendered:?}");
    }
}

/// A document that opens with a `key = value` line is not detected, sniffed or
/// converted: it is an ordinary Blastfile syntax error at its first offending
/// byte, naming the sections the file takes (§15.1, `R16`). This is the one
/// reader, so the first line is where every other document shape dies too.
#[test]
fn a_document_that_is_not_a_blastfile_gets_an_ordinary_syntax_error() {
    // `version = 1` is not a header the parser recognises.
    assert_eq!(
        refusal("version = 1\n"),
        "1:1: `version` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, \
         `trait`, `view`, `static`"
    );
    // The rest of such a document is never reached: there is no key map, no
    // detection hook and no "looks like…" branch to reach it, so the refusal is
    // one positioned line and the document yields no manifest.
    let errors =
        Manifest::from_blastfile_str("version = 1\n[proxy.db]\nprotocol = \"postgresql\"\n")
            .expect_err("a document that is not a Blastfile is refused");
    assert_eq!(errors.len(), 1, "{errors}");
    assert!(
        errors.errors()[0].to_string().starts_with("1:1: "),
        "{errors}"
    );
}

/// A value's control bytes are escaped, never printed raw (§18).
#[test]
fn a_control_byte_is_escaped() {
    // A mount's `root` is one of the shape refusals that quotes its value, so
    // it is where a pasted path would reach a message.
    let rendered = refusal("static \"/x\" {\n  root \"a\u{1}b\"\n}\n");
    assert!(rendered.contains("\\u{1}"), "{rendered:?}");
    assert!(!rendered.contains('\u{1}'), "{rendered:?}");
    assert!(!rendered.contains('\n'), "{rendered:?}");
    assert!(
        rendered.contains("must not contain a control byte"),
        "{rendered:?}"
    );
}

/// A value longer than 120 `char`s is cut with `…` (§18).
#[test]
fn a_long_value_is_cut_at_120_characters() {
    let name = "a".repeat(121);
    let source = format!("view {name} \"/x\" {{\n}}\nview {name} \"/y\" {{\n}}\n");
    let rendered = refusal(&source);
    assert!(rendered.contains("…"), "{rendered}");
    let cut = format!("\"{}\u{2026}\"", "a".repeat(120));
    assert!(rendered.contains(&cut), "{rendered}");
    // 120 `char`s is not cut.
    let name = "a".repeat(120);
    let source = format!("view {name} \"/x\" {{\n}}\nview {name} \"/y\" {{\n}}\n");
    let rendered = refusal(&source);
    assert!(!rendered.contains('…'), "{rendered}");
}

/// The shortest document is a document: an empty file lowers to a manifest
/// with nothing in it, and §7.1's three dropped rows stay absent — no mount, no
/// front controller, no `app` name. A file that declares none of them cannot
/// acquire one from nothing.
#[test]
fn an_empty_document_parses_with_no_mount_no_front_controller_and_no_name() {
    let manifest = Manifest::from_blastfile_str("").expect("an empty file is a Blastfile");
    assert_eq!(manifest.route_count(), 0);
    assert!(manifest.mounts().is_empty(), "{:?}", manifest.mounts());
    assert!(manifest.php().is_none());
    assert_eq!(manifest.name(), None);
}

/// Two mounts may not share a prefix: the second could never answer, so the
/// whole-file check refuses both indices (§9.1). This is where the retired
/// "second `[route.static]` block never answers" fact lives now — the two
/// blocks are one prefix, and the duplicate is the refusal.
#[test]
fn a_duplicate_mount_prefix_is_refused() {
    let source = "static \"/x\" {\n  root \"a\"\n}\nstatic \"/x\" {\n  root \"b\"\n}\n";
    let errors =
        Manifest::parse_blastfile(source, 20_000).expect_err("the second mount cannot answer");
    let error = errors.errors().first().expect("one error");
    assert!(
        matches!(error, ManifestError::DuplicateMount { prefix, first: 0, second: 1 } if prefix == "/x"),
        "{error:?}"
    );
}

/// A refused document never yields a manifest (R21).
#[test]
fn a_refused_document_yields_no_manifest() {
    for source in [
        "view v \"/x\" {\n  body\n}\n",
        "view v \"/x/{id:int\" {\n}\n",
        "postgres \"postgres://user:pw@host/db\"\n",
    ] {
        assert!(Manifest::from_blastfile_str(source).is_err(), "{source:?}");
        assert!(
            Manifest::parse_blastfile(source, 20_000).is_err(),
            "{source:?}"
        );
    }
}

/// A lowered-but-invalid document is refused by `validate`, and `from_blastfile_str`
/// still returns the model: the two entry points differ by exactly that step.
#[test]
fn the_two_entry_points_differ_by_validation() {
    let source = "view a \"/a\" {\n  postgres x { select t }\n}\nview b \"/b\" {\n  postgres y { select t }\n}\n";
    assert!(
        Manifest::from_blastfile_str(source).is_ok(),
        "lowering does not validate"
    );
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("validation refuses it");
    assert_eq!(errors.len(), 2, "one error per undeclared store");
}

/// `ManifestErrors` lists one line per problem and nothing else (§18): no
/// header, no `  - ` prefix, no trailing newline.
#[test]
fn the_error_list_is_one_line_per_problem() {
    let source = "view a \"/a\" {\n  postgres x { select t }\n}\nview b \"/b\" {\n  postgres y { select t }\n}\n";
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("validation refuses it");
    let rendered = errors.to_string();
    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(lines.len(), errors.len(), "{rendered}");
    for error in errors.errors() {
        let line = error.to_string();
        assert!(!line.contains('\n'), "one problem is one line: {line}");
        assert!(!line.starts_with("  - "), "no per-line prefix: {line}");
        assert!(
            !line.starts_with("manifest has "),
            "no header is folded into a problem: {line}"
        );
        assert!(lines.contains(&line.as_str()), "{line}\n{rendered}");
    }
    assert!(!rendered.contains("manifest has "), "{rendered}");
    assert!(!rendered.ends_with('\n'), "{rendered:?}");
    assert_eq!(
        rendered,
        errors
            .errors()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        "the join is exactly the problems"
    );
}

/// A predicate over one refusal, so the table below stays readable.
type Check = fn(&ManifestError) -> bool;

/// The variant of each row is the row's own, so a caller never parses a message
/// back into an error (§18).
#[test]
fn a_refusal_is_the_variant_of_its_row() {
    let cases: &[(&str, Check)] = &[
        ("postgres unrestricted\n", |error| {
            matches!(error, ManifestError::StoreNameReserved { .. })
        }),
        ("view v \"/x\" {\n  workers 4\n}\n", |error| {
            matches!(error, ManifestError::WorkersRemoved { .. })
        }),
        ("view v \"/x\" {\n  use ghost\n}\n", |error| {
            matches!(error, ManifestError::UnknownTrait { .. })
        }),
        ("entrypoint php \"index.phtml\"\n", |error| {
            matches!(error, ManifestError::PhpEntrypointInvalid { .. })
        }),
        ("static \"/x\" {\n  index \"index.html\"\n}\n", |error| {
            matches!(error, ManifestError::StaticRootMissing { .. })
        }),
        ("postgres $A\npostgres db $B\n", |error| {
            matches!(error, ManifestError::DuplicateProxyName { .. })
        }),
        (
            "static \"/x\" {\n  root \"a\"\n  root \"b\"\n}\n",
            |error| matches!(error, ManifestError::DuplicateStaticDirective { .. }),
        ),
    ];
    for (source, expected) in cases {
        let errors = Manifest::from_blastfile_str(source).expect_err("the document is refused");
        let first = errors.errors().first().expect("one error");
        assert!(expected(first), "{source:?}: {first:?}");
    }
}
