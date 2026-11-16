//! Discovery and the foreign-body law (`R16`, `R17`; design §15.1).
//!
//! One function discovers the file — `BLASTIT_MANIFEST` else `./Blastfile` in
//! the starting working directory — and one parser reads it. So this suite
//! proves, against the real binary:
//!
//! * `BLASTIT_MANIFEST` wins, and the default is `./Blastfile`;
//! * nothing walks upwards: a `Blastfile` in a parent is never read;
//! * a set-but-empty variable is refused with §15.1's sentence, exit `1` for
//!   `dev` and `2` for `check` and `fmt`;
//! * a body that is not a `Blastfile` — `version = 1` and a `[[route]]` table
//!   included — is the parser's ordinary `<file>:<line>:<col>` syntax error at
//!   `dev` (exit `1`), `check` and `fmt` (exit `2`), and no detector, no format
//!   name and no migration sentence appears beside it.
//!
//! The production boot cannot be reached unprivileged: stage 0 reads its
//! capability sets before it reads the manifest, so an unprivileged `blastit`
//! exits `1` on the `setcap` recipe (`critical_dev_and_fail_closed`). `dev` runs
//! the same stage-0 discovery and the same `config::missing_manifest`, so that
//! half is measured through `dev`; the boot's stream (`app.output` on fd 1) is
//! the same call with a different emitter.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// §15.1's one missing-file sentence, for the default path.
const MISSING: &str = "./Blastfile: no such file or directory (put a Blastfile in the \
                       working directory, or set BLASTIT_MANIFEST)";

/// §15.1's one sentence for a variable that is set and empty.
const EMPTY: &str = "BLASTIT_MANIFEST must not be empty: put ./Blastfile in the working \
                     directory or name a file";

/// A body that is not a `Blastfile`: the retired file's first two lines, so the
/// first token of the file is `version`.
const FOREIGN: &str = "\
version = 1

[[route]]
name = \"one\"
path = ['.*']
";

/// Distinguishes two directories created in the same process.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh directory, removed when the test ends.
struct Dir {
    /// The directory itself.
    path: PathBuf,
}

