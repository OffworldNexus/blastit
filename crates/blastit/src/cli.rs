//! The command line: which role to play, and in which installation (§2.1, §4).
//!
//! One program plays every role (§2), and the role is chosen by argv:
//!
//! ```text
//! blastit                                     stage 0: root, real environment
//! blastit --stage=1 --config-fd=N …           stage 1: root, app environment
//! blastit --role=dbproxy --secrets-fd=N …     dbproxy: uid B+3, empty environment
//! ```
//!
//! `--role=dbproxy` grants nothing by itself: the secrets arrive over the pipe
//! the flag names, and a process started by hand has no such pipe (§2). http,
//! the scheduler and every view are chosen by their *position* in the process
//! tree instead, and so have no flag at all.
//!
//! Nothing here reads the environment or a file on the command line's behalf:
//! stage 0 does that (§4). The two exceptions are documented where they are:
//! `PORT` (the documented `blastit dev` fallback) and `BLASTIT_PRESET` (which the
//! same boot reads, checked here so a typo is named before the capability gate).
//!
//! # The four modules
//!
//! * [`help`] — every word a person reads, and the one shape a refusal takes.
//! * [`parse`] — the grammar of the four subcommands and the re-exec lines.
//! * [`preset`] — the one resolver for `--preset` and `BLASTIT_PRESET`.
//! * [`run`] — where a validated command line is handed to the stage that runs
//!   it.
//!
//! # Library and binary
//!
//! This module is the whole binary. `crates/blastit`'s `main` calls it with a
//! [`Host`] that re-executes `/proc/self/exe` (§4), and `blastit._blastit` — the
//! extension a wheel installs — calls it with a host that re-executes the
//! interpreter it is running in (D2). Both play exactly the same roles; only
//! the two `execve`s of §4 differ.

use std::net::IpAddr;
use std::os::fd::RawFd;

use crate::host::Host;
use crate::logs::Colour;
use crate::py::Python;
use crate::stage0;
use crate::stage1;

mod help;
mod parse;
mod preset;
mod run;

use help::{help_for, refuse, usage};
use parse::{Subcommand, parse, parse_check, parse_dev, parse_fmt, parse_lsp};
use preset::env_is_known;
use run::{role_is_allowed, run_check, run_dev, run_fmt};

/// The four command shapes `0.2.0` exposes (§1.1).
///
/// A subcommand is the *first* argument; everything else is the unchanged
/// stage-0/stage-1/`--role` grammar the process tree uses internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Production `blastit`: fail-closed, presets, `--stage=1`/`--role=*`.
    Serve,
    /// `blastit dev`: the unprivileged single-UID simulation (`R2`).
    Dev,
    /// `blastit check` and its exact alias `blastit test` (`R3`).
    Check,
    /// `blastit fmt`: rewrite a `Blastfile` in the canonical layout (`R19`).
    Fmt,
    /// `blastit lsp`: speak LSP on stdin/stdout for an editor (`MB.22.3`).
    Lsp,
}

impl Command {
    /// The subcommand `argv` names, before any flags are parsed.
    pub fn from_argv(argv: &[String]) -> Self {
        match argv.first().map(String::as_str) {
            Some("dev") => Self::Dev,
            Some("check" | "test") => Self::Check,
            Some("fmt") => Self::Fmt,
            Some("lsp") => Self::Lsp,
            _ => Self::Serve,
        }
    }
}

/// `blastit dev`'s accepted arguments, validated (`R2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevOptions {
    /// `--port`, or `PORT`, or `8080`; always `1..=65535`.
    pub port: u16,
    /// `--host`, or `127.0.0.1`.
    pub host: IpAddr,
    /// `--learn`: observe statements instead of refusing them.
    pub learn: bool,
    /// `--no-reload`: turn live reload off — nothing is watched, and neither
    /// reload event is printed (`docs/reference/cli.md` `## blastit dev`).
    ///
    /// Reload is on by default, so this is the one user-facing switch and its
    /// value is `false` unless the flag is given.
    pub no_reload: bool,
}

/// `blastit check`'s accepted arguments, validated (`R3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOptions {
    /// `--json`: at most one JSON document on stdout.
    pub json: bool,
    /// `--learn`: propose routes and queries from the run.
    pub learn: bool,
    /// `--dry-run`, only with `--learn`.
    pub dry_run: bool,
    /// Everything after `--`, passed to `pytest -q` verbatim.
    pub pytest_args: Vec<String>,
}

/// `blastit fmt`'s accepted arguments, validated (`R19`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmtOptions {
    /// `--check`: change nothing, and report a file that is not canonical.
    pub check: bool,
    /// The file to format; `None` means the discovered default (`./Blastfile`
    /// unless the manifest variable names another).
    pub path: Option<std::path::PathBuf>,
}

