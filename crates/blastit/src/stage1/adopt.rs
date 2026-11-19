#![cfg_attr(not(feature = "python"), allow(dead_code))]

use std::io::Read;
#[cfg(feature = "python")]
use std::os::fd::{BorrowedFd, OwnedFd};
use std::os::fd::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use blastit_events::{Component, Event, EventKind, Lifecycle};
#[cfg(feature = "python")]
use blastit_scheduler::derived_view_cap;

use crate::config::Config;
use crate::logs::Emitter;
use crate::mem_usage;

use super::fail;

/// Reads the configuration document stage 0 wrote into the pipe (§4), the
/// `entrypoint php` probe timeout that travelled with it (`ADR-0010` §3), and —
/// in a debug build — the test-only swap point (`MB.3.64`).
///
/// The timeout is a **stage-0 decision**: `BLASTIT_PROBE_TIMEOUT_MS` is read
/// before the `execve` that swaps this process's environment, because
/// `blastit-env` strips every `BLASTIT_*` variable (`R12`), so stage 1 finds it
/// in the document and nowhere else. The swap point crosses the same barrier for
/// the same reason, and only a `debug_assertions` build has the member at all.
/// What stage 0 handed this image on the configuration pipe.
#[derive(Debug)]
pub(super) struct BootConfig {
    /// The parsed configuration.
    pub(super) config: Config,
    /// The `entrypoint php` probe's patience (`ADR-0010` §3).
    pub(super) probe_timeout: Duration,
    /// The debug-only `BLASTIT_TEST_SWAP_POINT` rendezvous (`MB.3.64`).
    pub(super) swap_point: Option<PathBuf>,
    /// The exact document bytes, re-supplied through a fresh pipe by a reload
    /// cycle (§4.6 step 6).
    pub(super) bytes: Vec<u8>,
    /// Stage 0's stage-1 program prefix, for the cycle.
    pub(super) restart: Option<Vec<String>>,
    /// The debug-only reload fault seam (`MB.23.5.2.2`).
    pub(super) reload_fault: Option<String>,
}

pub(super) fn read_config(
    emitter: &Emitter,
    dev: Option<&crate::dev_learn::Dev>,
    fd: RawFd,
) -> BootConfig {
    // SAFETY: stage 0 cleared `FD_CLOEXEC` on this descriptor and stage 1 is its
    // only owner.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut bytes = Vec::new();
    if let Err(err) = file.read_to_end(&mut bytes) {
        fail(emitter, dev, &format!("configuration pipe: {err}"));
    }
    drop(file);
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => fail(emitter, dev, &format!("configuration document: {err}")),
    };
    let probe_timeout = value
        .get(crate::stage0::PROBE_TIMEOUT_MEMBER)
        .and_then(serde_json::Value::as_u64)
        .map_or(crate::stage0::PROBE_TIMEOUT_DEFAULT, Duration::from_millis);
    #[cfg(debug_assertions)]
    let swap_point = value
        .get(crate::stage0::TEST_SWAP_POINT_MEMBER)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    #[cfg(not(debug_assertions))]
    let swap_point: Option<PathBuf> = None;
    // Stage 0's handover of the stage-1 program prefix (§4.6 step 6). Kept in
    // the configuration document, never in the environment: a `BLASTIT_*`
    // variable would be inherited by views, and this token must not be.
    let restart = value
        .get(crate::stage0::RESTART_MEMBER)
        .and_then(serde_json::Value::as_array)
        .map(|argv| {
            argv.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<String>>()
        })
        .filter(|argv| !argv.is_empty());
    // `MB.23.5.2.2`: the debug-only reload fault seam, read back where the swap
    // point is. A release build has neither the member's name nor this read.
    #[cfg(debug_assertions)]
    let reload_fault = value
        .get(crate::stage0::TEST_RELOAD_FAULT_MEMBER)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    #[cfg(not(debug_assertions))]
    let reload_fault: Option<String> = None;
    match Config::from_json(&value) {
        Ok(config) => BootConfig {
            config,
            probe_timeout,
            swap_point,
            bytes,
            restart,
            reload_fault,
        },
        Err(err) => fail(emitter, dev, &err.to_string()),
    }
}

