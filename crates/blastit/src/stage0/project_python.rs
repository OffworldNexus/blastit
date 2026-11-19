//! The project's interpreter, as stage 0 finds it (`MB.40.8`).
//!
//! A standalone `blastit` links a `libpython` and carries no project's
//! `site-packages`: it embeds CPython in stage 1 and reads the application's
//! modules from the directory it was started in. A developer's project is a
//! virtualenv, though, and `blastit dev` has to import the application from it —
//! so stage 0 reads two facts here and hands both across the `execve`:
//!
//! * the interpreter prefix the build recorded (`BLASTIT_PYTHON_HOME`), as the
//!   `PYTHONHOME` an embedded stage 1 needs to find `encodings` under `env -i`
//!   and after the binary is copied out of its build tree;
//! * the project venv — `$VIRTUAL_ENV` when activated and the boot is `dev` or
//!   `check`, else `<manifest_dir>/.venv` — whose `site-packages` goes onto the
//!   configuration's `sys_path`, matched against the CPython minor the binary
//!   embeds (`BLASTIT_PYTHON_MM`) so a venv built for another minor is refused
//!   by name instead of failing at the first import.
//!
//! **A production boot never reads `$VIRTUAL_ENV`.** The variable belongs to an
//! operator's shell, not to the deployment whose tree stage 0 is serving, and a
//! server that picked up an unrelated environment's `site-packages` would import
//! code nobody deployed. Only `<manifest_dir>/.venv` is accepted there, and
//! stage 1's `check_view_paths` authorises every entry it adds.
//!
//! Everything here is unprivileged and filesystem-only: no interpreter runs in
//! this process, so nothing here can be made to execute project code.

#[cfg(feature = "python")]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::host::{Host, Install};

/// The `PYTHONHOME` a standalone install hands stage 1 (`MB.40.8`).
///
/// `env` is the stage-1 environment stage 0 is about to `execve`: an operator's
/// own `PYTHONHOME` always wins, because it names an interpreter this build did
/// not choose. Compiled out of a build that links no `libpython` (the build
/// script emits no home either), so a standalone archive carries no build-host
/// path.
#[cfg(feature = "python")]
pub(super) fn python_home(install: Install, env: &BTreeMap<String, String>) -> Option<String> {
    if install != Install::Binary || env.contains_key("PYTHONHOME") {
        return None;
    }
    let home = option_env!("BLASTIT_PYTHON_HOME")?;
    if !Path::new(home).is_dir() {
        return None;
    }
    Some(home.to_string())
}

/// A project virtualenv stage 0 found, and the CPython release it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Venv {
    /// The venv root, as the `runtime` line names it.
    pub(super) root: PathBuf,
    /// The venv's CPython `major.minor`, from `lib/pythonX.Y/site-packages`.
    pub(super) version: String,
    /// `major.minor.patch` when `pyvenv.cfg` records one, else [`Self::version`].
    ///
    /// `uv venv` writes `version = 3.14.10` and `python -m venv` writes
    /// `version_info = 3.14.10.final.0`; the `runtime` line names the release a
    /// developer asked for, not just the ABI it matches.
    pub(super) release: String,
    /// That `site-packages`, as `os.fsencode` bytes (§4).
    pub(super) site_packages: Vec<u8>,
}

/// The venv a standalone `entrypoint python` boot serves from (`MB.40.8`).
///
/// `Ok(None)` for every other boot: a wheel install's interpreter already has
/// the project's `site-packages`, and an all-static or PHP file has no WSGI
/// callable to import.
///
/// `allow_env_var` is `true` for `dev` and `check` — the two unprivileged modes a
/// developer runs from their own shell — and `false` for the production boot,
/// which may only use the `.venv` that sits beside the manifest it is serving.
///
/// # Errors
///
/// The one-line refusal naming both CPython versions when the project's venv
/// was built for a different minor than the interpreter this binary embeds.
pub(super) fn project_venv(
    config: &Config,
    host: &Host,
    allow_env_var: bool,
) -> Result<Option<Venv>, String> {
    if host.install() != Install::Binary || config.app.is_none() {
        return Ok(None);
    }
    // The variable is read here, in the composition root, and passed down: the
    // discovery is then a pure function of its arguments, which is what a unit
    // test can exercise without touching the process environment.
    let activated = virtual_env(allow_env_var, std::env::var_os("VIRTUAL_ENV"));
    discover(
        &config.manifest_dir,
        activated.as_deref(),
        option_env!("BLASTIT_PYTHON_MM"),
    )
}