/// The internal grammar the process tree uses to start its own images.
#[derive(Debug, Default)]
struct Args {
    stage: Option<u32>,
    role: Option<String>,
    config_fd: Option<RawFd>,
    dbproxy_pid: Option<i32>,
    dbproxy_events_fd: Option<RawFd>,
    dbproxy_stdout_fd: Option<RawFd>,
    dbproxy_stderr_fd: Option<RawFd>,
    secrets_fd: Option<RawFd>,
    events_fd: Option<RawFd>,
    ready_fd: Option<RawFd>,
    /// `--seccomp=0|1`: §7.3's filters, for the one role that is `execve`'d and
    /// so cannot read `BLASTIT_SECCOMP` from the environment (§4).
    seccomp: Option<bool>,
    /// `--preset=NAME`: the §8.2 defaults to start from (E1).
    preset: Option<String>,
    /// `--dev-root=DIR`: the ephemeral `0700` root `blastit dev` created
    /// (`R20`). Internal to the dev supervisor's `execve` of stage 1.
    dev_root: Option<std::path::PathBuf>,
    /// `--dev-host=ADDR`: the address stage 1 binds for `blastit dev` (`R22`).
    dev_host: Option<IpAddr>,
    /// `--dev-obs-fd=N`: stage 1's end of dbproxy's `--learn` observation pipe
    /// (`R29`).
    dev_obs_fd: Option<RawFd>,
    /// `--dev-ctl-fd=N`: the `--learn` attribution socket's stage 1 end.
    dev_ctl_fd: Option<RawFd>,
    /// `--dev-reload=on|off`: whether stage 1 constructs the live-reload
    /// watcher (`ADR-0014` §4.1). Internal: stage 0 forwards the user's
    /// `--no-reload` here, and an absent token means `on` (the default).
    dev_reload: Option<bool>,
    /// `--dev-adopt-fd=N`: the read end of a reload cycle's adoption document
    /// (§4.6 step 6). Internal: only a replacement stage-1 image is started
    /// with it, and only a `--stage=1 --dev-root` boot accepts it.
    dev_adopt_fd: Option<RawFd>,
    /// `--dev-policy-fd=N`: this image's end of the dev-only dbproxy policy
    /// socket. Internal, like the rest of the `--dev-*` family.
    dev_policy_fd: Option<RawFd>,
    /// `--dev-reload-since=NANOS`: the instant the cycle that started this
    /// image began, for its one-shot post-boot reconcile.
    dev_reload_since: Option<u128>,
}

/// Runs the role the command line names, in the installation `host` describes.
///
/// `py` is the interpreter the caller is running in, when there is one:
/// `blastit._blastit.main()` is called *from* Python, so a wheel install always
/// has it and stage 1 uses that interpreter as it stands (§9.3). The standalone
/// binary passes `None` and embeds CPython itself.
///
/// Returns the process exit status, which is also what a Python console script
/// raises out of `main()` — the caller turns it into `ExitCode::from` or
/// `SystemExit`. Stage 0 and stage 1 never return at all.
pub fn main(host: Host, argv: Vec<String>, py: Option<Python<'_>>) -> u8 {
    // `A-4`: the terminal is asked once, here — the composition root every
    // image of this program starts at — and the answer travels as a value.
    // Nothing below this line reads `NO_COLOR`, `TERM` or `isatty` again.
    let colour = crate::term::colour();
    // Normative dispatch order (§4.2 E.1): a subcommand is the first argument,
    // so `dev` is checked before `check` before the stage-0/stage-1/`--role`
    // grammar. A stage-0 argument named `dev` is not a thing: the command is the
    // first word.
    match Command::from_argv(&argv) {
        Command::Dev => match parse_dev(&argv[1..]) {
            Ok(Subcommand::Help) => {
                println!("{}", help::DEV_USAGE);
                SUCCESS
            }
            Ok(Subcommand::Run(options)) => run_dev(&host, &options, py.is_some(), colour),
            Err(message) => {
                refuse("blastit dev", &message, false, colour);
                // `1` for `dev`'s own grammar: the protected CLI matrix pins
                // `critical_config_and_presets.rs:426-445` at `1`.
                FAILURE
            }
        },
        Command::Check => match parse_check(&argv[1..]) {
            Ok(Subcommand::Help) => {
                println!("{}", help::CHECK_USAGE);
                SUCCESS
            }
            Ok(Subcommand::Run(options)) => run_check(&host, &options),
            Err(message) => {
                refuse("blastit check", &message, false, colour);
                CHECK_FAILURE
            }
        },
        Command::Fmt => match parse_fmt(&argv[1..]) {
            Ok(Subcommand::Help) => {
                println!("{}", help::fmt_usage());
                SUCCESS
            }
            Ok(Subcommand::Run(options)) => run_fmt(&options),
            Err(message) => {
                refuse("blastit fmt", &message, false, colour);
                FMT_FAILURE
            }
        },
        Command::Lsp => match parse_lsp(&argv[1..]) {
            Ok(Subcommand::Help) => {
                println!("{}", help::LSP_USAGE);
                SUCCESS
            }
            Ok(Subcommand::Run(())) => run_lsp(),
            Err(message) => {
                refuse("blastit lsp", &message, false, colour);
                LSP_FAILURE
            }
        },
        Command::Serve => {
            // E1: the usage text lists the presets with their one-line
            // descriptions, which is the only way to find out what `--preset`
            // accepts, and the commands the binary has. It goes to stdout, so
            // its headers take stdout's answer.
            if argv
                .iter()
                .any(|argument| argument == "--help" || argument == "-h")
            {
                println!("{}", usage(colour.stdout));
                return SUCCESS;
            }
            // `A-8` / `A-14`: the version answers only a command line that
            // carries nothing else, so `blastit --preset nope -V` still names
            // the typo (`2`) instead of printing a version and exiting `0`.
            if argv.len() == 1 && matches!(argv[0].as_str(), "--version" | "-V") {
                println!("blastit {}", env!("CARGO_PKG_VERSION"));
                return SUCCESS;
            }
            // `blastit help [COMMAND]`: the word people type, answered with the
            // same text `--help` prints (`A-14`).
            if argv.first().map(String::as_str) == Some("help") {
                return match help_for(argv.get(1).map(String::as_str), colour.stdout) {
                    Ok(text) => {
                        println!("{text}");
                        SUCCESS
                    }
                    Err(message) => {
                        refuse("blastit", &message, false, colour);
                        USAGE_FAILURE
                    }
                };
            }
            match parse(&argv) {
                Ok(args) => {
                    // E1 (`A-11`): `BLASTIT_PRESET` is checked here, before
                    // stage 0 reads capabilities, so a misspelled name is named
                    // for what it is instead of being drowned by a refusal to
                    // start. The flag's own name was validated by `parse`.
                    if let Err(message) = env_is_known(&args) {
                        refuse("blastit", &message, true, colour);
                        return USAGE_FAILURE;
                    }
                    dispatch(args, &host, py, colour)
                }
                Err(refusal) => {
                    refuse("blastit", &refusal.message, true, colour);
                    refusal.exit
                }
            }
        }
    }
}