/// One component a reload's replacement image adopts instead of forking: its
/// pid and the read ends of its three pipes (§4.6 step 6).
#[derive(Debug, Clone, Copy)]
pub(crate) struct AdoptedComponent {
    pub(crate) pid: i32,
    pub(crate) stdout: RawFd,
    pub(crate) stderr: RawFd,
    pub(crate) events: RawFd,
}

/// The adoption document a reload's replacement image is started with.
///
/// UTF-8 JSON, at most 4096 bytes. `learn` is present exactly when the first
/// boot had `--learn`; `policy` exactly when it had a dev policy socket. An
/// unknown key, a missing key or a descriptor this image does not hold refuses
/// the boot: an adoption that half-succeeds would bind nothing and fork `http`
/// twice.
#[derive(Debug)]
pub(crate) struct Adoption {
    pub(crate) http: AdoptedComponent,
    pub(crate) dbproxy: Option<AdoptedComponent>,
    pub(crate) chan: RawFd,
    pub(crate) learn: Option<(RawFd, Option<RawFd>)>,
    pub(crate) policy: Option<RawFd>,
    /// The `(libpython, shim)` descriptors the previous image held across
    /// `exec_cycle` (design §1.2.1 item 6). Numbers only: the `OwnedFd`s are
    /// `supervise::Adoption`'s on the other side of the write.
    pub(crate) py_fds: Option<(RawFd, RawFd)>,
}

impl Adoption {
    /// Reads and validates the document on `fd`.
    ///
    /// # Errors
    ///
    /// One sentence for the boot: a size, a JSON, a shape or an open-descriptor
    /// refusal.
    pub(crate) fn read(fd: RawFd) -> Result<Self, String> {
        // SAFETY: `exec_cycle` cleared `FD_CLOEXEC` on exactly this descriptor
        // and handed it over; this image is its only owner.
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|err| format!("adoption document: {err}"))?;
        drop(file);
        if bytes.len() > MAX_ADOPTION_DOCUMENT {
            return Err(format!(
                "the adoption document is {} bytes, more than the {MAX_ADOPTION_DOCUMENT}-byte limit",
                bytes.len()
            ));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|err| format!("adoption document: {err}"))?;
        let object = value
            .as_object()
            .ok_or_else(|| "adoption document: the document is not an object".to_string())?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "version" | "http" | "dbproxy" | "chan" | "learn" | "policy" | "py_lib" | "py_shim"
            ) {
                return Err(format!("adoption document: unknown key `{key}`"));
            }
        }
        if object.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err("adoption document: `version` must be 1".to_string());
        }
        let component = |key: &str| -> Result<AdoptedComponent, String> {
            let value = object
                .get(key)
                .ok_or_else(|| format!("adoption document: missing `{key}`"))?;
            let component = AdoptedComponent {
                pid: member(value, key, "pid")?,
                stdout: member(value, key, "stdout")?,
                stderr: member(value, key, "stderr")?,
                events: member(value, key, "events")?,
            };
            for fd in [component.stdout, component.stderr, component.events] {
                open(fd).map_err(|err| format!("adoption document: `{key}`: {err}"))?;
            }
            Ok(component)
        };
        let http = component("http")?;
        let dbproxy = match object.contains_key("dbproxy") {
            true => Some(component("dbproxy")?),
            false => None,
        };
        let chan: RawFd = object
            .get("chan")
            .ok_or_else(|| "adoption document: missing `chan`".to_string())
            .and_then(|value| number(value, "chan"))?;
        open(chan).map_err(|err| format!("adoption document: `chan`: {err}"))?;
        let learn = match object.get("learn") {
            Some(value) => {
                let obs = member(value, "learn", "obs")?;
                open(obs).map_err(|err| format!("adoption document: `learn.obs`: {err}"))?;
                let ctl = match value.get("ctl") {
                    Some(ctl) => {
                        let ctl = number(ctl, "learn.ctl")?;
                        open(ctl)
                            .map_err(|err| format!("adoption document: `learn.ctl`: {err}"))?;
                        Some(ctl)
                    }
                    None => None,
                };
                Some((obs, ctl))
            }
            None => None,
        };
        let policy = match object.get("policy") {
            Some(value) => {
                let fd = number(value, "policy")?;
                open(fd).map_err(|err| format!("adoption document: `policy`: {err}"))?;
                Some(fd)
            }
            None => None,
        };
        let py_fds = match (object.get("py_lib"), object.get("py_shim")) {
            (None, None) => None,
            (Some(lib), Some(shim)) => {
                let lib = number(lib, "py_lib")?;
                let shim = number(shim, "py_shim")?;
                open(lib).map_err(|err| format!("adoption document: `py_lib`: {err}"))?;
                open(shim).map_err(|err| format!("adoption document: `py_shim`: {err}"))?;
                Some((lib, shim))
            }
            _ => {
                return Err("adoption document: `py_lib` and `py_shim` come together".to_string());
            }
        };
        Ok(Self {
            http,
            dbproxy,
            chan,
            learn,
            policy,
            py_fds,
        })
    }
}

