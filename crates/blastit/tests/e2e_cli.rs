#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `TESTING_PLAN.md` §3 — installation and project workflow: the CLI surface and
//! the safe local handoff, driven through the real binary.
//!
//! Two promises live here, and both are about the process image rather than a
//! library call:
//!
//! * **The command line** — `--version`, `help`/`-h`, the usage text (with the
//!   presets and without the internal re-exec flags), a bad command line's exit
//!   status, and colour decided per stream with `NO_COLOR`/`TERM=dumb`.
//! * **The safe handoff** — `dev`, `check`, `fmt` and `lsp` re-exec the
//!   project's own `blastit` (`.venv`, `vendor`, `node_modules`) with the same
//!   argv, cwd and environment and print nothing, while a candidate another uid
//!   could have written is skipped, an activated `$VIRTUAL_ENV` is ambient, and
//!   only `$BLASTIT_BIN` fails closed. Production shapes (`--preset`, the
//!   re-exec grammar, `--help`, `--version`) are answered by the image the user
//!   typed.
//!
//! The handoff fixtures are `#!/bin/sh` scripts that record what the new image
//! sees, so `$0`, the arguments, the working directory and the environment are
//! observable; one case copies `/bin/true` so the ELF/`execveat` branch runs, and
//! one hard-links the running image so the "this *is* the image" answer does.
//!
//! The fixture tree lives under the first root whose ancestors the handoff's own
//! tree rule accepts (`target/tmp` in this workspace has a group-writable
//! ancestor, so the probe falls through to `$HOME`). A root that cannot host a
//! good case is named in the panic instead of turning every test into a
//! confusing refusal. Colour is only observable through a terminal, so that one
//! story runs the real binary under `script(1)`'s pty.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The binary cargo built for this suite.
const BINARY: &str = env!("CARGO_BIN_EXE_blastit");

// ---------------------------------------------------------------------------
// The command line: version, help, presets and colour.
// ---------------------------------------------------------------------------

/// A throwaway working directory with an explicit environment for one run.
struct Cli {
    dir: PathBuf,
}

impl Cli {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("blastit-e2e-cli-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a fresh CLI directory");
        Self { dir }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    fn run(&self, arguments: &[&str]) -> Output {
        self.run_env(arguments, &[])
    }

