//! The §8.2 configuration: one typed [`Config`] built from the environment.
//!
//! Configuration comes only from environment variables (§5, §8.2), and stage 0
//! is the only stage that reads them: after the `execve` barrier the new image
//! has the *app* environment, in which every `BLASTIT_*` variable has been
//! removed (§4, §5.3). So stage 0 parses the environment once, and hands the
//! whole thing to stage 1 as JSON over a pipe ([`Config::to_json`] /
//! [`Config::from_json`]).
//!
//! The JSON holds **no secret**: it names the variables a proxy owns and the
//! sizes and timeouts of the server, never an upstream credential. Those travel
//! to dbproxy over `secrets.pipe` and are gone from the process image when
//! stage 1 starts (§4).
//!
//! Every error names the variable it came from, never a value (§8.1: a value
//! can be a credential).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use blastit_events::{Level, Lifecycle};
use blastit_frame::Limits;
use serde_json::{Map, Value};
use thiserror::Error;

/// Where init prints its event stream (§8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogFormat {
    /// One JSON object per line, the server default.
    Json,
    /// One coloured, human-readable line, for running blastit by hand.
    Pretty,
}

impl LogFormat {
    /// The `BLASTIT_LOG_FORMAT` spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Pretty => "pretty",
        }
    }
}

/// The privilege mode init detected, as the `boot` event reports it (D6).
///
/// One field of the configuration, not of the environment: stage 0 reads
/// `/proc/self/status` and decides ([`blastit_sys::caps::detect`]), and every
/// later `boot` event — including stage 1's preload `boot` — reports the same
/// mode, so an operator reading the log knows which of the two deployments they
/// are looking at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Privilege {
    /// uid 0: the container's own capability set authorises the boot (§10).
    Root,
    /// An unprivileged uid holding `capabilities` (D6's rootless mode).
    Capabilities {
        /// The names init holds, `cap_...`, as `setcap` spells them.
        capabilities: Vec<String>,
    },
}

impl Privilege {
    /// The `boot` event's `privilege` field: `root` or `capabilities`.
    pub fn mode(&self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Capabilities { .. } => "capabilities",
        }
    }

    /// The capabilities init holds; empty in root mode, where the interesting
    /// set is the container's.
    pub fn capabilities(&self) -> &[String] {
        match self {
            Self::Root => &[],
            Self::Capabilities { capabilities } => capabilities,
        }
    }

    /// Adds the mode and the capability names to a lifecycle event (§8.1, D6).
    ///
    /// The one place both stages build these fields from, so the two `boot`
    /// events cannot drift apart.
    pub fn lifecycle(&self, lifecycle: Lifecycle) -> Lifecycle {
        let lifecycle = lifecycle.with_privilege(self.mode());
        self.capabilities()
            .iter()
            .fold(lifecycle, |lifecycle, name| {
                lifecycle.with_capability(name.clone())
            })
    }

    /// The same, from a mode this crate only reads back (§4's JSON document).
    ///
    /// # Errors
    ///
    /// [`ConfigError::Document`] for a mode that is not one of the two, which is
    /// a document stage 0 did not write.
    pub fn from_json(value: &Value) -> Result<Option<Self>, ConfigError> {
        match value.get("privilege") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(mode)) => match mode.as_str() {
                "root" => Ok(Some(Self::Root)),
                "capabilities" => Ok(Some(Self::Capabilities {
                    capabilities: json_optional_string_list(value, "capabilities")?,
                })),
                other => Err(document(
                    "privilege",
                    format!("unknown mode `{other}` (root or capabilities)"),
                )),
            },
            Some(_) => Err(document(
                "privilege",
                "must be a string or null".to_string(),
            )),
        }
    }
}

/// When a route's view process is created (§2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnMode {
    /// On the first request that routes to it, the default.
    Lazy,
    /// At startup, for every route.
    Eager,
}

impl SpawnMode {
    /// The `BLASTIT_VIEW_SPAWN` spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lazy => "lazy",
            Self::Eager => "eager",
        }
    }
}

/// When §9.2 #7's heap immortalisation runs (`BLASTIT_IMMORTALIZE`).
///
/// Every CPython 3.12–3.14 has a marker that holds its reference counts still —
/// 3.14's is its own, the split word `crate::immortal::marker` writes — so the
/// default runs the pass on every interpreter blastit knows, and skips the ones
/// it does not (32-bit, big-endian, free-threaded, `Py_TRACE_REFS`, or a
/// version outside that range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Immortalize {
    /// Immortalise wherever the marker is known — CPython 3.12 to 3.14, as
    /// `crate::immortal::supported` decides — and skip it everywhere else. The
    /// default.
    Auto,
    /// The same interpreters as [`Immortalize::Auto`], asked for by an operator
    /// rather than assumed: `1` says the pass must run, and the interpreter
    /// still has the last word about whether its marker is known at all.
    On,
    /// Never immortalise, whatever the interpreter.
    Off,
}

