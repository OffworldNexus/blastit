//! The `[route.php]` grammar, its fatal errors and the two boot predicates
//! (`ADR-0010` §1/§4; design `backend-runtimes.md` `R3`/`R4`).
//!
//! Protected (critical tier). A PHP route names the one file an embedded
//! `libphp` worker executes, so a regression here would run a file the operator
//! never declared, or size a route's worker set without a bound. Every rule has
//! its denied case **and** its accepted twin, and each family asserts the exact
//! boot message the design pins, not merely "an error".
//!
//! One promise is structural rather than per-key: a PHP route is its **own
//! executor**, so a php-only manifest needs no `app` (`can_boot_without_app`)
//! while `is_all_static` keeps its 0.2.0 meaning.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Executor, Location, Manifest, ManifestError, PhpPolicy, UidLayout};

/// The one route every table-driven case declares.
fn location() -> Location {
    Location::Route {
        index: 0,
        name: "app".to_string(),
    }
}

/// A manifest with one route and the given route-level keys plus a
/// `[route.php]` body.
fn php_manifest(route_keys: &str, block: &str) -> String {
    format!(
        "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n{route_keys}\n[route.php]\n{block}"
    )
}

/// Parses a document that must parse, and returns what validation refuses.
fn errors_of(source: &str) -> Vec<ManifestError> {
    let manifest = Manifest::from_toml_str(source)
        .unwrap_or_else(|errors| panic!("should parse:\n{errors}\n{source}"));
    manifest
        .validate(UidLayout::DEFAULT_BASE)
        .expect_err("should not validate")
        .into_errors()
}

/// Validates a `[route.php]` body that must be refused.
fn violations(block: &str) -> Vec<ManifestError> {
    errors_of(&php_manifest("", block))
}

/// Parses and validates a `[route.php]` body that must be accepted, and checks
/// the route really carries the block: a lone PHP route has no group, proxy or
/// codec to warn about.
fn accepted(block: &str) -> Manifest {
    let source = php_manifest("", block);
    let (manifest, warnings) = Manifest::parse_and_validate(&source, UidLayout::DEFAULT_BASE)
        .unwrap_or_else(|errors| panic!("should boot:\n{errors}\n{source}"));
    assert_eq!(warnings, vec![], "the block warns about nothing");
    assert_eq!(manifest.route_count(), 1);
    assert!(
        manifest.route(0).expect("route 0").php().is_some(),
        "the route carries its [route.php] block"
    );
    manifest
}

/// The `entry` rule error, as validation builds it.
fn entry_invalid(entry: &str, rule: &str) -> ManifestError {
    ManifestError::PhpEntryInvalid {
        location: location(),
        entry: entry.to_string(),
        rule: rule.to_string(),
    }
}

/// The `workers` rule error, as validation builds it.
fn workers_invalid(workers: u32, why: &'static str) -> ManifestError {
    ManifestError::PhpWorkersInvalid {
        location: location(),
        workers,
        why,
    }
}

/// A one-line `[route.php]` is the documented manifest: it parses, validates,
/// and is the `php` executor — a typed plan carrying its own policy, never a
/// backend string.
#[test]
fn a_one_line_php_block_parses_validates_and_is_the_php_executor() {
    let manifest = accepted("entry = \"public/index.php\"\n");
    assert_eq!(
        manifest.route(0).expect("route 0").php(),
        Some(&PhpPolicy {
            entry: "public/index.php".to_string(),
            workers: None,
        })
    );

    // The kind table is the one a backend registry learns from, so the two
    // spellings it dispatches on are `wsgi` and `php`, and the retired external
    // pool's spelling must never come back.
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
        ("", "must be a relative path"),
        ("/public/index.php", "must be a relative path"),
        (r"public\index.php", "must not contain \\"),
        ("../index.php", "must not contain .."),
        ("public/../index.php", "must not contain .."),
        ("public/index.txt", "must end in .php"),
    ] {
        assert_eq!(
            violations(&format!("entry = {entry:?}\n")),
            vec![entry_invalid(entry, rule)],
            "entry {entry:?}"
        );
    }

    // A control byte cannot be written through Rust's `Debug` escaping, so the
    // TOML escape is spelled out.
    assert_eq!(
        violations("entry = \"in\\u0001dex.php\"\n"),
        vec![entry_invalid(
            "in\u{1}dex.php",
            "must not contain a control byte"
        )]
    );

    for entry in ["index.php", "public/index.PHP"] {
        accepted(&format!("entry = {entry:?}\n"));
    }

    // A missing `entry` is the parser's missing key, not a value rule.
    let source = "version = 1\n[[route]]\nname = \"app\"\npath = ['.*']\n[route.php]\n";
    assert_eq!(
        Manifest::from_toml_str(source)
            .expect_err("a block with no entry cannot parse")
            .into_errors(),
        vec![ManifestError::MissingKey {
            path: "route[0].php.entry".to_string(),
        }]
    );
}

