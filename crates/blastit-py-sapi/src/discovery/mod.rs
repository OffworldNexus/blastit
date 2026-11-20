//! Stage-0 Python discovery: which interpreter, which `libpython`, which shim
//! (design §2 of `docs/design/universal-distribution.md`).
//!
//! Filesystem-only: stage 0 never executes Python, so nothing here can import
//! project code. [`DiscoveryEnv`] carries every value the boot read, and its two
//! constructors are the type boundary: a [`ProductionEnv`] has no `virtual_env`
//! and no `path` field to pass at all, so "a deployment never reads the
//! operator's shell" is not re-checked with a guard.
//!
//! [`DevEnv`] walks `BLASTIT_PYTHON` -> `$VIRTUAL_ENV` -> `<manifest>/.venv` ->
//! `BLASTIT_WHEEL_PYTHON` -> `PATH`; a [`ProductionEnv`] walks `BLASTIT_PYTHON`
//! (absolute) -> `<manifest>/.venv` -> `BLASTIT_WHEEL_PYTHON` (absolute) ->
//! `/usr/local/bin`, `/usr/bin`, `/bin`.
//!
//! The refusal type and its sentences live in this module's private half, so
//! every refusal a boot can print is written once.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::abi::encode_py_minor;

mod error;
mod minor;

pub use self::error::DiscoveryError;

use self::minor::{minor_of, python_home, read_pyvenv};

/// CPython minors a WSGI view may be served by, newest first.
pub const SUPPORTED_MINORS: [u32; 2] = [14, 13];

/// The release the refusals tell a developer to install.
const DEFAULT_MINOR: &str = "3.14";

/// What a `blastit dev`/`blastit check` boot read before it chose to discover.
///
/// This is the only carrier of `$VIRTUAL_ENV`, `PATH` and
/// `$BLASTIT_PY_SHIM_DIR`: a deployment has no way to hand them in.
#[derive(Debug, Clone, Default)]
pub struct DevEnv {
    /// `BLASTIT_PYTHON`.
    pub blastit_python: Option<PathBuf>,
    /// `$VIRTUAL_ENV`.
    pub virtual_env: Option<PathBuf>,
    /// `BLASTIT_WHEEL_PYTHON` (`sys.executable`).
    pub wheel_python: Option<PathBuf>,
    /// `PATH`, as `std::env::split_paths` reads it.
    pub path: Option<OsString>,
    /// `$BLASTIT_PY_SHIM_DIR`, the development-only first stop.
    pub shim_dir: Option<PathBuf>,
    /// The running executable's directory.
    pub exe_dir: PathBuf,
    /// Its `target/` ancestor, for `<target>/py-sapi` in a `cargo` tree.
    pub cargo_target_ancestor: Option<PathBuf>,
}

/// What a `serve`/stage-1 boot read: absolute overrides and its own directory.
#[derive(Debug, Clone, Default)]
pub struct ProductionEnv {
    /// `BLASTIT_PYTHON`; a relative value is refused, not resolved.
    pub blastit_python: Option<PathBuf>,
    /// `BLASTIT_WHEEL_PYTHON` (`sys.executable`); ignored when relative.
    pub wheel_python: Option<PathBuf>,
    /// The running executable's directory, the only place a shim may sit.
    pub exe_dir: PathBuf,
}

/// What the boot read, in the shape that boot is allowed to read it.
#[derive(Debug, Clone)]
pub enum DiscoveryEnv {
    /// `blastit dev` and `blastit check`.
    Dev(DevEnv),
    /// A deployment (`serve`, or stage 1 re-reading its own configuration).
    Production(ProductionEnv),
}

impl DiscoveryEnv {
    /// A development boot's environment.
    #[must_use]
    pub fn dev(env: DevEnv) -> Self {
        Self::Dev(env)
    }

    /// A deployment's environment: no shell values exist to pass.
    #[must_use]
    pub fn production(env: ProductionEnv) -> Self {
        Self::Production(env)
    }

