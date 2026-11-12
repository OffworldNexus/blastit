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
//! stage 0 does that (§4).
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

use pyo3::Python;

use crate::host::Host;
use crate::stage0;
use crate::stage1;

/// The three command shapes `0.1.1` exposes (§1.1).
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
}

impl Command {
    /// The subcommand `argv` names, before any flags are parsed.
    pub fn from_argv(argv: &[String]) -> Self {
        match argv.first().map(String::as_str) {
            Some("dev") => Self::Dev,
            Some("check" | "test") => Self::Check,
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

/// What a subcommand's parser decided: print its help, or run it.
#[derive(Debug)]
enum Subcommand<T> {
    Help,
    Run(T),
}

/// What the command line says about which role to play.
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
}

/// How to use the program, printed with a command-line error.
///
/// The presets are listed with their one-line descriptions: `--preset` is the
/// one argument whose valid values an operator cannot guess (E1).
fn usage() -> String {
    format!(
        "{USAGE}\n\n\
         presets (`--preset NAME`, or `BLASTIT_PRESET=NAME`; every BLASTIT_* variable that \
         is set wins over one):\n{}",
        crate::config::preset_help()
    )
}

/// The roles and their arguments; [`usage`] adds the presets.
const USAGE: &str = "\
usage:
  blastit [--preset NAME]                   stage 0 (production)
  blastit dev [--port N] [--host ADDR] [--learn]
                                            unprivileged single-UID simulation
  blastit check [--json] [--learn [--dry-run]] [-- ARGS...]
                                            audit a pytest suite (alias: test)
  blastit --stage=1 --config-fd=N \\
          [--dbproxy-pid=P --dbproxy-events-fd=N \\
           --dbproxy-stdout-fd=N --dbproxy-stderr-fd=N]   stage 1
  blastit --role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0|1]";

/// `blastit dev --help` (§1.1).
const DEV_USAGE: &str = "\
usage: blastit dev [--port N] [--host ADDR] [--learn]

  --port N      port to bind, 1..=65535 (default 8080, or PORT when set)
  --host ADDR   IP address to bind (default 127.0.0.1); a non-loopback
                address needs BLASTIT_DEV_EXPOSE=1
  --learn       observe requests and SQL and propose manifest additions
  -h, --help    print this help

`blastit dev` runs the real five-role tree as the current uid: no sudo, no
capabilities, not a sandbox. Use `blastit` for production.";

/// `blastit check --help` (§1.1).
const CHECK_USAGE: &str = "\
usage: blastit check [--json] [--learn [--dry-run]] [-- ARGS...]
       blastit test  ...                    exact alias of `check`

  --json        write at most one JSON document to stdout; everything else,
                including pytest output, goes to stderr
  --learn       observe requests and SQL and propose manifest additions
  --dry-run     with --learn, report the proposal without writing it
  -- ARGS...    passed to `pytest -q` verbatim
  -h, --help    print this help

Exit codes: 0 clean, 1 completeness findings, 2 CLI/config error, 3 pytest
failed (including exit 5, no tests collected).";

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
    // Normative dispatch order (§4.2 E.1): a subcommand is the first argument,
    // so `dev` is checked before `check` before the stage-0/stage-1/`--role`
    // grammar. A stage-0 argument named `dev` is not a thing: the command is the
    // first word.
    match Command::from_argv(&argv) {
        Command::Dev => match parse_dev(&argv[1..]) {
            Ok(Subcommand::Help) => {
                println!("{DEV_USAGE}");
                SUCCESS
            }
            Ok(Subcommand::Run(options)) => run_dev(&host, &options, py.is_some()),
            Err(message) => {
                eprintln!("blastit dev: {message}");
                FAILURE
            }
        },
        Command::Check => match parse_check(&argv[1..]) {
            Ok(Subcommand::Help) => {
                println!("{CHECK_USAGE}");
                SUCCESS
            }
            Ok(Subcommand::Run(options)) => run_check(&options),
            Err(message) => {
                eprintln!("blastit check: {message}");
                CHECK_FAILURE
            }
        },
        Command::Serve => {
            // E1: the usage text lists the presets with their one-line
            // descriptions, which is the only way to find out what `--preset`
            // accepts, and the three commands the binary has.
            if argv
                .iter()
                .any(|argument| argument == "--help" || argument == "-h")
            {
                println!("{}", usage());
                return SUCCESS;
            }
            match parse(&argv) {
                Ok(args) => dispatch(args, &host, py),
                Err(message) => {
                    eprintln!("blastit: {message}\n{}", usage());
                    FAILURE
                }
            }
        }
    }
}