/// The `(libpython, shim)` descriptors this image adopts, re-verified.
///
/// Design §1.2.1 item 6: both the initial handoff (`--py-lib-fd`/
/// `--py-shim-fd`) and a replacement boot (`Adoption.py_fds`) are `OwnedFd`s
/// here, `fstat`ed against [`Config::python`] and given `FD_CLOEXEC` back
/// **immediately**, before TCP bind, the forks or any load. The single owner is
/// the pair this returns: `stage1::Adoption` keeps numbers only, and
/// `supervise::Adoption` takes these `OwnedFd`s when a cycle may re-exec.
///
/// A mismatched `(st_dev, st_ino)` is a hard refusal with the prose sentence a
/// replaced object has always printed.
#[cfg(feature = "python")]
pub(super) fn adopt_py_fds(
    config: &Config,
    adopted: Option<&Adoption>,
    lib: Option<RawFd>,
    shim: Option<RawFd>,
) -> Result<Option<(OwnedFd, OwnedFd)>, String> {
    let pair = match (adopted.and_then(|adoption| adoption.py_fds), lib, shim) {
        (Some(pair), _, _) => pair,
        (None, Some(lib), Some(shim)) => (lib, shim),
        (None, None, None) => {
            // Fail closed: a document that pins a runtime without a descriptor
            // would otherwise serve with nothing verified.
            if config.python.is_some() {
                return Err(
                    "the configuration pins a Python runtime but no descriptor came with it; \
                     restart blastit"
                        .to_string(),
                );
            }
            return Ok(None);
        }
        (None, ..) => {
            return Err("`--py-lib-fd` and `--py-shim-fd` must be given together".to_string());
        }
    };
    let Some(record) = &config.python else {
        return Err(
            "this boot was handed a pinned Python descriptor, but has no record to verify it \
             against; restart blastit"
                .to_string(),
        );
    };
    let lib = verify_py_fd(pair.0, record.lib, "libpython")?;
    let shim = verify_py_fd(pair.1, record.shim, "CPython shim")?;
    Ok(Some((lib, shim)))
}

