//! The manifest's discovery, its one reader, and the one sentence each refusal
//! prints (`ADR-0002`, design §15.1).

use std::path::{Path, PathBuf};

use blastit_manifest::Manifest;

use super::parse::parse_bool;
use super::types::ConfigError;

/// The one environment variable that names the manifest (`ADR-0002`, design
/// §15.1).
///
/// Every entry point and the CLI's refusals spell it through this constant, so
/// `cli.rs` cannot grow a second copy of the name and the help text cannot
/// disagree with the variable a boot reads.
pub(crate) const MANIFEST_ENV: &str = "BLASTIT_MANIFEST";

/// The private scratch directory of the view running under `uid` (§2.2, §7.2).
pub fn view_dir(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/blastit/views/{uid}"))
}

/// Where the manifest lives: `BLASTIT_MANIFEST` when set, else `./Blastfile` in
/// the starting working directory (design §15.1, `ADR-0002`).
///
/// This is the **one** discovery function: `blastit` (boot), `blastit dev`,
/// `blastit check` and `blastit fmt` all call it, so no entry point can read a
/// different file than another. There is **no** upward search: a monorepo
/// ancestor's policy is never picked up by accident. A set-but-empty
/// `BLASTIT_MANIFEST` is a mistake, not a fallback.
///
/// Nothing inspects the body to choose a path: a file that is not a `Blastfile`
/// is an ordinary `Blastfile:<line>:<col>` syntax error once it is parsed.
///
/// # Errors
///
/// [`ConfigError::EmptyManifest`] for a set-but-empty variable, and
/// [`ConfigError::NotUnicode`] for one the process cannot read as text.
pub(crate) fn manifest_path() -> Result<PathBuf, ConfigError> {
    match std::env::var_os(MANIFEST_ENV) {
        Some(value) => {
            let value = value.into_string().map_err(|_| ConfigError::NotUnicode {
                variable: MANIFEST_ENV.to_string(),
            })?;
            if value.is_empty() {
                return Err(ConfigError::EmptyManifest);
            }
            Ok(PathBuf::from(value))
        }
        None => Ok(PathBuf::from("./Blastfile")),
    }
}

/// The one sentence a missing manifest is reported with, at every entry point
/// (design §15.1).
///
/// It names `./Blastfile` — the default discovery — and `BLASTIT_MANIFEST`, the
/// variable that names another path, and nothing else: no second file name, no
/// "did you mean", no detection of a sibling file. A `BLASTIT_MANIFEST` that
/// names a missing path is named by the path it tried, which is the operator's
/// own string, never a second default.
pub(crate) fn missing_manifest(path: &Path) -> String {
    format!(
        "{}: no such file or directory (put a Blastfile in the working directory, or set \
         BLASTIT_MANIFEST)",
        display_path(path)
    )
}

/// The manifest path as one message can carry it (§18, §15.1).
///
/// `BLASTIT_MANIFEST` is the operator's own string, and a path holding a newline
/// or a control byte would otherwise print two lines out of a one-line sentence.
/// [`blastit_manifest::clipped`] escapes the control bytes and cuts the value at
/// 120 `char`s; every manifest-path message goes through this one helper, so the
/// rule has one home rather than a folded copy per entry point.
pub(crate) fn display_path(path: &Path) -> String {
    blastit_manifest::clipped(&path.display().to_string())
}

/// Why a discovered `Blastfile` could not be read (§15.1).
///
/// The two answers are kept apart because every entry point renders them
/// differently: absence is §15.1's one sentence, and any other refusal keeps
/// `blastfile::read`'s own message behind the path. `blastfile::read` is the one
/// reader, so a FIFO, a device or a file over the 16 MiB cap is refused from its
/// metadata before anything is opened — no entry point can block on it.
pub(crate) enum ManifestReadError {
    /// The path is not there.
    Missing,
    /// [`blastit_manifest::blastfile::read`] refused it.
    Refused(blastit_manifest::ManifestErrors),
}

