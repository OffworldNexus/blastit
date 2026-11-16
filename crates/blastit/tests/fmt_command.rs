//! `blastit fmt [--check] [PATH]` (`R19`; `docs/design/blastfile-dsl.md` §14.4).
//!
//! Every case runs the real binary in its own directory with an explicit
//! environment, so no case can see the suite's own `BLASTIT_*` variables or
//! another case's file.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
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
    /// A directory named after the test, so a leaked one is identifiable.
    fn new(tag: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("blastit-fmt-{}-{tag}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).expect("a fresh temporary directory");
        Self { path }
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
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A file that is not in canonical layout, and its canonical text.
const MESSY: &str = "app \"shop\"\nview products GET \"/products\" { body json ; prefork }\n";
const CANONICAL: &str =
    "app \"shop\"\n\nview products GET \"/products\" {\n    body json\n    prefork\n}\n";

/// `blastit fmt` rewrites the file and exits `0`.
#[test]
fn a_non_canonical_file_is_rewritten_and_exits_zero() {
    let dir = Dir::new("rewrite");
    dir.write("Blastfile", MESSY);
    let output = dir.run(&[], &["fmt"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(dir.read("Blastfile"), CANONICAL);
    assert_eq!(
        dir.entries(),
        ["Blastfile"],
        "a temporary file was left behind"
    );
}

/// `blastit fmt` on a canonical file exits `0` and changes nothing.
#[test]
fn a_canonical_file_is_left_alone() {
    let dir = Dir::new("canonical");
    dir.write("Blastfile", CANONICAL);
    let output = dir.run(&[], &["fmt"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(dir.read("Blastfile"), CANONICAL);
}

/// `--check` names a file that would change and exits `1`.
#[test]
fn check_reports_a_non_canonical_file_and_exits_one() {
    let dir = Dir::new("check-messy");
    dir.write("Blastfile", MESSY);
    let output = dir.run(&[], &["fmt", "--check"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "./Blastfile: not in canonical layout\n"
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(dir.read("Blastfile"), MESSY, "--check wrote the file");
}

/// `--check` on a canonical file exits `0` with nothing on stdout.
#[test]
fn check_on_a_canonical_file_exits_zero() {
    let dir = Dir::new("check-clean");
    dir.write("Blastfile", CANONICAL);
    let output = dir.run(&[], &["fmt", "--check"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

/// A file that does not parse exits `2` with the ordinary `file:line:col` on
/// stderr and nothing on stdout, and is left byte for byte.
#[test]
fn a_broken_file_exits_two_with_a_position() {
    let dir = Dir::new("broken");
    let broken = "view v \"/x\" {\n    body xml\n}\n";
    dir.write("Blastfile", broken);
    let output = dir.run(&[], &["fmt"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("./Blastfile:2:10: "),
        "the refusal names the file and the position: {stderr}"
    );
    assert_eq!(dir.read("Blastfile"), broken);
}

/// A missing file exits `2` and names the path with §15.1's one sentence.
#[test]
fn a_missing_file_exits_two() {
    let dir = Dir::new("missing");
    let output = dir.run(&[], &["fmt"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.trim_end(),
        "./Blastfile: no such file or directory (put a Blastfile in the working directory, or \
         set BLASTIT_MANIFEST)",
        "{stderr}"
    );
}

/// `PATH` names the file, and wins over `BLASTIT_MANIFEST`.
#[test]
fn a_path_argument_names_the_file() {
    let dir = Dir::new("path");
    dir.write("Other", MESSY);
    dir.write("Blastfile", CANONICAL);
    let output = dir.run(&[("BLASTIT_MANIFEST", "Blastfile")], &["fmt", "Other"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(dir.read("Other"), CANONICAL);
    assert_eq!(dir.read("Blastfile"), CANONICAL);
}

/// `BLASTIT_MANIFEST` names the file when no `PATH` does.
#[test]
fn blastit_manifest_names_the_file() {
    let dir = Dir::new("env");
    dir.write("Elsewhere", MESSY);
    let output = dir.run(&[("BLASTIT_MANIFEST", "Elsewhere")], &["fmt"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(dir.read("Elsewhere"), CANONICAL);
}

/// A set-but-empty `BLASTIT_MANIFEST` is a mistake, not a fallback.
#[test]
fn an_empty_blastit_manifest_is_refused() {
    let dir = Dir::new("empty-env");
    dir.write("Blastfile", MESSY);
    let output = dir.run(&[("BLASTIT_MANIFEST", "")], &["fmt"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BLASTIT_MANIFEST must not be empty"),
        "{stderr}"
    );
    assert_eq!(dir.read("Blastfile"), MESSY);
}

/// `--help` prints the documented usage and exits `0`, wherever it appears.
#[test]
fn help_is_printed_and_exits_zero() {
    let dir = Dir::new("help");
    dir.write("Blastfile", MESSY);
    for arguments in [vec!["fmt", "--help"], vec!["fmt", "--check", "--help"]] {
        let output = dir.run(&[], &arguments);
        assert!(output.status.success(), "{arguments:?}: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.starts_with("usage: blastit fmt [--check] [PATH]\n"),
            "{stdout}"
        );
        assert!(stdout.contains("--check"), "{stdout}");
        assert!(
            stdout.contains("Exit codes: 0 clean or rewritten"),
            "{stdout}"
        );
    }
    assert_eq!(dir.read("Blastfile"), MESSY, "--help wrote the file");
}

/// A second positional argument is refused, because `fmt` formats one file.
#[test]
fn a_second_path_is_refused() {
    let dir = Dir::new("two-paths");
    let output = dir.run(&[], &["fmt", "a", "b"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is a second path"), "{stderr}");
}

/// An unknown argument is refused rather than ignored.
#[test]
fn an_unknown_argument_is_refused() {
    let dir = Dir::new("unknown");
    let output = dir.run(&[], &["fmt", "--write"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown argument `--write`"), "{stderr}");
}