/// One descriptor: the number stage 0 recorded, still open, same inode, and
/// `FD_CLOEXEC` again.
#[cfg(feature = "python")]
fn verify_py_fd(fd: RawFd, expected: (RawFd, u64, u64), what: &str) -> Result<OwnedFd, String> {
    if fd != expected.0 {
        return Err(format!(
            "the pinned {what} descriptor does not match the one this boot recorded (expected \
             {}, found {fd}); restart blastit",
            expected.0
        ));
    }
    // SAFETY: `fd` is a number this image inherited from its stage-0 `execve`;
    // the borrow is short and only reads the descriptor's flags.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    open(fd)
        .map_err(|_| format!("the pinned {what} descriptor is no longer open; restart blastit"))?;
    let stat = rustix::fs::fstat(borrowed)
        .map_err(|err| format!("the pinned {what} descriptor: {err}; restart blastit"))?;
    if stat.st_dev != expected.1 || stat.st_ino != expected.2 {
        let path = std::fs::read_link(format!("/proc/self/fd/{fd}"))
            .unwrap_or_else(|_| PathBuf::from(format!("the {what}")));
        return Err(format!(
            "{} was replaced after blastit inspected it (inode {} is now {}); restore the file \
             and restart blastit",
            path.display(),
            expected.2,
            stat.st_ino
        ));
    }
    if let Err(err) = blastit_sys::set_cloexec(borrowed, true) {
        return Err(format!(
            "the pinned {what} descriptor: {err}; restart blastit"
        ));
    }
    // SAFETY: the descriptor is open, this image owns it, and this is the one
    // `OwnedFd` that will close it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// One integer member of a component object.
pub(super) fn member(value: &serde_json::Value, key: &str, name: &str) -> Result<RawFd, String> {
    value
        .get(name)
        .ok_or_else(|| format!("adoption document: `{key}` has no `{name}`"))
        .and_then(|value| number(value, name))
}

/// One descriptor number from a JSON value.
pub(super) fn number(value: &serde_json::Value, name: &str) -> Result<RawFd, String> {
    let raw = value
        .as_u64()
        .filter(|raw| i32::try_from(*raw).is_ok())
        .ok_or_else(|| format!("adoption document: `{name}` is not a descriptor number"))?;
    i32::try_from(raw).map_err(|_| format!("adoption document: `{name}` is not a descriptor"))
}

/// Whether this image really holds `fd`.
pub(super) fn open(fd: RawFd) -> Result<(), String> {
    // SAFETY: `F_GETFD` reads descriptor flags and takes no pointer.
    match unsafe { libc::fcntl(fd, libc::F_GETFD) } {
        -1 => Err(format!("descriptor {fd} is not open")),
        _ => Ok(()),
    }
}

/// The stage-1 program prefix a reload cycle re-execs (§4.6 step 6).
///
/// Stage 0's handover when the configuration carried one, else
/// `/proc/self/cmdline` up to the first `--stage=1` token — never
/// `/proc/self/exe`, which for a wheel is the interpreter. At most eight
/// tokens, and never an empty prefix.
#[cfg(feature = "python")]
pub(super) fn stage1_program(restart: Option<&[String]>) -> Result<Vec<std::ffi::CString>, String> {
    let argv: Vec<String> = match restart {
        Some(argv) => argv.to_vec(),
        None => {
            let raw = std::fs::read("/proc/self/cmdline")
                .map_err(|err| format!("reload: reading /proc/self/cmdline: {err}"))?;
            raw.split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .take_while(|token| !token.starts_with("--stage=1"))
                .collect()
        }
    };
    if argv.is_empty() || argv.len() > 8 {
        return Err(format!(
            "reload: the stage-1 program prefix has {} token(s), not 1..=8",
            argv.len()
        ));
    }
    argv.iter()
        .map(|token| {
            std::ffi::CString::new(token.as_str())
                .map_err(|_| "reload: the stage-1 program prefix contains a NUL byte".to_string())
        })
        .collect()
}

/// The `--dev-root` this image was started with, from its own argv.
#[cfg(feature = "python")]
///
/// The runtime root is stage 0's, and `crate::dev_learn::Dev` keeps it private;
/// the replacement image needs the same absolute path to re-exec itself with.
pub(super) fn dev_root_from_cmdline() -> Result<std::path::PathBuf, String> {
    let raw = std::fs::read("/proc/self/cmdline")
        .map_err(|err| format!("reload: reading /proc/self/cmdline: {err}"))?;
    raw.split(|byte| *byte == 0)
        .filter_map(|part| std::str::from_utf8(part).ok())
        .find_map(|token| token.strip_prefix("--dev-root="))
        .filter(|root| !root.is_empty())
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "reload: this image has no `--dev-root` in its argv".to_string())
}

