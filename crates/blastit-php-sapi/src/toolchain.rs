//! The per-minor PHP toolchain: discovery, refusal, and the shim filename.
//!
//! Discovery is the composition root's job, so this module reads the
//! environment only through [`Env`]: [`Toolchain::discover`] is the real one and
//! tests inject their own (a temporary directory holding a fake `php-config`
//! proves the allowed path; an empty `PATH` proves the refusal).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The PHP minors this crate has a shim ABI for, newest first.
pub const SUPPORTED_MINORS: [Minor; 4] = [Minor::Php86, Minor::Php85, Minor::Php84, Minor::Php83];

/// The `php-config` names searched on `PATH`, in order.
pub const CONFIG_NAMES: [&str; 5] = [
    "php-config",
    "php-config8.6",
    "php-config8.5",
    "php-config8.4",
    "php-config8.3",
];

/// One supported PHP minor. The shim is compiled once per variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Minor {
    /// PHP 8.3 — `ZEND_MODULE_API_NO` 20230831.
    Php83,
    /// PHP 8.4 — `ZEND_MODULE_API_NO` 20240924.
    Php84,
    /// PHP 8.5 — `ZEND_MODULE_API_NO` 20250925.
    Php85,
    /// PHP 8.6 — the minor `libphp8.6-embed` (`debian:bookworm-slim` + the
    /// Surý repository) and `php86-embed` (`alpine:edge`) ship.
    Php86,
}

impl Minor {
    /// Parses a `php-config --version` reply (`"8.5.4"`, `"8.5"`).
    ///
    /// Anything whose first two components are not `8.3`, `8.4`, `8.5` or
    /// `8.6` is `None`: an older or newer minor is refused, not guessed at.
    pub fn parse(version: &str) -> Option<Self> {
        let mut parts = version.trim().split('.');
        let major = parts.next()?.parse::<u32>().ok()?;
        let minor = parts.next()?.parse::<u32>().ok()?;
        match (major, minor) {
            (8, 3) => Some(Self::Php83),
            (8, 4) => Some(Self::Php84),
            (8, 5) => Some(Self::Php85),
            (8, 6) => Some(Self::Php86),
            _ => None,
        }
    }

    /// The code `blastit_php_minor()` returns from a shim of this minor.
    pub fn code(self) -> u32 {
        match self {
            Self::Php83 => 803,
            Self::Php84 => 804,
            Self::Php85 => 805,
            Self::Php86 => 806,
        }
    }

    /// The `<major><minor>` infix of the shim name (`"84"`).
    pub(crate) const fn digits(self) -> &'static str {
        match self {
            Self::Php83 => "83",
            Self::Php84 => "84",
            Self::Php85 => "85",
            Self::Php86 => "86",
        }
    }

    /// The dotted minor (`"8.4"`), for messages and package names.
    pub fn dotted(self) -> &'static str {
        match self {
            Self::Php83 => "8.3",
            Self::Php84 => "8.4",
            Self::Php85 => "8.5",
            Self::Php86 => "8.6",
        }
    }

    /// The shipped shim for this minor on this build's target, e.g.
    /// `libblastit_php84-x86_64-gnu.so` (`ADR-0010` §4.1).
    ///
    /// The `<arch>` and `<libc>` come from the compile-time target, not from a
    /// runtime probe, so the name the boot selects and the name `make php-sapi`
    /// writes cannot disagree on a native build.
    pub fn shim_file(self) -> String {
        format!(
            "libblastit_php{}-{}-{}.so",
            self.digits(),
            target_arch(),
            target_libc()
        )
    }

    /// Whether a shim reporting `code` may serve a host of this minor.
    ///
    /// The worker asks the shim for `blastit_php_minor()` and refuses a
    /// mismatch before the first request: a 8.3 shim against an 8.4 `libphp`
    /// reads `EG(bailout)` at the wrong offset, which turns `exit()` into a
    /// `SIGSEGV` (ADR-0010 §3).
    pub fn accepts(self, shim_code: u32) -> bool {
        shim_code == self.code()
    }

    /// The `php-config` names that provide this minor, most specific first.
    pub fn config_names(self) -> [&'static str; 2] {
        match self {
            Self::Php83 => ["php-config8.3", "php-config"],
            Self::Php84 => ["php-config8.4", "php-config"],
            Self::Php85 => ["php-config8.5", "php-config"],
            Self::Php86 => ["php-config8.6", "php-config"],
        }
    }
}

