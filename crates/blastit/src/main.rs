//! blastit: init (PID 1), its stages and roles (ARCHITECTURE.md §2.1, §4).
//!
//! One binary plays every role (§2), and the role is chosen by argv:
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

mod child;
mod config;
mod logs;
mod roles;
mod stage0;
mod stage1;
mod supervise;
mod zygote;

use std::os::fd::RawFd;
use std::process::ExitCode;

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
}

/// How to use the binary, printed with a command-line error.
const USAGE: &str = "\
usage:
  blastit                                   stage 0 (no arguments)
  blastit --stage=1 --config-fd=N \\
          [--dbproxy-pid=P --dbproxy-events-fd=N \\
           --dbproxy-stdout-fd=N --dbproxy-stderr-fd=N]   stage 1
  blastit --role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0|1]";

/// Where the exit code comes from: init's own status.
fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match parse(&arguments) {
        Ok(args) => dispatch(args),
        Err(message) => {
            eprintln!("blastit: {message}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the role the arguments name.
fn dispatch(args: Args) -> ExitCode {
    if let Some(role) = args.role.as_deref() {
        return match role {
            "dbproxy" => {
                let (Some(secrets_fd), Some(events_fd), Some(ready_fd)) =
                    (args.secrets_fd, args.events_fd, args.ready_fd)
                else {
                    eprintln!(
                        "blastit: --role=dbproxy needs --secrets-fd, --events-fd and --ready-fd"
                    );
                    return ExitCode::FAILURE;
                };
                // Absent means on: `BLASTIT_SECCOMP` defaults to on (§8.2), and
                // a dbproxy started by hand is no reason to run unfiltered.
                let seccomp = args.seccomp.unwrap_or(true);
                let code = roles::dbproxy(secrets_fd, events_fd, ready_fd, seccomp);
                ExitCode::from(u8::try_from(code).unwrap_or(1))
            }
            other => {
                eprintln!("blastit: unknown role `{other}`");
                ExitCode::FAILURE
            }
        };
    }

    match args.stage {
        None | Some(0) => stage0::run(),
        Some(1) => {
            let Some(config_fd) = args.config_fd else {
                eprintln!("blastit: --stage=1 needs --config-fd");
                return ExitCode::FAILURE;
            };
            stage1::run(stage1::Options {
                config_fd,
                dbproxy_pid: args.dbproxy_pid,
                dbproxy_events_fd: args.dbproxy_events_fd,
                dbproxy_stdout_fd: args.dbproxy_stdout_fd,
                dbproxy_stderr_fd: args.dbproxy_stderr_fd,
            })
        }
        Some(other) => {
            eprintln!("blastit: unknown stage `{other}`");
            ExitCode::FAILURE
        }
    }
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
            other => return Err(format!("unknown argument `{other}`")),
        }
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
        assert_eq!(
            parse(&["--seccomp=1".to_string()]).unwrap().seccomp,
            Some(true)
        );
    }
}
