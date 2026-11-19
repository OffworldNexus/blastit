//! The value types of §8.2, and the spellings the environment, the stage-1
//! document and the `boot` event all agree on.

use std::path::PathBuf;

use blastit_events::Lifecycle;
use thiserror::Error;

use super::document;
use super::manifest_io::display_path;

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
    /// events cannot drift apart. It is also where `R23`'s build is attached:
    /// the `preload` `boot` is the one lifecycle event that names the build it
    /// is about to serve, and stage 1 constructs it through this method
    /// (`crate::stage1`'s `preload_lifecycle`). Every other lifecycle event
    /// (`stage0`, `config`, `ready`, `shutdown`) leaves `build` out.
    pub fn lifecycle(&self, lifecycle: Lifecycle) -> Lifecycle {
        let lifecycle = if lifecycle.stage() == Some("preload") {
            lifecycle.with_build(env!("CARGO_PKG_VERSION"))
        } else {
            lifecycle
        };
        let lifecycle = lifecycle
            .with_privilege(self.mode.as_str())
            .with_bounding_set(self.bounding_set_dropped);
        self.capabilities.iter().fold(lifecycle, |lifecycle, name| {
            lifecycle.with_capability(name.clone())
        })
    }

    /// The same three members D6 adds to the document, resolved (see
    /// [`Privilege::from_json`]).
    ///
    /// `None` when the document names no mode at all, which is a boot that never
    /// detected one. The capability names are only the rootless mode's: in root
    /// mode the interesting set is the container's, so the document's list is
    /// not read — exactly as the hand-written accessor did.
    pub(super) fn from_parts(
        mode: Option<String>,
        capabilities: Vec<String>,
        bounding_set: Option<bool>,
    ) -> Result<Option<Self>, ConfigError> {
        let mode = match mode {
            None => return Ok(None),
            Some(mode) => match mode.as_str() {
                "root" => Mode::Root,
                "capabilities" => Mode::Capabilities,
                other => {
                    return Err(document(
                        "privilege",
                        format!("unknown mode `{other}` (root or capabilities)"),
                    ));
                }
            },
        };
        let capabilities = match mode {
            Mode::Capabilities => capabilities,
            Mode::Root => Vec::new(),
        };
        Ok(Some(Self {
            mode,
            capabilities,
            bounding_set_dropped: bounding_set.unwrap_or(true),
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
/// 3.14's is its own, the split word `blastit_view_python::immortal::marker`
/// writes — so the default runs the pass on every interpreter blastit knows,
/// and skips the ones it does not (32-bit, big-endian, free-threaded,
/// `Py_TRACE_REFS`, or a version outside that range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Immortalize {
    /// Immortalise wherever the marker is known — CPython 3.12 to 3.14, as
    /// `blastit_view_python::immortal::supported` decides — and skip it
    /// everywhere else. The default.
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

    /// The interpreter crate's own spelling of this setting.
    ///
    /// `blastit-view-python` owns the pass, so it owns the type, and it may not
    /// name this crate's `Config`. The map is therefore here, in the
    /// composition root: the environment variable is parsed where every other
    /// one is, and the crate that acts on it is handed a value it already
    /// knows.
    #[cfg(feature = "python")]
    pub(crate) fn setting(self) -> blastit_view_python::immortal::Setting {
        match self {
            Self::Auto => blastit_view_python::immortal::Setting::Auto,
            Self::On => blastit_view_python::immortal::Setting::On,
            Self::Off => blastit_view_python::immortal::Setting::Off,
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
/// is printed by stage 0 itself ([`crate::config::Config::preset_assumption_warning`]), because
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
    /// `BLASTIT_SECCOMP=0` was honoured outside a preset, so syscall and
    /// `egress` filtering are off (`R18`, `S6`).
    SeccompDisabled,
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
            Self::SeccompDisabled => "seccomp_disabled",
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
            "seccomp_disabled" => Ok(Self::SeccompDisabled),
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
            Self::SeccompDisabled => {
                "BLASTIT_SECCOMP=0: syscall and egress filtering are disabled, which is \
                 for emergency debugging only — set BLASTIT_SECCOMP=1"
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
    /// `BLASTIT_MANIFEST` is set but empty (design §15.1).
    ///
    /// A mistake, not a fallback: the operator named a file and named nothing.
    #[error(
        "BLASTIT_MANIFEST must not be empty: put ./Blastfile in the working directory or name a \
         file"
    )]
    EmptyManifest,
    /// The manifest could not be read or parsed while its `app`/`preload`
    /// fallback was resolved (`R7`, `R9`).
    ///
    /// The path is the discovered one and the reason comes from the parser; no
    /// value from the environment is ever carried.
    #[error("manifest {}: {reason}", display_path(path))]
    Manifest {
        /// The manifest path that was read.
        path: PathBuf,
        /// Why it could not be used.
        reason: String,
    },
}
