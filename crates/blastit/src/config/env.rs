//! The one place the process environment is read (§4, §5).

use std::collections::BTreeMap;
use std::path::PathBuf;

use blastit_events::Level;

use super::manifest_io::{manifest_dir, manifest_path, manifest_wiring};
use super::parse::{
    parse_bool, parse_client_ip_header, parse_duration, parse_i32, parse_immortalize,
    parse_percent, parse_size, parse_size_usize, parse_trusted_proxies, parse_u8, parse_u16,
    parse_u32, parse_u64, parse_usize, split_list,
};
use super::preset::{PresetChoice, PresetPolicy, Settings, preset_choice};
use super::settings::Config;
use super::types::{LogFormat, SpawnMode, Warning};
use super::{ConfigError, invalid};

impl Config {
    /// Production `blastit`: the process environment's configuration, with the
    /// production log default (`json`) and `BLASTIT_PRESET` honoured (`R10`).
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a variable that does not parse, a manifest name this
    /// build does not know, and a `BLASTIT_SECCOMP=0` that a preset forbids.
    pub fn from_env(preset: Option<&str>) -> Result<Self, ConfigError> {
        Self::from_env_with_format(preset, LogFormat::Json, PresetPolicy::Env)
    }

    /// The same, with the per-command `BLASTIT_LOG_FORMAT` default and an
    /// explicit [`PresetPolicy`] (`R2`, `R3`, `R10`).
    ///
    /// `blastit` defaults to `json`, while `blastit dev` and `blastit check`
    /// default to `pretty`. An explicit `BLASTIT_LOG_FORMAT` wins in all three,
    /// which is why the default is a parameter rather than a table in here. The
    /// policy is a parameter for the same reason: whether a command may read
    /// `BLASTIT_PRESET` is a decision of the command, not of the environment
    /// (`ADR-0005`), and a caller cannot leave it out.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a variable that does not parse, a manifest name this
    /// build does not know, and a `BLASTIT_SECCOMP=0` that a preset forbids.
    // Each field is read from its own variable and may fail on its own, so a
    // single struct literal would have to nest every error path inside it.
    #[allow(clippy::field_reassign_with_default)]
    pub fn from_env_with_format(
        preset: Option<&str>,
        default_log_format: LogFormat,
        policy: PresetPolicy,
    ) -> Result<Self, ConfigError> {
        let (chosen, preset_warnings) = preset_choice(preset, policy)?;
        let settings = Settings { preset: chosen };
        let mut config = Self::default();
        config.log_format = default_log_format;
        config.manifest = manifest_path()?;
        config.manifest_dir = manifest_dir(&config.manifest);
        config.port = settings
            .get("PORT")?
            .map_or(Ok(config.port), |value| parse_u16("PORT", &value))?;
        config.server_port = match settings.get("SERVER_PORT")? {
            Some(value) => parse_u16("SERVER_PORT", &value)?,
            None => config.port,
        };
        // R7/R9: the manifest's `app`/`preload` are fallbacks beneath a *set*
        // environment variable, and a set-but-empty variable wins. The file is
        // read only for the values the environment leaves open; the full
        // manifest is parsed again by the stage that boots the server.
        let app_from_env = std::env::var_os("BLASTIT_APP").is_some();
        let preload_from_env = std::env::var_os("BLASTIT_PRELOAD").is_some();
        let (manifest_app, manifest_preload) = if app_from_env && preload_from_env {
            (None, Vec::new())
        } else {
            manifest_wiring(&config.manifest)?
        };
        config.app = match settings.get("BLASTIT_APP")? {
            Some(value) if value.is_empty() => {
                return Err(invalid(
                    "BLASTIT_APP",
                    "must be `module:attribute`".to_string(),
                ));
            }
            Some(value) => Some(value),
            None => manifest_app,
        };
        config.preload = match settings.get("BLASTIT_PRELOAD")? {
            Some(value) => split_list(&value),
            None => manifest_preload,
        };
        // `ADR-0010` §3: the backend override and the shim directory are read
        // here, in the composition root, and *only* here — stage 0's discovery
        // runs before `blastit_env::build_from_process` strips every
        // `BLASTIT_*` variable across the stage-1 `execve`, and no child ever
        // sees either name again. Both are absolute: a relative path would be
        // resolved against a working directory stage 1 does not share.
        config.libphp = settings
            .get("BLASTIT_LIBPHP")?
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        config.php_shim_dir = settings
            .get("BLASTIT_PHP_SHIM_DIR")?
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        config.script_name = settings
            .get("BLASTIT_SCRIPT_NAME")?
            .filter(|value| !value.is_empty());
        if let Some(value) = settings.get("BLASTIT_LOG_FORMAT")? {
            config.log_format = match value.as_str() {
                "json" => LogFormat::Json,
                "pretty" => LogFormat::Pretty,
                other => {
                    return Err(invalid(
                        "BLASTIT_LOG_FORMAT",
                        format!("unknown format `{other}`"),
                    ));
                }
            };
        }
        if let Some(value) = settings.get("BLASTIT_LOG_LEVEL")? {
            config.log_level = value
                .parse::<Level>()
                .map_err(|error| invalid("BLASTIT_LOG_LEVEL", error.to_string()))?;
        }
        config.uid_base = defaulted(
            settings.get("BLASTIT_UID_BASE")?,
            "BLASTIT_UID_BASE",
            config.uid_base,
            parse_u32,
        )?;
        config.max_workers = defaulted(
            settings.get("BLASTIT_MAX_WORKERS")?,
            "BLASTIT_MAX_WORKERS",
            config.max_workers,
            parse_u32,
        )?;
        config.view_max_workers = settings
            .get("BLASTIT_VIEW_MAX_WORKERS")?
            .map(|value| parse_u32("BLASTIT_VIEW_MAX_WORKERS", &value))
            .transpose()?;
        config.queue_size = defaulted(
            settings.get("BLASTIT_QUEUE_SIZE")?,
            "BLASTIT_QUEUE_SIZE",
            config.queue_size,
            parse_usize,
        )?;
        config.queue_timeout = defaulted(
            settings.get("BLASTIT_QUEUE_TIMEOUT")?,
            "BLASTIT_QUEUE_TIMEOUT",
            config.queue_timeout,
            parse_duration,
        )?;
        config.request_timeout = defaulted(
            settings.get("BLASTIT_REQUEST_TIMEOUT")?,
            "BLASTIT_REQUEST_TIMEOUT",
            config.request_timeout,
            parse_duration,
        )?;
        config.stuck_threads = defaulted(
            settings.get("BLASTIT_STUCK_THREADS")?,
            "BLASTIT_STUCK_THREADS",
            config.stuck_threads,
            parse_u32,
        )?;
        if let Some(value) = settings.get("BLASTIT_VIEW_SPAWN")? {
            config.view_spawn = match value.as_str() {
                "lazy" => SpawnMode::Lazy,
                "eager" => SpawnMode::Eager,
                other => {
                    return Err(invalid(
                        "BLASTIT_VIEW_SPAWN",
                        format!("unknown mode `{other}` (lazy or eager)"),
                    ));
                }
            };
        }
        config.view_idle_timeout = defaulted(
            settings.get("BLASTIT_VIEW_IDLE_TIMEOUT")?,
            "BLASTIT_VIEW_IDLE_TIMEOUT",
            config.view_idle_timeout,
            parse_duration,
        )?;
        config.view_max_requests = defaulted(
            settings.get("BLASTIT_VIEW_MAX_REQUESTS")?,
            "BLASTIT_VIEW_MAX_REQUESTS",
            config.view_max_requests,
            parse_u64,
        )?;
        config.max_view_processes = settings
            .get("BLASTIT_MAX_VIEW_PROCESSES")?
            .map(|value| parse_u32("BLASTIT_MAX_VIEW_PROCESSES", &value))
            .transpose()?;
        config.view_mem_estimate = defaulted(
            settings.get("BLASTIT_VIEW_MEM_ESTIMATE")?,
            "BLASTIT_VIEW_MEM_ESTIMATE",
            config.view_mem_estimate,
            parse_size,
        )?;
        config.mem_high_watermark_percent = settings
            .get("BLASTIT_MEM_HIGH_WATERMARK")?
            .map(|value| parse_percent("BLASTIT_MEM_HIGH_WATERMARK", &value))
            .transpose()?
            .unwrap_or(config.mem_high_watermark_percent);
        config.immortalize = defaulted(
            settings.get("BLASTIT_IMMORTALIZE")?,
            "BLASTIT_IMMORTALIZE",
            config.immortalize,
            parse_immortalize,
        )?;
        config.max_header_bytes = defaulted(
            settings.get("BLASTIT_MAX_HEADER_BYTES")?,
            "BLASTIT_MAX_HEADER_BYTES",
            config.max_header_bytes,
            parse_size_usize,
        )?;
        config.max_body_bytes = defaulted(
            settings.get("BLASTIT_MAX_BODY_BYTES")?,
            "BLASTIT_MAX_BODY_BYTES",
            config.max_body_bytes,
            parse_size,
        )?;
        config.header_timeout = defaulted(
            settings.get("BLASTIT_HEADER_TIMEOUT")?,
            "BLASTIT_HEADER_TIMEOUT",
            config.header_timeout,
            parse_duration,
        )?;
        config.keepalive_timeout = defaulted(
            settings.get("BLASTIT_KEEPALIVE_TIMEOUT")?,
            "BLASTIT_KEEPALIVE_TIMEOUT",
            config.keepalive_timeout,
            parse_duration,
        )?;
        // D5: fair queuing, and the knobs of the queue it turns on.
        config.fair_queuing = defaulted(
            settings.get("BLASTIT_FAIR_QUEUING")?,
            "BLASTIT_FAIR_QUEUING",
            config.fair_queuing,
            parse_bool,
        )?;
        config.fq_outer_prefix_v4 = defaulted(
            settings.get("BLASTIT_FQ_OUTER_PREFIX_V4")?,
            "BLASTIT_FQ_OUTER_PREFIX_V4",
            config.fq_outer_prefix_v4,
            parse_u8,
        )?;
        config.fq_outer_prefix_v6 = defaulted(
            settings.get("BLASTIT_FQ_OUTER_PREFIX_V6")?,
            "BLASTIT_FQ_OUTER_PREFIX_V6",
            config.fq_outer_prefix_v6,
            parse_u8,
        )?;
        config.fq_outer_max = defaulted(
            settings.get("BLASTIT_FQ_OUTER_MAX")?,
            "BLASTIT_FQ_OUTER_MAX",
            config.fq_outer_max,
            parse_usize,
        )?;
        config.fq_inner_max_per_outer = defaulted(
            settings.get("BLASTIT_FQ_INNER_MAX_PER_OUTER")?,
            "BLASTIT_FQ_INNER_MAX_PER_OUTER",
            config.fq_inner_max_per_outer,
            parse_usize,
        )?;
        config.fq_inner_max = defaulted(
            settings.get("BLASTIT_FQ_INNER_MAX")?,
            "BLASTIT_FQ_INNER_MAX",
            config.fq_inner_max,
            parse_usize,
        )?;
        config.fq_burst = defaulted(
            settings.get("BLASTIT_FQ_BURST")?,
            "BLASTIT_FQ_BURST",
            config.fq_burst,
            parse_duration,
        )?;
        config.fq_max_debt = defaulted(
            settings.get("BLASTIT_FQ_MAX_DEBT")?,
            "BLASTIT_FQ_MAX_DEBT",
            config.fq_max_debt,
            parse_duration,
        )?;
        config.fq_debt_ttl = defaulted(
            settings.get("BLASTIT_FQ_DEBT_TTL")?,
            "BLASTIT_FQ_DEBT_TTL",
            config.fq_debt_ttl,
            parse_duration,
        )?;
        // An empty list is a list: `BLASTIT_FQ_INNER_HEADERS=` hashes no
        // header, which is a different queue from the default one.
        if let Some(value) = settings.get("BLASTIT_FQ_INNER_HEADERS")? {
            config.fq_inner_headers = split_list(&value);
        }
        if let Some(value) = settings.get("BLASTIT_FQ_INNER_COOKIES")? {
            config.fq_inner_cookies = split_list(&value);
        }
        config.fq_inner_include_ip = defaulted(
            settings.get("BLASTIT_FQ_INNER_INCLUDE_IP")?,
            "BLASTIT_FQ_INNER_INCLUDE_IP",
            config.fq_inner_include_ip,
            parse_bool,
        )?;
        let (client_ip_header, trusted_proxies, warnings) = client_ip_settings(
            settings.get("BLASTIT_CLIENT_IP_HEADER")?,
            settings.get("BLASTIT_TRUSTED_PROXIES")?,
            settings.get("BLASTIT_TRUST_FORWARDED")?,
            &config.client_ip_header,
            &config.trusted_proxies,
        )?;
        config.client_ip_header = client_ip_header;
        config.trusted_proxies = trusted_proxies;
        // The preset's own notices come first: they are about the boot's shape,
        // the client-IP ones about the variables underneath it.
        config.warnings = preset_warnings;
        config.warnings.extend(warnings);
        if let Some(value) = settings
            .get("BLASTIT_HEALTH_PATH")?
            .filter(|value| !value.is_empty())
        {
            config.health_path = value;
        }
        // Per-proxy caps are named after the manifest's proxies, so a preset
        // cannot supply them; this scans the environment as it always did.
        config.proxy_max_connections = proxy_max_connections()?;
        config.env_strip = settings
            .get("BLASTIT_ENV_STRIP")?
            .filter(|value| !value.is_empty());
        config.seccomp = defaulted(
            settings.get("BLASTIT_SECCOMP")?,
            "BLASTIT_SECCOMP",
            config.seccomp,
            parse_bool,
        )?;
        // R18: a preset deployment may not run without the filters. Outside a
        // preset the emergency value is honoured, loudly.
        if !config.seccomp {
            if let Some(preset) = chosen {
                return Err(invalid(
                    "BLASTIT_SECCOMP",
                    format!(
                        "must not be 0 while preset `{}` is active: remove \
                         BLASTIT_SECCOMP or set BLASTIT_SECCOMP=1",
                        preset.name()
                    ),
                ));
            }
            config.warnings.push(Warning::SeccompDisabled);
        }
        config.graceful_timeout = defaulted(
            settings.get("BLASTIT_GRACEFUL_TIMEOUT")?,
            "BLASTIT_GRACEFUL_TIMEOUT",
            config.graceful_timeout,
            parse_duration,
        )?;
        config.warmup = settings
            .get("BLASTIT_WARMUP")?
            .filter(|value| !value.is_empty());
        config.frame_max = defaulted(
            settings.get("BLASTIT_FRAME_MAX")?,
            "BLASTIT_FRAME_MAX",
            config.frame_max,
            parse_size_usize,
        )?;
        config.backlog = defaulted(
            settings.get("BLASTIT_BACKLOG")?,
            "BLASTIT_BACKLOG",
            config.backlog,
            parse_i32,
        )?;
        config.max_connections = defaulted(
            settings.get("BLASTIT_MAX_CONNECTIONS")?,
            "BLASTIT_MAX_CONNECTIONS",
            config.max_connections,
            parse_usize,
        )?;
        config.validate()?;
        config.preset = chosen.map(PresetChoice::resolve);
        Ok(config)
    }