/// The exit status of everything that refuses to start (§4).
const FAILURE: u8 = 1;

/// The exit status of `--help`, which is not a failure.
const SUCCESS: u8 = 0;

/// `blastit check`'s CLI failure, distinct from a finding (`1`) and pytest
/// (`3`) (`R4`).
const CHECK_FAILURE: u8 = 2;

/// Parses and validates `blastit dev`'s arguments (`R2`).
///
/// # Errors
///
/// A one-line message naming the argument and the fix. Every rejection is a
/// CLI failure (`exit 1`) with nothing on stdout.
fn parse_dev(arguments: &[String]) -> Result<Subcommand<DevOptions>, String> {
    let mut port = None;
    let mut host = None;
    let mut learn = false;
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        index += 1;
        match argument.as_str() {
            "-h" | "--help" => return Ok(Subcommand::Help),
            "--learn" => learn = true,
            "--manifest" => {
                return Err(
                    "`--manifest` is not accepted: put `blastit.toml` in the working \
                     directory, or set BLASTIT_MANIFEST"
                        .to_string(),
                );
            }
            "--preset" => {
                return Err(
                    "`--preset` is only for `blastit` (production); `blastit dev` \
                     ignores BLASTIT_PRESET"
                        .to_string(),
                );
            }
            "--port" => {
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "`--port` wants a value from 1 to 65535".to_string())?;
                index += 1;
                port = Some(parse_port(value)?);
            }
            "--host" => {
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "`--host` wants an IP address".to_string())?;
                index += 1;
                host = Some(parse_host(value)?);
            }
            other => {
                if let Some(value) = other.strip_prefix("--port=") {
                    port = Some(parse_port(value)?);
                } else if let Some(value) = other.strip_prefix("--host=") {
                    host = Some(parse_host(value)?);
                } else if other.starts_with('-') {
                    return Err(format!("unknown argument `{other}`"));
                } else {
                    return Err(format!("`{other}` is not an argument of `blastit dev`"));
                }
            }
        }
    }
    Ok(Subcommand::Run(DevOptions {
        port: match port {
            Some(port) => port,
            None => port_from_env()?,
        },
        host: host.unwrap_or(IpAddr::from([127, 0, 0, 1])),
        learn,
    }))
}

/// Parses and validates `blastit check` / `blastit test` (`R3`).
///
/// The split on `--` happens first: everything after it belongs to `pytest`,
/// and the two flags that would hijack the harness are refused by name.
///
/// # Errors
///
/// A one-line message naming the argument and the fix. Every rejection is
/// `exit 2` with nothing on stdout.
fn parse_check(arguments: &[String]) -> Result<Subcommand<CheckOptions>, String> {
    let (flags, forwarded) = match arguments.iter().position(|argument| argument == "--") {
        Some(index) => (&arguments[..index], &arguments[index + 1..]),
        None => (arguments, &[][..]),
    };
    for argument in forwarded {
        if argument == "--blastit-manifest" || argument.starts_with("--blastit-manifest=") {
            return Err(
                "pytest arguments must not include `--blastit-manifest`: set \
                 BLASTIT_MANIFEST or put `blastit.toml` in the working directory"
                    .to_string(),
            );
        }
        if argument == "--blastit-live-server" || argument.starts_with("--blastit-live-server=") {
            return Err(
                "pytest arguments must not include `--blastit-live-server`: `blastit check` \
                 provides the server"
                    .to_string(),
            );
        }
    }
    let mut options = CheckOptions {
        json: false,
        learn: false,
        dry_run: false,
        pytest_args: forwarded.to_vec(),
    };
    for argument in flags {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Subcommand::Help),
            "--json" => options.json = true,
            "--learn" => options.learn = true,
            "--dry-run" => options.dry_run = true,
            "--manifest" => {
                return Err(
                    "`--manifest` is not accepted: put `blastit.toml` in the working \
                     directory, or set BLASTIT_MANIFEST"
                        .to_string(),
                );
            }
            "--preset" => {
                return Err(
                    "`--preset` is only for `blastit` (production); `blastit check` \
                     ignores BLASTIT_PRESET"
                        .to_string(),
                );
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    if options.dry_run && !options.learn {
        return Err("`--dry-run` is only valid with `--learn`".to_string());
    }
    Ok(Subcommand::Run(options))
}

/// A `--port` / `PORT` value: `1..=65535` (`R2`).
fn parse_port(value: &str) -> Result<u16, String> {
    let port: u16 = value
        .trim()
        .parse()
        .map_err(|_| format!("`--port` wants a whole number from 1 to 65535, not `{value}`"))?;
    if port == 0 {
        return Err("`--port` wants a whole number from 1 to 65535, not `0`".to_string());
    }
    Ok(port)
}

/// `PORT` when neither `--port` nor its environment spelling is absent (`R2`).
fn port_from_env() -> Result<u16, String> {
    match std::env::var("PORT") {
        Ok(value) => parse_port(&value).map_err(|reason| format!("PORT: {reason}")),
        Err(std::env::VarError::NotPresent) => Ok(8080),
        Err(std::env::VarError::NotUnicode(_)) => Err("PORT: the value is not valid UTF-8".into()),
    }
}

/// A `--host` value: an IP address, not a host name (`R2`).
fn parse_host(value: &str) -> Result<IpAddr, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("`--host` wants an IP address such as 127.0.0.1, not `{value}`"))
}