/// The adoption document's size bound (§4.6 step 6).
pub(super) const MAX_ADOPTION_DOCUMENT: usize = 4096;

/// `MB.3.64`: pauses stage 1 at the test-only rendezvous, when — and only when —
/// a `debug_assertions` build with the `php` capability was handed one.
///
/// Stage 0 read `BLASTIT_TEST_SWAP_POINT` before the barrier and carried the
/// path in the configuration document; the wait itself
/// ([`blastit_view_php::pinning::await_test_swap`]) sits between stage 0's inode record
/// and the re-open `boot_bindings` performs, so a rename the test makes while
/// the boot is paused is *in* the window by construction.
///
/// A failure to create the rendezvous, or to be released from it, is this boot's
/// refusal through [`fail`] (`exit 1`, before any listener) — never a hang.
#[cfg(all(debug_assertions, feature = "php"))]
pub(super) fn test_swap_pause(
    emitter: &Emitter,
    dev: Option<&crate::dev_learn::Dev>,
    point: Option<&Path>,
) {
    if let Some(point) = point
        && let Err(message) = blastit_view_php::pinning::await_test_swap(point)
    {
        fail(emitter, dev, &message);
    }
}

/// The other two configurations — a release build, or one without the `php`
/// capability — have no seam to pause at: their whole body is this.
#[cfg(not(all(debug_assertions, feature = "php")))]
pub(super) fn test_swap_pause(
    _emitter: &Emitter,
    _dev: Option<&crate::dev_learn::Dev>,
    _point: Option<&Path>,
) {
}
/// §9.2 #7's pass as the shim reported it through `preload`.
///
/// The pass itself runs inside `blastit_py_abi_v1`'s `preload` (design §1.2);
/// this crate only carries the numbers to the boot line. `on` is
/// `"on"`/`"off"`, `objects` how many allocations carry the marker, `reason`
/// why the pass was skipped, and `elapsed_ns` what it cost (0 when skipped).
#[cfg(feature = "python")]
#[derive(Debug, Clone, Default)]
pub(super) struct Immortalised {
    /// Whether the pass ran.
    pub(super) on: bool,
    /// How many objects carry the marker afterwards.
    pub(super) objects: u64,
    /// Why the pass did not run, when it did not.
    pub(super) reason: Option<String>,
    /// How long the pass took, in nanoseconds.
    pub(super) elapsed_ns: u64,
}

/// The reading §8 Sizing's `boot` event reports, taken now that the app is
/// preloaded.
///
/// `before` is the reading from before the fork, and it is also the *proof*
/// that memory can be read: when it is `None` the machine has already said so
/// at `warn`, and re-reading here would say it twice. When the second reading
/// fails but the first succeeded, `before` is still a usable figure.
#[cfg(feature = "python")]
pub(super) fn refresh_memory(before: Option<mem_usage::Usage>) -> Option<mem_usage::Usage> {
    let before = before?;
    Some(mem_usage::read(Path::new("/")).unwrap_or(before))
}

/// §8 Sizing and §9.2 #7: what the preload cost, and the view process cap the
/// memory that is left buys.
///
/// This runs after the app was imported and the heap immortalised — or left
/// alone on purpose — so `mem_used` is the number §8 asks for ("usage after
/// preload") and the derived cap is the one the scheduler measures for itself,
/// from the same function.
#[cfg(feature = "python")]
pub(super) fn emit_preload(
    emitter: &Emitter,
    config: &Config,
    outcome: &Immortalised,
    memory: Option<mem_usage::Usage>,
    dev: bool,
) {
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Boot(preload_lifecycle(config, outcome, memory, dev)),
    ));
}