    /// `blastit dev`: the configuration for the unprivileged single-UID
    /// simulation (`R2`).
    ///
    /// Identical to a production read except in the two ways `dev` promises:
    /// `BLASTIT_LOG_FORMAT` defaults to `pretty`, and `BLASTIT_PRESET` is never
    /// read — no preset, no override warnings, and no
    /// `BLASTIT_SECCOMP=0`-under-a-preset abort (`ADR-0005`).
    ///
    /// # Errors
    ///
    /// The [`ConfigError`]s [`Config::from_env`] reports, except for
    /// `BLASTIT_PRESET`.
    pub fn for_dev() -> Result<Self, ConfigError> {
        Self::from_env_with_format(None, LogFormat::Pretty, PresetPolicy::IgnoreEnv)
    }

    /// `blastit check` and its exact alias `blastit test` (`R3`).
    ///
    /// The same contract as [`Config::for_dev`]: `pretty` by default, and an
    /// exported `BLASTIT_PRESET` is ignored, so a CI run audits the manifest in
    /// front of it rather than a deployment preset (`ADR-0005`).
    ///
    /// # Errors
    ///
    /// The [`ConfigError`]s [`Config::from_env`] reports, except for
    /// `BLASTIT_PRESET`.
    pub fn for_check() -> Result<Self, ConfigError> {
        Self::from_env_with_format(None, LogFormat::Pretty, PresetPolicy::IgnoreEnv)
    }
}

