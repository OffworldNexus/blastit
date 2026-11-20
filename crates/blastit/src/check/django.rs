//! The one-shot `pytest-django` handshake (`R30`, `ADR-0004`).
//!
//! A `pytest-django` suite has to create its `test_*` database on the real
//! server before it can run, and every query after that must go through dbproxy
//! against that same test database. The real proxy inputs therefore travel in a
//! `0600` file that the Python gate reads and unlinks *before* Django builds its
//! settings; the suite's own environment keeps the secret-free rewrite and the
//! gate sends `{"op":"set_db"}` once the database exists.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use blastit_manifest::Manifest;
use serde_json::json;

use super::pytest::imports;

/// Inputs needed to prepare the one-shot `pytest-django` handshake (`R30`).
pub struct DjangoPreflight<'a> {
    pub manifest: &'a Manifest,
    pub prepared: &'a blastit_env::Prepared,
    pub platform: &'a BTreeMap<String, String>,
    pub socket_dir: &'a str,
}

/// Writes the one-shot Django handshake when this is a `pytest-django` project
/// (`R30`, `ADR-0004`).
///
/// A project counts as Django when `pytest-django` imports **and** it names its
/// settings (`DJANGO_SETTINGS_MODULE`) or carries a `manage.py`: pytest-django
/// cannot run a suite without one of those, and a project that merely has the
/// package installed must keep the non-Django environment.
///
/// When the handshake cannot be prepared, this is a start failure (exit `2`):
/// running anyway would let Django's migrations and flushes reach the
/// developer's development database.
pub fn django_setup(
    preflight: &DjangoPreflight<'_>,
    python: &Path,
    runtime: &Path,
) -> Result<Option<PathBuf>, String> {
    if !django_project(preflight.platform) || !imports(python, preflight.prepared, "pytest_django")?
    {
        return Ok(None);
    }
    let document = django_document(
        preflight.manifest,
        preflight.prepared,
        preflight.platform,
        preflight.socket_dir,
    )?;
    let path = runtime.join("django-setup.json");
    let bytes =
        serde_json::to_vec(&document).map_err(|error| format!("the Django handshake: {error}"))?;
    write_private(&path, &bytes)?;
    Ok(Some(path))
}

/// Whether this directory is a Django project `pytest-django` could set up.
///
/// `pytest-django` refuses to start without settings, so a package that merely
/// happens to be installed in the same environment is not a Django suite.
fn django_project(platform: &BTreeMap<String, String>) -> bool {
    platform.contains_key("DJANGO_SETTINGS_MODULE") || Path::new("manage.py").is_file()
}

/// The JSON the Python gate reads: the real inputs, the swapped ones, the
/// local dbproxy endpoint, and the test database to select (`ADR-0004`).
///
/// `real` never leaves this file; the gate applies it while Django loads its
/// settings and then restores `swapped`, so the credential barrier holds for
/// every test module and test body. Only the first `postgresql` proxy is handed
/// over: the supported Django shape is one proxied database.
pub(super) fn django_document(
    manifest: &Manifest,
    prepared: &blastit_env::Prepared,
    platform: &BTreeMap<String, String>,
    socket_dir: &str,
) -> Result<serde_json::Value, String> {
    let Some((name, upstream)) = prepared.upstreams().first() else {
        return Err(
            "pytest-django is installed but the Blastfile declares no postgresql proxy, \
             so its test database has nowhere to go; declare a `postgres` store: \
             `postgres $DATABASE_URL` (see docs/frameworks/python/django.md), or remove \
             pytest-django from the environment blastit audits"
                .to_string(),
        );
    };
    let port = blastit_env::postgresql::local_port(0).ok_or_else(|| {
        format!(
            "{}: no local port left for the postgresql proxies",
            name.as_str()
        )
    })?;
    let mut real = BTreeMap::new();
    let mut swapped = BTreeMap::new();
    if let Some(proxy) = manifest.proxies().get(name) {
        for variable in proxy.input().values() {
            if let Some(value) = platform.get(variable) {
                real.insert(variable.clone(), value.clone());
            }
        }
    }
    if let Some(outputs) = prepared.outputs().get(name) {
        for variable in outputs {
            if let Some(value) = prepared.app_env().get(variable) {
                swapped.insert(variable.clone(), value.clone());
            }
            if let Some(value) = platform.get(variable) {
                real.entry(variable.clone())
                    .or_insert_with(|| value.clone());
            }
        }
    }
    if real.is_empty() || swapped.is_empty() {
        return Err(no_input_variables(name.as_str()));
    }
    Ok(json!({
        "proxy": name.as_str(),
        "database": format!("test_{}", upstream.database()),
        "socket_dir": socket_dir,
        "port": port,
        "user": blastit_env::postgresql::LOCAL_USER,
        "real": real,
        "swapped": swapped,
    }))
}

/// The refusal a store that hands the Django handshake no variable is printed
/// with: the block to write, as a `Blastfile` writes it, and never the retired
/// per-route table.
///
/// `real` and `swapped` are both filled for every store a `postgres` block can
/// declare, so this is the guard against a model that changes that — one
/// sentence, asserted on its own rather than through a manifest that cannot
/// reach it.
pub(super) fn no_input_variables(store: &str) -> String {
    format!(
        "pytest-django is installed but proxy `{store}` has no input variables to hand \
         over, so its test database cannot be created; name them in that store's block: \
         `postgres {{ user $DB_USER; password $DB_PASSWORD }}` and export them before \
         running blastit check"
    )
}

/// Creates `path` `0600` and writes `bytes` into it, owned by this uid.
pub(super) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| {
            format!(
                "{}: {error} (the Django handshake needs a writable TMPDIR)",
                path.display()
            )
        })?;
    // The umask can only clear bits; force exactly what the gate requires.
    rustix::fs::fchmod(&file, rustix::fs::Mode::from_bits_truncate(0o600))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.flush()
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// The one-shot Django handshake file, removed however the run ends.
///
/// The suite unlinks it as soon as it has read it; this is the backstop for the
/// paths where the suite never started (a spawn failure, a dead dbproxy), so a
/// crash cannot leave the real proxy inputs in `/tmp` after `blastit check`
/// returns (`R30`).
pub struct Handshake {
    path: Option<PathBuf>,
}

impl Handshake {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

impl Drop for Handshake {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}
