//! The `blastit-dbproxy` binary: the credentials-holding role on its own
//! (§2.2, §4).
//!
//! A wheel install ships this program at `blastit/_bin/blastit-dbproxy`, and
//! stage 0 `execve`s it with the same arguments it gives `/proc/self/exe` in the
//! standalone install, so both arrive at the same code
//! ([`blastit_dbproxy::role::dbproxy`]). The difference is what it is *not*: it
//! links no interpreter, so the process that holds the real credentials has no
//! CPython in it at all (D2).
//!
//! `--role=dbproxy` is required even though there is no other role to choose
//! here: it is the flag §4 documents, and keeping the command line identical is
//! what lets the same stage 0 serve both installations.

use std::os::fd::RawFd;
use std::process::ExitCode;

/// How to use the program, printed with a command-line error.
const USAGE: &str = "\
usage:
  blastit-dbproxy --role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0|1]";

/// Where the exit code comes from: the role's own status.
fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match parse(&arguments) {
        Ok(options) => {
            let code = blastit_dbproxy::role::dbproxy(
                options.secrets_fd,
                options.events_fd,
                options.ready_fd,
                options.seccomp,
            );
            ExitCode::from(u8::try_from(code).unwrap_or(1))
        }
        Err(message) => {
            eprintln!("blastit-dbproxy: {message}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// The descriptors and flags dbproxy is `execve`'d with (§4).
#[derive(Debug, PartialEq, Eq)]
struct Options {
    /// `--secrets-fd`: the read end of `secrets.pipe`.
    secrets_fd: RawFd,
    /// `--events-fd`: the write end of dbproxy's events pipe.
    events_fd: RawFd,
    /// `--ready-fd`: the write end of the ready pipe.
    ready_fd: RawFd,
    /// `--seccomp=0|1`: §7.3's filters. Absent means on: `BLASTIT_SECCOMP`
    /// defaults to on (§8.2), and a dbproxy started by hand is no reason to run
    /// unfiltered.
    seccomp: bool,
}

/// Parses `--key=value` arguments, refusing anything else.
///
/// # Errors
///
/// A one-line message naming the argument; the caller prints it with the usage.
fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut role = false;
    let mut secrets_fd = None;
    let mut events_fd = None;
    let mut ready_fd = None;
    let mut seccomp = None;
    for argument in arguments {
        let Some((key, value)) = argument.split_once('=') else {
            return Err(format!("`{argument}` is not `--key=value`"));
        };
        match key {
            "--role" => {
                if value != "dbproxy" {
                    return Err(format!("unknown role `{value}`"));
                }
                role = true;
            }
            "--secrets-fd" => secrets_fd = Some(descriptor(value, key)?),
            "--events-fd" => events_fd = Some(descriptor(value, key)?),
            "--ready-fd" => ready_fd = Some(descriptor(value, key)?),
            "--seccomp" => seccomp = Some(flag(value, key)?),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    if !role {
        return Err("`--role=dbproxy` is required".to_string());
    }
    match (secrets_fd, events_fd, ready_fd) {
        (Some(secrets_fd), Some(events_fd), Some(ready_fd)) => Ok(Options {
            secrets_fd,
            events_fd,
            ready_fd,
            seccomp: seccomp.unwrap_or(true),
        }),
        _ => Err("`--secrets-fd`, `--events-fd` and `--ready-fd` are required".to_string()),
    }
}

/// A descriptor number argument.
fn descriptor(value: &str, key: &str) -> Result<RawFd, String> {
    let fd: i32 = value
        .parse()
        .map_err(|_| format!("`{key}` wants a descriptor number, not `{value}`"))?;
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
    fn the_dbproxy_command_line_parses() {
        let options = parse(&[
            "--role=dbproxy".to_string(),
            "--secrets-fd=3".to_string(),
            "--events-fd=4".to_string(),
            "--ready-fd=5".to_string(),
            "--seccomp=0".to_string(),
        ])
        .unwrap();
        assert_eq!(
            options,
            Options {
                secrets_fd: 3,
                events_fd: 4,
                ready_fd: 5,
                seccomp: false,
            }
        );
    }

    #[test]
    fn seccomp_is_on_unless_it_is_turned_off() {
        let mut arguments = vec![
            "--role=dbproxy".to_string(),
            "--secrets-fd=3".to_string(),
            "--events-fd=4".to_string(),
            "--ready-fd=5".to_string(),
        ];
        assert!(parse(&arguments).unwrap().seccomp);
        arguments.push("--seccomp=0".to_string());
        assert!(!parse(&arguments).unwrap().seccomp);
    }

    #[test]
    fn the_role_is_required() {
        let err = parse(&["--secrets-fd=3".to_string()]).unwrap_err();
        assert!(err.contains("--role=dbproxy"), "{err}");
    }

    #[test]
    fn every_descriptor_is_required() {
        let err = parse(&["--role=dbproxy".to_string(), "--secrets-fd=3".to_string()]).unwrap_err();
        assert!(err.contains("--ready-fd"), "{err}");
    }

    #[test]
    fn an_unknown_argument_is_refused() {
        assert!(parse(&["--wat=1".to_string()]).is_err());
        assert!(parse(&["--role=http".to_string()]).is_err());
        assert!(parse(&["--role".to_string()]).is_err());
        assert!(parse(&["--ready-fd=x".to_string()]).is_err());
        assert!(parse(&["--ready-fd=-1".to_string()]).is_err());
        assert!(parse(&["--seccomp=2".to_string()]).is_err());
        assert!(parse(&["--seccomp=yes".to_string()]).is_err());
    }
}