    /// Whether this boot is a deployment.
    #[must_use]
    pub fn is_deployment(&self) -> bool {
        matches!(self, Self::Production(_))
    }
}

/// Everything a stage-0 boot resolves for the Python half of a view (§2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPython {
    /// The interpreter file (never executed in stage 0).
    pub interpreter: PathBuf,
    /// Its CPython minor, as the shim reports it (`313`/`314`).
    pub py_minor: u32,
    /// The installation prefix the embedded interpreter gets as `PYTHONHOME`.
    pub python_home: PathBuf,
    /// The venv root the interpreter belongs to, when it is inside one.
    pub venv_root: Option<PathBuf>,
    /// The full release `pyvenv.cfg` names (`3.14.10`), when it names one.
    pub release: Option<String>,
    /// The venv's `site-packages`, as `os.fsencode` bytes.
    pub sys_path: Vec<Vec<u8>>,
    /// `libpython3.<m>.so[.1.0]`, opened and pinned by the caller.
    pub libpython: PathBuf,
    /// The per-minor `libblastit_py3<m>-<arch>-<libc>.so` shim.
    pub shim: PathBuf,
}

/// The activation gate of §2.1, as a pure predicate.
///
/// `runs_own_executors` is the manifest's answer (a PHP-only or static-only
/// file runs its own executors) and `has_app` is `config.app.is_some()` *after*
/// the static-app refusal. A boot that runs its own executors and declares no
/// Python app therefore structurally never reaches [`discover`]: no search, no
/// `open`, no `dlopen`.
#[must_use]
pub fn activates(runs_own_executors: bool, has_app: bool) -> bool {
    !runs_own_executors || has_app
}

/// Resolves the interpreter, prefix, `site-packages`, `libpython` and shim.
///
/// # Errors
///
/// A [`DiscoveryError`] carrying one of the §2.3 sentences.
pub fn discover(
    env: &DiscoveryEnv,
    command: &str,
    manifest_dir: &Path,
) -> Result<DiscoveredPython, DiscoveryError> {
    let interpreter = interpreter(env, command, manifest_dir)?;
    let venv = venv_root(&interpreter);
    let config = venv.as_deref().and_then(read_pyvenv);
    let python_home = python_home(&interpreter, config.as_ref(), command)?;
    let (minor, free_threaded) = minor_of(
        &interpreter,
        venv.as_deref(),
        &python_home,
        config.as_ref().and_then(|config| config.minor),
        command,
    )?;
    if free_threaded {
        return Err(DiscoveryError::FreeThreaded {
            command: command.to_owned(),
            found: format!("3.{minor}t"),
            interpreter,
        });
    }
    if !SUPPORTED_MINORS.contains(&minor) {
        return Err(DiscoveryError::UnsupportedMinor {
            command: command.to_owned(),
            found: format!("3.{minor}"),
            interpreter,
        });
    }
    let sys_path = sys_path(venv.as_deref(), minor);
    let libpython = libpython(&python_home, minor, command)?;
    Ok(DiscoveredPython {
        interpreter,
        py_minor: encode_py_minor(3, minor),
        python_home,
        venv_root: venv,
        release: config.and_then(|config| config.release),
        sys_path,
        libpython,
        shim: shim(env, minor, command)?,
    })
}

/// The per-minor shim file name: `libblastit_py3<m>-<arch>-<libc>.so`.
#[must_use]
pub fn shim_file(minor: u32) -> String {
    format!(
        "libblastit_py3{minor}-{}-{}.so",
        std::env::consts::ARCH,
        libc_suffix()
    )
}

/// §2.2's interpreter order, filesystem-only.
fn interpreter(
    env: &DiscoveryEnv,
    command: &str,
    manifest_dir: &Path,
) -> Result<PathBuf, DiscoveryError> {
    match env {
        DiscoveryEnv::Dev(dev) => dev_interpreter(dev, command, manifest_dir),
        DiscoveryEnv::Production(production) => {
            production_interpreter(production, command, manifest_dir)
        }
    }
}