impl fmt::Display for Minor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.dotted())
    }
}

/// The compile-time target's architecture, as the shim name spells it.
///
/// `ADR-0010` §4.1: a shim is named for the `(arch, libc)` the *boot* was
/// compiled for, so the boot can combine that tuple with the host `libphp`'s
/// probed minor. This is therefore `cfg!(target_arch)` and never a runtime
/// `uname`, exactly as `stage0` spells it.
const fn target_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    }
}

/// The compile-time target's libc, as the shim name spells it (`gnu`/`musl`).
const fn target_libc() -> &'static str {
    if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    }
}

/// The one environment seam [`Toolchain::discover_with`] needs.
pub trait Env {
    /// The value of an environment variable, or `None` when unset.
    fn var(&self, name: &str) -> Option<String>;
}

/// The process environment: used by [`Toolchain::discover`] only.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

/// A usable per-minor PHP development toolchain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolchain {
    minor: Minor,
    php_config: PathBuf,
    include_dir: PathBuf,
    version: String,
}

impl Toolchain {
    /// Finds a supported `php-config` in the real process environment.
    pub fn discover() -> Result<Self, ToolchainError> {
        Self::discover_with(&ProcessEnv)
    }

    /// Finds a supported `php-config` through an injected [`Env`].
    ///
    /// `PHP_CONFIG` wins when it works. Otherwise every directory on `PATH` is
    /// tried, once per name in [`CONFIG_NAMES`] order, so an unversioned
    /// `php-config` that is not one of ours does not hide a versioned one.
    pub fn discover_with(env: &dyn Env) -> Result<Self, ToolchainError> {
        let mut unsupported_version: Option<(PathBuf, String)> = None;
        let mut include_error: Option<(PathBuf, String)> = None;
        let mut spawn_failure: Option<(PathBuf, String)> = None;

        if let Some(explicit) = env.var("PHP_CONFIG")
            && !explicit.is_empty()
        {
            match Self::from_exe(Path::new(&explicit)) {
                Ok(toolchain) => return Ok(toolchain),
                Err(ToolchainError::UnsupportedVersion { path, version }) => {
                    unsupported_version = Some((path, version));
                }
                Err(ToolchainError::IncludeDir { path, detail }) => {
                    include_error = Some((path, detail));
                }
                // A PHP_CONFIG pointing at nothing is a typo; PATH still gets
                // its turn, and `NotFound` names `PHP_CONFIG` too. A spawn
                // failure is *not* such a typo: it is remembered, because
                // "could not be started" must never become "not found".
                Err(ToolchainError::Spawn { path, detail }) => {
                    spawn_failure = Some((path, detail));
                }
                Err(ToolchainError::NotFound | ToolchainError::NotExecutable { .. }) => {}
            }
        }

        let path = env.var("PATH").unwrap_or_default();
        for name in CONFIG_NAMES {
            for dir in path.split(':') {
                if dir.is_empty() {
                    continue;
                }
                match Self::from_exe(&Path::new(dir).join(name)) {
                    Ok(toolchain) => return Ok(toolchain),
                    Err(ToolchainError::UnsupportedVersion { path, version }) => {
                        if unsupported_version.is_none() {
                            unsupported_version = Some((path, version));
                        }
                    }
                    Err(ToolchainError::IncludeDir { path, detail }) => {
                        if include_error.is_none() {
                            include_error = Some((path, detail));
                        }
                    }
                    Err(ToolchainError::Spawn { path, detail }) => {
                        if spawn_failure.is_none() {
                            spawn_failure = Some((path, detail));
                        }
                    }
                    Err(ToolchainError::NotFound | ToolchainError::NotExecutable { .. }) => {}
                }
            }
        }

        // Report the most actionable failure: a PHP the crate has no shim for,
        // then a toolchain whose headers are missing, then a candidate the
        // kernel would not start (the last is "we could not ask", so it must
        // never collapse into `NotFound`).
        if let Some((path, version)) = unsupported_version {
            return Err(ToolchainError::UnsupportedVersion { path, version });
        }
        if let Some((path, detail)) = include_error {
            return Err(ToolchainError::IncludeDir { path, detail });
        }
        if let Some((path, detail)) = spawn_failure {
            return Err(ToolchainError::Spawn { path, detail });
        }
        Err(ToolchainError::NotFound)
    }

