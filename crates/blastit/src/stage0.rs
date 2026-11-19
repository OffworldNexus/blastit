//! Stage 0: the privileged half of the boot (§4).
//!
//! ```text
//! boot → config → manifest → env swap → dirs + pg listeners
//!      → fork dbproxy → secrets.pipe → ready
//!      → execve barrier
//! ```
//!
//! Everything secret lives in this stage and is gone when it ends: the
//! platform's real credentials are read here, handed to dbproxy over a pipe, and
//! then the address space is replaced with the app environment (§4). Two rules
//! hold at every step: no secret is ever in `argv` or `environ`, and every
//! `fork` happens while init is single-threaded.
//!
//! One deviation from §4 is forced by dbproxy's own contract: when the manifest
//! grants nothing on any `postgresql` proxy there is no listener to serve and
//! `blastit_dbproxy::run_with_listeners` refuses to start without one. dbproxy is
//! therefore forked exactly when there is at least one listener, and not
//! otherwise.

use std::collections::BTreeMap;
use std::ffi::{CString, OsStr};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

use blastit_dbproxy::{DbproxyConfig, ListenerSpec, ProxySpec, write_config};
use blastit_events::{
    Component, Event, EventKind, Level, Lifecycle, ManifestWarning as WarningEvent, Runtime, Stream,
};
use blastit_manifest::{Manifest, PgGrant, ProxyName, UidLayout};
use blastit_sys::{
    Fork, Pid, Rlimits, Signal, bind_unix_listener, current_signal_mask, exec, harden_child,
    make_private_dir, pipe, restore_signal_mask_in_child, set_cloexec, set_dumpable,
};

use crate::child::{ChildIo, Deadline, read_one, wait_readable};
use crate::cli::DevOptions;
use crate::config::{Config, LogFormat, Mode, Privilege, view_dir};
use crate::dev_learn::{DevRoot, PRIVILEGE};
// The two provenance types are the deployment gate's (`php` only): the rest of
// stage 0 names neither.
#[cfg(feature = "php")]
use crate::host::{Claim, Provenance};
use crate::host::{EXEC, Host, Install, Placement, Reexec};
use crate::logs::{Emitter, fail, output_event};
use crate::mem_usage;
// The `entrypoint php` discovery gate (`MB.36.5`): a stage-0 dependency that
// names no SAPI type and no `dlopen`, so `init` can answer "can this host embed
// PHP?" without loading anything (`ADR-0010` §3, design §4.4 D5).
#[cfg(feature = "php")]
use blastit_view_php::discovery::{Deployment, PhpBoot, PhpSearch, PhpShimScope, php_boot_in};
// The discovery surface the `php_boot_gate` cases drive: their subjects are the
// crate's, so the names are the tests' only.
#[cfg(feature = "python")]
use crate::py::Python;
#[cfg(all(test, feature = "php"))]
use blastit_view_php::discovery::{PhpVersion, discover_php, shim_name};

/// How long stage 0 waits for dbproxy's ready byte before giving up (§4).
pub const DBPROXY_READY_TIMEOUT: Duration = Duration::from_secs(30);

/// The uid init is running as (D6).
///
/// The effective uid, which is the one a `setuid` from it would have to be
/// authorised for.
fn current_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Where the events made before the log settings are read wait (§8.1).
///
/// The format is one of the *settings*, and stage 0's first lines are made
/// before those settings exist: the capability sets, the placement, the sibling
/// warning, and the `boot` event that reports them. They are kept here, in the
/// order they were made, and written out once the configuration that says how
/// lines are printed has been parsed — so `BLASTIT_LOG_FORMAT=pretty` gives a
/// human first line like every other.
#[derive(Default)]
struct Early {
    events: Vec<Event>,
}

impl Early {
    /// Keeps `event` for later, in order.
    fn emit(&mut self, event: Event) {
        self.events.push(event);
    }

    /// Writes everything that waited into `emitter`, and empties the queue.
    fn flush(&mut self, emitter: &Emitter) {
        for event in self.events.drain(..) {
            emitter.emit(&event);
        }
    }
}

/// Fails a boot that has not resolved its configuration yet.
///
/// §8.1: the lines that waited are written as JSON — the format that needs no
/// settings, and the one an operator reading a failed boot can always parse —
/// and the error follows them.
fn fail_early(early: &mut Early, message: &str) -> ! {
    let json = Emitter::stdout(LogFormat::Json, Level::Info);
    early.flush(&json);
    fail(&json, message)
}

/// The `boot` event of stage 0 (§8.1), with D6's mode when it is known.
fn stage0_lifecycle(privilege: Option<&Privilege>) -> Lifecycle {
    let lifecycle = Lifecycle::default()
        .with_stage("stage0")
        .with_seccomp(crate::config::seccomp_requested());
    match privilege {
        Some(privilege) => privilege.lifecycle(lifecycle),
        None => lifecycle,
    }
}

/// The `boot` event of the phase that parsed the configuration (§8.1, E1).
///
/// The first `boot` line is printed before anything can fail, so it knows
/// nothing of the configuration — not the mode, not the preset. This one is the
/// line that says what the boot *is*: which privilege mode init detected, what
/// the §7.3 filters are set to, and which preset supplied which defaults. It
/// comes before the manifest is read, so a boot that fails later still says
/// which preset it was failing with.
fn config_lifecycle(privilege: &Privilege, config: &Config) -> Lifecycle {
    let lifecycle = privilege.lifecycle(
        Lifecycle::default()
            .with_stage("config")
            .with_seccomp(config.seccomp),
    );
    match &config.preset {
        Some(choice) => choice.lifecycle(lifecycle),
        None => lifecycle,
    }
}

/// Warns when another process already runs as init's uid (D6).
///
/// The file that may carry the capabilities is mode `0500`, owned by the uid
/// init runs as, so that uid is the *only* one that can run it — and running it
/// is becoming init (`setuid(0)`, every sandbox opened). A worker, a migration
/// or an operator's shell that shares that uid therefore inherits init's whole
/// authority, and no amount of checking inside blastit can undo that: it is a
/// deployment decision. This is the one place blastit can see it, at boot, when
/// nothing of its own is running yet but somebody else may be.
///
/// It is a warning and not a refusal: a container legitimately has other
/// processes of that uid (`docker exec` for a debug shell, a sidecar that drops
/// privileges, this very check's own shell), and blastit cannot tell a debug
/// shell from a threat.
fn warn_about_siblings(early: &mut Early) {
    let uid = current_uid();
    let siblings = others_with_uid(uid);
    if siblings.is_empty() {
        return;
    }
    early.emit(
        output_event(
            Component::Init,
            None,
            Stream::Stderr,
            &format!(
                "{} other process(es) run as uid {uid} ({}): that uid can run the file \
                 blastit's capabilities are on, so a worker, a migration or a shell \
                 should run as another uid (D6)",
                siblings.len(),
                siblings
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
        .with_level(Level::Warn),
    );
}

/// The pids, other than this process, that run as `uid` (D6).
///
/// Pure except for `/proc`: reading it is best effort — a pid that disappears
/// between the listing and the read is not a failure — and a `/proc` that cannot
/// be read at all reports nothing rather than failing a boot over a warning.
fn others_with_uid(uid: u32) -> Vec<i32> {
    let ours = std::process::id() as i32;
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids: Vec<i32> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<i32>().ok())
        .filter(|pid| *pid != ours)
        .filter(|pid| effective_uid(*pid) == Some(uid))
        .collect();
    pids.sort_unstable();
    pids
}

/// The effective uid of `pid`, from `/proc/<pid>/status` (D6).
///
/// `None` when the process is gone, or when its status cannot be read: a warning
/// is not worth failing a boot over.
fn effective_uid(pid: i32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}
fn privilege_from(held: &blastit_sys::Held) -> Privilege {
    Privilege {
        mode: match held.mode {
            blastit_sys::PrivilegeMode::Root => Mode::Root,
            blastit_sys::PrivilegeMode::Capabilities => Mode::Capabilities,
        },
        capabilities: match held.mode {
            // A rootless init is exactly what its permitted set says.
            blastit_sys::PrivilegeMode::Capabilities => held
                .capabilities
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            // Root's own set is the container's: §10 documents it, and the
            // event does not pretend to list it.
            blastit_sys::PrivilegeMode::Root => Vec::new(),
        },
        bounding_set_dropped: held.bounding_set_dropped,
    }
}

/// The sentence a boot that lacks capabilities fails with (D6/B1, B2).
///
/// [`blastit_sys::caps::Missing`] only states the fact (the uid and the missing
/// names), because *which file* should have been given them depends on the
/// installation — and this is the only place that knows it. Root is never told
/// to "start as root": it already is, and its capabilities come from the
/// container.
fn capability_advice(host: &Host, err: &std::io::Error) -> String {
    let install = host.install();
    let Some(blastit_sys::Error::MissingCapabilities { uid, missing }) =
        blastit_sys::Error::from_io(err)
    else {
        return err.to_string();
    };
    if *uid == 0 {
        return format!(
            "this boot runs as uid 0 and the container's effective capability set is \
             missing {missing}: uid 0 needs them as much as any other uid (they are what \
             `chown`s the view directories, `open`s them and `kill`s a view). Docker's \
             default capabilities include them — check `--cap-drop`, `--cap-add` and any \
             bounding-set restriction on the container.\n  \
             A rootless deployment gives them to a *dedicated copy* of the {} instead:\n  \
             {}\n  {}\n  \
             To run the same tree locally without root, use `blastit dev`; it is a \
             single-uid simulation, not a sandbox",
            install.file(),
            copy_recipe(install),
            install.naming()
        );
    }
    format!(
        "init runs as uid {uid} and is missing {missing}.\n  \
         A rootless deployment gives the capabilities to a *dedicated copy* of the {}, \
         never to the shared one:\n  {}\n  {}\n  \
         To run the same tree locally without root, use `blastit dev`; it is a \
         single-uid simulation, not a sandbox",
        install.file(),
        copy_recipe(install),
        install.naming()
    )
}

/// The four commands that make the capable copy, in this installation's words
/// (D6/B1, B2).
fn copy_recipe(install: Install) -> String {
    match install {
        Install::Wheel => format!(
            "cp \"$(readlink -f <the interpreter>)\" <same directory>/<interpreter>-init\n  \
             chown <the uid init runs as>:<its gid> <the copy>\n  chmod 0500 <the copy>\n  \
             setcap '{}+ep' <the copy>\n  \
             then start `<the copy> -m blastit` (the copy stays next to the original: \
             CPython finds its stdlib and its venv from the executable's own path)",
            blastit_sys::caps::Capability::SETCAP
        ),
        Install::Binary => format!(
            "cp <the blastit binary> <the binary>-init\n  \
             chown <the uid init runs as>:<its gid> <the copy>\n  chmod 0500 <the copy>\n  \
             setcap '{}+ep' <the copy>\n  \
             then start <the copy>, and set {EXEC}=<the blastit binary> so that \
             `--role=dbproxy` runs the capability-free copy",
            blastit_sys::caps::Capability::SETCAP,
        ),
    }
}

/// The half of `R13`'s app rule that every caller owns: an all-static manifest
/// needs no `app`, and a `BLASTIT_APP` that names one there is refused rather
/// than ignored.
///
/// Every route answers from the filesystem before an app could run, so the
/// setting can never take effect; a boot or a `check` that silently dropped it
/// would leave its operator believing an app was serving. The message names the
/// variable, never its value. `check` takes *only* this half — see
/// [`app_rule`] — because `blastit check` never demanded an `app`: the suite's
/// own configuration supplies the WSGI callable.
pub(crate) fn static_app_refusal(config: &Config, manifest: &Manifest) -> Option<String> {
    (is_static_only(manifest) && config.app.is_some()).then(|| {
        "BLASTIT_APP: the file serves static mounts and no views, so no app can run: \
         unset BLASTIT_APP, or declare a view"
            .to_string()
    })
}

/// True when the file declares nothing but static mounts: no callable, no front
/// controller and no view (`static "/" { root "public" }` is the whole file).
///
/// This is [`Manifest::is_all_static`] and nothing else: §9.5 answers the
/// question once, over mounts and views, and the boot asks the model's answer
/// rather than keeping a second reading that could drift from it.
pub(crate) fn is_static_only(manifest: &Manifest) -> bool {
    manifest.is_all_static()
}

/// True when every request is answered by one of the file's own executors: the
/// one `entrypoint php` front controller, or a static mount.
///
/// This is [`Manifest::can_boot_without_app`] and nothing else (§9.5, §12.2):
/// an `entrypoint php` no view reaches is the `EntrypointNotReached` warning,
/// never a missing `BLASTIT_APP`. A WSGI view with no mount and no front
/// controller is the case that still needs the callable.
pub(crate) fn runs_own_executors(manifest: &Manifest) -> bool {
    manifest.can_boot_without_app()
}

/// `R13`: the app rule a **boot** refuses with (`blastit`, `blastit dev`).
///
/// Both halves: a `BLASTIT_APP` no route could reach is refused
/// ([`static_app_refusal`]), and a manifest whose views do **not** all run on an
/// executor of the file's own needs a callable to forward to. The second half
/// asks [`runs_own_executors`] — "every request is answered by the front
/// controller or a mount" — rather than "every view needs an app": an
/// `entrypoint php` file is its own executor, so the *rule* demands no `app` for
/// a PHP-only manifest (`ADR-0010` §2); what such a boot needs instead is a
/// usable NTS `libphp`, and [`php_boot_gate`] is what refuses a host that has
/// none.
/// `blastit check` must not take the second half — a suite brings its own
/// `app` — so it calls [`static_app_refusal`] directly.
pub(crate) fn app_rule(config: &Config, manifest: &Manifest) -> Result<(), String> {
    if let Some(message) = static_app_refusal(config, manifest) {
        return Err(message);
    }
    if !runs_own_executors(manifest) && config.app.is_none() {
        return Err("BLASTIT_APP: the WSGI callable (`module:attribute`) is required".to_string());
    }
    Ok(())
}

/// `ADR-0011` (superseding `ADR-0006` decision 3): the ingress gate the
/// **production** path refuses with.
///
/// `body = "raw"` is the one ingress declaration nothing gates at run time —
/// `plan_body` streams the bytes into the application's own parser unchanged —
/// so the acknowledgement in the manifest is the whole of the decision, and an
/// unacknowledged one is a boot refusal naming the route and the key, before any
/// listener is bound. The rule is a sibling of [`app_rule`] rather than part of
/// it because only the serve path takes it: `blastit dev` is the local
/// single-uid simulation and keeps `ManifestWarning::RawIngress` (emitted with
/// the rest of the boot's warnings), and `blastit check` keeps the `raw_ingress`
/// finding so CI reports the same rule before anyone deploys.
///
/// Stage 1 calls this too, immediately after its own `parse_blastfile` of the
/// file: the configuration document carries the manifest's *path*, not a digest
/// of what stage 0 approved, so the re-read is a real checkpoint and not
/// belt-and-braces.
pub(crate) fn raw_ingress_gate(manifest: &Manifest) -> Result<(), String> {
    let refusals = manifest.raw_ingress_refusals();
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals.join("\n"))
    }
}

/// `MB.3.54`: the serve path's manifest gates, as a value [`bind_view_sockets`]
/// will not accept any other way.
///
/// The gate used to be a bare `if let Err(..)` statement in [`run`], between the
/// manifest parse and the first listener: deleting it — or letting it drift
/// after the bind — left every test green, because an unprivileged `cargo test`
/// cannot run stage 0 past [`blastit_sys::caps::detect`] (the suite that
/// measures the gate spells that out:
/// `crates/blastit/tests/critical_raw_ingress.rs`). The witness is the type
/// system instead: [`ServeGates::clear`] is the only constructor and it *is*
/// [`raw_ingress_gate`], and [`bind_view_sockets`] takes `&ServeGates`, so no
/// boot can bind a view socket without the gate having passed first in this
/// process — and removing the call site is a compile error rather than a silent
/// fail-open.
///
/// Crate-private on purpose, which is stronger than a `#[doc(hidden)] pub` seam:
/// nothing outside this crate can hold the proof or mint one, so the ordering
/// holds for every caller for as long as the crate compiles. What a privileged
/// run is still the only witness for is stage 0's *own* refusal message (the
/// gate's one home is unit-tested, and the refusal's process behaviour is
/// measured through stage 1's re-read of the same file).
pub(crate) struct ServeGates {
    /// Uninhabited outside this module: [`ServeGates::clear`] is the only way in.
    _private: (),
}

impl ServeGates {
    /// Runs the serve path's manifest gates and returns the proof they passed.
    fn clear(manifest: &Manifest) -> Result<Self, String> {
        raw_ingress_gate(manifest)?;
        Ok(Self { _private: () })
    }
}

/// `ADR-0009` §5: what a build with no Python support cannot serve, and the
/// sentence that names the artifact which can (`MB.3.11`).
///
/// A build with `python` off — the standalone archive of `ADR-0010` §4 — has no
/// interpreter in any role, so a manifest that needs one is refused **here, in
/// stage 0, before a listener is bound**, rather than at the first request or at
/// a fork that has already happened. The rule this replaces is
/// [`app_rule`]'s "the WSGI callable is required": on this build setting
/// `BLASTIT_APP` cannot help, so telling the operator to set it would send them
/// the wrong way.
///
/// An all-static manifest needs no interpreter at all and is left to
/// [`static_app_refusal`], which keeps its `BLASTIT_APP`-on-a-static-tree
/// sentence in both builds. So does an `entrypoint php` file: it runs in a `view`
/// worker with no interpreter anywhere near it, so this gate returns `None`
/// for a PHP-only manifest (`MB.3.5.4`) — [`php_boot_gate`] is the separate
/// gate that refuses such a boot when the host has no usable NTS `libphp`, and
/// it must not be mistaken for this one. The
/// `python` build has nothing to refuse; the function is compiled for the
/// tests that keep the sentences honest.
///
/// The retired external-PHP key (`MB.3.17`) needs no sentence here: the parser
/// refuses it as an unknown key before a boot gets this far.
#[cfg(any(not(feature = "python"), test))]
pub(crate) fn no_python_support(
    config: &Config,
    manifest: &Manifest,
    learn: bool,
) -> Option<String> {
    const INSTALL: &str = "install the Python package: uv add blastit";
    if learn {
        return Some(format!(
            "dev --learn: this build has no Python support (built without feature \
             \"python\") and --learn needs the interpreter for its gate; {INSTALL}, or \
             run a build with --features python"
        ));
    }
    if runs_own_executors(manifest) {
        return None;
    }
    if let Some(app) = config.app.as_deref() {
        return Some(format!(
            "BLASTIT_APP ({app}): this build has no Python support (built without feature \
             \"python\"), so it cannot import a WSGI application; {INSTALL}"
        ));
    }
    // The first view no mount or front controller answers: a static mount and an
    // `entrypoint php` both answer without an interpreter, so only a view that
    // nothing else reaches is a WSGI view this build cannot fork.
    let route = manifest
        .routes()
        .first()
        .filter(|_| manifest.php().is_none() && manifest.mounts().is_empty())?;
    Some(format!(
        "view {:?}: this build has no Python support (built without feature \"python\"), \
         and the view is reached by no `static` mount and no `entrypoint php`, so it needs a \
         runtime this build does not carry; {INSTALL}, or declare a `static` mount or an \
         `entrypoint php`",
        route.name()
    ))
}

/// The variable that tunes how long a stage-1 `entrypoint php` probe may take
/// (`ADR-0010` §3).
pub(crate) const PROBE_TIMEOUT_VAR: &str = "BLASTIT_PROBE_TIMEOUT_MS";

/// Its default: `ADR-0010` §3's `5000` ms.
pub(crate) const PROBE_TIMEOUT_DEFAULT: Duration = Duration::from_millis(5000);

/// The document member that carries the probe timeout across the `execve`.
///
/// `blastit-env` strips every `BLASTIT_*` variable from the stage-1 environment
/// (`ADR-0010` §3), so the knob is read here and travels in the configuration
/// document stage 0 writes — the same way `seccomp` and the warnings do.
pub(crate) const PROBE_TIMEOUT_MEMBER: &str = "php_probe_timeout_ms";

/// The document member that carries the stage-1 **program prefix** across the
/// `execve` (`MB.23.5.2.2`, design §4.6 step 6).
///
/// A reload cycle re-execs the program this image was started with — the
/// standalone binary for a container, `python -I -m blastit` for a wheel — and
/// that is a stage-0 fact: only stage 0 chose `Host::stage1()`. It travels in
/// the configuration document rather than in the environment for the same
/// reason the probe timeout does: every `BLASTIT_*` variable is stripped from a
/// view, and a token in the environment would be exactly the leak the strip
/// exists to prevent. `None` (stage 1's fallback) is `/proc/self/cmdline` up to
/// the first `--stage=1` token.
pub(crate) const RESTART_MEMBER: &str = "restart";

/// Reads `BLASTIT_PROBE_TIMEOUT_MS`, the one place a probe's patience is
/// decided.
///
/// `None` when it is unset: the document then says nothing, and stage 1 uses
/// [`PROBE_TIMEOUT_DEFAULT`] — one number in one place either way. A value that
/// is not a positive whole number of milliseconds is refused while the operator
/// can still see the sentence.
fn probe_timeout() -> Result<Option<Duration>, String> {
    let Some(text) = std::env::var_os(PROBE_TIMEOUT_VAR) else {
        return Ok(None);
    };
    let text = text.to_string_lossy();
    let millis: u64 = text.trim().parse().map_err(|_| {
        format!(
            "{PROBE_TIMEOUT_VAR}={text:?} is not a whole number of milliseconds; set it to e.g. \
             5000, or unset it for the default"
        )
    })?;
    if millis == 0 {
        return Err(format!(
            "{PROBE_TIMEOUT_VAR}=0 would kill every entrypoint php probe before it starts; set it \
             to e.g. 5000, or unset it for the default"
        ));
    }
    Ok(Some(Duration::from_millis(millis)))
}

/// `MB.3.64`: the test-only rendezvous that gives `critical_php_embed`'s
/// inode-pin legs a deterministic window between stage 0's record and stage 1's
/// re-open.
///
/// This is a **test seam, not a setting**. It exists only in a
/// `debug_assertions` build — every `cargo test` binary — and the constant
/// itself does not exist in a release build, so a shipped artifact has no name
/// to read: `scripts/check-no-test-seam.sh` measures that absence on the
/// packaged binary instead of trusting the gate.
///
/// It is read *here*, in the composition root, because the stage 0→1 `execve`
/// hands stage 1 an environment from which `blastit-env` has stripped every
/// `BLASTIT_*` variable (`R12`). The value therefore crosses the barrier in the
/// configuration document, exactly as [`PROBE_TIMEOUT_VAR`] does — and for the
/// same reason a route process never sees either name.
#[cfg(debug_assertions)]
pub(crate) const TEST_SWAP_POINT_VAR: &str = "BLASTIT_TEST_SWAP_POINT";

/// `MB.23.5.2.2`: the debug-only fault seam that makes the live-reload boot
/// take a ladder row on purpose.
///
/// `BLASTIT_TEST_RELOAD_FAULT=open` makes stage 1's watcher construction fail
/// `EPERM`, so `critical_dev_reload` can witness "exactly one
/// `dev.reload.unavailable`, then serve" against real processes. Read here, in
/// the composition root, because `blastit-env` strips every `BLASTIT_*`
/// variable before stage 1 — it travels in the configuration document exactly
/// as [`TEST_SWAP_POINT_VAR`] does, and exists only in a `debug_assertions`
/// build.
#[cfg(debug_assertions)]
pub(crate) const TEST_RELOAD_FAULT_VAR: &str = "BLASTIT_TEST_RELOAD_FAULT";

/// The configuration document member [`TEST_RELOAD_FAULT_VAR`] travels under.
#[cfg(debug_assertions)]
pub(crate) const TEST_RELOAD_FAULT_MEMBER: &str = "test_reload_fault";

/// The configuration document member [`TEST_SWAP_POINT_VAR`] travels under.
///
/// Stage 1 reads it back (`stage1::read_config`) and pauses at
/// [`crate::php_view::await_test_swap`]; a release build has neither constant,
/// so the member is never written and never honoured.
#[cfg(debug_assertions)]
pub(crate) const TEST_SWAP_POINT_MEMBER: &str = "test_swap_point";

