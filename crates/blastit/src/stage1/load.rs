//! Loading the pinned shim, and everything only an interpreter can answer
//! (design §1.2.1, §2.4).
//!
//! One installation, one order, and the order is the whole value of the slice:
//! `libpython` `RTLD_NOW | RTLD_GLOBAL`, then the shim
//! `RTLD_NOW | RTLD_LOCAL`, then `blastit_py_abi_v1`'s `init_paths`, then the
//! two §7.2 visibility gates, then `preload`. The forks of `http`/`scheduler`
//! happened **before** this, in `boot::run`, so they inherited no
//! half-initialised interpreter.
//!
//! Nothing here names an interpreter: every call is a `blastit_py_abi_v1`
//! entry point, and the refusal text is the shim's own sentence.

use std::os::fd::RawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use blastit_events::{Component, Level, Stream};
use blastit_manifest::UidLayout;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::logs::{Emitter, output_event};
use crate::mem_usage;

use super::adopt::{Immortalised, emit_preload, refresh_memory};

#[cfg(feature = "python")]
use blastit_py_sapi::{LoadedPy, PinnedRef, PyForkHooks, PyZygote};

/// What a successful load leaves stage 1 holding.
pub(super) struct Loaded {
    /// The `dlopen`ed shim (and `libpython`, for `Embed`). The supervisor
    /// borrows a [`blastit_py_sapi::PyRuntime`] from it.
    #[cfg(feature = "python")]
    pub(super) python: LoadedPy,
    /// The preloaded image every view is forked from — never freed (§1.2.1).
    #[cfg(feature = "python")]
    pub(super) zygote: PyZygote,
}

/// Everything the two load paths share: the boot's data and its log.
pub(super) struct Inputs<'a> {
    /// The configuration, already parsed.
    pub(super) config: &'a Config,
    /// One entry per view slot, for §7.2's uid layout.
    pub(super) routes: usize,
    /// init's event stream.
    pub(super) emitter: &'a Emitter,
    /// Whether this is an unprivileged `dev` boot.
    pub(super) dev: bool,
    /// The dev-only loaded-file socket the shim adopts (`-1` for none).
    pub(super) report_fd: RawFd,
    /// §8 Sizing's reading from before the fork.
    pub(super) memory: Option<mem_usage::Usage>,
    /// Whether this image is the first half of a run (§4.6 step 7).
    pub(super) announce: bool,
}

/// The `init_paths` request (design §1.2), as the shim reads it.
#[derive(Debug, Serialize)]
struct InitPathsRequest<'a> {
    sys_path: &'a [Vec<u8>],
    cwd: Option<&'a [u8]>,
    expected_minor: u32,
    app_module: Option<&'a str>,
    site_packages: bool,
    /// The prefix CPython must call home (design 00a72.4): the only path by
    /// which `Config.python.python_home` reaches the interpreter.
    python_home: Option<&'a [u8]>,
}

/// What `init_paths` answers: the two path sets (`version`/`prefix` are the
/// shim's banner, which this crate no longer prints: stage 0 owns the one
/// `runtime` line).
#[derive(Debug, Deserialize)]
struct PathsAnswer {
    #[serde(default)]
    view_paths: Vec<String>,
    #[serde(default)]
    sys_path: Vec<String>,
}

/// The `preload` request (design §1.2).
#[derive(Debug, Serialize)]
struct PreloadRequest<'a> {
    app: Option<&'a str>,
    preload: &'a [String],
    warmup: Option<&'a str>,
    report_fd: i32,
    root: Option<&'a [u8]>,
    immortalize: &'a str,
}

/// What `preload` answers: §9.2 #7's pass.
#[derive(Debug, Deserialize)]
struct PreloadAnswer {
    decision: String,
    objects: u64,
    reason: Option<String>,
    elapsed_ns: u64,
}

/// The two path sets a boot has to answer for (§5.3, §7.2).
struct Paths {
    view_paths: Vec<String>,
    sys_path: Vec<String>,
}

