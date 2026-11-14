//! The `[route.static]` grammar, its five fatal errors and the root audit
//! (`R1`–`R5`, `R13`; design §7.2).
//!
//! Protected (critical tier). A static root, an `index` or an `immutable`
//! pattern decides which files a route serves, so a regression here would serve
//! a directory the operator never declared — or publish a dotfile with every
//! miss. Every accepted form has its denied twin, and each family asserts the
//! exact boot message the design pins, not merely "an error".
//!
//! Existence is a separate call by design: [`Manifest::validate`] never touches
//! the filesystem (the documentation fences are parsed from the repository root,
//! where no `public/` exists), so `check_static_roots` is proven on both sides.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use blastit_manifest::{
    Location, Manifest, ManifestError, ManifestWarning, ProxyName, StaticPolicy, UidLayout,
};

/// The accepted page's complete manifest, verbatim (`docs/frameworks/static-and-spa.md`).
const PAGE: &str = r#"version = 1

[[route]]
name = "site"
path = ['.*']

[route.static]
root = "public"
spa = true
immutable = ["/assets/*"]
"#;

/// The one route every table-driven case declares.
fn location() -> Location {
    Location::Route {
        index: 0,
        name: "site".to_string(),
    }
}

/// A manifest with one route named `site` and the given route-level keys.
fn route_manifest(route_keys: &str, static_block: &str) -> String {
    format!(
        "version = 1\n\n[[route]]\nname = \"site\"\npath = ['.*']\n{route_keys}\n[route.static]\n{static_block}"
    )
}