/// The `$VIRTUAL_ENV` a boot is allowed to honour (`MB.40.8`).
///
/// A production boot gets `None`: an operator's shell variable must not decide
/// which code a server imports. The value is a parameter, so the decision is a
/// pure function with no process environment of its own.
fn virtual_env(allow_env_var: bool, value: Option<OsString>) -> Option<OsString> {
    if !allow_env_var {
        return None;
    }
    value.filter(|value| !value.is_empty())
}

/// [`project_venv`] without the configuration: which venv, and whether its
/// CPython is the embedded one.
///
/// `embedded` is the `major.minor` the build recorded, when it recorded one. A
/// venv that only holds `site-packages` for another minor is refused rather
/// than added to `sys.path`: the extension modules there would either be
/// ignored or crash on import, and a one-line boot refusal is the developer's
/// fix.
pub(super) fn discover(
    manifest_dir: &Path,
    virtual_env: Option<&OsStr>,
    embedded: Option<&str>,
) -> Result<Option<Venv>, String> {
    let Some(root) = venv_root(manifest_dir, virtual_env) else {
        return Ok(None);
    };
    let available = versions(&root);
    let Some(version) = choose(&available, embedded) else {
        return Ok(None);
    };
    if let Some(embedded) = embedded
        && version != embedded
    {
        return Err(format!(
            "the virtualenv {} was built for CPython {version}, but this blastit embeds \
             CPython {embedded}\n  \
             rebuild it:  uv venv --clear --python {embedded}\n  \
             or run from the project venv:  uv add blastit && uv run blastit dev",
            root.display()
        ));
    }
    let Some(directory) = site_packages(&root, &version) else {
        return Ok(None);
    };
    Ok(Some(Venv {
        release: release(&root, &version),
        root,
        version,
        site_packages: directory.as_os_str().as_bytes().to_vec(),
    }))
}

/// The venv root a project boot serves from: the activated one, else `.venv`.
///
/// `pyvenv.cfg` is what makes a directory a venv rather than a coincidence of
/// names (`python -m venv`, `uv venv` and `virtualenv` all write it), so a
/// stray `.venv/` beside the `Blastfile` is never mistaken for one.
fn venv_root(manifest_dir: &Path, virtual_env: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(value) = virtual_env {
        let path = PathBuf::from(value);
        if is_venv(&path) {
            return Some(path);
        }
    }
    let local = manifest_dir.join(".venv");
    is_venv(&local).then_some(local)
}

/// Whether `path` is a virtualenv directory.
fn is_venv(path: &Path) -> bool {
    path.is_dir() && path.join("pyvenv.cfg").is_file()
}

/// The CPython minors under `<venv>/lib` or `<venv>/lib64` that hold a
/// `site-packages` directory.
fn versions(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for lib in ["lib", "lib64"] {
        let Ok(entries) = std::fs::read_dir(root.join(lib)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(version) = name.strip_prefix("python") else {
                continue;
            };
            if !is_major_minor(version) {
                continue;
            }
            if entry.path().join("site-packages").is_dir() {
                found.insert(version.to_string());
            }
        }
    }
    found
}