    /// Validates one `php-config` path: version, then include directory.
    fn from_exe(exe: &Path) -> Result<Self, ToolchainError> {
        if !exe.is_file() {
            return Err(ToolchainError::NotExecutable {
                path: exe.to_path_buf(),
            });
        }
        let version = match run(exe, "--version") {
            Ok(version) => version,
            // It exists but did not answer: a PATH entry that is not our
            // `php-config`, so the search keeps going.
            Err(RunError::Failed(_detail)) => {
                return Err(ToolchainError::NotExecutable {
                    path: exe.to_path_buf(),
                });
            }
            Err(RunError::Spawn(err)) => return Err(spawn_error(exe, err)),
        };
        let version = version.trim().to_owned();
        let minor = Minor::parse(&version).ok_or_else(|| ToolchainError::UnsupportedVersion {
            path: exe.to_path_buf(),
            version: version.clone(),
        })?;
        let include = match run(exe, "--include-dir") {
            Ok(include) => include,
            Err(RunError::Failed(detail)) => {
                return Err(ToolchainError::IncludeDir {
                    path: exe.to_path_buf(),
                    detail,
                });
            }
            Err(RunError::Spawn(err)) => return Err(spawn_error(exe, err)),
        };
        let include_dir = PathBuf::from(include.trim());
        if !include_dir.is_dir() {
            return Err(ToolchainError::IncludeDir {
                path: exe.to_path_buf(),
                detail: format!("--include-dir returned {}", include_dir.display()),
            });
        }
        Ok(Self {
            minor,
            php_config: exe.to_path_buf(),
            include_dir,
            version,
        })
    }

    /// The minor this toolchain builds for.
    pub fn minor(&self) -> Minor {
        self.minor
    }

    /// The shim this toolchain builds, e.g. `libblastit_php84-x86_64-gnu.so`.
    pub fn shim_file(&self) -> String {
        self.minor.shim_file()
    }

    /// The `php-config` that was found.
    pub fn php_config(&self) -> &Path {
        &self.php_config
    }

    /// The `--include-dir` holding this minor's `php.h`.
    pub fn include_dir(&self) -> &Path {
        &self.include_dir
    }

    /// The full version string `php-config --version` printed.
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// Why no usable toolchain was found. Every message names the fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolchainError {
    /// No `php-config` on `PATH` and none through `PHP_CONFIG`.
    NotFound,
    /// A path that is not an executable, or did not answer `--version`.
    NotExecutable {
        /// The candidate that failed.
        path: PathBuf,
    },
    /// The kernel would not start this candidate, so whether it is usable is
    /// unknown: a transient `fork`/`exec` failure (`EAGAIN` under load,
    /// `ETXTBSY` while another writer held the file) that outlived the
    /// bounded retries.
    Spawn {
        /// The candidate that could not be started.
        path: PathBuf,
        /// The operating system's reason.
        detail: String,
    },
    /// A working `php-config` for a minor this crate has no shim for.
    UnsupportedVersion {
        /// The `php-config` that answered.
        path: PathBuf,
        /// What it reported.
        version: String,
    },
    /// `--include-dir` failed or does not name a directory.
    IncludeDir {
        /// The `php-config` that answered.
        path: PathBuf,
        /// What went wrong.
        detail: String,
    },
}

