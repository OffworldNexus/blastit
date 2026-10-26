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
//!
//! # D6: the file with the capabilities is never the file a child runs
//!
//! A rootless [`crate::Host`] is started from a file that carries the boot's
//! capabilities (B1, B2), and that file must be **init's alone**:
//!
//! * a child's bounding set is empty by the time it execs anything, and the
//!   kernel refuses to `execve` a file with file capabilities when the file's
//!   permitted set cannot be honoured — `EPERM`, "insufficient to execute
//!   correctly". So `--role=dbproxy` cannot be `/proc/self/exe` when that is the
//!   capable file: `BLASTIT_EXEC` names the capability-free copy for a
//!   standalone install;
//! * every other process of the same uid — an operator's `python manage.py`,
//!   a second `manage.py`, anything a deployment runs next to the server — must
//!   not find the capabilities either, which is why the capable file is a
//!   *dedicated copy* with mode `0500` owned by the uid init runs as, and the
//!   shared interpreter or binary stays capability-free.
//!
//! [`placement`] decides that, and refuses the two deployments that cannot
//! work, naming the file to fix.

use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use blastit_sys::exe_path;

/// The environment variable that names a standalone install's capability-free
/// binary (D6/B2).
///
/// Read by the standalone binary only; a wheel always `execve`s the
/// `blastit-dbproxy` it ships.
pub const EXEC: &str = "BLASTIT_EXEC";

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
    pub fn program(program: &Path) -> Self {
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
    /// D6: how this installation was made, which decides what a capability
    /// mistake tells an operator to fix.
    install: Install,
    /// D6: the capability-free counterpart of the file that may carry the
    /// capabilities — the interpreter a wheel's `sys.prefix` names, or the
    /// binary `BLASTIT_EXEC` names.
    ///
    /// `None` when the installation cannot name one, which is only allowed when
    /// nothing carries file capabilities: [`Host::placement`] refuses a
    /// deployment that has to be able to name it and cannot (B1, B2).
    plain: Option<PathBuf>,
}

/// Which install this is (D6).
///
/// It changes nothing about the roles, only about the sentence a capability
/// mistake produces: the file to `setcap` is an interpreter under a wheel and a
/// binary under a standalone install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    /// The standalone binary, started directly (§2.1).
    Binary,
    /// A wheel install: a console script on a project's interpreter (D2).
    Wheel,
}

impl Install {
    /// The noun an operator reads: what has to be copied and `setcap`ed.
    pub const fn file(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Wheel => "interpreter",
        }
    }

    /// The variable that names the capability-free file, for the standalone
    /// install; a wheel finds its own.
    pub const fn naming(self) -> &'static str {
        match self {
            Self::Binary => "BLASTIT_EXEC",
            Self::Wheel => "sys.prefix/bin/python (the console script's own interpreter)",
        }
    }
}

/// What stage 0 execs, once the deployment has been checked (D6/B1, B2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The program stage 1 is `execve`d as — init's image, which is the file a
    /// deployment may have given the capabilities to.
    pub init: PathBuf,
    /// The program `--role=dbproxy` is `execve`d as: a child's `execve`, which
    /// may never be a file with file capabilities while the child's bounding set
    /// is empty.
    pub dbproxy: PathBuf,
}