/// The configuration document stage 1 boots from, with the probe timeout — and,
/// in a debug build, the test-only swap point — folded in.
///
/// # Errors
///
/// [`probe_timeout`]'s refusal for a `BLASTIT_PROBE_TIMEOUT_MS` that is not a
/// positive whole number of milliseconds.
fn config_document(config: &Config) -> Result<serde_json::Value, String> {
    let mut value = config.to_json();
    if let Some(timeout) = probe_timeout()?
        && let Some(object) = value.as_object_mut()
    {
        let millis = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
        object.insert(
            PROBE_TIMEOUT_MEMBER.to_string(),
            serde_json::Value::from(millis),
        );
    }
    // `MB.3.64`: a path a test owns, carried across the `execve` for the same
    // reason the probe timeout is. Compiled out of every release build, so the
    // variable cannot name anything a shipped binary would act on.
    #[cfg(debug_assertions)]
    if let Some(point) = std::env::var_os(TEST_SWAP_POINT_VAR)
        && let Some(object) = value.as_object_mut()
    {
        object.insert(
            TEST_SWAP_POINT_MEMBER.to_string(),
            serde_json::Value::from(point.to_string_lossy().into_owned()),
        );
    }
    // `MB.23.5.2.2`: the reload ladder's fault seam, carried the same way and
    // compiled out of every release build.
    #[cfg(debug_assertions)]
    if let Some(fault) = std::env::var_os(TEST_RELOAD_FAULT_VAR)
        && let Some(object) = value.as_object_mut()
    {
        object.insert(
            TEST_RELOAD_FAULT_MEMBER.to_string(),
            serde_json::Value::from(fault.to_string_lossy().into_owned()),
        );
    }
    Ok(value)
}

/// `ADR-0010` §3/§4.1: the discovery gate an `entrypoint php` file is resolved
/// by, before any listener binds.
///
/// Runs in the stage-0 image and never loads anything: the `libphp` is opened,
/// `fstat`ed and read as an ELF `.dynsym` table by
/// [`blastit_view_php::discovery`] — `dlopen` happens inside a view worker,
/// never here. This function is the *configuration* half: it maps the values the
/// boot's configuration names into a [`PhpSearch`], decides whether this boot is
/// a deployment, and supplies the two provenance callbacks a deployment's gate
/// asks for.
///
/// The order is the order an operator can act on: the library (missing, a
/// directory, not an ELF, ZTS, no `php_version`, no version this boot can read)
/// and then the shim for the tuple. There is no `workers` arm: `ADR-0012` §12.3
/// removed the keyword, and the deployment's budget comes from configuration.
/// Every one of those refusals is fatal in **both** scopes: `ADR-0010` §1 gives
/// PHP one backend, so a host that cannot serve a declared `entrypoint php` file
/// gets §4.1's package sentence — in `blastit dev` and `blastit check` as much
/// as in a deployment. There is no development mode that serves a PHP route
/// another way.
#[cfg(feature = "php")]
pub(crate) fn php_boot_gate(config: &Config, manifest: &Manifest, scope: PhpShimScope) -> PhpBoot {
    // `R7`/`B2-R5` are `Production` rules (`ADR-0010` §3): a deployed boot
    // authorises every path a route uid must not be able to write, while `dev`
    // and `check` do not. That is not a convenience — the shim a developer has
    // just built lives in `target/php-sapi`, inside a checkout its own uid owns
    // and writes, so "no other uid can replace it" cannot hold there and would
    // refuse every development boot while protecting nothing: the uid that
    // could replace the shim is the uid that already runs the boot. A
    // deployment has no such coincidence — its route uids (`UID_BASE + 1000 +
    // i`) are not the uid that owns `$BLASTIT_EXEC`, the shims or
    // the `Blastfile`'s tree.
    let init_uid = current_uid();
    let objects = |what: &str, path: &Path| object_is_protected(init_uid, what, path);
    let trees = |path: &Path| tree_is_protected(init_uid, path);
    let deployment = (scope == PhpShimScope::Deployment).then(|| Deployment {
        init_uid,
        manifest_dir: config.manifest_dir.clone(),
        is_protected_object: &objects,
        is_protected_tree: &trees,
    });
    php_boot_in(
        manifest,
        &PhpSearch::new(config.libphp.clone(), config.php_shim_dir.clone(), scope),
        deployment.as_ref(),
    )
}

/// `R7`/`B2-R5`: the **object** decision a deployment boot hands the gate.
///
/// [`Provenance::check_objects`] is the whole public decision — ownership, no
/// file capabilities, and zero writable `DT_RPATH`/`DT_RUNPATH` entries — which
/// is why the gate takes a callback over it instead of re-implementing a
/// per-path test: the search-path half is private to [`crate::host`].
#[cfg(feature = "php")]
fn object_is_protected(init_uid: u32, what: &str, path: &Path) -> Result<(), String> {
    Provenance {
        init_uid,
        objects: vec![Claim::new(what, path)],
        trees: Vec::new(),
    }
    .check_objects()
}

/// `R7`/`B2-R5`: the **tree** decision a deployment boot hands the gate.
///
/// Ownership and permissions only: a tree is read, never loaded. The gate takes
/// it as a second callback because one callback would silently drop the object
/// decision's search-path half (design §4.4 D5).
#[cfg(feature = "php")]
fn tree_is_protected(init_uid: u32, path: &Path) -> Result<(), String> {
    Provenance {
        init_uid,
        objects: Vec::new(),
        trees: vec![Claim::new("the tree", path)],
    }
    .check_trees()
}

/// The same gate for a `dev`/`check` caller: no production provenance.
///
/// The unit tests use it as the seam they have always used; a boot reaches the
/// gate through [`php_boot_gate`], which decides the scope. It took the
/// per-view budget until `MB.21.4.10`: `ADR-0012` §12.3 removed `workers`, so a
/// manifest carries no cap and the deployment's is the scheduler's, which is why
/// the parameter is gone rather than ignored.
#[cfg(all(test, feature = "php"))]
fn php_boot(manifest: &Manifest, search: &PhpSearch) -> PhpBoot {
    php_boot_in(manifest, search, None)
}

/// `R7`/`B2-R5`: the **deployment** half of the `entrypoint php` gate, run over one
/// manifest directory and one pair — the seam the protected suite needs.
///
/// It runs the same function a deployment boot runs, `php_boot_in` with a
/// `Deployment`, because an unprivileged test process cannot reach a deployment
/// boot at all: `caps::detect` refuses it before `Config::from_env`
/// (`LESSONS.md`, `MB.1.6`). Driving [`Provenance`] directly — which the
/// protected legs also do — pins the *decision* but not the *call*: with this
/// seam deleted from `php_boot_in`, `tests/critical_php_embed.rs`'s
/// `the_deployment_gate_is_invoked_behind_its_sentence` is the leg that fails,
/// and without it only this module's unit tests would.
///
/// `Ok(())` is "the gate proceeds" — `Proceed` from the discovery *and* a
/// a file with no `entrypoint php`, which a deployment boots like any
/// other (the gate has nothing to authorise). `Err(message)` is a **refusal**:
/// the sentence the boot prints, the same one `php_boot`'s callers print.
///
/// Not part of the published API: the only caller is the protected suite, which
/// reaches it through `blastit::host::deployment_boot_check`.
#[cfg(feature = "php")]
pub fn deployment_boot_check(
    manifest_source: &str,
    manifest_dir: &Path,
    libphp: &Path,
    shim_dir: &Path,
    uid_base: u32,
) -> Result<(), String> {
    let (manifest, _warnings) = Manifest::parse_blastfile(manifest_source, uid_base)
        .map_err(|errors| errors.to_string())?;
    let search = PhpSearch {
        libphp: Some(libphp.to_path_buf()),
        shim_dirs: vec![shim_dir.to_path_buf()],
        lib_dirs: Vec::new(),
    };
    let init_uid = current_uid();
    let objects = |what: &str, path: &Path| object_is_protected(init_uid, what, path);
    let trees = |path: &Path| tree_is_protected(init_uid, path);
    let deployment = Deployment {
        init_uid,
        manifest_dir: manifest_dir.to_path_buf(),
        is_protected_object: &objects,
        is_protected_tree: &trees,
    };
    match php_boot_in(&manifest, &search, Some(&deployment)) {
        // Nothing to authorise is not a refusal: a manifest with no PHP route
        // boots (`MB.3.44` — reporting this as an error let a caller read a
        // *proceeding* manifest as a refused one, the confusion this seam
        // exists to prevent).
        PhpBoot::Proceed(_) | PhpBoot::Absent => Ok(()),
        PhpBoot::Refused(message) => Err(message),
    }
}

/// `R13`: the sentence a build without the `php` feature refuses an
/// `entrypoint php` manifest with.
///
/// One line, naming the build that has the backend, and never a link failure: a
/// `--no-default-features` build compiles, and a file that asks for a PHP route
/// is refused here — the same shape [`no_python_support`] has for the WSGI half.
#[cfg(not(feature = "php"))]
pub(crate) fn no_php_support(manifest: &Manifest) -> Option<String> {
    manifest.php().map(|php| {
        format!(
            "entrypoint php {:?}: this build has no PHP support (built without feature \
             \"php\"), so it cannot embed libphp; build with the php feature, or remove the \
             entrypoint from the Blastfile",
            php.entry
        )
    })
}

/// §8.1's `runtime` line for a boot whose entrypoint is **not** PHP (`MB.25.2`).
///
/// PHP's own line is emitted by the probe below the fork
/// ([`blastit_view_php::probe::probe_bindings`]): only a process that ran
/// `php_module_startup` can report the `php.ini` PHP actually opened, and a path
/// derived here from `PHPRC`, the cwd or `PHP_CONFIG_FILE_PATH` would be a
/// guess. The other two shapes are stage 0's to name, because it is the first
/// place that knows which of them the file resolved to.
///
/// `config.php` is `Some` only when the discovery gate resolved a library, and
/// `config.app` covers both `entrypoint python` (the model fills it from the
/// file when `BLASTIT_APP` is unset) and a `BLASTIT_APP` on a view. Neither set
/// means static mounts and no language runtime — including an `entrypoint php`
/// that no view reaches, which loads nothing and is already warned about.
///
/// A build that embeds CPython in stage 1 (`cargo` development builds, where
/// `interpreter` is `false`) has no interpreter *here*: the facts exist one
/// stage later, and inventing a version would be worse than saying nothing.
fn emit_runtime(emitter: &Emitter, config: &Config, interpreter: bool) {
    // A build without the `php` feature has no record to ask: no manifest that
    // reaches this point declares one (the gate refuses it first), so "no PHP
    // entrypoint" is the answer that cannot invent a line.
    #[cfg(feature = "php")]
    let entrypoint_php = config.php.is_some();
    #[cfg(not(feature = "php"))]
    let entrypoint_php = false;
    if let Some(runtime) = runtime_shape(entrypoint_php, config.app.is_some(), interpreter) {
        emitter.emit(&Event::now(Component::Init, EventKind::Runtime(runtime)));
    }
}

/// The decision table behind [`emit_runtime`], as a value: which shape a boot
/// resolved, or `None` when the line is not this stage's to emit.
///
/// The three inputs are what actually decides it — a resolved library, a
/// callable, and an interpreter live in *this* process — so the table is one
/// testable function rather than three conditions spread over two boot paths.
fn runtime_shape(entrypoint_php: bool, has_app: bool, interpreter: bool) -> Option<Runtime> {
    if entrypoint_php {
        // The probe reports it, after `php_module_startup`
        // (`blastit_view_php::probe`).
        return None;
    }
    if has_app {
        python_runtime(interpreter)
    } else {
        Some(Runtime::Static)
    }
}

/// What `entrypoint python` resolved to, from the interpreter that is already
/// running in this process (`MB.25.2`).
///
/// A wheel install is the only install where that is true: `blastit._blastit`
/// is called from the interpreter the wheel was installed into, so stage 0 can
/// ask it for `sys.version_info` and `sys.prefix` before anything is forked —
/// the earliest point the interpreter exists. `VIRTUAL_ENV` wins over
/// `sys.prefix` because that is the environment a developer named.
#[cfg(feature = "python")]
fn python_runtime(interpreter: bool) -> Option<Runtime> {
    use pyo3::prelude::PyAnyMethods as _;

    if !interpreter {
        return None;
    }
    // `try_attach`, not `attach`: a caller that said "an interpreter is live"
    // and is wrong gets no line, never a panic before the fork.
    Python::try_attach(|py| {
        let version = py.version_info();
        let environment = std::env::var("VIRTUAL_ENV")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                py.import("sys")
                    .and_then(|sys| sys.getattr("prefix"))
                    .and_then(|prefix| prefix.extract::<String>())
                    .ok()
            })?;
        Some(Runtime::python(
            format!("{}.{}.{}", version.major, version.minor, version.patch),
            environment,
        ))
    })
    .flatten()
}

/// A build with no interpreter has no `python` shape to report.
#[cfg(not(feature = "python"))]
fn python_runtime(_interpreter: bool) -> Option<Runtime> {
    None
}

/// `R13` step 5: the first declared static root the responder's uid could not
/// read, as the sentence a boot refuses with.
///
/// `probe` is the scheduler's uid in production — the process that answers every
/// static request — and the caller's in `dev` and `check`, where the prober *is*
/// the responder. The scheduler's own `StaticRoot::open` stays the final
/// authority; this exists so a deployment whose `0750` build output the
/// scheduler cannot read is refused before a listener is bound, rather than
/// serving a tree only some identities can see.
pub(crate) fn static_root_permission(
    manifest: &Manifest,
    manifest_dir: &Path,
    probe: u32,
    interpreter: bool,
) -> Option<String> {
    // `dev` and `check`: the prober *is* the responder, and an unprivileged
    // process may not call `setgroups(2)`, so the dropped-to probe cannot
    // answer at all and `visibility::unreadable` falls back to reading mode
    // bits as a *foreign* uid would — which refuses an ordinary `$HOME`
    // (0700, owned by the caller) over a perfectly readable `public/`. Ask the
    // kernel here instead, in this process's own identity: the same `read_dir`
    // the responder's `O_PATH|O_DIRECTORY` open performs.
    if probe == current_uid() {
        return manifest
            .static_roots(manifest_dir)
            .into_iter()
            .find(|(_, path)| !listable(path))
            .map(|(index, _)| static_root_refusal(manifest, index));
    }
    let dropped = crate::visibility::DroppedTo {
        uids: std::collections::BTreeSet::from([probe]),
        probe,
    };
    for (index, path) in manifest.static_roots(manifest_dir) {
        let refused =
            match crate::visibility::unreadable(std::slice::from_ref(&path), &dropped, interpreter)
            {
                Ok(None) => false,
                // A probe that could not answer is refused rather than assumed
                // fine: a root nobody proved readable is one nobody serves.
                Ok(Some(_)) | Err(_) => true,
            };
        if refused {
            return Some(static_root_refusal(manifest, index));
        }
    }
    None
}

/// Whether this process can list `path`: a root the responder cannot open is
/// one it cannot serve, and an `O_PATH|O_DIRECTORY` open needs exactly what a
/// directory listing needs on the directory and `x` on every ancestor.
fn listable(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => !matches!(entries.next(), Some(Err(_))),
        Err(_) => false,
    }
}

/// The declared root, as written, in the sentence a boot refuses with.
///
/// `index` is a **mount** index (§9.6), which is what `static_roots` returns.
fn static_root_refusal(manifest: &Manifest, index: usize) -> String {
    let root = manifest
        .mounts()
        .get(index)
        .map_or_else(String::new, |mount| mount.policy.root.clone());
    format!("static root {root:?}: permission denied")
}

/// Which layout process, if any, already runs under `uid` (D6).
///
/// Pure, so the collision check is a unit test rather than a deployment
/// surprise. A rootless `init` runs as an ordinary uid, and the layout hands
/// every other process its own uid out of the same space; if init's uid *is* one
/// of them, the process that was supposed to be sandboxed under it shares init's
/// identity — it could read init's private state, and a `setuid` back to that
/// uid would be no change at all. `None` means the uid is free.
fn layout_collision(manifest: &Manifest, layout: &UidLayout, uid: u32) -> Option<String> {
    if uid == layout.http() {
        return Some("the http process".to_string());
    }
    if uid == layout.scheduler() {
        return Some("the scheduler".to_string());
    }
    if uid == layout.dbproxy() {
        return Some("dbproxy".to_string());
    }
    manifest
        .routes()
        .iter()
        .enumerate()
        .find(|(index, _)| layout.view(*index) == uid)
        .map(|(_, route)| format!("route `{}`", route.name()))
}

/// How long stage 0 keeps reading a failed dbproxy's output before giving up
/// (§8.1).
///
/// Short on purpose: init is already failing the boot, and the child that never
/// reported ready is still alive and still holds the write end of its pipes —
/// see [`emit_child_output`].
pub const CHILD_OUTPUT_TIMEOUT: Duration = Duration::from_millis(250);

/// Largest configuration document init will write into a pipe.
///
/// The pipe holds 64 KiB and stage 1 is not reading yet, so a larger document
/// would deadlock the writer. A real configuration is a few hundred bytes.
pub const MAX_CONFIG_DOCUMENT: usize = 60 * 1024;

/// The limits dbproxy runs under: a descriptor budget sized from the
/// configuration, and no core file (§7.2).
///
/// `RLIMIT_NPROC` belongs to the view uids, so it is left alone here.
fn dbproxy_limits(config: &Config) -> Rlimits {
    Rlimits {
        nproc: None,
        nofile: Some(config.component_nofile()),
        core_zero: true,
    }
}

/// Runs stage 0. Never returns: it `execve`s stage 1 or exits non-zero.
///
/// `host` says how blastit was installed: which program the barrier and
/// dbproxy's own `execve` start, and — for a wheel — the interpreter's import
/// path, which travels with the configuration because stage 1 is re-executed
/// with `-I` (§4).
///
/// `interpreter` is `true` when a live CPython is in this image (a wheel: the
/// console script's), which is what its `fork` has to account for (§9.3, and
/// [`crate::forking`]).
///
/// `preset` is `--preset` when the command line carried one, and it wins over
/// `BLASTIT_PRESET` (E1, [`Config::from_env`]).
pub fn run(host: &Host, interpreter: bool, preset: Option<&str>) -> ! {
    // §8.1: the log format is one of the *settings*, and stage 0's first lines
    // are made before there is a configuration to read it from — so they are
    // kept here, in the order they were made, and written out once the settings
    // (preset included) have been resolved. A boot whose configuration cannot be
    // read at all writes them as JSON instead, followed by the error: JSON is
    // the one format that needs no settings to print.
    let mut early = Early::default();
    // D6: root or a handful of capabilities is read from the kernel, and it
    // decides whether the boot can build its sandboxes at all. It comes before
    // the first event, because the event has to report it.
    let held = match blastit_sys::caps::detect() {
        Ok(held) => held,
        Err(err) => {
            // A boot that cannot read its own capability sets cannot say which
            // mode it is in. It says nothing about the mode rather than
            // guessing, and stops: a child that keeps a capability nobody
            // looked for is exactly what §4 refuses to risk. The advice is the
            // one an operator can act on, which is why it comes from here
            // (`Host` knows how blastit was installed) and not from the error.
            early.emit(Event::now(
                Component::Init,
                EventKind::Boot(stage0_lifecycle(None)),
            ));
            fail_early(&mut early, &capability_advice(host, &err));
        }
    };
    let privilege = privilege_from(&held);
    // D6/B1, B2: the file that may carry the capabilities is init's alone. A
    // child that has dropped its bounding set cannot `execve` such a file at
    // all, and every process of init's uid can run it — so the boot checks
    // where each role's program comes from, and says what to fix.
    let placement = match host.placement(held.bounding_set_dropped) {
        Ok(placement) => placement,
        Err(message) => {
            early.emit(Event::now(
                Component::Init,
                EventKind::Boot(stage0_lifecycle(Some(&privilege))),
            ));
            fail_early(&mut early, &message);
        }
    };
    early.emit(Event::now(
        Component::Init,
        EventKind::Boot(stage0_lifecycle(Some(&privilege))),
    ));
    if !held.bounding_set_dropped {
        // M1: the boot runs, and says so, once.
        early.emit(
            output_event(
                Component::Init,
                None,
                Stream::Stderr,
                "cap_setpcap is not held: the children keep their bounding set. Every \
                 other capability set is still cleared, and `no_new_privs` still stops \
                 an `execve` from granting what the bounding set allows (D6/M1)",
            )
            .with_level(Level::Warn),
        );
    }
    // D6: init's uid is a dedicated one. Whatever else runs as it can run the
    // file the capabilities are on — and becoming init is `setuid(0)`. A
    // deployment decides who that uid is (a worker, a migration, an operator's
    // shell all belong somewhere else); this says when it did not, once, before
    // anything is forked.
    warn_about_siblings(&mut early);

    let mut config = match Config::from_env(preset) {
        Ok(config) => config,
        Err(err) => fail_early(&mut early, &err.to_string()),
    };
    // D6: init's mode belongs in the configuration, because stage 1's preload
    // `boot` event reports the same thing stage 0's does. It is set here, before
    // anything can fail with it, so the `boot` line below can say it — and so
    // E1's preset assumption can be checked against it.
    config.privilege = Some(privilege.clone());
    let emitter = Emitter::stdout(config.log_format, config.log_level);
    // The lines that waited for the settings, in the format they name.
    early.flush(&emitter);
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Boot(config_lifecycle(&privilege, &config)),
    ));
    // E1: a preset rests on a platform that hands init a particular identity, so
    // a boot that contradicts one says so here — before the manifest is read,
    // because a boot that fails on the manifest, on the environment rewrite or on
    // a view socket never reaches stage 1, and this is the line that explains
    // that boot to the operator who picked the preset.
    if let Some(warning) = config.preset_assumption_warning(privilege.mode) {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, warning.message())
                .with_level(Level::Warn),
        );
    }

    // The file is named before the setting: in a directory with no
    // `./Blastfile` that file is what is missing, and `BLASTIT_APP` would
    // send the operator looking for an environment variable instead of a
    // project (`docs/troubleshooting.md`). The sentence is §15.1's and is the
    // same one `dev`, `check` and `fmt` print.
    let source = match crate::config::read_manifest(&config.manifest) {
        Ok(source) => source,
        Err(error) => fail(&emitter, &error.sentence(&config.manifest)),
    };
    let (manifest, warnings) = match Manifest::parse_blastfile(&source, config.uid_base) {
        Ok(parsed) => parsed,
        Err(errors) => fail(&emitter, &errors.to_string()),
    };
    // `R13`: `BLASTIT_APP` is required unless the file answers every request
    // itself (§9.5). The manifest is parsed first, because that is a property of
    // the file, not of the environment — and a missing `./Blastfile` is still
    // named before the setting (`MB.1.6`).
    // `ADR-0009` §5: a build with no Python support (`ADR-0010` §4's standalone
    // archive) refuses a manifest that needs an interpreter here, before any
    // listener is bound, naming the artifact that has one. Ahead of `app_rule`
    // on purpose: on this build "set BLASTIT_APP" is advice that cannot work.
    #[cfg(not(feature = "python"))]
    if let Some(message) = no_python_support(&config, &manifest, false) {
        fail(&emitter, &message);
    }
    // `ADR-0010` §3, §4.1: before any listener binds, (1) a `workers` cap above
    // the view budget is refused naming both numbers, and (2) the NTS `libphp`
    // and the shim for its tuple are resolved statically — a host that cannot
    // embed PHP gets §4.1's package sentence, in `dev` as much as here. Ahead of
    // `app_rule`, whose "set BLASTIT_APP" advice cannot make a PHP route serve,
    // and before `bind_view_sockets`. The gate only reads the ELF: the `dlopen`
    // happens inside a view worker. What follows: `config.php` carries the
    // record, stage 1 re-opens it into the per-route bindings, and `Ctl::Spawn`
    // forks from those.
    #[cfg(not(feature = "php"))]
    if let Some(message) = no_php_support(&manifest) {
        fail(&emitter, &message);
    }
    #[cfg(feature = "php")]
    match php_boot_gate(&config, &manifest, PhpShimScope::Deployment) {
        PhpBoot::Absent => {}
        PhpBoot::Refused(message) => fail(&emitter, &message),
        PhpBoot::Proceed(runtime) => {
            // The line naming what was resolved is the probe's
            // (`php_view::probe_bindings`, `MB.25.2`): it is the process that
            // runs `php_module_startup`, so it is the only one that can say
            // which `php.ini` the workers will read. Stage 0 records the pair
            // here and says nothing about it in the log.
            //
            // The `(path, st_dev, st_ino)` record stage 1 re-opens and every
            // PHP worker's fork is built from (`ADR-0010` §3). A host without
            // a usable library never reaches here: the gate refused above.
            config.php = Some(runtime);
        }
    }
    if let Err(message) = app_rule(&config, &manifest) {
        fail(&emitter, &message);
    }
    // `ADR-0011` (superseding `ADR-0006` decision 3): `body = "raw"` is the one
    // ingress declaration nothing gates at run time, so the manifest's
    // acknowledgement is the whole of the decision. On **this** path — the serve
    // path, which `dev` does not take — an unacknowledged one is a boot refusal
    // before any listener is bound. `dev` keeps its warning and `check` keeps
    // its finding (`MB.3.53`).
    //
    // `MB.3.54`: the gate returns the proof `bind_view_sockets` demands, so this
    // statement cannot be deleted or moved after the bind without a compile
    // error — the witness an unprivileged test cannot be here, because `run`'s
    // first act (`caps::detect`) refuses an unprivileged boot before the
    // manifest is even read.
    let serve_gates = match ServeGates::clear(&manifest) {
        Ok(cleared) => cleared,
        Err(message) => fail(&emitter, &message),
    };
    // `R13` step 4: a static root that is missing, a file or unlistable is
    // named with the manifest that declares it, before any listener is bound.
    if let Err(errors) = manifest.check_static_roots(&config.manifest_dir) {
        fail(
            &emitter,
            &format!("{}: {errors}", config.manifest.display()),
        );
    }
    // `R13` step 5: and one the responder's own uid could not read is the same
    // refusal, before anything is forked to serve it.
    if let Some(message) = static_root_permission(
        &manifest,
        &config.manifest_dir,
        UidLayout::new(config.uid_base).scheduler(),
        interpreter,
    ) {
        fail(&emitter, &message);
    }
    for warning in &warnings {
        emitter.emit(&Event::now(
            Component::Init,
            EventKind::ManifestWarning(WarningEvent::new(warning.to_string())),
        ));
    }
    // §8.1: one `runtime` line saying what this boot will run. PHP's is the
    // probe's (below the fork, after `php_module_startup`); the other two are
    // decided here.
    emit_runtime(&emitter, &config, interpreter);

    let prepared = match blastit_env::build_from_process(&manifest, config.env_strip.as_deref()) {
        Ok(prepared) => prepared,
        Err(errors) => fail(&emitter, &errors.to_string()),
    };
    let layout = UidLayout::new(config.uid_base);
    // The rendered output variable *names* are what the `ready` event reports
    // (§8.1): they only exist here, after the rewrite, and stage 1 needs them.
    config.proxy_outputs = prepared
        .outputs()
        .iter()
        .map(|(name, names)| (name.as_str().to_string(), names.clone()))
        .collect();
    // D6: init is an ordinary uid in rootless mode, and the layout hands out
    // the uids of every other process. A collision would give a view init's
    // identity, so it is refused before any directory is created with it.
    if let Some(process) = layout_collision(&manifest, &layout, current_uid()) {
        fail(
            &emitter,
            &format!(
                "init runs as uid {}, which is the uid of {process} in the layout \
                 (BLASTIT_UID_BASE={}): run it as a uid below the layout, or raise \
                 BLASTIT_UID_BASE (D6)",
                current_uid(),
                config.uid_base
            ),
        );
    }

    let listeners = bind_view_sockets(&emitter, &manifest, &prepared, &layout, &serve_gates);
    let dbproxy = fork_dbproxy(
        &emitter,
        &config,
        &placement,
        interpreter,
        &prepared,
        listeners,
    );

    // The interpreter's import path travels in the configuration, not in the
    // environment: stage 1 is `execve`'d as an interpreter of its own (§4), and
    // a wheel re-executes it with `-I`, which drops `PYTHONPATH` and the
    // working directory. Empty for the standalone binary.
    config.sys_path = host.sys_path().to_vec();
    // §5.3: and so does the directory blastit was started in, which stage 1 puts
    // first on `sys.path` — the directory gunicorn, uwsgi and granian all make
    // importable, so a project that was never `pip install`ed serves its own
    // `wsgi:application`. It is read here, in stage 0, because that is the one
    // place that knows where the operator started the server.
    config.cwd = std::env::current_dir()
        .ok()
        .map(|dir| dir.into_os_string().into_vec());

    // Everything a secret touched is gone from the process image in one step.
    exec_stage1(
        &emitter,
        &config,
        host,
        &placement,
        prepared.app_env(),
        dbproxy,
    )
}

