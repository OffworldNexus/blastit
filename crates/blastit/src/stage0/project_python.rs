//! The Python half of a stage-0 boot: the call site, not the discovery.
//!
//! Every search, every refusal sentence and every ELF gate lives in
//! `blastit_py_sapi::discovery`/`loader` (design §2). What is left here is the
//! part only the composition root can do:
//!
//! * the **activation gate** (§2.1): a PHP-only or static-only boot returns
//!   before any search, `open`, or `dlopen`, structurally;
//! * the process environment, read once and handed in as a [`DiscoveryEnv`];
//! * `blastit_sys::pin` on the `libpython` and the shim, with the `.dynsym`
//!   gate, so the `(st_dev, st_ino)` pair can travel in the configuration;
//! * the deployment-only `Provenance` check ([`Boot::Serve`], never `Dev`), as a
//!   variant rather than a string a typo could miss;
//! * the minor-agreement refusal: a venv built for another CPython than the one
//!   this image links is named before stage 1 can fail inside `Py_Initialize`.
//!
//! Stage 0 never executes Python and never `dlopen`s anything: the two pinned
//! descriptors are opened, verified, and handed across the barrier.

use std::path::{Path, PathBuf};

use blastit_manifest::Manifest;
use blastit_py_sapi::abi::dotted_py_minor;
use blastit_py_sapi::discovery::{self, DevEnv, DiscoveredPython, DiscoveryEnv, ProductionEnv};
use blastit_py_sapi::loader::gate_libpython;
use blastit_sys::pin::SharedObject;

use crate::config::{Config, PythonHandoff};
use crate::host::{Claim, Install, Provenance};

/// Which stage-0 boot is asking.
///
/// A deployment runs the `R7`/`B2-R5` `Provenance` gate and has no shell to
/// read; a `dev`/`check` boot may use the activated venv and `PATH`. The
/// distinction is a variant, not a `&str`: a typo in the old comparison would
/// have turned the deployment gate off silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Boot {
    /// `blastit dev` and `blastit check`.
    Dev,
    /// A deployment (`serve`).
    Serve,
}

impl Boot {
    /// The verb a refusal names (`blastit {verb}:`).
    const fn verb(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Serve => "serve",
        }
    }

    /// Whether this boot authorises a deployment's objects and trees.
    const fn is_deployment(self) -> bool {
        matches!(self, Self::Serve)
    }
}

/// What stage 0 resolved, kept alive until the `execve` that hands it on.
///
/// The two [`SharedObject`]s own the descriptors: dropping them would close the
/// very fds the configuration records, so they live as long as the boot that
/// pinned them.
pub(super) struct PythonBoot {
    /// The record stage 1 re-verifies and loads.
    pub(super) handoff: PythonHandoff,
    /// The pinned `libpython`; held open, never loaded here.
    _lib: SharedObject,
    /// The pinned shim; held open, never loaded here.
    _shim: SharedObject,
    /// The `runtime` line's version (`3.14.10`, else `3.14`).
    version: String,
    /// The `runtime` line's environment (the venv root, else the prefix).
    environment: String,
}

impl PythonBoot {
    /// The `(version, environment)` pair of §8.1's `runtime` line.
    pub(super) fn runtime(&self) -> (&str, &str) {
        (&self.version, &self.environment)
    }
}

/// The activation gate of §2.1, as the composition root decides it.
///
/// Evaluated **after** `static_app_refusal`: a manifest whose every request is
/// answered by its own executors, and which declares no Python app, never
/// reaches the discovery — no search, no `open`, no `dlopen`.
pub(super) fn plan(config: &Config, manifest: &Manifest) -> bool {
    discovery::activates(manifest.can_boot_without_app(), config.app.is_some())
}

/// Resolves, pins and records the project's Python, or `None` when inactive.
///
/// # Errors
///
/// One of §2.3's/§2.4's sentences — including "no interpreter was found", which
/// is a refusal in every build that can serve WSGI — the minor-agreement
/// sentence, a refused ELF, or a deployment whose paths are not protected.
pub(super) fn boot(
    config: &Config,
    manifest: &Manifest,
    boot: Boot,
) -> Result<Option<PythonBoot>, String> {
    if !plan(config, manifest) {
        return Ok(None);
    }
    let env = discovery_env(boot);
    let found = discovery::discover(&env, boot.verb(), &config.manifest_dir)
        .map_err(|error| error.to_string())?;
    agree_minor(boot, &found)?;
    discover(&found, config, boot).map(Some)
}