impl Immortalize {
    /// The `BLASTIT_IMMORTALIZE` spelling, which is also the stage 1
    /// document's.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

/// One problem with the configuration.
///
/// Every variant names the variable (or JSON field) involved. No variant ever
/// carries a value from the environment: §8.1 forbids logging values that can
/// be credentials, and configuration variables can be.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A variable is set but cannot be parsed.
    #[error("{variable}: {reason}")]
    Invalid {
        /// The `BLASTIT_*` variable (or `PORT`, or `SERVER_PORT`).
        variable: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A variable the process cannot read as text.
    #[error("{variable}: the value is not valid UTF-8")]
    NotUnicode {
        /// The variable name.
        variable: String,
    },
    /// A field is missing or has the wrong shape in the stage 1 document.
    #[error("configuration document: {field}: {reason}")]
    Document {
        /// The JSON field.
        field: &'static str,
        /// What is wrong with it.
        reason: String,
    },
    /// The environment could not be read at all.
    #[error("reading the environment: {0}")]
    Io(#[from] std::io::Error),
}

/// Everything `BLASTIT_*` and `PORT` describe (§8.2).
///
/// The defaults are the ones §8.2 documents; each field names the variable it
/// comes from. `None` means "unset", which is not the same as a zero value.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// `PORT`: the port http is handed, bound by init on `0.0.0.0`.
    pub port: u16,
    /// `SERVER_PORT`: the port the *app* is told the server runs on (the load
    /// balancer's). Not in §8.2; it defaults to `PORT`.
    pub server_port: u16,
    /// `BLASTIT_APP`: the WSGI callable, `module:attr`.
    pub app: Option<String>,
    /// `BLASTIT_PRELOAD`: extra modules imported before the fork.
    pub preload: Vec<String>,
    /// `BLASTIT_MANIFEST`: the manifest path.
    pub manifest: PathBuf,
    /// `BLASTIT_SCRIPT_NAME`: the mount prefix stripped before routing.
    pub script_name: Option<String>,
    /// `BLASTIT_LOG_FORMAT`.
    pub log_format: LogFormat,
    /// `BLASTIT_LOG_LEVEL`: the minimum level printed.
    pub log_level: Level,
    /// `BLASTIT_UID_BASE`: base of the uid layout.
    pub uid_base: u32,
    /// `BLASTIT_MAX_WORKERS`: live request threads across every view.
    pub max_workers: u32,
    /// `BLASTIT_VIEW_MAX_WORKERS`: per-view thread cap; `None` means
    /// `BLASTIT_MAX_WORKERS`.
    pub view_max_workers: Option<u32>,
    /// `BLASTIT_QUEUE_SIZE`.
    pub queue_size: usize,
    /// `BLASTIT_QUEUE_TIMEOUT`.
    pub queue_timeout: Duration,
    /// `BLASTIT_REQUEST_TIMEOUT`.
    pub request_timeout: Duration,
    /// `BLASTIT_STUCK_THREADS`.
    pub stuck_threads: u32,
    /// `BLASTIT_VIEW_SPAWN`.
    pub view_spawn: SpawnMode,
    /// `BLASTIT_VIEW_IDLE_TIMEOUT`.
    pub view_idle_timeout: Duration,
    /// `BLASTIT_VIEW_MAX_REQUESTS`; zero disables the cap.
    pub view_max_requests: u64,
    /// `BLASTIT_MAX_VIEW_PROCESSES`; `None` derives it from memory (C3).
    pub max_view_processes: Option<u32>,
    /// `BLASTIT_VIEW_MEM_ESTIMATE`, in bytes.
    pub view_mem_estimate: u64,
    /// `BLASTIT_MEM_HIGH_WATERMARK`, as a percentage of `MemTotal`.
    pub mem_high_watermark_percent: f64,
    /// `BLASTIT_IMMORTALIZE`: when §9.2 #7's heap immortalisation runs.
    pub immortalize: Immortalize,
    /// `BLASTIT_MAX_HEADER_BYTES`.
    pub max_header_bytes: usize,
    /// `BLASTIT_MAX_BODY_BYTES`.
    pub max_body_bytes: u64,
    /// `BLASTIT_HEADER_TIMEOUT`.
    pub header_timeout: Duration,
    /// `BLASTIT_KEEPALIVE_TIMEOUT`.
    pub keepalive_timeout: Duration,
    /// `BLASTIT_TRUST_FORWARDED`.
    pub trust_forwarded: bool,
    /// `BLASTIT_HEALTH_PATH`.
    pub health_path: String,
    /// `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS`, by lower-cased proxy name.
    pub proxy_max_connections: BTreeMap<String, u32>,
    /// `BLASTIT_ENV_STRIP`.
    pub env_strip: Option<String>,
    /// `BLASTIT_SECCOMP`: the §7.3 per-role filters.
    pub seccomp: bool,
    /// `BLASTIT_GRACEFUL_TIMEOUT`.
    pub graceful_timeout: Duration,
    /// `BLASTIT_WARMUP`: `module:callable` run before the fork.
    pub warmup: Option<String>,
    /// `BLASTIT_FRAME_MAX`: the frame limit of `http.chan`, `view.chan` and
    /// `ctl.chan` (§3.1).
    pub frame_max: usize,
    /// `BLASTIT_BACKLOG`: the TCP listen backlog (§8.2).
    pub backlog: i32,
    /// `BLASTIT_MAX_CONNECTIONS`: client connections http serves at once
    /// (§8.2). http's own default is 1024, and the front end's
    /// `RLIMIT_NOFILE` is sized from it.
    pub max_connections: usize,
    /// The output variable names each proxy produced, by proxy name (§5.1).
    ///
    /// This is the one field that does not come from an environment variable:
    /// stage 0 fills it in from `blastit_env::Prepared::outputs()` after the
    /// environment rewrite, because it names the variables the *app* sees. It
    /// is a list of names, never a value, so it is no secret.
    pub proxy_outputs: BTreeMap<String, Vec<String>>,
    /// The interpreter's `sys.path` when the console script started (D2).    ///
    /// Another field that comes from no environment variable: stage 0 fills it
    /// in from the installation it runs in. A wheel's stage 1 is `execve`'d as
    /// `python -I -m blastit`, and `-I` drops `PYTHONPATH` and the working
    /// directory from `sys.path`, so the entries the user's interpreter had
    /// travel here and stage 1 puts them back before the first import (§4).
    /// Empty for the standalone binary, whose stage 1 embeds CPython and
    /// computes its own path.
    ///
    /// The entries are bytes — `os.fsencode`, because a path that is not valid
    /// UTF-8 is a path like any other — while the document is JSON. Each is
    /// carried one byte per character ([`latin1`]), which is the identity for
    /// the ASCII of every ordinary path.
    pub sys_path: Vec<Vec<u8>>,
    /// D6: the privilege mode init detected, and the capabilities it holds.
    ///
    /// Another field that comes from no environment variable: stage 0 reads
    /// `/proc/self/status` before it prints its first event
    /// ([`blastit_sys::caps::detect`]) and hands the answer to stage 1, so both
    /// `boot` events say which mode the server runs in. `None` only when the
    /// mode could not be read at all, which is a boot that is about to fail.
    pub privilege: Option<Privilege>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 8080,
            server_port: 8080,
            app: None,
            preload: Vec::new(),
            manifest: PathBuf::from("/app/blastit.toml"),
            script_name: None,
            log_format: LogFormat::Json,
            log_level: Level::Info,
            uid_base: 20_000,
            max_workers: 16,
            view_max_workers: None,
            queue_size: 256,
            queue_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            stuck_threads: 4,
            view_spawn: SpawnMode::Lazy,
            view_idle_timeout: Duration::from_secs(600),
            view_max_requests: 0,
            max_view_processes: None,
            view_mem_estimate: 32 * 1024 * 1024,
            mem_high_watermark_percent: 85.0,
            immortalize: Immortalize::Auto,
            max_header_bytes: 64 * 1024,
            max_body_bytes: 100 * 1024 * 1024,
            header_timeout: Duration::from_secs(10),
            keepalive_timeout: Duration::from_secs(75),
            trust_forwarded: true,
            health_path: "/_blastit/health".to_string(),
            proxy_max_connections: BTreeMap::new(),
            env_strip: None,
            seccomp: true,
            graceful_timeout: Duration::from_secs(25),
            warmup: None,
            frame_max: blastit_frame::MAX_FRAME_DEFAULT,
            backlog: 1024,
            max_connections: 1024,
            proxy_outputs: BTreeMap::new(),
            sys_path: Vec::new(),
            privilege: None,
        }
    }
}

