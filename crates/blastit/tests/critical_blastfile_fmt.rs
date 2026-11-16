//! Protected suite for `blastit fmt`'s write contract (`R19`, `R25`;
//! `docs/design/blastfile-dsl.md` §14.4).
//!
//! The command's promises are about what it does to the filesystem, so every
//! case asserts the disk, not only the exit code:
//!
//! * `--check` writes **nothing at all** — no rewrite, no temporary file, not
//!   even a changed `mtime` — and says so on stdout, with `1`;
//! * a rewrite is atomic and keeps the author's mode, and a write that cannot
//!   happen leaves the original byte for byte, with `2`;
//! * a file that is already canonical is not touched at all, so a `--check`
//!   gate never rewrites a clean tree;
//! * a parse error names `file:line:col` on stderr and writes nothing.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Distinguishes two directories created in the same process.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh directory, removed when the test ends.
struct Dir {
    /// The directory itself.
    path: PathBuf,
}

impl Dir {
    /// A directory named after the test.
    fn new(tag: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "blastit-fmt-critical-{}-{tag}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a fresh temporary directory");
        Self { path }
    }

    /// The directory.
    fn path(&self) -> &Path {
        &self.path
    }

    /// Writes one file into the directory and returns its path.
    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, content).expect("write the fixture");
        path
    }

    /// Reads one file back.
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path.join(name)).expect("read the fixture")
    }

    /// Every entry of the directory, sorted.
    fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.path)
            .expect("the directory is readable")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Runs `blastit` here with exactly `vars` in the child's environment.
    fn run(&self, vars: &[(&str, &str)], arguments: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command.args(arguments).env_clear();
        for (name, value) in vars {
            command.env(name, value);
        }
        command
            .current_dir(&self.path)
            .output()
            .expect("run the blastit binary")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A file that is not in canonical layout, and its canonical text.
const MESSY: &str = "app \"shop\"\nview products GET \"/products\" { body json ; prefork }\n";
const CANONICAL: &str =
    "app \"shop\"\n\nview products GET \"/products\" {\n    body json\n    prefork\n}\n";

/// The identity a file keeps when nothing writes it: its inode and its
/// modification time, to the nanosecond.
fn identity(path: &Path) -> (u64, i64, i64) {
    let metadata = std::fs::metadata(path).expect("the file exists");
    (metadata.ino(), metadata.mtime(), metadata.mtime_nsec())
}

/// Puts a directory's mode back so the test can remove it.
struct RestoreMode(PathBuf);

impl Drop for RestoreMode {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// `--check` on a non-canonical file writes nothing at all, in a directory it
/// could not write to even if it tried.
#[test]
fn check_writes_nothing_at_all() {
    let dir = Dir::new("check-writes-nothing");
    let file = dir.write("Blastfile", MESSY);
    let before = identity(&file);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
        .expect("chmod the directory read-only");
    let _restore = RestoreMode(dir.path().to_path_buf());

    let output = dir.run(&[], &["fmt", "--check"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "./Blastfile: not in canonical layout\n"
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(identity(&file), before, "--check touched the file");
    assert_eq!(dir.read("Blastfile"), MESSY, "--check rewrote the file");
    assert_eq!(dir.entries(), ["Blastfile"], "--check left a file behind");
}

/// `--check` on a canonical file exits `0` and touches nothing.
#[test]
fn check_on_a_canonical_file_touches_nothing() {
    let dir = Dir::new("check-clean");
    let file = dir.write("Blastfile", CANONICAL);
    let before = identity(&file);
    let output = dir.run(&[], &["fmt", "--check"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(identity(&file), before, "--check touched the file");
}

/// A rewrite is atomic — the file keeps its mode, and no temporary file
/// survives — and a second run does not write at all.
#[test]
fn in_place_rewrite_is_atomic_and_keeps_the_mode() {
    let dir = Dir::new("rewrite");
    let file = dir.write("Blastfile", MESSY);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640))
        .expect("chmod the fixture");

    let output = dir.run(&[], &["fmt"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(dir.read("Blastfile"), CANONICAL);
    assert_eq!(
        dir.entries(),
        ["Blastfile"],
        "a temporary file was left behind"
    );
    let mode = std::fs::metadata(&file).expect("the file exists").mode();
    assert_eq!(
        mode & 0o777,
        0o640,
        "the rewrite changed the mode to {:o}",
        mode & 0o777
    );

    // A canonical file is not rewritten, so the second run leaves it alone.
    let settled = identity(&file);
    let output = dir.run(&[], &["fmt"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(identity(&file), settled, "the second run rewrote the file");
}

/// A write that cannot happen leaves the original byte for byte, with `2`.
///
/// Cargo-mutants runs unprivileged, so the mode is enforced there; under uid 0
/// the kernel bypasses the check, the write succeeds, and this test says so
/// rather than asserting something it cannot observe.
#[test]
fn a_write_that_fails_leaves_the_original_intact() {
    let dir = Dir::new("unwritable");
    dir.write("Blastfile", MESSY);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
        .expect("chmod the directory read-only");
    let _restore = RestoreMode(dir.path().to_path_buf());

    // Probe with the same `O_CREAT | O_EXCL` `write_atomic` uses.
    let probe = dir.path().join("probe");
    let privileged = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if privileged {
        let _ = std::fs::remove_file(&probe);
        eprintln!("skipped: this process writes into a read-only directory (uid 0)");
        return;
    }

    let output = dir.run(&[], &["fmt"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("./Blastfile: the file could not be written: "),
        "{stderr}"
    );
    assert_eq!(dir.read("Blastfile"), MESSY, "the original was replaced");
    assert_eq!(
        dir.entries(),
        ["Blastfile"],
        "a temporary file was left behind"
    );
}

/// A parse error exits `2`, names `file:line:col` on stderr, writes nothing and
/// prints nothing on stdout.
#[test]
fn a_parse_error_writes_nothing() {
    let dir = Dir::new("broken");
    let broken = "view v \"/x\" {\n    body xml\n}\n";
    let file = dir.write("Blastfile", broken);
    let before = identity(&file);
    let output = dir.run(&[], &["fmt"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("./Blastfile:2:10: ") && stderr.contains("not a codec"),
        "{stderr}"
    );
    assert_eq!(identity(&file), before, "a refused file was rewritten");
    assert_eq!(dir.read("Blastfile"), broken);
}

/// A file that formatting would grow past the cap is refused with `2`, and the
/// original is untouched (`R25`).
///
/// The witness has to build a canonical form over 16 MiB: the formatter indents
/// every line, so the source is ~9 MiB of one-line grants.
#[test]
fn a_file_it_would_grow_past_the_cap_is_refused() {
    let dir = Dir::new("over-cap");
    let mut source = String::from("view v \"/x\" {\npostgres {\n");
    for _ in 0..1_000_000 {
        source.push_str("delete t\n");
    }
    source.push_str("}\n}\n");
    assert!(source.len() < 16_777_216, "{} bytes", source.len());
    let file = dir.write("Blastfile", &source);
    let before = identity(&file);

    let output = dir.run(&[], &["fmt"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("./Blastfile: the file is ") && stderr.contains("16777216"),
        "{stderr}"
    );
    assert_eq!(identity(&file), before, "an over-cap file was rewritten");
    assert_eq!(dir.read("Blastfile"), source);
}