    /// `env_clear` keeps the developer's shell out of the child: a case that is
    /// *about* the environment passes exactly what it needs.
    fn run_env(&self, arguments: &[&str], environment: &[(&str, &str)]) -> Output {
        let mut command = Command::new(BINARY);
        command
            .args(arguments)
            .current_dir(&self.dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/local/bin")
            .stdin(Stdio::null());
        for (name, value) in environment {
            command.env(name, value);
        }
        command.output().expect("run the blastit binary")
    }
}

impl Drop for Cli {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// §3: `--version` and `-V` are one line on stdout, exit `0`, and nothing on
/// stderr — the line a bug report asks for.
#[test]
fn the_version_is_one_line_on_stdout_and_nothing_on_stderr() {
    let cli = Cli::new("version");
    let expected = format!("blastit {}\n", env!("CARGO_PKG_VERSION"));
    for flag in ["--version", "-V"] {
        let output = cli.run(&[flag]);
        assert_eq!(output.status.code(), Some(0), "{flag}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{flag} is one line"
        );
        assert!(
            output.stderr.is_empty(),
            "{flag} wrote stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// §3: `help`/`-h`/`--help` list the four commands and both presets, and never
/// the re-exec grammar stage 0 writes for its own images.
#[test]
fn help_lists_the_commands_and_presets_but_never_the_internal_flags() {
    let cli = Cli::new("help");
    let output = cli.run(&["--help"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "help goes to stdout: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let usage = String::from_utf8_lossy(&output.stdout).into_owned();
    for expected in [
        "blastit dev",
        "blastit check",
        "blastit [--preset NAME]",
        "--version",
    ] {
        assert!(usage.contains(expected), "{expected} is missing: {usage}");
    }
    for preset in ["digitalocean-paas", "compose-traefik"] {
        assert!(usage.contains(preset), "{preset} is missing: {usage}");
    }
    // `A-8`: the grammar between stage 0's two images is plumbing, not a
    // feature; listing it beside `dev` sells an operator a flag they must never
    // type.
    for internal in [
        "--stage=1",
        "--role=dbproxy",
        "--config-fd",
        "--dev-root",
        "--secrets-fd",
    ] {
        assert!(
            !usage.contains(internal),
            "{internal} is internal and must not be advertised: {usage}"
        );
    }

    for flag in ["-h", "help"] {
        let same = cli.run(&[flag]);
        assert_eq!(same.status.code(), Some(0), "{flag}: {same:?}");
        assert_eq!(
            String::from_utf8_lossy(&same.stdout),
            usage,
            "`{flag}` prints the same text as `--help`"
        );
    }

    for (argv, expected) in [
        (&["dev", "--help"][..], "--port"),
        (&["help", "dev"][..], "--port"),
        (&["check", "--help"][..], "--dry-run"),
        (&["test", "-h"][..], "--json"),
        (&["fmt", "--help"][..], "--check"),
        (&["lsp", "--help"][..], "Reads LSP messages"),
    ] {
        let output = cli.run(argv);
        assert_eq!(output.status.code(), Some(0), "{argv:?}: {output:?}");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains(expected), "{argv:?}: {text}");
        assert!(output.stderr.is_empty(), "{argv:?}: {output:?}");
    }

    // `A-14`: an unknown command is a usage error (`2`), not a grammar lecture.
    let unknown = cli.run(&["help", "wat"]);
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    assert!(
        unknown.stdout.is_empty(),
        "a refusal writes nothing to stdout"
    );
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("unknown subcommand 'wat'"),
        "{unknown:?}"
    );
}

/// One rejected command line: the exit code, the stderr text it must name, and
/// the empty stdout every CLI failure promises.
struct Rejection {
    argv: &'static [&'static str],
    code: i32,
    stderr: &'static str,
}

/// §3: every bad command line exits with its documented status, on stderr,
/// naming the argument that was wrong.
#[test]
fn a_bad_command_line_exits_with_its_documented_status() {
    let cli = Cli::new("matrix");
    for case in [
        Rejection {
            argv: &["dev", "--manifest", "x"],
            code: 1,
            stderr: "--manifest",
        },
        Rejection {
            argv: &["dev", "--preset", "x"],
            code: 1,
            stderr: "--preset",
        },
        Rejection {
            argv: &["dev", "--port", "70000"],
            code: 1,
            stderr: "--port",
        },
        Rejection {
            argv: &["dev", "--host", "999.9.9.9"],
            code: 1,
            stderr: "--host",
        },
        // Production has no `--port`: the listener is the `PORT` setting, and
        // both spellings of the dev flag are refused by name (`2`, one sentence).
        Rejection {
            argv: &["--port", "8080"],
            code: 2,
            stderr: "--port",
        },
        Rejection {
            argv: &["-p", "8080"],
            code: 2,
            stderr: "--port",
        },
        Rejection {
            argv: &["check", "--manifest", "x"],
            code: 2,
            stderr: "--manifest",
        },
        Rejection {
            argv: &["check", "--preset", "x"],
            code: 2,
            stderr: "--preset",
        },
        Rejection {
            argv: &["check", "--dry-run"],
            code: 2,
            stderr: "--dry-run",
        },
        Rejection {
            argv: &["test", "--preset", "x"],
            code: 2,
            stderr: "--preset",
        },
        Rejection {
            argv: &["--stage=1", "--preset", "x"],
            code: 1,
            stderr: "--preset",
        },
        Rejection {
            argv: &["check", "--", "--blastit-manifest", "x"],
            code: 2,
            stderr: "--blastit-manifest",
        },
    ] {
        let output = cli.run(case.argv);
        assert_eq!(
            output.status.code(),
            Some(case.code),
            "{:?}: {}",
            case.argv,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stdout.is_empty(),
            "{:?} wrote stdout: {}",
            case.argv,
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(case.stderr),
            "{:?}: expected `{}` in `{stderr}`",
            case.argv,
            case.stderr
        );
    }
}

/// §3 (`A-11`): a misspelled preset — whichever spelling asked for it — names
/// the real presets and exits `2` before anything about capabilities.
#[test]
fn an_unknown_preset_names_the_valid_presets_and_exits_two() {
    let cli = Cli::new("preset");
    for (environment, arguments, source) in [
        (&[][..], &["--preset", "nope"][..], "--preset"),
        (&[][..], &["--preset=nope"][..], "--preset"),
        (&[("BLASTIT_PRESET", "nope")][..], &[][..], "BLASTIT_PRESET"),
    ] {
        let output = cli.run_env(arguments, environment);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        assert!(
            output.stdout.is_empty(),
            "{arguments:?} wrote stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        // The refusal paragraph, without the usage block under it: the usage
        // always names `--preset`, so asking the whole text would not tell the
        // two spellings apart.
        let (refusal, usage) = stderr
            .split_once("usage:")
            .unwrap_or_else(|| panic!("no usage block in `{stderr}`"));
        assert!(usage.contains("digitalocean-paas"), "{stderr}");
        assert!(refusal.contains("unknown preset `nope`"), "{stderr}");
        assert!(
            refusal.contains(source),
            "the refusal names the spelling that was used ({source}): {refusal}"
        );
        let other = if source == "--preset" {
            "BLASTIT_PRESET"
        } else {
            "--preset"
        };
        assert!(
            !refusal.contains(other),
            "the refusal blames a spelling the user did not use ({other}): {refusal}"
        );
        for preset in ["digitalocean-paas", "compose-traefik"] {
            assert!(refusal.contains(preset), "{preset} is missing: {refusal}");
        }
    }

    // The other spelling of a *known* preset is accepted by the grammar and
    // reaches stage 0: on this unprivileged host that is the capability refusal
    // (`1`), never the command line's `2`.
    let accepted = cli.run(&["--preset=digitalocean-paas"]);
    assert_eq!(
        accepted.status.code(),
        Some(1),
        "a known `--preset=NAME` must leave the parser and fail later, at stage 0: {accepted:?}"
    );
    let stderr = String::from_utf8_lossy(&accepted.stderr);
    assert!(
        !stderr.contains("unknown preset") && !stderr.contains("usage:"),
        "the command line was accepted: {stderr}"
    );

    // `A-14`: `--version` answers only a command line that carries nothing else,
    // so a typo is named instead of being swallowed by `-V`.
    let mixed = cli.run(&["--preset", "nope", "-V"]);
    assert_eq!(mixed.status.code(), Some(2), "{mixed:?}");
    assert!(mixed.stdout.is_empty(), "{mixed:?}");
    assert!(
        String::from_utf8_lossy(&mixed.stderr).contains("unknown preset `nope`"),
        "{mixed:?}"
    );
}

/// `script(1)`'s absolute path, found through this process's own `PATH`.
///
/// Not hardcoded: a container may carry util-linux under `/bin` or a prefix.
fn script_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join("script"))
        .find(|candidate| candidate.is_file())
}

/// The pty `script(1)` allocates: the only way the real binary can be told
/// apart from a redirect, because `Command::output()` always hands it pipes.
fn run_in_pty(script: &Path, dir: &Path, command: &str, environment: &[(&str, &str)]) -> Output {
    let mut run = Command::new(script);
    run.args(["-q", "-e", "-c", command, "/dev/null"])
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .stdin(Stdio::null());
    for (name, value) in environment {
        run.env(name, value);
    }
    run.output().expect("run the binary under a pty")
}

/// `'<binary>' '<arg>' …` for `script -c`'s `/bin/sh` line.
fn shell_command(arguments: &[&str]) -> String {
    let mut line = format!("'{BINARY}'");
    for argument in arguments {
        line.push_str(&format!(" '{argument}'"));
    }
    line
}

/// §3 (`A-4`): colour is decided per descriptor, and `NO_COLOR` (non-empty) and
/// `TERM=dumb` win over the terminal on **both** streams.
#[test]
fn colour_is_decided_per_stream_and_the_two_variables_turn_it_off() {
    let Some(script) = script_on_path() else {
        eprintln!(
            "SKIP colour_is_decided_per_stream_and_the_two_variables_turn_it_off: \
             no `script(1)` on PATH (install util-linux)"
        );
        assert!(
            std::env::var_os("BLASTIT_REQUIRE_PTY_TESTS").is_none(),
            "BLASTIT_REQUIRE_PTY_TESTS is set, so the colour rule must not go untested: \
             install util-linux (`script`) or drop the rule from the gate's claims"
        );
        return;
    };
    let cli = Cli::new("colour");
    let help = shell_command(&["--help"]);

    // Both streams are a terminal: the help is coloured.
    let coloured = run_in_pty(&script, &cli.dir, &help, &[("TERM", "xterm-256color")]);
    assert_eq!(coloured.status.code(), Some(0), "{coloured:?}");
    assert!(
        String::from_utf8_lossy(&coloured.stdout).contains('\u{1b}'),
        "a terminal on stdout gets colour: {}",
        String::from_utf8_lossy(&coloured.stdout)
    );

    // Each variable alone is enough to forbid it — and an empty `NO_COLOR` is
    // not "no colour".
    for (environment, coloured_expected) in [
        (&[("TERM", "xterm-256color"), ("NO_COLOR", "1")][..], false),
        (&[("TERM", "dumb"), ("NO_COLOR", "")][..], false),
        (&[("TERM", "xterm-256color"), ("NO_COLOR", "")][..], true),
    ] {
        let output = run_in_pty(&script, &cli.dir, &help, environment);
        assert_eq!(output.status.code(), Some(0), "{environment:?}: {output:?}");
        let has_escape = String::from_utf8_lossy(&output.stdout).contains('\u{1b}');
        assert_eq!(
            has_escape,
            coloured_expected,
            "{environment:?} decided stdout wrongly: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    // A redirected stdout is not a terminal, so that one stream loses colour
    // while the other is still the pty.
    let help_file = cli.path("help.txt");
    let redirected = format!("{help} > '{}'", help_file.display());
    let output = run_in_pty(
        &script,
        &cli.dir,
        &redirected,
        &[("TERM", "xterm-256color")],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let written = fs::read_to_string(&help_file).expect("the redirected help was written");
    assert!(written.contains("usage:"), "{written}");
    assert!(
        !written.contains('\u{1b}'),
        "stdout's answer is its own, not stderr's: {written}"
    );

    // A refusal goes to stderr: coloured on the pty, plain once redirected —
    // and the exit status survives `script`.
    let refusal = shell_command(&["--preset", "nope"]);
    let output = run_in_pty(&script, &cli.dir, &refusal, &[("TERM", "xterm-256color")]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains('\u{1b}'),
        "a terminal on stderr gets an escape sequence: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let err_file = cli.path("err.txt");
    let redirected = format!("{refusal} 2> '{}'", err_file.display());
    let output = run_in_pty(
        &script,
        &cli.dir,
        &redirected,
        &[("TERM", "xterm-256color")],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let written = fs::read_to_string(&err_file).expect("the redirected refusal was written");
    assert!(written.contains("unknown preset `nope`"), "{written}");
    assert!(
        !written.contains('\u{1b}'),
        "a file never gets escape codes: {written}"
    );
}

// ---------------------------------------------------------------------------
// The safe local handoff (§3): the project's own `blastit` serves the command.
// ---------------------------------------------------------------------------

/// Candidate roots for a fixture tree, most faithful to the handoff's own rule
/// first: a root whose ancestors another uid can write refuses the good case.
fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
        roots.push(PathBuf::from(dir));
    }
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("blastit-e2e-cli-tests"));
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"));
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("target/tmp"));
    }
    roots.push(std::env::temp_dir());
    roots
}

/// One fixture tree whose ancestors the handoff's tree rule accepts.
struct Project {
    dir: PathBuf,
}

/// `Project::new` or a loud skip: a sandbox with no writable `$HOME` (and no
/// other root whose ancestors are safe) cannot host the good case at all.
macro_rules! project {
    ($tag:expr) => {{
        let Some(project) = Project::new($tag) else {
            return;
        };
        project
    }};
}

impl Project {
    /// A fresh project under the first root that can hand off at all.
    ///
    /// The probe *is* the production rule: a marker script at
    /// `./.venv/bin/blastit` must answer `blastit fmt`. Nothing here re-states
    /// the rule, so a test never disagrees with the binary about it.
    fn new(tag: &str) -> Option<Self> {
        let mut tried = Vec::new();
        for root in roots() {
            let dir = root.join(format!("blastit-e2e-cli-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            if let Err(err) = fs::create_dir_all(&dir) {
                tried.push(format!("{}: {err}", root.display()));
                continue;
            }
            let probe = dir.join("probe-ran.txt");
            write_script(&dir.join(".venv/bin/blastit"), &probe);
            let output = run_in(&dir, &["fmt"], &[]);
            let handed_off = probe.exists();
            let _ = fs::remove_dir_all(dir.join(".venv"));
            let _ = fs::remove_file(&probe);
            if handed_off {
                return Some(Self { dir });
            }
            tried.push(format!(
                "{}: no handoff ({})",
                root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        eprintln!(
            "SKIP handoff fixture {tag}: no fixture root satisfies the tree rule (tried: {})",
            tried.join("; ")
        );
        assert!(
            std::env::var_os("BLASTIT_REQUIRE_HANDOFF_TESTS").is_none(),
            "BLASTIT_REQUIRE_HANDOFF_TESTS is set, so the handoff rule must not go untested: \
             give this runner a writable, non-world-writable root (`CARGO_TARGET_TMPDIR` or HOME)"
        );
        None
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    /// A marker-writing script at `relative`, and the marker it writes.
    fn shim(&self, relative: &str, marker: &str) -> PathBuf {
        let marker = self.path(marker);
        let _ = fs::remove_file(&marker);
        write_script(&self.path(relative), &marker);
        marker
    }

    fn run(&self, arguments: &[&str], environment: &[(&str, &str)]) -> Output {
        run_in(&self.dir, arguments, environment)
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Runs the real binary in `dir`, with exactly `environment` plus a `PATH` (the
/// fixture scripts need `/bin/sh`, which their `#!` line names) and a token the
/// scripts echo to prove the environment crosses the `execve`.
fn run_in(dir: &Path, arguments: &[&str], environment: &[(&str, &str)]) -> Output {
    run_with(Path::new(BINARY), dir, arguments, environment)
}

/// The same, from `binary`.
///
/// A fixture that hard-links the running image has to `execve` the file it
/// linked, not the one cargo built elsewhere: `handoff` compares the candidate's
/// device and inode with `/proc/self/exe`'s, so the copy the fixture links is
/// the one that must run.
fn run_with(binary: &Path, dir: &Path, arguments: &[&str], environment: &[(&str, &str)]) -> Output {
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .env("BLASTIT_TEST_TOKEN", "one-token")
        .stdin(Stdio::null());
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().expect("run the blastit binary")
}

/// Writes an executable `/bin/sh` script that records what the new image sees.
fn write_script(path: &Path, marker: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create the shim's directory");
    }
    let body = format!(
        "#!/bin/sh\n{{\n  printf 'zero=%s\\n' \"$0\"\n  printf 'args=%s\\n' \"$*\"\n  \
         printf 'pwd=%s\\n' \"$(pwd)\"\n  printf 'done=%s\\n' \"${{BLASTIT_HANDOFF_DONE-unset}}\"\n  \
         printf 'token=%s\\n' \"${{BLASTIT_TEST_TOKEN-unset}}\"\n}} > '{}'\n",
        marker.display()
    );
    fs::write(path, body).expect("write the shim");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("make the shim executable");
}

/// The marker's content, or a panic naming the run that failed to produce it.
fn marker_text(marker: &Path, output: &Output) -> String {
    fs::read_to_string(marker).unwrap_or_else(|err| {
        panic!(
            "{} did not run ({err})\nstdout: {}\nstderr: {}",
            marker.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Is this process root? (`/proc/self` is owned by the effective uid.)
fn is_root() -> bool {
    fs::metadata("/proc/self").is_ok_and(|meta| meta.uid() == 0)
}

/// A TCP port nothing is listening on: bound once, then released.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a probe port");
    listener
        .local_addr()
        .expect("the probe port's address")
        .port()
}

/// §3: the four commands a project's own `blastit` answers, with the command
/// line, working directory, environment and marker intact — and silence.
#[test]
fn the_four_commands_hand_off_with_argv_and_environment() {
    let project = project!("commands");
    let marker = project.shim(".venv/bin/blastit", "ran-commands.txt");
    for argument in ["dev", "check", "fmt", "lsp"] {
        let _ = fs::remove_file(&marker);
        let output = project.run(&[argument], &[]);
        let text = marker_text(&marker, &output);
        assert!(
            text.contains("done=1"),
            "the child did not see BLASTIT_HANDOFF_DONE=1: {text}"
        );
        assert!(
            text.contains(&format!("args={argument}")),
            "the command line was not preserved: {text}"
        );
        assert!(
            text.contains(&format!("pwd={}", project.dir.display())),
            "the working directory was not preserved: {text}"
        );
        assert!(
            text.contains("token=one-token"),
            "the environment was not inherited: {text}"
        );
        assert!(
            text.contains(&format!(
                "zero={}",
                project.path(".venv/bin/blastit").display()
            )),
            "$0 must name the project's own script: {text}"
        );
        assert!(
            output.stdout.is_empty(),
            "a valid handoff prints nothing on stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            output.stderr.is_empty(),
            "a valid handoff prints nothing on stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// §3: the three auto-discovered locations, in order, each on its own.
#[test]
fn every_auto_discovered_location_hands_off() {
    let project = project!("locations");
    for (index, relative) in [
        ".venv/bin/blastit",
        "vendor/bin/blastit",
        "node_modules/.bin/blastit",
    ]
    .into_iter()
    .enumerate()
    {
        let marker = project.shim(relative, &format!("ran-location-{index}.txt"));
        let output = project.run(&["dev"], &[]);
        let text = marker_text(&marker, &output);
        assert!(text.contains("done=1"), "{relative}: {text}");
        assert!(
            text.contains(&format!("zero={}", project.path(relative).display())),
            "{relative} is what the child ran: {text}"
        );
        // The next location is only reached when this one is gone.
        fs::remove_file(project.path(relative)).expect("remove the shim");
    }
}

/// §3: an activated `$VIRTUAL_ENV` decides which location is probed, and the
/// `./.venv` beside the working directory is then not consulted at all.
#[test]
fn an_activated_venv_replaces_the_project_venv() {
    let project = project!("activated");
    let project_marker = project.shim(".venv/bin/blastit", "ran-project.txt");
    let activated = project.path("elsewhere/venv");
    let marker = project.path("ran-activated.txt");
    write_script(&activated.join("bin/blastit"), &marker);
    let output = project.run(
        &["dev"],
        &[(
            "VIRTUAL_ENV",
            activated.to_str().expect("the fixture path is UTF-8"),
        )],
    );
    let text = marker_text(&marker, &output);
    assert!(text.contains("done=1"), "{text}");
    assert!(
        !project_marker.exists(),
        "`$VIRTUAL_ENV` set means no `./.venv` fallback"
    );
}

/// §3: a project `blastit` that is the running image is answered, not re-exec'd.
#[test]
fn a_candidate_that_is_this_image_does_not_hand_off() {
    let project = project!("self");
    // The candidate has to be a hard link to the image this test runs: `handoff`
    // compares the candidate's device and inode with `/proc/self/exe`'s, so only
    // the same inode makes the promise honest. A hard link cannot cross
    // filesystems (`EXDEV`), which is the `sudo`/container case; the copy is
    // that fallback only.
    let real = Path::new(BINARY);
    let image = project.path("global/blastit");
    let link = project.path(".venv/bin/blastit");
    fs::create_dir_all(link.parent().expect("the shim's directory")).expect("create .venv/bin");
    let runnable = match fs::hard_link(real, &link) {
        Ok(()) => real.to_path_buf(),
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
            fs::create_dir_all(image.parent().expect("the image's directory"))
                .expect("create global");
            fs::copy(real, &image).expect("copy the running image");
            fs::hard_link(&image, &link).unwrap_or_else(|err| {
                panic!("cannot hard-link the copied image for the self-handoff fixture: {err}")
            });
            image
        }
        Err(err) => {
            panic!("cannot hard-link the running image for the self-handoff fixture: {err}")
        }
    };
    // A parent another uid can write is exactly what the tree rule skips — so a
    // candidate that was *not* recognised as this image would be reported. The
    // identity answer precedes that check, so the answer is silence.
    fs::set_permissions(
        link.parent().expect("the shim's directory"),
        fs::Permissions::from_mode(0o777),
    )
    .expect("make the candidate's parent world-writable");
    let output = run_with(&runnable, &project.dir, &["dev"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("local handoff") && !stderr.contains("skipping"),
        "a self-handoff is silent, not a warning: {stderr}"
    );
    // `dev` ran for real and answered for this directory (which has no
    // `Blastfile`), so the loop stopped at the identity check instead of
    // re-executing itself for ever.
    assert_eq!(
        output.status.code(),
        Some(1),
        "the global image must answer `dev` itself: {stderr}"
    );
}

/// §3: a candidate another uid could have written is skipped, and probing goes
/// on — a `0775` checkout or a third-party `node_modules` never bricks `dev`.
#[test]
fn a_group_or_world_writable_binary_is_skipped_and_the_next_one_runs() {
    let project = project!("modes");
    for mode in [0o777, 0o775] {
        let _ = fs::remove_file(project.path("ran-vendor.txt"));
        let unsafe_marker = project.shim(".venv/bin/blastit", "ran-venv.txt");
        fs::set_permissions(
            project.path(".venv/bin/blastit"),
            fs::Permissions::from_mode(mode),
        )
        .expect("set the shim's mode");
        let vendor_marker = project.shim("vendor/bin/blastit", "ran-vendor.txt");
        let output = project.run(&["dev"], &[]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("skipping local binary"),
            "mode {mode:04o} must be skipped with one line: {stderr}"
        );
        assert!(
            stderr.contains("continuing with the next candidate or the global binary"),
            "the skip line says what ran instead: {stderr}"
        );
        assert!(
            stderr.contains(&format!("{mode:04o}")),
            "the warning names the mode: {stderr}"
        );
        assert!(
            !unsafe_marker.exists(),
            "mode {mode:04o} must not run: {stderr}"
        );
        assert!(
            vendor_marker.exists(),
            "probing continues past an unsafe candidate: {stderr}"
        );
        fs::remove_file(project.path(".venv/bin/blastit")).expect("remove the unsafe shim");
        fs::remove_file(project.path("vendor/bin/blastit")).expect("remove the vendor shim");
    }
}

/// §3: an unsafe `$VIRTUAL_ENV` is ambient — skipped, not a refusal.
#[test]
fn an_unsafe_activated_venv_is_skipped_and_the_next_candidate_runs() {
    let project = project!("activated-skip");
    let activated = project.path("activated");
    let unsafe_marker = project.path("ran-activated.txt");
    write_script(&activated.join("bin/blastit"), &unsafe_marker);
    fs::set_permissions(
        activated.join("bin/blastit"),
        fs::Permissions::from_mode(0o777),
    )
    .expect("make the activated shim world-writable");
    let vendor_marker = project.shim("vendor/bin/blastit", "ran-vendor.txt");
    let output = project.run(
        &["dev"],
        &[(
            "VIRTUAL_ENV",
            activated.to_str().expect("the fixture path is UTF-8"),
        )],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "an unsafe ambient venv must be skipped, not fail the command: {stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "blastit dev: skipping local binary {}",
            activated.join("bin/blastit").display()
        )),
        "the skip line names the command and the path: {stderr}"
    );
    assert!(
        stderr.contains("0777"),
        "the skip line names the real reason: {stderr}"
    );
    assert!(
        stderr.contains("continuing with the next candidate or the global binary"),
        "the skip line says what ran instead: {stderr}"
    );
    assert!(
        !stderr.contains("refusing local handoff"),
        "an ambient venv is never a refusal: {stderr}"
    );
    assert!(
        !unsafe_marker.exists(),
        "the unsafe binary never runs: {stderr}"
    );
    let text = marker_text(&vendor_marker, &output);
    assert!(
        text.contains("done=1"),
        "the next candidate ran after the skip: {text}"
    );
}

/// §3: a `$BLASTIT_BIN` the user named fails closed, with the sentence and fix.
///
/// This is the one explicit name left: the user pointed at a path, so a check
/// that fails is answered instead of skipped — and no auto-discovered candidate
/// is tried in its place.
#[test]
fn an_unsafe_blastit_bin_is_refused_with_the_fix() {
    let project = project!("explicit");
    let named = project.path("named/blastit");
    let marker = project.path("ran-named.txt");
    write_script(&named, &marker);
    fs::set_permissions(&named, fs::Permissions::from_mode(0o777))
        .expect("make the named shim world-writable");
    let output = project.run(
        &["dev"],
        &[(
            "BLASTIT_BIN",
            named.to_str().expect("the fixture path is UTF-8"),
        )],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "an explicitly named binary fails closed: {stderr}"
    );
    assert!(
        stderr.contains("blastit dev: refusing local handoff to"),
        "the refusal names the command and the path: {stderr}"
    );
    assert!(
        stderr.contains("BLASTIT_NO_LOCAL_HANDOFF=1"),
        "the refusal names the fix: {stderr}"
    );
    assert!(!marker.exists(), "an unsafe candidate never runs");
    assert!(
        !stderr.contains("skipping local binary"),
        "an explicit name is a refusal, not a skip: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "a refusal writes nothing to stdout"
    );
}

/// §3: a binary another uid owns is refused.
///
/// Building the fixture needs root (`chown` to another uid). The skip is
/// **loud** and, when `BLASTIT_REQUIRE_ROOT_TESTS` is set, fatal: a gate must
/// never report a pass for the one assertion an unprivileged runner cannot
/// make. `smoke (root)` sets that variable, because ambient `CI` is also true in
/// the unprivileged `test` job, where the fixture's `chown` legitimately cannot
/// run. Root is its real home.
#[test]
fn a_binary_owned_by_another_uid_is_refused() {
    if !is_root() {
        eprintln!(
            "SKIP a_binary_owned_by_another_uid_is_refused: creating another uid's file needs root"
        );
        assert!(
            std::env::var_os("BLASTIT_REQUIRE_ROOT_TESTS").is_none(),
            "BLASTIT_REQUIRE_ROOT_TESTS is set, so this safety rule must not go untested: run \
             this suite as root (or drop the rule from the gate's claims) — the fixture needs \
             `chown` to another uid"
        );
        return;
    }
    let project = project!("uid");
    let marker = project.shim(".venv/bin/blastit", "ran-uid.txt");
    std::os::unix::fs::chown(project.path(".venv/bin/blastit"), Some(65534), Some(65534))
        .expect("give the shim to another uid");
    let output = project.run(&["dev"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipping local binary"),
        "a foreign-owned candidate is skipped: {stderr}"
    );
    assert!(
        stderr.contains("owned by uid 65534"),
        "the warning names the owner: {stderr}"
    );
    assert!(!marker.exists(), "the foreign-owned candidate never runs");
}

/// §3: a symlink is followed only into a tree the rule accepts.
#[test]
fn a_symlink_inside_the_project_hands_off_and_one_into_an_unsafe_tree_does_not() {
    let project = project!("symlink");
    let real = project.path(".venv/lib/blastit");
    let good_marker = project.path("ran-good.txt");
    write_script(&real, &good_marker);
    fs::create_dir_all(project.path(".venv/bin")).expect("create .venv/bin");
    std::os::unix::fs::symlink("../lib/blastit", project.path(".venv/bin/blastit"))
        .expect("link the console script the way a venv does");
    let output = project.run(&["dev"], &[]);
    assert!(
        good_marker.exists(),
        "a symlink inside the project is a normal venv console script: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let outside = project.path("outside");
    fs::create_dir_all(&outside).expect("create the outside directory");
    let bad_marker = project.path("ran-bad.txt");
    write_script(&outside.join("blastit"), &bad_marker);
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o777))
        .expect("make the outside directory world-writable");
    fs::remove_file(project.path(".venv/bin/blastit")).expect("remove the good link");
    std::os::unix::fs::symlink(outside.join("blastit"), project.path(".venv/bin/blastit"))
        .expect("link outside the project");
    let output = project.run(&["dev"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("skipping local binary"),
        "a link into a world-writable directory is skipped: {stderr}"
    );
    assert!(
        stderr.contains("so another user could replace the binary"),
        "the warning names the directory rule: {stderr}"
    );
    assert!(!bad_marker.exists(), "the linked binary never runs");
}

/// §3: an ELF candidate runs from the verified descriptor (`execveat`).
#[test]
fn an_elf_candidate_runs_from_the_verified_descriptor() {
    let project = project!("elf");
    let victim = project.path(".venv/bin/blastit");
    fs::create_dir_all(victim.parent().expect("the shim's directory")).expect("create .venv/bin");
    let bytes = fs::read("/bin/true").expect("/bin/true exists on a Linux host");
    fs::write(&victim, bytes).expect("copy /bin/true");
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o755)).expect("make it executable");
    let output = project.run(&["dev"], &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "the ELF candidate must run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

/// A thread that alternates one path between two inodes, atomically.
///
/// `rename(2)` replaces the directory entry in one step, so a reader always sees
/// one of the two complete files — never a half-written one — which is exactly
/// the substitution the handoff's descriptor checks are meant to survive.
struct Churn {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Churn {
    fn start(target: PathBuf, scratch: PathBuf, entries: Vec<(Vec<u8>, u32)>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let mut round = 0_u32;
            while !flag.load(Ordering::Relaxed) && round < 1_000_000 {
                round += 1;
                for (index, (contents, mode)) in entries.iter().enumerate() {
                    let next = scratch.join(format!("next-{index}"));
                    if fs::write(&next, contents).is_ok()
                        && fs::set_permissions(&next, fs::Permissions::from_mode(*mode)).is_ok()
                    {
                        let _ = fs::rename(&next, &target);
                    }
                }
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Churn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// §3: the ELF branch starts the **verified descriptor**, so a path swapped under
/// it never runs a program verification refused.
///
/// The window between `verify` and `execveat` cannot be injected from a test, so
/// the probe is a churn: a thread renames the candidate between a trusted ELF
/// (`/bin/true`, `0755`) and a marker-writing script that the mode check must
/// skip (`0777`, "another user could have written it"). Every run either hands
/// off to the verified ELF or skips the script; a path re-open would run the
/// unverified script and leave its marker. `handed_off > 0` proves the churn did
/// reach the ELF branch, so the test cannot pass by skipping everything.
#[test]
fn a_swapped_path_never_runs_a_program_that_verification_skipped() {
    let project = project!("toctou");
    let candidate = project.path(".venv/bin/blastit");
    fs::create_dir_all(candidate.parent().expect("the shim's directory"))
        .expect("create .venv/bin");
    let scratch = project.path("churn");
    fs::create_dir_all(&scratch).expect("create the churn directory");
    let marker = project.path("swapped-ran.txt");
    let good = fs::read("/bin/true").expect("/bin/true exists on a Linux host");
    let evil = format!("#!/bin/sh\n: > '{}'\n", marker.display()).into_bytes();
    let churn = Churn::start(candidate, scratch, vec![(good, 0o755), (evil, 0o777)]);

    let mut handed_off = 0;
    for _ in 0..60 {
        let output = project.run(&["dev"], &[]);
        assert!(
            !marker.exists(),
            "an unverified swapped program ran:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        match output.status.code() {
            // The verified ELF ran (`/bin/true`, whatever its argv).
            Some(0) => handed_off += 1,
            // The `0777` script was skipped, so the global `dev` answered for a
            // directory that has no `Blastfile`.
            Some(1) => {}
            other => panic!(
                "unexpected status {other:?}:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }
    drop(churn);
    assert!(
        handed_off > 0,
        "no run reached the ELF branch, so the churn proved nothing"
    );
}

/// §3: a project file that is neither an ELF image nor a `#!` script gets its
/// own sentence — the one failure branch no other case reaches.
#[test]
fn a_file_that_is_neither_an_elf_image_nor_a_script_is_not_run() {
    let project = project!("not-a-program");
    let victim = project.path(".venv/bin/blastit");
    fs::create_dir_all(victim.parent().expect("the shim's directory")).expect("create .venv/bin");
    fs::write(
        &victim,
        "@echo off\r\nrem a batch file a `node_modules` shim might ship\r\n",
    )
    .expect("write a file nothing can execute");
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o755)).expect("make it executable");
    let output = project.run(&["dev"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot run local binary"),
        "a file that is not a program gets the exec sentence: {stderr}"
    );
    assert!(
        stderr.contains("neither an ELF image nor a `#!` script"),
        "the sentence names why: {stderr}"
    );
    assert!(
        stderr.contains("continuing with the next candidate or the global binary"),
        "the sentence says what ran instead: {stderr}"
    );
}

/// §3: the message names the word the user typed, so `blastit test` reports
/// `blastit test:` and not the `check` it aliases.
#[test]
fn the_message_names_the_command_the_user_typed() {
    let project = project!("test-alias");
    let marker = project.shim(".venv/bin/blastit", "ran-test.txt");
    fs::set_permissions(
        project.path(".venv/bin/blastit"),
        fs::Permissions::from_mode(0o777),
    )
    .expect("make the shim world-writable");
    let output = project.run(&["test"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("blastit test: skipping local binary"),
        "the line names `test`: {stderr}"
    );
    assert!(
        !stderr.contains("blastit check: skipping"),
        "the alias must not be renamed to its target in the handoff line: {stderr}"
    );
    assert!(!marker.exists());
}

/// §3: the production shapes are answered by this image, never the project's.
///
/// The flag list is only half of it: a future grammar that let a re-exec token
/// sit *after* a subcommand (`dev --stage=1`), or that accepted a subcommand
/// word for `Serve`, would re-open the bypass just as surely.
#[test]
fn the_production_shapes_never_hand_off() {
    let project = project!("serve");
    let marker = project.shim(".venv/bin/blastit", "ran-serve.txt");
    for arguments in [
        vec!["--version"],
        vec!["-V"],
        vec!["--help"],
        vec!["help"],
        vec!["help", "dev"],
        vec!["--preset", "digitalocean-paas"],
        vec!["--stage=1"],
        vec!["--stage=1", "--config-fd=0"],
        vec!["--role=dbproxy"],
        // A re-exec token after a subcommand word.
        vec!["dev", "--stage=1"],
        vec!["test", "--stage=1"],
        vec!["serve", "dev"],
        vec!["--role=dbproxy", "dev"],
        vec!["--stage=1", "dev"],
        vec!["dev", "--role=dbproxy"],
        vec!["dev", "--preset=x"],
    ] {
        let output = project.run(&arguments, &[]);
        assert!(
            !marker.exists(),
            "`blastit {}` handed off:\n{}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = project.run(&[], &[]);
    assert!(
        !marker.exists(),
        "bare `blastit` handed off:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// §3: `BLASTIT_HANDOFF_DONE` and `BLASTIT_NO_LOCAL_HANDOFF` are the switches,
/// and only `""`/`"0"` leave the handoff on.
#[test]
fn the_two_switches_control_the_handoff() {
    let project = project!("switches");
    let marker = project.shim(".venv/bin/blastit", "ran-switch.txt");
    for (name, value, hands_off) in [
        ("BLASTIT_HANDOFF_DONE", "1", false),
        ("BLASTIT_HANDOFF_DONE", "", true),
        ("BLASTIT_NO_LOCAL_HANDOFF", "1", false),
        ("BLASTIT_NO_LOCAL_HANDOFF", "yes", false),
        ("BLASTIT_NO_LOCAL_HANDOFF", "0", true),
        ("BLASTIT_NO_LOCAL_HANDOFF", "", true),
    ] {
        let _ = fs::remove_file(&marker);
        let output = project.run(&["dev"], &[(name, value)]);
        assert_eq!(
            marker.exists(),
            hands_off,
            "{name}={value:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// §3: an explicit `$BLASTIT_BIN` that **is** `/proc/self/exe` is not refused,
/// **and** the image that serves the run is the one that was started.
///
/// `ci.yml`'s `standalone` job runs the binary as
/// `docker run -v "$PWD/target/debug/blastit:/usr/local/bin/blastit:ro" -e
/// BLASTIT_BIN=/usr/local/bin/blastit …`, so the explicitly named candidate has
/// the same `(st_dev, st_ino)` as `/proc/self/exe`. The identity answer comes
/// first, and the candidate is not `exec`ed either: there is nothing to hand
/// off to.
///
/// The output alone cannot show that second half — a silent self-`execve` prints
/// exactly what a refusal-free run prints — so the case serves a real `dev` and
/// reads `/proc/<pid>/exe`, the file the serving image was `exec`ed from.
#[test]
fn an_explicit_blastit_bin_that_is_this_image_is_not_refused() {
    let project = project!("self-explicit");
    let real = Path::new(BINARY);
    let dir = project.path("usr/local/bin");
    fs::create_dir_all(&dir).expect("create the candidate's directory");
    // The parent a real `docker run -v` mount has: `0777` under
    // `debian:trixie-slim`, which is exactly what the tree rule refuses.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777))
        .expect("make the candidate's parent world-writable");
    let named = dir.join("blastit");
    // The same inode as the running image, which is what `handoff` compares. The
    // copy is the `EXDEV` fallback only, and it is the file this run starts, so
    // `/proc/self/exe` names it.
    let runnable = match fs::hard_link(real, &named) {
        Ok(()) => real.to_path_buf(),
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
            let image = project.path("global/blastit");
            fs::create_dir_all(image.parent().expect("the image's directory"))
                .expect("create global");
            fs::copy(real, &image).expect("copy the running image");
            fs::hard_link(&image, &named)
                .expect("cannot hard-link the copied image for the explicit self fixture");
            image
        }
        Err(err) => {
            panic!("cannot hard-link the running image for the explicit self fixture: {err}")
        }
    };

    // A manifest that serves, so the process is still there to be inspected once
    // the handoff decision is behind it (that decision is made before a listener
    // binds). `dev` needs no privileges: it is the single-uid simulation.
    fs::write(
        project.path("Blastfile"),
        "static \"/\" {\n    root \"site\"\n}\n",
    )
    .expect("write the manifest");
    fs::create_dir_all(project.path("site")).expect("create the static root");
    fs::write(project.path("site/index.html"), "hello from the fixture\n").expect("write the page");
    let port = free_port();

    let mut child = Command::new(&runnable)
        .args(["dev", "--port", &port.to_string()])
        .current_dir(&project.dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .env("BLASTIT_BIN", &named)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the running image");
    let mut serving = false;
    for _ in 0..400 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            serving = true;
            break;
        }
        if child.try_wait().expect("poll the run").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let pid = child.id();
    // The file the serving image was `exec`ed from. `argv[0]` cannot serve
    // here: `dev` re-execs itself with `/proc/self/exe`, so every `dev` image
    // names that literal string, whichever file it came from.
    let image = fs::read_link(format!("/proc/{pid}/exe"))
        .expect("the serving image is inspectable through /proc");
    let _ = child.kill();
    let output = child.wait_with_output().expect("collect the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let started = fs::canonicalize(&runnable).ok();

    assert!(
        serving,
        "the started image must answer on 127.0.0.1:{port}: {stderr}"
    );
    assert_eq!(
        fs::canonicalize(&image).ok(),
        started,
        "the image that served the run is not the one that was started: \
         /proc/{pid}/exe names the candidate ({}) instead of {}\n{stderr}",
        named.display(),
        runnable.display()
    );
    assert!(
        !stderr.contains("refusing local handoff"),
        "an explicit $BLASTIT_BIN that IS this image must not be refused: {stderr}"
    );
    assert!(
        !stderr.contains("skipping local binary"),
        "it is not skipped either — it is answered as the running image: {stderr}"
    );
    assert!(
        !stderr.contains("0777"),
        "no mode refusal may name the parent's mode: {stderr}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("usage: blastit"),
        "the run is not a CLI usage error: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
