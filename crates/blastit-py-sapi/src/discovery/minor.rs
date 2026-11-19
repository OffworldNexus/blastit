//! Which CPython a venv is: `pyvenv.cfg` first, directories only as a last
//! resort (design §2.3).
//!
//! A `uv venv` upgraded in place leaves the old minor's tree beside the new one,
//! so a directory scan cannot say which interpreter a venv holds: this module
//! reads what the file states and only scans when nothing states a version.

use std::path::{Path, PathBuf};

use super::{DEFAULT_MINOR, DiscoveryError};

/// What `pyvenv.cfg` states: the interpreter's `home`, the venv's minor and
/// the release it names.
pub(super) struct PyvenvCfg {
    /// `home = <dir>`'s parent: the install prefix (`PYTHONHOME`).
    pub(super) home: PathBuf,
    /// `version`/`version_info`'s `major.minor`, when it is a CPython 3.
    pub(super) minor: Option<u32>,
    /// The full `major.minor.patch` the same line states (`3.14.10`).
    pub(super) release: Option<String>,
}

/// Reads `pyvenv.cfg`'s `home`, `version`/`version_info` (`MB.40.8`, §2.3).
pub(super) fn read_pyvenv(root: &Path) -> Option<PyvenvCfg> {
    let text = std::fs::read_to_string(root.join("pyvenv.cfg")).ok()?;
    let mut home = None;
    let mut minor = None;
    let mut release = None;
    for line in text.lines() {
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        match name.trim() {
            "home" => home = Some(PathBuf::from(value.trim())),
            "version" | "version_info" => {
                minor = parse_cfg_minor(value.trim()).or(minor);
                release = parse_cfg_release(value.trim()).or(release);
            }
            _ => {}
        }
    }
    let home = home?;
    Some(PyvenvCfg {
        home: home.parent().unwrap_or(&home).to_path_buf(),
        minor,
        release,
    })
}

/// `3.14.10` / `3.14.10.final.0` -> `"3.14.10"`.
pub(super) fn parse_cfg_release(text: &str) -> Option<String> {
    let mut parts = text.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch: String = parts
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (major == 3 && !patch.is_empty()).then(|| format!("{major}.{minor}.{patch}"))
}

/// `3.14.10` / `3.14.10.final.0` -> `14`.
pub(super) fn parse_cfg_minor(text: &str) -> Option<u32> {
    let mut parts = text.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    (major == 3).then_some(minor)
}

/// `PYTHONHOME`: the venv's recorded `home` parent, else the exe's prefix.
pub(super) fn python_home(
    interpreter: &Path,
    config: Option<&PyvenvCfg>,
    command: &str,
) -> Result<PathBuf, DiscoveryError> {
    if let Some(config) = config {
        return Ok(config.home.clone());
    }
    let unresolvable = |detail: String| DiscoveryError::Unresolvable {
        command: command.to_owned(),
        path: interpreter.to_path_buf(),
        detail,
        minor: DEFAULT_MINOR.to_owned(),
    };
    let resolved =
        std::fs::canonicalize(interpreter).map_err(|err| unresolvable(err.to_string()))?;
    resolved
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| unresolvable("it names no installation prefix".to_owned()))
}

/// The interpreter's minor and whether it is free-threaded.
///
/// The order is the interpreter's own name, then `pyvenv.cfg`'s recorded
/// version, and only then the directories.
pub(super) fn minor_of(
    interpreter: &Path,
    venv: Option<&Path>,
    home: &Path,
    recorded: Option<u32>,
    command: &str,
) -> Result<(u32, bool), DiscoveryError> {
    if let Some(name) = interpreter.file_name()
        && let Some(found) = parse_minor(&name.to_string_lossy())
    {
        return Ok(found);
    }
    if let Some(minor) = recorded {
        return Ok((minor, free_threaded_tree(venv, home, minor)));
    }
    let mut directories: Vec<PathBuf> = Vec::new();
    if let Some(root) = venv {
        directories.push(root.join("lib"));
        directories.push(root.join("lib64"));
    }
    directories.push(home.join("lib"));
    directories.push(home.join("lib64"));
    for directory in &directories {
        if let Some(found) = minor_in_dir(Some(directory)) {
            return Ok(found);
        }
    }
    Err(DiscoveryError::UnreadableMinor {
        command: command.to_owned(),
        path: interpreter.to_path_buf(),
        detail: "its name and pyvenv.cfg both state no version".to_owned(),
    })
}

/// Whether `minor` has a free-threaded tree or `libpython3.<m>t.so`.
fn free_threaded_tree(venv: Option<&Path>, home: &Path, minor: u32) -> bool {
    let mut directories: Vec<PathBuf> = Vec::new();
    if let Some(root) = venv {
        directories.push(root.join("lib"));
        directories.push(root.join("lib64"));
    }
    directories.push(home.join("lib"));
    directories.push(home.join("lib64"));
    directories.iter().any(|directory| {
        std::fs::read_dir(directory).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                parse_minor(&entry.file_name().to_string_lossy())
                    .is_some_and(|(found, free)| found == minor && free)
            })
        })
    })
}

/// The highest minor named in `dir` by `python3.<m>[t]` or `libpython3.<m>…`.
fn minor_in_dir(directory: Option<&Path>) -> Option<(u32, bool)> {
    let entries = std::fs::read_dir(directory?).ok()?;
    let mut best: Option<(u32, bool)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((minor, free_threaded)) = parse_minor(&name) else {
            continue;
        };
        if best.is_none_or(|(current, _)| minor > current) {
            best = Some((minor, free_threaded));
        }
    }
    best
}

/// `python3.14`, `python3.13t`, `libpython3.14.so.1.0` -> `(minor, free)`.
pub(super) fn parse_minor(text: &str) -> Option<(u32, bool)> {
    let rest = text
        .strip_prefix("libpython3.")
        .or_else(|| text.strip_prefix("python3."))?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let minor: u32 = digits.parse().ok()?;
    let tail = &rest[digits.len()..];
    let free_threaded = tail.starts_with('t');
    // A name that continues after a `t` must still be an object (`t.so…`), so
    // `python3.13types` is not read as a free-threaded 3.13.
    if free_threaded && !(tail.len() == 1 || tail[1..].starts_with(".so")) {
        return None;
    }
    if !free_threaded && !(tail.is_empty() || tail.starts_with('.')) {
        return None;
    }
    Some((minor, free_threaded))
}
