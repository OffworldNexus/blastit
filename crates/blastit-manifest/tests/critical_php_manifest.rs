//! The `entrypoint php "<path>"` grammar, its fatal errors and the two boot
//! predicates (`ADR-0010` §1/§4, `ADR-0012` §12.3; design §12.2).
//!
//! Protected (critical tier). A front controller names the one file an embedded
//! `libphp` worker executes, so a regression here would run a file the operator
//! never declared, or size a view's worker set without a bound. Every rule has
//! its denied case **and** its accepted twin, and each family asserts the exact
//! boot message the design pins, not merely "an error".
//!
//! The subject moved and the promises did not: one `entrypoint php` is the whole
//! file's front controller (§4.3), so every case asserts `manifest.php()` and no
//! case reads a view's own block. Two retired rules lost their subject with the
//! per-view declaration and are **dropped**, with the reason: "the entry must be
//! beneath the view's static root" and "a deep-link answer never reaches the
//! front controller" (§12.2) — the `.php` `Hidden` rule is per request now, and
//! `fallback` is evaluated after the views.
//!
//! One promise is structural rather than per-key: a front controller is its
//! **own** executor, so a php-only file needs no `app` (`can_boot_without_app`)
//! while `is_all_static` keeps its 0.2.0 meaning.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{
    Executor, Manifest, ManifestError, ManifestWarning, PhpPolicy, Position, UidLayout,
};

/// A file with the given front controller, plus the catch-all view that
/// forwards to it (§7.2 row 3), so nothing warns and the entry is reached.
fn php_file(entry: &str) -> String {
    format!("entrypoint php {entry}\n\nview app \"/*\" {{\n    body none\n}}\n")
}

/// Parses a file that must not parse, and returns its problems.
fn parse_errors(source: &str) -> Vec<ManifestError> {
    Manifest::from_blastfile_str(source)
        .expect_err("should not parse")
        .into_errors()
}

/// The one-line refusals of `entrypoint php <entry>`.
fn violations(entry: &str) -> Vec<ManifestError> {
    parse_errors(&format!("entrypoint php {entry}\n"))
}

/// Parses and validates a file that must boot, and checks the file really
/// carries the one front controller: one entry and one catch-all view has no
/// group, store or codec to warn about.
fn accepted(entry: &str) -> Manifest {
    let source = php_file(entry);
    let (manifest, warnings) = Manifest::parse_blastfile(&source, UidLayout::DEFAULT_BASE)
        .unwrap_or_else(|errors| panic!("should boot:\n{errors}\n{source}"));
    assert_eq!(warnings, vec![], "the file warns about nothing");
    assert_eq!(
        manifest.routes().len(),
        1,
        "the catch-all view is one route"
    );
    assert!(
        manifest.php().is_some(),
        "the file carries its one front controller"
    );
    manifest
}

/// The `entry` rule error, as lowering builds it: it names the `entrypoint` and
/// the rule, never the path (§18).
fn entry_invalid(rule: &'static str) -> ManifestError {
    ManifestError::PhpEntrypointInvalid {
        at: Position::at(1, 1),
        rule,
    }
}

/// The one-line `workers` refusal (§6.7, §12.3): the removal, and the fix.
const WORKERS_REMOVED: &str = "`workers` was removed in 0.2.0: the scheduler owns \
                               parallelism, so delete the line and add nothing";

/// A one-line `entrypoint php` is the documented front controller: it parses,
/// validates, and is the `php` executor — a typed plan carrying its own policy,
/// never a backend string.
#[test]
fn a_one_line_entrypoint_parses_validates_and_is_the_php_executor() {
    let manifest = accepted("\"public/index.php\"");
    assert_eq!(
        manifest.php(),
        Some(&PhpPolicy {
            entry: "public/index.php".to_string(),
            // §12.3: no file sizes a worker count; the scheduler owns parallelism.
            workers: None,
        })
    );

    // The kind table is the one a backend registry learns from, so the two
    // spellings it dispatches on are `wsgi` and `php`, and the retired external
    // pool's spelling must never come back. The model keeps `workers` as an
    // `Option` nothing in a file can fill (§4.3), so the field still exists.
    let policy = Executor::Php(PhpPolicy {
        entry: "public/index.php".to_string(),
        workers: Some(4),
    });
    assert_eq!(policy.kind(), "php");
    assert_eq!(Executor::View.kind(), "wsgi");
    assert_eq!(Executor::ALL_KINDS, ["wsgi", "php"].as_slice());
    assert!(!Executor::ALL_KINDS.contains(&"fastcgi"));

    // An executor plan is a value: it clones, it compares, and it is not the
    // WSGI arm.
    assert_eq!(policy.clone(), policy);
    assert_ne!(policy, Executor::View);
}

