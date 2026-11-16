//! The `Blastfile` reader's input bounds (§4.5, R3a).
//!
//! `blastfile::read` is the one reader every entry point uses, and its job is
//! to be total: a file over the cap is refused from its metadata **before** a
//! byte is read (so a mode-`000` file of that size is `TooLarge`, not a
//! permission error), and anything that is not a regular file is refused from
//! the same metadata, so a FIFO with no writer or `/dev/zero` can never block
//! or exhaust a boot.
//!
//! The lexer's own rows — every token kind, every §5.3 refusal, the token and
//! trivia caps, the 1 MiB hostile corpus — live in `lexer.rs`'s `mod tests`,
//! because the tokens are `pub(crate)` and an integration test cannot see them
//! (§4.1, TS-3). `MB.21.3.8` extends this file with the token-cap rows once the
//! public parse entry point exists.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use blastit_manifest::ManifestError;
use blastit_manifest::blastfile;

/// The cap §4.5 pins, repeated here so a change to it fails this suite.
const MAX_SOURCE_BYTES: u64 = 1_048_576;

/// The size refusal, verbatim: position-less, so the caller prefixes the path
/// exactly once (§18).
const TOO_LARGE: &str = "the file is 1048577 bytes; Blastfile is limited to 1048576 bytes — \
                         split the grants into a `trait`, or delete a generated block";

/// A unique directory under `$TMPDIR`, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "blastit-lexer-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The one refusal `read` produced, or a panic naming what came back instead.
fn refusal(path: &Path) -> ManifestError {
    match blastfile::read(path) {
        Ok(text) => panic!(
            "expected a refusal from {}, read {} bytes",
            path.display(),
            text.len()
        ),
        Err(errors) => errors
            .errors()
            .first()
            .cloned()
            .expect("a refusal carries at least one error"),
    }
}

/// A regular file of `len` bytes, sparse, named `name` in `dir`.
fn sized_file(dir: &Path, name: &str, len: u64) -> PathBuf {
    let path = dir.join(name);
    let file = std::fs::File::create(&path).expect("a file");
    file.set_len(len).expect("a sized file");
    path
}

#[test]
fn the_size_cap_is_checked_without_reading() {
    let dir = TempDir::new("size-cap");

    // One byte over the cap is refused, and the refusal says which size it saw.
    let over = sized_file(dir.path(), "over", MAX_SOURCE_BYTES + 1);
    let error = refusal(&over);
    assert_eq!(error.to_string(), TOO_LARGE);
    assert!(matches!(
        error,
        ManifestError::TooLarge { bytes } if bytes == MAX_SOURCE_BYTES + 1
    ));

    // The same size with no permission at all is still `TooLarge`: the size is
    // known from the metadata, so no read — and no permission check — happens
    // first. This is the observable "the size was known before any read".
    let unreadable = sized_file(dir.path(), "unreadable", MAX_SOURCE_BYTES + 1);
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000))
        .expect("mode 000");
    let error = refusal(&unreadable);
    assert!(
        matches!(error, ManifestError::TooLarge { .. }),
        "a mode-000 file over the cap must be TooLarge, not {error}"
    );
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o600))
        .expect("mode 600");

    // Exactly the cap is read.
    let at_cap = sized_file(dir.path(), "at-cap", MAX_SOURCE_BYTES);
    let text = blastfile::read(&at_cap).expect("exactly the cap reads");
    assert_eq!(text.len(), MAX_SOURCE_BYTES as usize);
}

#[test]
fn a_fifo_cannot_exceed_the_cap() {
    let dir = TempDir::new("fifo");
    let fifo = dir.path().join("fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo runs");
    assert!(status.success(), "mkfifo failed: {status}");

    // `read` must refuse from the metadata, never open the FIFO: an open with
    // no writer blocks forever, which would be a hung boot. The thread plus a
    // timeout turns that regression into a failure instead of a hang.
    let (sender, receiver) = std::sync::mpsc::channel();
    let path = fifo.clone();
    std::thread::spawn(move || {
        let _ = sender.send(refusal(&path));
    });
    match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(error) => assert!(
            matches!(error, ManifestError::NotRegularFile),
            "a FIFO must be refused as not a regular file, got {error}"
        ),
        Err(_) => panic!("read blocked on a FIFO: it must refuse from the metadata before opening"),
    }

    // A character device is refused the same way, and just as quickly.
    let error = refusal(Path::new("/dev/zero"));
    assert!(
        matches!(error, ManifestError::NotRegularFile),
        "/dev/zero must be refused as not a regular file, got {error}"
    );
}

#[test]
fn a_missing_file_and_a_directory_are_refused() {
    let dir = TempDir::new("missing");

    let missing = dir.path().join("no-such-file");
    let error = refusal(&missing);
    assert!(
        matches!(error, ManifestError::Unreadable { .. }),
        "a missing file must be Unreadable, got {error}"
    );
    assert!(
        error.to_string().contains("No such file or directory"),
        "the operating system's sentence is kept: {error}"
    );

    let error = refusal(dir.path());
    assert!(
        matches!(error, ManifestError::NotRegularFile),
        "a directory must be refused as not a regular file, got {error}"
    );
}

/// The token cap is reachable through the public entry point, and it trips
/// while the lexer pushes rather than after the tree is built (`R3b`).
#[test]
fn the_token_cap_is_reachable_through_the_public_entry_point() {
    // 262 145 words is one token past the cap, and well under the size cap, so
    // the refusal is the token budget and not the file budget.
    let source = "a ".repeat(262_145);
    assert!(source.len() < 1_048_576, "the size cap is not what trips");
    let errors = blastit_manifest::Manifest::from_blastfile_str(&source)
        .expect_err("the token cap is a refusal");
    let rendered = errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    assert_eq!(
        rendered,
        "262145 tokens; Blastfile is limited to 262144 — split the file"
    );
}

/// A hostile document is refused cleanly: never a panic, never an unbounded
/// loop, and always one line (`R4`).
#[test]
fn a_hostile_document_is_refused_cleanly() {
    let hostile: &[(&str, String)] = &[
        ("a wall of braces", "{".repeat(1_048_576)),
        ("a wall of comments", "#x\n".repeat(262_145)),
        (
            "an unterminated string",
            format!("\"{}", "a".repeat(1_048_576 - 1)),
        ),
        ("a wall of views", "view v { ".repeat(100_000)),
        ("a file over the cap", "a".repeat(1_048_577)),
    ];
    for (label, source) in hostile {
        let errors = blastit_manifest::Manifest::from_blastfile_str(source)
            .expect_err("a hostile document is refused");
        assert!(!errors.is_empty(), "{label} must be refused");
        let rendered = errors
            .errors()
            .first()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert!(!rendered.contains('\n'), "{label}: {rendered:?}");
        assert!(!rendered.is_empty(), "{label}");
    }
}
