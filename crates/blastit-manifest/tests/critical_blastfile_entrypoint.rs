//! Entrypoints and the `workers` removal (§12, `R13`).
//!
//! The `workers` refusal is the owner decision of `ADR-0012`: the key is not a
//! keyword in Blastfile, so writing it anywhere is refused with the sentence
//! that says what to do instead — and it is refused at every site the retired
//! format had a spelling for.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::Manifest;

/// The `workers` sentence, verbatim (§6.7).
const WORKERS_REMOVED: &str = "`workers` was removed in 0.2.0: the scheduler owns parallelism, so \
                               delete the line and add nothing";

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// `workers` is refused wherever the retired format spelled it (§6.7, §12.3).
#[test]
fn workers_is_refused_at_every_site() {
    let cases: &[(&str, &str)] = &[
        ("view v \"/v\" {\n  workers 4\n}\n", "2:3"),
        ("view v {\n  GET \"/v\" {\n    workers 4\n  }\n}\n", "3:5"),
        ("entrypoint workers 4\n", "1:12"),
        ("entrypoint python \"a:b\" {\n  workers 4\n}\n", "2:3"),
        ("postgres {\n  workers 4\n}\n", "2:3"),
        ("trait t {\n  workers 4\n}\n", "2:3"),
        (
            "view v \"/v\" {\n  postgres {\n    workers 4\n  }\n}\n",
            "3:5",
        ),
        ("static \"/x\" {\n  workers 4\n}\n", "2:3"),
    ];
    for (source, at) in cases {
        assert_eq!(
            refusal(source),
            format!("{at}: {WORKERS_REMOVED}"),
            "{source:?}"
        );
    }
}

/// One `entrypoint` for the whole file, refused at the second (§6.7).
#[test]
fn a_file_declares_one_entrypoint() {
    assert_eq!(
        refusal("entrypoint python \"a:b\"\nentrypoint python \"c:d\"\n"),
        "2:1: a file declares one `entrypoint`; the first is on line 1"
    );
    assert_eq!(
        refusal("entrypoint python \"a:b\"\nentrypoint php \"index.php\"\n"),
        "2:1: a file declares one `entrypoint`; the first is on line 1"
    );
}

/// `check_php_entry`'s rules, each at the `entrypoint`'s own position, and the
/// path is never echoed (§12.2, §18).
#[test]
fn the_php_path_rules() {
    let cases: &[(&str, &str)] = &[
        ("/var/www/index.php", "must be a relative path"),
        ("public/index.phtml", "must end in .php"),
        ("public/../index.php", "must not contain .."),
        (r"public\\index.php", "must not contain \\"),
        ("public/\u{1}.php", "must not contain a control byte"),
    ];
    for (entry, rule) in cases {
        let source = format!("entrypoint php \"{entry}\"\n");
        assert_eq!(refusal(&source), format!("1:1: entrypoint php: {rule}"));
    }
    // A DSN pasted into the slot is refused without being printed.
    let rendered = refusal("entrypoint php \"postgres://user:pw@host/db\"\n");
    assert!(!rendered.contains("pw"), "{rendered}");
    assert!(!rendered.contains("postgres://"), "{rendered}");
    assert_eq!(rendered, "1:1: entrypoint php: must end in .php");
}

/// A front controller with no view is accepted: a static mount and a front
/// controller can ship together before the catch-all is written (§12.2).
#[test]
fn a_front_controller_with_no_view_is_accepted() {
    let source = "entrypoint php \"public/index.php\"\n\
                  static \"/assets\" {\n  root \"public/assets\"\n}\n";
    let (manifest, warnings) = Manifest::parse_blastfile(source, 20_000).expect("validates");
    assert_eq!(
        manifest.php().map(|php| php.entry.as_str()),
        Some("public/index.php")
    );
    assert!(manifest.routes().is_empty());
    assert_eq!(manifest.mounts().len(), 1);
    // No route carries the entry: it is the file's, not a view's.
    assert!(manifest.routes().iter().all(|route| route.php().is_none()));
    // `EntrypointNotReached` is `MB.21.3.0`'s warning and not in the model yet;
    // the boot still names the entry through `Manifest::php`.
    let _ = warnings;
}

/// The `entrypoint python` block holds `preload` only (§6.7, §12.1).
#[test]
fn the_python_block_holds_preload_only() {
    assert_eq!(
        refusal("entrypoint python \"a:b\" {\n  body json\n}\n"),
        "2:3: the `entrypoint python` block holds `preload` only"
    );
}

/// `preload` keeps written order and drops a duplicate (§12.1).
#[test]
fn preload_keeps_order_and_deduplicates() {
    let source = "entrypoint python \"a:b\" {\n    preload \"b\", \"a\", \"b\"\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(manifest.preload(), ["b", "a"]);
    assert_eq!(manifest.app(), Some("a:b"));
}