/// Shared stage-0 context of one `blastit dev` boot, once its [`DevRoot`]
/// exists (`R20`).
struct DevBoot<'a> {
    config: &'a Config,
    host: &'a Host,
    root: &'a DevRoot,
}

/// Fails a `blastit dev` boot with a diagnostic on stderr and exit `1`
/// (§1.1: a startup error writes zero bytes on stdout).
///
/// The root is removed by [`fail_dev_root`] or [`fail_with_root`] when one
/// exists; `_exit` runs no destructor, which is why every parent failure after
/// [`DevRoot::create`] goes through one of those helpers.
fn dev_fail(message: &str) -> ! {
    eprintln!("blastit dev: {message}");
    crate::coverage::flush();
    // SAFETY: the boot is aborting; `_exit` runs no handler.
    unsafe { libc::_exit(1) }
}

/// Removes `root` (`R20`) and aborts via [`dev_fail`].
fn fail_dev_root(root: &DevRoot, message: &str) -> ! {
    root.cleanup();
    dev_fail(message)
}

/// The sentence a `blastit dev` boot above `BLASTIT_MEM_HIGH_WATERMARK` fails
/// with, or `None` when a view process can start (§8 Sizing).
///
/// The reading is taken once, at boot, and is the one the scheduler will take:
/// [`mem_usage::read`] leaves out the page cache and slab the kernel reclaims
/// on demand, so a machine with a warm filesystem cache is not mistaken for a
/// machine without memory. The scheduler retires one idle view for memory
/// pressure and refuses to *spawn* one while aggregate usage is above the
/// watermark; with no view yet there is nothing to evict, so a boot here would
/// serve nothing and say nothing — every request would wait out
/// `BLASTIT_QUEUE_TIMEOUT` and answer `503`. A reading that cannot be taken is
/// not pressure, exactly as in the scheduler (§9.2 #8).
fn memory_reading() -> Option<mem_usage::Usage> {
    mem_usage::read(Path::new("/"))
}

/// [`memory_reading`]'s refusal, over the same reading the advisory uses.
fn memory_refusal(usage: &mem_usage::Usage, config: &Config) -> Option<String> {
    mem_usage::boot_refusal(usage, config.mem_high_watermark_percent)
}

/// Removes `root` (`R20`) and aborts via [`fail`] on `emitter`.
fn fail_with_root(emitter: &Emitter, root: &DevRoot, message: &str) -> ! {
    root.cleanup();
    fail(emitter, message)
}

/// `blastit_sys::pipe` for `dev` after [`DevRoot::create`]: cleans `root` on
/// failure before exiting (`R20`).
fn pipe_dev(emitter: &Emitter, root: &DevRoot, what: &str) -> (OwnedFd, OwnedFd) {
    match pipe() {
        Ok(pair) => pair,
        Err(err) => fail_with_root(emitter, root, &format!("{what}: {err}")),
    }
}

/// An argument or path as a [`CString`] after [`DevRoot::create`], cleaning
/// `root` if a NUL byte makes the re-exec impossible (`R20`).
fn cstring_dev(root: &DevRoot, value: impl AsRef<OsStr>) -> CString {
    match CString::new(value.as_ref().as_bytes().to_vec()) {
        Ok(value) => value,
        Err(_) => fail_dev_root(root, "an argument contains a NUL byte"),
    }
}

/// [`reexec_argv`] for `dev` after [`DevRoot::create`], cleaning `root` on any
/// NUL-byte failure (`R20`).
fn reexec_argv_dev(root: &DevRoot, reexec: &Reexec) -> Vec<CString> {
    let mut argv = vec![cstring_dev(root, &reexec.program)];
    argv.extend(reexec.prefix.iter().map(|arg| cstring_dev(root, arg)));
    argv
}

/// Runs `blastit dev`: the unprivileged single-uid simulation (`R19..R22`).
///
/// It is the same boot shape as [`run`] with three differences, all of them the
/// ones `ADR-0001` names. There is no capability detection and no `setresuid`:
/// every role runs as [`crate::dev_learn::euid`]. The runtime root is ephemeral
/// and `0700`, with one directory per route instead of one per view uid. And the
/// `execve` barrier's environment is the app environment itself — the only one
/// a single-uid simulation has — from which every `[proxy.*]` secret input,
/// every `BLASTIT_*` variable and every `BLASTIT_ENV_STRIP` name is already
/// gone.
///
/// `--learn` additionally hands dbproxy an observation pipe and an attribution
/// socket, and hands both to stage 1, which drains them and writes the manifest
/// on a clean `SIGTERM` (`R29`).
pub fn run_dev(host: &Host, options: &DevOptions, interpreter: bool) -> ! {
    // `S3`: a non-loopback bind has no authentication at all, so it needs the
    // second acknowledgement and says so before anything is written to stdout.
    if !crate::dev_learn::is_loopback(&options.host) && !crate::dev_learn::expose_requested() {
        eprintln!(
            "blastit dev: {}",
            crate::dev_learn::exposure_error(&options.host)
        );
        crate::coverage::flush();
        // SAFETY: nothing of blastit's is running yet; `_exit` runs no handler.
        unsafe { libc::_exit(1) }
    }
    let exposed = !crate::dev_learn::is_loopback(&options.host);

    // `R2`, `R10`: `pretty` by default and `BLASTIT_PRESET` never read.
    let mut config = match Config::for_dev() {
        Ok(config) => config,
        Err(err) => dev_fail(&err.to_string()),
    };
    config.port = options.port;
    if std::env::var_os("SERVER_PORT").is_none() {
        config.server_port = options.port;
    }
    // The privilege field is `root`/`capabilities` in the document; a dev boot
    // is neither, and stage 0 says `unprivileged-dev` in its own event below.
    config.privilege = None;
    let emitter = Emitter::stdout(config.log_format, config.log_level);
    // Every fatal path before the banner writes here, on descriptor 2: a
    // startup error has to leave stdout empty (§1.1).
    let startup = Emitter::new(2, config.log_format, config.log_level, false);

    // §8 Sizing: above `BLASTIT_MEM_HIGH_WATERMARK` the scheduler will not start
    // a view process, so a boot that ignores the reading serves nothing and says
    // nothing — every request waits out `BLASTIT_QUEUE_TIMEOUT` and answers
    // `503`. Refuse at boot instead, naming the setting and the reading (§1.1,
    // `docs/troubleshooting.md`). It is one reading for both the refusal and the
    // advisory below, so the two cannot disagree.
    let memory = memory_reading();
    if let Some(message) = memory
        .as_ref()
        .and_then(|usage| memory_refusal(usage, &config))
    {
        dev_fail(&message);
    }

    // The same order as `run`: the missing file first, the missing `app`
    // second (`docs/troubleshooting.md`), with §15.1's one sentence on
    // descriptor 2 — `dev` writes no startup error to stdout.
    let source = match crate::config::read_manifest(&config.manifest) {
        Ok(source) => source,
        Err(error) => dev_fail(&error.sentence(&config.manifest)),
    };
    let (manifest, warnings) = match Manifest::parse_blastfile(&source, config.uid_base) {
        Ok(parsed) => parsed,
        Err(errors) => dev_fail(&format!("{errors}")),
    };
    // `R13`: a site whose mounts answer every request needs no `app`; anything
    // else still names the setting as before (`MB.1.6`), and a `BLASTIT_APP`
    // that could never run is refused rather than ignored.
    //
    // Before it: `ADR-0009` §5's capability gate, so a python-less build says
    // "install the Python package: uv add blastit" (or "add a [route.static]
    // block") instead of sending the operator to a `BLASTIT_APP` that cannot
    // work there.
    #[cfg(not(feature = "python"))]
    if let Some(message) = no_python_support(&config, &manifest, options.learn) {
        dev_fail(&message);
    }
    // Same order as `run`: (1) the `workers` budget, (2) the `libphp`/shim
    // discovery. `dev` may also look in `BLASTIT_PHP_SHIM_DIR` and the
    // `target/php-sapi` directory `make php-sapi` writes (`R8`), which is the
    // only difference, and a host without a usable library is refused here
    // exactly as in a deployment (`ADR-0010` §1: one backend, no fallback).
    // Nothing has been forked or bound yet.
    #[cfg(not(feature = "php"))]
    if let Some(message) = no_php_support(&manifest) {
        dev_fail(&message);
    }
    #[cfg(feature = "php")]
    match php_boot_gate(&config, &manifest, PhpShimScope::Development) {
        PhpBoot::Absent => {}
        PhpBoot::Refused(message) => dev_fail(&message),
        PhpBoot::Proceed(runtime) => {
            // The line naming what was resolved is the probe's
            // (`php_view::probe_bindings`, `MB.25.2`): only the process that ran
            // `php_module_startup` can say which `php.ini` the workers read, and
            // it says so once the boot is known-good. A pre-banner lane would
            // have to keep that sentence on descriptor 2 and still could not
            // name the ini.
            //
            // Stage 1 re-opens these and every PHP worker is forked from the
            // binding built there (`ADR-0010` §3).
            config.php = Some(runtime);
        }
    }
    if let Err(message) = app_rule(&config, &manifest) {
        dev_fail(&message);
    }
    // `R13` steps 4–5: the root has to exist, be listable, and be readable by
    // the uid that answers, which in `dev` is the caller (`R19`).
    if let Err(errors) = manifest.check_static_roots(&config.manifest_dir) {
        dev_fail(&format!("{}: {errors}", config.manifest.display()));
    }
    if let Some(message) = static_root_permission(
        &manifest,
        &config.manifest_dir,
        crate::dev_learn::euid(),
        interpreter,
    ) {
        dev_fail(&message);
    }
    let prepared = match blastit_env::build_from_process(&manifest, config.env_strip.as_deref()) {
        Ok(prepared) => prepared,
        Err(errors) => dev_fail(&format!("{errors}")),
    };
    config.proxy_outputs = prepared
        .outputs()
        .iter()
        .map(|(name, names)| (name.as_str().to_string(), names.clone()))
        .collect();
    config.sys_path = host.sys_path().to_vec();
    config.cwd = std::env::current_dir()
        .ok()
        .map(|dir| dir.into_os_string().into_vec());

    let root = match DevRoot::create() {
        Ok(root) => root,
        Err(err) => dev_fail(&format!("runtime root: {err}")),
    };
    // Under `--learn`, slot `0` (`views/0`) is the dedicated unrouted-request
    // fallback view (`LEARN_FALLBACK_ROUTE`), and declared routes sit at
    // `1..=N` so no declared route ever shares a socket with the fallback view.
    let view_slots = manifest.routes().len() + usize::from(options.learn);
    if let Err(err) = root.create_view_dirs(view_slots) {
        fail_dev_root(&root, &format!("runtime root: {err}"));
    }
    let boot = DevBoot {
        config: &config,
        host,
        root: &root,
    };

    // `R20`: one listener per (route, proxy) the route may use — every declared
    // `postgresql` proxy under `--learn`, so one pass can observe them all.
    let listeners = match dev_view_sockets(&root, &manifest, &prepared, options.learn) {
        Ok(listeners) => listeners,
        Err(message) => fail_dev_root(&root, &message),
    };

    // `--learn`: dbproxy writes observations to `obs_write` (attributing each
    // statement directly from its per-route `ListenerSpec.route`) and stage 1
    // drains `obs_read`.
    let learn = if options.learn {
        let (obs_read, obs_write) = match pipe() {
            Ok(pair) => pair,
            Err(err) => fail_dev_root(&root, &format!("dev obs pipe: {err}")),
        };
        Some((obs_read, obs_write))
    } else {
        None
    };

    // The dev policy channel (`MB.23.5.3`, §4.6 step 2): one socketpair, stage
    // 1's end carried across the `execve` as `--dev-policy-fd` and dbproxy's end
    // in its configuration document. `--no-reload` creates neither end, exactly
    // like the watcher, the marks and the report sockets.
    let policy = if options.no_reload {
        None
    } else {
        Some(dev_policy_channel(&startup, &root))
    };

    let dbproxy = fork_dbproxy_dev(
        &startup,
        &boot,
        interpreter,
        &prepared,
        listeners,
        learn.as_ref().map(|(_, obs)| obs.as_raw_fd()),
        policy.as_ref().map(|(_, dbproxy)| dbproxy.as_raw_fd()),
    );
    // dbproxy has its own copy of its end now; the parent must not keep one, or
    // the channel would stay open after dbproxy exits.
    let policy = match policy {
        Some((stage1, dbproxy)) => {
            drop(dbproxy);
            Some(stage1)
        }
        None => None,
    };

    // The parent keeps the read end only.
    let learn = match learn {
        Some((obs_read, obs_write)) => {
            drop(obs_write);
            Some(obs_read)
        }
        None => None,
    };

    // Only now that the boot is known-good does anything reach stdout: the
    // banner, the boot event and the exposure warning (§1.2, §1.1).
    if config.log_format == LogFormat::Pretty {
        print!(
            "{}",
            crate::dev_learn::banner(&options.host, config.port, manifest.routes().len(), exposed)
        );
    }
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Boot(
            Lifecycle::default()
                .with_stage("stage0")
                .with_seccomp(config.seccomp)
                .with_privilege(PRIVILEGE),
        ),
    ));
    for warning in &warnings {
        emitter.emit(&Event::now(
            Component::Init,
            EventKind::ManifestWarning(WarningEvent::new(warning.to_string())),
        ));
    }
    // §8.1: one `runtime` line saying what this boot will run, now that the boot
    // is known-good (the banner and `boot` are already on stdout). PHP's line
    // comes from the probe below the fork; the other two are decided here.
    emit_runtime(&emitter, &config, interpreter);
    // A boot above the documented default is the operator's call once they
    // raised the knob, and it is also the state in which a view's first spawn
    // may have to wait for memory: say so once, with the reading and the cache
    // the reading left out (§1.1).
    if let Some(message) = memory
        .as_ref()
        .and_then(|usage| mem_usage::advisory(usage, config.mem_high_watermark_percent))
    {
        emitter.emit(
            &output_event(Component::Init, None, Stream::Stderr, &message).with_level(Level::Warn),
        );
    }
    if exposed {
        // Names the address, the port and the uid, and no event ever carries a
        // `port` *field* (§1.2, `R22`, `ARCHITECTURE.md` §8.1).
        emitter.emit(
            &output_event(
                Component::Init,
                None,
                Stream::Stderr,
                &format!(
                    "blastit dev is EXPOSED on {}:{} as uid {}: there is no authentication, \
                     so anyone who reaches it drives every route ({})",
                    options.host,
                    config.port,
                    crate::dev_learn::euid(),
                    crate::dev_learn::EXPOSE_VARIABLE
                ),
            )
            .with_level(Level::Warn),
        );
    }

    exec_stage1_dev(
        &boot,
        options.host,
        prepared.app_env(),
        dbproxy,
        learn,
        policy,
        options.no_reload,
    )
}

/// The dev-only dbproxy policy channel (`MB.23.5.3`, `ADR-0014` §4).
///
/// One `AF_UNIX`/`SOCK_STREAM` socketpair, created here and nowhere else: stage
/// 1's end travels across the `execve` as `--dev-policy-fd`, dbproxy's end
/// travels in its configuration document as `policy_fd` (legal only with
/// `dev_policy`). Both ends are close-on-exec at birth and switch to
/// non-blocking, because the supervisor writes its document through its poll set
/// and treats a short write as "wait for writability", never as a blocking call.
///
/// `--no-reload`, production, `check` and `test` never call this: the channel is
/// as dev-only as the watcher it exists to serve.
fn dev_policy_channel(emitter: &Emitter, root: &DevRoot) -> (OwnedFd, OwnedFd) {
    let (stage1, dbproxy) = match blastit_sys::socketpair_stream() {
        Ok(pair) => pair,
        Err(err) => fail_with_root(emitter, root, &format!("dev policy socket: {err}")),
    };
    for end in [&stage1, &dbproxy] {
        if let Err(err) = rustix::fs::fcntl_setfl(end, rustix::fs::OFlags::NONBLOCK) {
            fail_with_root(emitter, root, &format!("dev policy socket: {err}"));
        }
    }
    (stage1, dbproxy)
}

