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
use blastit_frame::{ClientIpConfig, Limits, TrustedProxies, is_header_name};
use blastit_scheduler::{
    FQ_BURST_DEFAULT, FQ_DEBT_TTL_DEFAULT, FQ_INNER_COOKIES_DEFAULT, FQ_INNER_HEADERS_DEFAULT,
    FQ_INNER_MAX_DEFAULT, FQ_INNER_MAX_PER_OUTER_DEFAULT, FQ_MAX_DEBT_DEFAULT,
    FQ_OUTER_MAX_DEFAULT, FQ_OUTER_PREFIX_V4_DEFAULT, FQ_OUTER_PREFIX_V6_DEFAULT, FairConfig,
};
use serde_json::{Map, Value};
use thiserror::Error;

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

/// A named set of configuration *defaults* (E1).
///
/// A preset is a table of `BLASTIT_*` variable names and the values it supplies
/// for them. Precedence is: the built-in default, then the preset, then every
/// variable that is actually set in the environment (and `PORT`) — a preset
/// never wins over an explicit setting, so `BLASTIT_PRESET=…` is always safe to
/// add to a deployment that already configures blastit by hand.
///
/// The values are blastit's own, so they may be named in the log; the *name* of
/// the preset and the settings it supplied are what the `boot` event reports
/// (§8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    /// The name `--preset=…` and `BLASTIT_PRESET=…` take.
    name: &'static str,
    /// One line for `--help` and the error that lists the known names.
    summary: &'static str,
    /// The variables it sets, in the order they are reported.
    settings: &'static [(&'static str, &'static str)],
    /// What its values assume about init's identity (§2.1).
    ///
    /// Not a setting — the mode is detected, not configured (§2.1) — so an
    /// assumption that does not hold is a warning at boot and never a refusal:
    /// a preset used on the wrong platform still works, it simply describes a
    /// deployment that is not this one.
    assumes: Mode,
}

/// The presets this build knows (E1).
///
/// Two, because two platforms were measured and written up: App Platform (§10)
/// and a Docker Compose stack with Traefik as the ingress (the measurement
/// beside §10). Everything else is a combination of the variables in §8.2, and
/// the README says so rather than inventing a third name.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "digitalocean-paas",
        summary: "DigitalOcean App Platform: root, do-connecting-ip, every peer trusted",
        settings: &[
            ("BLASTIT_CLIENT_IP_HEADER", "do-connecting-ip"),
            ("BLASTIT_TRUSTED_PROXIES", "*"),
            ("BLASTIT_FAIR_QUEUING", "on"),
            ("BLASTIT_LOG_FORMAT", "json"),
        ],
        assumes: Mode::Root,
    },
    Preset {
        name: "compose-traefik",
        summary: "Docker Compose behind Traefik: rootless, X-Forwarded-For, every peer trusted",
        settings: &[
            ("BLASTIT_CLIENT_IP_HEADER", "x-forwarded-for"),
            ("BLASTIT_TRUSTED_PROXIES", "*"),
            ("BLASTIT_FAIR_QUEUING", "on"),
            ("BLASTIT_LOG_FORMAT", "json"),
        ],
        assumes: Mode::Capabilities,
    },
];

impl Preset {
    /// The preset called `name`, or `None`.
    pub fn find(name: &str) -> Option<&'static Self> {
        PRESETS.iter().find(|preset| preset.name == name)
    }

    /// The name.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// The one-line description of a preset, for `--help` and the error that
    /// lists the known names.
    pub const fn summary(self) -> &'static str {
        self.summary
    }

    /// The `BLASTIT_*` variables it supplies, with the values, in the order
    /// §8.2 lists something similar.
    pub const fn settings(self) -> &'static [(&'static str, &'static str)] {
        self.settings
    }

    /// The value this preset supplies for `variable`, if any.
    ///
    /// This is what [`Settings`] falls back to when the environment does not
    /// have the variable.
    pub fn get(self, variable: &str) -> Option<&'static str> {
        self.settings
            .iter()
            .find(|(name, _)| *name == variable)
            .map(|(_, value)| *value)
    }

    /// The warning this preset's values deserve when init is in `mode` (E1).
    ///
    /// `None` when the two agree: a preset used where its assumption holds says
    /// nothing.
    pub const fn assumption_warning(self, mode: Mode) -> Option<Warning> {
        match (self.assumes, mode) {
            (Mode::Root, Mode::Capabilities) => Some(Warning::PresetExpectsRoot),
            (Mode::Capabilities, Mode::Root) => Some(Warning::PresetExpectsRootless),
            _ => None,
        }
    }
}

/// Every known preset's name and summary, for `--help` and for the error an
/// unknown name gets (E1).
pub fn preset_list() -> String {
    PRESETS
        .iter()
        .map(|preset| format!("{}: {}", preset.name(), preset.summary()))
        .collect::<Vec<String>>()
        .join("; ")
}

/// The known presets, one per line and indented, for the usage text (E1).
pub fn preset_help() -> String {
    PRESETS
        .iter()
        .map(|preset| format!("  {}: {}", preset.name(), preset.summary()))
        .collect::<Vec<String>>()
        .join("\n")
}

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
pub struct Privilege {
    /// Which of the two shapes init found itself in.
    pub mode: Mode,
    /// The names init holds, `cap_...`, as `setcap` spells them: empty in root
    /// mode, where the interesting set is the container's.
    pub capabilities: Vec<String>,
    /// Whether init can empty its children's bounding sets (`cap_setpcap`,
    /// D6/M1).
    ///
    /// `false` is not a failure — every other set is still cleared and
    /// `no_new_privs` closes what the bounding set would allow — but the `boot`
    /// event says which of the two happened, in both modes.
    pub bounding_set_dropped: bool,
}

/// Which shape init found itself in (D6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// uid 0: the container's own capability set authorises the boot (§10).
    Root,
    /// An unprivileged uid holding [`Privilege::capabilities`] (D6's rootless
    /// mode).
    Capabilities,
}

impl Mode {
    /// The `boot` event's `privilege` field: `root` or `capabilities`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Capabilities => "capabilities",
        }
    }
}

impl Privilege {
    /// Adds the mode, the capability names and the bounding-set fact to a
    /// lifecycle event (§8.1, D6).
    ///
    /// The one place both stages build these fields from, so the two `boot`
    /// events cannot drift apart.
    pub fn lifecycle(&self, lifecycle: Lifecycle) -> Lifecycle {
        let lifecycle = lifecycle
            .with_privilege(self.mode.as_str())
            .with_bounding_set(self.bounding_set_dropped);
        self.capabilities.iter().fold(lifecycle, |lifecycle, name| {
            lifecycle.with_capability(name.clone())
        })
    }

    /// The same, from the document stage 0 wrote (§4).
    ///
    /// # Errors
    ///
    /// [`ConfigError::Document`] for a mode that is not one of the two, which is
    /// a document stage 0 did not write.
    pub fn from_json(value: &Value) -> Result<Option<Self>, ConfigError> {
        let mode = match value.get("privilege") {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::String(mode)) => match mode.as_str() {
                "root" => Mode::Root,
                "capabilities" => Mode::Capabilities,
                other => {
                    return Err(document(
                        "privilege",
                        format!("unknown mode `{other}` (root or capabilities)"),
                    ));
                }
            },
            Some(_) => {
                return Err(document(
                    "privilege",
                    "must be a string or null".to_string(),
                ));
            }
        };
        let capabilities = match mode {
            Mode::Capabilities => json_optional_string_list(value, "capabilities")?,
            Mode::Root => Vec::new(),
        };
        Ok(Some(Self {
            mode,
            capabilities,
            bounding_set_dropped: json_optional_bool(value, "bounding_set")?.unwrap_or(true),
        }))
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

/// A notice about the configuration, for stage 1 to print once (§8.1, D5, E1).
///
/// Three things are worth a line at boot: a variable that is still read but must
/// not be used any more, a value blastit had to adjust to something usable, and a
/// preset whose assumptions do not hold for this boot. The first two are
/// decisions stage 0 made while parsing, so the stage 1 document carries the
/// token and stage 1 prints the sentence: the document describes the
/// configuration, and the wording of a log line belongs with the log. The third
/// is printed by stage 0 itself ([`Config::preset_assumption_warning`]), because
/// a boot that fails before stage 1 must still say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    /// `BLASTIT_TRUST_FORWARDED` was set to a true value, and is read as
    /// `BLASTIT_TRUSTED_PROXIES=*` — which is what it always meant: the last
    /// entry of anybody's `X-Forwarded-For`.
    TrustForwardedAny,
    /// `BLASTIT_TRUST_FORWARDED` was set to a false value, and is read as an
    /// empty `BLASTIT_TRUSTED_PROXIES` — no peer is trusted, as before.
    TrustForwardedNone,
    /// `BLASTIT_TRUST_FORWARDED` was set as well as `BLASTIT_TRUSTED_PROXIES`,
    /// and is ignored: the new variable is the one that configures the
    /// resolver.
    TrustForwardedIgnored,
    /// `BLASTIT_FQ_OUTER_PREFIX_V4` was longer than an IPv4 address.
    OuterPrefixV4Clamped,
    /// `BLASTIT_FQ_OUTER_PREFIX_V6` was longer than an IPv6 address.
    OuterPrefixV6Clamped,
    /// `BLASTIT_FQ_OUTER_MAX` was zero.
    OuterMaxClamped,
    /// `BLASTIT_FQ_INNER_MAX_PER_OUTER` was zero.
    InnerMaxPerOuterClamped,
    /// `BLASTIT_FQ_INNER_MAX` was zero.
    InnerMaxClamped,
    /// `BLASTIT_FQ_DEBT_TTL` was shorter than a second.
    DebtTtlFloored,
    /// `--preset` and `BLASTIT_PRESET` name different presets (E1).
    PresetFlagWins,
    /// A preset whose values assume root was used where init has no
    /// capabilities (E1, §10).
    PresetExpectsRoot,
    /// A preset whose values assume a rootless deployment was used where init
    /// runs as root (E1, D6).
    PresetExpectsRootless,
}