/// The `boot` payload of the preload phase (§8.1).
///
/// `immortalize` says whether §9.2 #7's pass ran, and `immortalize_reason` why
/// it did not; `immortalized` and `immortalize_ms` describe the pass itself
/// and are therefore a zero and a nothing when there was none. Apart from the
/// emitting, so that a test can read the fields back without a log.
#[cfg(feature = "python")]
pub(super) fn preload_lifecycle(
    config: &Config,
    outcome: &Immortalised,
    memory: Option<mem_usage::Usage>,
    dev: bool,
) -> Lifecycle {
    let mut lifecycle = Lifecycle::default()
        .with_stage("preload")
        .with_immortalize(outcome.on)
        .with_immortalized(outcome.objects);
    // `R22`, `R23`: adopting a `dev` root stamps `"unprivileged-dev"` (and the
    // binary's build) in stage 1 itself, even if `--stage=1 --dev-root=…` was
    // started without stage 0. Otherwise D6's stage-0 `Privilege` supplies both.
    if dev {
        lifecycle = lifecycle
            .with_build(env!("CARGO_PKG_VERSION"))
            .with_privilege(crate::dev_learn::PRIVILEGE);
    } else if let Some(privilege) = &config.privilege {
        lifecycle = privilege.lifecycle(lifecycle);
    }
    // E1: and the preset, for the same reason — a `boot` line in the middle of a
    // deployment's log has to be readable on its own.
    if let Some(choice) = &config.preset {
        lifecycle = choice.lifecycle(lifecycle);
    }
    if let Some(reason) = &outcome.reason {
        lifecycle = lifecycle.with_immortalize_reason(reason.clone());
    }
    if outcome.elapsed_ns > 0 {
        lifecycle = lifecycle.with_immortalize_ms(outcome.elapsed_ns as f64 / 1_000_000.0);
    }
    if let Some(usage) = memory {
        lifecycle = lifecycle
            .with_mem_total(usage.total())
            .with_mem_used(usage.used())
            .with_max_view_processes(derived_view_cap(
                usage.total(),
                usage.used(),
                config.view_mem_estimate,
            ));
    }
    lifecycle
}