/// Refuses a venv built for another CPython than the one this image links.
///
/// `MB.40.8`'s check, restored after the discovery moved: `PYTHONHOME` for the
/// linked interpreter is the build's own (transitional), and stage 1's
/// `Py_Initialize` cannot read another minor's `site-packages`. The refusal
/// names both versions and one command.
#[cfg(feature = "python")]
fn agree_minor(boot: Boot, found: &DiscoveredPython) -> Result<(), String> {
    let Some(linked) = option_env!("BLASTIT_PYTHON_MM") else {
        return Ok(());
    };
    // Only a project venv is this image's business: a host interpreter a wheel
    // will `dlopen` (`.1.4`) is the shim's own minor, checked by the shim.
    if found.venv_root.is_none() {
        return Ok(());
    }
    let found_mm = dotted_py_minor(found.py_minor);
    if found_mm == linked {
        return Ok(());
    }
    Err(format!(
        "blastit {}: the project's virtualenv is CPython {found_mm}, but this blastit embeds \
         CPython {linked}; use a {linked} virtualenv (uv venv --python {linked})",
        boot.verb()
    ))
}

/// A build with no linked interpreter has no minor to agree with.
#[cfg(not(feature = "python"))]
fn agree_minor(_boot: Boot, _found: &DiscoveredPython) -> Result<(), String> {
    Ok(())
}

/// The process environment, in the shape this boot may read it.
///
/// A deployment has no `$VIRTUAL_ENV`, no `PATH` and no development shim
/// directory to pass: the type says so, so the rule is not a guard.
fn discovery_env(boot: Boot) -> DiscoveryEnv {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let blastit_python = std::env::var_os("BLASTIT_PYTHON")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    // `// transitional (MB.42.2.1.4)`: a linked build's own interpreter is the
    // *last* dev candidate (§2.2 puts `<manifest>/.venv` first), so a project
    // venv still wins and an `env -i` shell still finds one.
    let wheel_python = std::env::var_os("BLASTIT_WHEEL_PYTHON")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dev_linked_python(boot));
    match boot {
        Boot::Serve => DiscoveryEnv::production(ProductionEnv {
            blastit_python,
            wheel_python: std::env::var_os("BLASTIT_WHEEL_PYTHON").map(PathBuf::from),
            exe_dir,
        }),
        Boot::Dev => DiscoveryEnv::dev(DevEnv {
            blastit_python,
            virtual_env: std::env::var_os("VIRTUAL_ENV")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            wheel_python,
            path: std::env::var_os("PATH"),
            shim_dir: std::env::var_os("BLASTIT_PY_SHIM_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            cargo_target_ancestor: cargo_target(&exe_dir),
            exe_dir,
        }),
    }
}

/// The interpreter this image was built against, for a `dev` shell that names
/// none.
///
/// `// transitional (MB.42.2.1.4)`: a `python`-feature build still links the
/// interpreter `PYO3_PYTHON` named, and `make` exports its `PYTHONHOME` to every
/// test binary. Until stage 1 loads the shim, that prefix *is* the interpreter
/// this image serves with, so a development shell with no `BLASTIT_PYTHON`,
/// venv or `PATH` entry is not a refusal — and the sentence stays reachable
/// because any candidate the shell *does* name is still checked. Delete with
/// the linked supervisor.
#[cfg(feature = "python")]
fn dev_linked_python(boot: Boot) -> Option<PathBuf> {
    if boot.is_deployment() {
        return None;
    }
    let home = option_env!("BLASTIT_PYTHON_HOME")?;
    // `make` exports the same prefix as `PYTHONHOME`; a child boot that did not
    // get it (a wheel, a stripped environment) simply searches what it can.
    let minor = option_env!("BLASTIT_PYTHON_MM")?;
    ["python3", &format!("python{minor}")]
        .into_iter()
        .map(|name| Path::new(home).join("bin").join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(not(feature = "python"))]
fn dev_linked_python(_boot: Boot) -> Option<PathBuf> {
    None
}

/// The `target/` ancestor of a `cargo` build, for `<target>/py-sapi` (§2.3).
fn cargo_target(exe_dir: &Path) -> Option<PathBuf> {
    exe_dir
        .ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "target"))
        .map(Path::to_path_buf)
}