impl Config {
    /// Reads the configuration from the process environment (§8.2).
    ///
    /// Every problem is reported one at a time, and every message names the
    /// variable it came from.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a variable that does not parse. Nothing is reported
    /// for an unset variable with a default.
    // Each field is read from its own variable and may fail on its own, so a
    // single struct literal would have to nest every error path inside it.
    #[allow(clippy::field_reassign_with_default)]
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut config = Self::default();
        config.port =
            variable("PORT")?.map_or(Ok(config.port), |value| parse_u16("PORT", &value))?;
        config.server_port = match variable("SERVER_PORT")? {
            Some(value) => parse_u16("SERVER_PORT", &value)?,
            None => config.port,
        };
        config.app = variable("BLASTIT_APP")?.filter(|value| !value.is_empty());
        config.preload = match variable("BLASTIT_PRELOAD")? {
            Some(value) => split_list(&value),
            None => Vec::new(),
        };
        if let Some(value) = variable("BLASTIT_MANIFEST")?.filter(|value| !value.is_empty()) {
            config.manifest = PathBuf::from(value);
        }
        config.script_name = variable("BLASTIT_SCRIPT_NAME")?.filter(|value| !value.is_empty());
        if let Some(value) = variable("BLASTIT_LOG_FORMAT")? {
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
        if let Some(value) = variable("BLASTIT_LOG_LEVEL")? {
            config.log_level = value
                .parse::<Level>()
                .map_err(|error| invalid("BLASTIT_LOG_LEVEL", error.to_string()))?;
        }
        config.uid_base = defaulted(
            variable("BLASTIT_UID_BASE")?,
            "BLASTIT_UID_BASE",
            config.uid_base,
            parse_u32,
        )?;
        config.max_workers = defaulted(
            variable("BLASTIT_MAX_WORKERS")?,
            "BLASTIT_MAX_WORKERS",
            config.max_workers,
            parse_u32,
        )?;
        config.view_max_workers = variable("BLASTIT_VIEW_MAX_WORKERS")?
            .map(|value| parse_u32("BLASTIT_VIEW_MAX_WORKERS", &value))
            .transpose()?;
        config.queue_size = defaulted(
            variable("BLASTIT_QUEUE_SIZE")?,
            "BLASTIT_QUEUE_SIZE",
            config.queue_size,
            parse_usize,
        )?;
        config.queue_timeout = defaulted(
            variable("BLASTIT_QUEUE_TIMEOUT")?,
            "BLASTIT_QUEUE_TIMEOUT",
            config.queue_timeout,
            parse_duration,
        )?;
        config.request_timeout = defaulted(
            variable("BLASTIT_REQUEST_TIMEOUT")?,
            "BLASTIT_REQUEST_TIMEOUT",
            config.request_timeout,
            parse_duration,
        )?;
        config.stuck_threads = defaulted(
            variable("BLASTIT_STUCK_THREADS")?,
            "BLASTIT_STUCK_THREADS",
            config.stuck_threads,
            parse_u32,
        )?;
        if let Some(value) = variable("BLASTIT_VIEW_SPAWN")? {
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
            variable("BLASTIT_VIEW_IDLE_TIMEOUT")?,
            "BLASTIT_VIEW_IDLE_TIMEOUT",
            config.view_idle_timeout,
            parse_duration,
        )?;
        config.view_max_requests = defaulted(
            variable("BLASTIT_VIEW_MAX_REQUESTS")?,
            "BLASTIT_VIEW_MAX_REQUESTS",
            config.view_max_requests,
            parse_u64,
        )?;
        config.max_view_processes = variable("BLASTIT_MAX_VIEW_PROCESSES")?
            .map(|value| parse_u32("BLASTIT_MAX_VIEW_PROCESSES", &value))
            .transpose()?;
        config.view_mem_estimate = defaulted(
            variable("BLASTIT_VIEW_MEM_ESTIMATE")?,
            "BLASTIT_VIEW_MEM_ESTIMATE",
            config.view_mem_estimate,
            parse_size,
        )?;
        config.mem_high_watermark_percent = variable("BLASTIT_MEM_HIGH_WATERMARK")?
            .map(|value| parse_percent("BLASTIT_MEM_HIGH_WATERMARK", &value))
            .transpose()?
            .unwrap_or(config.mem_high_watermark_percent);
        config.immortalize = defaulted(
            variable("BLASTIT_IMMORTALIZE")?,
            "BLASTIT_IMMORTALIZE",
            config.immortalize,
            parse_immortalize,
        )?;
        config.max_header_bytes = defaulted(
            variable("BLASTIT_MAX_HEADER_BYTES")?,
            "BLASTIT_MAX_HEADER_BYTES",
            config.max_header_bytes,
            parse_size_usize,
        )?;
        config.max_body_bytes = defaulted(
            variable("BLASTIT_MAX_BODY_BYTES")?,
            "BLASTIT_MAX_BODY_BYTES",
            config.max_body_bytes,
            parse_size,
        )?;
        config.header_timeout = defaulted(
            variable("BLASTIT_HEADER_TIMEOUT")?,
            "BLASTIT_HEADER_TIMEOUT",
            config.header_timeout,
            parse_duration,
        )?;
        config.keepalive_timeout = defaulted(
            variable("BLASTIT_KEEPALIVE_TIMEOUT")?,
            "BLASTIT_KEEPALIVE_TIMEOUT",
            config.keepalive_timeout,
            parse_duration,
        )?;
        config.trust_forwarded = defaulted(
            variable("BLASTIT_TRUST_FORWARDED")?,
            "BLASTIT_TRUST_FORWARDED",
            config.trust_forwarded,
            parse_bool,
        )?;
        if let Some(value) = variable("BLASTIT_HEALTH_PATH")?.filter(|value| !value.is_empty()) {
            config.health_path = value;
        }
        config.proxy_max_connections = proxy_max_connections()?;
        config.env_strip = variable("BLASTIT_ENV_STRIP")?.filter(|value| !value.is_empty());
        config.seccomp = defaulted(
            variable("BLASTIT_SECCOMP")?,
            "BLASTIT_SECCOMP",
            config.seccomp,
            parse_bool,
        )?;
        config.graceful_timeout = defaulted(
            variable("BLASTIT_GRACEFUL_TIMEOUT")?,
            "BLASTIT_GRACEFUL_TIMEOUT",
            config.graceful_timeout,
            parse_duration,
        )?;
        config.warmup = variable("BLASTIT_WARMUP")?.filter(|value| !value.is_empty());
        config.frame_max = defaulted(
            variable("BLASTIT_FRAME_MAX")?,
            "BLASTIT_FRAME_MAX",
            config.frame_max,
            parse_size_usize,
        )?;
        config.backlog = defaulted(
            variable("BLASTIT_BACKLOG")?,
            "BLASTIT_BACKLOG",
            config.backlog,
            parse_i32,
        )?;
        config.max_connections = defaulted(
            variable("BLASTIT_MAX_CONNECTIONS")?,
            "BLASTIT_MAX_CONNECTIONS",
            config.max_connections,
            parse_usize,
        )?;
        config.validate()?;
        Ok(config)
    }

