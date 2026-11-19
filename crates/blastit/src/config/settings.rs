//! [`Config`], §8.2's documented defaults, and the derived values the
//! components ask it for.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use blastit_events::Level;
use blastit_frame::{ClientIpConfig, Limits, TrustedProxies};
use blastit_scheduler::{
    FQ_BURST_DEFAULT, FQ_DEBT_TTL_DEFAULT, FQ_INNER_COOKIES_DEFAULT, FQ_INNER_HEADERS_DEFAULT,
    FQ_INNER_MAX_DEFAULT, FQ_INNER_MAX_PER_OUTER_DEFAULT, FQ_MAX_DEBT_DEFAULT,
    FQ_OUTER_MAX_DEFAULT, FQ_OUTER_PREFIX_V4_DEFAULT, FQ_OUTER_PREFIX_V6_DEFAULT, FairConfig,
};
#[cfg(feature = "php")]
use blastit_view_php::discovery::PhpRuntime;

use super::env::proxy_key;
use super::preset::PresetChoice;
use super::types::{Immortalize, LogFormat, Privilege, PythonHandoff, SpawnMode, Warning};
use super::{ConfigError, invalid};

/// §8.2's default for `BLASTIT_TRUSTED_PROXIES`.
///
/// These are the peers [`ClientIpConfig::default`] believes, spelled out here
/// because a [`Config`] field has to be a value the stage 1 document can carry;
/// the `the_default_trusted_proxies_are_the_resolver_s` test keeps the two from
/// drifting apart.
pub const TRUSTED_PROXIES_DEFAULT: &[&str] = &["127.0.0.1", "::1"];

