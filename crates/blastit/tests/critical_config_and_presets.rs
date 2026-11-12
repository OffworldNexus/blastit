#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Protected critical suite for `R1..R3`, `R6`, `R9..R10`, `R18` and `R23`:
//! the `0.1.1` CLI grammar, CWD-only manifest discovery, preset precedence and
//! the seccomp gate.
//!
//! Every test that reads or mutates the process environment, or changes the
//! working directory, holds the single [`ENV`] mutex through [`Sandbox`].
//! Subprocess checks run the real `blastit` binary so the exit code, stdout
//! purity and stderr text are the ones an operator sees.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use blastit::config::{Config, LogFormat, PresetPolicy, Warning};
use blastit_events::{Component, Event, EventKind, Lifecycle, decode_json, encode_json};

/// Serialises every test in this file that touches process-global state.
static ENV: Mutex<()> = Mutex::new(());

/// The environment variables this suite reads, writes or must clear.
const MANAGED: &[&str] = &[
    "BLASTIT_APP",
    "BLASTIT_PRELOAD",
    "BLASTIT_MANIFEST",
    "BLASTIT_PRESET",
    "BLASTIT_SECCOMP",
    "BLASTIT_LOG_FORMAT",
    "BLASTIT_CLIENT_IP_HEADER",
    "BLASTIT_TRUSTED_PROXIES",
    "BLASTIT_FAIR_QUEUING",
    "PORT",
];

