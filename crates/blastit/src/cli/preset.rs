//! Preset resolution for the command line (E1, `A-11`).
//!
//! A preset name is spelled in two places — `--preset NAME` and
//! `BLASTIT_PRESET` — and the sentence an unknown name gets is owned by
//! [`crate::config`], the module that knows every preset. Nothing here formats
//! that sentence itself, so the flag, the variable and the boot's own
//! diagnostic can never drift apart.

use super::Args;

/// A `--preset` value: the name this build knows, whitespace trimmed (E1).
///
/// Whitespace around a name is the operator's — an env file or a CI variable is
/// easy to pad by accident — and a value that is only whitespace is how a
/// deployment clears an inherited preset, which is a flag that was meant to
/// carry something rather than a name.
///
/// # Errors
///
/// A one-line message naming the preset and every preset this build has.
pub(super) fn name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("`--preset` wants a preset name".to_string());
    }
    if crate::config::Preset::find(name).is_none() {
        return Err(crate::config::preset_unknown(name, "--preset").to_string());
    }
    Ok(name.to_string())
}

/// Whether the preset `BLASTIT_PRESET` names — when no `--preset` was given — is
/// one this build knows (E1, `A-11`).
///
/// The variable is read at the same point the command line is resolved, which is
/// the boot's composition root; `--preset` wins over it exactly as
/// [`crate::config::Config::from_env`] documents, so a flag on the command line
/// means there is nothing left to check here. A value that is only whitespace is
/// how a deployment clears an inherited preset and is not an error.
///
/// # Errors
///
/// A one-line message naming the variable, the unknown name and the known
/// presets; the caller prints it with the usage and exits `2`.
pub(super) fn env_is_known(args: &Args) -> Result<(), String> {
    if args.preset.is_some() {
        return Ok(());
    }
    let Some(value) = std::env::var_os("BLASTIT_PRESET") else {
        return Ok(());
    };
    let Ok(value) = value.into_string() else {
        return Err("BLASTIT_PRESET: the value is not valid UTF-8".to_string());
    };
    let name = value.trim();
    if name.is_empty() || crate::config::Preset::find(name).is_some() {
        return Ok(());
    }
    Err(crate::config::preset_unknown(name, "BLASTIT_PRESET").to_string())
}
