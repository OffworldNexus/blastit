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
