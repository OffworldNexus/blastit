//! Entrypoints, `prefork` and `egress any` (§12), through the public entry
//! point (`R13`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Manifest, ManifestError};

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// `entrypoint python` fills `app` and `preload`, in written order with
/// duplicates dropped (§12.1).
#[test]
fn python_fills_the_callable_and_the_preload_list() {
    let source = "entrypoint python \"shop.wsgi:application\" {\n    \
                  preload \"shop.urls\", \"shop.signals\", \"shop.urls\"\n}\n\
                  view v \"/v\" {\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(manifest.app(), Some("shop.wsgi:application"));
    assert_eq!(manifest.preload(), ["shop.urls", "shop.signals"]);
    assert!(manifest.php().is_none());
}

/// A `python` entrypoint with no block preloads nothing.
#[test]
fn python_without_a_block_preloads_nothing() {
    let manifest = Manifest::from_blastfile_str("entrypoint python \"shop.wsgi:application\"\n")
        .expect("lowers");
    assert_eq!(manifest.app(), Some("shop.wsgi:application"));
    assert!(manifest.preload().is_empty());
}

/// `app "<name>"` is carried as `Manifest::name` and bounded by §4.5's cap
/// (`MAX_APP_NAME_CHARS`, 256 `char`s), refused at the name's own `line:col`.
#[test]
fn the_app_name_is_carried_and_bounded() {
    let manifest =
        Manifest::from_blastfile_str("app \"shop\"\nview v \"/v\" {\n}\n").expect("lowers");
    assert_eq!(manifest.name(), Some("shop"));

    // The cap counts `char`s: 256 two-byte characters are 512 bytes and legal.
    let at_cap = format!("app \"{}\"\nview v \"/v\" {{\n}}\n", "é".repeat(256));
    assert_eq!(
        Manifest::from_blastfile_str(&at_cap)
            .expect("at the cap lowers")
            .name()
            .map(str::len),
        Some(512)
    );

    // One past the cap, an empty name and a control byte are one sentence.
    let sentence = "1:5: an `app` name is 1 to 256 characters with no control byte";
    let over = format!("app \"{}\"\nview v \"/v\" {{\n}}\n", "a".repeat(257));
    assert_eq!(refusal(&over), sentence);
    assert_eq!(refusal("app \"\"\n"), sentence);
    assert_eq!(refusal("app \"a\tb\"\n"), sentence);
}

/// A second `preload` statement appends to the list (the parser accepts it, and
/// lowering concatenates in written order).
#[test]
fn a_second_preload_statement_appends() {
    let source = "entrypoint python \"shop.wsgi:application\" {\n    preload \"a\"\n    preload \"b\", \"a\"\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(manifest.preload(), ["a", "b"]);
}

/// `entrypoint php` fills the one `PhpPolicy`, with `workers` always `None`
/// (§12.2, §12.3).
#[test]
fn php_fills_the_one_front_controller() {
    let source = "entrypoint php \"public/index.php\"\nview app \"/*\" {\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    let php = manifest.php().expect("the front controller");
    assert_eq!(php.entry, "public/index.php");
    assert_eq!(php.workers, None, "§12.3: the cap is gone");
    assert!(manifest.app().is_none(), "a php file declares no callable");
    // No route carries its own block: the entry is the file's, not a view's.
    for route in manifest.routes() {
        assert!(route.php().is_none());
    }
}

/// `check_php_entry`'s rules, each at the `entrypoint`'s own position, and the
/// path is never echoed (§12.2, §18).
#[test]
fn the_php_entry_rules_are_kept() {
    let cases: &[(&str, &str)] = &[
        ("/index.php", "must be a relative path"),
        ("", "must be a relative path"),
        (r"public\\index.php", "must not contain \\"),
        ("../index.php", "must not contain .."),
        ("public/index.phtml", "must end in .php"),
    ];
    for (entry, rule) in cases {
        let source = format!("entrypoint php \"{entry}\"\n");
        let rendered = refusal(&source);
        assert_eq!(
            rendered,
            format!("1:1: entrypoint php: {rule}"),
            "{entry:?}"
        );
        assert!(
            !rendered.contains(entry) || entry.is_empty(),
            "the entry is never echoed: {rendered}"
        );
    }
    // A control byte is refused too, and it is escaped rather than printed raw.
    let rendered = refusal("entrypoint php \"public/\u{1}.php\"\n");
    assert_eq!(
        rendered,
        "1:1: entrypoint php: must not contain a control byte"
    );
    assert!(!rendered.contains('\u{1}'), "{rendered}");
}

/// A second `entrypoint` is refused, naming the first line (§6.7).
#[test]
fn a_second_entrypoint_is_refused() {
    assert_eq!(
        refusal("entrypoint php \"a.php\"\nentrypoint php \"b.php\"\n"),
        "2:1: a file declares one `entrypoint`; the first is on line 1"
    );
}

/// `workers` was removed, and the refusal says what to do instead (§6.7,
/// §12.3).
#[test]
fn workers_is_refused() {
    assert_eq!(
        refusal("view v \"/v\" {\n  workers 4\n}\n"),
        "2:3: `workers` was removed in 0.2.0: the scheduler owns parallelism, so delete the line and add nothing"
    );
    assert_eq!(
        refusal("entrypoint workers 4\n"),
        "1:12: `workers` was removed in 0.2.0: the scheduler owns parallelism, so delete the line and add nothing"
    );
}

/// The `module:attribute` colon rule is `validate`'s, unchanged (§12.1).
#[test]
fn a_callable_needs_a_colon() {
    let rendered = refusal("entrypoint python \"shop.wsgi.application\"\n");
    assert!(
        rendered.contains("must be `module:attribute`"),
        "{rendered}"
    );
    // The refusal is `InvalidApp`, not a lowering error: `from_blastfile_str`
    // accepts the document and `validate` refuses it.
    assert!(Manifest::from_blastfile_str("entrypoint python \"shop.wsgi.application\"\n").is_ok());
}

/// `prefork` and `egress any` set the route's two flags (§12.4).
#[test]
fn prefork_and_egress_set_the_route_flags() {
    let manifest = Manifest::from_blastfile_str("view v \"/v\" {\n  prefork\n  egress any\n}\n")
        .expect("lowers");
    let route = manifest.route(0).expect("one route");
    assert!(route.prefork());
    assert!(route.egress());
    // Neither is a per-view `static`/`php` block.
    assert!(route.static_policy().is_none());
    assert!(route.php().is_none());
}

/// `egress true` and `egress = true` are not spellings of `egress any`
/// (§12.4).
#[test]
fn egress_has_one_spelling() {
    assert_eq!(
        refusal("view v \"/v\" {\n  egress true\n}\n"),
        "2:10: expected `any`, found `true`"
    );
    assert_eq!(
        refusal("view v \"/v\" {\n  egress = any\n}\n"),
        "2:10: `=` is not Blastfile syntax; write the value after the keyword: `body json`"
    );
}

/// The refusal type of the php row is the new positioned variant, so a caller
/// can tell it from the retired per-route one.
#[test]
fn the_php_row_is_a_positioned_refusal() {
    let errors = Manifest::from_blastfile_str("entrypoint php \"index.phtml\"\n")
        .expect_err("the document is refused");
    assert!(matches!(
        errors.errors().first(),
        Some(ManifestError::PhpEntrypointInvalid { .. })
    ));
}