/// `entry` is a relative `.php` path that stays in the manifest's directory;
/// every refused spelling names its rule, and the accepted twins boot.
#[test]
fn php_entry_must_be_a_relative_php_path() {
    for (entry, rule) in [
        ("\"\"", "must be a relative path"),
        ("\"/public/index.php\"", "must be a relative path"),
        ("\"public\\\\index.php\"", "must not contain \\"),
        ("\"../index.php\"", "must not contain .."),
        ("\"public/../index.php\"", "must not contain .."),
        // `blastfile/lower.rs` `check_php_entry`'s inline copy is
        // `byte < 0x20`; the lexer copies a raw control byte verbatim, so a NUL
        // and a `0x1f` pin both ends of that range (and fail any predicate that
        // only knew `\n`/`\t`).
        ("\"in\u{0}dex.php\"", "must not contain a control byte"),
        ("\"in\u{1f}dex.php\"", "must not contain a control byte"),
        ("\"public/index.txt\"", "must end in .php"),
    ] {
        assert_eq!(
            violations(entry),
            vec![entry_invalid(rule)],
            "entry {entry}"
        );
    }

    for entry in ["\"index.php\"", "\"public/index.PHP\""] {
        accepted(entry);
    }

    // A missing path is the parser's own refusal, at the newline that stands
    // where the string belongs.
    assert_eq!(
        parse_errors("entrypoint php\n")
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec![r#"1:15: expected a `"…"` path, found a newline"#]
    );
}

/// The entry is declared once, for the whole file, so the rule that it had to
/// sit beneath "its" view's static root has nothing left to refuse (§12.2): the
/// `.php`/`.phtml`/`.phar` `Hidden` rule is per request and unconditional, so no
/// mount can disclose the front controller whatever the layout.
#[test]
fn a_front_controller_is_not_checked_against_a_mount_root() {
    // Outside the served tree: legal, because the mount cannot serve a `.php`
    // file at all — `critical_static_entry.rs`'s
    // `php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates`
    // owns that refusal.
    let outside = "entrypoint php \"app/index.php\"\n\n\
                   static \"/\" {\n    root \"public\"\n}\n\n\
                   view app \"/*\" {\n    body none\n}\n";
    let (manifest, warnings) = Manifest::parse_blastfile(outside, UidLayout::DEFAULT_BASE)
        .expect("the layout is not a rule");
    assert_eq!(warnings, vec![]);
    assert_eq!(
        manifest.php().expect("the front controller").entry,
        "app/index.php"
    );
    assert_eq!(manifest.mounts()[0].policy.root, "public");

    // The documented shape — the front controller inside the served tree — is
    // the accepted twin, and both lower to the same one `manifest.php()`.
    let inside = "entrypoint php \"public/index.php\"\n\n\
                  static \"/\" {\n    root \"public\"\n}\n\n\
                  view app \"/*\" {\n    body none\n}\n";
    let (manifest, warnings) = Manifest::parse_blastfile(inside, UidLayout::DEFAULT_BASE)
        .expect("the documented file boots");
    assert_eq!(warnings, vec![]);
    assert_eq!(
        manifest.php().expect("the front controller").entry,
        "public/index.php"
    );
    assert_eq!(manifest.mounts()[0].policy.root, "public");

    // With no mount the entry is simply relative to the manifest's directory.
    accepted("\"index.php\"");
}

/// A file declares one `entrypoint`, whichever language it names (§6.7): the
/// second one would silently replace the first.
#[test]
fn a_file_declares_one_entrypoint() {
    let twice = "entrypoint php \"index.php\"\n\nentrypoint python \"shop.wsgi:application\"\n";
    assert_eq!(
        parse_errors(twice),
        vec![ManifestError::DuplicateEntrypoint {
            at: Position::at(3, 1),
            first: 1,
        }]
    );
    assert_eq!(
        parse_errors(twice)[0].to_string(),
        "3:1: a file declares one `entrypoint`; the first is on line 1"
    );

    // The other order is the same rule: the front controller is not a second,
    // more specific declaration.
    let reverse = "entrypoint python \"shop.wsgi:application\"\n\nentrypoint php \"index.php\"\n";
    assert_eq!(
        parse_errors(reverse),
        vec![ManifestError::DuplicateEntrypoint {
            at: Position::at(3, 1),
            first: 1,
        }]
    );

    // The accepted twin: one entrypoint, and the catch-all view that reaches it.
    accepted("\"index.php\"");
}

/// `workers` was removed (§12.3, `ADR-0012`): the scheduler owns parallelism, so
/// the word is refused wherever a generated or hand-edited file might put it —
/// and **no** spelling sizes a worker count from a file.
#[test]
fn workers_is_gone_and_no_file_sizes_a_worker_count() {
    for (source, at) in [
        // At the top level, and in the entrypoint itself.
        (
            "entrypoint php \"index.php\"\nworkers 4\n",
            Position::at(2, 1),
        ),
        ("entrypoint workers 4\n", Position::at(1, 12)),
        // In a view body, in one endpoint's body, and in a mount.
        ("view app \"/*\" {\nworkers 4\n}\n", Position::at(2, 1)),
        (
            "view app { GET \"/*\" {\nworkers 4\n} }\n",
            Position::at(2, 1),
        ),
        (
            "static \"/\" {\n    root \"public\"\n    workers 4\n}\n",
            Position::at(3, 5),
        ),
        // The word is refused before its value is looked at, so no spelling of
        // a count can reach the policy — a string or an out-of-range integer
        // included.
        (
            "entrypoint php \"index.php\"\nworkers \"4\"\n",
            Position::at(2, 1),
        ),
        (
            "entrypoint php \"index.php\"\nworkers 4294967296\n",
            Position::at(2, 1),
        ),
    ] {
        assert_eq!(
            parse_errors(source),
            vec![ManifestError::WorkersRemoved { at }],
            "{source}"
        );
    }
    assert_eq!(
        parse_errors("entrypoint php \"index.php\"\nworkers 4\n")[0].to_string(),
        format!("2:1: {WORKERS_REMOVED}")
    );

    // The positive half: every accepted file answers `workers: None`, and the
    // whole policy is the one `manifest.php()` hands the scheduler. There is no
    // spelling that produces a count, so nothing can be sized from a file.
    let static_and_php = "entrypoint php \"public/index.php\"\n\n\
         static \"/\" {\n    root \"public\"\n    fallback \"index.html\"\n}\n\n\
         view app \"/*\" {\n    body none\n}\n";
    for (source, entry) in [
        ("entrypoint php \"index.php\"\n".to_string(), "index.php"),
        (php_file("\"public/index.php\""), "public/index.php"),
        (static_and_php.to_string(), "public/index.php"),
    ] {
        let manifest = Manifest::from_blastfile_str(&source).expect("the file parses");
        assert_eq!(
            manifest.php(),
            Some(&PhpPolicy {
                entry: entry.to_string(),
                workers: None,
            }),
            "{source}"
        );
    }
}

/// With no `[route.static]` block the entry is simply relative to the manifest's
/// directory — and a `fallback` beside the front controller is legal now, in
/// both directions: the retired "a deep-link answer never reaches the front
/// controller" has nothing left to refuse, because `fallback` is evaluated after
/// every view (§9.2). The precedence walk itself is
/// `critical_blastfile_parity.rs`'s.
#[test]
fn a_fallback_beside_a_front_controller_is_legal_now() {
    let with_fallback = "entrypoint php \"public/index.php\"\n\n\
                         static \"/\" {\n    root \"public\"\n    fallback \"index.html\"\n}\n\n\
                         view app \"/*\" {\n    body none\n}\n";
    let (manifest, warnings) = Manifest::parse_blastfile(with_fallback, UidLayout::DEFAULT_BASE)
        .expect("the front controller and the fallback ship together");
    assert_eq!(warnings, vec![]);
    assert_eq!(
        manifest.mounts()[0].policy.fallback.as_deref(),
        Some("index.html")
    );
    assert_eq!(
        manifest.php().expect("the front controller").entry,
        "public/index.php"
    );
    assert_eq!(manifest.routes().len(), 1, "the view is still reachable");

    // The accepted twin: no `fallback`, one entry, one view.
    let without = accepted("\"public/index.php\"");
    assert_eq!(without.php().expect("php").entry, "public/index.php");
    assert_eq!(without.routes().len(), 1);
}

/// `listen` is not a key of the form: blastit owns the channel, so a file that
/// tries to choose one is refused by name. The bare `entrypoint php` is the
/// whole declaration, with no block to hang a channel on.
#[test]
fn listen_is_not_a_key_of_the_php_form() {
    let source = "entrypoint php \"index.php\"\nlisten \"run/fpm.sock\"\n";
    assert_eq!(
        parse_errors(source),
        vec![ManifestError::UnknownKeyword {
            at: Position::at(2, 1),
            word: "listen".to_string(),
        }]
    );
    assert_eq!(
        parse_errors(source)[0].to_string(),
        "2:1: `listen` is not a keyword; the sections are `app`, `entrypoint`, \
         `postgres`, `trait`, `view`, `static`"
    );

    // A block after the path is not part of the form either.
    assert_eq!(
        parse_errors("entrypoint php \"index.php\" { listen \"run/fpm.sock\" }\n")
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["1:28: expected a newline, `;` or `}`, found `{`"]
    );

    accepted("\"index.php\"");
}

/// `CONNECT` and `TRACE` never reach a view — so a file that declares one is
/// refused once, for every view, before any worker is planned (§8.4). An
/// ordinary method set is the accepted twin, and `GET` still implies `HEAD`.
#[test]
fn connect_and_trace_cannot_reach_a_view() {
    for method in ["CONNECT", "TRACE"] {
        let source = format!(
            "entrypoint php \"index.php\"\n\nview app {method} \"/*\" {{\n    body none\n}}\n"
        );
        assert_eq!(
            parse_errors(&source),
            vec![ManifestError::MethodCannotReachView {
                at: Position::at(3, 10),
                method: method.to_string(),
            }],
            "method {method}"
        );
        assert_eq!(
            parse_errors(&source)[0].to_string(),
            format!(
                "3:10: `{method}` cannot reach a view; Blastit answers `GET`/`HEAD` \
                 static files and forwards the methods you declare"
            )
        );
    }

    for header in ["view app \"/*\"", "view app GET, POST \"/*\""] {
        let source = format!("entrypoint php \"index.php\"\n\n{header} {{\n    body none\n}}\n");
        let (manifest, _) = Manifest::parse_blastfile(&source, UidLayout::DEFAULT_BASE)
            .unwrap_or_else(|errors| panic!("should boot:\n{errors}\n{source}"));
        assert_eq!(manifest.routes().len(), 1);
        assert_eq!(manifest.php().expect("php").entry, "index.php");
    }

    // `GET` implies `HEAD`, and the two refused methods never arrive.
    let declared = Manifest::from_blastfile_str(
        "entrypoint php \"index.php\"\n\nview app GET, POST \"/*\" {\n    body none\n}\n",
    )
    .expect("the ordinary method set parses");
    assert_eq!(
        declared.route(0).expect("route 0").methods(),
        Some(["GET".to_string(), "HEAD".to_string(), "POST".to_string()].as_slice())
    );
}

/// The messages render exactly as the design's table spells them.
///
/// §18: the entry is **not** quoted — a pasted DSN or token can sit in that slot
/// — so the sentence names the `entrypoint` and the rule.
#[test]
fn the_php_messages_match_the_design_table() {
    assert_eq!(
        entry_invalid("must be a relative path").to_string(),
        "1:1: entrypoint php: must be a relative path"
    );
    assert_eq!(
        ManifestError::WorkersRemoved {
            at: Position::at(2, 1),
        }
        .to_string(),
        format!("2:1: {WORKERS_REMOVED}")
    );
    assert_eq!(
        ManifestError::DuplicateEntrypoint {
            at: Position::at(3, 1),
            first: 1,
        }
        .to_string(),
        "3:1: a file declares one `entrypoint`; the first is on line 1"
    );
}

/// §18: neither echoing variant carries the value it refused, because a pasted
/// token or DSN lands in the slot.
#[test]
fn a_refusal_never_carries_the_php_entry_it_refused() {
    const DSN: &str = "postgres://user:hunter2@db/public/index.php?pool=1";
    let errors = parse_errors(&format!("entrypoint php \"{DSN}\"\n"));
    assert_eq!(errors, vec![entry_invalid("must end in .php")]);
    let sentence = errors[0].to_string();
    assert_eq!(sentence, "1:1: entrypoint php: must end in .php");
    assert!(!sentence.contains("hunter2"), "{sentence}");
    assert!(!sentence.contains("postgres://"), "{sentence}");

    // The same rule holds for the token-shaped slot the application's name
    // owns: the refusal names the shape, never the value.
    let name = Manifest::from_blastfile_str("app \"\"\n")
        .expect_err("an empty application name is refused");
    assert_eq!(
        name.into_errors()[0].to_string(),
        "1:5: an `app` name is 1 to 256 characters with no control byte"
    );
}

/// A front controller is its own executor: a php-only file needs no `app`, while
/// a file with neither `app` nor a mount still refuses.
///
/// This is the manifest-level half of the boot rule. Because
/// `can_boot_without_app()` answers `true` for a php-only tree, a boot never
/// reaches the "install the Python package: uv add blastit" gate (`MB.3.11`)
/// for it: a PHP project has nothing to install from PyPI to get its route.
#[test]
fn a_php_entrypoint_is_its_own_executor_for_the_boot_predicates() {
    let php_only = Manifest::from_blastfile_str("entrypoint php \"index.php\"\n")
        .expect("the php-only file parses");
    assert_eq!(php_only.app(), None, "no app is declared");
    assert!(
        php_only.can_boot_without_app(),
        "a front controller is an executor"
    );
    assert!(
        !php_only.is_all_static(),
        "a front controller is not a static site"
    );

    // A front controller no view reaches still boots: the boot says so once, and
    // the sentence is the reminder to declare the catch-all view (§12.2).
    let (unreached, warnings) =
        Manifest::parse_blastfile("entrypoint php \"index.php\"\n", UidLayout::DEFAULT_BASE)
            .expect("the php-only file boots");
    assert_eq!(
        warnings,
        vec![ManifestWarning::EntrypointNotReached {
            entry: "index.php".to_string(),
        }]
    );
    assert!(unreached.can_boot_without_app());

    // Static files *and* a front controller: still bootable without an app, and
    // still not "all static" — the responder answers assets, PHP answers the
    // rest.
    let mixed = "entrypoint php \"public/index.php\"\n\n\
                 static \"/\" {\n    root \"public\"\n}\n\n\
                 view app \"/*\" {\n    body none\n}\n";
    let (mixed, _) =
        Manifest::parse_blastfile(mixed, UidLayout::DEFAULT_BASE).expect("the mixed file boots");
    assert!(mixed.can_boot_without_app());
    assert!(!mixed.is_all_static());

    // One `entrypoint php` applies to **every** view (§12.2), so a file with a
    // front controller and two views still needs no app: the retired "one PHP
    // route among WSGI views" no longer exists, because there is no per-view
    // front controller to single out.
    let two_views = "entrypoint php \"index.php\"\n\n\
                     view app \"/*\" {\n    body none\n}\n\n\
                     view api regex '/api/.*' {\n    body none\n}\n";
    let two_views =
        Manifest::from_blastfile_str(two_views).expect("the two views of a PHP app parse");
    assert_eq!(two_views.routes().len(), 2);
    assert!(two_views.can_boot_without_app());
    assert!(!two_views.is_all_static());

    // The refusal case: a WSGI view is not an executor of its own, and an empty
    // file has no view at all.
    let wsgi_only = Manifest::from_blastfile_str("view web \"/*\" {\n    body none\n}\n")
        .expect("the WSGI file parses");
    assert_eq!(wsgi_only.php(), None);
    assert!(!wsgi_only.can_boot_without_app());
    assert!(!wsgi_only.is_all_static());

    let empty = Manifest::from_blastfile_str("").expect("an empty file parses");
    assert!(!empty.can_boot_without_app());
    assert!(!empty.is_all_static());

    // A static file with no view also boots without an app, and only it is all
    // static.
    let static_only =
        Manifest::from_blastfile_str("static \"/\" {\n    root \"public\"\n}\n").expect("parses");
    assert_eq!(static_only.php(), None);
    assert!(static_only.is_all_static());
    assert!(static_only.can_boot_without_app());
}
