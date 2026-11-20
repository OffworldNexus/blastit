//! The static/SPA fixtures and the two-backend seam, shared by the §2 static
//! legs in `e2e_dev.rs`.
//!
//! [`answer`] runs the **scheduler's own pipeline** — `head_segments`, then
//! `resolve_file`, then `resolve_fallback` for a miss, then `respond` — on either
//! [`Backend`](blastit_scheduler::static_asset::Backend), so `openat2` and
//! `Walk` are compared on bytes, cache rules, conditional answers **and refusal
//! kinds** without the HTTP layer in between.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use blastit_manifest::Manifest;
use blastit_scheduler::static_asset::{Backend, StaticOutcome, StaticRoot};

/// The SPA shape the page documents: a history-mode fallback, and the
/// **pattern** spelling of `immutable` — `"/assets/*"` crosses `/`, so the
/// hashed chunks under `assets/chunks/` are cached and the unhashed originals
/// (`report.txt`) are not (`docs/reference/blastfile.md` §`immutable`).
pub const SPA_MANIFEST: &str = "\
static \"/\" {
    root \"site\"
    fallback \"index.html\"
    immutable \"/assets/*\"
}
";

/// The index shell every deep link answers with.
pub const SPA_INDEX: &str =
    "<!doctype html><title>shop</title><div id=app>blastit-spa-shell</div>\n";

/// A fingerprinted bundle directly under `assets/`.
pub const SPA_BUNDLE: &str = "console.log('blastit-bundle-9f3c');\n";

/// A fingerprinted chunk one directory deeper: the `"/assets/*"` pattern must
/// cross `/` to cache this one.
pub const SPA_CHUNK: &str = "export const vendor = 'blastit-chunk-1a2b';\n";

/// A file the pattern does **not** match, so it must not earn the year.
pub const SPA_REPORT: &str = "daily report: plain text\n";

/// Two nested mounts: the longest prefix answers (`docs/reference/blastfile.md`
/// §`Request precedence`), and neither tree has a fallback.
pub const MOUNTS_MANIFEST: &str = "\
static \"/static\" {
    root \"site/static\"
}

static \"/static/deep\" {
    root \"site/deep\"
}
";

/// Writes the SPA tree under `dir`: the shell, two fingerprinted files, an
/// unhashed original, the `.well-known` pair and every shape confinement must
/// refuse. The symlink points at `/etc/hostname`, so a responder that followed
/// it would leak a real file.
pub fn write_spa(dir: &Path) {
    write(dir, "site/index.html", SPA_INDEX);
    write(dir, "site/assets/app.9f3c.js", SPA_BUNDLE);
    write(dir, "site/assets/chunks/vendor.abc.js", SPA_CHUNK);
    write(dir, "site/report.txt", SPA_REPORT);
    write(
        dir,
        "site/.well-known/security.txt",
        "Contact: mailto:ops@example.test\n",
    );
    write(
        dir,
        "site/.well-known/acme-challenge/token-42",
        "acme-token-42\n",
    );
    write(
        dir,
        "site/.well-known/.env",
        "DATABASE_URL=postgres://well-known-leak\n",
    );
    write(dir, "site/.env", "DATABASE_URL=postgres://leak\n");
    write(
        dir,
        "site/.git/config",
        "[core]\n\trepositoryformatversion = 0\n",
    );
    write(dir, "site/console.php", "<?php echo 'source-leak';\n");
    write(dir, "site/sub/page.html", "<!doctype html><h1>sub</h1>\n");
    let _ = std::os::unix::fs::symlink("/etc/hostname", dir.join("site/redirect.js"));
}

/// Writes the two-mount tree: the same URL is answered by the deeper mount with
/// a different body, which is what makes precedence observable.
pub fn write_mounts(dir: &Path) {
    write(
        dir,
        "site/static/app.js",
        "console.log('from the shallow mount');\n",
    );
    write(
        dir,
        "site/deep/app.js",
        "console.log('from the deeper mount');\n",
    );
}

/// One fixture file, created with its parents.
fn write(dir: &Path, relative: &str, contents: &str) {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("a fixture parent directory");
    }
    std::fs::write(&path, contents).expect("a fixture file");
}

/// Opens `manifest`'s first mount under `dir` on `backend`.
pub fn open(dir: &Path, manifest: &str, backend: Backend) -> StaticRoot {
    let parsed = Manifest::from_blastfile_str(manifest).expect("the fixture manifest parses");
    StaticRoot::open_with(dir, &parsed.mounts()[0].policy, backend).expect("the fixture root opens")
}

/// One answer as a client sees it, **including which outcome produced it**.
#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    /// The status the responder or the scheduler answers.
    pub status: u16,
    /// `file`, `directory-index`, `spa-fallback`, `hidden`, `refused` or
    /// `fallback-missing`: parity has to cover *which* refusal, not only "not a
    /// file" — otherwise `Missing` and `Hidden` compare equal.
    pub kind: &'static str,
    /// `Cache-Control`, when the responder wrote one.
    pub cache: Option<String>,
    /// `ETag`, when the responder wrote one.
    pub etag: Option<String>,
    /// `Content-Length`, when the responder wrote one.
    pub length: Option<String>,
    /// The body bytes (empty for `HEAD` and `304`).
    pub body: Vec<u8>,
}