/// The client-IP settings of §8.2 (D5): the header, the peers whose header may
/// be believed, and what to warn about.
///
/// `BLASTIT_TRUST_FORWARDED` is the deprecated predecessor of
/// `BLASTIT_TRUSTED_PROXIES`, and it is still read so that an existing
/// deployment keeps working: `0` means no peer is trusted, and `1` means every
/// peer is (`*`), which is exactly what it used to mean — the last entry of
/// anybody's `X-Forwarded-For`. Either way the operator is told to move on, and
/// a `BLASTIT_TRUSTED_PROXIES` that is also set wins, with a warning saying so.
///
/// No warning is reported here for a value that is not a boolean at all:
/// `parse_bool` rejects it and the error names the variable.
fn client_ip_settings(
    header: Option<String>,
    trusted: Option<String>,
    legacy: Option<String>,
    default_header: &str,
    default_trusted: &[String],
) -> Result<(String, Vec<String>, Vec<Warning>), ConfigError> {
    let header = match header {
        Some(value) => parse_client_ip_header(&value)?,
        None => default_header.to_string(),
    };
    let mut warnings = Vec::new();
    let trusted = match trusted {
        Some(value) => {
            if legacy.is_some() {
                warnings.push(Warning::TrustForwardedIgnored);
            }
            parse_trusted_proxies(&value)?
        }
        None => match legacy {
            Some(value) => {
                let trust = parse_bool("BLASTIT_TRUST_FORWARDED", &value)?;
                warnings.push(if trust {
                    Warning::TrustForwardedAny
                } else {
                    Warning::TrustForwardedNone
                });
                if trust {
                    vec!["*".to_string()]
                } else {
                    Vec::new()
                }
            }
            None => default_trusted.to_vec(),
        },
    };
    Ok((header, trusted, warnings))
}