/// Binds the per-route sockets of a `dev` boot (`R20`).
///
/// In production the (route, proxy) pairs are the route's effective grants; in
/// `dev --learn` every declared `postgresql` proxy is bound for every route, so
/// one pass sees the statements a route would need grants for.
fn dev_view_sockets(
    root: &DevRoot,
    manifest: &Manifest,
    prepared: &blastit_env::Prepared,
    learn: bool,
) -> Result<Vec<(UnixListener, ListenerSpec)>, String> {
    let uid = crate::dev_learn::euid();
    let mut listeners: Vec<(UnixListener, ListenerSpec)> = Vec::new();
    if learn {
        for (proxy_index, (proxy_name, _upstream)) in prepared.upstreams().iter().enumerate() {
            let Some(port) = blastit_env::postgresql::local_port(proxy_index) else {
                return Err(format!(
                    "{}: no local port left for the postgresql proxies",
                    proxy_name.as_str()
                ));
            };
            let path = root.view_dir(0).join(format!(".s.PGSQL.{port}"));
            let listener = bind_unix_listener(&path, uid, 0o600)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            listeners.push((
                listener,
                ListenerSpec::enforcing(
                    crate::dev_learn::LEARN_FALLBACK_ROUTE,
                    proxy_name.as_str(),
                    uid,
                    Vec::new(),
                    false,
                    false,
                    Default::default(),
                ),
            ));
        }
    }
    let slot_offset = usize::from(learn);
    for (index, route) in manifest.routes().iter().enumerate() {
        let slot = index + slot_offset;
        for (proxy_index, (proxy_name, _upstream)) in prepared.upstreams().iter().enumerate() {
            let grant = manifest
                .effective_grants(index)
                .get(proxy_name)
                .and_then(|grant| grant.as_postgresql())
                .cloned();
            // A route with no declaration at all gets no socket. A route that
            // declares the proxy with an empty grant still gets one, so its
            // statements are denied with `42501` instead of the connection
            // failing with `ENOENT` (`docs/local.md`).
            if grant.is_none() && !learn && !route_declares_proxy(manifest, index, proxy_name) {
                continue;
            }
            let Some(port) = blastit_env::postgresql::local_port(proxy_index) else {
                return Err(format!(
                    "{}: no local port left for the postgresql proxies",
                    proxy_name.as_str()
                ));
            };
            let path = root.view_dir(slot).join(format!(".s.PGSQL.{port}"));
            // `SO_PASSCRED` and `0600`, owned by the one uid that may connect:
            // in `dev` that is the caller, the only identity a view has.
            let listener = bind_unix_listener(&path, uid, 0o600)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            let (queries, transaction_control, unrestricted, tables) = match &grant {
                Some(grant) => (
                    grant.queries().to_vec(),
                    grant.transaction_control(),
                    grant.is_unrestricted(),
                    grant.tables().clone(),
                ),
                None => (Vec::new(), false, false, Default::default()),
            };
            listeners.push((
                listener,
                ListenerSpec::enforcing(
                    route.name(),
                    proxy_name.as_str(),
                    uid,
                    queries,
                    transaction_control,
                    unrestricted,
                    tables,
                ),
            ));
        }
    }
    for (listener, spec) in &mut listeners {
        spec.fd = listener.as_raw_fd();
    }
    Ok(listeners)
}
/// Forks dbproxy for a `dev` boot (`R19`, `R21`).
///
/// The child is not `setresuid`'d: it already runs as the caller. Everything
/// else is the production sequence — empty `environ`, `PDEATHSIG`, no
/// `no_new_privs` exceptions, the role's seccomp filter — and the configuration
/// carries the `--learn` descriptors when the boot asked for them.
fn fork_dbproxy_dev(
    emitter: &Emitter,
    boot: &DevBoot<'_>,
    interpreter: bool,
    prepared: &blastit_env::Prepared,
    listeners: Vec<(UnixListener, ListenerSpec)>,
    obs_fd: Option<std::os::fd::RawFd>,
    policy_fd: Option<std::os::fd::RawFd>,
) -> Option<DbproxyHandover> {
    if listeners.is_empty() {
        return None;
    }
    let uid = crate::dev_learn::euid();
    let proxies: Vec<ProxySpec> = prepared
        .upstreams()
        .iter()
        .map(|(name, upstream)| ProxySpec {
            name: name.as_str().to_string(),
            host: upstream.host().to_string(),
            port: upstream.port(),
            user: upstream.user().to_string(),
            password: upstream.password().expose().to_string().into(),
            database: upstream.database().to_string(),
            sslmode: upstream.sslmode().map(str::to_string),
            ca_cert: upstream.ca_cert().map(|ca| ca.expose().to_string().into()),
            options: upstream.options().to_vec(),
            max_connections: boot.config.max_connections_for(name.as_str()),
        })
        .collect();
    let mut document = DbproxyConfig::enforcing(
        proxies,
        listeners.iter().map(|(_, spec)| spec.clone()).collect(),
    );
    document.mode = if obs_fd.is_some() {
        blastit_dbproxy::ListenerMode::Learn
    } else {
        blastit_dbproxy::ListenerMode::Enforce
    };
    // In `dev` the listener's uid is the caller's; root is only ever the caller
    // when the whole suite runs as root, and then it *is* the view's uid.
    document.allow_root_listener = uid == 0;
    document.obs_fd = obs_fd;
    // The dev policy channel's dbproxy end (`MB.23.5.3`): the document is the
    // only way dbproxy learns the descriptor, and `dev_policy` is what makes it
    // legal. Both are `None`/`false` on every boot this function is not called
    // for, which is what keeps a production dbproxy unable to acquire it.
    document.policy_fd = policy_fd;
    document.dev_policy = policy_fd.is_some();

    let (secrets_read, secrets_write) = pipe_dev(emitter, boot.root, "secrets.pipe");
    let (events_read, events_write) = pipe_dev(emitter, boot.root, "dbproxy events pipe");
    let (stdout_read, stdout_write) = pipe_dev(emitter, boot.root, "dbproxy stdout pipe");
    let (stderr_read, stderr_write) = pipe_dev(emitter, boot.root, "dbproxy stderr pipe");
    let (ready_read, ready_write) = pipe_dev(emitter, boot.root, "dbproxy ready pipe");

    let mut keep = vec![
        secrets_read.as_raw_fd(),
        events_write.as_raw_fd(),
        ready_write.as_raw_fd(),
    ];
    keep.extend(listeners.iter().map(|(listener, _)| listener.as_raw_fd()));
    if let Some(obs) = obs_fd {
        keep.push(obs);
    }
    if let Some(policy) = policy_fd {
        keep.push(policy);
    }

    let child = ChildIo {
        stdout: Some(stdout_write),
        stderr: Some(stderr_write),
        keep,
        limits: dbproxy_limits(boot.config),
        working_dir: None,
    };

    let forked = match crate::forking::fork(interpreter) {
        Ok(fork) => fork,
        Err(err) => fail_with_root(emitter, boot.root, &format!("fork: {err}")),
    };
    match forked {
        Fork::Parent(pid) => {
            drop(secrets_read);
            drop(events_write);
            drop(ready_write);
            drop(child);
            drop(listeners);

            let mut secrets = std::fs::File::from(secrets_write);
            if let Err(err) = write_config(&document, &mut secrets) {
                fail_with_root(emitter, boot.root, &format!("secrets.pipe: {err}"));
            }
            drop(secrets);

            if let Err(reason) = wait_ready(&ready_read, DBPROXY_READY_TIMEOUT) {
                emit_child_output(emitter, &stdout_read, Stream::Stdout);
                emit_child_output(emitter, &stderr_read, Stream::Stderr);
                fail_with_root(emitter, boot.root, &reason);
            }
            Some(DbproxyHandover {
                pid,
                events: events_read,
                stdout: stdout_read,
                stderr: stderr_read,
            })
        }
        Fork::Child => {
            let mut argv = reexec_argv(emitter, boot.host.dbproxy());
            argv.push(cstring(emitter, "--role=dbproxy"));
            argv.push(cstring(
                emitter,
                format!("--secrets-fd={}", secrets_read.as_raw_fd()),
            ));
            argv.push(cstring(
                emitter,
                format!("--events-fd={}", events_write.as_raw_fd()),
            ));
            argv.push(cstring(
                emitter,
                format!("--ready-fd={}", ready_write.as_raw_fd()),
            ));
            argv.push(cstring(
                emitter,
                format!("--seccomp={}", u8::from(boot.config.seccomp)),
            ));
            let outcome = child
                .clear_cloexec(&[])
                .and_then(|()| child.prepare())
                .and_then(|()| harden_child(Signal::KILL));
            if let Err(err) = outcome {
                fail(emitter, &format!("dbproxy child: {err}"));
            }
            let _ = set_dumpable(false);
            match current_signal_mask().and_then(restore_signal_mask_in_child) {
                Ok(()) => {}
                Err(err) => fail(emitter, &format!("dbproxy child: {err}")),
            }
            let err = exec(&program(emitter, boot.host.dbproxy()), &argv, &[]);
            fail(emitter, &format!("exec dbproxy: {err}"));
        }
    }
}

/// Crosses the `dev` `execve` barrier into stage 1 (`R20`).
fn exec_stage1_dev(
    boot: &DevBoot<'_>,
    host_addr: std::net::IpAddr,
    app_env: &BTreeMap<String, String>,
    dbproxy: Option<DbproxyHandover>,
    learn: Option<OwnedFd>,
    policy: Option<OwnedFd>,
    no_reload: bool,
) -> ! {
    let stage1 = boot.host.stage1();
    let mut document_value = match config_document(boot.config) {
        Ok(value) => value,
        Err(message) => fail_dev_root(boot.root, &message),
    };
    // The stage-1 program prefix a reload cycle re-execs (§4.6 step 6): the
    // standalone binary, or `python -I -m blastit` for a wheel. Stage 1 is the
    // only stage that can cycle and the only one that cannot see `Host`, so
    // stage 0 records the answer once here rather than let stage 1 guess from
    // `/proc/self/exe`.
    if let Some(object) = document_value.as_object_mut() {
        let mut restart: Vec<String> = vec![stage1.program.to_string_lossy().into_owned()];
        restart.extend(
            stage1
                .prefix
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned()),
        );
        object.insert(RESTART_MEMBER.to_string(), serde_json::Value::from(restart));
    }
    let document = match serde_json::to_vec(&document_value) {
        Ok(document) => document,
        Err(err) => fail_dev_root(boot.root, &format!("configuration document: {err}")),
    };
    if document.len() > MAX_CONFIG_DOCUMENT {
        fail_dev_root(
            boot.root,
            &format!(
                "the configuration document is {} bytes, more than the {MAX_CONFIG_DOCUMENT}-byte limit",
                document.len()
            ),
        );
    }
    let (config_read, config_write) = match pipe() {
        Ok(pair) => pair,
        Err(err) => fail_dev_root(boot.root, &format!("config pipe: {err}")),
    };
    let mut file = std::fs::File::from(config_write);
    if let Err(err) = file.write_all(&document) {
        fail_dev_root(boot.root, &format!("config pipe: {err}"));
    }
    drop(file);

    let mut argv = reexec_argv_dev(boot.root, stage1);
    argv.push(cstring_dev(boot.root, "--stage=1"));
    argv.push(cstring_dev(
        boot.root,
        format!("--config-fd={}", config_read.as_raw_fd()),
    ));
    argv.push(cstring_dev(
        boot.root,
        format!("--dev-root={}", boot.root.path().display()),
    ));
    argv.push(cstring_dev(boot.root, format!("--dev-host={host_addr}")));
    let mut keep = vec![config_read.as_raw_fd()];
    if let Some(dbproxy) = &dbproxy {
        argv.push(cstring_dev(
            boot.root,
            format!("--dbproxy-pid={}", dbproxy.pid.as_raw()),
        ));
        for (flag, fd) in [
            ("--dbproxy-events-fd", &dbproxy.events),
            ("--dbproxy-stdout-fd", &dbproxy.stdout),
            ("--dbproxy-stderr-fd", &dbproxy.stderr),
        ] {
            argv.push(cstring_dev(boot.root, format!("{flag}={}", fd.as_raw_fd())));
            keep.push(fd.as_raw_fd());
        }
    }
    if let Some(obs) = &learn {
        argv.push(cstring_dev(
            boot.root,
            format!("--dev-obs-fd={}", obs.as_raw_fd()),
        ));
        keep.push(obs.as_raw_fd());
    }
    // The dev policy channel's stage-1 end (`MB.23.5.3`): the supervisor writes
    // the document a `Blastfile` save produced and waits for the one-byte ack,
    // and carries this descriptor into the reload adoption document.
    if let Some(policy) = &policy {
        argv.push(cstring_dev(
            boot.root,
            format!("--dev-policy-fd={}", policy.as_raw_fd()),
        ));
        keep.push(policy.as_raw_fd());
    }
    // `--no-reload` is the user's flag and stage 1 is where the watcher would be
    // built, so this is the one token that carries it across the `execve`
    // (§4.1: off means no group, no mark, no hook, no report socket).
    if no_reload {
        argv.push(cstring_dev(boot.root, "--dev-reload=off"));
    }
    for fd in &keep {
        // SAFETY: every descriptor in `keep` is one this function created and
        // still owns.
        let borrowed = unsafe { BorrowedFd::borrow_raw(*fd) };
        if let Err(err) = set_cloexec(borrowed, false) {
            fail_dev_root(boot.root, &format!("config pipe: {err}"));
        }
    }

    let env: Vec<(String, String)> = app_env
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let err = exec(&cstring_dev(boot.root, &stage1.program), &argv, &env);
    fail_dev_root(boot.root, &format!("exec stage 1: {err}"))
}

/// Creates every view's private directory and binds the pg sockets it may use
/// (§2.2).
///
/// One listener per (route, `postgresql` proxy) the route names: its effective
/// grants, and the empty grants it declares explicitly (see
/// [`route_declares_proxy`]). The socket's port comes from `blastit_env`'s
/// per-proxy assignment, so every process derives the same number from the same
/// manifest (§5.2).
///
/// `_serve_gates` is the point of the signature (`MB.3.54`): it can only be
/// built by [`ServeGates::clear`], which runs [`raw_ingress_gate`], so no caller
/// can reach a bound view socket without the `ADR-0011` gate having run first.
/// The ordering is checked by the compiler — an unprivileged test cannot stand
/// where stage 0's refusal lives.
fn bind_view_sockets(
    emitter: &Emitter,
    manifest: &Manifest,
    prepared: &blastit_env::Prepared,
    layout: &UidLayout,
    _serve_gates: &ServeGates,
) -> Vec<(UnixListener, ListenerSpec)> {
    let mut listeners = Vec::new();
    for (index, route) in manifest.routes().iter().enumerate() {
        let uid = layout.view(index);
        let dir = view_dir(uid);
        // Every route gets a directory: a view `chdir`s into it and uses it as
        // `HOME` and `TMPDIR` (§5.3, §7.2).
        if let Err(err) = make_private_dir(&dir, uid, 0o700) {
            fail(emitter, &format!("{}: {err}", dir.display()));
        }
        // Effective grants first (the order the manifest's proxies sort in),
        // then the declared-but-empty ones, so a manifest that granted
        // something keeps exactly the sockets it had before.
        let effective = manifest.effective_grants(index);
        let mut wanted: Vec<(usize, &ProxyName, Option<&PgGrant>)> = Vec::new();
        for (proxy_name, grant) in &effective {
            let Some(pg) = grant.as_postgresql() else {
                continue;
            };
            let Some(proxy_index) = prepared
                .upstreams()
                .iter()
                .position(|(name, _)| name.as_str() == proxy_name.as_str())
            else {
                continue;
            };
            wanted.push((proxy_index, proxy_name, Some(pg)));
        }
        for (proxy_index, (proxy_name, _upstream)) in prepared.upstreams().iter().enumerate() {
            if effective.contains_key(proxy_name)
                || !route_declares_proxy(manifest, index, proxy_name)
            {
                continue;
            }
            wanted.push((proxy_index, proxy_name, None));
        }
        for (proxy_index, proxy_name, grant) in wanted {
            let Some(port) = blastit_env::postgresql::local_port(proxy_index) else {
                fail(
                    emitter,
                    &format!(
                        "{}: no local port left for the postgresql proxies",
                        proxy_name.as_str()
                    ),
                );
            };
            let path = dir.join(format!(".s.PGSQL.{port}"));
            let listener = match bind_unix_listener(&path, uid, 0o600) {
                Ok(listener) => listener,
                Err(err) => fail(emitter, &format!("{}: {err}", path.display())),
            };
            let (queries, transaction_control, unrestricted, tables) = match grant {
                Some(pg) => (
                    pg.queries().to_vec(),
                    pg.transaction_control(),
                    pg.is_unrestricted(),
                    pg.tables().clone(),
                ),
                None => (Vec::new(), false, false, Default::default()),
            };
            listeners.push((
                listener,
                ListenerSpec::enforcing(
                    route.name(),
                    proxy_name.as_str(),
                    uid,
                    queries,
                    transaction_control,
                    unrestricted,
                    tables,
                ),
            ));
        }
    }
    // The descriptor number of each listener is only known once the list is
    // built, and it is the number the child sees after the fork.
    for (listener, spec) in &mut listeners {
        spec.fd = listener.as_raw_fd();
    }
    listeners
}

/// Whether the route at `index` names `proxy` at all, even with an empty grant.
///
/// `Manifest::effective_grants` drops an empty grant, because it grants nothing;
/// the documentation still promises the route a listener that denies every
/// statement (`docs/local.md`: "`queries = []` gives it a listener that denies
/// every statement"), so the *declaration* is what decides the socket. A route
/// with no `[route.<proxy>]` and no group that names the proxy still gets no
/// listener, which is the other half of that page.
pub(crate) fn route_declares_proxy(manifest: &Manifest, index: usize, proxy: &ProxyName) -> bool {
    let Some(route) = manifest.routes().get(index) else {
        return false;
    };
    if route.grants().contains_key(proxy) {
        return true;
    }
    manifest
        .global_groups()
        .iter()
        .chain(route.groups().iter())
        .filter_map(|name| manifest.groups().get(name))
        .any(|group| group.grants().contains_key(proxy))
}

/// Forks dbproxy, hands it the secrets and waits for its ready byte (§4).
///
/// Returns the pipes stage 1 needs to collect dbproxy's logs, or `None` when
/// there was no listener to serve.
fn fork_dbproxy(
    emitter: &Emitter,
    config: &Config,
    placement: &Placement,
    interpreter: bool,
    prepared: &blastit_env::Prepared,
    listeners: Vec<(UnixListener, ListenerSpec)>,
) -> Option<DbproxyHandover> {
    if listeners.is_empty() {
        return None;
    }

    // §7.2: dbproxy is `execve`'d as `BLASTIT_UID_BASE + 3`, so the child has
    // to be able to *reach* the file before it is forked. A wheel installed
    // under a directory the dropped-to uids cannot pass — a checkout inside a
    // `0750` home, say — otherwise fails the boot with `exec dbproxy: Permission
    // denied (os error 13)`, which names neither the directory nor the fix.
    let layout = UidLayout::new(config.uid_base);
    // The route count is not needed here: the execute question never reads the
    // sticky exemption, which is the only thing the uid list is for.
    match crate::visibility::unexecutable(
        std::slice::from_ref(&placement.dbproxy),
        layout.dbproxy(),
        interpreter,
        &crate::visibility::DroppedTo::of(layout, 0),
    ) {
        Ok(Some(blocker)) => fail(emitter, &blocker.message()),
        Ok(None) => {}
        Err(timed_out) => fail(emitter, &timed_out.message()),
    }

    let proxies: Vec<ProxySpec> = prepared
        .upstreams()
        .iter()
        .map(|(name, upstream)| ProxySpec {
            name: name.as_str().to_string(),
            host: upstream.host().to_string(),
            port: upstream.port(),
            user: upstream.user().to_string(),
            password: upstream.password().expose().to_string().into(),
            database: upstream.database().to_string(),
            sslmode: upstream.sslmode().map(str::to_string),
            ca_cert: upstream.ca_cert().map(|ca| ca.expose().to_string().into()),
            options: upstream.options().to_vec(),
            max_connections: config.max_connections_for(name.as_str()),
        })
        .collect();
    let document = DbproxyConfig::enforcing(
        proxies,
        listeners.iter().map(|(_, spec)| spec.clone()).collect(),
    );

    let (secrets_read, secrets_write) = pipe_ok(emitter, "secrets.pipe");
    let (events_read, events_write) = pipe_ok(emitter, "dbproxy events pipe");
    let (stdout_read, stdout_write) = pipe_ok(emitter, "dbproxy stdout pipe");
    let (stderr_read, stderr_write) = pipe_ok(emitter, "dbproxy stderr pipe");
    let (ready_read, ready_write) = pipe_ok(emitter, "dbproxy ready pipe");

    let mut keep = vec![
        secrets_read.as_raw_fd(),
        events_write.as_raw_fd(),
        ready_write.as_raw_fd(),
    ];
    keep.extend(listeners.iter().map(|(listener, _)| listener.as_raw_fd()));

    let child = ChildIo {
        stdout: Some(stdout_write),
        stderr: Some(stderr_write),
        keep,
        limits: dbproxy_limits(config),
        working_dir: None,
    };
    let uid = UidLayout::new(config.uid_base).dbproxy();

    match fork_ok(emitter, interpreter) {
        Fork::Parent(pid) => {
            // The parent keeps only the read ends, and neither end of the pg
            // listeners: dbproxy owns those after the fork.
            drop(secrets_read);
            drop(events_write);
            drop(ready_write);
            drop(child);
            drop(listeners);

            let mut secrets = std::fs::File::from(secrets_write);
            if let Err(err) = write_config(&document, &mut secrets) {
                fail(emitter, &format!("secrets.pipe: {err}"));
            }
            drop(secrets);

            if let Err(reason) = wait_ready(&ready_read, DBPROXY_READY_TIMEOUT) {
                // Whatever dbproxy said on the way out is in the pipes init is
                // holding for stage 1, and a boot that fails with nothing but
                // "exited" is a boot nobody can debug (§8.1). On a *timeout* the
                // child is still alive and still holds the write ends, which is
                // why the drain is bounded (see `emit_child_output`).
                emit_child_output(emitter, &stdout_read, Stream::Stdout);
                emit_child_output(emitter, &stderr_read, Stream::Stderr);
                fail(emitter, &reason);
            }
            Some(DbproxyHandover {
                pid,
                events: events_read,
                stdout: stdout_read,
                stderr: stderr_read,
            })
        }
        Fork::Child => {
            // D6/B2: the *checked* program, not `host.dbproxy()`: a standalone
            // install whose own file carries the capabilities must run this role
            // as the capability-free copy, or the `execve` itself fails.
            let mut argv = reexec_argv(emitter, &Reexec::program(&placement.dbproxy));
            argv.push(cstring(emitter, "--role=dbproxy"));
            argv.push(cstring(
                emitter,
                format!("--secrets-fd={}", secrets_read.as_raw_fd()),
            ));
            argv.push(cstring(
                emitter,
                format!("--events-fd={}", events_write.as_raw_fd()),
            ));
            argv.push(cstring(
                emitter,
                format!("--ready-fd={}", ready_write.as_raw_fd()),
            ));
            // dbproxy is exec'd with an empty environment (§4), so §7.3's
            // `BLASTIT_SECCOMP` reaches it as an argument instead. A wheel
            // install hands the same argument to the Python-free
            // `blastit-dbproxy` binary it ships, which takes the same flags.
            argv.push(cstring(
                emitter,
                format!("--seccomp={}", u8::from(config.seccomp)),
            ));
            let outcome = child
                .clear_cloexec(&[])
                .and_then(|()| child.prepare())
                .and_then(|()| blastit_sys::drop_privileges(uid))
                .and_then(|()| harden_child(Signal::KILL));
            if let Err(err) = outcome {
                fail(emitter, &format!("dbproxy child: {err}"));
            }
            let _ = set_dumpable(false);
            match current_signal_mask().and_then(restore_signal_mask_in_child) {
                Ok(()) => {}
                Err(err) => fail(emitter, &format!("dbproxy child: {err}")),
            }
            // An empty environment: dbproxy's `/proc/<pid>/environ` is empty,
            // and the credentials arrive over the pipe it already holds (§4).
            let err = exec(
                &program(emitter, &Reexec::program(&placement.dbproxy)),
                &argv,
                &[],
            );
            fail(emitter, &format!("exec dbproxy: {err}"));
        }
    }
}

/// What stage 1 needs from the dbproxy handover (§4).
pub struct DbproxyHandover {
    /// dbproxy's pid, so stage 1 can tell its exit from a view's.
    pub pid: Pid,
    /// The read end of dbproxy's events pipe.
    pub events: OwnedFd,
    /// The read end of dbproxy's stdout pipe.
    pub stdout: OwnedFd,
    /// The read end of dbproxy's stderr pipe.
    pub stderr: OwnedFd,
}

/// Waits for dbproxy's ready byte, with a deadline (§4).
///
/// `timeout` is the whole budget, and it is a parameter so that a test can watch
/// it expire in milliseconds instead of waiting out [`DBPROXY_READY_TIMEOUT`].
///
/// # Errors
///
/// A message saying how the wait ended — the deadline, a failed `poll`, or the
/// child exiting first. The caller prints it together with whatever the child
/// left in its output pipes.
fn wait_ready(ready: &OwnedFd, timeout: Duration) -> Result<(), String> {
    let deadline = Deadline::after(timeout);
    loop {
        if deadline.expired() {
            return Err(format!(
                "dbproxy did not report ready within {}s",
                timeout.as_secs_f32()
            ));
        }
        match wait_readable(ready.as_fd(), deadline.left()) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => return Err(format!("waiting for dbproxy: {err}")),
        }
        match read_one(ready.as_fd()) {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => return Err("dbproxy exited before it was ready".to_string()),
            Err(err) => return Err(format!("waiting for dbproxy: {err}")),
        }
    }
}