impl fmt::Display for ToolchainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(
                f,
                "no php-config found (searched PHP_CONFIG, then {} on PATH); install the PHP embed \
                 development package — Debian/Ubuntu: apt install php8.6-dev libphp8.6-embed \
                 (stock Debian: apt install libphp-embed), Alpine: apk add php86-dev php86-embed, \
                 Fedora/RHEL: dnf install php86-php-devel php-embedded — or set \
                 PHP_CONFIG=/path/to/php-config8.6",
                CONFIG_NAMES.join(", ")
            ),
            Self::NotExecutable { path } => write!(
                f,
                "{} is not an executable php-config (or it did not answer --version)",
                path.display()
            ),
            Self::Spawn { path, detail } => write!(
                f,
                "could not run {} ({detail}): the host refused to start it, so this build cannot \
                 tell whether it is a usable php-config — retry, or set PHP_CONFIG to a php-config \
                 that runs",
                path.display()
            ),
            Self::UnsupportedVersion { path, version } => write!(
                f,
                "{} reports PHP {version}; this build has SAPI shims for 8.3, 8.4, 8.5 and 8.6 \
                 only (install the matching php8.X-dev package, or set PHP_CONFIG to one of them)",
                path.display()
            ),
            Self::IncludeDir { path, detail } => write!(
                f,
                "{} --include-dir is unusable ({detail}); install the matching php8.X-dev package",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ToolchainError {}

/// Why one `php-config <flag>` invocation did not produce an answer.
enum RunError {
    /// The kernel never started the process (`fork`/`exec` failed), so the
    /// candidate has not said anything about itself yet.
    Spawn(io::Error),
    /// The process ran and exited non-zero, or printed something unusable.
    Failed(String),
}

/// Maps a spawn failure onto the honest [`ToolchainError`].
///
/// `ENOENT` (the path vanished between `is_file` and `exec`), `EACCES`/`EPERM`
/// (present, not executable), a symlink loop, and `ENOEXEC` (a text file named
/// `php-config8.4`, with no ELF header and no `#!`) are facts about this PATH
/// entry. Every other reason means the kernel refused to start a file this
/// crate believes exists; that is "could not ask", never "not installed".
fn spawn_error(exe: &Path, err: io::Error) -> ToolchainError {
    let not_a_program = matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    ) || matches!(err.raw_os_error(), Some(ENOEXEC | ELOOP));
    if not_a_program {
        ToolchainError::NotExecutable {
            path: exe.to_path_buf(),
        }
    } else {
        ToolchainError::Spawn {
            path: exe.to_path_buf(),
            detail: err.to_string(),
        }
    }
}

/// `ENOEXEC`: the file exists and is executable but is not a program the
/// kernel can run. `std` has no `ErrorKind` for it, and the number is the same
/// on Linux and every other unix this crate builds for.
const ENOEXEC: i32 = 8;

/// `ELOOP`: the path is a symlink loop. `is_file()` already refuses a loop, so
/// this only covers the path changing between the check and `exec`; like
/// `ENOEXEC` it is a fact about the file, not a busy host.
const ELOOP: i32 = 40;

/// Whether a spawn failure is a host hiccup that a short wait can clear.
///
/// `WouldBlock` is `EAGAIN` (`fork` hit the process/thread limit under load)
/// and `ExecutableFileBusy` is `ETXTBSY`: some file description on this inode
/// is still open for writing. The fd table is shared by every thread, so a
/// child another thread forked while the file was open inherits that handle
/// (close-on-exec only takes effect when the child reaches `exec`); the write
/// reference can therefore outlive our own close by one fork window. Neither
/// reason says anything about the program.
fn is_transient_spawn(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::ExecutableFileBusy | io::ErrorKind::Interrupted
    )
}

/// Extra attempts after a transient spawn failure: the first try plus three.
const SPAWN_RETRIES: u32 = 3;

/// How long a transient spawn failure waits before the next attempt: enough
/// for a child forked in that window to reach `exec` and release the handle,
/// and for a short `EAGAIN` burst to pass.
const SPAWN_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(5);

/// Runs `exe <flag>` and returns its stdout, or the reason it failed.
///
/// A transient spawn failure is retried a bounded number of times; anything
/// that survives those attempts is returned as [`RunError::Spawn`], so the
/// caller never has to guess between "busy host" and "not installed".
fn run(exe: &Path, flag: &str) -> Result<String, RunError> {
    let mut retries = SPAWN_RETRIES;
    loop {
        match run_once(exe, flag) {
            Err(RunError::Spawn(err)) if retries > 0 && is_transient_spawn(&err) => {
                retries -= 1;
                std::thread::sleep(SPAWN_RETRY_DELAY);
            }
            other => return other,
        }
    }
}

/// A single attempt at `exe <flag>`; [`run`] owns the retry.
fn run_once(exe: &Path, flag: &str) -> Result<String, RunError> {
    let output = Command::new(exe)
        .arg(flag)
        .output()
        .map_err(RunError::Spawn)?;
    if !output.status.success() {
        let status = match output.status.code() {
            Some(code) => code.to_string(),
            None => "a signal".to_owned(),
        };
        return Err(RunError::Failed(format!("{flag} exited with {status}")));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| RunError::Failed(format!("{flag} returned non-UTF-8 output")))
}