/// §8.2's floor for `BLASTIT_FQ_DEBT_TTL` (D5).
///
/// A debt TTL shorter than this — and shorter than
/// [`blastit_scheduler::FAIR_GC_INTERVAL`] in particular — would be a TTL that
/// expires between two of the queue's own collection sweeps, so a shorter value
/// is read as this one, with a warning at boot.
pub const MIN_DEBT_TTL: Duration = Duration::from_secs(1);

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
    /// The directory holding the manifest, absolute: the base every
    /// `[route.static]` `root` is resolved against (`R2`, `R13`).
    ///
    /// Set once, when the manifest path is discovered, and carried in the
    /// stage-1 document, because the manifest path is relative to the boot
    /// working directory and stage 1 must not re-derive it from its own.
    pub manifest_dir: PathBuf,
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
    /// `BLASTIT_FAIR_QUEUING=on`: order the admission queue by the §2.4 fair
    /// scheduler instead of first-in-first-out (D5).
    ///
    /// `off` — the default — is exactly the FIFO, so the fair queue's knobs
    /// below are only read when it is on.
    pub fair_queuing: bool,
    /// `BLASTIT_FQ_OUTER_PREFIX_V4` (D5).
    pub fq_outer_prefix_v4: u8,
    /// `BLASTIT_FQ_OUTER_PREFIX_V6` (D5).
    pub fq_outer_prefix_v6: u8,
    /// `BLASTIT_FQ_OUTER_MAX` (D5).
    pub fq_outer_max: usize,
    /// `BLASTIT_FQ_INNER_MAX_PER_OUTER` (D5).
    pub fq_inner_max_per_outer: usize,
    /// `BLASTIT_FQ_INNER_MAX` (D5).
    pub fq_inner_max: usize,
    /// `BLASTIT_FQ_BURST` (D5).
    pub fq_burst: Duration,
    /// `BLASTIT_FQ_MAX_DEBT` (D5).
    pub fq_max_debt: Duration,
    /// `BLASTIT_FQ_DEBT_TTL` (D5).
    pub fq_debt_ttl: Duration,
    /// `BLASTIT_FQ_INNER_HEADERS` (D5).
    pub fq_inner_headers: Vec<String>,
    /// `BLASTIT_FQ_INNER_COOKIES` (D5).
    pub fq_inner_cookies: Vec<String>,
    /// `BLASTIT_FQ_INNER_INCLUDE_IP` (D5).
    pub fq_inner_include_ip: bool,
    /// `BLASTIT_CLIENT_IP_HEADER`: the header the client address is read from,
    /// and only from a peer in [`Config::trusted_proxies`] (D5).
    pub client_ip_header: String,
    /// `BLASTIT_TRUSTED_PROXIES`: the peers whose client-IP header may be
    /// believed, as written — IP addresses, CIDR networks or `*` (D5).
    ///
    /// The list is kept as text rather than as a parsed value because the stage
    /// 1 document carries the configuration: [`Config::client_ip`] is what
    /// anything resolves with, and it parses this list the same way on both
    /// sides.
    pub trusted_proxies: Vec<String>,
    /// Notices about the configuration itself, for stage 1 to print once (D5).
    ///
    /// A deprecated variable is translated rather than silently honoured, and a
    /// value that is out of range is clamped rather than refused; this is what
    /// says which happened. The sentences are built by blastit and name
    /// variables, never values (§8.1).
    pub warnings: Vec<Warning>,
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
    /// carried one byte per character (`latin1`), which is the identity for
    /// the ASCII of every ordinary path.
    pub sys_path: Vec<Vec<u8>>,
    /// The working directory `blastit` was launched in (D2, §5.3).
    ///
    /// Another field that comes from no environment variable: stage 0 reads it
    /// (`getcwd`) and stage 1 puts it first on `sys.path`, because that is what
    /// every other WSGI server does with the directory it was started in —
    /// gunicorn, uwsgi and granian all make the working directory importable, so
    /// a project that is not installed (`pip install -e .` never run) serves its
    /// own `wsgi:application` without `PYTHONPATH`. `-I` drops the working
    /// directory from `sys.path` along with `PYTHONPATH`, which is exactly what
    /// this puts back — and only this, so the isolation from *environment*
    /// variables stays.
    ///
    /// Bytes, like [`Config::sys_path`], because a directory is a path and a
    /// path is bytes; `None` when stage 0 could not read the working directory
    /// at all, which is a boot about to fail for other reasons.
    pub cwd: Option<Vec<u8>>,
    /// `BLASTIT_LIBPHP`: the NTS `libphp` a view embedding `entrypoint php` loads
    /// (`ADR-0010` §3).
    ///
    /// An absolute path, and stage 0's only candidate: set, it *is* the
    /// library, and a host where it is not a usable NTS `libphp` is refused
    /// rather than served by a distro copy the operator did not name. Read
    /// here, in the configuration composition root, because
    /// `blastit_env::build_from_process` strips every `BLASTIT_*` variable
    /// across the stage-1 `execve` — the discovery has to happen before that
    /// (`docs/knowledge/LESSONS.md`).
    pub libphp: Option<PathBuf>,
    /// `BLASTIT_PHP_SHIM_DIR`: the directory holding the `libblastit_php*.so`
    /// shims when they are not beside the executable (`MB.3.12`), `dev` and
    /// `check` only (`R8`).
    pub php_shim_dir: Option<PathBuf>,
    /// What stage 0 resolved for the file's `entrypoint php`: the NTS
    /// `libphp`, its inode, its version and the matching shim.
    ///
    /// Another field that comes from no environment variable: [`Config::from_env`]
    /// only carries the settings, while the discovery needs the manifest (does a
    /// route ask for PHP at all?) and the filesystem. Stage 1 reads this to
    /// re-open the recorded inode and load the exact shim (`ADR-0010` §3).
    ///
    /// `#[cfg(feature = "php")]`: the record's type lives in the crate that owns
    /// PHP, and a build without that feature has no worker to hand it to
    /// (`R13`).
    #[cfg(feature = "php")]
    pub php: Option<PhpRuntime>,
    /// What stage 0 resolved for a Python WSGI view: the pinned `libpython` and
    /// shim descriptors, the minor, and the prefix/`site-packages` stage 1
    /// needs (design §2.4).
    ///
    /// Another field that comes from no environment variable: stage 0's
    /// discovery fills it in, and stage 1 re-verifies the two descriptors
    /// against it before anything is loaded. `None` for a PHP-only or
    /// static-only boot, which never searches for an interpreter (§2.1).
    pub python: Option<PythonHandoff>,
    /// D6: the privilege mode init detected, and the capabilities it holds.
    ///
    /// Another field that comes from no environment variable: stage 0 reads
    /// `/proc/self/status` before it prints its first event
    /// ([`blastit_sys::caps::detect`]) and hands the answer to stage 1, so both
    /// `boot` events say which mode the server runs in. `None` only when the
    /// mode could not be read at all, which is a boot that is about to fail.
    pub privilege: Option<Privilege>,
    /// `--preset` / `BLASTIT_PRESET`: the preset in force, when one is (E1).
    ///
    /// The *effective* configuration is what carries the preset's values; this
    /// is the record of which preset they came from, so the `boot` event can
    /// say it and stage 1 can report it without knowing the preset tables.
    pub preset: Option<PresetChoice>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 8080,
            server_port: 8080,
            app: None,
            preload: Vec::new(),
            manifest: PathBuf::from("./Blastfile"),
            manifest_dir: PathBuf::new(),
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
            mem_high_watermark_percent: crate::mem_usage::DEFAULT_WATERMARK_PERCENT,
            immortalize: Immortalize::Auto,
            max_header_bytes: 64 * 1024,
            max_body_bytes: 100 * 1024 * 1024,
            header_timeout: Duration::from_secs(10),
            keepalive_timeout: Duration::from_secs(75),
            fair_queuing: false,
            fq_outer_prefix_v4: FQ_OUTER_PREFIX_V4_DEFAULT,
            fq_outer_prefix_v6: FQ_OUTER_PREFIX_V6_DEFAULT,
            fq_outer_max: FQ_OUTER_MAX_DEFAULT,
            fq_inner_max_per_outer: FQ_INNER_MAX_PER_OUTER_DEFAULT,
            fq_inner_max: FQ_INNER_MAX_DEFAULT,
            fq_burst: FQ_BURST_DEFAULT,
            fq_max_debt: FQ_MAX_DEBT_DEFAULT,
            fq_debt_ttl: FQ_DEBT_TTL_DEFAULT,
            fq_inner_headers: FQ_INNER_HEADERS_DEFAULT
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            fq_inner_cookies: FQ_INNER_COOKIES_DEFAULT
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            fq_inner_include_ip: true,
            client_ip_header: ClientIpConfig::default().header,
            trusted_proxies: TRUSTED_PROXIES_DEFAULT
                .iter()
                .map(|entry| (*entry).to_string())
                .collect(),
            warnings: Vec::new(),
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
            cwd: None,
            libphp: None,
            php_shim_dir: None,
            #[cfg(feature = "php")]
            php: None,
            python: None,
            privilege: None,
            preset: None,
        }
    }
}