/// The `blastit dev` supervisor's entry (`MA.2.3.3`).
///
/// The grammar above decided *what* to run; this hands the options to stage 0's
/// unprivileged mode, which never returns: it either `execve`s stage 1 or exits
/// non-zero. `host` is the installation, exactly as it is for production.
fn run_dev(host: &Host, options: &DevOptions, interpreter: bool) -> u8 {
    crate::stage0::run_dev(host, options, interpreter)
}

/// The `blastit check` engine's seam (`MA.2.3.5` fills this in).
fn run_check(options: &CheckOptions) -> u8 {
    let _ = options;
    eprintln!("blastit check: the check engine is not wired into this build yet (MA.2.3.5)");
    CHECK_FAILURE
}

/// Runs the role the arguments name.
fn dispatch(args: Args, host: &Host, py: Option<Python<'_>>) -> u8 {
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
        None | Some(0) => stage0::run(host, py.is_some(), args.preset.as_deref()),
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
                },
                interpreter,
            )
        }
        Some(other) => {
            eprintln!("blastit: unknown stage `{other}`");
            FAILURE
        }
    }
}

/// Whether an install that runs inside an interpreter may play the role (§2.2).
///
/// dbproxy holds the real credentials and must run no interpreter at all (§4),
/// so a wheel install — where blastit *is* a Python process — hands the role to
/// the Python-free `blastit-dbproxy` binary the wheel ships instead of playing
/// it in the process that imported the extension.
///
/// # Errors
///
/// A one-line message when the caller is in an interpreter and asked for
/// dbproxy.
fn role_is_allowed(role: &str, hosted: bool) -> Result<(), String> {
    if hosted && role == "dbproxy" {
        return Err(
            "--role=dbproxy runs no interpreter; use the `blastit-dbproxy` binary this \
             installation ships"
                .to_string(),
        );
    }
    Ok(())
}