    /// Checks the combinations of values that only make sense together.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] naming a variable.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.max_workers == 0 {
            return Err(invalid(
                "BLASTIT_MAX_WORKERS",
                "must be at least 1, or no request can ever run".to_string(),
            ));
        }
        if let Some(0) = self.view_max_workers {
            return Err(invalid(
                "BLASTIT_VIEW_MAX_WORKERS",
                "must be at least 1, or the view could never serve a request".to_string(),
            ));
        }
        // The view's `RLIMIT_NPROC` is `view_max_workers + 16` (§7.2); it must
        // leave room for the interpreter's own threads.
        let cap = self.view_worker_cap();
        if cap.checked_add(16).is_none() {
            return Err(invalid(
                "BLASTIT_VIEW_MAX_WORKERS",
                "is too large to size RLIMIT_NPROC from".to_string(),
            ));
        }
        if self.health_path.is_empty() || !self.health_path.starts_with('/') {
            return Err(invalid(
                "BLASTIT_HEALTH_PATH",
                "must be an absolute path".to_string(),
            ));
        }
        if !(0.0..=100.0).contains(&self.mem_high_watermark_percent) {
            return Err(invalid(
                "BLASTIT_MEM_HIGH_WATERMARK",
                "must be a percentage between 0% and 100%".to_string(),
            ));
        }
        Limits::new(self.frame_max)
            .map_err(|error| invalid("BLASTIT_FRAME_MAX", error.to_string()))?;
        if self.backlog < 1 {
            return Err(invalid("BLASTIT_BACKLOG", "must be at least 1".to_string()));
        }
        if self.max_connections < 1 {
            return Err(invalid(
                "BLASTIT_MAX_CONNECTIONS",
                "must be at least 1".to_string(),
            ));
        }
        if let Some(app) = &self.app
            && !app.contains(':')
        {
            return Err(invalid(
                "BLASTIT_APP",
                "must be `module:attribute`".to_string(),
            ));
        }
        if let Some(0) = self.max_view_processes {
            return Err(invalid(
                "BLASTIT_MAX_VIEW_PROCESSES",
                "must be at least 1, or no view process could ever run".to_string(),
            ));
        }
        if self.view_mem_estimate == 0 {
            return Err(invalid(
                "BLASTIT_VIEW_MEM_ESTIMATE",
                "must be at least 1 byte, or the view process cap cannot be derived from \
                 memory (§8 Sizing)"
                    .to_string(),
            ));
        }
        if let Some(warmup) = &self.warmup
            && !warmup.contains(':')
        {
            return Err(invalid(
                "BLASTIT_WARMUP",
                "must be `module:callable`".to_string(),
            ));
        }
        Ok(())
    }

    /// The per-view thread cap: `BLASTIT_VIEW_MAX_WORKERS`, or the global
    /// budget when that is unset (§8.2).
    pub fn view_worker_cap(&self) -> u32 {
        self.view_max_workers.unwrap_or(self.max_workers)
    }

    /// The `RLIMIT_NOFILE` http, the scheduler and dbproxy run with.
    ///
    /// The front end is the greedy one: one descriptor per client connection,
    /// bounded by `BLASTIT_MAX_CONNECTIONS`. The 256 are the listener, the
    /// channels, the log pipes and whatever a runtime keeps for itself; the
    /// scheduler (one descriptor per view process) and dbproxy (one per
    /// connection) stay well inside that.
    ///
    /// `RLIMIT_NPROC` is deliberately **not** set for these three: §7.2's
    /// thread budget exists for the view uids, which are the ones an attacker
    /// gets, and a limit on init's own children would only bite blastit itself.
    pub fn component_nofile(&self) -> u64 {
        self.max_connections as u64 + 256
    }

    /// `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` for `proxy`, or
    /// `BLASTIT_MAX_WORKERS` (§2.2, §8.2).
    pub fn max_connections_for(&self, proxy: &str) -> u32 {
        let key = proxy_key(proxy);
        self.proxy_max_connections
            .get(&key)
            .copied()
            .unwrap_or(self.max_workers)
    }

    /// The frame limits of every channel this process speaks on (§3.1).
    ///
    /// # Errors
    ///
    /// [`ConfigError`] when `BLASTIT_FRAME_MAX` is out of range; validation
    /// already rejects that, so this is a belt-and-braces check.
    pub fn limits(&self) -> Result<Limits, ConfigError> {
        Limits::new(self.frame_max).map_err(|error| invalid("BLASTIT_FRAME_MAX", error.to_string()))
    }

    /// The configuration as a JSON document, for stage 1 (§4).
    ///
    /// It holds no secret: proxy variable *names*, counts, sizes and timeouts.
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("port".into(), Value::from(self.port));
        object.insert("server_port".into(), Value::from(self.server_port));
        object.insert(
            "app".into(),
            self.app.clone().map_or(Value::Null, Value::from),
        );
        object.insert(
            "preload".into(),
            Value::Array(self.preload.iter().cloned().map(Value::from).collect()),
        );
        object.insert(
            "manifest".into(),
            Value::from(self.manifest.to_string_lossy().into_owned()),
        );
        object.insert(
            "script_name".into(),
            self.script_name.clone().map_or(Value::Null, Value::from),
        );
        object.insert("log_format".into(), Value::from(self.log_format.as_str()));
        object.insert("log_level".into(), Value::from(self.log_level.as_str()));
        object.insert("uid_base".into(), Value::from(self.uid_base));
        object.insert("max_workers".into(), Value::from(self.max_workers));
        object.insert(
            "view_max_workers".into(),
            self.view_max_workers.map_or(Value::Null, Value::from),
        );
        object.insert("queue_size".into(), Value::from(self.queue_size as u64));
        object.insert(
            "queue_timeout_ms".into(),
            Value::from(millis(self.queue_timeout)),
        );
        object.insert(
            "request_timeout_ms".into(),
            Value::from(millis(self.request_timeout)),
        );
        object.insert("stuck_threads".into(), Value::from(self.stuck_threads));
        object.insert("view_spawn".into(), Value::from(self.view_spawn.as_str()));
        object.insert(
            "view_idle_timeout_ms".into(),
            Value::from(millis(self.view_idle_timeout)),
        );
        object.insert(
            "view_max_requests".into(),
            Value::from(self.view_max_requests),
        );
        object.insert(
            "max_view_processes".into(),
            self.max_view_processes.map_or(Value::Null, Value::from),
        );
        object.insert(
            "view_mem_estimate".into(),
            Value::from(self.view_mem_estimate),
        );
        object.insert(
            "mem_high_watermark_percent".into(),
            Value::from(self.mem_high_watermark_percent),
        );
        object.insert("immortalize".into(), Value::from(self.immortalize.as_str()));
        object.insert(
            "max_header_bytes".into(),
            Value::from(self.max_header_bytes as u64),
        );
        object.insert("max_body_bytes".into(), Value::from(self.max_body_bytes));
        object.insert(
            "header_timeout_ms".into(),
            Value::from(millis(self.header_timeout)),
        );
        object.insert(
            "keepalive_timeout_ms".into(),
            Value::from(millis(self.keepalive_timeout)),
        );
        object.insert("trust_forwarded".into(), Value::from(self.trust_forwarded));
        object.insert("health_path".into(), Value::from(self.health_path.clone()));
        let connections = self
            .proxy_max_connections
            .iter()
            .map(|(name, cap)| (name.clone(), Value::from(*cap)))
            .collect();
        object.insert("proxy_max_connections".into(), Value::Object(connections));
        object.insert(
            "env_strip".into(),
            self.env_strip.clone().map_or(Value::Null, Value::from),
        );
        object.insert("seccomp".into(), Value::from(self.seccomp));
        object.insert(
            "graceful_timeout_ms".into(),
            Value::from(millis(self.graceful_timeout)),
        );
        object.insert(
            "warmup".into(),
            self.warmup.clone().map_or(Value::Null, Value::from),
        );
        object.insert("frame_max".into(), Value::from(self.frame_max as u64));
        object.insert("backlog".into(), Value::from(self.backlog));
        object.insert(
            "max_connections".into(),
            Value::from(self.max_connections as u64),
        );
        let outputs = self
            .proxy_outputs
            .iter()
            .map(|(name, names)| {
                (
                    name.clone(),
                    Value::Array(names.iter().cloned().map(Value::from).collect()),
                )
            })
            .collect();
        object.insert("proxy_outputs".into(), Value::Object(outputs));
        // D6: the mode init detected, and the capabilities it holds. `root` is
        // written as a plain mode with no list; the rootless case carries the
        // names, never a value of any kind.
        object.insert(
            "privilege".into(),
            match &self.privilege {
                None => Value::Null,
                Some(privilege) => Value::from(privilege.mode()),
            },
        );
        object.insert(
            "capabilities".into(),
            Value::Array(
                self.privilege
                    .as_ref()
                    .map(Privilege::capabilities)
                    .unwrap_or_default()
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "sys_path".into(),
            Value::Array(
                self.sys_path
                    .iter()
                    .map(|entry| Value::from(latin1(entry)))
                    .collect(),
            ),
        );
        Value::Object(object)
    }

    /// Rebuilds the configuration stage 0 wrote with [`Config::to_json`].
    ///
    /// # Errors
    ///
    /// [`ConfigError::Document`] for a field that is missing or has the wrong
    /// type; the message names the field, never a value.
    // Same reason as [`Config::from_env`]: every field has its own failure.
    #[allow(clippy::field_reassign_with_default)]
    pub fn from_json(value: &Value) -> Result<Self, ConfigError> {
        let mut config = Self::default();
        config.port = json_u16(value, "port")?;
        config.server_port = json_u16(value, "server_port")?;
        config.app = json_opt_string(value, "app")?;
        config.preload = json_string_list(value, "preload")?;
        config.manifest = PathBuf::from(json_string(value, "manifest")?);
        config.script_name = json_opt_string(value, "script_name")?;
        config.log_format = match json_string(value, "log_format")?.as_str() {
            "json" => LogFormat::Json,
            "pretty" => LogFormat::Pretty,
            other => return Err(document("log_format", format!("unknown format `{other}`"))),
        };
        config.log_level = json_string(value, "log_level")?
            .parse::<Level>()
            .map_err(|error| document("log_level", error.to_string()))?;
        config.uid_base = json_u32(value, "uid_base")?;
        config.max_workers = json_u32(value, "max_workers")?;
        config.view_max_workers = json_opt_u32(value, "view_max_workers")?;
        config.queue_size = json_usize(value, "queue_size")?;
        config.queue_timeout = json_duration(value, "queue_timeout_ms")?;
        config.request_timeout = json_duration(value, "request_timeout_ms")?;
        config.stuck_threads = json_u32(value, "stuck_threads")?;
        config.view_spawn = match json_string(value, "view_spawn")?.as_str() {
            "lazy" => SpawnMode::Lazy,
            "eager" => SpawnMode::Eager,
            other => return Err(document("view_spawn", format!("unknown mode `{other}`"))),
        };
        config.view_idle_timeout = json_duration(value, "view_idle_timeout_ms")?;
        config.view_max_requests = json_u64(value, "view_max_requests")?;
        config.max_view_processes = json_opt_u32(value, "max_view_processes")?;
        config.view_mem_estimate = json_u64(value, "view_mem_estimate")?;
        config.mem_high_watermark_percent = json_f64(value, "mem_high_watermark_percent")?;
        config.immortalize = match json_string(value, "immortalize")?.as_str() {
            "auto" => Immortalize::Auto,
            "on" => Immortalize::On,
            "off" => Immortalize::Off,
            other => {
                return Err(document(
                    "immortalize",
                    format!("unknown setting `{other}`"),
                ));
            }
        };
        config.max_header_bytes = json_usize(value, "max_header_bytes")?;
        config.max_body_bytes = json_u64(value, "max_body_bytes")?;
        config.header_timeout = json_duration(value, "header_timeout_ms")?;
        config.keepalive_timeout = json_duration(value, "keepalive_timeout_ms")?;
        config.trust_forwarded = json_bool(value, "trust_forwarded")?;
        config.health_path = json_string(value, "health_path")?;
        let connections = value
            .get("proxy_max_connections")
            .and_then(Value::as_object)
            .ok_or_else(|| document("proxy_max_connections", "must be an object".to_string()))?;
        let mut map = BTreeMap::new();
        for (name, cap) in connections {
            let cap = cap
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    document(
                        "proxy_max_connections",
                        "every value must be a u32".to_string(),
                    )
                })?;
            map.insert(name.clone(), cap);
        }
        config.proxy_max_connections = map;
        config.env_strip = json_opt_string(value, "env_strip")?;
        config.seccomp = json_bool(value, "seccomp")?;
        config.graceful_timeout = json_duration(value, "graceful_timeout_ms")?;
        config.warmup = json_opt_string(value, "warmup")?;
        config.frame_max = json_usize(value, "frame_max")?;
        config.backlog = json_i32(value, "backlog")?;
        config.max_connections = json_usize(value, "max_connections")?;
        config.proxy_outputs = json_string_lists(value, "proxy_outputs")?;
        config.privilege = Privilege::from_json(value)?;
        config.sys_path = json_string_list(value, "sys_path")?
            .iter()
            .map(|entry| unlatin1("sys_path", entry))
            .collect::<Result<Vec<Vec<u8>>, ConfigError>>()?;
        Ok(config)
    }
}

