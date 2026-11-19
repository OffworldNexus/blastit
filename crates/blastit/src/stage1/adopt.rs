#![cfg_attr(not(feature = "python"), allow(unused_imports))]

use std::io::Read;
use std::os::fd::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use blastit_events::{Component, Event, EventKind, Level, Lifecycle, Stream};
#[cfg(feature = "python")]
use blastit_scheduler::derived_view_cap;
#[cfg(feature = "python")]
use pyo3::exceptions::PyValueError;
#[cfg(feature = "python")]
use pyo3::prelude::*;

use crate::config::Config;
use crate::logs::{Emitter, output_event};
use crate::mem_usage;
use crate::py::Python;
#[cfg(feature = "python")]
use blastit_view_python::immortal::{Decision, Outcome};

use super::fail;
#[cfg(feature = "python")]
use super::python_fail;

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
                "version" | "http" | "dbproxy" | "chan" | "learn" | "policy"
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
        Ok(Self {
            http,
            dbproxy,
            chan,
            learn,
            policy,
        })
    }
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
/// §9.2 #7: makes the preloaded heap immortal, when the interpreter's marker is
/// known here.
///
/// The decision is `BLASTIT_IMMORTALIZE` against the running interpreter
/// ([`Decision`]): every interpreter blastit knows gets the
/// marker *it* reads — 3.14 CPython's own split word — and one whose marker
/// blastit does not know is skipped, so the server starts rather than refusing
/// and every view process simply copies more of the preloaded heap (§9.2 #6).
///
/// A skip is one `warn` line saying why; the same reason travels in the
/// preload `boot` event ([`preload_lifecycle`]). A pass that was *decided* and
/// then failed is still fatal: a heap that was not immortalised when it could
/// have been is the copy §9.2 #6 measured.
#[cfg(feature = "python")]
pub(super) fn immortalize(
    py: Python<'_>,
    config: &Config,
    emitter: &Emitter,
    dev: Option<&crate::dev_learn::Dev>,
) -> Outcome {
    let outcome = match blastit_view_python::immortal::apply(py, config.immortalize.setting()) {
        Ok(outcome) => outcome,
        Err(err) => python_fail(py, emitter, dev, "heap immortalisation", &err),
    };
    if let Decision::Off(reason) = &outcome.decision {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, reason).with_level(Level::Warn),
        );
    }
    outcome
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
    outcome: &Outcome,
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
    outcome: &Outcome,
    memory: Option<mem_usage::Usage>,
    dev: bool,
) -> Lifecycle {
    let mut lifecycle = Lifecycle::default()
        .with_stage("preload")
        .with_immortalize(outcome.decision.is_on())
        .with_immortalized(outcome.objects());
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
    if let Some(reason) = outcome.decision.reason() {
        lifecycle = lifecycle.with_immortalize_reason(reason);
    }
    if let Some(millis) = outcome.millis() {
        lifecycle = lifecycle.with_immortalize_ms(millis);
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

/// `module:attribute` as a Python object.
///
/// # Errors
///
/// A [`PyValueError`] when the spec is malformed, and whatever the import or
/// the attribute lookup raises.
#[cfg(feature = "python")]
pub(super) fn import_callable(py: Python<'_>, spec: &str) -> PyResult<Py<PyAny>> {
    let Some((module, attribute)) = spec.split_once(':') else {
        return Err(PyValueError::new_err(format!(
            "`{spec}` is not `module:attribute`"
        )));
    };
    let module = py.import(module)?;
    Ok(module.getattr(attribute)?.unbind())
}
