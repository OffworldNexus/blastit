#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Protected critical suite for the safe local CLI handoff (§4, `MB.42.2.3`).
//!
//! Every case drives the real `blastit` binary over a real fixture tree, because
//! the promises are about the process image: a project's own `blastit` runs
//! instead of the global one for `dev`/`check`/`fmt`/`lsp`, `BLASTIT_HANDOFF_DONE`
//! arrives in the child, and a candidate that fails a safety check is either
//! skipped (ambient or auto-discovered, probing continues) or refused (`exit 1`,
//! only for a `$BLASTIT_BIN` that names a path).
//!
//! The fixtures are `#!/bin/sh` scripts that write a marker, so the child's own
//! `$0`, arguments, working directory and environment are observable — and one
//! case copies `/bin/true` so the ELF/`execveat` branch runs too. The tree lives
//! under the first root whose ancestors are safe for §4's own rule
//! (`target/tmp` in this workspace has a group-writable ancestor, so the probe
//! falls through to `$HOME`); a root that cannot host a good case is named in
//! the panic instead of turning every test into a confusing refusal.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Candidate roots for a fixture tree, most faithful to the node's notes first.
fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
        roots.push(PathBuf::from(dir));
    }
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("blastit-handoff-tests"));
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"));
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("target/tmp"));
    }
    roots.push(std::env::temp_dir());
    roots
}

/// One fixture tree, whose ancestors §4 accepts.
struct Project {
    dir: PathBuf,
}