/// The private scratch directory of the view running under `uid` (§2.2, §7.2).
pub fn view_dir(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/blastit/views/{uid}"))
}

/// A byte string as the JSON string the configuration document can carry.
///
/// JSON has no byte strings, and a `sys.path` entry is bytes (`os.fsencode`):
/// one byte per character is the identity for the ASCII of every ordinary path
/// and round-trips the rest exactly, where a UTF-8 conversion would refuse a
/// path that is not valid UTF-8.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// The inverse of [`latin1`].
///
/// # Errors
///
/// [`ConfigError::Document`] for a character that is not a byte, which is a
/// document stage 0 did not write.
fn unlatin1(field: &'static str, value: &str) -> Result<Vec<u8>, ConfigError> {
    let mut bytes = Vec::with_capacity(value.len());
    for character in value.chars() {
        match u8::try_from(u32::from(character)) {
            Ok(byte) => bytes.push(byte),
            Err(_) => {
                return Err(document(
                    field,
                    format!("`{character}` is not a byte (the document carries latin-1)"),
                ));
            }
        }
    }
    Ok(bytes)
}

/// Whether `BLASTIT_SECCOMP` asks for the §7.3 filters, as the `boot` event
/// needs it (§8.1).
///
/// [`Config::from_env`] parses the same variable a moment later, but stage 0
/// prints `boot` before it parses anything, so this reads the variable on its
/// own — through the same [`parse_bool`] the rest of the configuration uses, so
/// the two cannot drift apart. A value that is not a boolean at all means on
/// here; [`Config::from_env`] rejects it a moment later, naming the variable.
pub fn seccomp_requested() -> bool {
    let Ok(Some(value)) = variable("BLASTIT_SECCOMP") else {
        return true;
    };
    parse_bool("BLASTIT_SECCOMP", &value).unwrap_or(true)
}

/// Reads one environment variable, or `None` when it is unset.
fn variable(name: &str) -> Result<Option<String>, ConfigError> {
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

/// Splits a comma- or whitespace-separated list, dropping empty entries.
fn split_list(value: &str) -> Vec<String> {
    value
        .split([',', ' ', '\t', '\n'])
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// A one-line error naming the variable.
fn invalid(variable: &str, reason: String) -> ConfigError {
    ConfigError::Invalid {
        variable: variable.to_string(),
        reason,
    }
}

/// A one-line document error naming the field.
fn document(field: &'static str, reason: String) -> ConfigError {
    ConfigError::Document { field, reason }
}

/// Milliseconds as a floating-point number, so sub-second values survive.
fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn parse_u16(name: &str, value: &str) -> Result<u16, ConfigError> {
    value
        .trim()
        .parse::<u16>()
        .map_err(|error| invalid(name, format!("expected a port: {error}")))
}

fn parse_u32(name: &str, value: &str) -> Result<u32, ConfigError> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|error| invalid(name, format!("expected a whole number: {error}")))
}

fn parse_i32(name: &str, value: &str) -> Result<i32, ConfigError> {
    value
        .trim()
        .parse::<i32>()
        .map_err(|error| invalid(name, format!("expected a whole number: {error}")))
}

fn parse_u64(name: &str, value: &str) -> Result<u64, ConfigError> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|error| invalid(name, format!("expected a whole number: {error}")))
}