/// When a `[route.static]` block is declared too, `entry` resolves *under* its
/// `root`: the front controller the page documents lives inside the served
/// tree, and a path beside it would be a file this route cannot reach.
#[test]
fn php_entry_is_under_static_root_when_both_blocks_are_set() {
    let with_static = |root: &str, entry: &str| {
        format!(
            "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n\
             [route.static]\nroot = {root:?}\n\n[route.php]\nentry = {entry:?}\n"
        )
    };

    for entry in ["index.php", "publicity/index.php"] {
        assert_eq!(
            errors_of(&with_static("public", entry)),
            vec![entry_invalid(entry, "must be under static root \"public\"")],
            "entry {entry:?}"
        );
    }

    // The documented page's shape: the front controller inside the served tree.
    let (manifest, warnings) = Manifest::parse_and_validate(
        &with_static("public", "public/index.php"),
        UidLayout::DEFAULT_BASE,
    )
    .expect("the documented static+php route boots");
    assert_eq!(warnings, vec![]);
    assert_eq!(
        manifest
            .route(0)
            .expect("route 0")
            .static_policy()
            .map(|policy| policy.root.as_str()),
        Some("public")
    );
    assert_eq!(
        manifest
            .route(0)
            .expect("route 0")
            .php()
            .map(|policy| policy.entry.as_str()),
        Some("public/index.php")
    );

    // With no `[route.static]` block the same entry is simply relative to
    // `blastit.toml`, and a root of `.` covers the whole manifest directory.
    accepted("entry = \"index.php\"\n");
    let dotted = "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n\
         [route.static]\nroot = \".\"\n\n[route.php]\nentry = \"index.php\"\n";
    assert!(Manifest::parse_and_validate(dotted, UidLayout::DEFAULT_BASE).is_ok());
}

/// `workers` is optional and **downward only**: it caps the concurrency blastit
/// derives, so it is never required, never a `listen`, and never a local pool
/// size the manifest gets to exceed. Absent is the normal spelling.
#[test]
fn php_workers_cap_is_optional_and_bounded() {
    for (workers, why) in [
        (0, "must be at least 1"),
        (65, "at most 64"),
        (u32::MAX, "at most 64"),
    ] {
        assert_eq!(
            violations(&format!("entry = \"index.php\"\nworkers = {workers}\n")),
            vec![workers_invalid(workers, why)],
            "workers {workers}"
        );
    }

    // The grammar bound is `1..=64`; whether an explicit value fits this
    // deployment's per-view budget is a boot check with a `Config`, not a
    // manifest rule (there is no budget on a machine that only parses).
    for workers in [1, 4, 64] {
        let manifest = accepted(&format!("entry = \"index.php\"\nworkers = {workers}\n"));
        assert_eq!(
            manifest
                .route(0)
                .expect("route 0")
                .php()
                .map(|policy| policy.workers),
            Some(Some(workers))
        );
    }
    let omitted = accepted("entry = \"index.php\"\n");
    assert_eq!(
        omitted
            .route(0)
            .expect("route 0")
            .php()
            .map(|policy| policy.workers),
        Some(None)
    );

    // A negative or too-large integer cannot be carried in the policy at all,
    // so it is the parser's type error rather than a value error.
    for block in [
        "entry = \"index.php\"\nworkers = -1\n",
        "entry = \"index.php\"\nworkers = 4294967296\n",
        "entry = \"index.php\"\nworkers = \"4\"\n",
    ] {
        assert!(
            Manifest::from_toml_str(&php_manifest("", block)).is_err(),
            "{block:?} cannot parse"
        );
    }
}

/// `listen` is not a key of `[route.php]`: blastit owns the channel, so a
/// manifest that tries to choose one is refused by name. `entry` alone parses.
#[test]
fn listen_is_not_a_php_key() {
    let source = php_manifest("", "entry = \"index.php\"\nlisten = \"run/fpm.sock\"\n");
    let errors = Manifest::from_toml_str(&source)
        .expect_err("listen is not a php key")
        .into_errors();
    assert_eq!(
        errors,
        vec![ManifestError::UnknownKey {
            path: "route[0].php.listen".to_string(),
        }]
    );
    assert!(
        errors[0]
            .to_string()
            .contains("route[0].php.listen: unknown key"),
        "the refusal names the key: {}",
        errors[0]
    );

    accepted("entry = \"index.php\"\n");
}

