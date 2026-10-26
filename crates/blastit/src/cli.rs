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

use std::os::fd::RawFd;

use pyo3::Python;

use crate::host::Host;
use crate::stage0;
use crate::stage1;

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
}

/// How to use the program, printed with a command-line error.
///
/// The presets are listed with their one-line descriptions: `--preset` is the
/// one argument whose valid values an operator cannot guess (E1).
fn usage() -> String {
    format!(
        "{USAGE}\n\n\
         presets (also `BLASTIT_PRESET=NAME`; an explicit BLASTIT_* variable wins over any \
         of them):\n{}",
        crate::config::preset_help()
    )
}

/// The roles and their arguments; [`usage`] adds the presets.
const USAGE: &str = "\
usage:
  blastit [--preset=NAME]                   stage 0
  blastit --stage=1 --config-fd=N \\
          [--dbproxy-pid=P --dbproxy-events-fd=N \\
           --dbproxy-stdout-fd=N --dbproxy-stderr-fd=N]   stage 1
  blastit --role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0|1]";

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
    // E1: the usage text lists the presets with their one-line descriptions,
    // which is the only way to find out what `--preset` accepts.
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

/// The exit status of everything that refuses to start (§4).
const FAILURE: u8 = 1;

/// The exit status of `--help`, which is not a failure.
const SUCCESS: u8 = 0;

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

/// Parses `--key=value` arguments, refusing anything else.
///
/// # Errors
///
/// A one-line message naming the argument; the caller prints it with the usage.
fn parse(arguments: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    for argument in arguments {
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
    fn the_usage_text_lists_the_presets() {
        // E1: `--preset` is the one argument whose values an operator cannot
        // guess, so `--help` (and every argument error) names them, one line
        // each.
        let usage = usage();
        assert!(usage.contains("--preset=NAME"), "{usage}");
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
}