/// `libpython` + the shim, prepare the interpreter, gate the paths and preload.
///
/// `py_fds` are the descriptors stage 1 re-verified after `test_swap_pause`.
///
/// # Errors
///
/// One sentence naming the step: a `dlopen` refusal, the shim's own
/// `init_paths`/`preload` sentence, or a §7.2 visibility gate.
#[cfg(feature = "python")]
pub(super) fn embed(
    inputs: &Inputs<'_>,
    py_fds: &(std::os::fd::OwnedFd, std::os::fd::OwnedFd),
) -> Result<Loaded, String> {
    let handoff = pinned(inputs.config)?;
    let lib_source = source_of(std::os::fd::AsRawFd::as_raw_fd(&py_fds.0));
    let shim_source = source_of(std::os::fd::AsRawFd::as_raw_fd(&py_fds.1));
    let loaded = LoadedPy::load_pinned(
        PinnedRef {
            fd: lib_source.0,
            source: lib_source.1.as_path(),
        },
        PinnedRef {
            fd: shim_source.0,
            source: shim_source.1.as_path(),
        },
        handoff.py_minor,
    )
    .map_err(|err| err.to_string())?;
    let paths = init_paths(&loaded, inputs.config, handoff)?;
    // `R19`: `blastit dev` gives every view the caller's uid, so there is no
    // other identity for the gate to ask about; a deployment runs it.
    if !inputs.dev {
        check_visibility(&paths, inputs.config, inputs.routes, Some(loaded.hooks()))?;
    }
    let preloaded = preload_with(&loaded, inputs)?;
    announce(inputs, &preloaded.outcome);
    Ok(Loaded {
        python: loaded,
        zygote: preloaded.zygote,
    })
}

/// The configuration's pinned runtime, or the sentence a boot without one owes.
#[cfg(feature = "python")]
fn pinned(config: &Config) -> Result<&crate::config::PythonHandoff, String> {
    config.python.as_ref().ok_or_else(|| {
        "this boot has no pinned Python runtime to serve with; restart blastit".to_string()
    })
}

/// The application's module name, for `VIEW_PATHS` (§7.2).
#[cfg(feature = "python")]
fn app_module(config: &Config) -> Option<&str> {
    config
        .app
        .as_deref()
        .and_then(|spec| spec.split_once(':'))
        .map(|(module, _)| module)
}

/// `init_paths`, decoded.
#[cfg(feature = "python")]
fn init_paths(
    loaded: &LoadedPy,
    config: &Config,
    handoff: &crate::config::PythonHandoff,
) -> Result<Paths, String> {
    let request = InitPathsRequest {
        sys_path: &handoff.sys_path,
        cwd: config.cwd.as_deref(),
        expected_minor: handoff.py_minor,
        app_module: app_module(config),
        // An embedded interpreter does not find the distribution's
        // `site-packages` on its own (§9.3).
        site_packages: true,
        python_home: Some(handoff.python_home.as_os_str().as_bytes()),
    };
    let request =
        serde_json::to_vec(&request).map_err(|err| format!("the init_paths request: {err}"))?;
    let answer = loaded
        .init_paths(&request)
        .map_err(|err| crate::py::refusal_message(&err))?;
    let answer: PathsAnswer =
        serde_json::from_slice(&answer).map_err(|err| format!("the init_paths answer: {err}"))?;
    Ok(Paths {
        view_paths: answer.view_paths,
        sys_path: answer.sys_path,
    })
}

/// What one `preload` call answered.
#[cfg(feature = "python")]
struct Preloaded {
    /// The opaque preloaded image (§1.2.1: never freed).
    zygote: PyZygote,
    /// §9.2 #7's pass, as the shim reported it.
    outcome: Immortalised,
}