impl Warning {
    /// The `snake_case` token the stage 1 document carries.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TrustForwardedAny => "trust_forwarded_any",
            Self::TrustForwardedNone => "trust_forwarded_none",
            Self::TrustForwardedIgnored => "trust_forwarded_ignored",
            Self::OuterPrefixV4Clamped => "outer_prefix_v4_clamped",
            Self::OuterPrefixV6Clamped => "outer_prefix_v6_clamped",
            Self::OuterMaxClamped => "outer_max_clamped",
            Self::InnerMaxPerOuterClamped => "inner_max_per_outer_clamped",
            Self::InnerMaxClamped => "inner_max_clamped",
            Self::DebtTtlFloored => "debt_ttl_floored",
            Self::PresetFlagWins => "preset_flag_wins",
            Self::PresetExpectsRoot => "preset_expects_root",
            Self::PresetExpectsRootless => "preset_expects_rootless",
        }
    }

    /// The token back to a warning.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Document`] for a token this build does not know, which is
    /// a document stage 0 did not write.
    pub fn parse(token: &str) -> Result<Self, ConfigError> {
        match token {
            "trust_forwarded_any" => Ok(Self::TrustForwardedAny),
            "trust_forwarded_none" => Ok(Self::TrustForwardedNone),
            "trust_forwarded_ignored" => Ok(Self::TrustForwardedIgnored),
            "outer_prefix_v4_clamped" => Ok(Self::OuterPrefixV4Clamped),
            "outer_prefix_v6_clamped" => Ok(Self::OuterPrefixV6Clamped),
            "outer_max_clamped" => Ok(Self::OuterMaxClamped),
            "inner_max_per_outer_clamped" => Ok(Self::InnerMaxPerOuterClamped),
            "inner_max_clamped" => Ok(Self::InnerMaxClamped),
            "debt_ttl_floored" => Ok(Self::DebtTtlFloored),
            "preset_flag_wins" => Ok(Self::PresetFlagWins),
            "preset_expects_root" => Ok(Self::PresetExpectsRoot),
            "preset_expects_rootless" => Ok(Self::PresetExpectsRootless),
            other => Err(document("warnings", format!("unknown token `{other}`"))),
        }
    }

    /// The line to print for this warning (§8.1).
    ///
    /// It names the variables and never a value, so an operator can act on it
    /// without the log saying what they configured.
    pub const fn message(self) -> &'static str {
        match self {
            Self::TrustForwardedAny => {
                "BLASTIT_TRUST_FORWARDED is deprecated (D5): it is read as \
                 BLASTIT_TRUSTED_PROXIES=*, which trusts every peer, as it always did. \
                 Use BLASTIT_CLIENT_IP_HEADER with BLASTIT_TRUSTED_PROXIES"
            }
            Self::TrustForwardedNone => {
                "BLASTIT_TRUST_FORWARDED is deprecated (D5): it is read as an empty \
                 BLASTIT_TRUSTED_PROXIES, which trusts no peer, as it always did. \
                 Use BLASTIT_CLIENT_IP_HEADER with BLASTIT_TRUSTED_PROXIES"
            }
            Self::TrustForwardedIgnored => {
                "BLASTIT_TRUST_FORWARDED is deprecated (D5) and is ignored because \
                 BLASTIT_TRUSTED_PROXIES is set: remove it"
            }
            Self::OuterPrefixV4Clamped => {
                "BLASTIT_FQ_OUTER_PREFIX_V4 is longer than an IPv4 address, and is read \
                 as 32"
            }
            Self::OuterPrefixV6Clamped => {
                "BLASTIT_FQ_OUTER_PREFIX_V6 is longer than an IPv6 address, and is read \
                 as 128"
            }
            Self::OuterMaxClamped => {
                "BLASTIT_FQ_OUTER_MAX is 0, and is read as 1: every client would share \
                 the overflow flow otherwise"
            }
            Self::InnerMaxPerOuterClamped => {
                "BLASTIT_FQ_INNER_MAX_PER_OUTER is 0, and is read as 1: every client of \
                 an outer flow would share its overflow flow otherwise"
            }
            Self::InnerMaxClamped => {
                "BLASTIT_FQ_INNER_MAX is 0, and is read as 1: every client would share \
                 the overflow flow otherwise"
            }
            Self::DebtTtlFloored => {
                "BLASTIT_FQ_DEBT_TTL is shorter than a second, and is read as 1s: a \
                 flow that owes time is remembered at least that long"
            }
            Self::PresetFlagWins => {
                "BLASTIT_PRESET is set and names a different preset than --preset: the \
                 command line wins, and the boot event says which preset is in force"
            }
            Self::PresetExpectsRoot => {
                "this preset is for a platform that starts blastit as root, and this boot \
                 has no capabilities: App Platform cannot grant them to a non-root user \
                 (§10), so a preset written for it needs a root start"
            }
            Self::PresetExpectsRootless => {
                "this preset is for a rootless deployment (D6), and this boot runs as \
                 root: the settings are the same, but init holds far more authority than \
                 the preset assumes — start it as an unprivileged user with the \
                 capabilities it needs"
            }
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

/// The preset in force, and what the configuration did with it (E1).
///
/// The boot event reports all three: the name, the settings the preset supplied,
/// and the names of the settings an explicit variable overrode. The preset's own
/// values are blastit's configuration — §8.2's tables, not a secret — while an
/// overriding value came from the environment, which §8.1 keeps out of the log
/// whether or not it looks sensitive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetChoice {
    /// The preset's name, as `--preset` or `BLASTIT_PRESET` named it.
    pub name: String,
    /// The settings the preset supplied that are in force: the `BLASTIT_*`
    /// variable name and the value the *preset* gave it.
    ///
    /// A map rather than a list, so the document and the event agree on one
    /// order (the names'), and a reader can look a setting up by name.
    pub supplied: BTreeMap<String, String>,
    /// The settings the preset supplied that an explicit variable overrode, by
    /// name — the value that won is never reported.
    pub overridden: Vec<String>,
}

impl PresetChoice {
    /// What the process environment did to `preset` (E1).
    ///
    /// Reads the environment to tell an override from a value the preset still
    /// supplies, so this belongs to stage 0 — the only stage whose environment
    /// is the operator's (§4).
    fn resolve(preset: &'static Preset) -> Self {
        let mut supplied = BTreeMap::new();
        let mut overridden = Vec::new();
        for (name, value) in preset.settings() {
            // A variable that is *set* wins even when it is empty: `BLASTIT_`
            // variables have set-but-empty meanings (`BLASTIT_TRUSTED_PROXIES=`
            // trusts nobody), and they are not the same as unset.
            if std::env::var_os(name).is_some() {
                overridden.push((*name).to_string());
            } else {
                supplied.insert((*name).to_string(), (*value).to_string());
            }
        }
        Self {
            name: preset.name().to_string(),
            supplied,
            overridden,
        }
    }

    /// Adds this preset to a `boot` (or `ready`) event's payload (§8.1).
    pub fn lifecycle(&self, lifecycle: Lifecycle) -> Lifecycle {
        let mut lifecycle = lifecycle.with_preset(self.name.clone());
        for (name, value) in &self.supplied {
            lifecycle = lifecycle.with_preset_setting(name.clone(), value.clone());
        }
        for name in &self.overridden {
            lifecycle = lifecycle.with_preset_override(name.clone());
        }
        lifecycle
    }
}

impl Config {
    /// The notice a preset's own assumptions deserve, given the mode init
    /// detected (E1, §2.1).
    ///
    /// `None` when the two agree, and when there is no preset at all. The value
    /// of a preset is its set of defaults, but each set rests on a platform that
    /// hands init a particular identity, and a deployment using a preset where
    /// that does not hold is a deployment whose assumptions are wrong in a way
    /// it cannot see from the outside. It is a warning and never a refusal —
    /// root in a rootless deployment still works, it is simply more authority
    /// than the preset asked for.
    ///
    /// Stage 0 asks this and prints the line itself, instead of carrying the
    /// token to stage 1 the way the other notices are carried: a boot that fails
    /// on the manifest, on the environment rewrite or on a view socket never
    /// reaches stage 1, and "this preset was written for the wrong kind of
    /// platform" is exactly the thing to know about *that* boot.
    pub fn preset_assumption_warning(&self, mode: Mode) -> Option<Warning> {
        let choice = self.preset.as_ref()?;
        Preset::find(&choice.name).and_then(|preset| preset.assumption_warning(mode))
    }
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
    /// carried one byte per character ([`latin1`]), which is the identity for
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
            privilege: None,
            preset: None,
        }
    }
}