/// `blastit dev`/`check`: the shell is readable, so it is consulted.
fn dev_interpreter(
    dev: &DevEnv,
    command: &str,
    manifest_dir: &Path,
) -> Result<PathBuf, DiscoveryError> {
    let venv = dev
        .virtual_env
        .as_deref()
        .filter(|root| is_venv(root))
        .map_or_else(|| manifest_dir.join(".venv"), Path::to_path_buf);
    if let Some(path) = dev.blastit_python.as_deref() {
        if !is_executable(path) {
            return Err(DiscoveryError::BlastitPythonNotExecutable {
                command: command.to_owned(),
                path: path.to_path_buf(),
                venv,
            });
        }
        return Ok(path.to_path_buf());
    }
    if let Some(executable) = dev.virtual_env.as_deref().and_then(venv_bin) {
        return Ok(executable);
    }
    if let Some(executable) = venv_bin(&manifest_dir.join(".venv")) {
        return Ok(executable);
    }
    if let Some(path) = dev.wheel_python.as_deref()
        && is_executable(path)
    {
        return Ok(path.to_path_buf());
    }
    let directories = dev_search_directories(dev);
    search(&directories).ok_or_else(|| DiscoveryError::NoInterpreter {
        command: command.to_owned(),
        searched: directories,
    })
}

/// A deployment: absolute overrides only, and never an operator's shell.
fn production_interpreter(
    production: &ProductionEnv,
    command: &str,
    manifest_dir: &Path,
) -> Result<PathBuf, DiscoveryError> {
    let venv = manifest_dir.join(".venv");
    if let Some(path) = production.blastit_python.as_deref() {
        if !path.is_absolute() {
            return Err(DiscoveryError::BlastitPythonNotAbsolute {
                command: command.to_owned(),
                path: path.to_path_buf(),
                venv,
            });
        }
        if !is_executable(path) {
            return Err(DiscoveryError::BlastitPythonNotExecutable {
                command: command.to_owned(),
                path: path.to_path_buf(),
                venv,
            });
        }
        return Ok(path.to_path_buf());
    }
    if let Some(executable) = venv_bin(&venv) {
        return Ok(executable);
    }
    if let Some(path) = production.wheel_python.as_deref()
        && path.is_absolute()
        && is_executable(path)
    {
        return Ok(path.to_path_buf());
    }
    let directories = posix_search_path();
    search(&directories).ok_or_else(|| DiscoveryError::NoInterpreter {
        command: command.to_owned(),
        searched: directories,
    })
}

/// `_CS_PATH`'s default directories: what `libc::execvp` searches when `PATH`
/// is unset. An empty environment must not collapse the search to zero
/// directories (`env -i blastit dev`, `MB.50`).
const POSIX_SEARCH_PATH: [&str; 3] = ["/usr/local/bin", "/usr/bin", "/bin"];

/// Those defaults, as paths.
fn posix_search_path() -> Vec<PathBuf> {
    POSIX_SEARCH_PATH.into_iter().map(PathBuf::from).collect()
}

/// The `PATH` directories a development boot searches: `PATH` when it is set,
/// `_CS_PATH`'s default when it is not.
fn dev_search_directories(dev: &DevEnv) -> Vec<PathBuf> {
    dev.path.as_deref().map_or_else(posix_search_path, |value| {
        std::env::split_paths(value).collect()
    })
}

/// The first supported interpreter of `directories`, in the directories' order.
fn search(directories: &[PathBuf]) -> Option<PathBuf> {
    // Per directory, the supported minors come first: `/usr/local/bin/python3.13`
    // exists on the runner while `/usr/bin/python3` is 3.12, and a 3.12 pick would
    // only be refused one step later. The generic names stay last, so an
    // old-only host still gets `UnsupportedMinor`, not `NoInterpreter`.
    let names: Vec<String> = SUPPORTED_MINORS
        .into_iter()
        .map(|minor| format!("python3.{minor}"))
        .chain(["python3".to_owned(), "python".to_owned()])
        .collect();
    directories.iter().find_map(|directory| {
        names
            .iter()
            .map(|name| directory.join(name))
            .find(|candidate| is_executable(candidate))
    })
}