impl Config {
    /// The per-view thread cap: `BLASTIT_VIEW_MAX_WORKERS`, or the global
    /// budget when that is unset (§8.2).
    pub fn view_worker_cap(&self) -> u32 {
        self.view_max_workers.unwrap_or(self.max_workers)
    }

    /// The one client-IP resolver, for the front end and the scheduler (D5).
    ///
    /// `BLASTIT_CLIENT_IP_HEADER` is believed only from a peer in
    /// `BLASTIT_TRUSTED_PROXIES`; on every other peer the header is not read at
    /// all. Both components get this same value, so the address the app sees as
    /// `REMOTE_ADDR` and the address fair queuing keys its flows on cannot
    /// disagree.
    ///
    /// An empty `BLASTIT_TRUSTED_PROXIES` trusts nobody — the list is
    /// authoritative, and the default is a documented list,
    /// [`TRUSTED_PROXIES_DEFAULT`].
    ///
    /// The header name and the list were checked by [`Config::validate`] before
    /// anything built a resolver; a value that is broken anyway — which no code
    /// path can produce — resolves like an empty list, which trusts nobody
    /// rather than everybody.
    pub fn client_ip(&self) -> ClientIpConfig {
        ClientIpConfig {
            header: self.client_ip_header.clone(),
            trusted: TrustedProxies::parse(&self.trusted_proxies).unwrap_or_default(),
        }
    }

    /// The scheduler's `BLASTIT_FQ_*` configuration (D5).
    ///
    /// The `BLASTIT_CLIENT_IP_*` settings come in here rather than out of
    /// `BLASTIT_FQ_*`: fair queuing keys its flows on the same client address
    /// the front end reports, so the one resolver is [`Config::client_ip`].
    pub fn fair_config(&self) -> FairConfig {
        FairConfig {
            outer_prefix_v4: self.fq_outer_prefix_v4,
            outer_prefix_v6: self.fq_outer_prefix_v6,
            outer_max: self.fq_outer_max,
            inner_max_per_outer: self.fq_inner_max_per_outer,
            inner_max: self.fq_inner_max,
            burst: self.fq_burst,
            max_debt: self.fq_max_debt,
            debt_ttl: self.fq_debt_ttl,
            inner_headers: self.fq_inner_headers.clone(),
            inner_cookies: self.fq_inner_cookies.clone(),
            inner_include_ip: self.fq_inner_include_ip,
            client_ip: self.client_ip(),
        }
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
}