/// The exit status of everything that refuses to start (§4).
const FAILURE: u8 = 1;

/// The exit status of `--help`, which is not a failure.
const SUCCESS: u8 = 0;

/// The exit status of a command line this build cannot act on (`A-12`).
///
/// `2` is what every subcommand uses for a bad flag, an unknown preset or an
/// unknown subcommand, and the stage-0 command line now answers the same way:
/// one mistake in argv is one exit status, whichever command caught it.
const USAGE_FAILURE: u8 = 2;

/// `blastit check`'s CLI failure, distinct from a finding (`1`) and pytest
/// (`3`) (`R4`).
const CHECK_FAILURE: u8 = 2;

/// `blastit fmt`'s "could not read, parse or write" failure (`R19`).
const FMT_FAILURE: u8 = 2;

/// `blastit fmt --check`'s "this file is not in canonical layout" (`R19`).
const FMT_NOT_CANONICAL: u8 = 1;

/// `blastit lsp`'s command-line failure: an argument it does not take is a CLI
/// error, not a session (`docs/design/blastfile-lsp-zed.md` §2.4).
const LSP_FAILURE: u8 = 2;

/// The `blastit lsp` session (`MB.22.3`).
///
/// The capability check comes **first**, before a byte of the editor's input is
/// read: a binary carrying `CAP_*` would grant them to this process image, and a
/// `Blastfile` is policy, not input, so an editor's text tool holds nothing.
/// `hold_none` is the check, not the drop — a plain install has nothing to drop.
///
/// It stays in this file on purpose: `critical_lsp.rs` reads `src/cli.rs` and
/// asserts that the check precedes the session, which is the invariant that
/// makes an editor's tool harmless even if a future refactor adds a capability.
///
/// # Returns
///
/// [`crate::lsp::run`]'s exit status: `0` only after the client said `shutdown`,
/// `1` when it disappeared without one.
fn run_lsp() -> u8 {
    if blastit_sys::caps::hold_none().is_err() {
        // §11.3's fourth fixed sentence: a failure here is not the client's
        // fault, and the error's own words name capabilities of the process,
        // never a document. It goes to stderr, because stdout is the protocol.
        eprintln!("blastit lsp: internal error");
        return FAILURE;
    }
    crate::lsp::run()
}