/// [`boot`]'s pinning and recording half, once the discovery found an object.
fn discover(found: &DiscoveredPython, config: &Config, boot: Boot) -> Result<PythonBoot, String> {
    let lib = SharedObject::open(&found.libpython, None).map_err(|err| err.to_string())?;
    gate_libpython(&lib, found.py_minor, boot.verb(), &found.interpreter)
        .map_err(|err| err.to_string())?;
    let shim = SharedObject::open(&found.shim, None).map_err(|err| err.to_string())?;
    if boot.is_deployment() {
        check_provenance(config, found)?;
    }
    let identity = |object: &SharedObject| {
        object
            .identity()
            .map_err(|err| format!("{}: {err}", object.source().display()))
    };
    let (lib_dev, lib_ino) = identity(&lib)?;
    let (shim_dev, shim_ino) = identity(&shim)?;
    let environment = found
        .venv_root
        .as_deref()
        .unwrap_or(&found.python_home)
        .display()
        .to_string();
    Ok(PythonBoot {
        handoff: PythonHandoff {
            py_minor: found.py_minor,
            python_home: found.python_home.clone(),
            sys_path: found.sys_path.clone(),
            lib: (lib.as_raw_fd(), lib_dev, lib_ino),
            shim: (shim.as_raw_fd(), shim_dev, shim_ino),
        },
        _lib: lib,
        _shim: shim,
        version: found
            .release
            .clone()
            .unwrap_or_else(|| dotted_py_minor(found.py_minor)),
        environment,
    })
}

/// `serve`'s `R7`/`B2-R5` half: the two objects and the trees a view reads.
fn check_provenance(config: &Config, found: &DiscoveredPython) -> Result<(), String> {
    let mut trees = vec![Claim::new("the Python installation", &found.python_home)];
    if let Some(venv) = &found.venv_root {
        trees.push(Claim::new("the project virtualenv", venv));
    }
    trees.push(Claim::new("the manifest directory", &config.manifest_dir));
    let provenance = Provenance {
        init_uid: rustix::process::geteuid().as_raw(),
        objects: vec![
            Claim::new("the shared libpython", &found.libpython),
            Claim::new("the CPython shim", &found.shim),
        ],
        trees,
    };
    provenance.check_objects()?;
    provenance.check_trees()
}