/// Prints what a child that died before reporting ready left in one of its
/// output pipes, as `app.output` events (§8.1).
///
/// This is the only place init reads a child's stdout or stderr itself: normally
/// stage 1 collects both, but a dbproxy that failed to start never gets that
/// far, and the reason it failed — a missing binary, a permission a dropped uid
/// does not have — is nothing but its own last words.
///
/// # Why the reads are non-blocking and bounded
///
/// A drain only *starts* once the wait for dbproxy's ready byte has failed, and
/// one of those failures is the timeout: there the child is very much alive and
/// still holds the write ends, so a plain read to `EOF` would block init
/// forever — turning a failure into a hang. The pipe is therefore switched to
/// non-blocking, and whatever has not arrived when [`CHILD_OUTPUT_TIMEOUT`] is
/// spent is not worth waiting for: init is on its way out.
fn emit_child_output(emitter: &Emitter, fd: &OwnedFd, stream: Stream) {
    /// Largest amount of a failed child's output init will log.
    const MAX: usize = 8 * 1024;

    let deadline = Deadline::after(CHILD_OUTPUT_TIMEOUT);
    // Best effort: a pipe that cannot be switched is still read to its current
    // end, and the deadline below bounds that too.
    let _ = rustix::fs::fcntl_setfl(fd, rustix::fs::OFlags::NONBLOCK);
    let mut bytes = vec![0_u8; MAX];
    let mut total = 0;
    while total < MAX {
        match rustix::io::read(fd, &mut bytes[total..]) {
            Ok(0) => break,
            Ok(count) => total += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => {
                // Nothing there *yet*: the child may still be alive and about to
                // say something, so wait for it — but not past the deadline.
                if deadline.expired()
                    || !wait_readable(fd.as_fd(), deadline.left()).unwrap_or(false)
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    bytes.truncate(total);
    for line in String::from_utf8_lossy(&bytes).lines() {
        emitter
            .emit(&output_event(Component::Dbproxy, None, stream, line).with_level(Level::Error));
    }
}

/// Crosses the `execve` barrier into stage 1 (§4).
///
/// The configuration is written into a pipe — it holds no secret, but it does
/// name every proxy variable — and exactly the descriptors stage 1 needs get
/// their `FD_CLOEXEC` cleared. Nothing secret is in the arguments or the
/// environment: `envp` is the app environment, from which every `BLASTIT_*`
/// variable, every proxy input and everything on `BLASTIT_ENV_STRIP` is already
/// gone.
fn exec_stage1(
    emitter: &Emitter,
    config: &Config,
    host: &Host,
    placement: &Placement,
    app_env: &BTreeMap<String, String>,
    dbproxy: Option<DbproxyHandover>,
) -> ! {
    let document_value = match config_document(config) {
        Ok(value) => value,
        Err(message) => fail(emitter, &message),
    };
    let document = match serde_json::to_vec(&document_value) {
        Ok(document) => document,
        Err(err) => fail(emitter, &format!("configuration document: {err}")),
    };
    if document.len() > MAX_CONFIG_DOCUMENT {
        fail(
            emitter,
            &format!(
                "the configuration document is {} bytes, more than the {MAX_CONFIG_DOCUMENT}-byte limit",
                document.len()
            ),
        );
    }
    let (config_read, config_write) = pipe_ok(emitter, "config pipe");
    let mut file = std::fs::File::from(config_write);
    if let Err(err) = file.write_all(&document) {
        fail(emitter, &format!("config pipe: {err}"));
    }
    drop(file);

    // D6/B1, B2: init's own image — the file a deployment may have given the
    // capabilities to. Stage 1 forks the children and drops their privileges,
    // so it needs them, and this `execve` comes from stage 0, whose bounding
    // set is still whole: the file capabilities apply.
    let stage1 = Reexec {
        program: placement.init.clone(),
        prefix: host.stage1().prefix.clone(),
    };
    let mut argv = reexec_argv(emitter, &stage1);
    argv.push(cstring(emitter, "--stage=1"));
    argv.push(cstring(
        emitter,
        format!("--config-fd={}", config_read.as_raw_fd()),
    ));
    let mut keep = vec![config_read.as_raw_fd()];
    if let Some(dbproxy) = &dbproxy {
        argv.push(cstring(
            emitter,
            format!("--dbproxy-pid={}", dbproxy.pid.as_raw()),
        ));
        for (flag, fd) in [
            ("--dbproxy-events-fd", &dbproxy.events),
            ("--dbproxy-stdout-fd", &dbproxy.stdout),
            ("--dbproxy-stderr-fd", &dbproxy.stderr),
        ] {
            argv.push(cstring(emitter, format!("{flag}={}", fd.as_raw_fd())));
            keep.push(fd.as_raw_fd());
        }
    }
    for fd in &keep {
        // SAFETY: every descriptor in `keep` is one this function created and
        // still owns.
        let borrowed = unsafe { BorrowedFd::borrow_raw(*fd) };
        if let Err(err) = set_cloexec(borrowed, false) {
            fail(emitter, &format!("config pipe: {err}"));
        }
    }

    let env: Vec<(String, String)> = app_env
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let err = exec(&program(emitter, &stage1), &argv, &env);
    fail(emitter, &format!("exec stage 1: {err}"))
}

/// The program and its prefix, as the `argv` a re-exec starts with (§4).
///
/// `argv[0]` is the program itself, as `execve`'s callers conventionally do: for
/// the standalone binary that is `/proc/self/exe`, and for a wheel install it is
/// the interpreter — which is also what CPython wants there, because it reads
/// `argv[0]` to find the interpreter whose `site-packages` the app needs.
fn reexec_argv(emitter: &Emitter, reexec: &Reexec) -> Vec<CString> {
    let mut argv = vec![cstring(emitter, &reexec.program)];
    argv.extend(reexec.prefix.iter().map(|arg| cstring(emitter, arg)));
    argv
}

/// The `execve` path of a re-exec, as a C string.
///
/// # Panics
///
/// Never: a path with a NUL byte in it cannot be passed to `execve`, so
/// [`cstring`] turns it into a fatal error with a message instead.
fn program(emitter: &Emitter, reexec: &Reexec) -> CString {
    cstring(emitter, &reexec.program)
}

/// An argument, or a path, as a C string, or a fatal error naming what is
/// wrong.
fn cstring(emitter: &Emitter, value: impl AsRef<OsStr>) -> CString {
    match CString::new(value.as_ref().as_bytes().to_vec()) {
        Ok(value) => value,
        Err(_) => fail(emitter, "an argument contains a NUL byte"),
    }
}

/// `blastit_sys::pipe`, fatal on failure.
fn pipe_ok(emitter: &Emitter, what: &str) -> (OwnedFd, OwnedFd) {
    match pipe() {
        Ok(pipe) => pipe,
        Err(err) => fail(emitter, &format!("{what}: {err}")),
    }
}

/// `fork()`, fatal on failure.
///
/// With a live interpreter in this image the fork runs under CPython's protocol
/// ([`crate::forking`]): a wheel's stage 0 forks dbproxy from inside the
/// console script's interpreter, and §9.3 applies to it as much as to a view.
fn fork_ok(emitter: &Emitter, interpreter: bool) -> Fork {
    match crate::forking::fork(interpreter) {
        Ok(fork) => fork,
        Err(err) => fail(emitter, &format!("fork: {err}")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::Read;
    use std::time::Instant;

    use super::*;

    /// Everything an emitter wrote to `read_end`, once its peer is closed.
    fn captured(fd: OwnedFd) -> String {
        let mut file = std::fs::File::from(fd);
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    /// Drains `child_output` and returns what was logged, with how long the
    /// drain itself took.
    fn drain(child_output: &OwnedFd, stream: Stream) -> (String, Duration) {
        let (capture_read, capture_write) = pipe().unwrap();
        let emitter = Emitter::new(
            capture_write.as_raw_fd(),
            LogFormat::Json,
            Level::Info,
            false,
        );
        let started = Instant::now();
        emit_child_output(&emitter, child_output, stream);
        let elapsed = started.elapsed();
        drop(capture_write);
        (captured(capture_read), elapsed)
    }

    /// The read end of a pipe whose write end the caller keeps for itself.
    fn output_pipe() -> (OwnedFd, OwnedFd) {
        pipe().unwrap()
    }

    /// A manifest with `routes` routes, in the order they matter here.
    fn manifest_with(routes: usize) -> Manifest {
        let mut source = String::new();
        for index in 0..routes {
            source.push_str(&format!("view r{index} \"/r{index}/\" {{}}\n"));
        }
        Manifest::parse_blastfile(&source, 20_000).unwrap().0
    }

    #[test]
    fn the_layout_collision_check_knows_every_process_it_hands_a_uid_to() {
        // D6: init is an ordinary uid in rootless mode, so its own uid must not
        // be one the layout gives away.
        let manifest = manifest_with(3);
        let layout = UidLayout::new(20_000);
        assert_eq!(
            layout_collision(&manifest, &layout, 20_001).unwrap(),
            "the http process"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 20_002).unwrap(),
            "the scheduler"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 20_003).unwrap(),
            "dbproxy"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 21_000).unwrap(),
            "route `r0`"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 21_002).unwrap(),
            "route `r2`"
        );
    }

    #[test]
    fn a_uid_outside_the_layout_is_free() {
        let manifest = manifest_with(2);
        let layout = UidLayout::new(20_000);
        // The ordinary case: a service uid, and root.
        assert!(layout_collision(&manifest, &layout, 0).is_none());
        assert!(layout_collision(&manifest, &layout, 1000).is_none());
        // A view uid of a route this manifest does not have is free too.
        assert!(layout_collision(&manifest, &layout, 21_002).is_none());
        // And the whole check disappears when the base is a high, private one.
        let layout = UidLayout::new(100_000);
        assert!(layout_collision(&manifest, &layout, 20_001).is_none());
        assert_eq!(
            layout_collision(&manifest, &layout, 101_001).unwrap(),
            "route `r1`"
        );
    }

    #[test]
    fn the_stage_zero_boot_event_says_the_mode() {
        let root = stage0_lifecycle(Some(&Privilege {
            mode: Mode::Root,
            capabilities: Vec::new(),
            bounding_set_dropped: true,
        }));
        assert_eq!(root.stage(), Some("stage0"));
        assert_eq!(root.privilege(), Some("root"));
        assert!(root.capabilities().is_empty());
        assert_eq!(root.bounding_set(), Some(true));

        let rootless = stage0_lifecycle(Some(&Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
            bounding_set_dropped: false,
        }));
        assert_eq!(rootless.privilege(), Some("capabilities"));
        assert_eq!(rootless.capabilities(), ["cap_chown", "cap_setuid"]);
        assert_eq!(
            rootless.bounding_set(),
            Some(false),
            "M1: a deployment without cap_setpcap says so"
        );

        // A boot that could not read its own capability sets says nothing about
        // the mode rather than guessing one.
        let unknown = stage0_lifecycle(None);
        assert_eq!(unknown.stage(), Some("stage0"));
        assert_eq!(unknown.privilege(), None);
        assert_eq!(unknown.bounding_set(), None);
    }

    #[test]
    fn the_configuration_boot_event_says_the_mode_and_the_preset() {
        // E1: the first `boot` line is printed before the configuration is
        // read, so this is the one that says what the boot is: the mode init
        // detected, whether the filters are on, and which preset supplied which
        // defaults.
        let privilege = Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_setuid".to_string()],
            bounding_set_dropped: true,
        };
        let mut config = Config::default();
        assert_eq!(
            config_lifecycle(&privilege, &config).stage(),
            Some("config")
        );
        assert_eq!(
            config_lifecycle(&privilege, &config).preset(),
            None,
            "no preset: the event says null, which the encoder writes"
        );

        config.preset = Some(crate::config::PresetChoice {
            name: "compose-traefik".to_string(),
            supplied: std::collections::BTreeMap::from([(
                "BLASTIT_CLIENT_IP_HEADER".to_string(),
                "x-forwarded-for".to_string(),
            )]),
            overridden: vec!["BLASTIT_LOG_FORMAT".to_string()],
        });
        let lifecycle = config_lifecycle(&privilege, &config);
        assert_eq!(lifecycle.stage(), Some("config"));
        assert_eq!(lifecycle.privilege(), Some("capabilities"));
        assert_eq!(lifecycle.capabilities(), ["cap_setuid"]);
        assert_eq!(lifecycle.seccomp(), Some(true), "the effective setting");
        assert_eq!(lifecycle.preset(), Some("compose-traefik"));
        assert_eq!(
            lifecycle.preset_settings().get("BLASTIT_CLIENT_IP_HEADER"),
            Some(&"x-forwarded-for".to_string())
        );
        assert_eq!(lifecycle.preset_overridden(), ["BLASTIT_LOG_FORMAT"]);

        // The filters being off is the configuration's word, not the
        // variable's: a `boot` line that disagrees with the running server is
        // the failure this field exists to prevent (§7.3).
        config.seccomp = false;
        assert_eq!(config_lifecycle(&privilege, &config).seccomp(), Some(false));
    }

    #[test]
    fn the_detected_mode_becomes_the_boot_field() {
        let root = privilege_from(&blastit_sys::Held {
            mode: blastit_sys::PrivilegeMode::Root,
            capabilities: Vec::new(),
            bounding_set_dropped: true,
        });
        assert_eq!(root.mode, Mode::Root);
        assert!(root.capabilities.is_empty());
        assert!(root.bounding_set_dropped);
        let rootless = privilege_from(&blastit_sys::Held {
            mode: blastit_sys::PrivilegeMode::Capabilities,
            capabilities: vec!["cap_chown", "cap_setuid"],
            bounding_set_dropped: false,
        });
        assert_eq!(rootless.mode, Mode::Capabilities);
        assert_eq!(rootless.capabilities, ["cap_chown", "cap_setuid"]);
        assert!(!rootless.bounding_set_dropped);
        assert_eq!(rootless.mode.as_str(), "capabilities");
    }
    /// A dbproxy that printed something and then hung holds its write end open:
    /// the drain has to read what it said without waiting for the process.
    #[test]
    fn a_live_child_that_printed_is_still_logged() {
        let (read_end, write_end) = output_pipe();
        rustix::io::write(&write_end, b"exec dbproxy: Permission denied\n").unwrap();

        let (text, elapsed) = drain(&read_end, Stream::Stdout);
        assert!(
            elapsed < Duration::from_secs(2),
            "the drain waited for a live child: {elapsed:?}"
        );
        assert!(text.contains("Permission denied"), "{text}");
        assert!(text.contains(r#""component":"dbproxy""#), "{text}");
        assert!(text.contains(r#""event":"app.output""#), "{text}");
    }

    /// And a live child that said *nothing* must not block the boot either: this
    /// is the timeout case, where the process is still alive and still holds the
    /// write end of both pipes.
    #[test]
    fn a_silent_live_child_does_not_block_the_drain() {
        let (read_end, _write_end) = output_pipe();

        let (text, elapsed) = drain(&read_end, Stream::Stderr);
        assert!(
            elapsed < Duration::from_secs(2),
            "the drain hung: {elapsed:?}"
        );
        assert!(text.is_empty(), "nothing was written: {text}");
    }

    /// The ordinary failure: dbproxy is gone, and what it printed on the way out
    /// is the only explanation of why.
    #[test]
    fn a_dead_child_output_becomes_app_output_events() {
        let (read_end, write_end) = output_pipe();
        rustix::io::write(&write_end, b"first\nsecond\n").unwrap();
        drop(write_end);

        let (text, _) = drain(&read_end, Stream::Stdout);
        assert!(text.contains("first"), "{text}");
        assert!(text.contains("second"), "{text}");
    }

    /// The wait itself: a dbproxy that never writes must expire the deadline —
    /// and quickly, which is why the timeout is a parameter.
    #[test]
    fn a_dbproxy_that_never_reports_ready_times_out() {
        let (ready, _write_end) = output_pipe();
        let started = Instant::now();
        let err = wait_ready(&ready, Duration::from_millis(50)).unwrap_err();
        assert!(err.contains("did not report ready within"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the deadline was not honoured: {:?}",
            started.elapsed()
        );
    }

    /// And a dbproxy that died first is reported as that, not as a timeout.
    #[test]
    fn a_dbproxy_that_exits_before_ready_is_reported_as_such() {
        let (ready, write_end) = output_pipe();
        drop(write_end);
        let err = wait_ready(&ready, Duration::from_secs(30)).unwrap_err();
        assert_eq!(err, "dbproxy exited before it was ready");
    }

    /// The arithmetic and the wording live in [`crate::mem_usage`]; this pins
    /// the wiring: `memory_refusal` compares the reading against *the config's*
    /// `BLASTIT_MEM_HIGH_WATERMARK`, the same ceiling the scheduler uses.
    #[test]
    fn memory_refusal_takes_its_ceiling_from_the_config() {
        let gib = 1024 * 1024 * 1024;
        let mut config = Config::for_dev().expect("a dev config");
        config.mem_high_watermark_percent = 85.0;
        let reading = |used: u64| crate::mem_usage::Usage::new(100 * gib, used, 0);
        assert!(memory_refusal(&reading(80 * gib), &config).is_none());
        assert!(
            memory_refusal(&reading(85 * gib), &config).is_none(),
            "the watermark is a ceiling, not a trigger"
        );
        let message = memory_refusal(&reading(86 * gib), &config).expect("above the watermark");
        assert!(message.contains("BLASTIT_MEM_HIGH_WATERMARK"), "{message}");
        assert!(message.contains("(85%)"), "the knob: {message}");
        assert!(message.contains("503"), "the symptom: {message}");
        // A 0% watermark can never be met: it is how a test forces the high arm
        // without faking `/proc`.
        config.mem_high_watermark_percent = 0.0;
        assert!(memory_refusal(&reading(1), &config).is_some());
        // An unreadable or empty reading is not pressure.
        assert!(memory_refusal(&reading(0), &config).is_none());
    }

    #[test]
    fn an_explicit_empty_grant_still_names_the_proxy_for_its_route() {
        let source = "\
postgres $DATABASE_URL

trait denied {
    postgres {}
}

view locked regex '/locked/.*' {
    postgres {}
}

view grouped regex '/grouped/.*' {
    use denied
}

view ungranted regex '/ungranted/.*' {}
";
        let (manifest, _) = Manifest::parse_blastfile(source, 20_000).unwrap();
        let db = ProxyName::new("db");
        assert!(
            route_declares_proxy(&manifest, 0, &db),
            "an inline `queries = []` is a declaration"
        );
        assert!(
            route_declares_proxy(&manifest, 1, &db),
            "an attached group that names the proxy is a declaration"
        );
        assert!(
            !route_declares_proxy(&manifest, 2, &db),
            "a route that names no proxy gets no listener"
        );
        // The denied twin: `effective_grants` is what drops the empty grant, so
        // the two functions disagree on `locked` on purpose.
        assert!(
            !manifest.effective_grants(0).contains_key(&db),
            "the manifest model still calls an empty grant empty"
        );
    }

    /// `MB.23.5.3`: the dev policy channel is one `AF_UNIX`/`SOCK_STREAM`
    /// socketpair, both ends close-on-exec and non-blocking, and the two ends
    /// really are one connected pair.
    #[test]
    fn the_dev_policy_channel_is_one_nonblocking_socketpair() {
        let emitter = Emitter::new(2, LogFormat::Json, Level::Info, false);
        let root = DevRoot::create().expect("a runtime root");
        let (stage1, dbproxy) = dev_policy_channel(&emitter, &root);
        for (name, end) in [("stage 1", &stage1), ("dbproxy", &dbproxy)] {
            let status = rustix::fs::fcntl_getfl(end).expect("the status flags");
            assert!(
                status.contains(rustix::fs::OFlags::NONBLOCK),
                "the {name} end is non-blocking: the supervisor writes through its poll set"
            );
            let flags = rustix::io::fcntl_getfd(end).expect("the descriptor flags");
            assert!(
                flags.contains(rustix::io::FdFlags::CLOEXEC),
                "the {name} end is close-on-exec at birth"
            );
        }
        // One connected pair: a byte written on one end is readable on the other.
        rustix::io::write(&stage1, b"x").expect("written");
        let mut byte = [0u8; 1];
        assert_eq!(rustix::io::read(&dbproxy, &mut byte).expect("read"), 1);
        assert_eq!(byte[0], b'x');
        root.cleanup();
    }

    /// `MB.25.2`: the `runtime` line's decision table. A resolved library is the
    /// probe's to report (only it ran `php_module_startup`), no callable means
    /// static mounts and no language runtime, and a callable is only nameable
    /// where an interpreter is already live — a wrong "yes" answers no line, not
    /// a panic, and never an invented version.
    #[test]
    fn the_runtime_shape_is_decided_by_what_the_boot_loads() {
        assert_eq!(
            runtime_shape(true, true, true),
            None,
            "PHP's line is the probe's, never stage 0's"
        );
        assert_eq!(
            runtime_shape(false, false, false),
            Some(Runtime::Static),
            "no library and no callable is a static-only file"
        );
        assert_eq!(
            runtime_shape(false, true, false),
            None,
            "no interpreter here: there is nothing honest to say"
        );
    }
}

/// `ADR-0009` §5: the boot gate of a build with no Python support (`MB.3.11`).
///
/// What it allows is as much a part of the gate as what it refuses: a
/// static-only manifest has to boot in the standalone archive, or the artifact
/// serves nothing at all.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod no_python_capability {
    use super::*;

    /// A static-only manifest: one `static` mount, no app.
    const STATIC_ONLY: &str = "\
static \"/\" {
    root \"site\"
}
";

    /// One route that declares no executor of its own: a WSGI view.
    const WITH_A_VIEW: &str = "\
view web regex '.*' {}
";

    /// One PHP route and no app: its own executor (`ADR-0010` §2), so this
    /// build can serve it without an interpreter anywhere in the image.
    const PHP_ONLY: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

    /// The page's shape: files from disk, everything else to the front
    /// controller. Still no interpreter.
    const STATIC_AND_PHP: &str = "\
entrypoint php \"public/index.php\"

static \"/\" {
    root \"public\"
}

view app regex '.*' {}
";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        manifest
    }

    #[test]
    fn a_static_only_boot_is_allowed() {
        assert_eq!(
            no_python_support(&Config::default(), &manifest(STATIC_ONLY), false),
            None
        );
    }

    #[test]
    fn an_app_is_refused_and_names_the_package_to_install() {
        let config = Config {
            app: Some("web.wsgi:app".to_string()),
            ..Config::default()
        };
        let message = no_python_support(&config, &manifest(WITH_A_VIEW), false).unwrap();
        assert!(message.contains("BLASTIT_APP (web.wsgi:app)"), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "{message}"
        );
    }

    #[test]
    fn a_view_no_mount_or_entrypoint_reaches_is_refused_by_name() {
        let message = no_python_support(&Config::default(), &manifest(WITH_A_VIEW), false).unwrap();
        assert!(message.contains("view \"web\""), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "{message}"
        );
        assert!(message.contains("`static` mount"), "{message}");
        assert!(message.contains("`entrypoint php`"), "{message}");
    }

    #[test]
    fn learn_needs_the_gate_and_says_so() {
        let message = no_python_support(&Config::default(), &manifest(STATIC_ONLY), true).unwrap();
        assert!(message.starts_with("dev --learn"), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "{message}"
        );
    }

    /// The one combination the gate deliberately leaves alone: a
    /// `BLASTIT_APP` on an all-static tree keeps `static_app_refusal`'s
    /// sentence in both builds, because installing the wheel would not make
    /// that app reachable either.
    #[test]
    fn an_app_on_a_static_tree_keeps_the_rule_that_already_explains_it() {
        let config = Config {
            app: Some("web.wsgi:app".to_string()),
            ..Config::default()
        };
        assert_eq!(
            no_python_support(&config, &manifest(STATIC_ONLY), false),
            None
        );
        assert!(
            static_app_refusal(&config, &manifest(STATIC_ONLY))
                .unwrap()
                .starts_with("BLASTIT_APP: the file serves static mounts and no views"),
        );
    }

    /// A PHP route runs in a `view` worker with no interpreter, so **this**
    /// gate and the python-less build must say nothing about installing
    /// Python (`MB.3.5.4`). The boot's own answer for such a route is
    /// [`php_boot_gate`]'s — a different gate, with a different sentence.
    ///
    /// The denied twin in the same test is the manifest that *is* WSGI-shaped:
    /// a route with no `[route.static]` and no `entrypoint php` keeps the gate's
    /// sentence, so the early return cannot have swallowed the whole rule.
    #[test]
    fn a_php_only_manifest_is_not_refused_by_the_capability_gate() {
        assert_eq!(
            no_python_support(&Config::default(), &manifest(PHP_ONLY), false),
            None
        );
        assert_eq!(
            no_python_support(&Config::default(), &manifest(STATIC_AND_PHP), false),
            None
        );

        let message = no_python_support(&Config::default(), &manifest(WITH_A_VIEW), false).unwrap();
        assert!(message.contains("view \"web\""), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "a WSGI view keeps the sentence that names the artifact: {message}"
        );
    }
}

/// `ADR-0010` §3/§4.1: the `entrypoint php` discovery gate and its two exact
/// sentences.
///
/// Every rule here has a denied case and an accepted twin: a host with no
/// `libphp` at all, one whose object is ZTS or not an ELF at all, one whose
/// version no shim matches — beside the manifest that declares no PHP route and
/// must be left alone, and the versioned NTS object with its shim that resolves.
/// (`workers` is not among them: `ADR-0012` §12.3 removed the keyword, so a
/// `Blastfile` can never carry a cap above a budget.)
///
/// The library fixtures are written out byte by byte (an ELF64 header plus a
/// `.dynsym` and its `.dynstr`), so the inline suite needs no C toolchain and
/// every case chooses exactly the exported names it needs. The `cc`-built
/// constructor-canary stubs — proving zero stage-0 `dlopen` from the outside —
/// belong to the protected suite (`MB.3.13.4`).
#[cfg(all(test, feature = "php"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod php_boot_gate {
    use std::path::PathBuf;

    use super::*;
    // The ELF facts the fixtures below write out byte by byte: the walker's own
    // constants, imported rather than respelled, so a fixture cannot drift from
    // the table it feeds (`MB.36.5`).
    use blastit_view_php::discovery::{
        EM_AARCH64, EM_X86_64, SHT_DYNSYM, SHT_STRTAB, decode_aarch64, decode_x86_64, library_dirs,
        php_refusal_message, target_tuple,
    };

    /// The page's shape: files from disk, the rest to the front controller.
    const STATIC_AND_PHP: &str = "\
entrypoint php \"public/index.php\"

static \"/\" {
    root \"public\"
}

