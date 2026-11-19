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
//!
//! One module per concern, because the file this grew in was four thousand
//! lines of it: [`types`] holds the value types and the spellings they share,
//! [`settings`] the [`Config`] struct and its documented defaults, [`env`] the
//! one place the process environment is read, [`parse`] the `BLASTIT_*` value
//! parsers, [`preset`] the preset tables and their precedence, [`validate`] the
//! combinations only meaningful together, [`manifest_io`] the manifest's
//! discovery and its one sentence per refusal, [`document`] the writer of the
//! stage-1 document, and [`wire`] the `serde` structs that read it back.

mod document;
mod env;
mod manifest_io;
mod parse;
mod preset;
mod settings;
mod types;
mod validate;
mod wire;

#[cfg(test)]
mod tests;
#[cfg(all(test, feature = "php"))]
mod tests_php;

// `ManifestReadError` is reachable from every entry point, but its name is
// never written down: `read_manifest`'s callers use `.sentence()` on the value
// it returns. The path stays what it was when the module was one file.
#[allow(unused_imports)]
pub(crate) use manifest_io::{
    MANIFEST_ENV, ManifestReadError, display_path, manifest_path, missing_manifest, read_manifest,
};
pub use manifest_io::{mount_point, seccomp_requested, view_dir};
pub use preset::{
    PRESETS, Preset, PresetChoice, PresetPolicy, preset_help, preset_list, preset_unknown,
};
pub use settings::{Config, MIN_DEBT_TTL, TRUSTED_PROXIES_DEFAULT};
pub use types::{ConfigError, Immortalize, LogFormat, Mode, Privilege, SpawnMode, Warning};

/// A one-line error naming the variable.
pub(super) fn invalid(variable: &str, reason: String) -> ConfigError {
    ConfigError::Invalid {
        variable: variable.to_string(),
        reason,
    }
}

/// A one-line document error naming the field.
pub(super) fn document(field: &'static str, reason: String) -> ConfigError {
    ConfigError::Document { field, reason }
}