/// The one `preload` call and its answer.
#[cfg(feature = "python")]
fn preload_with(loaded: &LoadedPy, inputs: &Inputs<'_>) -> Result<Preloaded, String> {
    let config = inputs.config;
    let request = PreloadRequest {
        app: config.app.as_deref(),
        preload: &config.preload,
        warmup: config.warmup.as_deref(),
        report_fd: inputs.report_fd,
        root: config.cwd.as_deref(),
        immortalize: config.immortalize.as_token(),
    };
    let request =
        serde_json::to_vec(&request).map_err(|err| format!("the preload request: {err}"))?;
    let (zygote, answer) = loaded
        .preload(&request)
        .map_err(|err| crate::py::refusal_message(&err))?;
    let answer: PreloadAnswer =
        serde_json::from_slice(&answer).map_err(|err| format!("the preload answer: {err}"))?;
    let outcome = Immortalised {
        on: answer.decision == "on",
        objects: answer.objects,
        reason: answer.reason.clone(),
        elapsed_ns: answer.elapsed_ns,
    };
    warn_if_skipped(inputs.emitter, &outcome);
    Ok(Preloaded { zygote, outcome })
}

/// The one line a first image owes after `preload`: the `boot` event.
///
/// The `runtime` line is **stage 0's** alone (`stage0::emit_runtime`): it is
/// printed once, before the fork, with the version stage 0 resolved. A second
/// line here would be the duplication `docs/reference/log-events.md` forbids.
#[cfg(feature = "python")]
fn announce(inputs: &Inputs<'_>, outcome: &Immortalised) {
    if !inputs.announce {
        return;
    }
    emit_preload(
        inputs.emitter,
        inputs.config,
        outcome,
        refresh_memory(inputs.memory),
        inputs.dev,
    );
}

/// The one `warn` line an §9.2 #7 pass that was decided and skipped owes.
fn warn_if_skipped(emitter: &Emitter, outcome: &Immortalised) {
    if outcome.on {
        return;
    }
    if let Some(reason) = &outcome.reason {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, reason).with_level(Level::Warn),
        );
    }
}

/// §7.2's two gates, over the sets the interpreter itself reported.
///
/// # Errors
///
/// The blocker's own sentence: a path a view could not read, or a `sys.path`
/// entry another uid could write.
#[cfg(feature = "python")]
fn check_visibility(
    paths: &Paths,
    config: &Config,
    routes: usize,
    hooks: Option<PyForkHooks<'_>>,
) -> Result<(), String> {
    let layout = UidLayout::new(config.uid_base);
    let dropped = crate::visibility::DroppedTo::of(layout, routes);
    let readable: Vec<PathBuf> = paths.view_paths.iter().map(PathBuf::from).collect();
    match crate::visibility::unreadable(&readable, &dropped, hooks) {
        Ok(Some(blocker)) => return Err(blocker.message()),
        Ok(None) => {}
        Err(timed_out) => return Err(timed_out.message()),
    }
    let mut entries: Vec<PathBuf> = paths.sys_path.iter().map(PathBuf::from).collect();
    // §5.3: the working directory blastit was started in is a `sys.path` entry
    // like any other — and the place the application's own module comes from.
    if let Some(cwd) = config.cwd.as_deref() {
        let cwd = PathBuf::from(std::ffi::OsString::from_vec(cwd.to_vec()));
        if !entries.contains(&cwd) {
            entries.insert(0, cwd);
        }
    }
    match crate::visibility::writable(&entries, &dropped, hooks) {
        Ok(Some(writable)) => Err(writable.message()),
        Ok(None) => Ok(()),
        Err(timed_out) => Err(timed_out.message()),
    }
}
/// The file a pinned descriptor names, for a refusal sentence (never a load).
#[cfg(feature = "python")]
fn source_of(fd: RawFd) -> (RawFd, PathBuf) {
    let path = std::fs::read_link(Path::new(&format!("/proc/self/fd/{fd}")))
        .unwrap_or_else(|_| PathBuf::from(format!("the pinned Python object (descriptor {fd})")));
    (fd, path)
}
