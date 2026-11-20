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

#[cfg(test)]
mod tests {
    use super::minor::{parse_cfg_minor, parse_minor};
    use super::*;
    use crate::abi::{PY_MINOR_313, PY_MINOR_314};

    /// A directory under the workspace's `target/tmp`, removed when dropped.
    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/pysapi");
            let dir = root.join(format!("{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("the fixture directory");
            Self { dir }
        }

        /// An executable file, the way a `python3` entry point is one.
        fn executable(&self, path: &Path) -> PathBuf {
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
            std::fs::write(path, b"#!/bin/sh\nexit 1\n").expect("the interpreter file");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .expect("the execute bit");
            path.to_path_buf()
        }

        /// An installation prefix holding a shared `libpython3.<m>.so.1.0`.
        fn prefix(&self, name: &str, minor: u32) -> PathBuf {
            let prefix = self.dir.join(name);
            std::fs::create_dir_all(prefix.join("lib")).expect("the prefix");
            std::fs::write(
                prefix
                    .join("lib")
                    .join(format!("libpython3.{minor}.so.1.0")),
                b"a shared object",
            )
            .expect("libpython");
            prefix
        }

        /// A venv whose `pyvenv.cfg` records `home` and `version`, whose `bin/`
        /// has a `python3` entry point, and which holds a `3.<minor>` tree.
        fn venv(&self, name: &str, home: &Path, minor: u32) -> PathBuf {
            let root = self.dir.join(name);
            std::fs::create_dir_all(root.join("bin")).expect("the venv");
            std::fs::write(
                root.join("pyvenv.cfg"),
                format!("home = {}\nversion = 3.{minor}.1\n", home.display()),
            )
            .expect("pyvenv.cfg");
            self.executable(&root.join("bin/python3"));
            std::fs::create_dir_all(
                root.join("lib")
                    .join(format!("python3.{minor}"))
                    .join("site-packages"),
            )
            .expect("site-packages");
            root
        }

        /// An extra `lib/python3.<minor>/site-packages` tree in `venv`.
        fn tree(&self, venv: &Path, minor: u32) {
            std::fs::create_dir_all(
                venv.join("lib")
                    .join(format!("python3.{minor}"))
                    .join("site-packages"),
            )
            .expect("the extra tree");
        }

        /// A shim in `directory`.
        fn shim(&self, directory: &Path, minor: u32) -> PathBuf {
            std::fs::create_dir_all(directory).expect("the shim directory");
            let path = directory.join(shim_file(minor));
            std::fs::write(&path, b"a shim").expect("the shim");
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// The dev env every test starts from: nothing set, the fixture's `bin/`.
    fn env(fixture: &Fixture) -> DiscoveryEnv {
        DiscoveryEnv::dev(DevEnv {
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        })
    }

    #[test]
    fn a_venv_with_a_shared_libpython_and_a_shim_is_discovered() {
        let fixture = Fixture::new("venv-ok");
        let prefix = fixture.prefix("prefix", 14);
        let venv = fixture.venv("venv", &prefix.join("bin"), 14);
        let shim = fixture.shim(&fixture.dir.join("bin"), 14);
        let manifest = fixture.dir.join("project");
        std::fs::create_dir_all(&manifest).expect("the manifest directory");
        let DiscoveryEnv::Dev(dev) = env(&fixture) else {
            panic!("the dev env")
        };
        let env = DiscoveryEnv::dev(DevEnv {
            virtual_env: Some(venv.clone()),
            ..dev
        });

        let found = discover(&env, "dev", &manifest).expect("a venv for a supported minor");
        assert_eq!(found.interpreter, venv.join("bin/python3"));
        assert_eq!(found.py_minor, PY_MINOR_314);
        assert_eq!(found.python_home, prefix);
        assert_eq!(found.libpython, prefix.join("lib/libpython3.14.so.1.0"));
        assert_eq!(found.shim, shim);
        assert_eq!(
            found.sys_path,
            vec![
                venv.join("lib/python3.14/site-packages")
                    .as_os_str()
                    .as_bytes()
                    .to_vec()
            ],
            "the venv's site-packages crosses as os.fsencode bytes"
        );
    }

    #[test]
    fn pyvenv_cfg_decides_the_minor_after_an_in_place_upgrade() {
        // `uv venv` upgraded in place: the 3.14 tree was added beside 3.13, and
        // a directory scan would pair the 3.13 interpreter with libpython3.14.
        let fixture = Fixture::new("upgrade");
        let prefix = fixture.prefix("prefix", 13);
        let venv = fixture.venv("venv", &prefix.join("bin"), 13);
        fixture.tree(&venv, 14);
        fixture.shim(&fixture.dir.join("bin"), 13);
        let DiscoveryEnv::Dev(dev) = env(&fixture) else {
            panic!("the dev env")
        };
        let env = DiscoveryEnv::dev(DevEnv {
            virtual_env: Some(venv),
            ..dev
        });

        let found = discover(&env, "dev", &fixture.dir).expect("the venv's own version wins");
        assert_eq!(found.py_minor, PY_MINOR_313, "pyvenv.cfg says 3.13");
        assert_eq!(found.libpython, prefix.join("lib/libpython3.13.so.1.0"));
    }

    #[test]
    fn an_invalid_blastit_python_is_refused_verbatim() {
        let fixture = Fixture::new("invalid-env");
        let missing = fixture.dir.join("not-a-python");
        let manifest = fixture.dir.join("project");
        let env = DiscoveryEnv::dev(DevEnv {
            blastit_python: Some(missing.clone()),
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        });

        let error = discover(&env, "dev", &manifest).expect_err("not a file");
        assert_eq!(
            error.to_string(),
            format!(
                "blastit dev: BLASTIT_PYTHON ({}) is not an executable file; unset \
                 BLASTIT_PYTHON, or point it at {}/bin/python3",
                missing.display(),
                manifest.join(".venv").display()
            ),
            "the §2.3 sentence, byte for byte"
        );
    }

    #[test]
    fn a_relative_blastit_python_is_refused_in_a_deployment_verbatim() {
        let fixture = Fixture::new("relative");
        let manifest = fixture.dir.join("project");
        let executable = fixture.executable(&fixture.dir.join("python3.14"));
        let relative = executable
            .strip_prefix("/")
            .expect("absolute")
            .to_path_buf();
        let env = DiscoveryEnv::production(ProductionEnv {
            blastit_python: Some(relative.clone()),
            exe_dir: fixture.dir.join("bin"),
            ..ProductionEnv::default()
        });

        let error = discover(&env, "serve", &manifest).expect_err("relative");
        assert_eq!(
            error.to_string(),
            format!(
                "blastit serve: BLASTIT_PYTHON ({}) is not an absolute path; a deployment reads \
                 only absolute paths — use an absolute path such as {}/bin/python3",
                relative.display(),
                manifest.join(".venv").display()
            ),
            "the deployment sentence, byte for byte"
        );
    }

    #[test]
    fn a_free_threaded_interpreter_is_refused_verbatim() {
        let fixture = Fixture::new("free-threaded");
        let interpreter = fixture.executable(&fixture.dir.join("python3.13t"));
        let env = DiscoveryEnv::dev(DevEnv {
            blastit_python: Some(interpreter.clone()),
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        });

        let error = discover(&env, "dev", &fixture.dir).expect_err("3.13t");
        assert_eq!(
            error.to_string(),
            format!(
                "blastit dev: CPython 3.13t at {} is free-threaded and cannot serve WSGI views; \
                 reinstall it without --free-threaded (uv python install cpython-3.14)",
                interpreter.display()
            )
        );
    }

    #[test]
    fn no_interpreter_names_where_it_looked_and_one_command() {
        let error = DiscoveryError::NoInterpreter {
            command: "check".to_owned(),
            searched: vec![
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ],
        };
        assert_eq!(
            error.to_string(),
            "blastit check: no CPython 3.13 or 3.14 interpreter was found (searched \
             /usr/local/bin, /usr/bin, /bin); run uv python install 3.14"
        );
        assert!(
            !error.to_string().contains("distribution's"),
            "a distro package that does not exist everywhere is never named"
        );
    }

    #[test]
    fn an_unsupported_minor_is_refused_verbatim() {
        let fixture = Fixture::new("3.12");
        let interpreter = fixture.executable(&fixture.dir.join("python3.12"));
        let env = DiscoveryEnv::dev(DevEnv {
            blastit_python: Some(interpreter.clone()),
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        });

        let error = discover(&env, "dev", &fixture.dir).expect_err("3.12");
        assert_eq!(
            error.to_string(),
            format!(
                "blastit dev: CPython 3.12 at {} is not supported for WSGI views (CPython 3.13 \
                 and 3.14 are supported; Python 3.12+ runs blastit check, fmt, and fold): run uv \
                 python install 3.14 && uv venv --python 3.14",
                interpreter.display()
            )
        );
    }

    #[test]
    fn the_missing_libpython_sentence_names_the_minor_and_every_remedy() {
        let message = DiscoveryError::NoLibpython {
            command: "dev".to_owned(),
            python_home: PathBuf::from("/opt/python"),
            minor: "3.14".to_owned(),
        }
        .to_string();
        for expected in [
            "/opt/python",
            "no shared libpython for CPython 3.14",
            "apt install libpython3.14",
            "apk add python3",
            "dnf install python3-libs",
            "uv python install 3.14",
        ] {
            assert!(message.contains(expected), "{expected} missing: {message}");
        }
    }

    #[test]
    fn a_php_only_or_static_only_boot_never_searches_for_an_interpreter() {
        assert!(
            !activates(true, false),
            "PHP-only/static-only: never search"
        );
        assert!(
            activates(false, false),
            "a Python file's own routes: search"
        );
        assert!(activates(true, true), "an app beside them: search");
        assert!(activates(false, true), "an app: search");
    }

    #[test]
    fn an_unset_path_searches_the_posix_defaults() {
        // `env_clear()` leaves `PATH` unset (`critical_dev_reload`), and an empty
        // environment used to mean zero directories searched. `execvp` searches
        // `_CS_PATH`'s defaults instead, and so does the dev boot.
        let fixture = Fixture::new("unset-path");
        let dev = DevEnv {
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        };
        assert_eq!(
            dev_search_directories(&dev),
            vec![
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ]
        );
        match dev_interpreter(&dev, "dev", &fixture.dir.join("project")) {
            Ok(found) => assert!(
                POSIX_SEARCH_PATH
                    .iter()
                    .any(|directory| { found.parent() == Some(Path::new(directory)) }),
                "{} is not under the POSIX defaults",
                found.display()
            ),
            Err(DiscoveryError::NoInterpreter { searched, .. }) => assert_eq!(
                searched,
                posix_search_path(),
                "the refusal names the defaults it searched"
            ),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
        assert_eq!(
            dev_search_directories(&DevEnv {
                path: Some(OsString::from("/opt/one:/opt/two")),
                ..DevEnv::default()
            }),
            vec![PathBuf::from("/opt/one"), PathBuf::from("/opt/two")],
            "a set PATH is still read verbatim"
        );
    }

    #[test]
    fn a_supported_interpreter_beside_the_generic_one_wins() {
        let fixture = Fixture::new("supported-beside-generic");
        let bin = fixture.dir.join("bin");
        fixture.executable(&bin.join("python3"));
        let supported = fixture.executable(&bin.join("python3.13"));
        let dev = DevEnv {
            path: Some(bin.into_os_string()),
            exe_dir: fixture.dir.join("exe"),
            ..DevEnv::default()
        };
        assert_eq!(
            dev_interpreter(&dev, "dev", &fixture.dir.join("project")).expect("a supported minor"),
            supported
        );
    }

    #[test]
    fn an_old_python3_alone_is_still_refused_by_its_own_minor() {
        let fixture = Fixture::new("old-python3");
        let bin = fixture.dir.join("bin");
        fixture.executable(&bin.join("python3"));
        // The runner's `/usr/bin/python3` is 3.12: the generic name is still
        // offered, so the refusal stays the honest `UnsupportedMinor`.
        std::fs::create_dir_all(fixture.dir.join("lib/python3.12")).expect("the 3.12 tree");
        let dev = DevEnv {
            path: Some(bin.into_os_string()),
            exe_dir: fixture.dir.join("exe"),
            ..DevEnv::default()
        };
        let error = discover(&DiscoveryEnv::dev(dev), "dev", &fixture.dir.join("project"))
            .expect_err("3.12");
        assert!(
            matches!(error, DiscoveryError::UnsupportedMinor { ref found, .. } if found == "3.12"),
            "{error}"
        );
    }

    #[test]
    fn a_set_path_is_searched_in_its_own_order() {
        let fixture = Fixture::new("path-order");
        let first = fixture.dir.join("first");
        let second = fixture.dir.join("second");
        let older = fixture.executable(&first.join("python3.13"));
        let newer = fixture.executable(&second.join("python3.14"));
        let dev = DevEnv {
            path: Some(OsString::from(format!(
                "{}:{}",
                first.display(),
                second.display()
            ))),
            exe_dir: fixture.dir.join("exe"),
            ..DevEnv::default()
        };
        assert_ne!(older, newer);
        assert_eq!(
            dev_interpreter(&dev, "dev", &fixture.dir.join("project")).expect("3.13"),
            older,
            "the first directory wins, even over a newer minor later"
        );
    }

    #[test]
    fn the_shim_search_is_scope_dependent() {
        let fixture = Fixture::new("shim-scope");
        let prefix = fixture.prefix("prefix", 14);
        let interpreter = fixture.executable(&prefix.join("bin/python3.14"));
        fixture.shim(&fixture.dir.join("dev-shims"), 14);
        let dev_env = DiscoveryEnv::dev(DevEnv {
            blastit_python: Some(interpreter.clone()),
            shim_dir: Some(fixture.dir.join("dev-shims")),
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        });
        let production_env = DiscoveryEnv::production(ProductionEnv {
            blastit_python: Some(interpreter),
            exe_dir: fixture.dir.join("bin"),
            ..ProductionEnv::default()
        });

        let dev = discover(&dev_env, "dev", &fixture.dir).expect("dev finds the shim");
        assert_eq!(dev.shim, fixture.dir.join("dev-shims").join(shim_file(14)));

        let error = discover(&production_env, "serve", &fixture.dir)
            .expect_err("a deployment does not search the development directory");
        assert_eq!(
            error.to_string(),
            format!(
                "blastit serve: CPython 3.14 is ready, but no {} shim sits beside this blastit; install the \
                 blastit build that ships the CPython 3.14 shim",
                shim_file(14)
            )
        );
    }

    #[test]
    fn the_development_shim_refusal_names_the_variable() {
        let fixture = Fixture::new("shim-dev-refusal");
        let prefix = fixture.prefix("prefix", 14);
        let interpreter = fixture.executable(&prefix.join("bin/python3.14"));
        let env = DiscoveryEnv::dev(DevEnv {
            blastit_python: Some(interpreter),
            shim_dir: Some(fixture.dir.join("dev-shims")),
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        });

        let error = discover(&env, "check", &fixture.dir).expect_err("no shim");
        let message = error.to_string();
        assert_eq!(
            message,
            format!(
                "blastit check: CPython 3.14 is ready, but no {} shim was found (looked in {}, {}); \
                 build one with make py-sapi, or point BLASTIT_PY_SHIM_DIR at a directory that \
                 holds it",
                shim_file(14),
                fixture.dir.join("dev-shims").display(),
                fixture.dir.join("bin").display()
            )
        );
    }

    #[test]
    fn a_production_boot_ignores_the_activated_venv_and_serves_the_local_one() {
        let fixture = Fixture::new("production");
        let manifest = fixture.dir.join("project");
        std::fs::create_dir_all(&manifest).expect("the manifest directory");
        let prefix = fixture.prefix("prefix", 14);
        let activated = fixture.venv("activated", &prefix.join("bin"), 14);
        let local = fixture.venv("project/.venv", &prefix.join("bin"), 14);
        fixture.shim(&fixture.dir.join("bin"), 14);

        let dev_env = DiscoveryEnv::dev(DevEnv {
            virtual_env: Some(activated.clone()),
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        });
        let production_env = DiscoveryEnv::production(ProductionEnv {
            exe_dir: fixture.dir.join("bin"),
            ..ProductionEnv::default()
        });
        assert_eq!(
            discover(&dev_env, "dev", &manifest)
                .expect("dev serves the activated venv")
                .interpreter,
            activated.join("bin/python3")
        );
        assert_eq!(
            discover(&production_env, "serve", &manifest)
                .expect("serve serves the .venv it was given")
                .interpreter,
            local.join("bin/python3"),
            "a deployment has no virtual_env field to honour"
        );
        assert!(
            production_env.is_deployment(),
            "the constructor decides the boot"
        );
        assert!(!dev_env.is_deployment());
    }

    #[test]
    fn a_venv_for_an_unsupported_minor_is_refused_by_its_own_version() {
        let fixture = Fixture::new("venv-3.12");
        let prefix = fixture.prefix("prefix", 12);
        let venv = fixture.venv("venv", &prefix.join("bin"), 12);
        let env = DiscoveryEnv::dev(DevEnv {
            virtual_env: Some(venv),
            exe_dir: fixture.dir.join("bin"),
            ..DevEnv::default()
        });

        let error = discover(&env, "dev", &fixture.dir).expect_err("3.12");
        assert!(
            matches!(error, DiscoveryError::UnsupportedMinor { ref found, .. } if found == "3.12"),
            "{error}"
        );
    }

    #[test]
    fn the_interpreter_name_parses_both_the_dotted_and_packed_spellings() {
        assert_eq!(parse_minor("python3.14"), Some((14, false)));
        assert_eq!(parse_minor("python3.13t"), Some((13, true)));
        assert_eq!(parse_minor("libpython3.14.so.1.0"), Some((14, false)));
        assert_eq!(parse_minor("libpython3.13t.so.1.0"), Some((13, true)));
        assert_eq!(parse_minor("python3"), None);
        assert_eq!(parse_minor("python3.13types"), None);
        assert_eq!(parse_minor("python"), None);
        assert_eq!(parse_cfg_minor("3.14.10"), Some(14));
        assert_eq!(parse_cfg_minor("3.13.2"), Some(13));
        assert_eq!(parse_cfg_minor("2.7.18"), None);
    }

    #[test]
    fn the_shim_name_and_minor_table_are_the_designs() {
        let libc = if cfg!(target_env = "musl") {
            "musl"
        } else {
            "gnu"
        };
        assert_eq!(
            shim_file(14),
            format!("libblastit_py314-{}-{libc}.so", std::env::consts::ARCH)
        );
        assert_eq!(
            shim_file(13),
            format!("libblastit_py313-{}-{libc}.so", std::env::consts::ARCH)
        );
        assert_eq!(DEFAULT_MINOR, "3.14");
        assert_eq!(SUPPORTED_MINORS, [14, 13]);
        assert_eq!((PY_MINOR_313, PY_MINOR_314), (313, 314));
    }
}