view app regex '.*' {}
";

    /// Two PHP routes: the gate names the **first** one, so the sentence is
    /// actionable and stable.
    const TWO_PHP_ROUTES: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}

view api regex '/api/.*' {}
";

    /// The ADR-0010 §4.1 name, pinned to literals rather than to this module's
    /// own helpers: `target_arch`/`target_libc` are the second spelling of the
    /// tuple (see their docs), so this test is what keeps the boot's name and
    /// `Minor::shim_file()`'s name in step for every supported target. The
    /// php-sapi half of the same literals lives in
    /// `crates/blastit-php-sapi/tests/critical_shim_contract.rs`.
    #[test]
    fn the_shim_name_matches_the_adr_literal() {
        #[cfg(all(target_arch = "x86_64", not(target_env = "musl")))]
        const SHIM_84: &str = "libblastit_php84-x86_64-gnu.so";
        #[cfg(all(target_arch = "x86_64", target_env = "musl"))]
        const SHIM_84: &str = "libblastit_php84-x86_64-musl.so";
        #[cfg(all(target_arch = "aarch64", not(target_env = "musl")))]
        const SHIM_84: &str = "libblastit_php84-aarch64-gnu.so";
        #[cfg(all(target_arch = "aarch64", target_env = "musl"))]
        const SHIM_84: &str = "libblastit_php84-aarch64-musl.so";

        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            assert_eq!(shim_name(PhpVersion::new(8, 4)), SHIM_84);
            // Only the infix moves between minors: the tuple is the build's.
            assert_eq!(
                shim_name(PhpVersion::new(8, 3)),
                SHIM_84.replace("php84-", "php83-")
            );
            assert_eq!(
                shim_name(PhpVersion::new(8, 5)),
                SHIM_84.replace("php84-", "php85-")
            );
            // A minor with no shim ABI keeps the same shape, so the refusal can
            // still name the file the host would need.
            assert_eq!(
                shim_name(PhpVersion::new(8, 6)),
                SHIM_84.replace("php84-", "php86-")
            );
        }
    }

    /// A PHP route with no cap at all: the deployment's budget is the budget.
    ///
    /// `Blastfile` has no `workers` keyword at all (`ADR-0012` §12.3: the
    /// scheduler owns parallelism), so this is the only PHP shape a file can
    /// declare, and the budget rule has no over-cap input to refuse.
    const PHP_ONLY: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

    const STATIC_ONLY: &str = "\
static \"/\" {
    root \"site\"
}
";

    const WITH_A_VIEW: &str = "\
view web regex '.*' {}
";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        manifest
    }

    /// A directory under the test temp dir, removed on drop.
    struct Scratch {
        dir: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("blastit-php-gate-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        /// Writes `contents` as `name` and returns the path.
        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.dir.join(name);
            std::fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// The two provenance callbacks a deployment boot supplies, over
    /// `crate::host` — literally the functions [`php_boot_gate`] hands the
    /// gate, so these cases measure the gate and not a stand-in.
    ///
    /// Both are needed, and both are real: the object decision is ownership +
    /// file capabilities + the zero-writable `DT_RPATH`/`DT_RUNPATH` scan, the
    /// tree decision is ownership only (design §4.4 D5). A no-op closure here
    /// would silently invert every deployment case below.
    /// One provenance callback as `Deployment` takes it: the object decision
    /// receives the claim's noun as well as its path, and the tree decision
    /// receives only the path.
    type ObjectCheck = Box<dyn Fn(&str, &Path) -> Result<(), String>>;
    type TreeCheck = Box<dyn Fn(&Path) -> Result<(), String>>;

    struct HostChecks {
        init_uid: u32,
        objects: ObjectCheck,
        trees: TreeCheck,
    }

    impl HostChecks {
        fn new(init_uid: u32) -> Self {
            Self {
                init_uid,
                objects: Box::new(move |what, path| object_is_protected(init_uid, what, path)),
                trees: Box::new(move |path| tree_is_protected(init_uid, path)),
            }
        }

        /// The `Deployment` a deployment boot would build for this directory.
        fn deployment(&self, manifest_dir: &Path) -> Deployment<'_> {
            Deployment {
                init_uid: self.init_uid,
                manifest_dir: manifest_dir.to_path_buf(),
                is_protected_object: self.objects.as_ref(),
                is_protected_tree: self.trees.as_ref(),
            }
        }
    }

    /// A little-endian ELF shared object whose `.dynsym` exports `names` plus
    /// the `php_version` symbol the worker needs (`MB.3.59`). The symbol is a
    /// function with **no body**, so the version comes from the file name — the
    /// fast path — exactly as a distro soname does.
    ///
    /// Layout: the 64-byte ELF header, the dynamic symbol table (a null symbol
    /// then one per name), its string table, then the null, `SHT_DYNSYM`,
    /// `SHT_STRTAB` section headers. Nothing is loaded: the gate reads these
    /// bytes the way it reads a distro `libphp`.
    fn shared_object(names: &[&str]) -> Vec<u8> {
        shared_object_with(names, None, true)
    }

    /// The same object, with a `php_version` body that points at `version` in a
    /// read-only section — the static fact `MB.3.59` reads when the path states
    /// no minor.
    fn shared_object_with_version(names: &[&str], version: &str) -> Vec<u8> {
        shared_object_with(names, Some(version), true)
    }

    /// An NTS object with **no** `php_version` in its `.dynsym` at all.
    fn shared_object_without_php_version(names: &[&str]) -> Vec<u8> {
        shared_object_with(names, None, false)
    }

    /// How the fixture types the `php_version` symbol (`MB.3.61`).
    ///
    /// Presence is read from the **name**, so every shape below is an object
    /// that exports `php_version`; only the body fact differs.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PhpVersionSymbol {
        /// `STT_FUNC` with the body's true size — what a distro build produces.
        Function,
        /// `STT_NOTYPE`, the body bytes still in `.text`: an odd but valid
        /// object that does export the symbol.
        NotType,
        /// `STT_FUNC` with `st_size == 0`: the body is there, its extent is not
        /// stated.
        ZeroSize,
    }

    fn shared_object_with(
        names: &[&str],
        version: Option<&str>,
        exports_php_version: bool,
    ) -> Vec<u8> {
        shared_object_in(
            names,
            version,
            exports_php_version,
            PhpVersionSymbol::Function,
        )
    }

    /// The same object with `php_version` typed/shaped by `symbol` (`MB.3.61`).
    fn shared_object_in(
        names: &[&str],
        version: Option<&str>,
        exports_php_version: bool,
        symbol: PhpVersionSymbol,
    ) -> Vec<u8> {
        let machine: u16 = if cfg!(target_arch = "aarch64") {
            EM_AARCH64
        } else {
            EM_X86_64
        };
        let mut exports: Vec<&str> = names.to_vec();
        if exports_php_version && !exports.contains(&"php_version") {
            exports.push("php_version");
        }
        let mut strtab = vec![0u8];
        let mut offsets = Vec::new();
        for name in &exports {
            offsets.push(u32::try_from(strtab.len()).unwrap());
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
        }
        let dynsym_off = 64usize;
        let dynsym_len = 24 * (1 + exports.len());
        let strtab_off = dynsym_off + dynsym_len;
        let text_off = (strtab_off + strtab.len() + 7) & !7;
        let code_len = if version.is_some() {
            if machine == EM_AARCH64 { 12 } else { 8 }
        } else {
            0
        };
        let rodata_off = (text_off + code_len + 7) & !7;
        let string: Vec<u8> = version
            .map(|version| {
                let mut bytes = version.as_bytes().to_vec();
                bytes.push(0);
                bytes
            })
            .unwrap_or_default();
        let shoff = (rodata_off + string.len() + 7) & !7;
        let mut file = vec![0u8; shoff + 5 * 64];
        file[..4].copy_from_slice(b"\x7fELF");
        file[4] = 2; // ELFCLASS64
        file[5] = 1; // ELFDATA2LSB
        file[6] = 1; // EV_CURRENT
        file[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        file[18..20].copy_from_slice(&machine.to_le_bytes());
        file[0x28..0x30].copy_from_slice(&u64::try_from(shoff).unwrap().to_le_bytes());
        file[0x34..0x36].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
        file[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
        file[0x3c..0x3e].copy_from_slice(&5u16.to_le_bytes()); // e_shnum

        let mut dynsym = vec![0u8; 24];
        for (index, offset) in offsets.iter().enumerate() {
            let mut entry = vec![0u8; 24];
            entry[..4].copy_from_slice(&offset.to_le_bytes());
            entry[4] = if exports[index] == "php_version" {
                // `STT_NOTYPE` (0) or `STB_GLOBAL | STT_FUNC` (0x12): both
                // export the *name*, which is the fact the gate reads.
                match symbol {
                    PhpVersionSymbol::NotType => 0x10,
                    PhpVersionSymbol::Function | PhpVersionSymbol::ZeroSize => 0x12,
                }
            } else {
                0x11 // STB_GLOBAL | STT_OBJECT
            };
            if exports[index] == "php_version" && version.is_some() {
                entry[6..8].copy_from_slice(&3u16.to_le_bytes()); // .text
                entry[8..16].copy_from_slice(&u64::try_from(text_off).unwrap().to_le_bytes());
                let stated = match symbol {
                    PhpVersionSymbol::ZeroSize => 0,
                    PhpVersionSymbol::Function | PhpVersionSymbol::NotType => code_len,
                };
                entry[16..24].copy_from_slice(&u64::try_from(stated).unwrap().to_le_bytes());
            }
            dynsym.extend_from_slice(&entry);
        }
        file[dynsym_off..dynsym_off + dynsym.len()].copy_from_slice(&dynsym);
        file[strtab_off..strtab_off + strtab.len()].copy_from_slice(&strtab);
        if version.is_some() {
            let code = php_version_body(machine, text_off, rodata_off);
            assert_eq!(code.len(), code_len, "the fixture's body is one size");
            file[text_off..text_off + code.len()].copy_from_slice(&code);
            file[rodata_off..rodata_off + string.len()].copy_from_slice(&string);
        }

        let header = |index: usize| shoff + index * 64;
        // The null header is already zeroed.
        let dynsym_header = header(1);
        file[dynsym_header + 4..dynsym_header + 8].copy_from_slice(&SHT_DYNSYM.to_le_bytes());
        file[dynsym_header + 0x18..dynsym_header + 0x20]
            .copy_from_slice(&u64::try_from(dynsym_off).unwrap().to_le_bytes());
        file[dynsym_header + 0x20..dynsym_header + 0x28]
            .copy_from_slice(&u64::try_from(dynsym.len()).unwrap().to_le_bytes());
        file[dynsym_header + 0x28..dynsym_header + 0x2c].copy_from_slice(&2u32.to_le_bytes());
        file[dynsym_header + 0x38..dynsym_header + 0x40].copy_from_slice(&24u64.to_le_bytes());
        let strtab_header = header(2);
        file[strtab_header + 4..strtab_header + 8].copy_from_slice(&SHT_STRTAB.to_le_bytes());
        file[strtab_header + 0x18..strtab_header + 0x20]
            .copy_from_slice(&u64::try_from(strtab_off).unwrap().to_le_bytes());
        file[strtab_header + 0x20..strtab_header + 0x28]
            .copy_from_slice(&u64::try_from(strtab.len()).unwrap().to_le_bytes());
        let text_header = header(3);
        file[text_header + 4..text_header + 8].copy_from_slice(&1u32.to_le_bytes()); // PROGBITS
        file[text_header + 0x8..text_header + 0x10].copy_from_slice(&6u64.to_le_bytes()); // ALLOC|EXEC
        file[text_header + 0x10..text_header + 0x18]
            .copy_from_slice(&u64::try_from(text_off).unwrap().to_le_bytes());
        file[text_header + 0x18..text_header + 0x20]
            .copy_from_slice(&u64::try_from(text_off).unwrap().to_le_bytes());
        file[text_header + 0x20..text_header + 0x28]
            .copy_from_slice(&u64::try_from(code_len).unwrap().to_le_bytes());
        let rodata_header = header(4);
        file[rodata_header + 4..rodata_header + 8].copy_from_slice(&1u32.to_le_bytes());
        file[rodata_header + 0x8..rodata_header + 0x10].copy_from_slice(&2u64.to_le_bytes()); // ALLOC
        file[rodata_header + 0x10..rodata_header + 0x18]
            .copy_from_slice(&u64::try_from(rodata_off).unwrap().to_le_bytes());
        file[rodata_header + 0x18..rodata_header + 0x20]
            .copy_from_slice(&u64::try_from(rodata_off).unwrap().to_le_bytes());
        file[rodata_header + 0x20..rodata_header + 0x28]
            .copy_from_slice(&u64::try_from(string.len()).unwrap().to_le_bytes());
        file
    }

    /// `return "PHP_VERSION";` as the host's architecture compiles it.
    fn php_version_body(machine: u16, text: usize, string: usize) -> Vec<u8> {
        if machine == EM_AARCH64 {
            let pages = (string >> 12) as i64 - (text >> 12) as i64;
            let imm = pages & 0x1f_ffff;
            let adrp =
                0x9000_0000u32 | (((imm >> 2) as u32 & 0x7_ffff) << 5) | ((imm as u32 & 0x3) << 29);
            let add = 0x9100_0000u32 | (((string & 0xfff) as u32) << 10);
            let mut code = Vec::new();
            code.extend_from_slice(&adrp.to_le_bytes());
            code.extend_from_slice(&add.to_le_bytes());
            code.extend_from_slice(&0xd65f_03c0u32.to_le_bytes()); // ret
            code
        } else {
            // `lea rax,[rip+disp]` lands on the byte *after* the instruction.
            let next = text + 7;
            let disp = i32::try_from(string as i64 - next as i64).unwrap();
            let mut code = vec![0x48, 0x8d, 0x05];
            code.extend_from_slice(&disp.to_le_bytes());
            code.push(0xc3); // ret
            code
        }
    }

    /// A search over one explicit NTS object and the shims in `shim_dir`.
    fn search_for(libphp: &Path, shim_dir: &Path) -> PhpSearch {
        PhpSearch {
            libphp: Some(libphp.to_path_buf()),
            shim_dirs: vec![shim_dir.to_path_buf()],
            lib_dirs: Vec::new(),
        }
    }

    /// An 8.4 NTS object and its correctly named shim, both under `scratch`.
    fn resolved_84(scratch: &Scratch) -> (PathBuf, PhpSearch) {
        let libphp = scratch.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        let shims = scratch.dir.join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::write(
            shims.join(shim_name(PhpVersion::new(8, 4))),
            b"the shim is not loaded here",
        )
        .unwrap();
        (
            libphp.clone(),
            PhpSearch {
                libphp: Some(libphp),
                shim_dirs: vec![shims],
                lib_dirs: Vec::new(),
            },
        )
    }

    /// A `BLASTIT_LIBPHP` that is a symlink — the distro's
    /// `libphp8.so -> libphp8.4.so` shape — is recorded as its **target**: stage
    /// 1 re-opens each object `O_NOFOLLOW`-strictly, so recording the link
    /// itself would kill the worker with `ELOOP` after the banner instead of
    /// loading the library the gate inspected.
    #[test]
    fn a_symlinked_libphp_and_shim_are_recorded_as_their_targets() {
        let scratch = Scratch::new("symlink");
        let real = scratch.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        let link = scratch.dir.join("libphp8.so");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let shims = Scratch::new("symlink-shims");
        let real_shim = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded here");
        let linked_dir = Scratch::new("symlink-shim-dir");
        let linked_shim = linked_dir.dir.join(shim_name(PhpVersion::new(8, 4)));
        std::os::unix::fs::symlink(&real_shim, &linked_shim).unwrap();

        let search = search_for(&link, &linked_dir.dir);
        let runtime = discover_php(&search).expect("a link to an NTS 8.4 object and its shim");
        assert_eq!(runtime.libphp, real, "the record is the resolved library");
        assert_eq!(runtime.shim, real_shim, "and the resolved shim");
        for path in [&runtime.libphp, &runtime.shim] {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            assert!(
                !metadata.file_type().is_symlink(),
                "{} is still a link, so stage 1's O_NOFOLLOW open would fail",
                path.display()
            );
        }
        assert!(
            matches!(php_boot(&manifest(PHP_ONLY), &search), PhpBoot::Proceed(_)),
            "a legitimately symlinked distro library keeps booting"
        );
    }

    /// The link that goes nowhere is a §4.1 refusal, not an `errno` in a
    /// worker: the route, its `entry`, the path, the package and
    /// `BLASTIT_LIBPHP`.
    #[test]
    fn a_dangling_symlink_is_refused_with_the_package_sentence() {
        let scratch = Scratch::new("dangling");
        let link = scratch.dir.join("libphp8.4.so");
        std::os::unix::fs::symlink(scratch.dir.join("not-installed.so"), &link).unwrap();
        let shims = Scratch::new("dangling-shims");

        let message = php_boot(&manifest(PHP_ONLY), &search_for(&link, &shims.dir))
            .refusal()
            .expect("a dangling link cannot be embedded")
            .to_string();
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("entrypoint php \"public/index.php\""),
            "{message}"
        );
        assert!(message.contains(&link.display().to_string()), "{message}");
        assert!(
            message.contains("install the PHP embed package"),
            "{message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(
            !message.contains("Too many levels of symbolic links"),
            "the refusal is the sentence, not a raw errno: {message}"
        );
    }

    /// A host with nothing to embed refuses **only** when a route asks for PHP.
    ///
    /// The refusal is `ADR-0010` §4.1's missing-library sentence: the route and
    /// its `entry`, the names that were searched and where, the three distro
    /// packages and `BLASTIT_LIBPHP`. A static tree and a WSGI-shaped manifest
    /// boot as before — the same `stage_0` must not refuse a site that never
    /// asked for PHP.
    #[test]
    fn a_host_with_no_libphp_refuses_only_when_a_php_route_exists() {
        let empty = Scratch::new("empty");
        let search = PhpSearch {
            libphp: None,
            shim_dirs: vec![empty.dir.clone()],
            lib_dirs: vec![empty.dir.clone()],
        };
        for source in [STATIC_ONLY, WITH_A_VIEW] {
            assert!(
                matches!(php_boot(&manifest(source), &search), PhpBoot::Absent),
                "{source}"
            );
        }
        let message = php_boot(&manifest(STATIC_AND_PHP), &search)
            .refusal()
            .expect("a PHP route")
            .to_string();
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("entrypoint php \"public/index.php\""),
            "{message}"
        );
        assert!(message.contains("no usable NTS libphp"), "{message}");
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        for package in [
            "apt install libphp-embed",
            "apk add php85-embed",
            "dnf install php-embedded",
        ] {
            assert!(
                message.contains(package),
                "the refusal names {package}: {message}"
            );
        }
        assert!(
            message.contains(&empty.dir.display().to_string()),
            "the refusal names the directories that were searched: {message}"
        );
        assert!(
            !message.contains("MB.3.13.3"),
            "a host with no library gets the package sentence, never the wiring one: {message}"
        );

        // The first PHP route is the one named, never the second.
        let two = php_boot(&manifest(TWO_PHP_ROUTES), &search)
            .refusal()
            .expect("a PHP route")
            .to_string();
        assert!(two.contains("route \"app\""), "{two}");
        assert!(!two.contains("api/index.php"), "{two}");

        // The `dev` twin is the same refusal: `ADR-0010` §1 gives PHP one
        // backend, so a developer host with nothing installed gets the package
        // sentence too, not a boot that serves the route another way.
        let dev = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("a PHP-free dev host is refused")
            .to_string();
        assert!(dev.contains("no usable NTS libphp"), "{dev}");
        assert!(
            dev.contains("apt install libphp-embed"),
            "dev names the package: {dev}"
        );
    }

    /// A ZTS build is refused with the tuple and the shim it would need — and
    /// the same object with `tsrm_startup` gone resolves.
    #[test]
    fn a_zts_libphp_is_refused_naming_the_tuple_and_the_shim() {
        let scratch = Scratch::new("zts");
        let zts = scratch.file(
            "libphp8.4.so",
            &shared_object(&["executor_globals", "tsrm_startup"]),
        );
        let shims = Scratch::new("zts-shims");
        let shim = shims.file(
            &shim_name(PhpVersion::new(8, 4)),
            b"never loaded in stage 0",
        );
        let search = search_for(&zts, &shims.dir);
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("a ZTS host is refused")
            .to_string();
        assert!(message.contains("ZTS (thread-safe)"), "{message}");
        assert!(
            message.contains(&format!("(PHP 8.4, {})", target_tuple())),
            "the tuple: {message}"
        );
        assert!(
            message.contains(&format!("libblastit_php84-{}.so", target_tuple())),
            "the shim that tuple needs: {message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");
        assert!(shim.is_file(), "the shim exists: ZTS is the refusal");

        // The accepted twin: the same file name without `tsrm_startup`.
        let nts = scratch.file("libphp-8.4.so", &shared_object(&["executor_globals"]));
        let search = search_for(&nts, &shims.dir);
        let runtime = discover_php(&search).expect("an NTS 8.4 object with its shim");
        assert_eq!(runtime.version, PhpVersion::new(8, 4));
        assert_eq!(runtime.libphp, nts);
        assert_eq!(runtime.shim, shim);
        assert!(runtime.ino > 0, "the inode is recorded for stage 1");
        assert!(
            matches!(php_boot(&manifest(PHP_ONLY), &search), PhpBoot::Proceed(_)),
            "the host resolves, so the boot proceeds to stage 1's binding"
        );
    }

    /// An object that is not an ELF — a text file, a directory, a path that is
    /// not there — is refused, never guessed at.
    #[test]
    fn a_libphp_that_is_not_an_elf_object_is_refused() {
        let scratch = Scratch::new("notelf");
        let shims = Scratch::new("notelf-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let cases = [
            (
                scratch.file("libphp8.4.so", b"this is not an ELF shared object"),
                "not an ELF shared object",
            ),
            (scratch.dir.join("libphp-8.4.so"), "cannot be opened"),
        ];
        for (path, expected) in cases {
            let message = php_boot(&manifest(PHP_ONLY), &search_for(&path, &shims.dir))
                .refusal()
                .expect("an unusable path is refused")
                .to_string();
            assert!(message.contains(expected), "{path:?}: {message}");
            assert!(
                message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
                "{message}"
            );
        }
        // A directory named like the library.
        let directory = scratch.dir.join("libphp8.3.so");
        std::fs::create_dir_all(&directory).unwrap();
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&directory, &shims.dir))
            .refusal()
            .expect("a directory is not a library")
            .to_string();
        assert!(message.contains("not a regular file"), "{message}");
    }

    /// The version comes from the file name (in either spelling), and a name
    /// that carries none is refused rather than guessed.
    #[test]
    fn the_version_comes_from_the_file_name() {
        let scratch = Scratch::new("version");
        let shims = Scratch::new("version-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 3)), b"never loaded");

        let unversioned = scratch.file("libphp.so", &shared_object(&["executor_globals"]));
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&unversioned, &shims.dir))
            .refusal()
            .expect("an unversioned name with no readable version cannot choose a shim")
            .to_string();
        assert!(
            message.contains("exports `php_version`"),
            "the `.dynsym` fact is named: {message}"
        );
        assert!(
            message.contains("could not be read from its sections"),
            "the static fact is named: {message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(
            !message.contains("names no PHP major.minor"),
            "the old file-name-only sentence is gone: {message}"
        );

        // The accepted twin, in the distro's hyphenated spelling.
        let hyphenated = scratch.file("libphp-8.3.so", &shared_object(&["executor_globals"]));
        let runtime =
            discover_php(&search_for(&hyphenated, &shims.dir)).expect("libphp-8.3.so and its shim");
        assert_eq!(runtime.version, PhpVersion::new(8, 3));
        assert_eq!(
            runtime.shim.file_name().unwrap().to_str().unwrap(),
            format!("libblastit_php83-{}.so", target_tuple())
        );
    }

    /// `MB.3.59`: Alpine's own package path states its minor in the **packed**
    /// file name (`libphp83.so`) or in the per-minor parent directory
    /// (`/usr/lib/php84/libphp.so`), and neither is a versioned name.
    #[test]
    fn the_packed_name_and_the_alpine_directory_state_the_minor() {
        let scratch = Scratch::new("alpine-spellings");
        let shims = Scratch::new("alpine-spellings-shims");
        for minor in [3u8, 4] {
            let _ = shims.file(&shim_name(PhpVersion::new(8, minor)), b"never loaded");
        }

        // `libphp83.so`: the name states 8.3 with no dot.
        let packed = scratch.file("libphp83.so", &shared_object(&["executor_globals"]));
        let runtime = discover_php(&search_for(&packed, &shims.dir)).expect("libphp83.so");
        assert_eq!(runtime.version, PhpVersion::new(8, 3));

        // `/usr/lib/php84/libphp.so`: the name states nothing, the directory
        // states 8.4.
        std::fs::create_dir_all(scratch.dir.join("php84")).unwrap();
        let directory = scratch.file("php84/libphp.so", &shared_object(&["executor_globals"]));
        let runtime = discover_php(&search_for(&directory, &shims.dir)).expect("php84/libphp.so");
        assert_eq!(runtime.version, PhpVersion::new(8, 4));
        assert_eq!(
            runtime.shim.file_name().unwrap().to_str().unwrap(),
            format!("libblastit_php84-{}.so", target_tuple())
        );
    }

    /// `MB.3.59`: the object's own bytes name the minor when the path states
    /// none — the `php_version` body's string, read statically.
    #[test]
    fn the_object_bytes_name_the_minor_when_the_path_does_not() {
        let scratch = Scratch::new("object-version");
        let shims = Scratch::new("object-version-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let unversioned = scratch.file(
            "libphp.so",
            &shared_object_with_version(&["executor_globals"], "8.4.27"),
        );
        let runtime =
            discover_php(&search_for(&unversioned, &shims.dir)).expect("the object names 8.4");
        assert_eq!(runtime.version, PhpVersion::new(8, 4));
        assert_eq!(
            runtime.shim.file_name().unwrap().to_str().unwrap(),
            format!("libblastit_php84-{}.so", target_tuple())
        );
    }

    /// `MB.3.59`: two statements that disagree are a refusal naming both, never
    /// a preference — the shim is chosen by this number.
    #[test]
    fn two_disagreeing_minors_are_refused_naming_both() {
        let scratch = Scratch::new("version-conflict");
        let shims = Scratch::new("version-conflict-shims");
        for minor in [3u8, 4] {
            let _ = shims.file(&shim_name(PhpVersion::new(8, minor)), b"never loaded");
        }
        // The file name states 8.3, the directory states 8.4.
        std::fs::create_dir_all(scratch.dir.join("php84")).unwrap();
        let conflicted = scratch.file("php84/libphp83.so", &shared_object(&["executor_globals"]));
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&conflicted, &shims.dir))
            .refusal()
            .expect("two minors cannot both choose a shim")
            .to_string();
        assert!(message.contains("more than one PHP minor"), "{message}");
        assert!(message.contains("PHP 8.3"), "{message}");
        assert!(message.contains("PHP 8.4"), "{message}");
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");
    }

    /// `MB.3.59`: an NTS object whose `.dynsym` carries no `php_version` is
    /// refused naming the symbol, and a file name that **does** carry a
    /// supported minor does not change the sentence.
    #[test]
    fn a_libphp_without_php_version_is_refused_naming_the_symbol() {
        let scratch = Scratch::new("no-php-version");
        let shims = Scratch::new("no-php-version-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let versioned_name = scratch.file(
            "libphp8.4.so",
            &shared_object_without_php_version(&["executor_globals"]),
        );
        let message = php_boot(
            &manifest(PHP_ONLY),
            &search_for(&versioned_name, &shims.dir),
        )
        .refusal()
        .expect("a libphp without php_version cannot be embedded")
        .to_string();
        assert!(
            message.contains("exports no `php_version`"),
            "the missing symbol is named: {message}"
        );
        assert!(
            message.contains("its file name states PHP 8.4"),
            "the name is named, but does not rescue it: {message}"
        );
        assert!(
            !message.contains("names no PHP major.minor"),
            "a versioned name must not change the sentence: {message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");
    }

    /// An installed minor this build ships no shim for is refused with the
    /// support sentence, not with the `.dynsym` story of a symbol it may or may
    /// not export — and the same holds when `BLASTIT_LIBPHP` names it.
    #[test]
    fn an_unsupported_minor_is_refused_with_the_support_sentence() {
        let scratch = Scratch::new("unsupported-minor");
        let shims = Scratch::new("unsupported-minor-shims");
        let old = scratch.file(
            "libphp8.2.so",
            &shared_object_without_php_version(&["executor_globals"]),
        );
        let sentence = "PHP 8.2 is not supported — Blastit supports PHP 8.3, 8.4 and 8.5";

        // `BLASTIT_LIBPHP` names it: the object is opened, and the version its
        // own name states is refused before the symbol walk.
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&old, &shims.dir))
            .refusal()
            .expect("an unsupported minor cannot be embedded")
            .to_string();
        assert!(message.ends_with(sentence), "{message}");
        assert!(
            !message.contains("exports no `php_version`"),
            "the support sentence replaces the symbol story: {message}"
        );

        // No supported spelling exists, and the only installed library is the
        // unsupported one: the refusal scan finds it and says the same thing.
        let only_old = Scratch::new("only-unsupported");
        let _ = only_old.file(
            "libphp8.2.so",
            &shared_object_without_php_version(&["executor_globals"]),
        );
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        search.lib_dirs = vec![only_old.dir.clone()];
        search.shim_dirs = vec![shims.dir.clone()];
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("an 8.2-only host is refused")
            .to_string();
        assert!(message.ends_with(sentence), "{message}");

        // The per-minor directory spelling states the same minor, so an
        // unversioned `libphp.so` in `/usr/lib/php82` is refused the same way.
        let per_minor = Scratch::new("only-unsupported-per-minor");
        std::fs::create_dir_all(per_minor.dir.join("php82")).unwrap();
        let _ = per_minor.file(
            "php82/libphp.so",
            &shared_object_without_php_version(&["executor_globals"]),
        );
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        search.lib_dirs = vec![per_minor.dir.join("php82")];
        search.shim_dirs = vec![shims.dir.clone()];
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("the per-minor directory states the minor")
            .to_string();
        assert!(message.ends_with(sentence), "{message}");
    }

    /// `MB.3.59`: the discovery directories include Alpine's per-minor embed
    /// directories, so an unset `BLASTIT_LIBPHP` can reach its own package.
    #[test]
    fn the_alpine_embed_directories_are_searched() {
        let dirs = library_dirs();
        for wanted in [
            "/usr/lib/php85",
            "/usr/lib/php84",
            "/usr/lib/php83",
            "/usr/local/lib/php85",
            "/usr/local/lib/php84",
            "/usr/local/lib/php83",
        ] {
            assert!(
                dirs.iter().any(|dir| dir == Path::new(wanted)),
                "{wanted} is missing from {dirs:?}"
            );
        }
        // Order is preference: the plain `/usr/lib` comes before a per-minor
        // directory, and `/usr/local` after both.
        let position = |wanted: &str| {
            dirs.iter()
                .position(|dir| dir == Path::new(wanted))
                .unwrap_or(usize::MAX)
        };
        assert!(position("/usr/lib") < position("/usr/lib/php85"));
        assert!(position("/usr/lib/php85") < position("/usr/local/lib"));
    }

    /// `MB.3.61`: a host carrying **two** embed packages must serve from the
    /// newer library, not from the first name in the list.
    ///
    /// The reproduced shape is Alpine's: `php83-embed` installs
    /// `/usr/lib/libphp83.so` (packed name) and `php85-embed` installs
    /// `/usr/lib/php85/libphp.so` (per-minor directory). With
    /// `BLASTIT_LIBPHP` unset a name-major walk found `libphp83.so` first and
    /// served 8.3, although the 8.5 library was right there.
    #[test]
    fn two_embed_packages_serve_from_the_newer_library() {
        let scratch = Scratch::new("two-packages");
        let shims = Scratch::new("two-packages-shims");
        for minor in [3u8, 5] {
            let _ = shims.file(&shim_name(PhpVersion::new(8, minor)), b"never loaded");
        }
        // The two packages' own paths, in the directories `library_dirs()`
        // names — the same layout an Alpine host gets.
        std::fs::create_dir_all(scratch.dir.join("php85")).unwrap();
        let old = scratch.file("libphp83.so", &shared_object(&["executor_globals"]));
        let new = scratch.file("php85/libphp.so", &shared_object(&["executor_globals"]));
        assert!(
            old.is_file() && new.is_file(),
            "both packages are installed"
        );

        // `BLASTIT_LIBPHP` unset: `PhpSearch::new(None, …)` is exactly the
        // library half of that, and the directories are the host's order.
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        assert!(
            search.libphp.is_none(),
            "an unset BLASTIT_LIBPHP is no override at all"
        );
        search.lib_dirs = vec![scratch.dir.clone(), scratch.dir.join("php85")];
        search.shim_dirs = vec![shims.dir.clone()];

        let boot = php_boot(&manifest(PHP_ONLY), &search);
        let PhpBoot::Proceed(runtime) = boot else {
            panic!("two packages, and the 8.5 library serves: {boot:?}");
        };
        assert_eq!(runtime.version, PhpVersion::new(8, 5), "the newer library");
        assert_eq!(
            runtime.libphp, new,
            "the served library is the 8.5 one, not the 8.3 name that came first"
        );

        // The single-package twin: the packed 8.3 name alone still serves.
        let only_old = Scratch::new("only-8.3");
        let old = only_old.file("libphp83.so", &shared_object(&["executor_globals"]));
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        search.lib_dirs = vec![only_old.dir.clone()];
        search.shim_dirs = vec![shims.dir.clone()];
        let runtime = discover_php(&search).expect("8.3 alone still serves");
        assert_eq!(runtime.version, PhpVersion::new(8, 3));
        assert_eq!(runtime.libphp, old);

        // And the 8.5 package alone still serves, from its own directory.
        let only_new = Scratch::new("only-8.5");
        std::fs::create_dir_all(only_new.dir.join("php85")).unwrap();
        let new = only_new.file("php85/libphp.so", &shared_object(&["executor_globals"]));
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        search.lib_dirs = vec![only_new.dir.join("php85")];
        search.shim_dirs = vec![shims.dir.clone()];
        let runtime = discover_php(&search).expect("8.5 alone still serves");
        assert_eq!(runtime.version, PhpVersion::new(8, 5));
        assert_eq!(runtime.libphp, new);
    }

    /// `MB.3.61`: `php_version` **presence** is the name in the `.dynsym`, so an
    /// odd object that exports it as `STT_NOTYPE` or with `st_size == 0` is not
    /// told it "exports no `php_version`" — a sentence about a symbol it has.
    /// The body is a separate fact: a versioned path still serves, and with no
    /// minor anywhere the refusal names the unreadable body (`NoVersion`).
    #[test]
    fn php_version_presence_is_the_name_not_the_body_shape() {
        let shims = Scratch::new("odd-symbols-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");

        for (shape, label) in [
            (PhpVersionSymbol::NotType, "STT_NOTYPE"),
            (PhpVersionSymbol::ZeroSize, "st_size-zero"),
        ] {
            let scratch = Scratch::new(&format!("odd-symbols-{label}"));
            // A versioned name: the symbol's presence is enough, and the minor
            // comes from the path because the body states nothing.
            let versioned = scratch.file(
                "libphp8.4.so",
                &shared_object_in(&["executor_globals"], None, true, shape),
            );
            let runtime = discover_php(&search_for(&versioned, &shims.dir))
                .unwrap_or_else(|refusal| panic!("{label}: refused as {refusal:?}"));
            assert_eq!(runtime.version, PhpVersion::new(8, 4), "{label}");

            // The unversioned twin: no body version, no minor in the path, so
            // the only true sentence is the one that names the body it cannot
            // read — never that the symbol is absent.
            let unversioned = scratch.file(
                "libphp.so",
                &shared_object_in(&["executor_globals"], None, true, shape),
            );
            let message = php_boot(&manifest(PHP_ONLY), &search_for(&unversioned, &shims.dir))
                .refusal()
                .unwrap_or_else(|| panic!("{label}: no version anywhere cannot choose a shim"))
                .to_string();
            assert!(
                !message.contains("exports no `php_version`"),
                "{label}: the object does export it, so that sentence is false: {message}"
            );
            assert!(
                message.contains("exports `php_version` in its `.dynsym`"),
                "{label}: the real cause is the unreadable body: {message}"
            );
            assert!(
                message.contains("could not be read from its sections"),
                "{label}: {message}"
            );

            // The genuinely missing symbol keeps its own sentence: the two
            // refusals stay distinguishable.
            let absent = scratch.file(
                "libphp8.4.so",
                &shared_object_without_php_version(&["executor_globals"]),
            );
            let message = php_boot(&manifest(PHP_ONLY), &search_for(&absent, &shims.dir))
                .refusal()
                .expect("no php_version name at all is refused")
                .to_string();
            assert!(
                message.contains("exports no `php_version`"),
                "{label}: the absence sentence still belongs to the absent symbol: {message}"
            );
        }
    }

    /// The two `php_version` bodies the resolver decodes, on both architectures
    /// — the aarch64 half cannot run on an x86_64 host, so it is pinned by
    /// bytes rather than left to an arm runner.
    #[test]
    fn the_version_body_is_decoded_on_both_architectures() {
        let disp = i32::try_from(0x2000i64 - 0x1007).unwrap();
        let mut x86 = vec![0x48, 0x8d, 0x05];
        x86.extend_from_slice(&disp.to_le_bytes());
        x86.push(0xc3);
        assert_eq!(decode_x86_64(&x86, 0x1000), Some(0x2000));
        // `endbr64` is skipped, not mistaken for the body: the displacement of
        // the body after it is relative to *its* end, not the function's start.
        let mut protected = vec![0xf3, 0x0f, 0x1e, 0xfa];
        let disp = i32::try_from(0x2000i64 - 0x100bi64).unwrap();
        protected.extend_from_slice(&[0x48, 0x8d, 0x05]);
        protected.extend_from_slice(&disp.to_le_bytes());
        protected.push(0xc3);
        assert_eq!(decode_x86_64(&protected, 0x1000), Some(0x2000));
        // Anything else is not this shape: a refusal, never a guess.
        assert_eq!(
            decode_x86_64(&[0xb8, 0x2b, 0x3a, 0x01, 0x00, 0xc3], 0x1000),
            None
        );

        // `adrp x0, page; add x0, x0, #0x345; ret` reaches 0x2000 + 0x345.
        let adrp = 0x9000_0000u32 | (1 << 29);
        let add = 0x9100_0000u32 | (0x345 << 10);
        let mut arm = Vec::new();
        arm.extend_from_slice(&adrp.to_le_bytes());
        arm.extend_from_slice(&add.to_le_bytes());
        arm.extend_from_slice(&0xd65f_03c0u32.to_le_bytes());
        assert_eq!(decode_aarch64(&arm, 0x1000), Some(0x2345));
        assert_eq!(decode_aarch64(&[0x1f, 0x20, 0x03, 0xd5], 0x1000), None);
    }

    /// Nothing matched to the tuple is a refusal that names the shim, the
    /// tuple, the directories and the package; a shim in a later directory is
    /// the accepted twin.
    #[test]
    fn the_shim_is_chosen_by_the_tuple_and_a_missing_one_names_it() {
        let scratch = Scratch::new("shim");
        let libphp = scratch.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        let first = Scratch::new("shim-first");
        let second = Scratch::new("shim-second");
        let search = PhpSearch {
            libphp: Some(libphp.clone()),
            shim_dirs: vec![first.dir.clone(), second.dir.clone()],
            lib_dirs: Vec::new(),
        };
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("no shim for the tuple")
            .to_string();
        assert!(
            message.contains(&format!("no shim libblastit_php84-{}.so", target_tuple())),
            "{message}"
        );
        assert!(
            message.contains(&format!("(PHP 8.4, {})", target_tuple())),
            "{message}"
        );
        assert!(
            message.contains(&first.dir.display().to_string())
                && message.contains(&second.dir.display().to_string()),
            "the refusal names every directory it looked in: {message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");

        // The accepted twin: the shim in the second directory is found.
        let shim = second.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let runtime = discover_php(&search).expect("the shim in the second directory");
        assert_eq!(runtime.shim, shim);

        // And the `dev` twin of the missing shim: `make php-sapi` is optional in
        // a development checkout, and `ADR-0010` §1 gives PHP one backend — so
        // `dev` refuses a missing shim exactly as a deployment does, naming the
        // tuple and the `make php-sapi` artifact that fixes it.
        let empty = Scratch::new("shim-dev-empty");
        let dev_search = PhpSearch {
            libphp: Some(libphp),
            shim_dirs: vec![empty.dir.clone()],
            lib_dirs: Vec::new(),
        };
        let dev = php_boot(&manifest(PHP_ONLY), &dev_search)
            .refusal()
            .expect("dev refuses a missing shim too")
            .to_string();
        assert!(
            dev.contains(&format!("libblastit_php84-{}.so", target_tuple())),
            "{dev}"
        );
    }

    /// `ADR-0010` §2's `workers` cap was downward only, and `ADR-0012` §12.3
    /// removed the keyword from the language: a `Blastfile` carries no per-view
    /// cap, so the deployment's budget is the only one there is and this gate
    /// has nothing to compare it against. The accepted case is the whole rule —
    /// the sibling that refused an over-cap `workers`, and the parameter that
    /// carried the budget, are deleted with the keyword.
    #[test]
    fn a_php_boot_with_no_workers_cap_passes_any_budget() {
        let scratch = Scratch::new("workers");
        let (_, resolved) = resolved_84(&scratch);
        assert!(
            matches!(
                php_boot(&manifest(PHP_ONLY), &resolved),
                PhpBoot::Proceed(_)
            ),
            "a view declares no cap, so the deployment's budget is the budget"
        );
    }

    /// The version-less refusal still carries the `ADR-0010` §4.1 tuple shape,
    /// with the version marked unknown; a name that states no `major.minor` is
    /// exactly that case.
    #[test]
    fn a_version_less_refusal_still_names_the_tuple_with_a_question_mark() {
        let scratch = Scratch::new("no-version");
        let shims = Scratch::new("no-version-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let unversioned = scratch.file("libphp.so", b"this is not an ELF shared object");
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&unversioned, &shims.dir))
            .refusal()
            .expect("not an ELF, and its name states no version")
            .to_string();
        assert!(
            message.contains(&format!("(PHP ?, {})", target_tuple())),
            "the tuple shape survives an unknown version: {message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
    }

    /// `Config::validate` refuses a relative `BLASTIT_LIBPHP` before a boot
    /// reaches the gate, and the gate refuses it too: `php_boot` is a pure
    /// function and the search is injectable, so the defence is tested where it
    /// lives rather than assumed.
    #[test]
    fn a_relative_libphp_is_refused_by_the_gate_too() {
        let search = PhpSearch {
            libphp: Some(PathBuf::from("libphp.so")),
            shim_dirs: Vec::new(),
            lib_dirs: Vec::new(),
        };
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("a relative override cannot be the library the boot inspected")
            .to_string();
        assert!(message.contains("BLASTIT_LIBPHP"), "{message}");
        assert!(message.contains("not an absolute path"), "{message}");
        assert!(message.contains("apt install libphp-embed"), "{message}");
    }

    /// The native twin: on this host the real distro library is inspected
    /// statically and resolves, and on a host without one the same gate names
    /// the package. Both branches are an assertion — neither is a skip.
    #[test]
    fn the_host_library_resolves_or_the_refusal_names_the_package() {
        let shims = Scratch::new("host-shims");
        for minor in [2u8, 3, 4] {
            let _ = shims.file(
                &shim_name(PhpVersion::new(8, minor)),
                b"never loaded at boot",
            );
        }
        let search = PhpSearch {
            libphp: None,
            shim_dirs: vec![shims.dir.clone()],
            lib_dirs: library_dirs(),
        };
        match discover_php(&search) {
            Ok(runtime) => {
                assert!(runtime.libphp.is_file(), "{:?}", runtime.libphp);
                assert!(runtime.dev > 0 && runtime.ino > 0);
                // The minor comes from the name, the per-minor directory or
                // the object's own bytes (`MB.3.59`); the shim is the one that
                // minor needs.
                assert_eq!(runtime.version.major(), 8);
                assert!(
                    (2..=4).contains(&runtime.version.minor()),
                    "{} is not a supported minor",
                    runtime.version.dotted()
                );
                assert!(
                    runtime
                        .shim
                        .file_name()
                        .and_then(OsStr::to_str)
                        .is_some_and(|name| name.contains(&runtime.version.number())),
                    "{} is not the shim for PHP {}",
                    runtime.shim.display(),
                    runtime.version.dotted()
                );
                assert!(
                    matches!(php_boot(&manifest(PHP_ONLY), &search), PhpBoot::Proceed(_)),
                    "this host resolves the pair"
                );
            }
            Err(refusal) => {
                let message = php_refusal_message("app", "public/index.php", &refusal);
                assert!(message.contains("apt install libphp-embed"), "{message}");
                assert!(message.contains("BLASTIT_LIBPHP"), "{message}");
            }
        }
    }

    /// `ADR-0010` §3: zero `dlopen` in stage 0. The gate opens, `fstat`s and
    /// reads the ELF; the load happens inside a view worker. A constructor
    /// canary proves this from the outside (`MB.3.13.4`); this proves it by
    /// construction, so no one adds the call while that suite is not looking.
    #[test]
    fn the_gate_reads_the_elf_and_never_loads_it() {
        const SOURCE: &str = include_str!("stage0.rs");
        // The needles are split so this test's own source does not match them.
        for banned in [
            concat!("dl", "open("),
            concat!("dlm", "open("),
            concat!("lib", "loading"),
            concat!("blastit_php", "_sapi"),
            concat!("Loaded", "::load"),
        ] {
            assert!(
                !SOURCE.contains(banned),
                "stage 0 must not load the library: found `{banned}`"
            );
        }
    }

    // ------------------------------------------- production provenance (R7)

    /// A `Scratch` under a directory only root or this process's uid can write.
    ///
    /// **A panic, never a `None`**: a `None` returned into a test body that then
    /// `return`s early is a test that asserts nothing on exactly the hosts where
    /// the rule is hardest to check (the reviewer's MINOR on `MB.3.40`). A host
    /// with no such directory is a host a deployment boot could not run on
    /// either, so the call-site tests would prove nothing there — and every
    /// rejected candidate is named with its reason, so the panic says why.
    ///
    /// Not [`Scratch`] itself: that one lives in `temp_dir()`, and `/tmp` is
    /// `1777` — a `0755` fixture *under* it is still a tree another uid could
    /// replace, which is exactly what these tests measure (`LESSONS.md`,
    /// `MB.3.40`). The candidates are the protected suite's
    /// (`tests/critical_php_embed.rs::protected_root`); the fixture is a
    /// [`Scratch`], so it is removed on the panic path too.
    fn protected_scratch(tag: &str) -> Scratch {
        use std::os::unix::fs::PermissionsExt;

        let mut candidates = Vec::new();
        if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
            candidates.push(PathBuf::from(dir));
        }
        candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"));
        if let Some(dir) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(dir));
        }
        for root in ["/opt", "/usr/local", "/srv"] {
            candidates.push(PathBuf::from(root));
        }
        let mut tried = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        for root in candidates {
            if !root.is_dir() {
                tried.push(format!("{}: not a directory", root.display()));
                continue;
            }
            let root = std::fs::canonicalize(&root).unwrap_or(root);
            if seen.contains(&root) {
                continue;
            }
            seen.push(root.clone());
            let dir = root.join(format!(
                "blastit-php-gate-protected-{}-{tag}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            if let Err(err) = std::fs::create_dir_all(&dir) {
                tried.push(format!("{}: create_dir_all failed ({err})", dir.display()));
                continue;
            }
            if let Err(err) = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
            {
                tried.push(format!("{}: chmod 0755 failed ({err})", dir.display()));
                let _ = std::fs::remove_dir_all(&dir);
                continue;
            }
            let probe = dir.join("probe");
            let reason = match std::fs::write(&probe, b"#!/bin/sh\n") {
                Err(err) => Some(format!("a probe file cannot be written ({err})")),
                Ok(()) => match crate::host::facts_of(&probe) {
                    Err(err) => Some(format!("the probe cannot be inspected ({err})")),
                    Ok(facts) => crate::host::protected(&facts, current_uid()).err(),
                },
            };
            let _ = std::fs::remove_file(&probe);
            match reason {
                Some(reason) => {
                    tried.push(format!("{}: {reason}", dir.display()));
                    let _ = std::fs::remove_dir_all(&dir);
                }
                None => return Scratch { dir },
            }
        }
        panic!(
            "no directory on this host is one uid's alone (tried [{}]), so a deployment boot \
             could not run on this host either and these tests would assert nothing",
            tried.join("; ")
        );
    }

    /// The accepted pair: a NTS stub `libphp` and the shim for its tuple.
    fn stub_pair(scratch: &Scratch) -> PhpSearch {
        let libphp = scratch.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        scratch.file(&shim_name(PhpVersion::new(8, 4)), &shared_object(&[]));
        search_for(&libphp, &scratch.dir)
    }

    /// `R7`/`B2-R5`: the deployment gate refuses a pair another uid could
    /// replace, keeping `ADR-0010` §4.1's sentence — and the *same* pair in a
    /// directory only root or init's uid can write proceeds, so the permission
    /// bits are the decision and not the path.
    #[test]
    fn a_deployment_boot_refuses_a_pair_a_route_uid_could_replace() {
        use std::os::unix::fs::PermissionsExt;

        let protected = protected_scratch("pair");
        let good = stub_pair(&protected);
        // `PHP_ONLY`'s front controller, at the accepted shape: a tree claim
        // that does not exist is a refusal of its own (`facts_of`'s error).
        std::fs::create_dir_all(protected.dir.join("public")).unwrap();
        let entry = protected.dir.join("public/index.php");
        std::fs::write(&entry, b"<?php echo 1;\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        let runtime = discover_php(&good).expect("a stub pair this host accepts");
        assert!(
            crate::host::Provenance {
                init_uid: current_uid(),
                objects: vec![
                    Claim::new("the NTS libphp", runtime.libphp.clone()),
                    Claim::new("the embedded shim", runtime.shim.clone()),
                ],
                trees: Vec::new(),
            }
            .check_objects()
            .is_ok(),
            "a 0755 pair owned by init's uid is the accepted shape"
        );

        // The library's directory, writable by another uid.
        let shared = Scratch::new("pair-shared");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let search = stub_pair(&shared);
        let checks = HostChecks::new(current_uid());
        let deployment = checks.deployment(&protected.dir);
        let message = php_boot_in(&manifest(PHP_ONLY), &search, Some(&deployment))
            .refusal()
            .expect("a writable libphp directory cannot be a deployment")
            .to_string();
        // `ADR-0010` §4.1's shape: the route, its entry, the offending path, the
        // tuple and the shim, the package, and the override.
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("entrypoint php \"public/index.php\""),
            "{message}"
        );
        assert!(message.contains("the NTS libphp"), "{message}");
        assert!(
            message.contains(&search.libphp.as_ref().unwrap().display().to_string()),
            "the offending library is named: {message}"
        );
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );
        assert!(message.contains("chown"), "and the fix: {message}");
        assert!(message.contains("(PHP 8.4, "), "{message}");
        assert!(
            message.contains(&shim_name(PhpVersion::new(8, 4))),
            "{message}"
        );
        assert!(
            message.contains("install the PHP embed package"),
            "{message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );

        // The shim is the pair's other half: a protected library with a shim in
        // a writable directory is still a pair a route uid owns.
        let shims = protected.dir.join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::set_permissions(&shims, std::fs::Permissions::from_mode(0o777)).unwrap();
        let shim = shims.join(shim_name(PhpVersion::new(8, 4)));
        std::fs::write(&shim, shared_object(&[])).unwrap();
        let search = search_for(&protected.dir.join("libphp8.4.so"), &shims);
        let message = php_boot_in(&manifest(PHP_ONLY), &search, Some(&deployment))
            .refusal()
            .expect("a writable shim directory cannot be a deployment")
            .to_string();
        assert!(message.contains("the embedded shim"), "{message}");
        assert!(
            message.contains(&shim.display().to_string()),
            "the offending shim is named: {message}"
        );
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );

        // And the same pair, once its directories are the accepted shape, is
        // what a deployment boots: nothing in `dev`'s answer changed.
        assert!(
            matches!(
                php_boot_in(&manifest(PHP_ONLY), &good, Some(&deployment)),
                PhpBoot::Proceed(_)
            ),
            "a 0755 pair plus a protected tree must not be refused"
        );
    }

    /// `B2-R5`: the tree the worker's `cwd`/`include_path` is — the `Blastfile`'s
    /// directory, each `entrypoint php` and each `static` root — is
    /// refused when a route uid could write it, in its own sentence (this one is
    /// fixed by `chown`/`chmod`, not by installing a package).
    #[test]
    fn a_deployment_boot_refuses_a_tree_a_route_uid_could_write() {
        use std::os::unix::fs::PermissionsExt;

        let protected = protected_scratch("tree");
        let search = stub_pair(&protected);
        // One PHP route and one static route, on disjoint trees: a manifest that
        // declares both blocks on *one* route must keep the entry under the
        // root, which would make the two legs interfere.
        let manifest_source = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}

