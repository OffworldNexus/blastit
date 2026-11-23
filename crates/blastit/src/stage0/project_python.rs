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
use blastit_py_sapi::discovery::{
    self, DevEnv, DiscoveredPython, DiscoveryEnv, ProductionEnv, SUPPORTED_MINORS,
};
use blastit_py_sapi::loader::gate_libpython;
use blastit_py_sapi::{default_minor, supported_minors_sentence};
use blastit_sys::pin::SharedObject;

use crate::config::{Config, PythonHandoff};
use crate::host::{Claim, Provenance};

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
    // The shim this boot found was resolved for `found.py_minor` (design §2.3),
    // and stage 1 re-checks the loaded shim against it (`MinorMismatch`), so a
    // boot whose shim is another CPython is still the named both-versions
    // refusal — never a fatal `init_fs_encoding` inside the interpreter.
    agree_minor(boot, &found)?;
    discover(&found, config, boot).map(Some)
}

/// Refuses a boot whose discovered CPython is one this build ships no shim for.
///
/// The both-versions sentence a project venv on another minor gets (design §2.3):
/// the interpreter may be perfectly usable for `blastit check`, but a WSGI view is
/// served by the per-minor shim this build carries, and installing a second
/// CPython is a different action from reinstalling one. The runtime half — the
/// loaded shim's own `py_minor` against the discovered one — is the loader's
/// (`LoadError::MinorMismatch`), which names both in dotted form too.
fn agree_minor(boot: Boot, found: &DiscoveredPython) -> Result<(), String> {
    // `SUPPORTED_MINORS` is the minor alone (`14`, `13`); the discovery record is
    // the wire encoding (`314`, `313`).
    if SUPPORTED_MINORS.contains(&(found.py_minor % 100)) {
        return Ok(());
    }
    Err(format!(
        "blastit {}: CPython {} at {} is not supported for WSGI views ({} are supported); run \
         uv python install {} && uv venv --python {}",
        boot.verb(),
        dotted_py_minor(found.py_minor),
        found.interpreter.display(),
        supported_minors_sentence(),
        default_minor(),
        default_minor()
    ))
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
    // A linked build's own interpreter is the
    // *last* dev candidate (§2.2 puts `<manifest>/.venv` first), so a project
    // venv still wins and an `env -i` shell still finds one.
    let wheel_python = std::env::var_os("BLASTIT_WHEEL_PYTHON")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| prefix_python(boot));
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

/// `<PYTHONHOME>/bin/python3`, when a dev shell (or its harness) activated a prefix.
///
/// `PYTHONHOME` names an interpreter prefix the caller already chose, so probing it
/// is not a build-host path baked into the artifact:
/// it is a *dev* candidate only, and a deployment still reads `BLASTIT_PYTHON`, the
/// manifest venv, the wheel's interpreter or the system directories (design §2.2).
fn prefix_python(boot: Boot) -> Option<PathBuf> {
    if boot.is_deployment() {
        return None;
    }
    let home = std::env::var_os("PYTHONHOME").filter(|value| !value.is_empty())?;
    ["python3", "python"]
        .into_iter()
        .map(|name| Path::new(&home).join("bin").join(name))
        .find(|candidate| candidate.is_file())
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
    // Every refusal this half prints carries the boot's verb exactly once, the
    // way the discovery's own sentences do (`blastit {cmd}: …`).
    let verb = boot.verb();
    let head = |err: String| format!("blastit {verb}: {err}");
    let lib = SharedObject::open(&found.libpython, None).map_err(|err| head(err.to_string()))?;
    gate_libpython(&lib, found.py_minor, verb, &found.interpreter)
        .map_err(|err| head(err.to_string()))?;
    let shim = SharedObject::open(&found.shim, None).map_err(|err| head(err.to_string()))?;
    if boot.is_deployment() {
        check_provenance(config, found).map_err(head)?;
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