/// A unique working directory plus a clean, restored environment.
struct Sandbox {
    dir: PathBuf,
    previous_cwd: PathBuf,
    saved: Vec<(&'static str, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let lock = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let saved = MANAGED
            .iter()
            .map(|name| (*name, std::env::var_os(name)))
            .collect();
        for name in MANAGED {
            // SAFETY: the suite's single mutex serialises every `set_var` and
            // `remove_var` in this test binary, and the child processes this
            // file starts get an explicit environment.
            unsafe { std::env::remove_var(name) };
        }
        let previous_cwd = std::env::current_dir().expect("the test starts in a directory");
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "blastit-ma232-{}-{tag}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("a fresh temporary directory");
        std::env::set_current_dir(&dir).expect("the sandbox becomes the working directory");
        Self {
            dir,
            previous_cwd,
            saved,
            _lock: lock,
        }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn enter(&self, path: &Path) {
        std::env::set_current_dir(path).expect("enter the sandbox subdirectory");
    }

    fn set_env(&self, name: &str, value: &str) {
        // SAFETY: see `Sandbox::new`.
        unsafe { std::env::set_var(name, value) };
    }

    fn remove_env(&self, name: &str) {
        // SAFETY: see `Sandbox::new`.
        unsafe { std::env::remove_var(name) };
    }

    fn write_manifest(&self, content: &str) {
        fs::write(self.dir.join("blastit.toml"), content).expect("write blastit.toml");
    }

    fn run_cli(&self, arguments: &[&str]) -> Output {
        self.run_cli_env(&[], arguments)
    }

    /// The same, with exactly `vars` in the child's environment.
    ///
    /// `env_clear` keeps the suite's own environment out of the child — every
    /// `BLASTIT_*` variable the tests set would otherwise leak into it — so a
    /// case that is *about* the environment passes what it needs explicitly.
    fn run_cli_env(&self, vars: &[(&str, &str)], arguments: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command.args(arguments).env_clear();
        for (name, value) in vars {
            command.env(name, value);
        }
        command
            .current_dir(&self.dir)
            .output()
            .expect("run the blastit binary")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.previous_cwd).expect("restore the working directory");
        for (name, value) in self.saved.drain(..) {
            // SAFETY: see `Sandbox::new`; the mutex is still held.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

const MANIFEST: &str = "\
version = 1
app = \"demo:application\"
preload = [\"one\", \"two\"]

[[route]]
name = \"home\"
path = [\"/\"]
";

#[test]
fn cwd_manifest_is_discovered_with_no_upward_search() {
    let sandbox = Sandbox::new("discover");
    sandbox.write_manifest(MANIFEST);

    let config = Config::from_env(None).expect("the CWD manifest is enough");
    assert_eq!(config.manifest, PathBuf::from("./blastit.toml"));
    assert_eq!(config.app.as_deref(), Some("demo:application"));
    assert_eq!(config.preload, ["one", "two"]);

    // A parent manifest is never consulted: the same file one directory up does
    // not make `./blastit.toml` exist in a child.
    let parent = sandbox.path().join("parent.toml");
    fs::write(&parent, MANIFEST).expect("write the parent manifest");
    let child = sandbox.path().join("child");
    fs::create_dir(&child).expect("create the child directory");
    sandbox.enter(&child);
    let config = Config::from_env(None).expect("a missing ./blastit.toml is not a parse error");
    assert_eq!(config.manifest, PathBuf::from("./blastit.toml"));
    assert_eq!(config.app, None, "no upward search reached the parent");
    assert!(config.preload.is_empty());
}

#[test]
fn a_set_but_empty_variable_wins_over_the_manifest() {
    let sandbox = Sandbox::new("empty");
    sandbox.write_manifest(MANIFEST);

    sandbox.set_env("BLASTIT_APP", "");
    let error = Config::from_env(None).expect_err("BLASTIT_APP=\"\" is not a callable");
    assert!(error.to_string().contains("BLASTIT_APP"), "{error}");
    assert!(error.to_string().contains("module:attribute"), "{error}");
    sandbox.remove_env("BLASTIT_APP");

    sandbox.set_env("BLASTIT_MANIFEST", "");
    let error = Config::from_env(None).expect_err("BLASTIT_MANIFEST=\"\" is not a path");
    assert!(error.to_string().contains("BLASTIT_MANIFEST"), "{error}");
    sandbox.remove_env("BLASTIT_MANIFEST");

    sandbox.set_env("BLASTIT_PRELOAD", "");
    let config = Config::from_env(None).expect("an empty preload is a value");
    assert!(
        config.preload.is_empty(),
        "BLASTIT_PRELOAD=\"\" clears the list"
    );
    assert_eq!(
        config.app.as_deref(),
        Some("demo:application"),
        "the manifest still supplies the app"
    );
    sandbox.remove_env("BLASTIT_PRELOAD");

    sandbox.set_env("BLASTIT_APP", "override:application");
    let config = Config::from_env(None).expect("a set app wins over the manifest");
    assert_eq!(config.app.as_deref(), Some("override:application"));
}

#[test]
fn both_presets_supply_their_four_values() {
    for (name, header) in [
        ("digitalocean-paas", "do-connecting-ip"),
        ("compose-traefik", "x-forwarded-for"),
    ] {
        let sandbox = Sandbox::new(name);
        sandbox.set_env("BLASTIT_APP", "demo:application");
        let config = Config::from_env(Some(name)).expect("a known preset");
        let choice = config.preset.as_ref().expect("the preset is recorded");
        assert_eq!(choice.name, name);
        assert_eq!(choice.supplied.len(), 4, "{name}: {choice:?}");
        assert!(choice.overridden.is_empty(), "{name}: {choice:?}");
        assert_eq!(config.log_format, LogFormat::Json);
        assert!(config.fair_queuing, "{name}");
        assert_eq!(config.client_ip_header, header, "{name}");
        assert_eq!(config.trusted_proxies, ["*"], "{name}");
    }
}

#[test]
fn a_set_variable_overrides_a_preset_even_when_empty() {
    let sandbox = Sandbox::new("override");
    sandbox.set_env("BLASTIT_APP", "demo:application");
    sandbox.set_env("BLASTIT_PRESET", "compose-traefik");
    sandbox.set_env("BLASTIT_CLIENT_IP_HEADER", "x-real-ip");
    sandbox.set_env("BLASTIT_TRUSTED_PROXIES", "");
    sandbox.set_env("BLASTIT_FAIR_QUEUING", "off");
    sandbox.set_env("BLASTIT_LOG_FORMAT", "pretty");

    let config = Config::from_env(Some("digitalocean-paas")).expect("the flag wins");
    let choice = config.preset.as_ref().expect("a preset is recorded");
    assert_eq!(choice.name, "digitalocean-paas");
    assert!(choice.supplied.is_empty(), "{choice:?}");
    for name in [
        "BLASTIT_CLIENT_IP_HEADER",
        "BLASTIT_TRUSTED_PROXIES",
        "BLASTIT_FAIR_QUEUING",
        "BLASTIT_LOG_FORMAT",
    ] {
        assert!(
            choice
                .overridden
                .iter()
                .any(|overridden| overridden == name),
            "{name} is overridden: {choice:?}"
        );
    }
    assert_eq!(choice.overridden.len(), 4, "{choice:?}");
    assert_eq!(config.log_format, LogFormat::Pretty);
    assert!(!config.fair_queuing);
    assert_eq!(config.client_ip_header, "x-real-ip");
    assert!(config.trusted_proxies.is_empty(), "an empty list wins");
    assert!(config.warnings.contains(&Warning::PresetFlagWins));
}

#[test]
fn log_format_defaults_to_json_for_serve_and_pretty_for_dev_and_check() {
    let sandbox = Sandbox::new("log-format");
    sandbox.set_env("BLASTIT_APP", "demo:application");

    assert_eq!(
        Config::from_env(None).expect("serve").log_format,
        LogFormat::Json
    );
    assert_eq!(
        Config::from_env_with_format(None, LogFormat::Pretty, PresetPolicy::Env)
            .expect("dev or check")
            .log_format,
        LogFormat::Pretty
    );

    sandbox.set_env("BLASTIT_LOG_FORMAT", "json");
    assert_eq!(
        Config::from_env_with_format(None, LogFormat::Pretty, PresetPolicy::Env)
            .expect("an explicit variable wins")
            .log_format,
        LogFormat::Json
    );
}

#[test]
fn dev_and_check_ignore_an_exported_preset() {
    let sandbox = Sandbox::new("dev-preset");
    sandbox.set_env("BLASTIT_APP", "demo:application");
    // A developer's shell — or a CI image built for a deployment — exports a
    // production preset. `blastit dev` and `blastit check` are documented to
    // ignore it (R2, R3, ADR-0005): it may not replace their `pretty` default
    // with the preset's `json`, and it may not turn the emergency
    // `BLASTIT_SECCOMP=0` into the production abort (R18).
    sandbox.set_env("BLASTIT_PRESET", "compose-traefik");
    sandbox.set_env("BLASTIT_SECCOMP", "0");

    for config in [Config::for_dev(), Config::for_check()] {
        let config = config.expect("an exported preset must not reach dev or check");
        assert_eq!(config.preset, None, "no preset is in force");
        assert_eq!(
            config.log_format,
            LogFormat::Pretty,
            "the preset's json must not replace dev's and check's default"
        );
        assert!(!config.seccomp);
        assert!(config.warnings.contains(&Warning::SeccompDisabled));
        assert!(
            !config.warnings.contains(&Warning::PresetFlagWins),
            "nothing read the variable, so nothing lost to a flag"
        );
    }

    // The same environment through production *is* the documented abort: the
    // policy, not the environment, is what makes the difference.
    let error = Config::from_env(None).expect_err("a preset may never run without seccomp");
    let message = error.to_string();
    assert!(message.contains("BLASTIT_SECCOMP"), "{message}");
    assert!(message.contains("compose-traefik"), "{message}");

    // "Never read" is literal: a preset name this build does not know is not
    // dev's or check's error to report, while production refuses it by name.
    sandbox.set_env("BLASTIT_PRESET", "compose-trafik");
    sandbox.remove_env("BLASTIT_SECCOMP");
    for config in [Config::for_dev(), Config::for_check()] {
        let config = config.expect("an unknown preset in the shell is not dev's error");
        assert_eq!(config.preset, None);
        assert_eq!(config.log_format, LogFormat::Pretty);
    }
    let error = Config::from_env(None).expect_err("production reads the variable");
    assert!(error.to_string().contains("BLASTIT_PRESET"), "{error}");
    assert!(error.to_string().contains("compose-trafik"), "{error}");
}

#[test]
fn seccomp_zero_aborts_with_a_preset_and_warns_without_one() {
    let sandbox = Sandbox::new("seccomp");
    sandbox.set_env("BLASTIT_APP", "demo:application");
    sandbox.set_env("BLASTIT_SECCOMP", "0");

    let error = Config::from_env(Some("compose-traefik"))
        .expect_err("a preset may never run without seccomp");
    let message = error.to_string();
    assert!(message.contains("BLASTIT_SECCOMP"), "{message}");
    assert!(message.contains("preset"), "{message}");

    let config = Config::from_env(None).expect("outside a preset the value is honoured");
    assert!(!config.seccomp);
    assert!(config.warnings.contains(&Warning::SeccompDisabled));
}

#[test]
fn a_boot_event_round_trips_the_build_and_never_a_port() {
    use blastit::config::{Mode, Privilege};

    let build = env!("CARGO_PKG_VERSION");
    let privilege = Privilege {
        mode: Mode::Root,
        capabilities: Vec::new(),
        bounding_set_dropped: true,
    };
    // The real stage 1 path: `preload_lifecycle` applies `Privilege::lifecycle`
    // to a lifecycle whose stage is `preload`, and `R23`'s build rides along.
    let preload = privilege.lifecycle(Lifecycle::default().with_stage("preload"));
    assert_eq!(
        preload.build(),
        Some(build),
        "the preload boot carries the build"
    );
    assert_eq!(
        privilege
            .lifecycle(Lifecycle::default().with_stage("stage0"))
            .build(),
        None,
        "stage0's boot does not"
    );

    let boot = Event::new(Component::Init, EventKind::Boot(preload));
    let line = encode_json(&boot);
    assert!(line.contains(&format!(r#""build":"{build}""#)), "{line}");
    let decoded = decode_json(&line).expect("the encoder's own line decodes");
    let EventKind::Boot(lifecycle) = decoded.kind() else {
        panic!("a boot line decodes as a boot: {line}");
    };
    assert_eq!(lifecycle.build(), Some(build));

    for event in [
        Event::new(
            Component::Init,
            EventKind::Boot(Lifecycle::default().with_stage("stage0")),
        ),
        Event::new(
            Component::Init,
            EventKind::Ready(Lifecycle::default().with_stage("serving")),
        ),
        Event::new(
            Component::Init,
            EventKind::Shutdown(Lifecycle::default().with_stage("drain")),
        ),
    ] {
        let line = encode_json(&event);
        assert!(!line.contains(r#""build""#), "{line}");
        assert!(!line.contains(r#""port""#), "{line}");
    }

    let dev_boot = Event::new(
        Component::Init,
        EventKind::Boot(Lifecycle::default().with_privilege("unprivileged-dev")),
    );
    assert!(
        encode_json(&dev_boot).contains(r#""privilege":"unprivileged-dev""#),
        "dev boots name their privilege"
    );
}

/// One rejected command line: the exit code, the stderr text it must name, and
/// the empty stdout every CLI failure promises.
struct Rejection {
    argv: &'static [&'static str],
    code: i32,
    stderr: &'static str,
}

#[test]
fn cli_matrix_rejects_each_bad_command_line() {
    let sandbox = Sandbox::new("matrix");
    let cases = [
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
    ];
    for case in cases {
        let output = sandbox.run_cli(case.argv);
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

#[test]
fn an_environment_driven_abort_is_a_real_exit_code_and_names_its_cause() {
    let sandbox = Sandbox::new("process-env");

    // `dev`'s port is `PORT` when `--port` is absent (R2). A value that is not
    // a port is the same exit `1` an operator's shell produces — the real
    // process, not an in-process error — and the cause is named on stderr.
    let output = sandbox.run_cli_env(&[("PORT", "70000")], &["dev", "--learn"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "a CLI failure writes nothing to stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("PORT"), "{stderr}");
    assert!(
        stderr.contains("70000"),
        "the error names the value: {stderr}"
    );

    // `BLASTIT_SECCOMP=0` with a preset must never reach a running server
    // (R18, ADR-0005). The real binary exits `1`; when its boot can hold init's
    // capabilities it reaches the configuration and says which variable it
    // refused (the exact text is asserted in-process by
    // `seccomp_zero_aborts_with_a_preset_and_warns_without_one`, which is the
    // only place an unprivileged suite can force stage 0 to read a
    // configuration at all — before that it refuses on the capability sets).
    let output = sandbox.run_cli_env(
        &[
            ("BLASTIT_PRESET", "compose-traefik"),
            ("BLASTIT_SECCOMP", "0"),
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains(r#""event":"ready""#),
        "the boot aborts before serving: {combined}"
    );
    if combined.contains(r#""stage":"config""#) {
        assert!(
            combined.contains("BLASTIT_SECCOMP"),
            "the abort names the gate: {combined}"
        );
    }
}

#[test]
fn top_level_help_lists_the_three_commands_and_the_presets() {
    let sandbox = Sandbox::new("help");
    let output = sandbox.run_cli(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in ["blastit dev", "blastit check", "blastit [--preset NAME]"] {
        assert!(stdout.contains(expected), "{stdout}");
    }
    for preset in ["digitalocean-paas", "compose-traefik"] {
        assert!(stdout.contains(preset), "{stdout}");
    }
    assert!(!stdout.contains("--version"), "{stdout}");
    assert!(output.stderr.is_empty(), "{output:?}");

    let version = sandbox.run_cli(&["--version"]);
    assert_eq!(version.status.code(), Some(1));
    assert!(version.stdout.is_empty());
    assert!(!version.stderr.is_empty());

    for (argv, expected) in [
        (&["dev", "--help"][..], "--port"),
        (&["check", "--help"][..], "--dry-run"),
        (&["test", "-h"][..], "--json"),
    ] {
        let output = sandbox.run_cli(argv);
        assert_eq!(output.status.code(), Some(0), "{argv:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(expected), "{argv:?}: {stdout}");
    }
}