impl ManifestReadError {
    /// The one sentence this failure prints, with the path escaped.
    pub(crate) fn sentence(&self, path: &Path) -> String {
        match self {
            Self::Missing => missing_manifest(path),
            Self::Refused(errors) => format!("{}: {}", display_path(path), errors_line(errors)),
        }
    }
}

/// The reader's refusals on one line, `; ` between them.
///
/// [`blastit_manifest::ManifestErrors`]' own `Display` is a header and one
/// indented line per problem — right for a boot that reports a whole file, wrong
/// for the one sentence a missing or unreadable *path* produces (§15.1). A read
/// refusal has no position to line up, so joining is lossless.
fn errors_line(errors: &blastit_manifest::ManifestErrors) -> String {
    errors
        .errors()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// Reads the discovered manifest through the one reader (`blastfile::read`).
///
/// A missing file is [`ManifestReadError::Missing`]; every other failure is the
/// reader's refusal, rendered as `<path>: <reason>` by
/// [`ManifestReadError::sentence`]. The `stat` is only consulted on the error
/// path, to tell the two apart without parsing the operating system's sentence.
pub(crate) fn read_manifest(path: &Path) -> Result<String, ManifestReadError> {
    blastit_manifest::blastfile::read(path).map_err(|errors| {
        if std::fs::metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            ManifestReadError::Missing
        } else {
            ManifestReadError::Refused(errors)
        }
    })
}

/// The directory the manifest lives in, absolute: what a `static` mount's `root`
/// is relative to (`R2`, `R13`).
///
/// A bare `Blastfile` has an empty parent, which means the working
/// directory. The path is made absolute once, here, because stage 1 runs from a
/// different working directory and the responder opens `root` beneath it.
pub(super) fn manifest_dir(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::path::absolute(parent).unwrap_or_else(|_| parent.to_path_buf())
}

/// The manifest's top-level `app` and `preload`, for the fields the
/// environment leaves open (`R7`, `R9`).
///
/// A missing file is not an error here: the boot that needs a manifest reports
/// its absence with the path it tried. A file that exists but does not parse is
/// an error, because silently booting with a different callable is worse than
/// naming the parse failure.
pub(super) fn manifest_wiring(path: &Path) -> Result<(Option<String>, Vec<String>), ConfigError> {
    let source = match read_manifest(path) {
        Ok(source) => source,
        Err(ManifestReadError::Missing) => {
            return Ok((None, Vec::new()));
        }
        Err(ManifestReadError::Refused(errors)) => {
            return Err(ConfigError::Manifest {
                path: path.to_path_buf(),
                reason: errors_line(&errors),
            });
        }
    };
    let manifest =
        Manifest::from_blastfile_str(&source).map_err(|errors| ConfigError::Manifest {
            path: path.to_path_buf(),
            reason: errors.to_string(),
        })?;
    Ok((
        manifest.app().map(str::to_string),
        manifest.preload().to_vec(),
    ))
}

/// Whether `BLASTIT_SECCOMP` asks for the §7.3 filters, as the `boot` event
/// needs it (§8.1).
///
/// [`crate::config::Config::from_env`] parses the same variable a moment later, but stage 0
/// prints `boot` before it parses anything, so this reads the variable on its
/// own — through the same `parse_bool` the rest of the configuration uses, so
/// the two cannot drift apart. A value that is not a boolean at all means on
/// here; [`crate::config::Config::from_env`] rejects it a moment later, naming
/// the variable.
pub fn seccomp_requested() -> bool {
    let Ok(Some(value)) = super::env::variable("BLASTIT_SECCOMP") else {
        return true;
    };
    parse_bool("BLASTIT_SECCOMP", &value).unwrap_or(true)
}

/// The mount point a view reports as `SCRIPT_NAME`.
///
/// The same normalisation `blastit_scheduler` applies to
/// `BLASTIT_SCRIPT_NAME`: trailing slashes are dropped, a missing leading slash
/// is added, and an empty result means "mounted at the root". Both the WSGI
/// view (`zygote`) and the embedded-PHP view (`php_view`) report the mount
/// point from this one implementation.
pub fn mount_point(script_name: Option<&str>) -> String {
    let Some(value) = script_name else {
        return String::new();
    };
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}
