//! The preset tables (E1), the precedence that decides which preset is in force,
//! and the record of what it supplied.

use std::collections::BTreeMap;

use blastit_events::Lifecycle;

use super::env::variable;
use super::settings::Config;
use super::types::{Mode, Warning};
use super::{ConfigError, invalid};

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
    /// This is what `Settings` falls back to when the environment does not
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

/// The one sentence an unknown preset name gets (E1).
///
/// `source` is the spelling the operator actually wrote — `--preset` or
/// `BLASTIT_PRESET` — and the list is every preset this build knows, because the
/// alternatives are what they need next. This function is the *single* owner of
/// that sentence: the boot's own lookup and the command line's early check both
/// call it, so the flag, the variable and the refusal cannot drift apart.
pub fn preset_unknown(name: &str, source: &str) -> ConfigError {
    invalid(
        source,
        format!(
            "unknown preset `{name}`: the known presets are {}",
            preset_list()
        ),
    )
}

/// The known presets, one per line and indented, for the usage text (E1).
pub fn preset_help() -> String {
    PRESETS
        .iter()
        .map(|preset| format!("  {}: {}", preset.name(), preset.summary()))
        .collect::<Vec<String>>()
        .join("\n")
}

/// Whether a [`Config`] may take its preset from `BLASTIT_PRESET` (E1,
/// `ADR-0005`).
///
/// Production `blastit` reads the variable. `blastit dev`, `blastit check` and
/// `blastit test` never do, so a preset exported in a developer's shell cannot
/// change a local or CI run: it would otherwise replace `dev`'s `pretty` log
/// default with the preset's `json`, and turn a harmless `BLASTIT_SECCOMP=0`
/// into the production abort (`R2`, `R3`, `R18`, `ADR-0005`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetPolicy {
    /// Honour `BLASTIT_PRESET`; `--preset` wins over it (production `blastit`).
    Env,
    /// Never read `BLASTIT_PRESET` (`blastit dev`, `check`, `test`).
    IgnoreEnv,
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
    pub(super) fn resolve(preset: &'static Preset) -> Self {
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

/// Where a configuration value comes from: the environment, or the preset when
/// the environment has nothing to say (E1).
///
/// The environment always wins, which is what makes `BLASTIT_PRESET=…` safe to
/// add to a deployment that already sets variables by hand: a preset only fills
/// in what nobody named. `BLASTIT_PRESET` itself and the per-proxy caps are
/// read from the environment directly — the first decides which preset this is,
/// and the second is named after the manifest's proxies.
pub(super) struct Settings {
    pub(super) preset: Option<&'static Preset>,
}

impl Settings {
    /// One variable: the environment's value, else the preset's, else unset.
    ///
    /// # Errors
    ///
    /// [`ConfigError::NotUnicode`] for a variable the process cannot read as
    /// text, exactly as [`variable`] reports it.
    pub(super) fn get(&self, name: &str) -> Result<Option<String>, ConfigError> {
        if let Some(value) = variable(name)? {
            return Ok(Some(value));
        }
        Ok(self
            .preset
            .and_then(|preset| preset.get(name))
            .map(str::to_string))
    }
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
/// [`PresetPolicy::IgnoreEnv`] skips the variable entirely: `blastit dev`,
/// `check` and `test` have no preset, so an exported `BLASTIT_PRESET` cannot
/// reach their configuration at all (`ADR-0005`).
///
/// # Errors
///
/// [`ConfigError::Invalid`] naming the source and listing the known presets.
pub(super) fn preset_choice(
    flag: Option<&str>,
    policy: PresetPolicy,
) -> Result<(Option<&'static Preset>, Vec<Warning>), ConfigError> {
    // The variable is read the way the flag is: whitespace around a name is the
    // operator's — an env file or a CI variable is easy to pad by accident — and
    // a value that is only whitespace is how a deployment clears an inherited
    // one.
    let from_env = match policy {
        PresetPolicy::Env => variable("BLASTIT_PRESET")?
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty()),
        PresetPolicy::IgnoreEnv => None,
    };
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
    Preset::find(name)
        .map(Some)
        .ok_or_else(|| preset_unknown(name, source))
}