view docs regex '/assets/.*' {}

static \"/assets\" {
    root \"site\"
}
";
        // The accepted shape this test starts from: the entry at `0644`, the
        // static root at `0755`, both owned by init's uid.
        std::fs::create_dir_all(protected.dir.join("public")).unwrap();
        let entry = protected.dir.join("public/index.php");
        std::fs::write(&entry, b"<?php echo 1;\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        let root = protected.dir.join("site");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let checks = HostChecks::new(current_uid());
        let accepted = checks.deployment(&protected.dir);
        assert!(
            matches!(
                php_boot_in(&manifest(manifest_source), &search, Some(&accepted)),
                PhpBoot::Proceed(_)
            ),
            "a 0755 tree owned by init's uid is the accepted shape: {:?}",
            php_boot_in(&manifest(manifest_source), &search, Some(&accepted))
        );

        // 1. The directory the manifest was read from.
        let shared = Scratch::new("tree-shared");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let checks = HostChecks::new(current_uid());
        let message = php_boot_in(
            &manifest(manifest_source),
            &search,
            Some(&checks.deployment(&shared.dir)),
        )
        .refusal()
        .expect("a writable manifest directory cannot be a deployment")
        .to_string();
        assert!(
            message.contains("the directory holding the Blastfile"),
            "the tree is named for what it is: {message}"
        );
        assert!(
            message.contains(&shared.dir.display().to_string()),
            "{message}"
        );
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );
        assert!(message.contains("chown"), "the fix: {message}");
        assert!(
            !message.contains("install the PHP embed package"),
            "a tree is not fixed by a package: {message}"
        );

        // 2. The front controller itself, world-writable inside a protected tree.
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o666)).unwrap();
        let message = php_boot_in(&manifest(manifest_source), &search, Some(&accepted))
            .refusal()
            .expect("a writable front controller cannot be a deployment")
            .to_string();
        assert!(
            message.contains("route \"app\": entrypoint php \"public/index.php\""),
            "the entry is named with its route: {message}"
        );
        assert!(message.contains(&entry.display().to_string()), "{message}");
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();

        // 3. A static root: the responder reads it, and the manifest declares
        // one next to the PHP route.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let message = php_boot_in(&manifest(manifest_source), &search, Some(&accepted))
            .refusal()
            .expect("a writable static root cannot be a deployment")
            .to_string();
        assert!(
            message.contains("static \"/assets\": root \"site\""),
            "the static root is named with its route: {message}"
        );
        assert!(message.contains(&root.display().to_string()), "{message}");

        // The same tree at `0755` is accepted: the bits, not the path.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(
                php_boot_in(&manifest(manifest_source), &search, Some(&accepted)),
                PhpBoot::Proceed(_)
            ),
            "the same tree at 0755 is what a deployment boots"
        );
    }

    /// `R13`: a path a PHP manifest names that is **not there** is refused with
    /// the path and "no such file or directory", not with the provenance
    /// sentence's "it cannot be inspected (No such file or directory)" — which
    /// reads as a permissions problem and sends the operator to `chmod` a file
    /// that does not exist (the reviewer's MINOR on `MB.3.40`).
    #[test]
    fn a_missing_entry_or_static_root_names_the_missing_path() {
        use std::os::unix::fs::PermissionsExt;

        let protected = protected_scratch("missing");
        let search = stub_pair(&protected);
        std::fs::create_dir_all(protected.dir.join("public")).unwrap();
        let entry = protected.dir.join("public/index.php");
        std::fs::write(&entry, b"<?php echo 1;\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        let checks = HostChecks::new(current_uid());
        let deployment = checks.deployment(&protected.dir);
        // A PHP route and a static route, as in the tree test above: the gate
        // only runs for a manifest with an `entrypoint php`, so the static half must
        // ride beside one.
        let source = |entry: &str, root: &str| {
            format!(
                "entrypoint php \"{entry}\"\n\nview app regex '.*' {{}}\n\nview docs regex \
                 '/assets/.*' {{}}\n\nstatic \"/assets\" {{\n    root \"{root}\"\n}}\n"
            )
        };
        let refuse = |source: &str| {
            php_boot_in(&manifest(source), &search, Some(&deployment))
                .refusal()
                .expect("a manifest naming a path that is not there cannot be a deployment")
                .to_string()
        };

        // 1. The front controller.
        let absent = "public/missing.php";
        let missing = protected.dir.join(absent);
        std::fs::create_dir_all(protected.dir.join("site")).unwrap();
        std::fs::set_permissions(
            protected.dir.join("site"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let message = refuse(&source(absent, "site"));
        assert!(message.contains(absent), "the entry is named: {message}");
        assert!(
            message.contains(&missing.display().to_string()),
            "and the path it resolves to: {message}"
        );
        assert!(
            message.contains("no such file or directory"),
            "a missing path says so: {message}"
        );
        assert!(
            !message.contains("cannot be inspected"),
            "a path that is not there is not an unreadable one: {message}"
        );

        // 2. A `[route.static] root`.
        std::fs::remove_dir_all(protected.dir.join("site")).unwrap();
        let site = protected.dir.join("site");
        let message = refuse(&source("public/index.php", "site"));
        assert!(
            message.contains("static \"/assets\": root \"site\""),
            "the static root is named with its route: {message}"
        );
        assert!(
            message.contains(&site.display().to_string()),
            "and the path it resolves to: {message}"
        );
        assert!(
            message.contains("no such file or directory"),
            "a missing path says so: {message}"
        );
        assert!(
            !message.contains("cannot be inspected"),
            "a path that is not there is not an unreadable one: {message}"
        );

        // 3. A **dangling symlink** is the same answer, and that is what
        // `fs::metadata` means by `NotFound`: it follows the link, so the file
        // the boot would read is the one that is not there.
        let dangling = protected.dir.join("public/dangling.php");
        std::os::unix::fs::symlink(protected.dir.join("public/gone.php"), &dangling)
            .expect("a dangling symlink");
        let message = refuse(&source("public/dangling.php", "site"));
        assert!(
            message.contains("public/dangling.php") && message.contains("is missing"),
            "a dangling symlink is missing, not unreadable: {message}"
        );
        assert!(!message.contains("cannot be inspected"), "{message}");
        std::fs::remove_file(&dangling).unwrap();

        // And the same manifest with both paths created is what a deployment
        // boots, so the two refusals are the missing path and nothing else.
        std::fs::create_dir_all(&site).unwrap();
        std::fs::set_permissions(&site, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(
                php_boot_in(
                    &manifest(&source("public/index.php", "site")),
                    &search,
                    Some(&deployment)
                ),
                PhpBoot::Proceed(_)
            ),
            "the same manifest with both paths there is what a deployment boots"
        );
    }

    /// The seam's contract (`MB.3.44`): `Ok(())` is "the gate proceeds" and
    /// `Err` is a **refusal**.
    ///
    /// A deployment whose manifest declares no `entrypoint php` has no pair
    /// and no entry to authorise — `php_boot_in` answers `Absent` and the boot
    /// proceeds, exactly as `dev`/`check` do. Mapping that to `Err` let a caller
    /// read a *proceeding* manifest as a refused one, which is the confusion the
    /// seam exists to prevent, so both arms are pinned here.
    #[test]
    fn the_deployment_seam_proceeds_on_an_absent_gate_and_refuses_a_bad_pair() {
        use std::os::unix::fs::PermissionsExt;

        let protected = protected_scratch("seam");
        std::fs::create_dir_all(protected.dir.join("public")).unwrap();
        let entry = protected.dir.join("public/index.php");
        std::fs::write(&entry, b"<?php echo 1;\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _pair = stub_pair(&protected);
        let libphp = protected.dir.join("libphp8.4.so");

        // 1. No `entrypoint php`: nothing to authorise, so the boot proceeds.
        let static_only = "\
static \"/\" {
    root \"site\"
}
";
        assert_eq!(
            deployment_boot_check(static_only, &protected.dir, &libphp, &protected.dir, 20_000),
            Ok(()),
            "a manifest with no entrypoint php proceeds through the deployment gate"
        );

        // 2. A refused pair still produces `ADR-0010` §4.1's sentence, so arm 1
        // is not passing because the seam never looks.
        let shared = Scratch::new("seam-shared");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let writable = shared.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        shared.file(&shim_name(PhpVersion::new(8, 4)), &shared_object(&[]));
        let message =
            deployment_boot_check(PHP_ONLY, &protected.dir, &writable, &shared.dir, 20_000)
                .expect_err("a pair another uid could replace must refuse the boot");
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );
        assert!(
            message.contains("install the PHP embed package"),
            "{message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );

        // 3. And the accepted pair at 0755 is `Ok`, so the refusal above is the
        // permission bits and nothing else.
        assert_eq!(
            deployment_boot_check(PHP_ONLY, &protected.dir, &libphp, &protected.dir, 20_000),
            Ok(()),
            "a 0755 pair owned by init's uid is what a deployment boots"
        );
    }

    /// The other half of the scope rule: a `dev`/`check` boot is handed the very
    /// same paths and must not ask the production question at all — the shim a
    /// developer just built lives in a checkout that uid writes.
    #[test]
    fn a_development_boot_does_not_apply_the_production_rule() {
        use std::os::unix::fs::PermissionsExt;

        let shared = Scratch::new("dev-scope");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let search = stub_pair(&shared);
        assert!(
            matches!(php_boot(&manifest(PHP_ONLY), &search), PhpBoot::Proceed(_)),
            "`dev`/`check` must keep booting out of a world-writable checkout"
        );
        // And the production question about those same bytes is a refusal, so
        // this test cannot pass by the fixture being acceptable after all.
        let checks = HostChecks::new(current_uid());
        let message = php_boot_in(
            &manifest(PHP_ONLY),
            &search,
            Some(&checks.deployment(&shared.dir)),
        )
        .refusal()
        .expect("the same bytes are refused by a deployment")
        .to_string();
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );
    }
}

/// `R13` after `ADR-0010` §2: an `entrypoint php` file is its own executor, so the
/// app rule must never demand the WSGI callable for it (`MB.3.5.4`).
///
/// The old predicate read "not all-static" as "needs an app" and refused a
/// PHP-only manifest with `BLASTIT_APP: the WSGI callable (…) is required` —
/// advice that cannot make a view serve. What the rule actually asks is
/// [`Manifest::can_boot_without_app`]: does every request reach an executor the
/// file owns?
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod app_rule_executors {
    use super::*;

    /// A PHP route with no app: the app rule must not demand the callable.
    /// (What an `entrypoint php` boot needs is a usable NTS `libphp`, and
    /// [`php_boot_gate`] is the gate that asks; that is a different sentence.)
    const PHP_ONLY: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

    /// Files from disk, everything else to the front controller: also no app.
    const STATIC_AND_PHP: &str = "\
entrypoint php \"public/index.php\"

static \"/\" {
    root \"public\"
}

view app regex '.*' {}
";

    /// One route that declares no executor: a WSGI view, which needs the app.
    const WITH_A_VIEW: &str = "\
view web regex '.*' {}
";

    /// The sentence the rule refuses with, exactly.
    const REQUIRE_APP: &str = "BLASTIT_APP: the WSGI callable (`module:attribute`) is required";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        manifest
    }

    /// Every own-executor shape needs no `app`: PHP only, and the page's
    /// static-plus-PHP route.
    #[test]
    fn a_php_manifest_needs_no_app() {
        for source in [PHP_ONLY, STATIC_AND_PHP] {
            assert_eq!(
                app_rule(&Config::default(), &manifest(source)),
                Ok(()),
                "{source}"
            );
        }
    }

    /// The refusal case did not vanish: a manifest with no `app` and no route
    /// that runs its own executor still refuses, with the sentence that names
    /// the setting — and never the Python-free build's `uv add` advice, which
    /// would send a PHP developer to install a package the route cannot use.
    #[test]
    fn a_manifest_with_no_executor_still_requires_the_callable() {
        // A `Blastfile` declares one entrypoint for the whole file (§12.2), so
        // "a PHP view beside a WSGI view" is not a shape the language offers;
        // the refusal is measured on the shapes that need a callable.
        for source in [WITH_A_VIEW, "", "view web regex '.*' {\n    body json\n}\n"] {
            let message = app_rule(&Config::default(), &manifest(source)).unwrap_err();
            assert_eq!(message, REQUIRE_APP, "{source}");
            assert!(
                !message.contains("uv add"),
                "the app rule never names the package: {message}"
            );
        }
    }

    /// `BLASTIT_APP` still satisfies the rule for a WSGI view, and the
    /// all-static-with-an-app refusal is untouched by the wider predicate.
    #[test]
    fn the_two_halves_of_the_rule_still_hold() {
        let with_app = Config {
            app: Some("web.wsgi:app".to_string()),
            ..Config::default()
        };
        assert_eq!(app_rule(&with_app, &manifest(WITH_A_VIEW)), Ok(()));

        let static_only = "static \"/\" {\n    root \"site\"\n}\n";
        assert_eq!(app_rule(&Config::default(), &manifest(static_only)), Ok(()));
        let message = app_rule(&with_app, &manifest(static_only)).unwrap_err();
        assert!(
            message.starts_with("BLASTIT_APP: the file serves static mounts and no views"),
            "{message}"
        );
    }

    /// §9.5/§12.2: an `entrypoint php` no view reaches needs **no**
    /// `BLASTIT_APP`. This is the arm the boot's own predicate used to disagree
    /// on: it demanded at least one view for the PHP half, so a file that is a
    /// mount and a front controller was refused with "the WSGI callable is
    /// required" — advice that cannot make it serve. The boot now asks the
    /// model's one answer.
    ///
    /// `EntrypointNotReached` (the reminder for such a file) is `MB.21.3.0`'s
    /// warning and lives in `validate.rs`; it is not in the model yet
    /// (`critical_blastfile_entrypoint.rs` records the same gap), so what this
    /// fixture set pins is the half this leaf owns: no missing app, and not a
    /// static file.
    #[test]
    fn a_php_entrypoint_no_view_reaches_needs_no_app() {
        // The bare shape is the discriminating one: no view and no mount, so the
        // boot's old "PHP needs a view" reading demanded the callable while the
        // model already said this file is its own executor.
        let source = "entrypoint php \"public/index.php\"\n";
        let (front_controller, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        assert!(
            front_controller.can_boot_without_app(),
            "§12.2 answers true"
        );
        assert!(
            !front_controller.is_all_static(),
            "a front controller is not static"
        );
        assert_eq!(app_rule(&Config::default(), &front_controller), Ok(()));

        // The page's shape — a mount beside the front controller — is bootable
        // without an app too.
        let with_mount = "entrypoint php \"public/index.php\"\n\
                          static \"/assets\" {\n    root \"public/assets\"\n}\n";
        assert_eq!(app_rule(&Config::default(), &manifest(with_mount)), Ok(()));

        // The deny side is the same view with no entrypoint and no mount: nothing
        // answers it, so the callable is required again.
        let without_entrypoint = "view web regex '.*' {}\n";
        assert_eq!(
            app_rule(&Config::default(), &manifest(without_entrypoint)),
            Err(REQUIRE_APP.to_string())
        );
    }
}

