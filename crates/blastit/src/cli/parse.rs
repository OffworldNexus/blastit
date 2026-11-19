//! The command line's grammar: four subcommands, and the re-exec grammar (§4.2).
//!
//! Everything here is pure: no file, no capability, and only the one environment
//! variable the documentation gives a subcommand (`PORT` as `blastit dev`'s
//! fallback). [`super::main`] decides what to run; this module decides whether
//! the words are ones this build understands, and with which exit status.

use std::net::IpAddr;
use std::os::fd::RawFd;

use super::help::Refusal;
use super::preset;
use super::{Args, CheckOptions, DevOptions, FmtOptions};

/// What a subcommand's parser decided: print its help, or run it.
#[derive(Debug)]
pub(super) enum Subcommand<T> {
    /// `-h` / `--help`.
    Help,
    /// The validated options.
    Run(T),
}

/// Parses and validates `blastit dev`'s arguments (`R2`).
///
/// # Errors
///
/// A one-line message naming the argument and the fix. Every rejection is a
/// CLI failure (`exit 1`, the status the protected CLI matrix pins) with
/// nothing on stdout.
pub(super) fn parse_dev(arguments: &[String]) -> Result<Subcommand<DevOptions>, String> {
    let mut port = None;
    let mut host = None;
    let mut learn = false;
    let mut no_reload = false;
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        index += 1;
        match argument.as_str() {
            "-h" | "--help" => return Ok(Subcommand::Help),
            "--learn" => learn = true,
            "--no-reload" => no_reload = true,
            "--manifest" => {
                return Err(format!(
                    "`--manifest` is not accepted: put `Blastfile` in the working \
                     directory, or set {}",
                    crate::config::MANIFEST_ENV
                ));
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
                    return Err(format!(
                        "unknown argument `{other}` — run 'blastit dev --help'"
                    ));
                } else {
                    return Err(format!(
                        "unknown subcommand '{other}' — run 'blastit dev --help'"
                    ));
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
        no_reload,
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
pub(super) fn parse_check(arguments: &[String]) -> Result<Subcommand<CheckOptions>, String> {
    let (flags, forwarded) = match arguments.iter().position(|argument| argument == "--") {
        Some(index) => (&arguments[..index], &arguments[index + 1..]),
        None => (arguments, &[][..]),
    };
    for argument in forwarded {
        if argument == "--blastit-manifest" || argument.starts_with("--blastit-manifest=") {
            return Err(format!(
                "pytest arguments must not include `--blastit-manifest`: set \
                 {} or put `Blastfile` in the working directory",
                crate::config::MANIFEST_ENV
            ));
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
                return Err(format!(
                    "`--manifest` is not accepted: put `Blastfile` in the working \
                     directory, or set {}",
                    crate::config::MANIFEST_ENV
                ));
            }
            "--preset" => {
                return Err(
                    "`--preset` is only for `blastit` (production); `blastit check` \
                     ignores BLASTIT_PRESET"
                        .to_string(),
                );
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}` — run 'blastit check --help'"
                ));
            }
        }
    }
    if options.dry_run && !options.learn {
        return Err("`--dry-run` is only valid with `--learn`".to_string());
    }
    Ok(Subcommand::Run(options))
}

/// Parses and validates `blastit fmt` (`R19`).
///
/// One file per invocation, so a second positional argument is a mistake named
/// as one. `--help` wins wherever it appears, which is what makes
/// `blastit fmt --check --help` print the help rather than format anything.
///
/// # Errors
///
/// A one-line message naming the argument and the fix. Every rejection is
/// `exit 2` with nothing on stdout.
pub(super) fn parse_fmt(arguments: &[String]) -> Result<Subcommand<FmtOptions>, String> {
    let mut check = false;
    let mut path = None;
    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Subcommand::Help),
            "--check" => check = true,
            "--manifest" => {
                return Err(format!(
                    "`--manifest` is not accepted: put `Blastfile` in the working \
                     directory, or set {}",
                    crate::config::MANIFEST_ENV
                ));
            }
            "--preset" => {
                return Err(
                    "`--preset` is only for `blastit` (production); `blastit fmt` \
                     ignores BLASTIT_PRESET"
                        .to_string(),
                );
            }
            other if other.starts_with('-') => {
                return Err(format!(
                    "unknown argument `{other}` — run 'blastit fmt --help'"
                ));
            }
            other => {
                if path.is_some() {
                    return Err(format!(
                        "`{other}` is a second path: `blastit fmt` formats one file"
                    ));
                }
                path = Some(std::path::PathBuf::from(other));
            }
        }
    }
    Ok(Subcommand::Run(FmtOptions { check, path }))
}

/// Parses and validates `blastit lsp` (`MB.22.3`).
///
/// The session has no argument: the editor writes the protocol on stdin and
/// reads it on stdout, so anything else on the command line is a CLI error
/// (`exit 2`) rather than a flag this server half-understands. `--help` wins
/// wherever it appears, as it does for `fmt`.
///
/// # Errors
///
/// A one-line message naming the argument. Every rejection is `exit 2` with
/// nothing on stdout.
pub(super) fn parse_lsp(arguments: &[String]) -> Result<Subcommand<()>, String> {
    // `--help` wins wherever it appears, which is what makes
    // `blastit lsp --bogus --help` print the help rather than refuse.
    if arguments
        .iter()
        .any(|argument| argument == "-h" || argument == "--help")
    {
        return Ok(Subcommand::Help);
    }
    match arguments.first() {
        None => Ok(Subcommand::Run(())),
        Some(argument) => Err(format!(
            "`{argument}` is not an argument: `blastit lsp` speaks LSP on \
             stdin/stdout and reads no file — run 'blastit lsp --help'"
        )),
    }
}

/// The one flag an operator carries over from `blastit dev` (`MB.17`).
///
/// `blastit --port 4242` is the command an owner actually typed, and the
/// generic "`…` is not `--key=value`" names neither the command that has the
/// flag nor the setting production reads instead. `--port`/`-p` never belongs
/// to this grammar at all — stage 0 carries the port to stage 1 inside the
/// config descriptor — so this sentence is the whole answer.
pub(super) const PORT_IS_DEV_ONLY: &str =
    "`--port` is a `blastit dev` flag; production takes `PORT=<n>` or `--preset`";

/// Whether `argument` is the dev-only port flag, with or without a value.
fn is_dev_only_flag(argument: &str) -> bool {
    let key = argument.split_once('=').map_or(argument, |(key, _)| key);
    matches!(key, "--port" | "-p")
}

/// Parses `--key=value` arguments — and `--preset NAME`, which people type —
/// refusing anything else.
///
/// # Errors
///
/// A sentence naming the argument, with the exit status it deserves: `2` for
/// what a person typed, `1` for a malformed re-exec of this program by itself.
pub(super) fn parse(arguments: &[String]) -> Result<Args, Refusal> {
    let mut args = Args::default();
    // A re-exec of this program by itself — `--stage=1 …`, `--role=…` — is not
    // an operator's typing: its grammar keeps exit `1`, the status the protected
    // CLI matrix pins (`critical_config_and_presets.rs:466-470`). It is decided
    // once, from the spellings `dispatch` reads back out of [`Args`], because the
    // exit status is the only thing that depends on it and `Args` does not know
    // until the whole line has been read.
    let reexec = arguments.iter().any(|argument| {
        matches!(
            argument
                .split_once('=')
                .map_or(argument.as_str(), |(key, _)| key),
            "--stage" | "--role"
        )
    });
    let refuse = move |message: String| {
        if reexec {
            Refusal::internal(message)
        } else {
            Refusal::usage(message)
        }
    };
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        index += 1;
        // E1: `--preset NAME` is how the flag is written in the documentation
        // and how an operator types it; `--preset=NAME` is the spelling every
        // other argument of this program uses. Both are accepted, and this is
        // the only argument that takes a value of its own: the rest name
        // descriptors and roles, where a stray word means a mistake.
        //
        // The name is validated *where it is found*, by the one resolver, so a
        // typo is named ahead of whatever else the line gets wrong:
        // `--preset nope -V` says `nope`, not `-V` (`A-11`).
        if argument == "--preset" {
            let value = arguments
                .get(index)
                .filter(|next| !next.starts_with('-'))
                .ok_or_else(|| refuse("`--preset` wants a preset name".to_string()))?;
            index += 1;
            args.preset = Some(preset::name(value).map_err(refuse)?);
            continue;
        }
        if is_dev_only_flag(argument) {
            return Err(Refusal::usage(PORT_IS_DEV_ONLY.to_string()));
        }
        let Some((key, value)) = argument.split_once('=') else {
            // `A-14`: a word that is not a flag is a command this build does
            // not have, and a bare flag is missing its value — neither is the
            // "`…` is not `--key=value`" sentence, which answered a question
            // nobody asked.
            return Err(Refusal::usage(if argument.starts_with('-') {
                format!("unknown argument `{argument}` — run 'blastit --help'")
            } else {
                format!("unknown subcommand '{argument}' — run 'blastit --help'")
            }));
        };
        // `A-12`: a value that is not what the flag takes is a mistake in what
        // somebody typed, so every one of these is a usage refusal (`2`) — the
        // re-exec lines below are the only ones that keep `1`.
        match key {
            "--stage" => args.stage = Some(number(value, key).map_err(Refusal::usage)?),
            "--role" => args.role = Some(value.to_string()),
            "--config-fd" => {
                args.config_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--dbproxy-pid" => {
                args.dbproxy_pid = Some(number(value, key).map_err(Refusal::usage)?);
            }
            "--dbproxy-events-fd" => {
                args.dbproxy_events_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--dbproxy-stdout-fd" => {
                args.dbproxy_stdout_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--dbproxy-stderr-fd" => {
                args.dbproxy_stderr_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--secrets-fd" => {
                args.secrets_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--events-fd" => {
                args.events_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--ready-fd" => {
                args.ready_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--seccomp" => args.seccomp = Some(flag(value, key).map_err(Refusal::usage)?),
            "--preset" => args.preset = Some(preset::name(value).map_err(refuse)?),
            // The `dev`-only arguments travel from stage 0 to stage 1 and name
            // state an operator never types: the ephemeral root, the bind
            // address, the internal `--learn` descriptors and the reload on/off
            // token (`R20`, `R22`, `R29`, §4.1).
            "--dev-root" => args.dev_root = Some(std::path::PathBuf::from(value)),
            "--dev-host" => args.dev_host = Some(parse_host(value).map_err(Refusal::usage)?),
            "--dev-obs-fd" => {
                args.dev_obs_fd = Some(descriptor(value, key).map_err(Refusal::usage)?)
            }
            "--dev-ctl-fd" => {
                args.dev_ctl_fd = Some(descriptor(value, key).map_err(Refusal::usage)?)
            }
            // `--no-reload` is the user's; this is how stage 0 carries it across
            // the `execve`, beside the other internal `--dev-*` tokens.
            "--dev-reload" => {
                args.dev_reload = Some(on_off(value, key).map_err(Refusal::usage)?);
            }
            "--dev-adopt-fd" => {
                args.dev_adopt_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--dev-policy-fd" => {
                args.dev_policy_fd = Some(descriptor(value, key).map_err(Refusal::usage)?);
            }
            "--dev-reload-since" => {
                args.dev_reload_since = Some(value.parse().map_err(|_| {
                    Refusal::usage(format!(
                        "`--dev-reload-since` is not a unix-nanosecond number: {value}"
                    ))
                })?);
            }
            // Design §2.4: the pinned pair crosses the barrier as numbers. Like
            // the rest of the re-exec grammar these are never typed by a person
            // and are hidden from `--help`. A build without the linked
            // interpreter has no handoff to accept.
            #[cfg(feature = "python")]
            "--py-lib-fd" => args.py_lib_fd = Some(descriptor(value, key).map_err(Refusal::usage)?),
            #[cfg(feature = "python")]
            "--py-shim-fd" => {
                args.py_shim_fd = Some(descriptor(value, key).map_err(Refusal::usage)?)
            }
            other => {
                return Err(Refusal::usage(format!(
                    "unknown argument `{other}` — run 'blastit --help'"
                )));
            }
        }
    }
    // `--seccomp` is how §7.3's `BLASTIT_SECCOMP` reaches the one role that is
    // `execve`'d with an empty environment; every other role is a `fork` of a
    // process that still has the variable.
    if args.seccomp.is_some() && args.role.as_deref() != Some("dbproxy") {
        return Err(refuse(
            "`--seccomp` is only for `--role=dbproxy`".to_string(),
        ));
    }
    // E1: the preset belongs to stage 0, which is the stage that reads the
    // environment. Stage 1 gets the values the preset supplied through the
    // configuration document, the same way it gets every other setting.
    if reexec && args.preset.is_some() {
        return Err(refuse("`--preset` is only for stage 0".to_string()));
    }
    let any_dev = args.dev_root.is_some()
        || args.dev_host.is_some()
        || args.dev_obs_fd.is_some()
        || args.dev_ctl_fd.is_some()
        || args.dev_reload.is_some()
        || args.dev_adopt_fd.is_some()
        || args.dev_policy_fd.is_some()
        || args.dev_reload_since.is_some();
    if any_dev && (args.stage != Some(1) || args.role.is_some()) {
        return Err(refuse(
            "`--dev-*` arguments are only for `--stage=1`".to_string(),
        ));
    }
    if any_dev && args.dev_root.is_none() {
        return Err(refuse("`--dev-*` arguments need `--dev-root`".to_string()));
    }
    // Design §2.4: the pinned Python pair is stage-1 grammar, and a boot that
    // names one descriptor without the other has nothing to verify.
    #[cfg(feature = "python")]
    {
        if (args.py_lib_fd.is_some() || args.py_shim_fd.is_some()) && args.stage != Some(1) {
            return Err(refuse(
                "`--py-lib-fd`/`--py-shim-fd` are only for `--stage=1`".to_string(),
            ));
        }
        if args.py_lib_fd.is_some() != args.py_shim_fd.is_some() {
            return Err(refuse(
                "`--py-lib-fd` and `--py-shim-fd` must be given together".to_string(),
            ));
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

/// A `on`/`off` argument: the spelling the internal reload token uses, so the
/// `--dev-reload=on|off` the design names is what travels (§4.1, §4.6).
fn on_off(value: &str, key: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        other => Err(format!("`{key}` wants on or off, not `{other}`")),
    }
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