/// The map key of a proxy name: lower-cased, with `-` folded to `_`, because
/// §8.2 spells the variable `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS`.
pub(super) fn proxy_key(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

/// Reads every `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` in the environment.
fn proxy_max_connections() -> Result<BTreeMap<String, u32>, ConfigError> {
    const PREFIX: &str = "BLASTIT_PROXY_";
    const SUFFIX: &str = "_MAX_CONNECTIONS";
    let mut map = BTreeMap::new();
    for (variable, value) in std::env::vars() {
        let Some(rest) = variable.strip_prefix(PREFIX) else {
            continue;
        };
        let Some(name) = rest.strip_suffix(SUFFIX) else {
            continue;
        };
        if name.is_empty() {
            return Err(invalid(&variable, "the proxy name is empty".to_string()));
        }
        let value = parse_u32(&variable, &value)?;
        map.insert(proxy_key(name), value);
    }
    Ok(map)
}

/// Reads one environment variable, or `None` when it is unset.
pub(super) fn variable(name: &str) -> Result<Option<String>, ConfigError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::NotUnicode {
            variable: name.to_string(),
        }),
    }
}

/// Applies `parse` to a variable only when it is set.
fn defaulted<T, F>(
    value: Option<String>,
    name: &str,
    default: T,
    parse: F,
) -> Result<T, ConfigError>
where
    F: FnOnce(&str, &str) -> Result<T, ConfigError>,
{
    match value {
        Some(value) => parse(name, &value),
        None => Ok(default),
    }
}