fn parse_usize(name: &str, value: &str) -> Result<usize, ConfigError> {
    value
        .trim()
        .parse::<usize>()
        .map_err(|error| invalid(name, format!("expected a whole number: {error}")))
}

/// Parses `0`/`1`/`true`/`false`/`yes`/`no`/`on`/`off` (§8.2 uses `0` and
/// `1`).
fn parse_bool(name: &str, value: &str) -> Result<bool, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(invalid(name, format!("expected 0 or 1, not `{other}`"))),
    }
}

/// Parses `auto`, or any [`parse_bool`] spelling, as [`Immortalize`] (§8.2
/// writes the booleans as `1` and `0`).
fn parse_immortalize(name: &str, value: &str) -> Result<Immortalize, ConfigError> {
    let text = value.trim();
    if text.eq_ignore_ascii_case("auto") {
        return Ok(Immortalize::Auto);
    }
    match parse_bool(name, text) {
        Ok(true) => Ok(Immortalize::On),
        Ok(false) => Ok(Immortalize::Off),
        // `parse_bool` names the spellings it knows; `auto` is the one it does
        // not, so the message here is this variable's own.
        Err(_) => Err(invalid(
            name,
            format!("expected auto, 0 or 1, not `{text}`"),
        )),
    }
}

/// Parses `10s`, `500ms`, `1.5m` or a bare number of seconds.
fn parse_duration(name: &str, value: &str) -> Result<Duration, ConfigError> {
    let text = value.trim();
    let (number, scale) = if let Some(rest) = text.strip_suffix("ms") {
        (rest, 0.001_f64)
    } else if let Some(rest) = text.strip_suffix('s') {
        (rest, 1.0)
    } else if let Some(rest) = text.strip_suffix('m') {
        (rest, 60.0)
    } else if let Some(rest) = text.strip_suffix('h') {
        (rest, 3600.0)
    } else {
        (text, 1.0)
    };
    let number = number
        .trim()
        .parse::<f64>()
        .map_err(|error| invalid(name, format!("expected a duration: {error}")))?;
    if !number.is_finite() || number < 0.0 {
        return Err(invalid(
            name,
            "expected a duration that is zero or more".to_string(),
        ));
    }
    seconds(name, number * scale)
}

/// A `Duration` from a number of seconds, refusing values that cannot be one.
///
/// `Duration::from_secs_f64` panics on a value it cannot represent, and the
/// value comes from outside the process, so it is bounded here instead.
fn seconds(name: &str, seconds: f64) -> Result<Duration, ConfigError> {
    const LIMIT: f64 = 1e15;
    if !seconds.is_finite() || !(0.0..=LIMIT).contains(&seconds) {
        return Err(invalid(
            name,
            "expected a duration under about 31 million years".to_string(),
        ));
    }
    Ok(Duration::from_secs_f64(seconds))
}

/// Parses `64 KiB`, `100 MiB`, `1GB` or a bare byte count.
fn parse_size(name: &str, value: &str) -> Result<u64, ConfigError> {
    let text = value.trim();
    let lower = text.to_ascii_lowercase();
    let (number, scale): (&str, f64) = if let Some(rest) = lower.strip_suffix("kib") {
        (rest, 1024.0)
    } else if let Some(rest) = lower.strip_suffix("mib") {
        (rest, 1024.0 * 1024.0)
    } else if let Some(rest) = lower.strip_suffix("gib") {
        (rest, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(rest) = lower.strip_suffix("kb") {
        (rest, 1000.0)
    } else if let Some(rest) = lower.strip_suffix("mb") {
        (rest, 1_000_000.0)
    } else if let Some(rest) = lower.strip_suffix("gb") {
        (rest, 1_000_000_000.0)
    } else if let Some(rest) = lower.strip_suffix('b') {
        (rest, 1.0)
    } else {
        (lower.as_str(), 1.0)
    };
    // The slice above always points into `lower`, so indexing back into `text`
    // is not needed: parse the lower-cased remainder directly.
    let number = number
        .trim()
        .parse::<f64>()
        .map_err(|error| invalid(name, format!("expected a size: {error}")))?;
    if !number.is_finite() || number < 0.0 {
        return Err(invalid(
            name,
            "expected a size that is zero or more".to_string(),
        ));
    }
    let bytes = number * scale;
    if bytes > u64::MAX as f64 {
        return Err(invalid(name, "the size is too large".to_string()));
    }
    Ok(bytes as u64)
}

/// [`parse_size`] narrowed to `usize`, for byte counts held in memory.
fn parse_size_usize(name: &str, value: &str) -> Result<usize, ConfigError> {
    let bytes = parse_size(name, value)?;
    usize::try_from(bytes)
        .map_err(|_| invalid(name, "the size does not fit this platform".to_string()))
}

/// Parses `85%` or a bare number, as a percentage.
fn parse_percent(name: &str, value: &str) -> Result<f64, ConfigError> {
    let text = value.trim();
    let text = text.strip_suffix('%').unwrap_or(text).trim();
    text.parse::<f64>()
        .map_err(|error| invalid(name, format!("expected a percentage: {error}")))
}

/// The map key of a proxy name: lower-cased, with `-` folded to `_`, because
/// §8.2 spells the variable `BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS`.
fn proxy_key(name: &str) -> String {
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

/// One JSON field of the stage 1 document.
fn field<'a>(value: &'a Value, name: &'static str) -> Result<&'a Value, ConfigError> {
    value
        .get(name)
        .ok_or_else(|| document(name, "the field is missing".to_string()))
}

fn json_string(value: &Value, name: &'static str) -> Result<String, ConfigError> {
    field(value, name)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| document(name, "must be a string".to_string()))
}

fn json_opt_string(value: &Value, name: &'static str) -> Result<Option<String>, ConfigError> {
    match field(value, name)? {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(document(name, "must be a string or null".to_string())),
    }
}

fn json_string_list(value: &Value, name: &'static str) -> Result<Vec<String>, ConfigError> {
    let array = field(value, name)?
        .as_array()
        .ok_or_else(|| document(name, "must be an array".to_string()))?;
    let mut out = Vec::with_capacity(array.len());
    for entry in array {
        let entry = entry
            .as_str()
            .ok_or_else(|| document(name, "every entry must be a string".to_string()))?;
        out.push(entry.to_string());
    }
    Ok(out)
}

/// A JSON array of strings, or an empty list when the field is absent or null.
///
/// Used for a field stage 0 only writes in one of its cases (D6's capability
/// names in rootless mode), so a reader does not have to care which.
///
/// # Errors
///
/// [`ConfigError::Document`] when the field is there but is not an array of
/// strings.
fn json_optional_string_list(
    value: &Value,
    name: &'static str,
) -> Result<Vec<String>, ConfigError> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(_) => json_string_list(value, name),
    }
}

/// A JSON object of string lists, for the per-proxy output variable names.
fn json_string_lists(
    value: &Value,
    name: &'static str,
) -> Result<BTreeMap<String, Vec<String>>, ConfigError> {
    let object = field(value, name)?
        .as_object()
        .ok_or_else(|| document(name, "must be an object".to_string()))?;
    let mut out = BTreeMap::new();
    for (key, entry) in object {
        let array = entry
            .as_array()
            .ok_or_else(|| document(name, "every value must be an array".to_string()))?;
        let mut names = Vec::with_capacity(array.len());
        for item in array {
            let item = item
                .as_str()
                .ok_or_else(|| document(name, "every entry must be a string".to_string()))?;
            names.push(item.to_string());
        }
        out.insert(key.clone(), names);
    }
    Ok(out)
}

