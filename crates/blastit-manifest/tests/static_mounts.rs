//! §9.4's `immutable` caps, per **mount**: more than 32 patterns in one mount,
//! or one pattern over the pattern compiler's 4096-byte cap, is refused **at
//! the pattern that breaks it**, with the sentence and `line:col` a `view`
//! pattern gets (`MB.31.2`). There is one pattern grammar and one refusal set.
//!
//! The two static shapes the user documentation promises live here too, because
//! the pages that show them still carry the retired opening and `docs_fences`
//! counts those fences without executing them. Each is written as the `Blastfile`
//! `MB.21.5.1` will put on the page, and each proves the fact the page makes:
//! the app-less site's `root` / `index` / `fallback` / `immutable` (§9.1, §9.3),
//! and the Django page's one pattern that reaches every depth (§9.4).
//!
//! `critical_static_manifest.rs` pins the same caps through the mount's own
//! grammar; this file pins them where the retired reader's `[route.static]`
//! shape cannot reach them, and keeps the accepted twin beside every refusal so
//! a validator that refused every list would fail here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Manifest, StaticMount};

/// The first refusal `source` produces, rendered as one line (§18).
///
/// [`Manifest::parse_blastfile`] is the boot and `check` entry point, so the
/// caps are asserted where a user meets them: parse, lower **and** validate.
fn refusal(source: &str) -> String {
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("the file is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// The manifest `source` validates to, with the uid base every fixture uses.
fn accepted(source: &str) -> Manifest {
    let (manifest, _warnings) =
        Manifest::parse_blastfile(source, 20_000).expect("the file is accepted");
    manifest
}

/// A `static` block holding `patterns` as one `immutable` list.
fn mount(prefix: &str, patterns: &[String]) -> String {
    let list = patterns
        .iter()
        .map(|pattern| format!("{pattern:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("static {prefix:?} {{\n  root \"public\"\n  immutable {list}\n}}\n")
}

/// The pattern texts a mount carries, in written order.
fn texts(mount: &StaticMount) -> Vec<&str> {
    mount.policy.immutable.texts().collect()
}

/// `count` distinct, legal patterns: `/p0`, `/p1`, …
fn globs(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("/p{index}")).collect()
}

/// A `/`-anchored pattern of exactly `bytes` bytes: `/` then `a`s.
fn pattern_of(bytes: usize) -> String {
    format!("/{}", "a".repeat(bytes - 1))
}

/// The `line:col` of the first occurrence of `needle` in `source`, counted from
/// the needle's first byte (so a quoted needle points at its quote).
fn position_of(source: &str, needle: &str) -> (u32, u32) {
    let (line, text) = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(needle))
        .expect("the needle is written");
    let col = text.find(needle).expect("the needle is in its line");
    (
        u32::try_from(line + 1).expect("a line number fits"),
        u32::try_from(col + 1).expect("a column fits"),
    )
}

/// 32 patterns are the documented ceiling: 32 lower, and the 33rd is refused at
/// its own pattern.
#[test]
fn a_mount_may_declare_at_most_32_immutable_patterns() {
    // The accepted twin: exactly the cap.
    let allowed = globs(32);
    let manifest = accepted(&mount("/assets", &allowed));
    let declared = &manifest.mounts()[0];
    assert_eq!(declared.prefix, "/assets");
    assert_eq!(texts(declared), allowed, "every pattern survives");

    // One past the cap: the sentence names the 33rd pattern and where it is.
    let source = mount("/assets", &globs(33));
    let (line, col) = position_of(&source, "\"/p32\"");
    let rendered = refusal(&source);
    assert_eq!(
        rendered,
        format!(
            "{line}:{col}: a mount holds at most 32 `immutable` patterns; one pattern whose \
             trailing `/*` crosses `/` reaches every depth, and `regex '…'` reaches a hashed name"
        ),
        "the first pattern past the limit is the one to delete"
    );
    assert!(!rendered.contains('\n'), "{rendered:?}");
}

/// A pattern's own cap is the compiler's 4096 bytes, not a second ceiling: 4096
/// is legal and 4097 is refused with the pattern sentence.
#[test]
fn a_mount_may_declare_patterns_of_at_most_4096_bytes() {
    // The accepted twin: exactly the cap.
    let longest = pattern_of(4_096);
    let manifest = accepted(&mount("/assets", std::slice::from_ref(&longest)));
    assert_eq!(texts(&manifest.mounts()[0]), [longest.as_str()]);

    // One byte over: the pattern's own sentence, not a `static immutable` one.
    let rendered = refusal(&mount("/assets", &[pattern_of(4_097)]));
    assert_eq!(rendered, "3:13: a pattern holds at most 4096 bytes");
    assert!(!rendered.contains('\n'), "one line (§18): {rendered:?}");
}

/// The cap blames the pattern in the **second** mount's list: with two mounts,
/// the first one's two patterns are untouched and the refusal's `line:col` falls
/// inside the second block.
#[test]
fn the_cap_names_the_pattern_in_the_mount_that_broke_it() {
    let source = format!(
        "{}{}",
        mount("/assets", &globs(2)),
        mount("/assets/v2", &globs(33))
    );
    let (line, col) = position_of(&source, "\"/p32\"");
    assert!(line > 4, "the 33rd pattern is in the second mount");
    let rendered = refusal(&source);
    assert_eq!(
        rendered,
        format!(
            "{line}:{col}: a mount holds at most 32 `immutable` patterns; one pattern whose \
             trailing `/*` crosses `/` reaches every depth, and `regex '…'` reaches a hashed name"
        )
    );
}

/// The app-less site `docs/frameworks/static-and-spa.md` documents: one mount at
/// `/` serving `public`, answering a deep link with `index.html`, caching
/// `/assets/*` for a year (§9.1, §9.3).
///
/// The page still carries the retired opening, which this crate can no longer
/// parse, so the shape is proved here against the `Blastfile` a reader will copy
/// when `MB.21.5.1` rewrites the page; `docs_fences` keeps the page's fence count
/// pinned until then.
fn documented_static_site() -> &'static str {
    "static \"/\" {\n    root \"public\"\n    index \"index.html\"\n    fallback \"index.html\"\n    immutable \"/assets/*\"\n}\n"
}

/// That shape as one predicate: no app, no view, no front controller, and the
/// one mount the page promises.
fn is_the_documented_static_site(manifest: &Manifest) -> bool {
    if !manifest.is_all_static()
        || manifest.app().is_some()
        || manifest.php().is_some()
        || manifest.route_count() != 0
    {
        return false;
    }
    let [mount] = manifest.mounts() else {
        return false;
    };
    mount.prefix == "/"
        && mount.policy.root == "public"
        && mount.policy.index == "index.html"
        // The page's `spa = true`, lowered to the file a miss is answered with
        // (§9.3).
        && mount.policy.fallback.as_deref() == Some("index.html")
        && mount.policy.immutable.texts().collect::<Vec<_>>() == ["/assets/*"]
}

/// The documented site is one mount, and an all-static file needs no app.
#[test]
fn the_documented_static_site_is_one_mount() {
    let (manifest, warnings) =
        Manifest::parse_blastfile(documented_static_site(), 20_000).expect("the page's site boots");
    assert_eq!(warnings, vec![], "the site warns about nothing");
    assert!(
        is_the_documented_static_site(&manifest),
        "the site is one mount serving `public` with a fallback and `/assets/*`:\n{}",
        documented_static_site()
    );
    assert!(
        manifest.can_boot_without_app(),
        "the page's sentence is that an all-static site needs no app"
    );
    assert!(manifest.routes().is_empty(), "a mount is not a route");
}

/// The canary: the predicate above would pass if it always answered true, so
/// every promise the page makes is changed once and has to stop satisfying it.
#[test]
fn a_drifted_static_site_is_not_the_documented_shape() {
    for (from, to) in [
        ("root \"public\"", "root \"dist\""),
        ("index \"index.html\"", "index \"main.html\""),
        ("fallback \"index.html\"", "fallback \"app.html\""),
        ("immutable \"/assets/*\"", "immutable \"/static/*\""),
        ("static \"/\"", "static \"/site\""),
    ] {
        let drifted = documented_static_site().replace(from, to);
        assert_ne!(
            drifted,
            documented_static_site(),
            "the canary must change the fixture: {from}"
        );
        let (manifest, _) =
            Manifest::parse_blastfile(&drifted, 20_000).expect("the drifted site still boots");
        assert!(
            !is_the_documented_static_site(&manifest),
            "a site with {to} is no longer the documented shape"
        );
    }
}

/// The `immutable` list `docs/frameworks/python/django.md` documented before
/// `MB.31.2`: one glob per depth, as deep as the deepest fingerprinted file
/// Django's admin ships. It is kept here as the **canary** that a six-row list
/// is not what the page carries any more.
fn retired_immutable_depths() -> Vec<String> {
    [
        "/static/*",
        "/static/*/*",
        "/static/*/*/*",
        "/static/*/*/*/*",
        "/static/*/*/*/*/*",
        "/static/*/*/*/*/*/*",
    ]
    .map(str::to_string)
    .to_vec()
}

/// The mount `docs/frameworks/python/django.md` documents: one pattern, not one
/// glob per depth (`MB.31.2`), whose trailing `/*` crosses `/` at any depth.
fn documented_django_mount() -> String {
    "static \"/static\" {\n  root \"staticfiles\"\n  immutable \"/static/*\"\n}\n".to_owned()
}

/// The Django page's one pattern reaches the admin's deepest fingerprinted file,
/// and the mount keeps the two defaults the fragment omitted.
#[test]
fn the_django_page_documents_one_pattern_that_reaches_every_depth() {
    let manifest = accepted(&documented_django_mount());
    assert_eq!(manifest.mounts().len(), 1, "one mount, not a route");
    let mount = &manifest.mounts()[0];
    assert_eq!(mount.prefix, "/static");
    assert_eq!(mount.policy.root, "staticfiles");
    assert_eq!(
        texts(mount),
        ["/static/*"],
        "one pattern replaces the six-glob staircase"
    );
    // The deepest name Django's admin ships is six segments under `/static/`:
    // `patterns` is the matcher, so this is the fact the page's sentence makes.
    assert!(
        mount
            .policy
            .immutable
            .matches("/static/admin/js/vendor/select2/i18n/de.js"),
        "a trailing `/*` crosses `/`, so one pattern reaches every depth"
    );
    assert!(
        !mount.policy.immutable.matches("/staticfiles/app.js"),
        "and it is `/`-anchored: it names no other tree"
    );
    // The page carries one pattern, and the retired six-row shape is not it: a
    // list of the old per-depth spelling is still legal text, but it is not what
    // a reader is told to write any more.
    assert_ne!(
        texts(mount),
        retired_immutable_depths(),
        "one pattern replaces the six-glob staircase"
    );
    assert!(
        !mount.policy.immutable.matches("/static"),
        "the mount's own prefix is not a file under it"
    );
    // The fragment declared neither, so both are the §9.1 defaults.
    assert_eq!(mount.policy.index, "index.html");
    assert_eq!(mount.policy.fallback, None, "the page declares no fallback");
}