/// Runs the role the arguments name.
fn dispatch(args: Args, host: &Host, py: Option<Python<'_>>, colour: Colour) -> u8 {
    if let Some(role) = args.role.as_deref() {
        if let Err(message) = role_is_allowed(role, py.is_some()) {
            eprintln!("blastit: {message}");
            return FAILURE;
        }
        return match role {
            "dbproxy" => {
                let (Some(secrets_fd), Some(events_fd), Some(ready_fd)) =
                    (args.secrets_fd, args.events_fd, args.ready_fd)
                else {
                    eprintln!(
                        "blastit: --role=dbproxy needs --secrets-fd, --events-fd and --ready-fd"
                    );
                    return FAILURE;
                };
                // D6: this process is this file `execve`d again, and a binary
                // with file capabilities is granted them *by the execve* —
                // `no_new_privs`, which the child set before exec'ing, is what
                // normally stops that, but the invariant belongs to this
                // process rather than to the kernel's promise. dbproxy holds the
                // real credentials (§2.2): it may not hold a capability.
                // `hold_none` is the check, not the drop: a wheel's dbproxy
                // starts with nothing to drop.
                if let Err(err) = blastit_sys::caps::hold_none() {
                    eprintln!("blastit: dbproxy: {err}");
                    return FAILURE;
                }
                // Absent means on: `BLASTIT_SECCOMP` defaults to on (§8.2), and
                // a dbproxy started by hand is no reason to run unfiltered.
                let seccomp = args.seccomp.unwrap_or(true);
                let code = crate::roles::dbproxy(secrets_fd, events_fd, ready_fd, seccomp);
                u8::try_from(code).unwrap_or(FAILURE)
            }
            other => {
                eprintln!("blastit: unknown role `{other}`");
                FAILURE
            }
        };
    }

    match args.stage {
        None | Some(0) => stage0::run(host, py.is_some(), args.preset.as_deref(), colour),
        Some(1) => {
            let Some(config_fd) = args.config_fd else {
                eprintln!("blastit: --stage=1 needs --config-fd");
                return FAILURE;
            };
            let interpreter = match py {
                Some(py) => stage1::Interpreter::Hosted(py),
                None => stage1::Interpreter::Embed,
            };
            stage1::run(
                stage1::Options {
                    config_fd,
                    dbproxy_pid: args.dbproxy_pid,
                    dbproxy_events_fd: args.dbproxy_events_fd,
                    dbproxy_stdout_fd: args.dbproxy_stdout_fd,
                    dbproxy_stderr_fd: args.dbproxy_stderr_fd,
                    dev_root: args.dev_root,
                    dev_host: args.dev_host,
                    dev_obs_fd: args.dev_obs_fd,
                    dev_ctl_fd: args.dev_ctl_fd,
                    dev_reload: args.dev_reload.unwrap_or(true),
                    dev_adopt_fd: args.dev_adopt_fd,
                    dev_policy_fd: args.dev_policy_fd,
                    dev_reload_since: args.dev_reload_since,
                },
                interpreter,
                colour,
            )
        }
        Some(other) => {
            eprintln!("blastit: unknown stage `{other}`");
            FAILURE
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::help::{help_for, usage};
    use super::parse::{Subcommand, parse, parse_check, parse_dev};
    use super::run::role_is_allowed;
    use super::*;

    #[test]
    fn no_arguments_is_stage_zero() {
        let args = parse(&[]).unwrap();
        assert!(args.stage.is_none());
        assert!(args.role.is_none());
    }

    #[test]
    fn the_dbproxy_command_line_parses() {
        let args = parse(&[
            "--role=dbproxy".to_string(),
            "--secrets-fd=3".to_string(),
            "--events-fd=4".to_string(),
            "--ready-fd=5".to_string(),
            "--seccomp=0".to_string(),
        ])
        .unwrap();
        assert_eq!(args.role.as_deref(), Some("dbproxy"));
        assert_eq!(args.secrets_fd, Some(3));
        assert_eq!(args.events_fd, Some(4));
        assert_eq!(args.ready_fd, Some(5));
        assert_eq!(args.seccomp, Some(false));
    }

    #[test]
    fn the_stage_one_command_line_parses() {
        let args = parse(&[
            "--stage=1".to_string(),
            "--config-fd=7".to_string(),
            "--dbproxy-pid=12".to_string(),
            "--dbproxy-events-fd=8".to_string(),
            "--dbproxy-stdout-fd=9".to_string(),
            "--dbproxy-stderr-fd=10".to_string(),
        ])
        .unwrap();
        assert_eq!(args.stage, Some(1));
        assert_eq!(args.config_fd, Some(7));
        assert_eq!(args.dbproxy_pid, Some(12));
        assert_eq!(args.dbproxy_stderr_fd, Some(10));

        // Internal `--dev-*` flags belong to `--stage=1` with `--dev-root`.
        let dev_stage1 = parse(&[
            "--stage=1".to_string(),
            "--config-fd=7".to_string(),
            "--dev-root=/tmp/blastit-dev-test".to_string(),
            "--dev-host=127.0.0.1".to_string(),
            "--dev-obs-fd=11".to_string(),
            "--dev-ctl-fd=12".to_string(),
            "--dev-reload=off".to_string(),
            "--dev-adopt-fd=13".to_string(),
            "--dev-policy-fd=14".to_string(),
            "--dev-reload-since=123456789".to_string(),
        ])
        .unwrap();
        assert_eq!(
            dev_stage1.dev_root.as_deref(),
            Some(std::path::Path::new("/tmp/blastit-dev-test"))
        );
        assert_eq!(dev_stage1.dev_obs_fd, Some(11));
        assert_eq!(dev_stage1.dev_ctl_fd, Some(12));
        assert_eq!(
            dev_stage1.dev_reload,
            Some(false),
            "`--no-reload` travels to stage 1 as `--dev-reload=off`"
        );
        assert_eq!(dev_stage1.dev_adopt_fd, Some(13));
        assert_eq!(dev_stage1.dev_policy_fd, Some(14));
        assert_eq!(dev_stage1.dev_reload_since, Some(123_456_789));
        assert!(
            parse(&[
                "--stage=1".to_string(),
                "--config-fd=7".to_string(),
                "--dev-root=/tmp/blastit-dev-test".to_string(),
                "--dev-reload=maybe".to_string(),
            ])
            .is_err(),
            "the token takes `on` or `off`, nothing else"
        );
        assert!(parse(&["--dev-root=/tmp/x".to_string()]).is_err());
        // The cycle's tokens are stage-1-only too, and each needs `--dev-root`.
        for token in [
            "--dev-adopt-fd=13",
            "--dev-policy-fd=14",
            "--dev-reload-since=1",
        ] {
            assert!(
                parse(&[
                    "--stage=1".to_string(),
                    "--config-fd=7".to_string(),
                    token.to_string()
                ])
                .is_err(),
                "`{token}` without `--dev-root` is refused"
            );
            assert!(
                parse(&[token.to_string()]).is_err(),
                "`{token}` outside `--stage=1` is refused"
            );
        }
        assert!(
            parse(&[
                "--stage=1".to_string(),
                "--config-fd=7".to_string(),
                "--dev-obs-fd=11".to_string(),
            ])
            .is_err()
        );
    }

    #[test]
    fn an_unknown_argument_is_refused() {
        assert!(parse(&["--wat=1".to_string()]).is_err());
        assert!(parse(&["--stage".to_string()]).is_err());
        assert!(parse(&["--stage=x".to_string()]).is_err());
        assert!(parse(&["--config-fd=-1".to_string()]).is_err());
        assert!(parse(&["--seccomp=2".to_string()]).is_err());
        assert!(parse(&["--seccomp=yes".to_string()]).is_err());
        // The flag belongs to one role only: everything else reads
        // `BLASTIT_SECCOMP` from its own environment.
        assert!(parse(&["--seccomp=1".to_string()]).is_err());
        assert!(parse(&["--stage=1".to_string(), "--seccomp=0".to_string()]).is_err());
        assert!(
            parse(&[
                "--role=dbproxy".to_string(),
                "--secrets-fd=3".to_string(),
                "--events-fd=4".to_string(),
                "--ready-fd=5".to_string(),
                "--seccomp=1".to_string(),
            ])
            .is_ok()
        );
    }

    #[test]
    fn a_value_this_build_cannot_read_is_a_usage_error() {
        // `A-12`: `--stage=x`, `--seccomp=maybe` and a `--dev-*` token outside
        // its stage are mistakes in what somebody typed, so they exit `2` like
        // every other unknown argument — not `1`, which is reserved for the
        // re-exec lines the protected CLI matrix pins.
        for argv in [
            &["--stage=x"][..],
            &["--seccomp=maybe"][..],
            &["--stage=1", "--seccomp=maybe"][..],
            &["--dev-root=/tmp/x"][..],
        ] {
            let argv: Vec<String> = argv.iter().map(|argument| argument.to_string()).collect();
            let refusal = parse(&argv).expect_err("refused");
            assert_eq!(refusal.exit, USAGE_FAILURE, "{argv:?}: {refusal:?}");
        }
        // The genuine re-exec lines keep `1`.
        for argv in [
            &["--stage=1", "--seccomp=0"][..],
            &["--stage=1", "--config-fd=7", "--dev-obs-fd=11"][..],
        ] {
            let argv: Vec<String> = argv.iter().map(|argument| argument.to_string()).collect();
            let refusal = parse(&argv).expect_err("refused");
            assert_eq!(refusal.exit, FAILURE, "{argv:?}: {refusal:?}");
        }
    }

    #[test]
    fn the_dev_only_port_flag_is_answered_with_the_two_places_it_belongs() {
        // The command an owner typed (`MB.17`) must name both the command that
        // has the flag and the setting production reads, never "not
        // `--key=value`", which answers a question nobody asked.
        for argument in ["--port", "--port=4242", "-p", "-p=4242"] {
            let refusal = parse(&[argument.to_string()]).expect_err(argument);
            assert_eq!(refusal.exit, USAGE_FAILURE, "{argument}");
            let message = refusal.message;
            assert_eq!(message, parse::PORT_IS_DEV_ONLY, "{argument}");
            assert!(message.contains("blastit dev"), "{message}");
            assert!(message.contains("PORT="), "{message}");
        }
        // A genuine key/value mistake keeps its own sentence — it is a
        // different bug — but names the help that lists the flag (`A-14`).
        let bare = parse(&["--stage".to_string()]).expect_err("a bare --stage");
        assert_eq!(
            bare.message,
            "unknown argument `--stage` — run 'blastit --help'"
        );
        // A word where a flag belongs is a command this build does not have,
        // and says so instead of asking for `--key=value` (`A-14`).
        let word = parse(&["serve".to_string()]).expect_err("a positional");
        assert_eq!(
            word.message,
            "unknown subcommand 'serve' — run 'blastit --help'"
        );
        assert_eq!(word.exit, USAGE_FAILURE);
    }

    #[test]
    fn a_stage_zero_preset_typo_is_a_usage_error_naming_the_presets() {
        // `A-11`: the flagship case — a typo used to be drowned by ten JSON
        // lines about `cap_chown`. `parse` runs before stage 0, so the sentence
        // comes first, and one resolver owns it.
        for argv in [
            &["--preset", "nope"][..],
            &["--preset=nope"][..],
            &["--preset", " wrong "][..],
        ] {
            let argv: Vec<String> = argv.iter().map(|argument| argument.to_string()).collect();
            let refusal = parse(&argv).expect_err("a preset typo");
            assert_eq!(refusal.exit, USAGE_FAILURE, "{argv:?}");
            assert!(refusal.message.contains("unknown preset"), "{refusal:?}");
            assert!(refusal.message.contains("digitalocean-paas"), "{refusal:?}");
        }
        // The documented spellings are accepted, and whitespace is the
        // operator's.
        for argv in [
            &["--preset", "compose-traefik"][..],
            &["--preset=digitalocean-paas"][..],
            &["--preset", " digitalocean-paas "][..],
        ] {
            let argv: Vec<String> = argv.iter().map(|argument| argument.to_string()).collect();
            assert!(parse(&argv).is_ok(), "{argv:?}");
        }
        // A re-exec of this program by itself keeps `1` (the protected matrix
        // pins `--stage=1 --preset x`), and still names the flag.
        let restage: Vec<String> = ["--stage=1", "--preset", "x"]
            .iter()
            .map(|argument| argument.to_string())
            .collect();
        let refusal = parse(&restage).expect_err("stage 1 has no preset");
        assert_eq!(refusal.exit, FAILURE);
        assert!(refusal.message.contains("--preset"), "{refusal:?}");
    }

    #[test]
    fn the_standalone_binary_plays_every_role() {
        assert!(role_is_allowed("dbproxy", false).is_ok());
        assert!(role_is_allowed("http", false).is_ok());
    }

    #[test]
    fn an_interpreter_never_holds_the_credentials() {
        let err = role_is_allowed("dbproxy", true).unwrap_err();
        assert!(err.contains("blastit-dbproxy"), "{err}");
    }

    #[test]
    fn the_preset_flag_parses_and_belongs_to_stage_zero() {
        // E1: `--preset=NAME` is a stage 0 argument. Stage 1 reads the values
        // the preset supplied out of the configuration document instead, and
        // the roles below it have no configuration to apply a preset to.
        let args = parse(&["--preset=compose-traefik".to_string()]).unwrap();
        assert_eq!(args.preset.as_deref(), Some("compose-traefik"));
        assert!(args.stage.is_none());

        // Whitespace around it is the operator's, not part of the name.
        let padded = parse(&["--preset= compose-traefik ".to_string()]).unwrap();
        assert_eq!(padded.preset.as_deref(), Some("compose-traefik"));

        // An empty value is a flag that was meant to carry something.
        assert!(parse(&["--preset=".to_string()]).is_err());
        assert!(parse(&["--preset= ".to_string()]).is_err());
        assert!(
            parse(&[
                "--stage=1".to_string(),
                "--preset=compose-traefik".to_string()
            ])
            .is_err()
        );
        assert!(
            parse(&[
                "--role=dbproxy".to_string(),
                "--secrets-fd=3".to_string(),
                "--events-fd=4".to_string(),
                "--ready-fd=5".to_string(),
                "--preset=compose-traefik".to_string(),
            ])
            .is_err()
        );
    }

    #[test]
    fn the_preset_flag_takes_its_value_as_a_separate_argument_too() {
        // E1: `--preset NAME` is what the documentation and an operator's shell
        // history use, so it is accepted beside the `--key=value` spelling.
        let spaced = parse(&["--preset".to_string(), "digitalocean-paas".to_string()]).unwrap();
        assert_eq!(spaced.preset.as_deref(), Some("digitalocean-paas"));

        // Either spelling, and either way whitespace around the name is the
        // operator's, not part of it.
        let joined = parse(&["--preset=digitalocean-paas".to_string()]).unwrap();
        assert_eq!(joined.preset, spaced.preset);
        let padded = parse(&["--preset".to_string(), " digitalocean-paas ".to_string()]).unwrap();
        assert_eq!(padded.preset.as_deref(), Some("digitalocean-paas"));

        // A flag with nothing after it is a flag that was meant to carry
        // something, and the next argument being another flag says the same
        // thing: a preset is never called `--help`.
        assert!(parse(&["--preset".to_string()]).is_err());
        assert!(parse(&["--preset".to_string(), "--help".to_string()]).is_err());
        assert!(parse(&["--preset".to_string(), "--preset=x".to_string()]).is_err());
        // A name that is not a preset is refused here, at the command line,
        // and the error lists the ones there are (`A-11`).
        let error = parse(&["--preset".to_string(), "nope".to_string()])
            .expect_err("`nope` is not a preset");
        assert!(error.message.contains("unknown preset `nope`"), "{error:?}");
        for preset in crate::config::PRESETS {
            assert!(error.message.contains(preset.name()), "{error:?}");
        }

        // The last one wins, whichever spelling each uses: this is an argument,
        // not a set.
        let twice = parse(&[
            "--preset".to_string(),
            "compose-traefik".to_string(),
            "--preset=digitalocean-paas".to_string(),
        ])
        .unwrap();
        assert_eq!(twice.preset.as_deref(), Some("digitalocean-paas"));

        // And it still belongs to stage 0 on its own.
        assert!(
            parse(&[
                "--stage=1".to_string(),
                "--preset".to_string(),
                "x".to_string()
            ])
            .is_err()
        );
    }

    #[test]
    fn the_usage_text_lists_the_presets() {
        // E1: `--preset` is the one argument whose values an operator cannot
        // guess, so `--help` (and every argument error) names them, one line
        // each, and shows the spelling the documentation uses.
        let usage = usage(false);
        assert!(usage.contains("--preset NAME"), "{usage}");
        assert!(usage.contains("BLASTIT_PRESET=NAME"), "{usage}");
        for preset in crate::config::PRESETS {
            assert!(usage.contains(preset.name()), "{usage}");
            assert!(usage.contains(preset.summary()), "{usage}");
        }
        // And one per line, not run together.
        assert!(
            usage.lines().filter(|line| line.contains(": ")).count() >= 2,
            "{usage}"
        );
    }

    #[test]
    fn the_usage_hides_the_reexec_grammar_and_offers_version_and_help() {
        // `A-8`: `--stage=1` and `--role=dbproxy` are plumbing between two
        // images of this same program. They are still parsed (the tests above
        // prove it), and they are not something a person is taught to type.
        let plain = usage(false);
        for internal in ["--stage=1", "--role=dbproxy", "--config-fd", "--secrets-fd"] {
            assert!(!plain.contains(internal), "{internal} is hidden:\n{plain}");
        }
        assert!(plain.contains("-V, --version"), "{plain}");
        assert!(
            plain.contains("blastit help [dev|check|fmt|lsp]"),
            "{plain}"
        );
        assert!(plain.contains("-h, --help"), "{plain}");
        assert!(!plain.contains('\u{1b}'), "{plain}");
        // The same text on a terminal colours only its two headers, so every
        // line of the body is byte-for-byte the plain one.
        let coloured = usage(true);
        assert!(coloured.starts_with("\u{1b}[1m\u{1b}[36musage:\u{1b}[0m\n"));
        let stripped = coloured
            .replace("\u{1b}[1m\u{1b}[36m", "")
            .replace("\u{1b}[0m", "");
        assert_eq!(stripped, plain);
    }

    #[test]
    fn help_names_every_command_and_refuses_an_unknown_one() {
        assert!(help_for(None, false).expect("help").starts_with("usage:"));
        assert!(
            help_for(Some("blastit"), false)
                .expect("blastit")
                .starts_with("usage:")
        );
        assert!(
            help_for(Some("dev"), false)
                .expect("dev")
                .contains("--port")
        );
        assert!(
            help_for(Some("check"), false)
                .expect("check")
                .contains("--dry-run")
        );
        assert!(
            help_for(Some("test"), false)
                .expect("test")
                .contains("--json")
        );
        assert!(
            help_for(Some("fmt"), false)
                .expect("fmt")
                .contains("--check")
        );
        assert!(
            help_for(Some("lsp"), false)
                .expect("lsp")
                .contains("blastit lsp")
        );
        // `serve` is not a command this build has: `blastit help serve` is
        // refused exactly as `blastit serve` is (`A-14`).
        for unknown in ["wat", "serve"] {
            assert_eq!(
                help_for(Some(unknown), false).expect_err("unknown"),
                format!("unknown subcommand '{unknown}' — run 'blastit --help'")
            );
        }
    }

    /// The `Run` half of a parsed subcommand, for the grammar twins below.
    ///
    /// `run_dev` and `run_check` are seams that later leaves fill in, so their
    /// exit codes say nothing about the grammar; what the grammar decided is the
    /// options, and that is what these tests assert.
    fn run<T>(subcommand: Subcommand<T>) -> T {
        match subcommand {
            Subcommand::Run(options) => options,
            Subcommand::Help => panic!("`--help` is not a command line to run"),
        }
    }

    #[test]
    fn the_dev_grammar_is_exactly_the_documented_one() {
        // R2: `--port N`/`--port=N` and `--host ADDR`/`--host=ADDR`, `--learn`,
        // and the documented default for everything that was not given. A port
        // is always passed here, so the ambient `PORT` cannot change the result.
        let loopback = IpAddr::from([127, 0, 0, 1]);
        assert_eq!(
            run(parse_dev(&["--port".to_string(), "9000".to_string()]).unwrap()),
            DevOptions {
                port: 9000,
                host: loopback,
                learn: false,
                no_reload: false,
            }
        );
        assert_eq!(
            run(parse_dev(&[
                "--port=9001".to_string(),
                "--host=127.0.0.1".to_string(),
                "--learn".to_string(),
            ])
            .unwrap()),
            DevOptions {
                port: 9001,
                host: loopback,
                learn: true,
                no_reload: false,
            }
        );
        // `--no-reload` is the one user-facing reload switch, and reload is on
        // unless it is given (`docs/reference/cli.md` `## blastit dev`).
        assert!(run(parse_dev(&["--no-reload".to_string()]).unwrap()).no_reload);
        // The internal stage-1 tokens are not `dev` arguments: a developer who
        // types one gets a sentence, not a silently ignored flag.
        for internal in [
            "--dev-reload=on",
            "--dev-adopt-fd=3",
            "--dev-policy-fd=4",
            "--dev-reload-since=1",
        ] {
            let error = parse_dev(&[internal.to_string()]).unwrap_err();
            assert!(error.contains(internal), "{internal}: {error}");
        }
        assert_eq!(
            run(parse_dev(&[
                "--port=9002".to_string(),
                "--host".to_string(),
                "::1".to_string(),
            ])
            .unwrap())
            .host,
            "::1".parse::<IpAddr>().unwrap()
        );
        // The last spelling of a value wins, whichever one it is (`parse`'s
        // rule, kept the same here).
        assert_eq!(
            run(parse_dev(&[
                "--port=9003".to_string(),
                "--port".to_string(),
                "9004".to_string(),
            ])
            .unwrap())
            .port,
            9004
        );
        // R2's floor: a port is 1..=65535, so 0 is not "any free port".
        for bad in [
            &["--port".to_string(), "0".to_string()][..],
            &["--port=0".to_string()][..],
            &["--port".to_string(), "65536".to_string()][..],
            &["--port=soon".to_string()][..],
        ] {
            let error = parse_dev(bad).unwrap_err();
            assert!(error.contains("--port"), "{bad:?}: {error}");
        }
        // `--help` is not a command line to run.
        assert!(matches!(
            parse_dev(&["--help".to_string()]).unwrap(),
            Subcommand::Help
        ));
    }

    #[test]
    fn the_check_grammar_is_exactly_the_documented_one() {
        // R3: the three flags, their combination rule, and the `--` split.
        assert_eq!(
            run(parse_check(&[]).unwrap()),
            CheckOptions {
                json: false,
                learn: false,
                dry_run: false,
                pytest_args: Vec::new(),
            }
        );
        assert_eq!(
            run(parse_check(&["--learn".to_string(), "--dry-run".to_string()]).unwrap()),
            CheckOptions {
                json: false,
                learn: true,
                dry_run: true,
                pytest_args: Vec::new(),
            }
        );
        assert_eq!(
            run(parse_check(&[
                "--json".to_string(),
                "--learn".to_string(),
                "--dry-run".to_string(),
                "--".to_string(),
                "-q".to_string(),
                "tests".to_string(),
            ])
            .unwrap()),
            CheckOptions {
                json: true,
                learn: true,
                dry_run: true,
                pytest_args: vec!["-q".to_string(), "tests".to_string()],
            }
        );
        // `--dry-run` without `--learn` is the combination §1.1 refuses.
        let error = parse_check(&["--dry-run".to_string()]).unwrap_err();
        assert!(error.contains("--dry-run"), "{error}");
        // Everything after `--` belongs to `pytest`, even a word that looks
        // like this command's own flag.
        let forwarded = run(parse_check(&[
            "--learn".to_string(),
            "--".to_string(),
            "--dry-run".to_string(),
        ])
        .unwrap());
        assert!(
            !forwarded.dry_run,
            "a forwarded word is not this command's flag"
        );
        assert_eq!(forwarded.pytest_args, ["--dry-run"]);
        // And the two forwarded flags that would hijack the harness are refused
        // by name, whichever spelling they use (R3).
        let error =
            parse_check(&["--".to_string(), "--blastit-live-server=1".to_string()]).unwrap_err();
        assert!(error.contains("--blastit-live-server"), "{error}");
        assert!(matches!(
            parse_check(&["-h".to_string()]).unwrap(),
            Subcommand::Help
        ));
    }
}