/// `spa = true` beside `[route.php]` answers every miss itself, so it is a PHP
/// dead declaration (`PhpDeadDeclaration`), never the static one; `spa = false`
/// is the accepted twin and the static rule keeps its own sentence.
#[test]
fn spa_beside_php_never_reaches_the_front_controller() {
    let source = "version = 1\n[[route]]\nname = \"app\"\npath = ['.*']\n\
                  [route.static]\nroot = \"public\"\nspa = true\n\n\
                  [route.php]\nentry = \"public/index.php\"\n";
    assert_eq!(
        errors_of(source),
        vec![ManifestError::PhpDeadDeclaration {
            location: location(),
            key: "spa = true".to_string(),
            why: "spa = true never reaches the front controller",
        }]
    );

    let twin = source.replace("spa = true", "spa = false");
    assert!(
        Manifest::parse_and_validate(&twin, UidLayout::DEFAULT_BASE).is_ok(),
        "spa = false reaches the front controller"
    );
}

/// `CONNECT` and `TRACE` never reach a PHP worker; an ordinary method set (or
/// none at all) is the accepted twin.
#[test]
fn connect_and_trace_cannot_reach_a_php_worker() {
    for (method, why) in [
        ("CONNECT", "CONNECT cannot reach a PHP worker"),
        ("TRACE", "TRACE cannot reach a PHP worker"),
    ] {
        assert_eq!(
            errors_of(&php_manifest(
                &format!("methods = [\"{method}\"]\n"),
                "entry = \"index.php\"\n"
            )),
            vec![ManifestError::PhpDeadDeclaration {
                location: location(),
                key: method.to_string(),
                why,
            }],
            "method {method}"
        );
    }

    for route_keys in ["", "methods = [\"GET\", \"POST\"]\n"] {
        let source = php_manifest(route_keys, "entry = \"index.php\"\n");
        let (manifest, _warnings) = Manifest::parse_and_validate(&source, UidLayout::DEFAULT_BASE)
            .unwrap_or_else(|errors| panic!("should boot:\n{errors}\n{source}"));
        assert_eq!(manifest.route_count(), 1);
    }
}

/// The three messages render exactly as the design's table spells them.
#[test]
fn the_php_messages_match_the_design_table() {
    assert_eq!(
        entry_invalid("index.php", "must be under static root \"public\"").to_string(),
        "route \"app\" (index 0): php entry \"index.php\": must be under static root \"public\""
    );
    assert_eq!(
        workers_invalid(0, "must be at least 1").to_string(),
        "route \"app\" (index 0): php workers 0: must be at least 1"
    );
    assert_eq!(
        ManifestError::PhpDeadDeclaration {
            location: location(),
            key: "spa = true".to_string(),
            why: "spa = true never reaches the front controller",
        }
        .to_string(),
        "route \"app\" (index 0): spa = true never reaches the front controller"
    );
}

/// A PHP route is its own executor: a php-only manifest needs no `app`, while a
/// manifest with neither `app` nor a static/PHP route still refuses.
///
/// This is the manifest-level half of the boot rule. Because
/// `can_boot_without_app()` answers `true` for a php-only tree, a boot never
/// reaches the "install the Python package: uv add blastit" gate (`MB.3.11`)
/// for it: a PHP project has nothing to install from PyPI to get its route.
#[test]
fn a_php_route_is_its_own_executor_for_the_boot_predicates() {
    let php_only = Manifest::from_toml_str(&php_manifest("", "entry = \"index.php\"\n"))
        .expect("the php-only manifest parses");
    assert_eq!(php_only.app(), None, "no app is declared");
    assert!(
        php_only.can_boot_without_app(),
        "a PHP route is an executor"
    );
    assert!(
        !php_only.is_all_static(),
        "a PHP route is not a static site"
    );

    // Static files *and* a front controller: still bootable without an app, and
    // still not "all static" — the responder answers assets, PHP answers the
    // rest.
    let mixed = Manifest::from_toml_str(
        "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n\
         [route.static]\nroot = \"public\"\n\n[route.php]\nentry = \"public/index.php\"\n",
    )
    .expect("the static+php manifest parses");
    assert!(mixed.can_boot_without_app());
    assert!(!mixed.is_all_static());

    // The refusal case: a WSGI view is not an executor of its own, and an empty
    // manifest has no route at all.
    let wsgi_only =
        Manifest::from_toml_str("version = 1\n\n[[route]]\nname = \"web\"\npath = ['.*']\n")
            .expect("the WSGI manifest parses");
    assert!(!wsgi_only.can_boot_without_app());
    assert!(!wsgi_only.is_all_static());

    let empty = Manifest::from_toml_str("version = 1\n").expect("an empty manifest parses");
    assert!(!empty.can_boot_without_app());
    assert!(!empty.is_all_static());

    // One PHP route among WSGI views still needs the app for the views.
    let php_and_wsgi = Manifest::from_toml_str(
        "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n\
         [route.php]\nentry = \"index.php\"\n\n\
         [[route]]\nname = \"web\"\npath = ['/api/.*']\n",
    )
    .expect("the mixed manifest parses");
    assert!(!php_and_wsgi.can_boot_without_app());
    assert!(!php_and_wsgi.is_all_static());
}