/// `ADR-0011` (`MB.3.53`): the production path's ingress rule, as the serve path
/// calls it.
///
/// The stage-1 half is measured by a real production process
/// (`crates/blastit/tests/critical_raw_ingress.rs`); this half is the rule the
/// serve path calls, and it is a unit test because an unprivileged `cargo test`
/// cannot reach a stage-0 gate that lives after the manifest parse — stage 0
/// reads its capability sets first and refuses an unprivileged boot there
/// (`blastit_sys::caps::detect`).
#[cfg(test)]
mod raw_ingress_gate_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{Manifest, ServeGates, raw_ingress_gate};

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        manifest
    }

    const REFUSED: &str = "\
app \"app:app\"

view upload POST \"/upload\" {
    body raw
}
";

    const ACCEPTED: &str = "\
app \"app:app\"

view upload POST \"/upload\" {
    body raw unvalidated
}
";

    /// The gate has exactly one refusal, and it is §10.2's sentence: it names
    /// the view, what was declared (`body raw`) and the node to write
    /// (`body raw unvalidated`); the acknowledgement opens it, which is what
    /// makes this a gate rather than a blanket refusal of the codec.
    #[test]
    fn an_unacknowledged_raw_view_is_refused_and_an_acknowledged_one_is_not() {
        let message = raw_ingress_gate(&manifest(REFUSED)).unwrap_err();
        assert_eq!(
            message,
            "view \"upload\": body raw hands the request body to the application unvalidated, and \
             the production path refuses it without the acknowledgement; write `body raw \
             unvalidated` in that view, or declare the codec it takes (json, form, none)",
            "§10.2, verbatim"
        );
        assert_eq!(raw_ingress_gate(&manifest(ACCEPTED)), Ok(()));
    }

    /// A structured codec is untouched by the gate: only `raw` has no runtime
    /// validation for the acknowledgement to stand in for.
    #[test]
    fn every_other_codec_and_no_ingress_at_all_pass_the_gate() {
        for body in ["json", "form", "none"] {
            let source = format!(
                "app \"app:app\"\n\nview upload POST \"/upload\" {{\n    body {body}\n}}\n"
            );
            assert_eq!(
                raw_ingress_gate(&manifest(&source)),
                Ok(()),
                "body {body} needs no acknowledgement"
            );
        }
        let no_ingress = "app \"app:app\"\n\nview home regex '.*' {}\n";
        assert_eq!(raw_ingress_gate(&manifest(no_ingress)), Ok(()));
    }

    /// Every offender is named, in route order, so a manifest edited in two
    /// places is fixed in one boot rather than one denial at a time.
    #[test]
    fn every_unacknowledged_route_is_named_in_route_order() {
        let source = REFUSED.replace("view upload", "view first")
            + "\nview second POST \"/second\" {\n    body raw\n}\n";
        let message = raw_ingress_gate(&manifest(&source)).unwrap_err();
        assert_eq!(message.lines().count(), 2, "{message}");
        assert!(
            message.lines().next().unwrap().contains("\"first\""),
            "{message}"
        );
        assert!(
            message.lines().nth(1).unwrap().contains("\"second\""),
            "{message}"
        );
    }

    /// `MB.3.54` (item 3): the serve path cannot reach a bound view socket
    /// without the gate. The ordering half is [`bind_view_sockets`]'s
    /// `&ServeGates` parameter — a compile error, not a test, which is why it is
    /// the witness an unprivileged suite cannot be. This is the half a test can
    /// hold: the proof is withheld from exactly the manifest the gate refuses,
    /// and granted to the acknowledged one.
    #[test]
    fn the_serve_path_proof_is_withheld_from_a_refused_manifest() {
        assert!(
            ServeGates::clear(&manifest(REFUSED)).is_err(),
            "an unacknowledged raw route must not hand out the proof the bind needs"
        );
        assert!(
            ServeGates::clear(&manifest(ACCEPTED)).is_ok(),
            "the acknowledgement is what opens the gate"
        );
    }
}
