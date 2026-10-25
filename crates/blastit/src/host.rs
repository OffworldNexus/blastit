//! How blastit was installed, and therefore how it starts itself again
//! (§2.1, §4).
//!
//! Every stage after stage 0 is a new program image: the `execve` barrier that
//! hides the real credentials also means blastit has to be able to name itself.
//! This module is the whole of that knowledge, and it is the only thing the
//! packaging changes:
//!
//! * the **standalone binary** re-executes `/proc/self/exe`, exactly as §4
//!   describes;
//! * a **wheel install** is a Python program: `blastit` is the console script
//!   of the interpreter the wheel was installed into, `blastit._blastit.main()`
//!   runs inside it, and `/proc/self/exe` is that interpreter. Stage 1 is
//!   therefore re-executed as `python -I -m blastit` — the *same* interpreter,
//!   which is what makes the application's dependencies importable, because the
//!   `site-packages` CPython finds next to its own executable are the venv's.
//!
//! dbproxy is the exception either way. It holds the real credentials (§2.2),
//! so it must not run an interpreter at all: a wheel ships it as a separate
//! Python-free binary (`blastit/_bin/blastit-dbproxy`, mode `0755`) and stage 0
//! `execve`s that instead of itself. The standalone binary keeps playing the
//! role itself, with the `--role=dbproxy` it has always used.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use blastit_sys::exe_path;

/// A program that plays one of blastit's roles, and how to ask it for one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reexec {
    /// The path `execve` is given.
    pub program: PathBuf,
    /// The arguments that come before the role's own: `-I -m blastit` under the
    /// wheel, nothing for the standalone binary.
    pub prefix: Vec<OsString>,
}

impl Reexec {
    /// A program asked for a role by its own arguments alone.
    fn program(program: &Path) -> Self {
        Self {
            program: program.to_path_buf(),
            prefix: Vec::new(),
        }
    }
}

/// The installation blastit is running from (§2.1, §4).
///
/// Stage 0 is the only stage that uses it: it is what the two `execve`s of §4
/// name, and it carries the interpreter's import path across the barrier into
/// stage 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    stage1: Reexec,
    dbproxy: Reexec,
    /// The interpreter's `sys.path` as the console script found it, in bytes
    /// (`os.fsencode`): empty for the standalone binary, whose stage 1 embeds
    /// CPython.
    sys_path: Vec<Vec<u8>>,
}

impl Host {
    /// The standalone binary: every role is `/proc/self/exe` (§4).
    pub fn binary() -> Self {
        let program = Reexec::program(exe_path());
        Self {
            stage1: program.clone(),
            dbproxy: program,
            sys_path: Vec::new(),
        }
    }

    /// A wheel install: the extension is running inside `executable`, and
    /// dbproxy is the Python-free binary the wheel ships next to it.
    ///
    /// `sys_path` is the interpreter's import path at the moment the console
    /// script started — `PYTHONPATH`, the working directory, the venv's
    /// `site-packages` — as bytes (`os.fsencode`, so a path that is not valid
    /// UTF-8 survives). Stage 1 is re-executed with `-I`, which drops the first
    /// two, so stage 0 records them here and stage 1 puts them back (§4).
    pub fn wheel(executable: PathBuf, dbproxy: PathBuf, sys_path: Vec<Vec<u8>>) -> Self {
        Self {
            stage1: Reexec {
                program: executable,
                // `-I`: stage 1 is `execve`'d with the application's
                // environment, and the isolation keeps that environment from
                // changing how the interpreter starts (§4). The venv is still
                // found, because it is found next to the executable, and
                // `sys_path` is restored explicitly.
                prefix: vec![
                    OsString::from("-I"),
                    OsString::from("-m"),
                    OsString::from("blastit"),
                ],
            },
            dbproxy: Reexec::program(&dbproxy),
            sys_path,
        }
    }

    /// What stage 0 `execve`s to cross the barrier into stage 1 (§4).
    pub fn stage1(&self) -> &Reexec {
        &self.stage1
    }

    /// What stage 0 `execve`s for the credentials-holding role (§4).
    pub fn dbproxy(&self) -> &Reexec {
        &self.dbproxy
    }

    /// The interpreter's import path, for stage 1 to restore (§4).
    pub fn sys_path(&self) -> &[Vec<u8>] {
        &self.sys_path
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_reexecutes_itself_for_every_role() {
        let host = Host::binary();
        assert_eq!(host.stage1().program, Path::new("/proc/self/exe"));
        assert_eq!(host.dbproxy().program, Path::new("/proc/self/exe"));
        assert!(host.stage1().prefix.is_empty());
        assert!(host.dbproxy().prefix.is_empty());
        assert!(host.sys_path().is_empty());
    }

    #[test]
    fn a_wheel_reexecutes_the_interpreter_and_the_dbproxy_binary() {
        let site_packages = "/app/.venv/lib/python3.14/site-packages/blastit";
        let host = Host::wheel(
            PathBuf::from("/app/.venv/bin/python"),
            PathBuf::from(format!("{site_packages}/_bin/blastit-dbproxy")),
            vec![b"/app".to_vec(), vec![b'/', b'a', 0xff, b'p']],
        );
        assert_eq!(
            host.stage1().program,
            Path::new("/app/.venv/bin/python"),
            "stage 1 is the interpreter the wheel was installed into"
        );
        assert_eq!(
            host.stage1().prefix,
            vec![
                OsString::from("-I"),
                OsString::from("-m"),
                OsString::from("blastit")
            ]
        );
        assert_eq!(
            host.dbproxy().program,
            Path::new(&format!("{site_packages}/_bin/blastit-dbproxy")),
            "dbproxy runs no interpreter"
        );
        assert!(host.dbproxy().prefix.is_empty());
        assert_eq!(
            host.sys_path(),
            [b"/app".to_vec(), vec![b'/', b'a', 0xff, b'p']],
            "the import path travels as bytes, invalid UTF-8 included"
        );
    }
}
