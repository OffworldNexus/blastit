//! The fixture tree of design §5.2/§7.1: **S**, **F**, **N** and **D** over one
//! real directory, built in `std::env::temp_dir()` (no `tempfile` dependency)
//! and removed on drop.
//!
//! S = the page's route (`spa = true`, `immutable = ["/assets/*"]`); F = the
//! same tree with `spa` unset (the front-controller shape; the manifest-level
//! `app` is the caller's business); N = the same tree with `spa = false` and no
//! `app`; D = `immutable = ["/index.html"]` for the cache-kind rows. Every
//! fixture is the same tree, so every file row is comparable across policies.
//!
//! The preconditions are hard failures, never skips: the runner is non-root, the
//! filesystem is case-sensitive and it does not normalize `café.js` — rows
//! 25/31/37/38 invert silently otherwise (design §7.1).

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use blastit_manifest::StaticPolicy;
use blastit_scheduler::static_asset::{Backend, STATIC_MAX_FILE_BYTES, StaticRoot};
use rustix::fs::{CWD, Mode, chmod, mkfifoat};

/// The hashed asset the page's fence answers.
pub const APP_JS: &str = "assets/app.4f1c2a9b7d3e.js";
/// The ACME token the `.well-known` exemption makes public.
pub const TOKEN: &str = ".well-known/acme-challenge/token";
/// A dotfile at the root (row 15).
pub const DOT_ENV: &str = ".env";
/// A dotfile directory (rows 16/17).
pub const GIT_CONFIG: &str = ".git/config";
/// The NFC spelling of the non-ASCII fixture file (row 37). The NFD spelling
/// is a different filename (row 38).
pub const CAFE_NFC: &str = "café.js";
/// The NFD spelling of the same word.
pub const CAFE_NFD: &str = "cafe\u{301}.js";
/// A filename with a space: `0x20` is legal, one byte above the C0 limit
/// (design §5.1 refuses `byte < 0x20` only). Requested as `/my%20file.css`.
pub const SPACE_CSS: &str = "my file.css";
/// `cap.bin`: exactly `STATIC_MAX_FILE_BYTES`, the inclusive ceiling.
pub const CAP_BIN: &str = "cap.bin";
/// `grow.bin`: one byte, grown past the ceiling by the TOCTOU hook.
pub const GROW_BIN: &str = "grow.bin";

/// `index.html` — the root index and the SPA answer.
pub const INDEX_HTML: &[u8] = b"<!doctype html><title>fixture root</title>\n";
/// `docs/index.html` — the directory index.
pub const DOCS_HTML: &[u8] = b"<!doctype html><title>fixture docs</title>\n";
/// `assets/app.4f1c2a9b7d3e.js` — the immutable exact file.
pub const APP_JS_BYTES: &[u8] = b"console.log('app 4f1c2a9b7d3e');\n";
/// `.well-known/acme-challenge/token` — the exception that proves the rule.
pub const TOKEN_BYTES: &[u8] = b"acme-token-fixture\n";
/// `.env` — never served.
pub const DOT_ENV_BYTES: &[u8] = b"SECRET=fixture-only\n";
/// `.git/config` — never served.
pub const GIT_CONFIG_BYTES: &[u8] = b"[core]\n\trepositoryformatversion = 0\n";
/// `café.js`, NFC bytes — served only under that exact name.
pub const CAFE_BYTES: &[u8] = b"// caf\xc3\xa9 fixture\n";
/// `my file.css` — a space is a legal filename byte, not a control.
pub const SPACE_CSS_BYTES: &[u8] = b"body{margin:0}\n";
/// `grow.bin` before the TOCTOU hook grows it past the ceiling.
pub const GROW_BIN_BYTES: &[u8] = b"g";
/// `swap.js` — the TOCTOU substitution leaf.
pub const SWAP_BYTES: &[u8] = b"swap-original-inode\n";
/// `swap.js` after the hook swapped it for a different inode.
pub const SWAPPED_BYTES: &[u8] = b"swap-substituted-inode\n";

/// The four policies of design §5.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixture {
    /// `spa = true`, `immutable = ["/assets/*"]`.
    S,
    /// `spa = false`: a miss forwards to the manifest's app.
    F,
    /// An all-static root with no app.
    N,
    /// `spa = true`, `immutable = ["/index.html"]`.
    D,
}

impl Fixture {
    /// The policy this fixture exercises, with `root` relative to the fixture's
    /// base directory.
    pub fn policy(self) -> StaticPolicy {
        let (spa, immutable): (bool, &[&str]) = match self {
            Fixture::S => (true, &["/assets/*"]),
            Fixture::F | Fixture::N => (false, &[]),
            Fixture::D => (true, &["/index.html"]),
        };
        StaticPolicy {
            root: "root".to_string(),
            index: "index.html".to_string(),
            spa,
            immutable: immutable
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect(),
        }
    }
}