impl Project {
    /// A fresh project under the first root that can hand off at all.
    ///
    /// The probe is the production rule itself: a marker script at
    /// `./.venv/bin/blastit` must answer `blastit fmt`. Nothing here re-states
    /// the rule, so a test never disagrees with the binary about it.
    fn new(tag: &str) -> Self {
        let mut tried = Vec::new();
        for root in roots() {
            let dir = root.join(format!("blastit-handoff-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            if let Err(err) = fs::create_dir_all(&dir) {
                tried.push(format!("{}: {err}", root.display()));
                continue;
            }
            let probe = dir.join("probe-ran.txt");
            write_script(&dir.join(".venv/bin/blastit"), &probe, "");
            let output = run_in(&dir, &["fmt"], &[]);
            let handed_off = probe.exists();
            let _ = fs::remove_dir_all(dir.join(".venv"));
            let _ = fs::remove_file(&probe);
            if handed_off {
                return Self { dir };
            }
            tried.push(format!(
                "{}: no handoff ({})",
                root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        panic!(
            "no fixture root satisfies §4's tree rule: {}",
            tried.join("; ")
        );
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    /// A marker-writing script at `relative`, and the marker it writes.
    fn shim(&self, relative: &str, marker: &str) -> PathBuf {
        let marker = self.path(marker);
        let _ = fs::remove_file(&marker);
        write_script(&self.path(relative), &marker, "");
        marker
    }

    fn run(&self, arguments: &[&str], environment: &[(&str, &str)]) -> Output {
        run_in(&self.dir, arguments, environment)
    }
}

impl Drop for Project {
    /// Every case leaves the tree behind on a panic; the fixture is disposable.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Runs the real binary in `dir`, with exactly `environment` plus a `PATH` (the
/// fixture scripts need `/bin/sh`, which their `#!` line names) and a token the
/// scripts echo to prove the environment crosses the `execve`.
fn run_in(dir: &Path, arguments: &[&str], environment: &[(&str, &str)]) -> Output {
    run_with(
        Path::new(env!("CARGO_BIN_EXE_blastit")),
        dir,
        arguments,
        environment,
    )
}

/// The same, from `binary`.
///
/// A fixture that hard-links the running image has to `execve` the file it
/// linked, not the one cargo built elsewhere: `handoff` compares the candidate's
/// device and inode with `/proc/self/exe`'s, so the copy the fixture links is the
/// one that must run.
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
fn write_script(path: &Path, marker: &Path, extra: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create the shim's directory");
    }
    let body = format!(
        "#!/bin/sh\n{{\n  printf 'zero=%s\\n' \"$0\"\n  printf 'args=%s\\n' \"$*\"\n  \
         printf 'pwd=%s\\n' \"$(pwd)\"\n  printf 'done=%s\\n' \"${{BLASTIT_HANDOFF_DONE-unset}}\"\n  \
         printf 'token=%s\\n' \"${{BLASTIT_TEST_TOKEN-unset}}\"\n{extra}}} > '{}'\n",
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

/// §4: the four commands a project's own `blastit` answers, with the command
/// line, working directory, environment and marker intact.
#[test]
fn the_four_commands_hand_off_with_argv_and_environment() {
    let project = Project::new("commands");
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

/// §4: the three auto-discovered locations, in order, each on its own.
#[test]
fn every_auto_discovered_location_hands_off() {
    let project = Project::new("locations");
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

/// §4: an activated `$VIRTUAL_ENV` decides which location is probed, and the
/// `./.venv` beside the working directory is then not consulted at all.
#[test]
fn an_activated_venv_replaces_the_project_venv() {
    let project = Project::new("activated");
    let project_marker = project.shim(".venv/bin/blastit", "ran-project.txt");
    let activated = project.path("elsewhere/venv");
    let marker = project.path("ran-activated.txt");
    write_script(&activated.join("bin/blastit"), &marker, "");
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

/// §4 step 3: a project `blastit` that is the running image stops probing.
///
/// The runtime half is that `dev` answers with no handoff line at all; the
/// ordering half (checked against the source, as `critical_lsp.rs` does for the
/// capability check) is that the identity test runs before any `execve`.
#[test]
fn a_candidate_that_is_this_image_does_not_hand_off() {
    let project = Project::new("self");
    // The candidate has to be a hard link to the image this test runs: `handoff`
    // compares the candidate's device and inode with `/proc/self/exe`'s, so only
    // the same inode makes the promise honest.
    //
    // The direct link is tried first, because a fresh copy must never be
    // `execve`d under the parallel harness: `Command::output()` forks the whole
    // process, and a fork taken while a write descriptor on that copy is open (or
    // a descendant that forks without `exec`, as `blastit` itself does) leaves the
    // inode busy — `execve` then returns `ETXTBSY`. A hard link cannot cross
    // filesystems, which is the `sudo`/container case (fixture under `$HOME`,
    // `target/` elsewhere); that suite runs with `--test-threads=1`, so the copy
    // is the `EXDEV` fallback only.
    let real = Path::new(env!("CARGO_BIN_EXE_blastit"));
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
    let output = run_with(&runnable, &project.dir, &["dev"], &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("local handoff") && !stderr.contains("skipping"),
        "a self-handoff is silent, not a warning: {stderr}"
    );
    // `dev` ran for real and answered for this directory (which has no
    // `Blastfile`), so the loop stopped at the identity check instead of
    // re-executing itself for ever.
    assert!(
        !stderr.is_empty(),
        "the global image must answer `dev` itself"
    );

    let source = include_str!("../src/cli/handoff.rs");
    let identity = source
        .find("if verified.is_self")
        .expect("the identity answer `verify` carries");
    let exec = source.find("exec_verified(&verified").expect("the exec");
    assert!(
        identity < exec,
        "the identity check must precede the exec in handoff.rs"
    );
    // One `/proc/self/exe` stat, and it is `verify`'s: re-deriving the answer in
    // `attempt` could fail transiently and `execve` the running image for nothing.
    assert_eq!(
        source.matches("blastit_sys::exe_path()").count(),
        1,
        "the running image is identified once, in `verify`, and carried on `Verified`"
    );
}

/// §4: a candidate another uid could have written is skipped, and probing goes
/// on — a `0775` checkout or a third-party `node_modules` never bricks `dev`.
#[test]
fn a_group_or_world_writable_binary_is_skipped_and_the_next_one_runs() {
    let project = Project::new("modes");
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
            "mode {mode:04o} must not run: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            vendor_marker.exists(),
            "probing continues past an unsafe candidate: {stderr}"
        );
        fs::remove_file(project.path(".venv/bin/blastit")).expect("remove the unsafe shim");
        fs::remove_file(project.path("vendor/bin/blastit")).expect("remove the vendor shim");
    }
}

/// §4: an unsafe `$VIRTUAL_ENV` is skipped, and probing continues.
///
/// An activated venv is *ambient*: the shell happens to have one, so a `0775`
/// checkout (or a `umask 002` container) must not turn every `dev` into a
/// security-flavoured `exit 1`. The unsafe binary still never runs.
#[test]
fn an_unsafe_activated_venv_is_skipped_and_the_next_candidate_runs() {
    let project = Project::new("activated-skip");
    let activated = project.path("activated");
    let unsafe_marker = project.path("ran-activated.txt");
    write_script(&activated.join("bin/blastit"), &unsafe_marker, "");
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

/// §4: a `$BLASTIT_BIN` the user named fails closed, with the sentence and fix.
///
/// This is the one explicit name left: the user pointed at a path, so a check
/// that fails is answered instead of skipped — and no auto-discovered candidate
/// is tried in its place.
#[test]
fn an_unsafe_blastit_bin_is_refused_with_the_fix() {
    let project = Project::new("explicit");
    let named = project.path("named/blastit");
    let marker = project.path("ran-named.txt");
    write_script(&named, &marker, "");
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

/// §4: a binary another uid owns is refused.
///
/// Building the fixture needs root (`chown` to another uid). The skip is
/// **loud** and, under `CI`, fatal: a gate must never report a pass for the one
/// assertion an unprivileged runner cannot make. Root is its real home.
#[test]
fn a_binary_owned_by_another_uid_is_refused() {
    if !is_root() {
        eprintln!(
            "SKIP a_binary_owned_by_another_uid_is_refused: creating another uid's file needs root"
        );
        assert!(
            std::env::var_os("CI").is_none(),
            "CI is set, so this safety rule must not go untested: run this suite as root \
             (or drop the rule from the gate's claims) — the fixture needs `chown` to another uid"
        );
        return;
    }
    let project = Project::new("uid");
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

/// §4: a symlink is followed only into a tree the rule accepts.
#[test]
fn a_symlink_inside_the_project_hands_off_and_one_into_an_unsafe_tree_does_not() {
    let project = Project::new("symlink");
    let real = project.path(".venv/lib/blastit");
    let good_marker = project.path("ran-good.txt");
    write_script(&real, &good_marker, "");
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
    write_script(&outside.join("blastit"), &bad_marker, "");
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

/// §4: an ELF candidate runs from the verified descriptor (`execveat`).
#[test]
fn an_elf_candidate_runs_from_the_verified_descriptor() {
    let project = Project::new("elf");
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

/// §4 step 6: a project file that is neither an ELF image nor a `#!` script gets
/// its own sentence — the one failure branch no other case reaches.
#[test]
fn a_file_that_is_neither_an_elf_image_nor_a_script_is_not_run() {
    let project = Project::new("not-a-program");
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

/// §4: the message names the word the user typed, so `blastit test` reports
/// `blastit test:` and not the `check` it aliases.
#[test]
fn the_message_names_the_command_the_user_typed() {
    let project = Project::new("test-alias");
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

/// `R6`–`R8`: the production shapes answer from this image, never the project's.
///
/// The flag list is only half of it: a future grammar that let a re-exec token
/// sit *after* a subcommand (`dev --stage=1`), or that accepted a subcommand
/// word for `Serve`, would re-open the bypass just as surely. Every candidate
/// vector below names a shape that must never hand off.
#[test]
fn the_production_shapes_never_hand_off() {
    let project = Project::new("serve");
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
        vec!["--internal-json"],
        // A re-exec token after a subcommand word.
        vec!["dev", "--stage=1"],
        vec!["test", "--stage=1"],
        vec!["serve", "dev"],
        vec!["--role=dbproxy", "dev"],
        vec!["--stage=1", "dev"],
        vec!["dev", "--role=dbproxy"],
        vec!["dev", "--preset=x"],
        vec!["--internal-json", "dev"],
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

/// §4: `BLASTIT_HANDOFF_DONE` and `BLASTIT_NO_LOCAL_HANDOFF` are the switches,
/// and only `""`/`"0"` leave the handoff on.
#[test]
fn the_two_switches_control_the_handoff() {
    let project = Project::new("switches");
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

/// §4 step 6: the exec never re-opens the candidate by path.
///
/// The one `unsafe` block in `handoff.rs` is read here, as `critical_lsp.rs`
/// reads `src/cli.rs` for its ordering promise: an ELF image is started from
/// `verified.bin`'s descriptor, and nothing inside that function resolves a
/// path again.
///
/// The assertion is on the **syscall number** (`libc::SYS_execveat`) and not on
/// the `libc::execveat` wrapper, which `libc` declares for glibc only: the
/// syscall form is the one that compiles on glibc *and* musl (`MB.42.10`), and
/// it is exactly as strong — `AT_EMPTY_PATH` on the verified descriptor.
#[test]
fn the_exec_never_reopens_the_verified_path() {
    let source = include_str!("../src/cli/handoff.rs");
    let start = source
        .find("fn exec_verified")
        .expect("the one exec function");
    let tail = &source[start..];
    let end = tail.find("\n}\n").expect("the end of exec_verified");
    let body = &tail[..end];
    assert_eq!(
        body.matches("libc::SYS_execveat").count(),
        1,
        "one descriptor-based exec, by syscall number (the form both libcs declare)"
    );
    assert!(
        body.contains("libc::syscall("),
        "execveat goes through `libc::syscall`: `libc::execveat` is gnu-only"
    );
    assert!(
        body.contains("AT_EMPTY_PATH"),
        "the exec starts the verified descriptor (`AT_EMPTY_PATH`), never a path"
    );
    assert_eq!(
        body.matches("unsafe {").count(),
        2,
        "one `unsafe` per branch: the ELF syscall and the `#!` execve"
    );
    assert!(
        body.contains("verified.bin.as_raw_fd()"),
        "execveat must name the verified descriptor"
    );
    for forbidden in [
        "canonicalize",
        "symlink_metadata",
        "open(",
        "openat(",
        "metadata(",
    ] {
        assert!(
            !body.contains(forbidden),
            "the exec must not touch the path again (`{forbidden}`)"
        );
    }
}

/// §4: an explicit `$BLASTIT_BIN` that **is** `/proc/self/exe` is not refused,
/// **and** the image that serves the run is the one that was started (`MB.49`).
///
/// `ci.yml`'s `standalone` job runs the binary as
/// `docker run -v "$PWD/target/debug/blastit:/usr/local/bin/blastit:ro" -e
/// BLASTIT_BIN=/usr/local/bin/blastit …`, so the explicitly named candidate has
/// the same `(st_dev, st_ino)` as `/proc/self/exe`. Before `MB.49` the ownership,
/// mode and tree checks ran first and refused it — an `exit 1` on the artifact
/// that was already running. The identity answer comes first now, and the
/// candidate is not `exec`ed either: there is nothing to hand off to.
///
/// The output alone cannot show that second half — a silent self-`execve` prints
/// exactly what a refusal-free run prints — so the case serves a real `dev` (a
/// static manifest, so the process outlives the handoff decision) and reads
/// `/proc/<pid>/exe`, the file the serving image was `exec`ed from. A handoff
/// `exec`s the candidate, and the kernel then reports the candidate's path.
#[test]
fn an_explicit_blastit_bin_that_is_this_image_is_not_refused() {
    let project = Project::new("self-explicit");
    let real = Path::new(env!("CARGO_BIN_EXE_blastit"));
    let dir = project.path("usr/local/bin");
    fs::create_dir_all(&dir).expect("create the candidate's directory");
    // The parent a real `docker run -v` mount has: `0777` under
    // `debian:trixie-slim`, which is exactly what the tree rule refuses.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777))
        .expect("make the candidate's parent world-writable");
    let named = dir.join("blastit");
    // The same inode as the running image, which is what `handoff` compares. The
    // copy is the `EXDEV` fallback only (the fixture and `target/` can sit on
    // different filesystems), and it is the file this run starts, so
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
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let pid = child.id();
    // The file the serving image was `exec`ed from. A handoff would have exec'd
    // the candidate, and the kernel reports the path the image was opened by —
    // measured under `MB.49` with a handoff to a *copy* of this binary:
    // `/proc/<pid>/exe` then read the candidate's path, not the copy's.
    //
    // `argv[0]` cannot serve here: `dev` re-execs itself (`--stage=1 …`) with
    // `/proc/self/exe` as `argv[0]`, so every `dev` image names that literal
    // string, whichever file it came from.
    let image = fs::read_link(format!("/proc/{pid}/exe")).ok();
    let _ = child.kill();
    let output = child.wait_with_output().expect("collect the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let started = fs::canonicalize(&runnable).ok();

    assert!(
        serving,
        "the started image must answer on 127.0.0.1:{port}: {stderr}"
    );
    if let Some(image) = image {
        assert_eq!(
            fs::canonicalize(&image).ok(),
            started,
            "the image that served the run is not the one that was started: \
             /proc/{pid}/exe names the candidate ({}) instead of {}\n{stderr}",
            named.display(),
            runnable.display()
        );
    }
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
