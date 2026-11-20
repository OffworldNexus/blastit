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
//! stage 0 does that (§4). The four documented exceptions are named where they
//! are read, all before any capability gate: `PORT` (`blastit dev`'s fallback),
//! `BLASTIT_PRESET` (checked here so a typo is named before the gate), and
//! `handoff`'s `VIRTUAL_ENV` / `BLASTIT_HANDOFF_DONE` / `BLASTIT_NO_LOCAL_HANDOFF`.
//!
//! # The five modules
//!
//! * `handoff` — §4: hand `dev`/`check`/`fmt`/`lsp` to the project's own
//!   `blastit` before this image acts on them.
//! * `help` — every word a person reads, and the one shape a refusal takes.
//! * `parse` — the grammar of the four subcommands and the re-exec lines.
//! * `preset` — the one resolver for `--preset` and `BLASTIT_PRESET`.
//! * `run` — where a validated command line is handed to the stage that runs
//!   it.
//!
//! # Library and binary
//!
//! This module is the whole binary. `crates/blastit`'s `main` calls it with a
//! [`Host`] that re-executes `/proc/self/exe` (§4), and a wheel's console script
//! `exec`s that same binary (`blastit/_bin/blastit`), so there is one shape of
//! every role and no interpreter hosts a stage (`MB.42.2.2.2`).

use std::net::IpAddr;
use std::os::fd::RawFd;

use crate::host::Host;
use crate::logs::Colour;
use crate::stage0;
use crate::stage1;

mod handoff;
mod help;
mod parse;
mod preset;
mod run;

use help::{help_for, refuse, usage};
use parse::{Subcommand, parse, parse_check, parse_dev, parse_fmt, parse_lsp};
use preset::env_is_known;
use run::{run_check, run_dev, run_fmt};

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
    /// `--py-lib-fd=N`: the pinned `libpython` stage 0 opened (design §2.4).
    /// Internal: never typed by an operator, and never accepted without
    /// `--stage=1`.
    #[cfg(feature = "python")]
    py_lib_fd: Option<RawFd>,
    /// `--py-shim-fd=N`: the pinned per-minor shim, beside `--py-lib-fd`.
    #[cfg(feature = "python")]
    py_shim_fd: Option<RawFd>,
}

/// Runs the role the command line names, in the installation `host` describes.
///
/// Returns the process exit status. Stage 0 and stage 1 never return at all.
pub fn main(host: Host, argv: Vec<String>) -> u8 {
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
            Ok(Subcommand::Run(options)) => {
                after_handoff(Command::Dev, &argv, || run_dev(&host, &options, colour))
            }
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
            Ok(Subcommand::Run(options)) => {
                after_handoff(Command::Check, &argv, || run_check(&host, &options, colour))
            }
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
            Ok(Subcommand::Run(options)) => {
                after_handoff(Command::Fmt, &argv, || run_fmt(&options))
            }
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
            Ok(Subcommand::Run(())) => after_handoff(Command::Lsp, &argv, run_lsp),
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
                    dispatch(args, &host, colour)
                }
                Err(refusal) => {
                    refuse("blastit", &refusal.message, true, colour);
                    refusal.exit
                }
            }
        }
    }
}

/// §4: let the project's own `blastit` answer `command`, or run `this` image.
///
/// Only `dev`, `check`, `fmt` and `lsp` reach here. `Serve` answers from the
/// binary that was typed, so a deployment is the artifact the operator
/// installed (`R6`–`R8`); a valid handoff replaces
/// this process image and never returns, and a refusal has already been
/// printed, so `Some` is the status to exit with.
fn after_handoff(command: Command, argv: &[String], this: impl FnOnce() -> u8) -> u8 {
    match handoff::attempt(command, argv) {
        Some(status) => status,
        None => this(),
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
fn dispatch(args: Args, host: &Host, colour: Colour) -> u8 {
    if let Some(role) = args.role.as_deref() {
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
        None | Some(0) => stage0::run(host, args.preset.as_deref(), colour),
        Some(1) => {
            let Some(config_fd) = args.config_fd else {
                eprintln!("blastit: --stage=1 needs --config-fd");
                return FAILURE;
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
                    #[cfg(feature = "python")]
                    py_lib_fd: args.py_lib_fd,
                    #[cfg(feature = "python")]
                    py_shim_fd: args.py_shim_fd,
                },
                colour,
            )
        }
        Some(other) => {
            eprintln!("blastit: unknown stage `{other}`");
            FAILURE
        }
    }
}