impl Host {
    /// The standalone binary: every role is `/proc/self/exe` (§4).
    ///
    /// [`EXEC`] (`BLASTIT_EXEC`) names the capability-free copy of this binary
    /// when the deployment gave the capabilities to a dedicated one (D6/B2); it
    /// is what [`Host::placement`] then hands to `--role=dbproxy`.
    pub fn binary() -> Self {
        let program = Reexec::program(exe_path());
        let plain = std::env::var_os(EXEC)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        Self {
            stage1: program.clone(),
            dbproxy: program,
            sys_path: Vec::new(),
            install: Install::Binary,
            plain,
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
    ///
    /// `plain` is the interpreter a *child* must use — `sys.prefix/bin/python`,
    /// which is what a project's own `python` is (D6/B1). It is the same file as
    /// `executable` under an ambient-capability deployment, and a *different*
    /// one when the deployment gave the capabilities to a dedicated copy.
    pub fn wheel(
        executable: PathBuf,
        dbproxy: PathBuf,
        plain: Option<PathBuf>,
        sys_path: Vec<Vec<u8>>,
    ) -> Self {
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
            install: Install::Wheel,
            plain,
        }
    }

    /// What stage 0 `execve`s to cross the barrier into stage 1 (§4).
    pub fn stage1(&self) -> &Reexec {
        &self.stage1
    }

    /// What stage 0 `execve`s for the credentials-holding role, once the
    /// deployment has been checked (D6).
    ///
    /// [`Host::placement`] is the one to ask: this is the *unchecked* answer,
    /// which is `/proc/self/exe` for a standalone install even when that file
    /// carries the capabilities a child cannot be granted.
    pub fn dbproxy(&self) -> &Reexec {
        &self.dbproxy
    }

    /// The interpreter's import path, for stage 1 to restore (§4).
    pub fn sys_path(&self) -> &[Vec<u8>] {
        &self.sys_path
    }

    /// Which install this is (D6).
    pub fn install(&self) -> Install {
        self.install
    }

    /// The capability-free program this installation can name (D6).
    pub fn plain(&self) -> Option<&Path> {
        self.plain.as_deref()
    }

    /// The programs stage 0 must `execve`, or the reason it cannot (D6/B1, B2).
    ///
    /// `drops_bounding` is whether the children's bounding sets will be emptied
    /// ([`blastit_sys::caps::detect`] says): a child cannot `execve` a file with
    /// file capabilities at all, so `--role=dbproxy` must not point at one.
    ///
    /// # Errors
    ///
    /// A sentence naming the file to fix: `BLASTIT_EXEC` for a standalone
    /// install, the interpreter for a wheel. See [`placement`].
    pub fn placement(&self, drops_bounding: bool) -> Result<Placement, String> {
        // `BLASTIT_EXEC` is an operator's variable, so it is the one worth
        // validating by name: a wheel's capability-free interpreter comes from
        // the installation's own detection, which checks it is there.
        if self.install == Install::Binary
            && let Some(plain) = self.plain()
            && !plain.is_file()
        {
            return Err(format!(
                "{EXEC}={} is not a file: a rootless standalone install names the \
                 capability-free copy of the binary there",
                plain.display()
            ));
        }
        let child = match self.install {
            // A standalone install plays every role itself, and `BLASTIT_EXEC`
            // is the capability-free copy of it (D6/B2).
            Install::Binary => self.plain().unwrap_or(&self.dbproxy.program),
            // A wheel's dbproxy runs no interpreter and needs no capability, so
            // the shipped binary is always a child program (D6/B1).
            Install::Wheel => &self.dbproxy.program,
        };
        placement(
            self.install,
            &self.stage1.program,
            child,
            self.plain(),
            drops_bounding,
            has_file_caps,
        )
    }
}

/// Whether `path` carries file capabilities (`security.capability`, D6).
///
/// This is the attribute the kernel reads at an `execve` to grant the file's
/// permitted set; a child whose bounding set is empty cannot honour it, which is
/// why the boot refuses to point a child at such a file at all (B1, B2). A
/// non-root process may read the attribute of any file it can read, and a file
/// that is not there has none.
pub fn has_file_caps(path: &Path) -> bool {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `getxattr` takes a NUL-terminated path, a name, a buffer and its
    // length; a null buffer of length zero asks for the size, and it reads no
    // memory we do not own.
    let size = unsafe {
        libc::getxattr(
            path.as_ptr(),
            c"security.capability".as_ptr(),
            std::ptr::null_mut(),
            0,
        )
    };
    size > 0
}

/// Decides which program plays each role, or names what a deployment must
/// change (D6/B1, B2).
///
/// Pure — `caps_of` is the only thing it cannot compute by itself — so every
/// deployment shape, and the sentence it produces, is a unit test:
///
/// * a file that carries the capabilities must be a **dedicated copy**, or every
///   other process of the same uid could run it and become `init`: it may not be
///   the capability-free file the children use ([`Install::Wheel`]'s
///   interpreter, [`Install::Binary`]'s binary);
/// * a child may not `execve` a file with file capabilities **at all** when its
///   bounding set will be emptied, so `--role=dbproxy` may not point at the
///   capable file; the fix is `BLASTIT_EXEC`.
///
/// # Errors
///
/// The sentence an operator reads, naming the file and the `setcap` line.
pub fn placement(
    install: Install,
    init: &Path,
    child: &Path,
    plain: Option<&Path>,
    drops_bounding: bool,
    caps_of: impl Fn(&Path) -> bool,
) -> Result<Placement, String> {
    if caps_of(init) {
        // The capable file must be init's alone.
        let Some(plain) = plain else {
            return Err(format!(
                "{} carries file capabilities and this installation cannot name the \
                 capability-free {} the children must use instead: {}",
                init.display(),
                install.file(),
                install.naming()
            ));
        };
        if plain == init {
            return Err(capable_copy_advice(install, init));
        }
    }
    if drops_bounding && caps_of(child) {
        return Err(format!(
            "{} carries file capabilities, and the children of this boot cannot \
             `execve` such a file at all (their bounding set is empty): point {} at \
             the capability-free copy",
            child.display(),
            install.naming()
        ));
    }
    Ok(Placement {
        init: init.to_path_buf(),
        dbproxy: child.to_path_buf(),
    })
}

/// The sentence an operator gets when the *shared* file carries the
/// capabilities (D6/B1, B2).
///
/// It is worth being long: the mistake is easy to make (`setcap … python`), the
/// fix is four commands, and the reason — every process of that uid could
/// otherwise become init — is not visible in the failure itself.
fn capable_copy_advice(install: Install, init: &Path) -> String {
    let resolved = std::fs::read_link(init).unwrap_or_else(|_| init.to_path_buf());
    let copy = resolved.with_file_name(format!(
        "{}-init",
        resolved
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let tail = match install {
        Install::Wheel => format!(
            "  then start `{} -m blastit` instead of the shared interpreter\n  \
             (a copied interpreter has to stay in the same directory: CPython finds \
             its stdlib and its venv from the executable's own path)",
            copy.display()
        ),
        Install::Binary => format!(
            "  then start {}, and set {EXEC}={} so that `--role=dbproxy` runs the \
             capability-free binary",
            copy.display(),
            resolved.display()
        ),
    };
    format!(
        "{init} carries file capabilities: give them to a *dedicated copy* instead, \
         because every process of the uid init runs as could run that file \
         otherwise — and init must be the only one that can.\n  \
         cp {src} {copy}\n  chown <the uid init runs as>:<its gid> {copy}\n  \
         chmod 0500 {copy}\n  setcap '{setcap}+ep' {copy}\n{tail}",
        init = resolved.display(),
        src = resolved.display(),
        copy = copy.display(),
        setcap = blastit_sys::caps::Capability::SETCAP,
        tail = tail,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Serialises the two tests that read or write `BLASTIT_EXEC`.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The lock described on [`ENV`]; a poisoned lock still runs the next test.
    fn env() -> std::sync::MutexGuard<'static, ()> {
        ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `caps_of` for the pure `placement` tests: exactly these paths carry file
    /// capabilities.
    fn caps_of<'a>(paths: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
        move |path| paths.iter().any(|held| Path::new(held) == path)
    }

    #[test]
    fn the_binary_reexecutes_itself_for_every_role() {
        let _env = env();
        // SAFETY: `env()` is held, so no other test reads or writes the variable.
        unsafe { std::env::remove_var(EXEC) };
        let host = Host::binary();
        assert_eq!(host.stage1().program, Path::new("/proc/self/exe"));
        assert_eq!(host.dbproxy().program, Path::new("/proc/self/exe"));
        assert!(host.stage1().prefix.is_empty());
        assert!(host.dbproxy().prefix.is_empty());
        assert!(host.sys_path().is_empty());
        assert_eq!(host.install(), Install::Binary);
        assert_eq!(host.plain(), None);
        // Nothing carries file capabilities, so there is nothing to keep apart.
        assert_eq!(
            host.placement(true).unwrap(),
            Placement {
                init: PathBuf::from("/proc/self/exe"),
                dbproxy: PathBuf::from("/proc/self/exe"),
            }
        );
    }

    #[test]
    fn a_rootless_binary_plays_dbproxy_as_the_capability_free_copy() {
        let _env = env();
        // The *test* binary's own path, which is a file and is not the file the
        // capabilities are on: enough to exercise the check and the choice
        // (B2). `CARGO_BIN_EXE_blastit` is only set for the integration tests,
        // so the path is derived from this executable instead.
        let plain = std::env::current_exe().unwrap();
        // SAFETY: `env()` is held, so no other test reads or writes the variable.
        unsafe { std::env::set_var(EXEC, &plain) };
        let host = Host::binary();
        assert_eq!(host.install(), Install::Binary);
        assert_eq!(host.plain(), Some(plain.as_path()));
        assert_eq!(host.placement(true).unwrap().dbproxy, plain);
        // SAFETY: `env()` is still held.
        unsafe { std::env::remove_var(EXEC) };
    }

    #[test]
    fn a_binary_that_is_not_there_is_refused_by_name() {
        let _env = env();
        // SAFETY: `env()` is held, so no other test reads or writes the variable.
        unsafe { std::env::set_var(EXEC, "/nonexistent/blastit") };
        let err = Host::binary().placement(true).unwrap_err();
        assert!(err.contains("BLASTIT_EXEC=/nonexistent/blastit"), "{err}");
        assert!(err.contains("not a file"), "{err}");
        // SAFETY: `env()` is still held.
        unsafe { std::env::remove_var(EXEC) };
    }

    #[test]
    fn a_wheel_reexecutes_the_interpreter_and_the_dbproxy_binary() {
        let site_packages = "/app/.venv/lib/python3.14/site-packages/blastit";
        let host = Host::wheel(
            PathBuf::from("/app/.venv/bin/python"),
            PathBuf::from(format!("{site_packages}/_bin/blastit-dbproxy")),
            Some(PathBuf::from("/app/.venv/bin/python")),
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
        assert_eq!(host.install(), Install::Wheel);
        // Nothing carries file capabilities here, so the wheel's dbproxy binary
        // is the child program, and the check passes (B1).
        assert_eq!(
            host.placement(true).unwrap(),
            Placement {
                init: PathBuf::from("/app/.venv/bin/python"),
                dbproxy: PathBuf::from(format!("{site_packages}/_bin/blastit-dbproxy")),
            }
        );
    }

    #[test]
    fn a_shared_interpreter_with_the_capabilities_is_refused_with_the_recipe() {
        // B1: `setcap` on the interpreter a project's `python` resolves to means
        // *every* process of that uid could run it and become init.
        let interpreter = Path::new("/usr/local/bin/python3.13");
        let err = placement(
            Install::Wheel,
            interpreter,
            Path::new("/usr/local/lib/python3.13/site-packages/blastit/_bin/blastit-dbproxy"),
            Some(interpreter),
            true,
            caps_of(&["/usr/local/bin/python3.13"]),
        )
        .unwrap_err();
        assert!(err.contains("dedicated copy"), "{err}");
        assert!(err.contains("cp /usr/local/bin/python3.13"), "{err}");
        assert!(err.contains("/usr/local/bin/python3.13-init"), "{err}");
        assert!(err.contains("chmod 0500"), "{err}");
        assert!(err.contains(blastit_sys::caps::Capability::SETCAP), "{err}");
        assert!(err.contains("-m blastit"), "{err}");
        assert!(
            err.contains("CPython finds its stdlib and its venv"),
            "a copied interpreter has to stay next to the original: {err}"
        );
    }

    #[test]
    fn a_shared_binary_with_the_capabilities_is_refused_with_the_recipe() {
        // B2: the standalone install's own version of the same mistake.
        let program = Path::new("/usr/local/bin/blastit");
        let err = placement(
            Install::Binary,
            program,
            program,
            Some(program),
            true,
            caps_of(&["/usr/local/bin/blastit"]),
        )
        .unwrap_err();
        assert!(err.contains("cp /usr/local/bin/blastit"), "{err}");
        assert!(err.contains("/usr/local/bin/blastit-init"), "{err}");
        assert!(err.contains("chmod 0500"), "{err}");
        assert!(err.contains(EXEC), "it names BLASTIT_EXEC: {err}");
        assert!(err.contains(blastit_sys::caps::Capability::SETCAP), "{err}");
    }

    #[test]
    fn a_capability_that_cannot_be_named_is_refused_by_name() {
        // A wheel that cannot say which interpreter a child should use, while
        // the one it runs from carries file capabilities: there is no safe way
        // to continue (B1).
        let err = placement(
            Install::Wheel,
            Path::new("/opt/blastit/python-init"),
            Path::new("/w/_bin/blastit-dbproxy"),
            None,
            true,
            caps_of(&["/opt/blastit/python-init"]),
        )
        .unwrap_err();
        assert!(err.contains("cannot name"), "{err}");
        assert!(err.contains("sys.prefix/bin/python"), "{err}");
    }

    #[test]
    fn a_child_may_not_be_pointed_at_a_file_with_capabilities() {
        // B2's other half: with an empty bounding set the `execve` itself fails,
        // so the boot refuses and says which variable to point elsewhere.
        let err = placement(
            Install::Binary,
            Path::new("/proc/self/exe"),
            Path::new("/usr/local/bin/blastit-init"),
            Some(Path::new("/usr/local/bin/blastit-init")),
            true,
            caps_of(&["/usr/local/bin/blastit-init"]),
        )
        .unwrap_err();
        assert!(err.contains("bounding set is empty"), "{err}");
        assert!(err.contains(EXEC), "{err}");

        // M1: without `cap_setpcap` the bounding set survives, the `execve`
        // works, and `no_new_privs` is what stops the grant — so the same
        // deployment is allowed, and it was allowed before D6 too.
        assert!(
            placement(
                Install::Binary,
                Path::new("/proc/self/exe"),
                Path::new("/usr/local/bin/blastit-init"),
                Some(Path::new("/usr/local/bin/blastit-init")),
                false,
                caps_of(&["/usr/local/bin/blastit-init"]),
            )
            .is_ok()
        );
    }

    #[test]
    fn a_wheel_keeps_its_own_dbproxy_binary_whatever_the_capabilities_are() {
        // The wheel's dbproxy runs no interpreter and needs no capability: as
        // long as *it* has none, the placement is fine, and the interpreter the
        // deployment gave the capabilities to is init's (B1).
        let placement = placement(
            Install::Wheel,
            Path::new("/usr/local/bin/python3.13-init"),
            Path::new("/app/.venv/lib/python3.13/site-packages/blastit/_bin/blastit-dbproxy"),
            Some(Path::new("/usr/local/bin/python3.13")),
            true,
            caps_of(&["/usr/local/bin/python3.13-init"]),
        )
        .unwrap();
        assert_eq!(placement.init, Path::new("/usr/local/bin/python3.13-init"));
        assert_eq!(
            placement.dbproxy,
            Path::new("/app/.venv/lib/python3.13/site-packages/blastit/_bin/blastit-dbproxy")
        );
    }

    #[test]
    fn has_file_caps_wants_an_attribute_that_is_there() {
        // The attribute only exists when capabilities are set, so a file
        // without one answers false — and so does a path that is not there.
        assert!(!has_file_caps(Path::new("/proc/self/status")));
        assert!(!has_file_caps(Path::new("/nonexistent/blastit")));
    }

    #[test]
    fn has_file_caps_sees_one_that_is() {
        if !rustix::process::geteuid().is_root() {
            println!("skipped: setting security.capability needs CAP_SETFCAP");
            return;
        }
        // Write the attribute the kernel reads at an `execve`: a
        // `vfs_cap_data` with `VFS_CAP_REVISION_2 | VFS_CAP_FLAGS_EFFECTIVE`
        // and `CAP_CHOWN` in its permitted set. Setting it needs CAP_SETFCAP,
        // which root has — and which is why a *deployment* needs root or
        // `setcap` to do the same thing.
        let path = std::env::temp_dir().join(format!("blastit-caps-{}", std::process::id()));
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        let mut value = [0_u8; 20];
        value[..4].copy_from_slice(&0x0200_0001_u32.to_le_bytes());
        value[4..8].copy_from_slice(&1_u32.to_le_bytes());
        let name = c"security.capability";
        let path_c = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `setxattr` takes a NUL-terminated path, a NUL-terminated
        // name, a buffer of the length given and a flags word; it reads no
        // memory we do not own.
        let wrote = unsafe {
            libc::setxattr(
                path_c.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        assert_eq!(wrote, 0, "setxattr: {}", std::io::Error::last_os_error());
        assert!(
            has_file_caps(&path),
            "an attribute that is there is seen (B1, B2)"
        );
        std::fs::remove_file(&path).unwrap();
        assert!(!has_file_caps(&path), "and gone when the file is");
    }
}