fn json_u64(value: &Value, name: &'static str) -> Result<u64, ConfigError> {
    field(value, name)?
        .as_u64()
        .ok_or_else(|| document(name, "must be a non-negative whole number".to_string()))
}

fn json_u32(value: &Value, name: &'static str) -> Result<u32, ConfigError> {
    let raw = json_u64(value, name)?;
    u32::try_from(raw).map_err(|_| document(name, "does not fit in 32 bits".to_string()))
}

fn json_opt_u32(value: &Value, name: &'static str) -> Result<Option<u32>, ConfigError> {
    match field(value, name)? {
        Value::Null => Ok(None),
        _ => Ok(Some(json_u32(value, name)?)),
    }
}

fn json_usize(value: &Value, name: &'static str) -> Result<usize, ConfigError> {
    let raw = json_u64(value, name)?;
    usize::try_from(raw).map_err(|_| document(name, "does not fit this platform".to_string()))
}

fn json_i32(value: &Value, name: &'static str) -> Result<i32, ConfigError> {
    let raw = field(value, name)?
        .as_i64()
        .ok_or_else(|| document(name, "must be a whole number".to_string()))?;
    i32::try_from(raw).map_err(|_| document(name, "does not fit in 32 bits".to_string()))
}

fn json_u16(value: &Value, name: &'static str) -> Result<u16, ConfigError> {
    let raw = json_u64(value, name)?;
    u16::try_from(raw).map_err(|_| document(name, "must be a port number".to_string()))
}

fn json_f64(value: &Value, name: &'static str) -> Result<f64, ConfigError> {
    field(value, name)?
        .as_f64()
        .ok_or_else(|| document(name, "must be a number".to_string()))
}

fn json_bool(value: &Value, name: &'static str) -> Result<bool, ConfigError> {
    field(value, name)?
        .as_bool()
        .ok_or_else(|| document(name, "must be true or false".to_string()))
}