/// `<venv>/bin/{python3,python}` when the directory is a venv.
fn venv_bin(root: &Path) -> Option<PathBuf> {
    if !is_venv(root) {
        return None;
    }
    ["python3", "python"]
        .into_iter()
        .map(|name| root.join("bin").join(name))
        .find(|candidate| is_executable(candidate))
}

/// A real file with at least one execute bit.
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// A directory `python -m venv`, `uv venv` or `virtualenv` wrote.
fn is_venv(path: &Path) -> bool {
    path.is_dir() && path.join("pyvenv.cfg").is_file()
}

/// The venv root `interpreter` belongs to, when it is inside one.
fn venv_root(interpreter: &Path) -> Option<PathBuf> {
    let root = interpreter.parent()?.parent()?;
    is_venv(root).then(|| root.to_path_buf())
}

/// `site-packages` for the chosen minor, `lib` before `lib64`, prepended.
fn sys_path(venv: Option<&Path>, minor: u32) -> Vec<Vec<u8>> {
    let Some(root) = venv else {
        return Vec::new();
    };
    ["lib", "lib64"]
        .into_iter()
        .map(|lib| {
            root.join(lib)
                .join(format!("python3.{minor}"))
                .join("site-packages")
        })
        .filter(|directory| directory.is_dir())
        .map(|directory| directory.as_os_str().as_bytes().to_vec())
        .collect()
}

/// §2.3's `libpython` search: `.so.1.0` then `.so`, across the prefix first.
fn libpython(home: &Path, minor: u32, command: &str) -> Result<PathBuf, DiscoveryError> {
    let triplet = lib_triplet();
    let directories = [
        home.join("lib").join(&triplet),
        home.join("lib64"),
        home.join("lib"),
        PathBuf::from("/usr/lib").join(&triplet),
        PathBuf::from("/usr/lib64"),
        PathBuf::from("/usr/lib"),
        PathBuf::from("/usr/local/lib"),
    ];
    for name in [
        format!("libpython3.{minor}.so.1.0"),
        format!("libpython3.{minor}.so"),
    ] {
        for directory in &directories {
            let candidate = directory.join(&name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(DiscoveryError::NoLibpython {
        command: command.to_owned(),
        python_home: home.to_path_buf(),
        minor: format!("3.{minor}"),
    })
}

/// §2.3's shim search: a deployment looks beside the executable only.
fn shim(env: &DiscoveryEnv, minor: u32, command: &str) -> Result<PathBuf, DiscoveryError> {
    let name = shim_file(minor);
    let (deployment, directories) = match env {
        DiscoveryEnv::Dev(dev) => {
            let mut directories: Vec<PathBuf> = Vec::new();
            directories.extend(dev.shim_dir.clone());
            directories.push(dev.exe_dir.clone());
            directories.extend(
                dev.cargo_target_ancestor
                    .as_ref()
                    .map(|ancestor| ancestor.join("py-sapi")),
            );
            (false, directories)
        }
        DiscoveryEnv::Production(production) => (true, vec![production.exe_dir.clone()]),
    };
    for directory in &directories {
        let candidate = directory.join(&name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(DiscoveryError::NoShim {
        command: command.to_owned(),
        deployment,
        minor: format!("3.{minor}"),
        name,
        searched: directories,
    })
}

/// `<arch>-linux-<libc>`, the directory CPython and the distros install into.
fn lib_triplet() -> String {
    format!("{}-linux-{}", std::env::consts::ARCH, libc_suffix())
}

/// This build's libc, as the shim file name spells it.
fn libc_suffix() -> &'static str {
    if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    }
}