impl Dir {
    fn new(tag: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "blastit-discovery-{}-{tag}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a fresh temporary directory");
        Self { path }
    }

    /// Writes one file into the directory and returns its path.
    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the fixture's parent");
        }
        std::fs::write(&path, content).expect("write the fixture");
        path
    }

    /// Creates one subdirectory and returns it as its own [`Dir`].
    fn subdir(&self, name: &str) -> Self {
        let path = self.path.join(name);
        std::fs::create_dir_all(&path).expect("a subdirectory");
        Self { path }
    }

    /// The `blastit` command for a run here, with exactly `vars` in the child's
    /// environment and both streams captured.
    fn command(&self, vars: &[(&str, &str)], arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command.args(arguments).env_clear();
        for (name, value) in vars {
            command.env(name, value);
        }
        // The loader and interpreter variables a `dev` boot needs to start at
        // all; they are not `BLASTIT_*` and change no answer here.
        for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.current_dir(&self.path);
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        command.stdin(std::process::Stdio::null());
        command
    }

    /// Runs `blastit` here with exactly `vars` in the child's environment.
    fn run(&self, vars: &[(&str, &str)], arguments: &[&str]) -> Output {
        self.command(vars, arguments)
            .output()
            .expect("run the blastit binary")
    }

    /// Runs `blastit`, failing if it has not exited within `limit`.
    ///
    /// A regression on a manifest that is not a regular file is an open that
    /// never returns, so the test must bound the wait instead of hanging the
    /// suite: on the deadline the child is killed and the failure names what it
    /// had written.
    fn run_bounded(
        &self,
        vars: &[(&str, &str)],
        arguments: &[&str],
        limit: std::time::Duration,
    ) -> Output {
        let mut child = self
            .command(vars, arguments)
            .spawn()
            .expect("spawn the blastit binary");
        let deadline = std::time::Instant::now() + limit;
        loop {
            match child.try_wait().expect("poll the blastit binary") {
                Some(_) => break,
                None if std::time::Instant::now() >= deadline => {
                    let _ = child.kill();
                    let output = child.wait_with_output().expect("reap the blastit binary");
                    panic!(
                        "`blastit {}` did not exit within {limit:?}: it blocked on the manifest; \
                         stderr so far: {}",
                        arguments.join(" "),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                None => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        child
            .wait_with_output()
            .expect("collect the blastit output")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A `static "/" { root "public" }` file whose root exists: `check` audits it,
/// finds a directory and exits `0` without a suite.
fn readable_site() -> String {
    "static \"/\" {\n    root \"site-root\"\n}\n".to_string()
}

/// The same file whose root is absent: `check` exits `2` naming it, so the two
/// spellings are distinguishable by exit code alone.
fn missing_site() -> String {
    "static \"/\" {\n    root \"nowhere\"\n}\n".to_string()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn blastit_manifest_wins_over_the_default() {
    let dir = Dir::new("variable-wins");
    std::fs::create_dir_all(dir.path.join("site-root")).expect("the served root");
    // The default is broken (its root is absent) and the named file is clean:
    // the exit codes alone say which one was read.
    dir.write("Blastfile", &missing_site());
    dir.write("Other", &readable_site());

    let named = dir.run(&[("BLASTIT_MANIFEST", "Other")], &["check", "--json"]);
    assert_eq!(
        named.status.code(),
        Some(0),
        "the variable names the file that is read: {}",
        stderr_of(&named)
    );

    let defaulted = dir.run(&[], &["check", "--json"]);
    assert_eq!(
        defaulted.status.code(),
        Some(2),
        "without the variable the default is read: {}",
        stderr_of(&defaulted)
    );
    assert!(
        stderr_of(&defaulted).contains("static root \"nowhere\""),
        "{}",
        stderr_of(&defaulted)
    );

    // `fmt` obeys the same discovery: the named file is canonical (exit 0), and
    // the default is not even in canonical layout, so formatting it would
    // rewrite it — which `--check` refuses to do and reports.
    dir.write("Other", &readable_site());
    let fmt_named = dir.run(&[("BLASTIT_MANIFEST", "Other")], &["fmt", "--check"]);
    assert_eq!(fmt_named.status.code(), Some(0), "{fmt_named:?}");
}

#[test]
fn the_default_is_dot_slash_blastfile_only() {
    let dir = Dir::new("default");
    std::fs::create_dir_all(dir.path.join("site-root")).expect("the served root");
    dir.write("Blastfile", &readable_site());
    // A second file beside it is never consulted, whatever it holds: the
    // default is the one name, not "the only manifest-shaped file here".
    dir.write("Other", FOREIGN);

    let check = dir.run(&[], &["check", "--json"]);
    assert_eq!(check.status.code(), Some(0), "{}", stderr_of(&check));
    assert!(
        stderr_of(&check).is_empty(),
        "the all-static audit says nothing: {}",
        stderr_of(&check)
    );

    // And `fmt` writes the `Blastfile`, not the other file: canonical in,
    // byte-identical out, with the second file untouched.
    let fmt = dir.run(&[], &["fmt", "--check"]);
    assert_eq!(fmt.status.code(), Some(0), "{fmt:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path.join("Other")).expect("the other file"),
        FOREIGN,
        "no entry point reads a second file name"
    );
}

#[test]
fn nothing_is_read_above_the_working_directory() {
    let dir = Dir::new("no-upward-search");
    std::fs::create_dir_all(dir.path.join("site-root")).expect("the served root");
    dir.write("Blastfile", &readable_site());
    let child = dir.subdir("app");

    // The parent's file would audit clean; the working directory has none, so
    // every entry point says so instead of walking up.
    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = child.run(&[], arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}: {output:?}");
        assert_eq!(
            stderr_of(&output).trim_end(),
            format!("blastit check: {MISSING}"),
            "{arguments:?}"
        );
    }
    let fmt = child.run(&[], &["fmt"]);
    assert_eq!(fmt.status.code(), Some(2), "{fmt:?}");
    assert_eq!(stderr_of(&fmt).trim_end(), MISSING);

    // `dev` fails on the same sentence, on fd 2, and writes nothing to stdout:
    // an advertisement of a server that cannot start is a lie.
    let dev = child.run(&[("BLASTIT_MEM_HIGH_WATERMARK", "100")], &["dev"]);
    assert_eq!(dev.status.code(), Some(1), "{dev:?}");
    assert!(dev.stdout.is_empty(), "{dev:?}");
    assert_eq!(
        stderr_of(&dev).trim_end(),
        format!("blastit dev: {MISSING}")
    );
}

#[test]
fn a_set_but_empty_manifest_variable_is_refused_at_every_entry_point() {
    let dir = Dir::new("empty-variable");
    std::fs::create_dir_all(dir.path.join("site-root")).expect("the served root");
    dir.write("Blastfile", &readable_site());

    // Found or not, an empty variable is a mistake: it never falls back to the
    // default, which is why a readable `Blastfile` is present in the directory.
    let dev = dir.run(&[("BLASTIT_MANIFEST", "")], &["dev"]);
    assert_eq!(dev.status.code(), Some(1), "{dev:?}");
    assert!(dev.stdout.is_empty(), "{dev:?}");
    assert_eq!(stderr_of(&dev).trim_end(), format!("blastit dev: {EMPTY}"));

    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = dir.run(&[("BLASTIT_MANIFEST", "")], arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}: {output:?}");
        assert_eq!(
            stderr_of(&output).trim_end(),
            format!("blastit check: {EMPTY}"),
            "{arguments:?}"
        );
    }

    let fmt = dir.run(&[("BLASTIT_MANIFEST", "")], &["fmt"]);
    assert_eq!(fmt.status.code(), Some(2), "{fmt:?}");
    assert_eq!(stderr_of(&fmt).trim_end(), EMPTY);
}

#[test]
fn a_body_that_is_not_a_blastfile_is_an_ordinary_syntax_error() {
    let dir = Dir::new("foreign-body");
    dir.write("Blastfile", FOREIGN);

    // `dev`: exit `1`, nothing on stdout, the first token named at `1:1`, and
    // no second sentence.
    let dev = dir.run(&[("BLASTIT_MEM_HIGH_WATERMARK", "100")], &["dev"]);
    assert_eq!(dev.status.code(), Some(1), "{dev:?}");
    assert!(dev.stdout.is_empty(), "{dev:?}");
    assert_foreign(&stderr_of(&dev), "blastit dev: ");

    // `check`: exit `2`, nothing on stdout.
    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = dir.run(&[], arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}: {output:?}");
        assert_foreign(&stderr_of(&output), "blastit check: ");
    }

    // `fmt`: the same refusal, and the file is left byte for byte.
    let fmt = dir.run(&[], &["fmt"]);
    assert_eq!(fmt.status.code(), Some(2), "{fmt:?}");
    assert!(fmt.stdout.is_empty(), "{fmt:?}");
    let stderr = stderr_of(&fmt);
    assert!(
        stderr.starts_with("./Blastfile:1:1: "),
        "`fmt` names the file and the position on one line: {stderr}"
    );
    assert_foreign(&stderr, "");
    assert_eq!(
        std::fs::read_to_string(dir.path.join("Blastfile")).expect("the file"),
        FOREIGN,
        "a file that does not parse is never rewritten"
    );
}

/// How long an entry point that must not block gets before the test fails it.
const BOUNDED: std::time::Duration = std::time::Duration::from_secs(20);

/// An operator's `BLASTIT_MANIFEST` holding a newline prints **one** line.
///
/// The path is the operator's own string, so a control byte in it must be
/// escaped rather than written out: every manifest-path message goes through
/// `error::clipped` (§18, §15.1). A path that printed its newline would turn one
/// sentence into two, and the second would read as a second, unrelated error.
#[test]
fn a_manifest_path_holding_a_newline_prints_one_escaped_line() {
    let dir = Dir::new("escaped-path");
    let named = "./a\nb";

    // `check`: exit `2`, nothing on stdout, one line on stderr, the newline
    // written as `\n`, and the path still named.
    for arguments in [&["check"][..], &["check", "--json"][..]] {
        let output = dir.run(&[("BLASTIT_MANIFEST", named)], arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}: {output:?}");
        let stderr = stderr_of(&output);
        assert_eq!(stderr.lines().count(), 1, "{arguments:?}: {stderr:?}");
        assert!(
            stderr.contains("./a\\nb: no such file or directory"),
            "the path is escaped, not printed: {stderr:?}"
        );
    }

    // `dev`: the same sentence on fd 2, exit `1`, one line.
    let dev = dir.run(
        &[
            ("BLASTIT_MANIFEST", named),
            ("BLASTIT_MEM_HIGH_WATERMARK", "100"),
        ],
        &["dev"],
    );
    assert_eq!(dev.status.code(), Some(1), "{dev:?}");
    assert!(dev.stdout.is_empty(), "{dev:?}");
    let stderr = stderr_of(&dev);
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    assert!(
        stderr.contains("./a\\nb: no such file or directory"),
        "{stderr:?}"
    );

    // `fmt`: the same one sentence, exit `2`.
    let fmt = dir.run(&[("BLASTIT_MANIFEST", named)], &["fmt"]);
    assert_eq!(fmt.status.code(), Some(2), "{fmt:?}");
    let stderr = stderr_of(&fmt);
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    assert!(
        stderr.contains("./a\\nb: no such file or directory"),
        "{stderr:?}"
    );
}

/// A FIFO at `./Blastfile` is refused from its metadata, never opened.
///
/// `fs::read_to_string` on a FIFO with no writer blocks for ever, so a
/// regression here is a command that never returns — the whole reason the one
/// reader is `blastfile::read`, which `stat`s first. Both entry points must
/// answer promptly with their own exit code, and one line naming the file.
#[test]
fn a_fifo_at_the_manifest_path_cannot_block_dev_or_check() {
    let dir = Dir::new("fifo-manifest");
    let fifo = dir.path.join("Blastfile");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo runs");
    assert!(status.success(), "mkfifo failed: {status}");

    let check = dir.run_bounded(&[], &["check"], BOUNDED);
    assert_eq!(check.status.code(), Some(2), "{check:?}");
    assert!(check.stdout.is_empty(), "{check:?}");
    let stderr = stderr_of(&check);
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    assert!(
        stderr.contains("Blastfile: ") && stderr.contains("not a regular file"),
        "the refusal names the file and the reason: {stderr:?}"
    );

    let dev = dir.run_bounded(&[("BLASTIT_MEM_HIGH_WATERMARK", "100")], &["dev"], BOUNDED);
    assert_eq!(dev.status.code(), Some(1), "{dev:?}");
    assert!(dev.stdout.is_empty(), "{dev:?}");
    let stderr = stderr_of(&dev);
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    assert!(
        stderr.contains("Blastfile: ") && stderr.contains("not a regular file"),
        "the refusal names the file and the reason: {stderr:?}"
    );

    // `fmt` reads through the same reader and answers the same way.
    let fmt = dir.run_bounded(&[], &["fmt"], BOUNDED);
    assert_eq!(fmt.status.code(), Some(2), "{fmt:?}");
    assert!(stderr_of(&fmt).contains("not a regular file"), "{fmt:?}");
}

/// The ordinary syntax error, and nothing beside it: the first token, its
/// position, no second file name, no format name, no migration sentence.
fn assert_foreign(stderr: &str, prefix: &str) {
    let body = stderr
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("expected the command's own prefix {prefix:?}: {stderr}"));
    assert!(body.contains("1:1:"), "the refusal is positioned: {stderr}");
    assert!(
        body.contains("`version` is not a keyword"),
        "the refusal names the token and how a file opens: {stderr}"
    );
    assert!(
        body.contains("Blastfile"),
        "the refusal names the file it read: {stderr}"
    );
    for banned in ["TOML", "migrat", "convert", "detected", "did you mean"] {
        assert!(
            !body.contains(banned),
            "a file that does not parse is a file that does not parse; {banned:?} leaked: {stderr}"
        );
    }
}