/// A manifest whose only route is static, with the given `[route.static]` body.
fn static_manifest(block: &str) -> String {
    route_manifest("", block)
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

/// Parses a document that must not parse, and returns its problems.
fn parse_errors(source: &str) -> Vec<ManifestError> {
    Manifest::from_toml_str(source)
        .expect_err("should not parse")
        .into_errors()
}

/// Validates a `[route.static]` body that must be refused.
fn violations(block: &str) -> Vec<ManifestError> {
    errors_of(&static_manifest(block))
}

/// Parses and validates a `[route.static]` body that must be accepted; a lone
/// static route has no group, proxy or codec to warn about.
fn accepted(block: &str) -> Manifest {
    let source = static_manifest(block);
    let (manifest, warnings) = Manifest::parse_and_validate(&source, UidLayout::DEFAULT_BASE)
        .unwrap_or_else(|errors| panic!("should boot:\n{errors}\n{source}"));
    assert_eq!(
        warnings,
        vec![],
        "a static route on its own warns about nothing"
    );
    manifest
}

/// The static block of `site`.
fn policy(manifest: &Manifest) -> &StaticPolicy {
    manifest
        .route(0)
        .expect("route 0")
        .static_policy()
        .expect("a static policy")
}

fn root_invalid(root: &str, rule: &'static str) -> ManifestError {
    ManifestError::StaticRootInvalid {
        location: location(),
        root: root.to_string(),
        rule,
    }
}

fn index_invalid(index: &str, rule: &'static str) -> ManifestError {
    ManifestError::StaticIndexInvalid {
        location: location(),
        index: index.to_string(),
        rule,
    }
}

fn bad_pattern(pattern: &str, rule: &'static str) -> ManifestError {
    ManifestError::StaticImmutablePattern {
        location: location(),
        pattern: pattern.to_string(),
        rule,
    }
}

fn dead(key: &str, why: &'static str) -> ManifestError {
    ManifestError::StaticDeadDeclaration {
        location: location(),
        key: key.to_string(),
        why,
    }
}

#[test]
fn the_pages_manifest_parses_with_the_defaults_it_omits() {
    let (manifest, warnings) =
        Manifest::parse_and_validate(PAGE, UidLayout::DEFAULT_BASE).expect("the page boots");
    assert_eq!(warnings, vec![]);
    assert_eq!(manifest.app(), None, "the page's block needs no app");
    assert!(manifest.is_all_static());
    assert_eq!(
        policy(&manifest),
        &StaticPolicy {
            root: "public".to_string(),
            // Omitted by the page, so the default has to appear here.
            index: "index.html".to_string(),
            spa: true,
            immutable: vec!["/assets/*".to_string()],
        }
    );

    // The accepted twin of every default: all three omitted, all three filled.
    assert_eq!(
        policy(&accepted("root = 'public'\n")),
        &StaticPolicy {
            root: "public".to_string(),
            index: "index.html".to_string(),
            spa: false,
            immutable: vec![],
        }
    );
    let explicit = accepted(
        "root = 'public'\nindex = 'shell.html'\nspa = false\nimmutable = ['/assets/*/*', '/index.html']\n",
    );
    assert_eq!(policy(&explicit).index, "shell.html");
    assert_eq!(policy(&explicit).immutable, ["/assets/*/*", "/index.html"]);
}

#[test]
fn every_bad_root_names_the_rule_it_breaks() {
    let cases = [
        (r#"root = """#, root_invalid("", "must be a relative path")),
        (
            r#"root = "/srv/public""#,
            root_invalid("/srv/public", "must be a relative path"),
        ),
        (
            r#"root = "/""#,
            root_invalid("/", "must be a relative path"),
        ),
        (
            r#"root = "../public""#,
            root_invalid("../public", "must not contain .."),
        ),
        (
            r#"root = "current/../../public""#,
            root_invalid("current/../../public", "must not contain .."),
        ),
        (
            r#"root = 'pub\lic'"#,
            root_invalid(r"pub\lic", r"must not contain \"),
        ),
        (
            r#"root = "pub\u0000lic""#,
            root_invalid("pub\u{0}lic", "must not contain a control byte"),
        ),
        (
            r#"root = "pub\u001Flic""#,
            root_invalid("pub\u{1f}lic", "must not contain a control byte"),
        ),
    ];
    for (block, expected) in cases {
        assert_eq!(violations(block), vec![expected], "{block}");
    }

    // The message the operator reads: the route, the value and the fix.
    assert_eq!(
        violations(r#"root = "/srv/public""#)[0].to_string(),
        r#"route "site" (index 0): static root "/srv/public": must be a relative path"#
    );

    // Every relative spelling is fine, a space included.
    for root in ["public", "./public", "a/b", "current/public", "pub lic"] {
        assert_eq!(policy(&accepted(&format!("root = '{root}'\n"))).root, root);
    }
}

#[test]
fn a_static_block_without_a_root_and_a_static_route_without_a_path_are_refused() {
    assert_eq!(
        parse_errors(&static_manifest("spa = true\n")),
        vec![ManifestError::MissingKey {
            path: "route[0].static.root".to_string(),
        }]
    );
    assert_eq!(
        parse_errors(
            "version = 1\n\n[[route]]\nname = \"site\"\n\n[route.static]\nroot = 'public'\n"
        ),
        vec![ManifestError::MissingKey {
            path: "route[0].path".to_string(),
        }],
        "a static route with no `path` can never be reached"
    );
}

#[test]
fn an_unknown_static_key_is_an_error_not_a_silent_drop() {
    // The design names this one: `[route.static]` is never a grant table.
    let errors = parse_errors(&static_manifest(
        "root = 'public'\nqueries = ['SELECT 1']\n",
    ));
    assert_eq!(
        errors,
        vec![ManifestError::UnknownKey {
            path: "route[0].static.queries".to_string(),
        }]
    );
    assert_eq!(
        errors[0].to_string(),
        "route[0].static.queries: unknown key"
    );

    assert_eq!(
        parse_errors(&static_manifest(
            "root = 'public'\n[route.static.tables]\nx = { select = '*' }\n"
        )),
        vec![ManifestError::UnknownKey {
            path: "route[0].static.tables".to_string(),
        }]
    );
}

#[test]
fn every_static_key_has_exactly_one_type() {
    let cases = [
        (
            "root = 1\n",
            "route[0].static.root",
            "a string",
            "an integer",
        ),
        (
            "root = 'public'\nindex = true\n",
            "route[0].static.index",
            "a string",
            "a boolean",
        ),
        (
            "root = 'public'\nspa = 'yes'\n",
            "route[0].static.spa",
            "a boolean",
            "a string",
        ),
        (
            "root = 'public'\nimmutable = '/assets/*'\n",
            "route[0].static.immutable",
            "an array",
            "a string",
        ),
        (
            "root = 'public'\nimmutable = ['/a', 2]\n",
            "route[0].static.immutable[1]",
            "a string",
            "an integer",
        ),
    ];
    for (block, path, expected, found) in cases {
        assert_eq!(
            parse_errors(&static_manifest(block)),
            vec![ManifestError::WrongType {
                path: path.to_string(),
                expected,
                found,
            }],
            "{block}"
        );
    }

    // The block itself is a table, never a bare path.
    assert_eq!(
        parse_errors(
            "version = 1\n\n[[route]]\nname = \"site\"\npath = ['.*']\nstatic = \"public\"\n"
        ),
        vec![ManifestError::WrongType {
            path: "route[0].static".to_string(),
            expected: "a table",
            found: "a string",
        }]
    );
}

#[test]
fn every_bad_index_names_the_rule_it_breaks() {
    let cases = [
        (
            r#"index = """#,
            index_invalid("", "must be one or more segments"),
        ),
        (
            r#"index = "a//b""#,
            index_invalid("a//b", "must be one or more segments"),
        ),
        (
            r#"index = "a/""#,
            index_invalid("a/", "must be one or more segments"),
        ),
        (
            r#"index = "/index.html""#,
            index_invalid("/index.html", "must be a relative path"),
        ),
        (
            r#"index = "../index.html""#,
            index_invalid("../index.html", "must not contain .."),
        ),
        (
            r#"index = ".env""#,
            index_invalid(".env", "must not contain a dotfile segment"),
        ),
        (
            r#"index = "docs/.env""#,
            index_invalid("docs/.env", "must not contain a dotfile segment"),
        ),
        (
            r#"index = 'a\b'"#,
            index_invalid(r"a\b", r"must not contain \"),
        ),
        (
            r#"index = "a\u0000b""#,
            index_invalid("a\u{0}b", "must not contain a control byte"),
        ),
    ];
    for (block, expected) in cases {
        assert_eq!(
            violations(&format!("root = 'public'\n{block}\n")),
            vec![expected],
            "{block}"
        );
    }
    assert_eq!(
        violations("root = 'public'\nindex = '.env'\n")[0].to_string(),
        r#"route "site" (index 0): static index ".env": must not contain a dotfile segment"#
    );

    for index in ["index.html", "shell.html", "docs/index.html", "a b.html"] {
        let manifest = accepted(&format!("root = 'public'\nindex = '{index}'\n"));
        assert_eq!(policy(&manifest).index, index);
    }
}

#[test]
fn every_bad_immutable_pattern_names_the_rule_it_breaks() {
    let longest = format!("/{}", "a".repeat(257));
    let thirty_three: Vec<String> = (0..33).map(|at| format!("'/a{at}'")).collect();
    let cases = [
        (
            "immutable = ['assets/*']\n".to_string(),
            bad_pattern("assets/*", "must start with /"),
        ),
        (
            "immutable = ['']\n".to_string(),
            bad_pattern("", "must start with /"),
        ),
        (
            "immutable = ['/assets/../*']\n".to_string(),
            bad_pattern("/assets/../*", "must not contain .."),
        ),
        (
            r#"immutable = ["/assets/\u0000*"]"#.to_string(),
            bad_pattern("/assets/\u{0}*", "must not contain a control byte"),
        ),
        (
            format!("immutable = ['{longest}']\n"),
            bad_pattern(&longest, "at most 256 bytes"),
        ),
        (
            format!("immutable = [{}]\n", thirty_three.join(", ")),
            bad_pattern("/a32", "at most 32 patterns"),
        ),
    ];
    for (block, expected) in cases {
        assert_eq!(
            violations(&format!("root = 'public'\n{block}")),
            vec![expected],
            "{block}"
        );
    }
    assert_eq!(
        violations("root = 'public'\nimmutable = ['assets/*']\n")[0].to_string(),
        r#"route "site" (index 0): static immutable "assets/*": must start with /"#
    );

    // A literal pattern is legal: the design's fixture D caches `/index.html`
    // itself, and the page documents `*` as the only metacharacter. Both limits
    // are inclusive.
    let manifest = accepted("root = 'public'\nimmutable = ['/index.html', '/assets/*/*']\n");
    assert_eq!(policy(&manifest).immutable, ["/index.html", "/assets/*/*"]);
    let boundary: Vec<String> = (0..32).map(|at| format!("'/a{at}'")).collect();
    assert_eq!(
        policy(&accepted(&format!(
            "root = 'public'\nimmutable = [{}]\n",
            boundary.join(", ")
        )))
        .immutable
        .len(),
        32
    );
    let longest = format!("/{}", "a".repeat(255));
    assert_eq!(
        policy(&accepted(&format!(
            "root = 'public'\nimmutable = ['{longest}']\n"
        )))
        .immutable,
        [longest]
    );
}

#[test]
fn a_spa_route_refuses_each_declaration_that_could_never_run() {
    let spa_why = "spa = true never reaches the app";
    let ingress = route_manifest(
        "",
        "root = 'public'\nspa = true\n\n[route.ingress]\nbody = \"json\"\n",
    );
    assert_eq!(errors_of(&ingress), vec![dead("[route.ingress]", spa_why)]);
    assert_eq!(
        errors_of(&ingress)[0].to_string(),
        r#"route "site" (index 0): spa = true never reaches the app; [route.ingress] can never run"#
    );

    for (route_keys, block, key) in [
        (
            "prefork = true\n",
            "root = 'public'\nspa = true\n",
            "prefork = true",
        ),
        (
            "groups = [\"session\"]\n",
            "root = 'public'\nspa = true\n",
            "groups",
        ),
    ] {
        let group = "version = 1\n\n[proxy.db]\nprotocol = \"postgresql\"\n\
                     input = { url = \"DATABASE_URL\" }\n\n[group.session.db]\nqueries = []\n\n\
                     [[route]]\nname = \"site\"\npath = ['.*']\n";
        let source = format!("{group}{route_keys}\n[route.static]\n{block}");
        assert_eq!(errors_of(&source), vec![dead(key, spa_why)], "{key}");
    }

    // A grant is a dead declaration too, and the message names its table.
    let grant = "version = 1\n\n[proxy.db]\nprotocol = \"postgresql\"\n\
                 input = { url = \"DATABASE_URL\" }\n\n[[route]]\nname = \"site\"\npath = ['.*']\n\n\
                 [route.static]\nroot = 'public'\nspa = true\n\n[route.db]\nqueries = []\n";
    assert_eq!(errors_of(grant), vec![dead("[route.db]", spa_why)]);
}

#[test]
fn only_a_route_that_can_reach_the_app_may_declare_them() {
    // No app in the manifest: the same declarations are dead, for the other
    // documented reason.
    let ingress = route_manifest("", "root = 'public'\n\n[route.ingress]\nbody = \"json\"\n");
    assert_eq!(
        errors_of(&ingress),
        vec![dead("[route.ingress]", "the manifest declares no app")]
    );

    // The front controller: one `.*` route, no `spa`, the app declared — every
    // declaration is live and validates.
    let source = "version = 1\napp = \"app:app\"\n\n\
                  [proxy.db]\nprotocol = \"postgresql\"\ninput = { url = \"DATABASE_URL\" }\n\n\
                  [group.session.db]\nqueries = []\n\n\
                  [[route]]\nname = \"site\"\npath = ['.*']\nprefork = true\ngroups = [\"session\"]\n\n\
                  [route.static]\nroot = 'public'\n\n\
                  [route.ingress]\nbody = \"json\"\n\n\
                  [route.db]\nqueries = []\n";
    let (manifest, warnings) = Manifest::parse_and_validate(source, UidLayout::DEFAULT_BASE)
        .expect("the front controller boots");
    assert_eq!(warnings, vec![]);
    let route = manifest.route(0).expect("route 0");
    assert!(!route.static_policy().expect("policy").spa);
    assert!(route.ingress().is_some());
    assert!(route.prefork());
    assert_eq!(route.groups().len(), 1);
    assert_eq!(route.grants().len(), 1);
    assert!(!manifest.is_all_static());
}

#[test]
fn only_a_manifest_whose_every_route_is_static_needs_no_app() {
    let page = Manifest::parse_and_validate(PAGE, UidLayout::DEFAULT_BASE)
        .expect("the page boots")
        .0;
    assert!(page.is_all_static());

    // An `app` makes it not all-static even when every route serves files: the
    // app still answers what the routes do not.
    let with_app = Manifest::parse_and_validate(
        &format!("app = \"app:app\"\n{PAGE}"),
        UidLayout::DEFAULT_BASE,
    )
    .expect("the page plus an app boots")
    .0;
    assert!(!with_app.is_all_static());

    // A static route next to a forwarding route, no `app`: the manifest is not
    // all-static, so the boot still needs `BLASTIT_APP` (that check is the
    // CLI's, `MB.2.6`), and the static route cannot reach an app either.
    let mixed = "version = 1\n\n[[route]]\nname = \"site\"\npath = ['/assets/.*']\n\n\
                 [route.static]\nroot = 'public'\n\n\
                 [[route]]\nname = \"app\"\npath = ['.*']\n";
    let (manifest, _) = Manifest::parse_and_validate(mixed, UidLayout::DEFAULT_BASE)
        .expect("the mixed manifest validates");
    assert_eq!(manifest.app(), None);
    assert!(!manifest.is_all_static(), "one route is not static");

    // Neither an empty document nor an app-only one is a static site.
    assert!(
        !Manifest::from_toml_str("version = 1\n")
            .expect("empty parses")
            .is_all_static()
    );
    let app_only = "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n\
                    [route.ingress]\nbody = \"json\"\n";
    assert!(
        !Manifest::from_toml_str(app_only)
            .expect("app only parses")
            .is_all_static()
    );
}

#[test]
fn static_is_a_reserved_route_key_even_when_a_proxy_of_that_name_exists() {
    // `[proxy.static]` stays legal to declare, but a route's `static` key is
    // always the block, so the proxy can never be granted.
    let source = "version = 1\napp = \"app:app\"\n\n\
                  [proxy.static]\nprotocol = \"postgresql\"\ninput = { url = \"STATIC_URL\" }\n\n\
                  [[route]]\nname = \"site\"\npath = ['.*']\nprefork = true\n\n\
                  [route.static]\nroot = 'public'\n";
    let (manifest, warnings) =
        Manifest::parse_and_validate(source, UidLayout::DEFAULT_BASE).expect("the manifest boots");
    let route = manifest.route(0).expect("route 0");
    assert!(route.static_policy().is_some(), "the block, not a grant");
    assert!(route.grants().is_empty());
    assert_eq!(
        warnings,
        vec![ManifestWarning::UngrantedProxy {
            proxy: ProxyName::from("static"),
        }]
    );
}

#[test]
fn validation_never_touches_the_filesystem() {
    // The split the design requires: the page's block validates from a
    // directory with no `public/`, and the explicit probe then refuses it.
    let dir = Scratch::new("split");
    let (manifest, _) =
        Manifest::parse_and_validate(PAGE, UidLayout::DEFAULT_BASE).expect("the page validates");
    let errors = manifest
        .check_static_roots(&dir.path)
        .expect_err("no public/ here");
    assert_eq!(
        errors.errors(),
        [ManifestError::StaticRootUnusable {
            location: location(),
            root: "public".to_string(),
            reason: "no such file or directory",
        }]
    );
    assert_eq!(
        errors.errors()[0].to_string(),
        r#"route "site" (index 0): static root "public": no such file or directory"#
    );
}

#[test]
fn a_listable_root_is_accepted_while_its_index_need_not_exist_yet() {
    let dir = Scratch::new("accept");
    std::fs::create_dir(dir.join("public")).expect("mkdir");

    // The design audits the root only: a missing `index.html` with `spa = false`
    // is a `404` at request time, never a boot error.
    let manifest = accepted("root = 'public'\nindex = 'shell.html'\nspa = false\n");
    assert_eq!(manifest.check_static_roots(&dir.path), Ok(()));
    assert!(!dir.join("public/shell.html").exists());

    // A symlinked root is what a `current` deployment looks like, and symlinks
    // are followed.
    std::os::unix::fs::symlink(dir.join("public"), dir.join("current")).expect("symlink");
    let manifest = accepted("root = 'current'\n");
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
    let source = "version = 1\n\n\
                  [[route]]\nname = \"first\"\npath = ['.*']\n\n[route.static]\nroot = 'ok'\n\n\
                  [[route]]\nname = \"second\"\npath = ['/b/.*']\n\n[route.static]\nroot = 'afile'\n\n\
                  [[route]]\nname = \"third\"\npath = ['/c/.*']\n\n[route.static]\nroot = 'gone'\n";
    let (manifest, _) = Manifest::parse_and_validate(source, UidLayout::DEFAULT_BASE)
        .expect("the manifest validates");
    assert_eq!(
        manifest.static_roots(&dir.path),
        vec![
            (0, dir.join("ok")),
            (1, dir.join("afile")),
            (2, dir.join("gone")),
        ]
    );

    let unusable = |index: usize, name: &str, root: &str, reason: &'static str| {
        ManifestError::StaticRootUnusable {
            location: Location::Route {
                index,
                name: name.to_string(),
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
            unusable(1, "second", "afile", "not a directory"),
            unusable(2, "third", "gone", "no such file or directory"),
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

    let manifest = accepted("root = 'locked'\n");
    let errors = manifest
        .check_static_roots(&dir.path)
        .expect_err("an unlistable root cannot be served");
    assert_eq!(
        errors.errors(),
        [ManifestError::StaticRootUnusable {
            location: location(),
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