fn json_duration(value: &Value, name: &'static str) -> Result<Duration, ConfigError> {
    let millis = json_f64(value, name)?;
    if !millis.is_finite() || millis < 0.0 {
        return Err(document(
            name,
            "must be a number of milliseconds that is zero or more".to_string(),
        ));
    }
    seconds(name, millis / 1000.0).map_err(|_| document(name, "is out of range".to_string()))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::field_reassign_with_default
)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    /// Serialises the tests that read or write the process environment.
    ///
    /// [`Config::from_env`] reads every `BLASTIT_*` variable at once, so a test
    /// that calls it must not run while another test is changing one.
    static ENV: Mutex<()> = Mutex::new(());

    /// The lock described on [`ENV`]; a poisoned lock still runs the next test.
    fn env() -> MutexGuard<'static, ()> {
        ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn durations_accept_the_documented_spellings() {
        assert_eq!(
            parse_duration("BLASTIT_QUEUE_TIMEOUT", "10s").unwrap(),
            Duration::from_secs(10)
        );
        assert_eq!(
            parse_duration("BLASTIT_QUEUE_TIMEOUT", "500ms").unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(
            parse_duration("BLASTIT_QUEUE_TIMEOUT", "1.5m").unwrap(),
            Duration::from_secs(90)
        );
        assert_eq!(
            parse_duration("BLASTIT_QUEUE_TIMEOUT", "0").unwrap(),
            Duration::ZERO
        );
        assert!(parse_duration("BLASTIT_QUEUE_TIMEOUT", "soon").is_err());
    }

    #[test]
    fn sizes_accept_the_documented_spellings() {
        assert_eq!(parse_size("V", "64 KiB").unwrap(), 64 * 1024);
        assert_eq!(parse_size("V", "100MiB").unwrap(), 100 * 1024 * 1024);
        assert_eq!(parse_size("V", "32 MiB").unwrap(), 32 * 1024 * 1024);
        assert_eq!(parse_size("V", "1GB").unwrap(), 1_000_000_000);
        assert_eq!(parse_size("V", "512").unwrap(), 512);
        assert_eq!(parse_size("V", "8 b").unwrap(), 8);
        assert!(parse_size("V", "much").is_err());
    }

    #[test]
    fn percentages_accept_the_documented_spelling() {
        assert_eq!(parse_percent("V", "85%").unwrap(), 85.0);
        assert_eq!(parse_percent("V", "85").unwrap(), 85.0);
        assert!(parse_percent("V", "lots").is_err());
    }

    #[test]
    fn booleans_accept_zero_and_one() {
        assert!(parse_bool("V", "1").unwrap());
        assert!(!parse_bool("V", "0").unwrap());
        assert!(parse_bool("V", "yes").unwrap());
        assert!(!parse_bool("V", "no").unwrap());
        assert!(parse_bool("V", "maybe").is_err());
    }

    #[test]
    fn the_immortalisation_setting_takes_auto_and_the_booleans() {
        assert_eq!(parse_immortalize("V", "auto").unwrap(), Immortalize::Auto);
        assert_eq!(parse_immortalize("V", " AUTO ").unwrap(), Immortalize::Auto);
        assert_eq!(parse_immortalize("V", "1").unwrap(), Immortalize::On);
        assert_eq!(parse_immortalize("V", "on").unwrap(), Immortalize::On);
        assert_eq!(parse_immortalize("V", "true").unwrap(), Immortalize::On);
        assert_eq!(parse_immortalize("V", "0").unwrap(), Immortalize::Off);
        assert_eq!(parse_immortalize("V", "off").unwrap(), Immortalize::Off);
        assert_eq!(parse_immortalize("V", "false").unwrap(), Immortalize::Off);
        let err = parse_immortalize("BLASTIT_IMMORTALIZE", "sometimes").unwrap_err();
        assert!(err.to_string().contains("BLASTIT_IMMORTALIZE"), "{err}");
        assert!(err.to_string().contains("auto, 0 or 1"), "{err}");
    }

    #[test]
    fn the_immortalisation_setting_is_read_from_the_environment() {
        let _env = env();
        // SAFETY: `env()` is held for the whole test, so no other test reads or
        // writes the environment while this one runs.
        unsafe {
            std::env::remove_var("BLASTIT_IMMORTALIZE");
            assert_eq!(
                Config::from_env().unwrap().immortalize,
                Immortalize::Auto,
                "the default is auto"
            );
            for (value, expected) in [
                ("1", Immortalize::On),
                ("on", Immortalize::On),
                ("0", Immortalize::Off),
                ("off", Immortalize::Off),
                ("auto", Immortalize::Auto),
            ] {
                std::env::set_var("BLASTIT_IMMORTALIZE", value);
                assert_eq!(
                    Config::from_env().unwrap().immortalize,
                    expected,
                    "BLASTIT_IMMORTALIZE={value}"
                );
            }
            std::env::set_var("BLASTIT_IMMORTALIZE", "sometimes");
            let err = Config::from_env().unwrap_err();
            assert!(err.to_string().contains("BLASTIT_IMMORTALIZE"), "{err}");
            std::env::remove_var("BLASTIT_IMMORTALIZE");
        }
    }

    #[test]
    fn the_boot_seccomp_flag_uses_the_configuration_spellings() {
        let _env = env();
        // SAFETY: `env()` is held for the whole test, so no other test reads or
        // writes the environment while this one runs; the variable is put back
        // before it returns.
        unsafe {
            // The `boot` event is printed before the configuration is parsed,
            // so the flag is read on its own — but through `parse_bool`, so it
            // cannot accept anything `Config::from_env` would reject.
            for value in ["0", "off", "false", "no", "False", " 0 "] {
                std::env::set_var("BLASTIT_SECCOMP", value);
                assert!(!seccomp_requested(), "{value} means off");
            }
            for value in ["1", "on", "yes", "true", "TRUE"] {
                std::env::set_var("BLASTIT_SECCOMP", value);
                assert!(seccomp_requested(), "{value} means on");
            }
            // Unset, and a value that is not a boolean at all: on here, and
            // `Config::from_env` rejects the second one right after.
            std::env::remove_var("BLASTIT_SECCOMP");
            assert!(seccomp_requested());
            std::env::set_var("BLASTIT_SECCOMP", "maybe");
            assert!(seccomp_requested());
            assert!(parse_bool("BLASTIT_SECCOMP", "maybe").is_err());
            std::env::remove_var("BLASTIT_SECCOMP");
        }
    }

    #[test]
    fn proxy_variable_names_fold_to_one_key() {
        assert_eq!(proxy_key("db"), "db");
        assert_eq!(proxy_key("ANALYTICS"), "analytics");
        assert_eq!(proxy_key("analytics-db"), "analytics_db");
    }

    #[test]
    fn the_json_document_round_trips() {
        let mut config = Config::default();
        config.port = 9000;
        config.app = Some("web.wsgi:application".to_string());
        config.preload = vec!["web.urls".to_string()];
        config.log_format = LogFormat::Pretty;
        config.log_level = Level::Warn;
        config.view_max_workers = Some(4);
        config.max_view_processes = Some(7);
        config.proxy_max_connections.insert("db".to_string(), 3);
        config.max_header_bytes = 4096;
        config.max_body_bytes = 1 << 20;
        config.health_path = "/health".to_string();
        config.warmup = Some("web.warmup:run".to_string());
        config.immortalize = Immortalize::Off;
        config.sys_path = vec![
            b"/app".to_vec(),
            b"/app/.venv/lib/python3.14/site-packages".to_vec(),
            vec![b'/', b'a', 0xff, b'p'],
        ];
        config.privilege = Some(Privilege::Capabilities {
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
        });
        config.validate().unwrap();

        let json = config.to_json();
        let text = serde_json::to_string(&json).unwrap();
        assert!(!text.contains("BLASTIT_"), "{text}");
        assert_eq!(json["immortalize"], serde_json::json!("off"));
        let back = Config::from_json(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn an_immortalisation_setting_the_document_cannot_carry_names_the_field() {
        let mut json = Config::default().to_json();
        assert_eq!(json["immortalize"], serde_json::json!("auto"));
        json["immortalize"] = serde_json::json!("sometimes");
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("immortalize"), "{err}");
    }

    #[test]
    fn a_sys_path_entry_that_is_not_utf8_survives_the_document() {
        let mut config = Config::default();
        // `os.fsencode` bytes that are not a UTF-8 string at all.
        config.sys_path = vec![vec![b'/', b'a', 0xff, b'p'], b"/app".to_vec()];

        let json = config.to_json();
        assert_eq!(json["sys_path"][0], serde_json::json!("/a\u{ff}p"));
        let back = Config::from_json(&json).unwrap();
        assert_eq!(back.sys_path, config.sys_path);
    }

    #[test]
    fn a_sys_path_entry_outside_latin1_is_refused() {
        let mut json = Config::default().to_json();
        json["sys_path"] = serde_json::json!(["/app/\u{20ac}"]);
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("sys_path"), "{err}");
    }

    #[test]
    fn the_privilege_mode_round_trips_and_says_what_it_is() {
        let mut config = Config::default();
        assert_eq!(config.privilege, None, "a default document says nothing");
        let json = config.to_json();
        assert_eq!(json["privilege"], serde_json::Value::Null);
        assert_eq!(json["capabilities"], serde_json::json!([]));

        // Root: a mode, no list.
        config.privilege = Some(Privilege::Root);
        let json = config.to_json();
        assert_eq!(json["privilege"], serde_json::json!("root"));
        assert_eq!(json["capabilities"], serde_json::json!([]));
        let back = Config::from_json(&json).unwrap();
        assert_eq!(back.privilege, Some(Privilege::Root));
        assert_eq!(Privilege::Root.mode(), "root");
        assert!(Privilege::Root.capabilities().is_empty());

        // Rootless: the mode and the names init holds.
        config.privilege = Some(Privilege::Capabilities {
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
        });
        let json = config.to_json();
        assert_eq!(json["privilege"], serde_json::json!("capabilities"));
        assert_eq!(
            json["capabilities"],
            serde_json::json!(["cap_chown", "cap_setuid"])
        );
        let back = Config::from_json(&json).unwrap();
        assert_eq!(back.privilege, config.privilege);
        assert_eq!(
            config.privilege.as_ref().unwrap().mode(),
            "capabilities",
            "the boot event's spelling"
        );
        assert_eq!(
            config.privilege.as_ref().unwrap().capabilities(),
            ["cap_chown", "cap_setuid"]
        );
    }

    #[test]
    fn a_privilege_mode_the_document_cannot_carry_names_the_field() {
        let mut json = Config::default().to_json();
        json["privilege"] = serde_json::json!("sometimes");
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("privilege"), "{err}");
        json["privilege"] = serde_json::json!(1);
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("privilege"), "{err}");
    }

    #[test]
    fn the_privilege_fields_reach_a_lifecycle_event() {
        let privilege = Privilege::Capabilities {
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
        };
        let lifecycle = privilege.lifecycle(Lifecycle::default().with_stage("stage0"));
        assert_eq!(lifecycle.privilege(), Some("capabilities"));
        assert_eq!(lifecycle.capabilities(), ["cap_chown", "cap_setuid"]);
        let root = Privilege::Root.lifecycle(Lifecycle::default().with_stage("stage0"));
        assert_eq!(root.privilege(), Some("root"));
        assert!(root.capabilities().is_empty());
    }

    #[test]
    fn a_document_missing_a_field_names_it() {
        let mut json = Config::default().to_json();
        json.as_object_mut().unwrap().remove("max_workers");
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("max_workers"), "{err}");
    }

    #[test]
    fn a_health_path_must_be_absolute() {
        let mut config = Config::default();
        config.health_path = "health".to_string();
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("BLASTIT_HEALTH_PATH"), "{err}");
    }

    #[test]
    fn an_app_must_be_module_colon_attribute() {
        let mut config = Config::default();
        config.app = Some("web.wsgi".to_string());
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("BLASTIT_APP"), "{err}");
    }

    #[test]
    fn frame_max_is_validated() {
        let mut config = Config::default();
        config.frame_max = 4;
        assert!(config.validate().is_err());
        config.frame_max = blastit_frame::MAX_FRAME_DEFAULT;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn a_view_cap_of_zero_is_refused() {
        // §8 Sizing: a cap of zero could never fork a view process, so every
        // request would time out waiting for one.
        let mut config = Config::default();
        config.max_view_processes = Some(0);
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("BLASTIT_MAX_VIEW_PROCESSES"),
            "{err}"
        );
        config.max_view_processes = Some(1);
        assert!(config.validate().is_ok());
        config.max_view_processes = None;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn a_view_mem_estimate_of_zero_is_refused() {
        // §8 Sizing: the estimate is the divisor of the derived cap.
        let mut config = Config::default();
        config.view_mem_estimate = 0;
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("BLASTIT_VIEW_MEM_ESTIMATE"),
            "{err}"
        );
        config.view_mem_estimate = 1;
        assert!(config.validate().is_ok());
    }
}
