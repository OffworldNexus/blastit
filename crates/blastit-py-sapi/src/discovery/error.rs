//! The refusal type and the §2.3 sentences, kept beside the resolution that
//! produces them.

use std::fmt;
use std::path::PathBuf;

use super::DEFAULT_MINOR;

/// Why a boot refuses to serve a Python view. Every case is one sentence.
#[derive(Debug)]
#[non_exhaustive]
pub enum DiscoveryError {
    /// `BLASTIT_PYTHON` names a file that is not an executable file.
    BlastitPythonNotExecutable {
        /// The command the refusal names (`blastit <cmd>:`).
        command: String,
        /// The value the variable held.
        path: PathBuf,
        /// The venv root the fix points at.
        venv: PathBuf,
    },
    /// `BLASTIT_PYTHON` is relative, and this is a deployment.
    BlastitPythonNotAbsolute {
        /// The command the refusal names.
        command: String,
        /// The value the variable held.
        path: PathBuf,
        /// The venv root the fix points at.
        venv: PathBuf,
    },
    /// The interpreter is a free-threaded build (`3.13t`/`3.14t`).
    FreeThreaded {
        /// The command the refusal names.
        command: String,
        /// The version the interpreter reports, e.g. `3.13t`.
        found: String,
        /// The interpreter file.
        interpreter: PathBuf,
    },
    /// The interpreter's minor is not one a WSGI view can run on.
    UnsupportedMinor {
        /// The command the refusal names.
        command: String,
        /// The version that was found, e.g. `3.12`.
        found: String,
        /// The interpreter file.
        interpreter: PathBuf,
    },
    /// No interpreter file was found.
    NoInterpreter {
        /// The command the refusal names.
        command: String,
    },
    /// The interpreter's installation prefix could not be resolved.
    Unresolvable {
        /// The interpreter file.
        path: PathBuf,
        /// What went wrong.
        detail: String,
        /// The release the fix installs.
        minor: String,
    },
    /// The interpreter's CPython minor could not be read from anything.
    UnreadableMinor {
        /// The interpreter file.
        path: PathBuf,
        /// What was read and found wanting.
        detail: String,
    },
    /// The prefix holds no shared `libpython`.
    NoLibpython {
        /// The resolved prefix.
        python_home: PathBuf,
        /// The minor, dotted (`3.14`).
        minor: String,
    },
    /// No per-minor shim was found where this boot may look.
    NoShim {
        /// Whether this was a deployment (the two sentences differ).
        deployment: bool,
        /// The minor, dotted (`3.14`).
        minor: String,
        /// The file name that was searched for.
        name: String,
        /// The directories that were searched, in order.
        searched: Vec<PathBuf>,
    },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlastitPythonNotExecutable {
                command,
                path,
                venv,
            } => write!(
                f,
                "blastit {command}: BLASTIT_PYTHON ({}) is not an executable file; unset \
                 BLASTIT_PYTHON, or point it at {}/bin/python3",
                path.display(),
                venv.display()
            ),
            Self::BlastitPythonNotAbsolute {
                command,
                path,
                venv,
            } => write!(
                f,
                "blastit {command}: BLASTIT_PYTHON ({}) is not an absolute path; a deployment \
                 reads only absolute paths — use an absolute path such as {}/bin/python3",
                path.display(),
                venv.display()
            ),
            Self::FreeThreaded {
                command,
                found,
                interpreter,
            } => write!(
                f,
                "blastit {command}: CPython {found} at {} is free-threaded and cannot serve \
                 WSGI views; reinstall it without --free-threaded (uv python install \
                 cpython-{DEFAULT_MINOR})",
                interpreter.display()
            ),
            Self::UnsupportedMinor {
                command,
                found,
                interpreter,
            } => write!(
                f,
                "blastit {command}: CPython {found} at {} is not supported for WSGI views \
                 (CPython 3.13 and 3.14 are supported; Python 3.12+ runs blastit check, fmt, \
                 and fold): run uv python install {DEFAULT_MINOR} && uv venv --python \
                 {DEFAULT_MINOR}",
                interpreter.display()
            ),
            Self::NoInterpreter { command } => write!(
                f,
                "blastit {command}: no CPython interpreter was found; install it (uv python \
                 install {DEFAULT_MINOR}, or your distribution's python3.14 package)"
            ),
            Self::Unresolvable {
                path,
                detail,
                minor,
            } => write!(
                f,
                "CPython at {} cannot be used: {detail}; reinstall it with uv python install \
                 {minor}",
                path.display()
            ),
            Self::UnreadableMinor { path, detail } => write!(
                f,
                "CPython at {} does not say which minor it is ({detail}); reinstall it with uv \
                 python install {DEFAULT_MINOR}",
                path.display()
            ),
            Self::NoLibpython { python_home, minor } => write!(
                f,
                "CPython {minor} is installed at {}, but this host has no shared libpython for \
                 CPython {minor}; install it — Debian/Ubuntu: apt install libpython{minor}, \
                 Alpine: apk add python3, Fedora/RHEL: dnf install python3-libs — or provision \
                 one with uv python install {minor}",
                python_home.display()
            ),
            Self::NoShim {
                deployment,
                minor,
                name,
                searched,
            } => {
                if *deployment {
                    write!(
                        f,
                        "CPython {minor} is ready, but no {name} shim sits beside this blastit; \
                         install the blastit build that ships the CPython {minor} shim"
                    )
                } else {
                    write!(
                        f,
                        "CPython {minor} is ready, but no {name} shim was found (looked in {}); \
                         build one with make py-sapi, or point BLASTIT_PY_SHIM_DIR at a directory \
                         that holds it",
                        searched
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
        }
    }
}

impl std::error::Error for DiscoveryError {}