/// Parses `--key=value` arguments — and `--preset NAME`, which people type —
/// refusing anything else.
///
/// # Errors
///
/// A one-line message naming the argument; the caller prints it with the usage.
fn parse(arguments: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        index += 1;
        // E1: `--preset NAME` is how the flag is written in the documentation
        // and how an operator types it; `--preset=NAME` is the spelling every
        // other argument of this program uses. Both are accepted, and this is
        // the only argument that takes a value of its own: the rest name
        // descriptors and roles, where a stray word means a mistake.
        if argument == "--preset" {
            let value = arguments
                .get(index)
                .filter(|next| !next.starts_with('-'))
                .ok_or_else(|| "`--preset` wants a preset name".to_string())?;
            index += 1;
            args.preset = Some(preset_name(value)?);
            continue;
        }
        let Some((key, value)) = argument.split_once('=') else {
            return Err(format!("`{argument}` is not `--key=value`"));
        };
        match key {
            "--stage" => args.stage = Some(number(value, key)?),
            "--role" => args.role = Some(value.to_string()),
            "--config-fd" => args.config_fd = Some(descriptor(value, key)?),
            "--dbproxy-pid" => args.dbproxy_pid = Some(number(value, key)?),
            "--dbproxy-events-fd" => args.dbproxy_events_fd = Some(descriptor(value, key)?),
            "--dbproxy-stdout-fd" => args.dbproxy_stdout_fd = Some(descriptor(value, key)?),
            "--dbproxy-stderr-fd" => args.dbproxy_stderr_fd = Some(descriptor(value, key)?),
            "--secrets-fd" => args.secrets_fd = Some(descriptor(value, key)?),
            "--events-fd" => args.events_fd = Some(descriptor(value, key)?),
            "--ready-fd" => args.ready_fd = Some(descriptor(value, key)?),
            "--seccomp" => args.seccomp = Some(flag(value, key)?),
            "--preset" => args.preset = Some(preset_name(value)?),
            // The four `dev`-only arguments travel from stage 0 to stage 1 and
            // name state an operator never types: the ephemeral root, the bind
            // address, and the two internal `--learn` descriptors (`R20`,
            // `R22`, `R29`).
            "--dev-root" => args.dev_root = Some(std::path::PathBuf::from(value)),
            "--dev-host" => args.dev_host = Some(parse_host(value)?),
            "--dev-obs-fd" => args.dev_obs_fd = Some(descriptor(value, key)?),
            "--dev-ctl-fd" => args.dev_ctl_fd = Some(descriptor(value, key)?),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    // `--seccomp` is how §7.3's `BLASTIT_SECCOMP` reaches the one role that is
    // `execve`'d with an empty environment; every other role is a `fork` of a
    // process that still has the variable.
    if args.seccomp.is_some() && args.role.as_deref() != Some("dbproxy") {
        return Err("`--seccomp` is only for `--role=dbproxy`".to_string());
    }
    // E1: the preset belongs to stage 0, which is the stage that reads the
    // environment. Stage 1 gets the values the preset supplied through the
    // configuration document, the same way it gets every other setting, and the
    // roles below stage 1 have no configuration of their own.
    if args.preset.is_some() && (args.stage == Some(1) || args.role.is_some()) {
        return Err("`--preset` is only for stage 0".to_string());
    }
    let any_dev = args.dev_root.is_some()
        || args.dev_host.is_some()
        || args.dev_obs_fd.is_some()
        || args.dev_ctl_fd.is_some();
    if any_dev && (args.stage != Some(1) || args.role.is_some()) {
        return Err("`--dev-*` arguments are only for `--stage=1`".to_string());
    }
    if any_dev && args.dev_root.is_none() {
        return Err("`--dev-*` arguments need `--dev-root`".to_string());
    }
    Ok(args)
}

/// A whole number argument.
fn number<T: std::str::FromStr>(value: &str, key: &str) -> Result<T, String> {
    value
        .parse::<T>()
        .map_err(|_| format!("`{key}` wants a whole number, not `{value}`"))
}

/// A descriptor number argument.
fn descriptor(value: &str, key: &str) -> Result<RawFd, String> {
    let fd: i32 = number(value, key)?;
    if fd < 0 {
        return Err(format!("`{key}` wants a descriptor number, not `{value}`"));
    }
    Ok(fd)
}

/// A `0`/`1` argument, the spelling every boolean the command line carries
/// uses.
fn flag(value: &str, key: &str) -> Result<bool, String> {
    match value {
        "1" => Ok(true),
        "0" => Ok(false),
        other => Err(format!("`{key}` wants 0 or 1, not `{other}`")),
    }
}

/// A `--preset` argument: a name, without the whitespace an operator may have
/// typed around it (E1).
///
/// The name itself is checked where the presets are known (E1,
/// [`crate::stage0`] hands it to `Config::from_env`), which is also where
/// `BLASTIT_PRESET` is checked; this only refuses the empty value, which is a
/// flag that was meant to carry something.
fn preset_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("`--preset` wants a preset name".to_string());
    }
    Ok(name.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
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
        ])
        .unwrap();
        assert_eq!(
            dev_stage1.dev_root.as_deref(),
            Some(std::path::Path::new("/tmp/blastit-dev-test"))
        );
        assert_eq!(dev_stage1.dev_obs_fd, Some(11));
        assert_eq!(dev_stage1.dev_ctl_fd, Some(12));
        assert!(parse(&["--dev-root=/tmp/x".to_string()]).is_err());
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
        // A name that is not a preset is refused where the presets are known
        // (`Config::from_env`), not here.
        assert_eq!(
            parse(&["--preset".to_string(), "nope".to_string()])
                .unwrap()
                .preset
                .as_deref(),
            Some("nope")
        );

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
        let usage = usage();
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
            }
        );
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