impl Config {
    /// Reads the configuration from the process environment (§8.2).
    ///
    /// `preset` is `--preset`'s value when the command line carried one. It
    /// wins over `BLASTIT_PRESET`, because the command line is the more
    /// explicit of the two — the flag is typed at the process that is being
    /// started, the variable may well come from an image or an orchestrator —
    /// and the `boot` event names the preset it settled on, so a flag that
    /// disagreed with the variable is visible in the log rather than silent.
    /// Both are *defaults*: every variable that is actually set (and `PORT`)
    /// wins over whatever the preset supplies.
    ///
    /// Every problem is reported one at a time, and every message names the
    /// variable it came from.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a variable that does not parse, and for a preset
    /// name this build does not know. Nothing is reported for an unset variable
    /// with a default.
    // Each field is read from its own variable and may fail on its own, so a
    // single struct literal would have to nest every error path inside it.
    #[allow(clippy::field_reassign_with_default)]
    pub fn from_env(preset: Option<&str>) -> Result<Self, ConfigError> {
        let (chosen, preset_warnings) = preset_choice(preset)?;
        let settings = Settings { preset: chosen };
        let mut config = Self::default();
        config.port = settings
            .get("PORT")?
            .map_or(Ok(config.port), |value| parse_u16("PORT", &value))?;
        config.server_port = match settings.get("SERVER_PORT")? {
            Some(value) => parse_u16("SERVER_PORT", &value)?,
            None => config.port,
        };
        config.app = settings
            .get("BLASTIT_APP")?
            .filter(|value| !value.is_empty());
        config.preload = match settings.get("BLASTIT_PRELOAD")? {
            Some(value) => split_list(&value),
            None => Vec::new(),
        };
        if let Some(value) = settings
            .get("BLASTIT_MANIFEST")?
            .filter(|value| !value.is_empty())
        {
            config.manifest = PathBuf::from(value);
        }
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

    /// Checks the combinations of values that only make sense together, and
    /// clamps the ones that have a usable nearest value (§8.2, D5).
    ///
    /// # Errors
    ///
    /// [`ConfigError`] naming a variable.
    pub fn validate(&mut self) -> Result<(), ConfigError> {
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
        // D5: the fair queue's knobs are clamped to something usable rather than
        // refused, the way the reference implementation does it — a /33 prefix is
        // a /32, and a table that holds nothing is a table that holds one — and
        // the operator is told once at boot.
        self.clamp_fair_queue();
        // A name that can never match a request is a setting that does nothing:
        // the fair queue's header and cookie lists are tokens (§3.1), so a name
        // with a space, a colon or a control byte in it is refused rather than
        // hashed into a key nothing will ever produce.
        for (variable, kind, names) in [
            ("BLASTIT_FQ_INNER_HEADERS", "header", &self.fq_inner_headers),
            ("BLASTIT_FQ_INNER_COOKIES", "cookie", &self.fq_inner_cookies),
        ] {
            if names.iter().any(|name| !is_header_name(name)) {
                return Err(invalid(
                    variable,
                    format!("every entry must be a {kind} name, never a value"),
                ));
            }
        }
        if !is_header_name(&self.client_ip_header) {
            return Err(invalid(
                "BLASTIT_CLIENT_IP_HEADER",
                "must be a header name, without its colon".to_string(),
            ));
        }
        if let Err(reason) = check_trusted_proxies(&self.trusted_proxies) {
            return Err(invalid("BLASTIT_TRUSTED_PROXIES", reason));
        }
        Ok(())
    }

    /// The per-view thread cap: `BLASTIT_VIEW_MAX_WORKERS`, or the global
    /// budget when that is unset (§8.2).
    pub fn view_worker_cap(&self) -> u32 {
        self.view_max_workers.unwrap_or(self.max_workers)
    }

    /// Brings the `BLASTIT_FQ_*` knobs into the range the queue can use (D5).
    ///
    /// Each of these has an obvious nearest usable value — a prefix cannot be
    /// longer than the address it masks, a table cannot hold zero flows, and a
    /// debt TTL shorter than a second is a TTL that expires between two checks
    /// — so a value out of range is adjusted, not refused: a server that will
    /// not start over a prefix length is worse than one that starts and says
    /// what it did ([`Config::warnings`], printed once at boot).
    fn clamp_fair_queue(&mut self) {
        if self.fq_outer_prefix_v4 > 32 {
            self.fq_outer_prefix_v4 = 32;
            self.warnings.push(Warning::OuterPrefixV4Clamped);
        }
        if self.fq_outer_prefix_v6 > 128 {
            self.fq_outer_prefix_v6 = 128;
            self.warnings.push(Warning::OuterPrefixV6Clamped);
        }
        for (value, warning) in [
            (&mut self.fq_outer_max, Warning::OuterMaxClamped),
            (
                &mut self.fq_inner_max_per_outer,
                Warning::InnerMaxPerOuterClamped,
            ),
            (&mut self.fq_inner_max, Warning::InnerMaxClamped),
        ] {
            if *value == 0 {
                *value = 1;
                self.warnings.push(warning);
            }
        }
        if self.fq_debt_ttl < MIN_DEBT_TTL {
            self.fq_debt_ttl = MIN_DEBT_TTL;
            self.warnings.push(Warning::DebtTtlFloored);
        }
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
        object.insert("fair_queuing".into(), Value::from(self.fair_queuing));
        object.insert(
            "fq_outer_prefix_v4".into(),
            Value::from(self.fq_outer_prefix_v4),
        );
        object.insert(
            "fq_outer_prefix_v6".into(),
            Value::from(self.fq_outer_prefix_v6),
        );
        object.insert("fq_outer_max".into(), Value::from(self.fq_outer_max as u64));
        object.insert(
            "fq_inner_max_per_outer".into(),
            Value::from(self.fq_inner_max_per_outer as u64),
        );
        object.insert("fq_inner_max".into(), Value::from(self.fq_inner_max as u64));
        object.insert("fq_burst_ms".into(), Value::from(millis(self.fq_burst)));
        object.insert(
            "fq_max_debt_ms".into(),
            Value::from(millis(self.fq_max_debt)),
        );
        object.insert(
            "fq_debt_ttl_ms".into(),
            Value::from(millis(self.fq_debt_ttl)),
        );
        object.insert(
            "fq_inner_headers".into(),
            Value::Array(
                self.fq_inner_headers
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "fq_inner_cookies".into(),
            Value::Array(
                self.fq_inner_cookies
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "fq_inner_include_ip".into(),
            Value::from(self.fq_inner_include_ip),
        );
        object.insert(
            "client_ip_header".into(),
            Value::from(self.client_ip_header.clone()),
        );
        object.insert(
            "trusted_proxies".into(),
            Value::Array(
                self.trusted_proxies
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "warnings".into(),
            Value::Array(
                self.warnings
                    .iter()
                    .map(|warning| Value::from(warning.as_str()))
                    .collect(),
            ),
        );
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
        // D6: the mode init detected, the capabilities it holds, and whether
        // the children's bounding set can be emptied. `root` is written as a
        // plain mode with no list; the rootless case carries the names, never a
        // value of any kind.
        object.insert(
            "privilege".into(),
            match &self.privilege {
                None => Value::Null,
                Some(privilege) => Value::from(privilege.mode.as_str()),
            },
        );
        object.insert(
            "capabilities".into(),
            Value::Array(
                self.privilege
                    .as_ref()
                    .map(|privilege| privilege.capabilities.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "bounding_set".into(),
            match self
                .privilege
                .as_ref()
                .map(|privilege| privilege.bounding_set_dropped)
            {
                None => Value::Null,
                Some(dropped) => Value::from(dropped),
            },
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
        // D2: the directory `blastit` was started in, in bytes like `sys.path`
        // — stage 1 puts it first on `sys.path`, which is what makes a project
        // that is not installed importable without `PYTHONPATH` (§5.3).
        object.insert(
            "cwd".into(),
            match &self.cwd {
                None => Value::Null,
                Some(dir) => Value::from(latin1(dir)),
            },
        );
        // E1: which preset is in force, what it supplied and what an explicit
        // variable overrode. Every value here is one of blastit's own, so the
        // document stays free of the operator's environment.
        object.insert(
            "preset".into(),
            match &self.preset {
                None => Value::Null,
                Some(choice) => {
                    let mut preset = Map::new();
                    preset.insert("name".into(), Value::from(choice.name.clone()));
                    preset.insert(
                        "supplied".into(),
                        Value::Object(
                            choice
                                .supplied
                                .iter()
                                .map(|(name, value)| (name.clone(), Value::from(value.clone())))
                                .collect(),
                        ),
                    );
                    preset.insert(
                        "overridden".into(),
                        Value::Array(choice.overridden.iter().cloned().map(Value::from).collect()),
                    );
                    Value::Object(preset)
                }
            },
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
        config.fair_queuing = json_bool(value, "fair_queuing")?;
        config.fq_outer_prefix_v4 = json_u8(value, "fq_outer_prefix_v4")?;
        config.fq_outer_prefix_v6 = json_u8(value, "fq_outer_prefix_v6")?;
        config.fq_outer_max = json_usize(value, "fq_outer_max")?;
        config.fq_inner_max_per_outer = json_usize(value, "fq_inner_max_per_outer")?;
        config.fq_inner_max = json_usize(value, "fq_inner_max")?;
        config.fq_burst = json_duration(value, "fq_burst_ms")?;
        config.fq_max_debt = json_duration(value, "fq_max_debt_ms")?;
        config.fq_debt_ttl = json_duration(value, "fq_debt_ttl_ms")?;
        config.fq_inner_headers = json_string_list(value, "fq_inner_headers")?;
        config.fq_inner_cookies = json_string_list(value, "fq_inner_cookies")?;
        config.fq_inner_include_ip = json_bool(value, "fq_inner_include_ip")?;
        config.client_ip_header = json_string(value, "client_ip_header")?;
        config.trusted_proxies = json_string_list(value, "trusted_proxies")?;
        config.warnings = json_string_list(value, "warnings")?
            .iter()
            .map(|token| Warning::parse(token))
            .collect::<Result<Vec<Warning>, ConfigError>>()?;
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
        config.preset = preset_from_json(value)?;
        config.sys_path = json_string_list(value, "sys_path")?
            .iter()
            .map(|entry| unlatin1("sys_path", entry))
            .collect::<Result<Vec<Vec<u8>>, ConfigError>>()?;
        config.cwd = json_opt_string(value, "cwd")?
            .map(|dir| unlatin1("cwd", &dir))
            .transpose()?;
        Ok(config)
    }
}

/// The private scratch directory of the view running under `uid` (§2.2, §7.2).
pub fn view_dir(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/blastit/views/{uid}"))
}

/// The `preset` member of the stage 1 document (E1).
///
/// The name is carried as text and is **not** looked up in [`PRESETS`]: what
/// stage 1 reports is what stage 0 decided, so a document written by a build
/// with a preset this one does not know is still a document stage 1 can boot
/// from — and a preset that was removed from the table cannot silently change
/// which defaults a running server was configured with.
///
/// # Errors
///
/// [`ConfigError::Document`] when the member is missing or has the wrong shape;
/// the message names the field, never a value.
fn preset_from_json(value: &Value) -> Result<Option<PresetChoice>, ConfigError> {
    let field = field(value, "preset")?;
    if field.is_null() {
        return Ok(None);
    }
    let object = field
        .as_object()
        .ok_or_else(|| document("preset", "must be an object or null".to_string()))?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| document("preset", "must name the preset".to_string()))?
        .to_string();
    let supplied = object
        .get("supplied")
        .and_then(Value::as_object)
        .ok_or_else(|| document("preset", "`supplied` must be an object".to_string()))?
        .iter()
        .map(|(variable, value)| {
            let value = value.as_str().ok_or_else(|| {
                document(
                    "preset",
                    "every supplied value must be a string".to_string(),
                )
            })?;
            Ok((variable.clone(), value.to_string()))
        })
        .collect::<Result<BTreeMap<String, String>, ConfigError>>()?;
    let overridden = object
        .get("overridden")
        .and_then(Value::as_array)
        .ok_or_else(|| document("preset", "`overridden` must be an array".to_string()))?
        .iter()
        .map(|name| {
            name.as_str()
                .map(str::to_string)
                .ok_or_else(|| document("preset", "`overridden` must name variables".to_string()))
        })
        .collect::<Result<Vec<String>, ConfigError>>()?;
    Ok(Some(PresetChoice {
        name,
        supplied,
        overridden,
    }))
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

/// The preset `--preset` or `BLASTIT_PRESET` asks for, checked (E1).
///
/// `--preset` wins when both are set: see [`Config::from_env`]. The name is
/// validated here, and the error names the *source* the operator actually
/// wrote — the flag or the variable — and lists the presets this build knows.
/// When both are set and disagree, the boot says so once, because a variable
/// that silently lost to the flag is exactly the sort of thing an operator
/// finds out about in production.
///
/// # Errors
///
/// [`ConfigError::Invalid`] naming the source and listing the known presets.
fn preset_choice(
    flag: Option<&str>,
) -> Result<(Option<&'static Preset>, Vec<Warning>), ConfigError> {
    // The variable is read the way the flag is: whitespace around a name is the
    // operator's — an env file or a CI variable is easy to pad by accident — and
    // a value that is only whitespace is how a deployment clears an inherited
    // one.
    let from_env = variable("BLASTIT_PRESET")?
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let (name, source) = match flag {
        Some(name) => {
            let warning = match &from_env {
                Some(other) if other != name => vec![Warning::PresetFlagWins],
                _ => Vec::new(),
            };
            return Ok((look_up_preset(name, "--preset")?, warning));
        }
        None => match from_env {
            Some(name) => (name, "BLASTIT_PRESET"),
            None => return Ok((None, Vec::new())),
        },
    };
    Ok((look_up_preset(&name, source)?, Vec::new()))
}

/// The preset called `name`, or the error that lists the ones there are (E1).
///
/// # Errors
///
/// [`ConfigError::Invalid`] naming `source` (the flag or the variable) and
/// every known preset, because the name that was asked for is the mistake worth
/// correcting and the alternatives are what the operator needs next.
fn look_up_preset(name: &str, source: &str) -> Result<Option<&'static Preset>, ConfigError> {
    Preset::find(name).map(Some).ok_or_else(|| {
        invalid(
            source,
            format!(
                "unknown preset `{name}`: the known presets are {}",
                preset_list()
            ),
        )
    })
}

/// Where a configuration value comes from: the environment, or the preset when
/// the environment has nothing to say (E1).
///
/// The environment always wins, which is what makes `BLASTIT_PRESET=…` safe to
/// add to a deployment that already sets variables by hand: a preset only fills
/// in what nobody named. `BLASTIT_PRESET` itself and the per-proxy caps are
/// read from the environment directly — the first decides which preset this is,
/// and the second is named after the manifest's proxies.
struct Settings {
    preset: Option<&'static Preset>,
}

impl Settings {
    /// One variable: the environment's value, else the preset's, else unset.
    ///
    /// # Errors
    ///
    /// [`ConfigError::NotUnicode`] for a variable the process cannot read as
    /// text, exactly as [`variable`] reports it.
    fn get(&self, name: &str) -> Result<Option<String>, ConfigError> {
        if let Some(value) = variable(name)? {
            return Ok(Some(value));
        }
        Ok(self
            .preset
            .and_then(|preset| preset.get(name))
            .map(str::to_string))
    }
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

/// Parses a prefix length or any other small unsigned number.
fn parse_u8(name: &str, value: &str) -> Result<u8, ConfigError> {
    value
        .trim()
        .parse::<u8>()
        .map_err(|error| invalid(name, format!("expected a whole number under 256: {error}")))
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
/// [`parse_bool`] rejects it and the error names the variable.
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

/// `BLASTIT_CLIENT_IP_HEADER`: a header name, lower-cased.
fn parse_client_ip_header(value: &str) -> Result<String, ConfigError> {
    let header = value.trim().to_ascii_lowercase();
    if !is_header_name(&header) {
        return Err(invalid(
            "BLASTIT_CLIENT_IP_HEADER",
            "must be a header name, without its colon".to_string(),
        ));
    }
    Ok(header)
}

/// `BLASTIT_TRUSTED_PROXIES`: IP addresses, CIDR networks or `*`.
fn parse_trusted_proxies(value: &str) -> Result<Vec<String>, ConfigError> {
    let list = split_list(value);
    check_trusted_proxies(&list).map_err(|reason| invalid("BLASTIT_TRUSTED_PROXIES", reason))?;
    Ok(list)
}

/// Why a trusted proxy list cannot be used, in one sentence and without its
/// entries (§8.1: an error names the variable, never a value).
fn check_trusted_proxies(list: &[String]) -> Result<(), String> {
    TrustedProxies::parse(list)
        .map(|_| ())
        .map_err(|_| TRUSTED_PROXIES_REASON.to_string())
}

/// What can be wrong with a `BLASTIT_TRUSTED_PROXIES` entry. The port is named
/// because that is the mistake an operator makes: one peer is not a network.
const TRUSTED_PROXIES_REASON: &str =
    "every entry must be an IP address, a CIDR network or `*`; a port is not a network";

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

fn json_u8(value: &Value, name: &'static str) -> Result<u8, ConfigError> {
    let raw = json_u64(value, name)?;
    u8::try_from(raw).map_err(|_| document(name, "does not fit in a byte".to_string()))
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

/// A JSON boolean, or `None` when the field is absent or null.
///
/// Used for a field stage 0 only writes once it has detected something (D6's
/// bounding-set fact), so a reader does not have to care which.
///
/// # Errors
///
/// [`ConfigError::Document`] when the field is there but is not a boolean.
fn json_optional_bool(value: &Value, name: &'static str) -> Result<Option<bool>, ConfigError> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => json_bool(value, name).map(Some),
    }
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

    /// An address, for the client-IP tests.
    fn ip(text: &str) -> std::net::IpAddr {
        text.parse().unwrap()
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
                Config::from_env(None).unwrap().immortalize,
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
                    Config::from_env(None).unwrap().immortalize,
                    expected,
                    "BLASTIT_IMMORTALIZE={value}"
                );
            }
            std::env::set_var("BLASTIT_IMMORTALIZE", "sometimes");
            let err = Config::from_env(None).unwrap_err();
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
        // D5: the fair queue and the one client-IP resolver.
        config.fair_queuing = true;
        config.fq_outer_prefix_v4 = 16;
        config.fq_outer_prefix_v6 = 64;
        config.fq_outer_max = 8;
        config.fq_inner_max_per_outer = 2;
        config.fq_inner_max = 5;
        config.fq_burst = Duration::from_millis(1);
        config.fq_max_debt = Duration::from_millis(2);
        config.fq_debt_ttl = Duration::from_millis(3);
        config.fq_inner_headers = vec!["x-api-key".to_string()];
        config.fq_inner_cookies = Vec::new();
        config.fq_inner_include_ip = false;
        config.client_ip_header = "do-connecting-ip".to_string();
        config.trusted_proxies = vec!["10.0.0.0/8".to_string(), "*".to_string()];
        config.warnings = vec![Warning::TrustForwardedAny];
        config.sys_path = vec![
            b"/app".to_vec(),
            b"/app/.venv/lib/python3.14/site-packages".to_vec(),
            vec![b'/', b'a', 0xff, b'p'],
        ];
        config.privilege = Some(Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
            bounding_set_dropped: true,
        });
        config.validate().unwrap();

        let json = config.to_json();
        let text = serde_json::to_string(&json).unwrap();
        assert!(!text.contains("BLASTIT_"), "{text}");
        assert_eq!(json["immortalize"], serde_json::json!("off"));
        assert_eq!(json["fair_queuing"], serde_json::json!(true));
        assert_eq!(json["fq_outer_prefix_v4"], serde_json::json!(16));
        assert_eq!(json["fq_burst_ms"], serde_json::json!(1.0));
        assert_eq!(
            json["client_ip_header"],
            serde_json::json!("do-connecting-ip")
        );
        let back = Config::from_json(&json).unwrap();
        assert_eq!(back, config);
        assert_eq!(back.fair_config().client_ip, config.client_ip());
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
    fn the_working_directory_blastit_started_in_travels_in_the_document() {
        // D2/§5.3: stage 1 puts it first on `sys.path`, so it is carried as
        // bytes like every other path — a directory that is not valid UTF-8 is
        // a directory like any other.
        let mut config = Config::default();
        assert_eq!(config.cwd, None, "nothing was started in a directory yet");
        assert_eq!(config.to_json()["cwd"], serde_json::Value::Null);
        config.cwd = Some(vec![b'/', b'a', 0xff, b'p']);
        let json = config.to_json();
        assert_eq!(json["cwd"], serde_json::json!("/a\u{ff}p"));
        assert_eq!(Config::from_json(&json).unwrap().cwd, config.cwd);

        // A directory outside latin-1, and a member that is not a string: a
        // document stage 0 did not write names the field.
        let mut json = Config::default().to_json();
        json["cwd"] = serde_json::json!("/app/\u{20ac}");
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("cwd"), "{err}");
        let mut json = Config::default().to_json();
        json["cwd"] = serde_json::json!(["/app"]);
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("cwd"), "{err}");
    }

    #[test]
    fn the_privilege_mode_round_trips_and_says_what_it_is() {
        let mut config = Config::default();
        assert_eq!(config.privilege, None, "a default document says nothing");
        let json = config.to_json();
        assert_eq!(json["privilege"], serde_json::Value::Null);
        assert_eq!(json["capabilities"], serde_json::json!([]));
        assert_eq!(json["bounding_set"], serde_json::Value::Null);

        // Root: a mode, no list, and the bounding-set fact anyway (M1: root
        // empties it too, when the container has `cap_setpcap`).
        config.privilege = Some(Privilege {
            mode: Mode::Root,
            capabilities: Vec::new(),
            bounding_set_dropped: true,
        });
        let json = config.to_json();
        assert_eq!(json["privilege"], serde_json::json!("root"));
        assert_eq!(json["capabilities"], serde_json::json!([]));
        assert_eq!(json["bounding_set"], serde_json::json!(true));
        let back = Config::from_json(&json).unwrap();
        assert_eq!(back.privilege, config.privilege);
        assert_eq!(Mode::Root.as_str(), "root");

        // Rootless: the mode, the names init holds, and the one it could not
        // empty.
        config.privilege = Some(Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
            bounding_set_dropped: false,
        });
        let json = config.to_json();
        assert_eq!(json["privilege"], serde_json::json!("capabilities"));
        assert_eq!(
            json["capabilities"],
            serde_json::json!(["cap_chown", "cap_setuid"])
        );
        assert_eq!(json["bounding_set"], serde_json::json!(false));
        let back = Config::from_json(&json).unwrap();
        assert_eq!(back.privilege, config.privilege);
        assert_eq!(Mode::Capabilities.as_str(), "capabilities");
        assert_eq!(
            config.privilege.as_ref().unwrap().capabilities,
            ["cap_chown", "cap_setuid"]
        );
        assert!(!config.privilege.as_ref().unwrap().bounding_set_dropped);
        // A document that predates the field still reads as "dropped", which is
        // what the boot in force does unless it says otherwise.
        let mut older = json.clone();
        older.as_object_mut().unwrap().remove("bounding_set");
        assert!(
            Config::from_json(&older)
                .unwrap()
                .privilege
                .unwrap()
                .bounding_set_dropped
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
        json["privilege"] = serde_json::json!("capabilities");
        json["bounding_set"] = serde_json::json!("maybe");
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("bounding_set"), "{err}");
    }

    #[test]
    fn the_privilege_fields_reach_a_lifecycle_event() {
        let privilege = Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
            bounding_set_dropped: true,
        };
        let lifecycle = privilege.lifecycle(Lifecycle::default().with_stage("stage0"));
        assert_eq!(lifecycle.privilege(), Some("capabilities"));
        assert_eq!(lifecycle.capabilities(), ["cap_chown", "cap_setuid"]);
        assert_eq!(lifecycle.bounding_set(), Some(true));
        let root = Privilege {
            mode: Mode::Root,
            capabilities: Vec::new(),
            bounding_set_dropped: false,
        }
        .lifecycle(Lifecycle::default().with_stage("stage0"));
        assert_eq!(root.privilege(), Some("root"));
        assert_eq!(root.bounding_set(), Some(false), "M1 says which happened");
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

    #[test]
    fn the_fair_queuing_defaults_are_the_scheduler_s() {
        // §8.2 (D5): the `BLASTIT_FQ_*` defaults are granian's, and they are
        // `FairConfig`'s, because that is what the scheduler queues with.
        let config = Config::default();
        assert!(!config.fair_queuing, "fair queuing is off by default");
        assert_eq!(config.fair_config(), FairConfig::default());
        assert_eq!(config.fq_outer_prefix_v4, 24);
        assert_eq!(config.fq_outer_prefix_v6, 56);
        assert_eq!(config.fq_outer_max, 65_536);
        assert_eq!(config.fq_inner_max_per_outer, 1_024);
        assert_eq!(config.fq_inner_max, 262_144);
        assert_eq!(config.fq_burst, Duration::from_millis(20));
        assert_eq!(config.fq_max_debt, Duration::from_secs(5));
        assert_eq!(config.fq_debt_ttl, Duration::from_secs(30));
        assert!(config.fq_inner_include_ip);
        assert_eq!(
            config.fq_inner_headers,
            [
                "user-agent",
                "accept-language",
                "sec-ch-ua",
                "sec-ch-ua-platform",
                "sec-ch-ua-mobile",
                "authorization"
            ]
        );
        assert_eq!(config.fq_inner_cookies, ["sessionid", "session"]);
    }

    #[test]
    fn the_default_trusted_proxies_are_the_resolver_s() {
        // The list is spelled out in this module because the document carries
        // values, so this is what keeps it equal to the library's default.
        let config = Config::default();
        assert_eq!(config.client_ip(), ClientIpConfig::default());
        assert_eq!(config.client_ip_header, "x-forwarded-for");
        assert_eq!(config.trusted_proxies, ["127.0.0.1", "::1"]);
        assert!(
            config.warnings.is_empty(),
            "nothing is deprecated by default"
        );
    }

    #[test]
    fn the_fair_queuing_settings_are_read_from_the_environment() {
        let _env = env();
        // SAFETY: `env()` is held for the whole test, so no other test reads or
        // writes the environment while this one runs; every variable is removed
        // again before it returns.
        unsafe {
            let vars = [
                "BLASTIT_FAIR_QUEUING",
                "BLASTIT_FQ_OUTER_PREFIX_V4",
                "BLASTIT_FQ_OUTER_PREFIX_V6",
                "BLASTIT_FQ_OUTER_MAX",
                "BLASTIT_FQ_INNER_MAX_PER_OUTER",
                "BLASTIT_FQ_INNER_MAX",
                "BLASTIT_FQ_BURST",
                "BLASTIT_FQ_MAX_DEBT",
                "BLASTIT_FQ_DEBT_TTL",
                "BLASTIT_FQ_INNER_HEADERS",
                "BLASTIT_FQ_INNER_COOKIES",
                "BLASTIT_FQ_INNER_INCLUDE_IP",
            ];
            for name in vars {
                std::env::remove_var(name);
            }
            assert!(!Config::from_env(None).unwrap().fair_queuing);

            std::env::set_var("BLASTIT_FAIR_QUEUING", "on");
            std::env::set_var("BLASTIT_FQ_OUTER_PREFIX_V4", "16");
            std::env::set_var("BLASTIT_FQ_OUTER_PREFIX_V6", "48");
            std::env::set_var("BLASTIT_FQ_OUTER_MAX", "10");
            std::env::set_var("BLASTIT_FQ_INNER_MAX_PER_OUTER", "3");
            std::env::set_var("BLASTIT_FQ_INNER_MAX", "9");
            std::env::set_var("BLASTIT_FQ_BURST", "0");
            std::env::set_var("BLASTIT_FQ_MAX_DEBT", "250ms");
            std::env::set_var("BLASTIT_FQ_DEBT_TTL", "2m");
            std::env::set_var("BLASTIT_FQ_INNER_HEADERS", "user-agent, x-api-key");
            std::env::set_var("BLASTIT_FQ_INNER_COOKIES", "");
            std::env::set_var("BLASTIT_FQ_INNER_INCLUDE_IP", "0");
            let config = Config::from_env(None).unwrap();
            assert!(config.fair_queuing);
            assert_eq!(config.fq_outer_prefix_v4, 16);
            assert_eq!(config.fq_outer_prefix_v6, 48);
            assert_eq!(config.fq_outer_max, 10);
            assert_eq!(config.fq_inner_max_per_outer, 3);
            assert_eq!(config.fq_inner_max, 9);
            assert_eq!(config.fq_burst, Duration::ZERO);
            assert_eq!(config.fq_max_debt, Duration::from_millis(250));
            assert_eq!(config.fq_debt_ttl, Duration::from_secs(120));
            assert_eq!(config.fq_inner_headers, ["user-agent", "x-api-key"]);
            assert!(
                config.fq_inner_cookies.is_empty(),
                "an empty list hashes no cookie, and is not the default list"
            );
            assert!(!config.fq_inner_include_ip);
            // The scheduler gets exactly what was read.
            let fair = config.fair_config();
            assert_eq!(fair.outer_prefix_v4, 16);
            assert_eq!(fair.inner_headers, ["user-agent", "x-api-key"]);
            assert!(!fair.inner_include_ip);

            // Out-of-range values are clamped, not refused, and the boot
            // warning says so (D5, as the reference implementation does).
            std::env::set_var("BLASTIT_FQ_OUTER_PREFIX_V4", "33");
            std::env::set_var("BLASTIT_FQ_INNER_MAX", "0");
            std::env::set_var("BLASTIT_FQ_DEBT_TTL", "250ms");
            let clamped = Config::from_env(None).unwrap();
            assert_eq!(clamped.fq_outer_prefix_v4, 32);
            assert_eq!(clamped.fq_inner_max, 1);
            assert_eq!(clamped.fq_debt_ttl, MIN_DEBT_TTL);
            assert_eq!(
                clamped.warnings,
                vec![
                    Warning::OuterPrefixV4Clamped,
                    Warning::InnerMaxClamped,
                    Warning::DebtTtlFloored
                ]
            );
            for warning in &clamped.warnings {
                assert!(
                    warning.message().contains("BLASTIT_FQ_"),
                    "the warning names the variable: {:?}",
                    warning.message()
                );
            }
            std::env::set_var("BLASTIT_FQ_OUTER_PREFIX_V4", "16");
            std::env::set_var("BLASTIT_FQ_INNER_MAX", "9");
            std::env::set_var("BLASTIT_FQ_DEBT_TTL", "2m");

            // A value that is not a number at all *is* refused: there is no
            // nearest usable value to a word.
            std::env::set_var("BLASTIT_FQ_INNER_MAX", "many");
            let err = Config::from_env(None).unwrap_err();
            assert!(err.to_string().contains("BLASTIT_FQ_INNER_MAX"), "{err}");
            std::env::set_var("BLASTIT_FQ_INNER_MAX", "9");

            std::env::set_var("BLASTIT_FAIR_QUEUING", "sometimes");
            let err = Config::from_env(None).unwrap_err();
            assert!(err.to_string().contains("BLASTIT_FAIR_QUEUING"), "{err}");

            for name in vars {
                std::env::remove_var(name);
            }
        }
    }

    #[test]
    fn the_client_ip_settings_are_read_from_the_environment() {
        let _env = env();
        // SAFETY: see the test above.
        unsafe {
            for name in [
                "BLASTIT_CLIENT_IP_HEADER",
                "BLASTIT_TRUSTED_PROXIES",
                "BLASTIT_TRUST_FORWARDED",
            ] {
                std::env::remove_var(name);
            }
            let default = Config::from_env(None).unwrap();
            assert_eq!(default.client_ip(), ClientIpConfig::default());

            // App Platform's setting: the edge's own header, believed from the
            // ingress networks only.
            std::env::set_var("BLASTIT_CLIENT_IP_HEADER", "Do-Connecting-IP");
            std::env::set_var("BLASTIT_TRUSTED_PROXIES", "10.0.0.0/8, 100.64.0.0/10");
            let config = Config::from_env(None).unwrap();
            assert_eq!(
                config.client_ip_header, "do-connecting-ip",
                "a header name is case-insensitive"
            );
            let client_ip = config.client_ip();
            assert_eq!(client_ip.header, "do-connecting-ip");
            let headers = vec![(b"do-connecting-ip".to_vec(), b"203.0.113.7".to_vec())];
            assert_eq!(
                client_ip.resolve("10.244.3.9:1", &headers).unwrap().ip(),
                ip("203.0.113.7"),
                "the ingress is trusted"
            );
            assert_eq!(
                client_ip.resolve("8.8.8.8:1", &headers).unwrap().ip(),
                ip("8.8.8.8"),
                "anywhere else the header is not read"
            );

            // `*`, and the list that trusts nobody.
            std::env::set_var("BLASTIT_TRUSTED_PROXIES", "*");
            assert!(Config::from_env(None).unwrap().client_ip().trusted.is_any());
            std::env::set_var("BLASTIT_TRUSTED_PROXIES", " ");
            let none = Config::from_env(None).unwrap();
            assert!(none.trusted_proxies.is_empty());
            assert!(
                !none.client_ip().trusted.contains(&ip("127.0.0.1")),
                "an empty list trusts nobody"
            );

            for name in ["BLASTIT_CLIENT_IP_HEADER", "BLASTIT_TRUSTED_PROXIES"] {
                std::env::remove_var(name);
            }
        }
    }

    #[test]
    fn a_trusted_proxy_that_is_not_a_network_names_its_variable() {
        let _env = env();
        // SAFETY: see the test above.
        unsafe {
            std::env::remove_var("BLASTIT_TRUST_FORWARDED");
            for value in [
                "127.0.0.1:8080",
                "example.com",
                "10.0.0.0/33",
                "10.0.0.0/x",
                "2001:db8::/129",
            ] {
                std::env::set_var("BLASTIT_TRUSTED_PROXIES", value);
                let err = Config::from_env(None).unwrap_err();
                let text = err.to_string();
                assert!(
                    text.contains("BLASTIT_TRUSTED_PROXIES"),
                    "{value:?}: {text}"
                );
                assert!(
                    !text.contains(value.trim()),
                    "{value:?}: the error must not quote the value: {text}"
                );
            }
            // A port is the mistake worth naming, because it looks like one
            // peer and is not a network at all.
            std::env::set_var("BLASTIT_TRUSTED_PROXIES", "127.0.0.1:8080");
            assert!(
                Config::from_env(None)
                    .unwrap_err()
                    .to_string()
                    .contains("port"),
                "the port is named as the likely mistake"
            );
            std::env::remove_var("BLASTIT_TRUSTED_PROXIES");
        }
    }

    #[test]
    fn a_client_ip_header_that_is_not_a_header_name_names_its_variable() {
        let _env = env();
        // SAFETY: see the test above.
        unsafe {
            std::env::remove_var("BLASTIT_TRUST_FORWARDED");
            for value in ["x forwarded for", "x-forwarded-for:", "host:8080", ""] {
                std::env::set_var("BLASTIT_CLIENT_IP_HEADER", value);
                let err = Config::from_env(None).unwrap_err();
                assert!(
                    err.to_string().contains("BLASTIT_CLIENT_IP_HEADER"),
                    "{value:?}: {err}"
                );
            }
            std::env::remove_var("BLASTIT_CLIENT_IP_HEADER");
        }
    }

    #[test]
    fn the_deprecated_trust_forwarded_is_translated_and_warned_about() {
        let _env = env();
        // SAFETY: see the test above.
        unsafe {
            for name in [
                "BLASTIT_CLIENT_IP_HEADER",
                "BLASTIT_TRUSTED_PROXIES",
                "BLASTIT_TRUST_FORWARDED",
            ] {
                std::env::remove_var(name);
            }

            // `0` is what it always was: trust nobody, so the peer is the
            // client.
            std::env::set_var("BLASTIT_TRUST_FORWARDED", "0");
            let config = Config::from_env(None).unwrap();
            assert!(config.trusted_proxies.is_empty());
            assert_eq!(
                config.client_ip().resolve("1.2.3.4:1", &[]).unwrap().ip(),
                ip("1.2.3.4")
            );
            assert_eq!(
                config.warnings,
                vec![Warning::TrustForwardedNone],
                "`0` is translated, and said out loud"
            );
            assert!(
                Warning::TrustForwardedNone.message().contains("deprecated"),
                "the message names the variable and its replacement"
            );

            // `1` is the old rule as well: the last entry of anybody's header,
            // which is `*` here — every peer is trusted and the rightmost entry
            // wins.
            std::env::set_var("BLASTIT_TRUST_FORWARDED", "1");
            let config = Config::from_env(None).unwrap();
            assert_eq!(config.trusted_proxies, ["*"]);
            assert_eq!(config.warnings, vec![Warning::TrustForwardedAny]);
            assert!(config.client_ip().trusted.is_any());

            // The new variable wins, and says so.
            std::env::set_var("BLASTIT_TRUSTED_PROXIES", "10.0.0.0/8");
            let config = Config::from_env(None).unwrap();
            assert_eq!(config.trusted_proxies, ["10.0.0.0/8"]);
            assert_eq!(config.warnings, vec![Warning::TrustForwardedIgnored]);
            assert!(
                Warning::TrustForwardedIgnored.message().contains("ignored"),
                "an ignored setting is named as ignored"
            );

            // A value that is not a boolean at all is an error, not a
            // warning.
            std::env::remove_var("BLASTIT_TRUSTED_PROXIES");
            std::env::set_var("BLASTIT_TRUST_FORWARDED", "maybe");
            let err = Config::from_env(None).unwrap_err();
            assert!(err.to_string().contains("BLASTIT_TRUST_FORWARDED"), "{err}");

            std::env::remove_var("BLASTIT_TRUST_FORWARDED");
        }
    }

    #[test]
    fn a_fair_queue_entry_that_is_not_a_name_is_refused() {
        // §3.1: a header name is a token, and so is a cookie name (RFC 6265). An
        // entry that is a *value* — or that carries a space or a colon — would
        // be hashed against a name no request can have, so it is refused rather
        // than quietly doing nothing.
        let mut config = Config::default();
        for bad in [
            "user agent",
            "user-agent:",
            "a=b",
            "",
            "a\nb",
            "Bearer s3cret",
            "cookie=sessionid",
        ] {
            config.fq_inner_headers = vec![bad.to_string()];
            let err = config.validate().unwrap_err();
            assert!(
                err.to_string().contains("BLASTIT_FQ_INNER_HEADERS"),
                "{bad:?}: {err}"
            );
            assert!(err.to_string().contains("header name"), "{bad:?}: {err}");
        }
        for bad in ["session id", "sessionid=", "sessionid; Path=/", ""] {
            config.fq_inner_headers = vec!["user-agent".to_string()];
            config.fq_inner_cookies = vec![bad.to_string()];
            let err = config.validate().unwrap_err();
            assert!(
                err.to_string().contains("BLASTIT_FQ_INNER_COOKIES"),
                "{bad:?}: {err}"
            );
            assert!(err.to_string().contains("cookie name"), "{bad:?}: {err}");
        }

        // The names HTTP actually uses pass, including the defaults and the
        // empty-but-for-that case of hashing no header at all.
        config.fq_inner_headers = vec!["x-api-key".to_string(), "sec-ch-ua".to_string()];
        config.fq_inner_cookies = vec!["sessionid".to_string(), "csrf.token".to_string()];
        config.validate().unwrap();
        config.fq_inner_headers = Vec::new();
        config.fq_inner_cookies = Vec::new();
        config.validate().unwrap();
    }

    #[test]
    fn the_fair_queue_knobs_are_clamped_and_reported() {
        // D5, as the reference implementation does it: a prefix that cannot be
        // longer than the address it masks, a table that holds nothing, and a
        // debt TTL under a second all have a nearest usable value, so none of
        // them stops the server from starting.
        let mut config = Config::default();
        config.fq_outer_prefix_v4 = 33;
        config.fq_outer_prefix_v6 = 129;
        config.fq_outer_max = 0;
        config.fq_inner_max_per_outer = 0;
        config.fq_inner_max = 0;
        config.fq_debt_ttl = Duration::from_millis(250);
        config.validate().unwrap();
        assert_eq!(config.fq_outer_prefix_v4, 32);
        assert_eq!(config.fq_outer_prefix_v6, 128);
        assert_eq!(config.fq_outer_max, 1);
        assert_eq!(config.fq_inner_max_per_outer, 1);
        assert_eq!(config.fq_inner_max, 1);
        assert_eq!(config.fq_debt_ttl, MIN_DEBT_TTL);
        assert_eq!(
            config.warnings,
            vec![
                Warning::OuterPrefixV4Clamped,
                Warning::OuterPrefixV6Clamped,
                Warning::OuterMaxClamped,
                Warning::InnerMaxPerOuterClamped,
                Warning::InnerMaxClamped,
                Warning::DebtTtlFloored,
            ]
        );
        // The queue is configured with the clamped values, not with the raw
        // ones, and clamping is not a way of turning fair queuing on.
        let fair = config.fair_config();
        assert_eq!(fair.outer_prefix_v4, 32);
        assert_eq!(fair.outer_max, 1);
        assert_eq!(fair.debt_ttl, MIN_DEBT_TTL);
        assert!(!config.fair_queuing);

        // In range, or exactly on the boundary, nothing is adjusted and
        // nothing is reported.
        let mut config = Config::default();
        config.fq_debt_ttl = MIN_DEBT_TTL;
        config.fq_outer_prefix_v4 = 32;
        config.fq_outer_prefix_v6 = 128;
        config.validate().unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.fq_outer_prefix_v6, 128);
    }

    #[test]
    fn the_configuration_warnings_travel_to_stage_1() {
        // The document is what stage 1 reads, and it is stage 1 that prints the
        // warning (§8.1), so the decision has to survive the round trip.
        let mut config = Config::default();
        config.warnings = vec![Warning::TrustForwardedIgnored, Warning::DebtTtlFloored];
        let json = config.to_json();
        assert_eq!(
            json["warnings"],
            serde_json::json!(["trust_forwarded_ignored", "debt_ttl_floored"])
        );
        assert_eq!(Config::from_json(&json).unwrap().warnings, config.warnings);

        // Every token round-trips, and a token this build does not know is a
        // document error that names the field.
        for warning in [
            Warning::TrustForwardedAny,
            Warning::TrustForwardedNone,
            Warning::TrustForwardedIgnored,
        ] {
            assert_eq!(Warning::parse(warning.as_str()).unwrap(), warning);
            assert!(
                warning.message().contains("BLASTIT_TRUST_FORWARDED")
                    && warning.message().contains("BLASTIT_TRUSTED_PROXIES"),
                "the message says what to do instead"
            );
        }
        for warning in [
            Warning::OuterPrefixV4Clamped,
            Warning::OuterPrefixV6Clamped,
            Warning::OuterMaxClamped,
            Warning::InnerMaxPerOuterClamped,
            Warning::InnerMaxClamped,
            Warning::DebtTtlFloored,
        ] {
            assert_eq!(Warning::parse(warning.as_str()).unwrap(), warning);
            assert!(
                warning.message().contains("BLASTIT_FQ_") && warning.message().contains("read as"),
                "the message says what it was read as: {:?}",
                warning.message()
            );
        }
        let mut json = json;
        json["warnings"] = serde_json::json!(["something_else"]);
        let err = Config::from_json(&json).unwrap_err();
        assert!(err.to_string().contains("warnings"), "{err}");
    }

    /// The environment variables a preset could supply, cleared, so that a test
    /// reads what the preset says rather than what the shell had.
    ///
    /// # Safety
    ///
    /// The caller holds [`ENV`].
    unsafe fn clear_preset_variables() {
        unsafe { std::env::remove_var("BLASTIT_PRESET") };
        for preset in PRESETS {
            for (name, _) in preset.settings() {
                unsafe { std::env::remove_var(name) };
            }
        }
    }

    /// [`clear_preset_variables`], for as long as it lives (E1).
    ///
    /// A test that leaves `BLASTIT_PRESET`, or a variable a preset supplies,
    /// behind — an assertion that failed half-way is enough — would make every
    /// test after it read a configuration nobody asked for. Dropping the guard
    /// clears them again, and guards drop before every other local of the test
    /// that made them.
    struct NoPreset;

    impl NoPreset {
        /// # Safety
        ///
        /// The caller must hold [`ENV`]: this writes the process environment.
        unsafe fn new() -> Self {
            unsafe { clear_preset_variables() };
            Self
        }
    }

    impl Drop for NoPreset {
        fn drop(&mut self) {
            // SAFETY: the caller of `new` holds `ENV`, and this runs before the
            // guard on it is dropped.
            unsafe { clear_preset_variables() };
        }
    }

    #[test]
    fn each_preset_supplies_the_documented_defaults() {
        // E1: the table in §8.2. The expectations are spelled out here rather
        // than read from `PRESETS`, so that a value that changes has to change
        // in the documentation too.
        let expected: &[(&str, &str, &[&str], bool)] = &[
            ("digitalocean-paas", "do-connecting-ip", &["*"], true),
            ("compose-traefik", "x-forwarded-for", &["*"], true),
        ];
        let _env = env();
        // SAFETY: `env()` is held for the whole test, so no other test reads or
        // writes the environment while this one runs.
        unsafe {
            let _clean = NoPreset::new();
            for (name, header, trusted, fair) in expected {
                std::env::set_var("BLASTIT_PRESET", name);
                let config = Config::from_env(None).unwrap();
                assert_eq!(config.client_ip_header, *header, "{name}");
                assert_eq!(config.trusted_proxies, *trusted, "{name}");
                assert_eq!(config.fair_queuing, *fair, "{name}");
                assert_eq!(config.log_format, LogFormat::Json, "{name}");
                // The resolved resolver agrees: every peer is trusted, so a
                // client-IP header is read from anywhere (§2.3).
                assert!(config.client_ip().trusted.is_any(), "{name}");
                // The queue is configured with the preset's knobs, and the
                // switch is on: granian's defaults, none of the `FQ_*`
                // variables busy. The client-IP settings are the preset's, so
                // they are carried through to the queue as well.
                assert_eq!(
                    config.fair_config(),
                    FairConfig {
                        client_ip: config.client_ip(),
                        ..FairConfig::default()
                    },
                    "granian's defaults, {name}"
                );
            }
            std::env::remove_var("BLASTIT_PRESET");
        }
    }

    #[test]
    fn no_preset_leaves_the_documented_defaults_alone() {
        // E1: a deployment with neither the flag nor the variable is exactly
        // the deployment §8.2 describes, and the boot says `preset: null`.
        let _env = env();
        // SAFETY: `env()` is held for the whole test.
        unsafe {
            std::env::remove_var("BLASTIT_PRESET");
            let _clean = NoPreset::new();
            let config = Config::from_env(None).unwrap();
            assert!(config.preset.is_none());
            assert_eq!(config.client_ip_header, ClientIpConfig::default().header);
            assert_eq!(
                config.trusted_proxies,
                TRUSTED_PROXIES_DEFAULT
                    .iter()
                    .map(|entry| (*entry).to_string())
                    .collect::<Vec<String>>()
            );
            assert!(!config.fair_queuing);
        }
    }

    #[test]
    fn an_explicit_variable_overrides_the_preset() {
        // E1's precedence: built-in default < preset < explicit variable. A
        // preset is a set of *defaults*, so `BLASTIT_PRESET=…` is always safe
        // to add next to variables an operator already set.
        let _env = env();
        // SAFETY: `env()` is held for the whole test.
        unsafe {
            let _clean = NoPreset::new();
            std::env::set_var("BLASTIT_PRESET", "compose-traefik");
            std::env::set_var("BLASTIT_LOG_FORMAT", "pretty");
            std::env::set_var("BLASTIT_FAIR_QUEUING", "0");
            std::env::set_var("BLASTIT_TRUSTED_PROXIES", "127.0.0.1");
            let config = Config::from_env(None).unwrap();
            assert_eq!(config.log_format, LogFormat::Pretty);
            assert!(!config.fair_queuing);
            assert_eq!(config.trusted_proxies, vec!["127.0.0.1".to_string()]);
            assert!(!config.client_ip().trusted.is_any());
            // The one the preset still supplies.
            assert_eq!(config.client_ip_header, "x-forwarded-for");

            let choice = config.preset.expect("the preset is in force");
            assert_eq!(choice.name, "compose-traefik");
            assert_eq!(
                choice.supplied,
                BTreeMap::from([(
                    "BLASTIT_CLIENT_IP_HEADER".to_string(),
                    "x-forwarded-for".to_string()
                )])
            );
            assert_eq!(
                choice.overridden,
                vec![
                    "BLASTIT_TRUSTED_PROXIES".to_string(),
                    "BLASTIT_FAIR_QUEUING".to_string(),
                    "BLASTIT_LOG_FORMAT".to_string(),
                ]
            );

            // A variable that is set to nothing is set: an empty
            // `BLASTIT_TRUSTED_PROXIES` means "trust nobody", and the preset's
            // `*` may not come back through the empty value.
            std::env::set_var("BLASTIT_TRUSTED_PROXIES", "");
            let config = Config::from_env(None).unwrap();
            assert!(config.trusted_proxies.is_empty());
            assert!(!config.client_ip().trusted.is_any());

            for name in [
                "BLASTIT_PRESET",
                "BLASTIT_LOG_FORMAT",
                "BLASTIT_FAIR_QUEUING",
                "BLASTIT_TRUSTED_PROXIES",
            ] {
                std::env::remove_var(name);
            }
        }
    }

    #[test]
    fn the_flag_names_the_preset_and_wins_over_the_variable() {
        // E1: `--preset` is the more explicit of the two — it is typed at the
        // process that is being started — so it wins, and the boot says which
        // preset it settled on. When the two disagree, that is worth a line of
        // its own.
        let _env = env();
        // SAFETY: `env()` is held for the whole test.
        unsafe {
            let _clean = NoPreset::new();
            std::env::set_var("BLASTIT_PRESET", "compose-traefik");

            let config = Config::from_env(Some("digitalocean-paas")).unwrap();
            let choice = config.preset.expect("the preset is in force");
            assert_eq!(choice.name, "digitalocean-paas");
            assert_eq!(config.client_ip_header, "do-connecting-ip");
            assert_eq!(config.warnings, vec![Warning::PresetFlagWins]);

            // The same name twice is not a disagreement.
            let config = Config::from_env(Some("compose-traefik")).unwrap();
            assert_eq!(
                config.preset.expect("the preset is in force").name,
                "compose-traefik"
            );
            assert!(config.warnings.is_empty(), "{:?}", config.warnings);

            // The flag alone, with no variable: no warning either.
            std::env::remove_var("BLASTIT_PRESET");
            let config = Config::from_env(Some("compose-traefik")).unwrap();
            assert!(config.warnings.is_empty(), "{:?}", config.warnings);
            assert_eq!(config.client_ip_header, "x-forwarded-for");
        }
    }

    #[test]
    fn an_unknown_preset_name_is_refused_and_lists_the_known_ones() {
        // E1: a name that is not a preset is a startup error, and the error
        // says what the presets are — an operator who typo'd `compose-trafik`
        // needs the list more than they need the name they wrote.
        let _env = env();
        // SAFETY: `env()` is held for the whole test.
        unsafe {
            let _clean = NoPreset::new();
            let err = Config::from_env(Some("compose-trafik")).unwrap_err();
            let text = err.to_string();
            assert!(text.contains("--preset"), "{text}");
            assert!(text.contains("compose-trafik"), "{text}");
            for preset in PRESETS {
                assert!(text.contains(preset.name()), "{text}");
            }

            std::env::set_var("BLASTIT_PRESET", "compose-trafik");
            let err = Config::from_env(None).unwrap_err();
            let text = err.to_string();
            assert!(text.contains("BLASTIT_PRESET"), "{text}");
            assert!(text.contains("compose-traefik"), "{text}");

            // The flag is checked before the variable, so a bad flag is
            // reported even when the variable is fine — and a bad variable is
            // reported when there is no flag.
            std::env::set_var("BLASTIT_PRESET", "compose-traefik");
            assert!(Config::from_env(Some("nope")).is_err());
            assert!(Config::from_env(Some("compose-traefik")).is_ok());
            std::env::remove_var("BLASTIT_PRESET");

            // An empty variable is "unset", not "the preset called ''": it is
            // how a deployment clears an inherited value.
            std::env::set_var("BLASTIT_PRESET", "");
            assert!(Config::from_env(None).unwrap().preset.is_none());
            std::env::remove_var("BLASTIT_PRESET");
        }
    }

    #[test]
    fn the_preset_name_is_trimmed_however_it_arrives() {
        // E1: the variable is read the way the flag is — an env file, a compose
        // `environment:` entry or a CI variable is easy to pad by accident — and
        // a value that is only whitespace is an unset variable.
        let _env = env();
        // SAFETY: `env()` is held for the whole test.
        unsafe {
            let _clean = NoPreset::new();

            std::env::set_var("BLASTIT_PRESET", "  compose-traefik  ");
            let config = Config::from_env(None).unwrap();
            assert_eq!(
                config.preset.as_ref().map(|choice| choice.name.as_str()),
                Some("compose-traefik")
            );
            assert_eq!(config.client_ip_header, "x-forwarded-for");

            // A padded name still matches the flag, so a padded variable is not
            // a disagreement worth a warning.
            let config = Config::from_env(Some("compose-traefik")).unwrap();
            assert!(config.warnings.is_empty(), "{:?}", config.warnings);
            assert_eq!(
                config.preset.as_ref().map(|choice| choice.name.as_str()),
                Some("compose-traefik")
            );

            for empty in ["", " ", "\t", "\n ", " \t"] {
                std::env::set_var("BLASTIT_PRESET", empty);
                let config = Config::from_env(None).unwrap();
                assert!(config.preset.is_none(), "{empty:?} is unset");
            }
        }
    }

    #[test]
    fn the_stage_one_document_carries_the_preset() {
        // E1: the preset travels in the configuration document like everything
        // else, because stage 1's environment is the app's and has no
        // `BLASTIT_*` variable in it at all (§4, §5.3).
        let _env = env();
        // SAFETY: `env()` is held for the whole test.
        unsafe {
            let _clean = NoPreset::new();
            std::env::set_var("BLASTIT_PRESET", "compose-traefik");
            std::env::set_var("BLASTIT_LOG_FORMAT", "pretty");
            let config = Config::from_env(None).unwrap();
            let json = config.to_json();
            assert_eq!(json["preset"]["name"], "compose-traefik");
            assert_eq!(
                json["preset"]["supplied"]["BLASTIT_CLIENT_IP_HEADER"],
                "x-forwarded-for"
            );
            assert_eq!(
                json["preset"]["overridden"],
                serde_json::json!(["BLASTIT_LOG_FORMAT"])
            );
            assert_eq!(Config::from_json(&json).unwrap(), config);

            // The preset member names the variable that was overridden, and
            // never the value that won: that one came from the environment, and
            // §8.1 keeps environment values out of the log and out of the
            // document's preset record alike. (The *effective* value is in the
            // document, of course — that is what the document is for.)
            let recorded = json["preset"].to_string();
            assert!(!recorded.contains("pretty"), "{recorded}");
            assert!(recorded.contains("BLASTIT_LOG_FORMAT"), "{recorded}");

            // No preset: `null`, and the round trip keeps that.
            std::env::remove_var("BLASTIT_PRESET");
            std::env::remove_var("BLASTIT_LOG_FORMAT");
            let none = Config::from_env(None).unwrap();
            let json = none.to_json();
            assert!(json["preset"].is_null(), "{json}");
            let back = Config::from_json(&json).unwrap();
            assert!(back.preset.is_none());
            assert_eq!(back, none);

            // A document stage 0 did not write is refused, naming the field.
            let mut broken = json;
            broken["preset"] = serde_json::json!({"name": 7});
            let err = Config::from_json(&broken).unwrap_err();
            assert!(err.to_string().contains("preset"), "{err}");
            let mut broken = none.to_json();
            broken["preset"] = serde_json::json!("compose-traefik");
            let err = Config::from_json(&broken).unwrap_err();
            assert!(err.to_string().contains("preset"), "{err}");
            let mut missing = none.to_json();
            missing.as_object_mut().unwrap().remove("preset");
            let err = Config::from_json(&missing).unwrap_err();
            assert!(err.to_string().contains("preset"), "{err}");
        }
    }

    #[test]
    fn a_preset_whose_assumption_does_not_hold_is_named_but_never_refused() {
        // E1, §2.1: the mode is detected rather than configured, so a preset
        // used where its platform's identity does not hold says so, and never
        // refuses: root in a rootless deployment still works. Stage 0 prints the
        // line itself, which is why this returns the notice instead of adding it
        // to `warnings` for stage 1.
        let chosen = |preset: &str| Config {
            preset: Some(PresetChoice {
                name: preset.to_string(),
                supplied: BTreeMap::new(),
                overridden: Vec::new(),
            }),
            ..Config::default()
        };

        let digitalocean = chosen("digitalocean-paas");
        let warning = digitalocean
            .preset_assumption_warning(Mode::Capabilities)
            .expect("a root preset booted without capabilities");
        assert_eq!(warning, Warning::PresetExpectsRoot);
        assert!(warning.message().contains("root"));
        assert_eq!(
            Warning::parse(warning.as_str()).unwrap(),
            Warning::PresetExpectsRoot,
            "the token round-trips even though stage 0 prints the sentence"
        );
        assert!(
            digitalocean.warnings.is_empty(),
            "the notice is printed at the boot, not carried to stage 1: {:?}",
            digitalocean.warnings
        );

        // The mode the preset assumes says nothing at all, and asking twice
        // cannot say anything twice.
        assert_eq!(digitalocean.preset_assumption_warning(Mode::Root), None);
        assert_eq!(
            digitalocean.preset_assumption_warning(Mode::Capabilities),
            Some(Warning::PresetExpectsRoot)
        );

        // And the other way round: a rootless preset booted as root.
        let rootless = chosen("compose-traefik");
        let warning = rootless
            .preset_assumption_warning(Mode::Root)
            .expect("a rootless preset booted as root");
        assert_eq!(warning, Warning::PresetExpectsRootless);
        assert!(warning.message().contains("rootless"));
        assert_eq!(
            Warning::parse(warning.as_str()).unwrap(),
            Warning::PresetExpectsRootless
        );
        assert_eq!(rootless.preset_assumption_warning(Mode::Capabilities), None);

        // No preset, no notice — whatever mode the boot is in — and a name this
        // build does not know has nothing to assume.
        let none = Config::default();
        assert_eq!(none.preset_assumption_warning(Mode::Root), None);
        assert_eq!(none.preset_assumption_warning(Mode::Capabilities), None);
        assert_eq!(
            chosen("gone-in-a-later-build").preset_assumption_warning(Mode::Root),
            None
        );
    }

    #[test]
    fn every_preset_table_is_well_formed() {
        // Every preset is shipped, so its table has to be usable: the names are
        // §8.2 variables, the values parse, and the help text names them all.
        for preset in PRESETS {
            let name = preset.name();
            assert!(!name.is_empty(), "{name}");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "`{name}` is a slug an operator can type"
            );
            assert!(!preset.summary().is_empty(), "{name}");
            let mut names: Vec<&str> = Vec::new();
            for (variable, value) in preset.settings() {
                assert!(
                    variable.starts_with("BLASTIT_"),
                    "{name} sets `{variable}`, which is not a BLASTIT_* variable"
                );
                assert!(!names.contains(variable), "{name} sets {variable} twice");
                names.push(variable);
                assert!(
                    !value.is_empty() && !value.contains(char::is_whitespace),
                    "{name}: `{variable}` is set to a value with whitespace in it"
                );
                assert_eq!(
                    preset.get(variable),
                    Some(*value),
                    "{name} must be able to look up {variable}"
                );
            }
            // The tables this build ships are exactly the two §8.2 documents;
            // anything else would be a preset nobody measured.
            let known = ["digitalocean-paas", "compose-traefik"];
            assert!(known.contains(&name), "`{name}` is not a documented preset");
            assert!(
                preset_list().contains(name) && preset_help().contains(name),
                "{name} is missing from the help text"
            );
            assert!(preset_help().lines().count() == PRESETS.len());
            // A preset this build does not know is not a preset.
            assert!(Preset::find(name).is_some());
        }
        assert!(Preset::find("nope").is_none());
        assert_eq!(PRESETS.len(), 2);
    }
}