#[cfg(all(test, feature = "python"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::os::fd::{AsRawFd, IntoRawFd};
    use std::os::unix::fs::MetadataExt;

    use super::*;

    /// A directory under the workspace's `target/tmp`, removed when dropped.
    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/adopt-py");
            let dir = root.join(format!("{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("the fixture directory");
            Self { dir }
        }

        /// A file opened for reading, with the identity stage 0 would record.
        fn pinned(&self, name: &str) -> (std::fs::File, (u64, u64)) {
            let path = self.dir.join(name);
            std::fs::write(&path, b"a pinned shared object").expect("the object");
            let file = std::fs::File::open(&path).expect("the opened object");
            let metadata = file.metadata().expect("its metadata");
            let identity = (metadata.dev(), metadata.ino());
            (file, identity)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A config whose `python` record names `lib` and `shim`.
    fn config_with(lib: (RawFd, u64, u64), shim: (RawFd, u64, u64)) -> Config {
        Config {
            python: Some(crate::config::PythonHandoff {
                py_minor: 314,
                python_home: PathBuf::from("/opt/python/3.14"),
                sys_path: Vec::new(),
                lib,
                shim,
            }),
            ..Config::default()
        }
    }

    #[test]
    fn a_replaced_libpython_descriptor_is_refused_by_identity() {
        let fixture = Fixture::new("swap");
        // The object stage 0 inspected, and a real second object (a rename over
        // the first, reopened) whose inode is what the descriptor now names.
        let (inspected, (dev, ino)) = fixture.pinned("libpython3.14.so.1.0");
        let side = fixture.dir.join("replacement");
        std::fs::write(&side, b"something else").expect("the replacement");
        std::fs::rename(&side, fixture.dir.join("libpython3.14.so.1.0"))
            .expect("renaming the replacement over the object");
        let replaced = std::fs::File::open(fixture.dir.join("libpython3.14.so.1.0"))
            .expect("the replacement is open");
        let fd = replaced.as_raw_fd();
        drop(inspected);

        let refusal =
            verify_py_fd(fd, (fd, dev, ino), "libpython").expect_err("a moved inode is refused");
        assert!(
            refusal.contains("was replaced after blastit inspected it"),
            "{refusal}"
        );
        assert!(
            refusal.contains("restore the file and restart blastit"),
            "{refusal}"
        );
        assert!(
            refusal.contains(&format!("inode {ino} is now")),
            "the sentence names both identities: {refusal}"
        );
        assert!(!refusal.contains("stage 0"), "{refusal}");
    }

    #[test]
    fn an_adoption_pair_is_reverified_and_cloexec_is_restored() {
        let fixture = Fixture::new("adoption");
        let (lib, (lib_dev, lib_ino)) = fixture.pinned("libpython3.14.so.1.0");
        let (shim, (shim_dev, shim_ino)) = fixture.pinned("libblastit_py314-x86_64-gnu.so");
        // `into_raw_fd` hands ownership to `adopt_py_fds`; a `File` left alive
        // beside it would close the same descriptor twice.
        let (lib_fd, shim_fd) = (lib.into_raw_fd(), shim.into_raw_fd());
        let config = config_with((lib_fd, lib_dev, lib_ino), (shim_fd, shim_dev, shim_ino));
        let adoption = Adoption {
            http: AdoptedComponent {
                pid: 0,
                stdout: -1,
                stderr: -1,
                events: -1,
            },
            dbproxy: None,
            chan: -1,
            learn: None,
            policy: None,
            py_fds: Some((lib_fd, shim_fd)),
        };

        let adopted = adopt_py_fds(&config, Some(&adoption), None, None)
            .expect("the recorded pair is accepted")
            .expect("the pair is present");
        assert_eq!(adopted.0.as_raw_fd(), lib_fd);
        assert_eq!(adopted.1.as_raw_fd(), shim_fd);
        for fd in [lib_fd, shim_fd] {
            // SAFETY: `F_GETFD` reads the descriptor flags and takes no pointer.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_ne!(
                flags & libc::FD_CLOEXEC,
                0,
                "descriptor {fd} must not survive another execve"
            );
        }
        drop(adopted);
    }

    #[test]
    fn a_pinned_pair_without_a_record_is_refused() {
        let fixture = Fixture::new("record");
        let (file, _identity) = fixture.pinned("libpython3.14.so.1.0");
        let fd = file.into_raw_fd();
        let refusal = adopt_py_fds(&Config::default(), None, Some(fd), Some(fd))
            .expect_err("a pair with no record cannot be verified");
        assert_eq!(
            refusal,
            "this boot was handed a pinned Python descriptor, but has no record to verify it \
             against; restart blastit"
        );
    }

    #[test]
    fn a_record_without_a_descriptor_is_refused() {
        let fixture = Fixture::new("missing");
        let (lib, (lib_dev, lib_ino)) = fixture.pinned("libpython3.14.so.1.0");
        let (shim, (shim_dev, shim_ino)) = fixture.pinned("libblastit_py314-x86_64-gnu.so");
        let config = config_with(
            (lib.as_raw_fd(), lib_dev, lib_ino),
            (shim.as_raw_fd(), shim_dev, shim_ino),
        );
        let refusal = adopt_py_fds(&config, None, None, None)
            .expect_err("a pinned runtime needs its descriptors");
        assert_eq!(
            refusal,
            "the configuration pins a Python runtime but no descriptor came with it; restart \
             blastit"
        );
    }
}