/// The `PYTHONHOME` a standalone image hands stage 1.
///
/// `// transitional (MB.42.2.1.4)`: this is the **linked** interpreter's own
/// prefix — the one `Py_Initialize` in this image reads — recorded by the build
/// script, and only for [`Install::Binary`]. A wheel's stage 1 already runs on
/// its interpreter and gets no `PYTHONHOME` from us. The discovered prefix
/// stays in [`Config::python`] for the shim's `init_paths` in `.1.4`.
///
/// An operator's own `PYTHONHOME` always wins: it names an interpreter this
/// build did not choose.
pub(super) fn python_home(
    install: Install,
    app_env: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    if install != Install::Binary || app_env.contains_key("PYTHONHOME") {
        return None;
    }
    let home = option_env!("BLASTIT_PYTHON_HOME")?;
    Path::new(home).is_dir().then(|| home.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A manifest source whose every request one of its own executors answers.
    fn static_only() -> Manifest {
        Manifest::parse_blastfile("static \"/\" { root \"public\" }\n", 20_000)
            .expect("a static-only file")
            .0
    }

    /// A manifest that runs its own `entrypoint php` front controller.
    fn php_only() -> Manifest {
        Manifest::parse_blastfile(
            "entrypoint php \"public/index.php\"\nview app regex '.*' {}\n",
            20_000,
        )
        .expect("a PHP file")
        .0
    }

    /// A config with no Python app and nothing discovered yet.
    fn config(dir: &Path) -> Config {
        Config {
            manifest_dir: dir.to_path_buf(),
            ..Config::default()
        }
    }

    #[test]
    fn a_php_only_or_static_boot_never_plans_a_search() {
        let dir = std::env::temp_dir();
        let config = config(&dir);
        assert!(
            !plan(&config, &static_only()),
            "a static-only file is answered by its mount"
        );
        assert!(
            !plan(&config, &php_only()),
            "an entrypoint php file runs its own executor"
        );
    }

    #[test]
    fn an_unreadable_blastit_python_is_the_discoverys_refusal() {
        let env = DiscoveryEnv::dev(DevEnv {
            blastit_python: Some(PathBuf::from("/definitely/not/a/python")),
            ..DevEnv::default()
        });
        let refusal = discovery::discover(&env, "dev", &std::env::temp_dir())
            .expect_err("the missing interpreter is a refusal");
        assert!(
            matches!(
                refusal,
                discovery::DiscoveryError::BlastitPythonNotExecutable { .. }
            ),
            "{refusal}"
        );
    }

    #[test]
    fn python_home_comes_from_the_linked_install_and_only_for_a_binary() {
        let empty = BTreeMap::new();
        let binary = python_home(Install::Binary, &empty);
        assert_eq!(
            binary.is_some(),
            option_env!("BLASTIT_PYTHON_HOME").is_some(),
            "a build script that recorded a home is what the boot hands over"
        );
        assert_eq!(
            python_home(Install::Wheel, &empty),
            None,
            "a wheel's stage 1 already runs on its interpreter"
        );
        let operator = BTreeMap::from([("PYTHONHOME".to_string(), "/elsewhere".to_string())]);
        assert_eq!(python_home(Install::Binary, &operator), None);
    }

    #[test]
    fn a_production_boot_has_no_shell_to_read() {
        // `ProductionEnv` has no `virtual_env`/`path`/`shim_dir` field at all:
        // the guarantee is the type. This drives the discovery with it: only the
        // `<manifest_dir>/.venv` it was given can be chosen, never a shell.
        let Some(python) = option_env!("BLASTIT_PYTHON_MM")
            .and_then(|minor| std::env::var_os(format!("PYO3_PYTHON_{minor}")))
            .or_else(|| std::env::var_os("PYO3_PYTHON"))
        else {
            return;
        };
        let python = std::fs::canonicalize(python).expect("the build interpreter");
        let dir = std::env::temp_dir().join(format!("blastit-prod-python-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let venv = dir.join(".venv");
        std::fs::create_dir_all(venv.join("bin")).expect("the venv");
        std::os::unix::fs::symlink(&python, venv.join("bin/python3")).expect("the interpreter");
        let minor = option_env!("BLASTIT_PYTHON_MM").unwrap_or("3.14");
        std::fs::write(
            venv.join("pyvenv.cfg"),
            format!(
                "home = {}\nversion = {minor}.0\n",
                python.parent().expect("bin").display()
            ),
        )
        .expect("pyvenv.cfg");
        let env = DiscoveryEnv::production(ProductionEnv {
            exe_dir: dir.join("bin"),
            ..ProductionEnv::default()
        });
        // A deployment looks for the shim beside its own executable only.
        std::fs::create_dir_all(dir.join("bin")).expect("the executable directory");
        std::fs::write(dir.join("bin").join(discovery::shim_file(14)), b"a shim")
            .expect("the shim");
        let found = discovery::discover(&env, "serve", &dir).expect("the local .venv");
        assert_eq!(found.interpreter, venv.join("bin/python3"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_venv_for_another_minor_is_refused_naming_both() {
        let Some(linked) = option_env!("BLASTIT_PYTHON_MM") else {
            return;
        };
        let found = DiscoveredPython {
            interpreter: PathBuf::from("/tmp/venv/bin/python3"),
            py_minor: if linked == "3.13" { 314 } else { 313 },
            python_home: PathBuf::from("/opt/python"),
            venv_root: Some(PathBuf::from("/tmp/venv")),
            release: None,
            sys_path: Vec::new(),
            libpython: PathBuf::from("/opt/python/lib/libpython3.14.so.1.0"),
            shim: PathBuf::from("/opt/blastit/libblastit_py314-x86_64-gnu.so"),
        };
        let refusal = agree_minor(Boot::Dev, &found).expect_err("a different minor");
        assert!(
            refusal.contains("the project's virtualenv is CPython"),
            "{refusal}"
        );
        assert!(
            refusal.contains(linked),
            "the linked minor is named: {refusal}"
        );
        assert!(refusal.contains("uv venv --python"), "{refusal}");
        // No venv: a host interpreter the shim will load is not this check's.
        let host = DiscoveredPython {
            venv_root: None,
            ..found
        };
        assert!(agree_minor(Boot::Dev, &host).is_ok());
    }
}
