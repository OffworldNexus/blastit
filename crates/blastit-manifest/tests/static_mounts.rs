//! §9.4's `immutable` caps, per **mount**: more than 32 patterns in one mount,
//! or one pattern longer than 256 bytes, is refused, and the sentence names the
//! mount instead of a route.
//!
//! The two static shapes the user documentation promises live here too, because
//! the pages that show them still carry the retired opening and `docs_fences`
//! counts those fences without executing them. Each is written as the `Blastfile`
//! `MB.21.5.1` will put on the page, and each proves the fact the page makes:
//! the app-less site's `root` / `index` / `fallback` / `immutable` (§9.1, §9.3),
//! and the Django page's one cache glob per depth (§9.4).
//!
//! `critical_static_manifest.rs` pins the two caps through the retired reader's
//! `[route.static]` shape, where the subject was a route. That subject is gone: a
//! `Blastfile` writes `immutable` inside `static "<prefix>" { … }` and nothing
//! else declares it (§9.1), so a cap has to be provable against the declaration
//! a user writes. The accepted twin is in each test, so a validator that refused
//! every list would fail here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::Manifest;

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

/// `count` distinct, valid globs: `/p0`, `/p1`, …
fn globs(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("/p{index}")).collect()
}

/// A `/`-anchored pattern of exactly `bytes` bytes: `/` then `a`s.
fn pattern_of(bytes: usize) -> String {
    format!("/{}", "a".repeat(bytes - 1))
}

/// 32 patterns are the documented ceiling: 32 parse, and the 33rd is refused.
#[test]
fn a_mount_may_declare_at_most_32_immutable_patterns() {
    // The accepted twin: exactly the cap.
    let allowed = globs(32);
    let manifest = accepted(&mount("/assets", &allowed));
    let declared = &manifest.mounts()[0];
    assert_eq!(declared.prefix, "/assets");
    assert_eq!(declared.policy.immutable, allowed, "every pattern survives");

    // One past the cap: the sentence names the mount and the pattern to delete.
    let mut too_many = globs(33);
    too_many.push("/p99".to_string());
    let rendered = refusal(&mount("/assets", &too_many));
    assert_eq!(
        rendered, "mount \"/assets\" (index 0): static immutable \"/p32\": at most 32 patterns",
        "the first pattern past the limit is the one an operator deletes"
    );
    assert!(!rendered.contains('\n'), "{rendered:?}");
}

/// A pattern longer than a path component cannot name a file: 256 bytes is the
/// ceiling and 257 is refused.
#[test]
fn a_mount_may_declare_patterns_of_at_most_256_bytes() {
    // The accepted twin: exactly the ceiling.
    let longest = pattern_of(256);
    let manifest = accepted(&mount("/assets", std::slice::from_ref(&longest)));
    assert_eq!(manifest.mounts()[0].policy.immutable, [longest]);

    // One byte over: refused, and the mount is the subject.
    let rendered = refusal(&mount("/assets", &[pattern_of(257)]));
    assert!(
        rendered.starts_with("mount \"/assets\" (index 0): static immutable "),
        "the mount is named: {rendered}"
    );
    assert!(
        rendered.contains("at most 256 bytes"),
        "the rule is named: {rendered}"
    );
    assert!(!rendered.contains('\n'), "one line (§18): {rendered:?}");
}

/// The index is the mount's own, not a route's: with two mounts, the second
/// one's refusal names `index 1` and its prefix.
#[test]
fn the_sentence_names_the_mount_that_broke_the_cap() {
    let source = format!(
        "{}{}",
        mount("/assets", &globs(2)),
        mount("/assets/v2", &globs(33))
    );
    let rendered = refusal(&source);
    assert_eq!(
        rendered,
        "mount \"/assets/v2\" (index 1): static immutable \"/p32\": at most 32 patterns"
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
        && mount.policy.immutable == ["/assets/*"]
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

/// The `immutable` list `docs/frameworks/django.md` documents: one pattern per
/// depth, as deep as the deepest fingerprinted file Django's admin ships —
/// `admin/js/vendor/select2/i18n/<lang>.js` is six segments under `/static/`, so
/// a depth left out revalidates those hashed files on every request.
fn documented_immutable_depths() -> Vec<String> {
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

/// The mount `docs/frameworks/django.md` documents once `MB.21.5.1` rewrites the
/// page: its `[route.static]` fragment becomes this `static` block, and the depth
/// list is the fact that has to survive the rewrite.
fn documented_django_mount() -> String {
    let list = documented_immutable_depths()
        .iter()
        .map(|pattern| format!("{pattern:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("static \"/static\" {{\n  root \"staticfiles\"\n  immutable {list}\n}}\n")
}

/// The Django page's list is one glob per depth, and the mount keeps the two
/// defaults the fragment omitted.
#[test]
fn the_django_page_documents_one_glob_per_depth() {
    let manifest = accepted(&documented_django_mount());
    assert_eq!(manifest.mounts().len(), 1, "one mount, not a route");
    let mount = &manifest.mounts()[0];
    assert_eq!(mount.prefix, "/static");
    assert_eq!(mount.policy.root, "staticfiles");
    assert_eq!(
        mount.policy.immutable,
        documented_immutable_depths(),
        "the page lists one immutable glob per depth, as deep as the deepest \
         fingerprinted file Django's admin ships"
    );
    assert_eq!(
        mount.policy.immutable.len(),
        6,
        "six segments under `/static/` is the deepest the admin ships"
    );
    // The fragment declared neither, so both are the §9.1 defaults.
    assert_eq!(mount.policy.index, "index.html");
    assert_eq!(mount.policy.fallback, None, "the page declares no fallback");
}