/// The documented status of each refusal: a malformed target is `400`, a hidden
/// entry (`..` is malformed, a dotfile/symlink/source is hidden) is `404`.
pub fn answer(
    root: &StaticRoot,
    target: &str,
    method: &[u8],
    if_none_match: Option<&[u8]>,
) -> Answer {
    let outcome = match blastit_scheduler::static_asset::head_segments(target) {
        Err(outcome) => outcome,
        Ok(parts) => {
            let segments: Vec<&str> = parts.iter().map(String::as_str).collect();
            match root.resolve_file(&segments) {
                StaticOutcome::Missing => root.resolve_fallback(),
                other => other,
            }
        }
    };
    let (kind, status) = match &outcome {
        StaticOutcome::File(file) => match file.kind {
            blastit_scheduler::static_asset::AnswerKind::Exact => ("file", None),
            blastit_scheduler::static_asset::AnswerKind::DirectoryIndex => {
                ("directory-index", None)
            }
            blastit_scheduler::static_asset::AnswerKind::SpaFallback => ("spa-fallback", None),
        },
        StaticOutcome::Hidden => ("hidden", Some(404)),
        StaticOutcome::Refused(_) => ("refused", Some(400)),
        StaticOutcome::FallbackMissing => ("fallback-missing", Some(404)),
        StaticOutcome::Missing => ("missing", Some(404)),
    };
    let StaticOutcome::File(file) = outcome else {
        return Answer {
            status: status.unwrap_or(404),
            kind,
            cache: None,
            etag: None,
            length: None,
            body: Vec::new(),
        };
    };
    let response = root.respond(file, method, if_none_match);
    let head = |name: &[u8]| {
        response
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| String::from_utf8_lossy(value).into_owned())
    };
    let body = match response.body {
        None => Vec::new(),
        Some(source) => {
            let mut file = std::fs::File::from(source.file);
            file.seek(SeekFrom::Start(source.offset))
                .expect("seeking the body");
            let mut bytes = vec![0u8; usize::try_from(source.len).expect("a length")];
            file.read_exact(&mut bytes).expect("reading the body");
            bytes
        }
    };
    Answer {
        status: response.status,
        kind,
        cache: head(b"cache-control"),
        etag: head(b"etag"),
        length: head(b"content-length"),
        body,
    }
}

/// §2/§6: `openat2` and `Walk` answer every row identically — same bytes, cache,
/// `HEAD`/`304` shape and refusal kind.
pub fn assert_parity(dir: &Path) {
    use blastit_scheduler::static_asset::Backend;

    let openat2 = open(dir, SPA_MANIFEST, Backend::Openat2);
    let walk = open(dir, SPA_MANIFEST, Backend::Walk);

    // Every row is one behaviour, run against both backends and compared — and
    // the *kind* is asserted, so `Hidden` and `Missing` can never compare equal.
    for (target, kind) in [
        ("/", "directory-index"),
        ("/assets/app.9f3c.js", "file"),
        ("/.well-known/security.txt", "file"),
        ("/dashboard/settings", "spa-fallback"),
        ("/../etc/passwd", "refused"),
        ("/%2e%2e/etc/passwd", "refused"),
        ("/.env", "hidden"),
        ("/.well-known/.env", "hidden"),
        ("/.git/config", "hidden"),
        ("/redirect.js", "hidden"),
        ("/console.php", "hidden"),
    ] {
        for method in [b"GET".as_slice(), b"HEAD".as_slice()] {
            let first = answer(&openat2, target, method, None);
            let second = answer(&walk, target, method, None);
            assert_eq!(
                first.kind, kind,
                "{target} is a `{kind}`, not a `{}`",
                first.kind
            );
            assert_eq!(first, second, "the two backends disagree about {target}");
        }
    }

    // `HEAD` is the `GET` head with no body on both backends, a fallback answers
    // it, and a matching validator is the same `304`.
    for root in [&openat2, &walk] {
        let get = answer(root, "/assets/app.9f3c.js", b"GET", None);
        let head = answer(root, "/assets/app.9f3c.js", b"HEAD", None);
        assert_eq!(head.status, 200);
        assert_eq!(head.length, get.length, "HEAD promises the GET's length");
        assert_eq!(head.etag, get.etag, "HEAD repeats the validator");
        assert!(head.body.is_empty() && !get.body.is_empty());
        assert_eq!(
            answer(root, "/dashboard/settings", b"HEAD", None).status,
            200
        );
        let cached = answer(root, "/assets/app.9f3c.js", b"GET", Some(b"*"));
        assert_eq!(cached.status, 304, "the validator revalidates on both");
        assert!(cached.body.is_empty(), "a `304` carries no body");
    }
}