/// Which version this boot serves: the embedded one when the venv has it, the
/// venv's own otherwise (so the caller can refuse it with both numbers).
fn choose(available: &BTreeSet<String>, embedded: Option<&str>) -> Option<String> {
    if let Some(embedded) = embedded
        && available.contains(embedded)
    {
        return Some(embedded.to_string());
    }
    available.iter().next().cloned()
}

/// `<venv>/<lib>/python<version>/site-packages`, in `lib` before `lib64`.
fn site_packages(root: &Path, version: &str) -> Option<PathBuf> {
    for lib in ["lib", "lib64"] {
        let candidate = root
            .join(lib)
            .join(format!("python{version}"))
            .join("site-packages");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// The full `major.minor.patch` a `pyvenv.cfg` records, or `version`.
///
/// Only a value whose `major.minor` is the directory this boot matched is read,
/// so a stale file cannot name a release the `site-packages` beside it was not
/// built for.
fn release(root: &Path, version: &str) -> String {
    let Ok(text) = std::fs::read_to_string(root.join("pyvenv.cfg")) else {
        return version.to_string();
    };
    for line in text.lines() {
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if !matches!(name.trim(), "version_info" | "version") {
            continue;
        }
        let mut parts = value.trim().split('.');
        let (Some(major), Some(minor), Some(rest)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let patch: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if patch.is_empty() {
            continue;
        }
        if format!("{major}.{minor}") == version {
            return format!("{version}.{patch}");
        }
    }
    version.to_string()
}

/// Whether `text` is exactly `digits "." digits` (`3.14`, never `3` or `3.14t`).
fn is_major_minor(text: &str) -> bool {
    let mut parts = text.split('.');
    let (Some(major), Some(minor), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !major.is_empty()
        && !minor.is_empty()
        && major.bytes().all(|byte| byte.is_ascii_digit())
        && minor.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A directory with a name of its own, removed when dropped.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("blastit-venv-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        /// Makes `path` a venv holding `site-packages` for `version`.
        fn venv(&self, path: &Path, version: &str) -> PathBuf {
            self.venv_with(path, version, "")
        }

        /// The same, with `extra` lines appended to `pyvenv.cfg`.
        fn venv_with(&self, path: &Path, version: &str, extra: &str) -> PathBuf {
            std::fs::create_dir_all(path.join("bin")).unwrap();
            std::fs::write(path.join("pyvenv.cfg"), format!("home = /usr/bin\n{extra}")).unwrap();
            let site = path
                .join("lib")
                .join(format!("python{version}"))
                .join("site-packages");
            std::fs::create_dir_all(&site).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
            site
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn a_matching_venv_contributes_its_site_packages() {
        let dir = TempDir::new("match");
        let project = dir.path.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let site = dir.venv(&project.join(".venv"), "3.14");

        assert!(
            discover(&project, None, Some("3.14"))
                .unwrap()
                .is_some_and(|venv| venv.version == "3.14"),
            "a .venv built for the embedded minor is served"
        );
        let venv = discover(&project, None, Some("3.14")).unwrap().unwrap();
        assert_eq!(venv.root, project.join(".venv"));
        assert_eq!(
            venv.site_packages,
            site.as_os_str().as_bytes(),
            "the path crosses the execve as os.fsencode bytes"
        );
        assert_eq!(venv.release, "3.14", "no pyvenv version line, no patch");
    }

    #[test]
    fn a_venv_for_another_minor_is_refused_by_name() {
        let dir = TempDir::new("mismatch");
        let project = dir.path.join("project");
        std::fs::create_dir_all(&project).unwrap();
        dir.venv(&project.join(".venv"), "3.12");

        let error = discover(&project, None, Some("3.14")).unwrap_err();
        assert!(error.contains("3.12"), "{error}");
        assert!(error.contains("3.14"), "{error}");
        assert!(error.contains("uv venv --clear --python 3.14"), "{error}");
        assert!(
            error.contains("uv add blastit && uv run blastit dev"),
            "{error}"
        );
        assert!(
            error.contains("\n  rebuild it:"),
            "the stage-0 advice is one sentence then indented fixes: {error}"
        );
    }

    #[test]
    fn the_release_is_read_from_pyvenv_cfg() {
        for (extra, expected) in [
            ("version = 3.14.10\n", "3.14.10"),
            ("version_info = 3.14.10.final.0\n", "3.14.10"),
            ("version = 3.12.4\n", "3.14"),
        ] {
            let dir = TempDir::new("release");
            let project = dir.path.join("project");
            std::fs::create_dir_all(&project).unwrap();
            dir.venv_with(&project.join(".venv"), "3.14", extra);
            let venv = discover(&project, None, Some("3.14"))
                .unwrap()
                .expect("the venv");
            assert_eq!(venv.release, expected, "{extra:?}");
        }
    }

    #[test]
    fn an_activated_venv_wins_over_the_local_one_in_dev() {
        let dir = TempDir::new("activated");
        let project = dir.path.join("project");
        std::fs::create_dir_all(&project).unwrap();
        dir.venv(&project.join(".venv"), "3.12");
        let active = dir.path.join("active");
        dir.venv(&active, "3.14");

        let venv = discover(&project, Some(active.as_os_str()), Some("3.14"))
            .unwrap()
            .expect("the activated venv");
        assert_eq!(venv.root, active);
        assert_eq!(venv.version, "3.14");
    }

    #[test]
    fn a_production_boot_never_reads_the_activated_venv() {
        // `dev` and `check` honour the shell variable; a production boot only
        // ever accepts the `.venv` beside the manifest it serves.
        assert_eq!(
            virtual_env(true, Some(OsString::from("/somewhere/activated"))),
            Some(OsString::from("/somewhere/activated"))
        );
        assert_eq!(
            virtual_env(false, Some(OsString::from("/somewhere/activated"))),
            None
        );
        assert_eq!(virtual_env(true, Some(OsString::new())), None);
    }

    #[test]
    fn lib64_is_read_when_lib_has_no_site_packages() {
        let dir = TempDir::new("lib64");
        let project = dir.path.join("project");
        let venv_root = project.join(".venv");
        std::fs::create_dir_all(venv_root.join("bin")).unwrap();
        std::fs::write(venv_root.join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();
        let site = venv_root
            .join("lib64")
            .join("python3.14")
            .join("site-packages");
        std::fs::create_dir_all(&site).unwrap();

        let venv = discover(&project, None, Some("3.14"))
            .unwrap()
            .expect("lib64 holds the venv");
        assert_eq!(venv.site_packages, site.as_os_str().as_bytes());
    }

    #[test]
    fn a_project_without_a_venv_contributes_nothing() {
        let dir = TempDir::new("none");
        let project = dir.path.join("project");
        std::fs::create_dir_all(&project).unwrap();
        // Denied twin: a directory called `.venv` is not a venv, and an empty
        // one has nothing to import.
        std::fs::create_dir_all(project.join(".venv")).unwrap();
        assert!(discover(&project, None, Some("3.14")).unwrap().is_none());
    }

    #[test]
    fn a_build_with_no_recorded_minor_takes_the_venvs_own() {
        let dir = TempDir::new("unrecorded");
        let project = dir.path.join("project");
        std::fs::create_dir_all(&project).unwrap();
        dir.venv(&project.join(".venv"), "3.13");
        let venv = discover(&project, None, None).unwrap().expect("the venv");
        assert_eq!(venv.version, "3.13");
    }

    #[test]
    fn a_static_boot_discovers_no_project_venv() {
        let dir = TempDir::new("static");
        let config = Config {
            manifest_dir: dir.path.clone(),
            ..Config::default()
        };
        dir.venv(&dir.path.join(".venv"), "3.14");
        // No callable: a static mount answers every request, so no interpreter
        // is ever asked for this file's modules.
        assert!(
            project_venv(&config, &Host::binary(), true)
                .unwrap()
                .is_none()
        );
    }
}