/// A live fixture: `<base>/root` is the served tree, `<base>/outside` the
/// sibling `link2` escapes to.
#[derive(Debug)]
pub struct StaticTree {
    /// The tempdir the tree lives in; removed on drop.
    pub base: PathBuf,
    /// `<base>/root`, what `StaticPolicy.root = "root"` names.
    pub root: PathBuf,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl StaticTree {
    /// Builds the whole S/F/N/D tree for `fixture` and returns it.
    ///
    /// Panics on a failed precondition or a failed write: a fixture that
    /// cannot be built is a broken test, never a skip (design §7.1).
    pub fn build(fixture: Fixture) -> Self {
        assert_ne!(
            rustix::process::geteuid().as_raw(),
            0,
            "the static fixture needs an unprivileged runner (chmod 000 must hide a file)"
        );

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "blastit-static-{}-{nanos}-{count}-{fixture:?}",
            std::process::id()
        ));
        let root = base.join("root");
        fs::create_dir_all(&root).expect("fixture root directory");
        fs::create_dir_all(base.join("outside")).expect("fixture outside directory");

        write(&root.join("index.html"), INDEX_HTML);
        write(&root.join("docs/index.html"), DOCS_HTML);
        write(&root.join(APP_JS), APP_JS_BYTES);
        write(&root.join(TOKEN), TOKEN_BYTES);
        write(&root.join(DOT_ENV), DOT_ENV_BYTES);
        write(&root.join(GIT_CONFIG), GIT_CONFIG_BYTES);
        write(&root.join(CAFE_NFC), CAFE_BYTES);
        write(&root.join("swap.js"), SWAP_BYTES);
        write(&root.join(SPACE_CSS), SPACE_CSS_BYTES);
        write(&root.join(GROW_BIN), GROW_BIN_BYTES);

        // `cap.bin` is **exactly** the default ceiling: the inclusive side of
        // `finish`'s `len > max_bytes` (`MB.2.4.3`). `set_len` past nothing
        // written makes it sparse (0 B on the disk this fixture runs on), so
        // an at-cap file costs no tmpfs and is never read by the suite.
        let cap = root.join(CAP_BIN);
        {
            let file = fs::File::create(&cap).expect("fixture cap.bin");
            file.set_len(STATIC_MAX_FILE_BYTES)
                .expect("fixture cap.bin length");
        }

        // `big.bin` is larger than the default ceiling without occupying it:
        // `set_len` past a few written bytes makes it sparse.
        let big = root.join("big.bin");
        {
            let mut file = fs::File::create(&big).expect("fixture big.bin");
            file.write_all(b"x").expect("fixture big.bin byte");
            file.set_len(STATIC_MAX_FILE_BYTES + 1)
                .expect("fixture big.bin length");
        }

        // An unreadable regular file: the probe still sees it (`O_PATH` needs
        // search permission on the parents, not read on the file) and the read
        // open must answer `Hidden`.
        let mode000 = root.join("mode000");
        write(&mode000, b"unreadable\n");
        chmod(&mode000, Mode::from_bits_truncate(0o000)).expect("fixture mode000");

        // A FIFO and a UNIX socket: both are classified through the `O_PATH`
        // probe and never read-opened.
        let fifo = root.join("fifo");
        mkfifoat(CWD, &fifo, Mode::from_bits_truncate(0o644)).expect("fixture fifo");
        let socket = root.join("sock");
        let listener = UnixListener::bind(&socket).expect("fixture socket");
        drop(listener);

        // The three symlinks of rows 22/23/39.
        std::os::unix::fs::symlink("/etc/passwd", root.join("link"))
            .expect("fixture link -> /etc/passwd");
        std::os::unix::fs::symlink("../outside", root.join("link2"))
            .expect("fixture link2 -> ../outside");
        std::os::unix::fs::symlink("/proc", root.join("proclink"))
            .expect("fixture proclink -> /proc");

        // Hard preconditions, asserted as failures (design §7.1).
        assert!(
            !root.join("DOCS").exists(),
            "the fixture filesystem folds case: row 31 (`/DOCS`) cannot mean a miss"
        );
        assert!(
            root.join(CAFE_NFC).exists(),
            "the fixture filesystem cannot name café.js with its NFC bytes"
        );
        assert!(
            !root.join(CAFE_NFD).exists(),
            "the fixture filesystem normalizes café.js: row 38 cannot be a miss"
        );

        Self { base, root }
    }

    /// Opens `fixture`'s policy over this tree on `backend`.
    pub fn open(&self, fixture: Fixture, backend: Backend) -> StaticRoot {
        StaticRoot::open_with(&self.base, &fixture.policy(), backend)
            .expect("the fixture root opens")
    }

    /// `<base>/root/<rel>`.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

impl Drop for StaticTree {
    fn drop(&mut self) {
        // A `mode 000` file is removed by its parent's permission, not its own.
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// Writes `bytes` to `path`, creating parent directories.
fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture parent directory");
    }
    fs::write(path, bytes).unwrap_or_else(|error| panic!("fixture {}: {error}", path.display()));
}

/// The bytes a served URL path must return, for the matrix's byte assertion.
///
/// An unknown path returns an empty slice so the caller's comparison fails
/// loudly instead of panicking here.
pub fn content_of(url_path: &str) -> &'static [u8] {
    match url_path {
        "/index.html" => INDEX_HTML,
        "/docs/index.html" => DOCS_HTML,
        "/assets/app.4f1c2a9b7d3e.js" => APP_JS_BYTES,
        "/.well-known/acme-challenge/token" => TOKEN_BYTES,
        "/café.js" => CAFE_BYTES,
        "/my file.css" => SPACE_CSS_BYTES,
        _ => &[],
    }
}
