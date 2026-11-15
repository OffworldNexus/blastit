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
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use blastit_dbproxy::{DbproxyConfig, ListenerSpec, ProxySpec, write_config};
use blastit_events::{
    Component, Event, EventKind, Level, Lifecycle, ManifestWarning as WarningEvent, Stream,
};
use blastit_manifest::{Manifest, PgGrant, ProxyName, UidLayout};
use blastit_sys::{
    Fork, Pid, Rlimits, Signal, bind_unix_listener, current_signal_mask, exec, harden_child,
    make_private_dir, pipe, restore_signal_mask_in_child, set_cloexec, set_dumpable,
};

use crate::child::{ChildIo, Deadline, read_one, wait_readable};
use crate::cli::DevOptions;
use crate::config::{Config, LogFormat, Mode, PhpRuntime, PhpVersion, Privilege, view_dir};
use crate::dev_learn::{DevRoot, PRIVILEGE};
use crate::host::{Claim, EXEC, Host, Install, Placement, Provenance, Reexec};
use crate::logs::{Emitter, fail, output_event};
use crate::mem_usage;

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
    (manifest.is_all_static() && config.app.is_some()).then(|| {
        "BLASTIT_APP: every route is static, so no route can reach an app: \
         unset BLASTIT_APP, or declare a route without [route.static]"
            .to_string()
    })
}

/// `R13`: the app rule a **boot** refuses with (`blastit`, `blastit dev`).
///
/// Both halves: a `BLASTIT_APP` no route could reach is refused
/// ([`static_app_refusal`]), and a manifest whose routes do **not** all run
/// their own executor needs a callable to forward to. The second half asks
/// [`Manifest::can_boot_without_app`] — "every route is its own executor" —
/// rather than "every route is static": a `[route.php]` route is its own
/// executor, so the *rule* demands no `app` for a PHP-only manifest
/// (`ADR-0010` §2); what such a boot needs instead is a usable NTS `libphp`,
/// and [`php_boot_gate`] is what refuses a host that has none.
/// `blastit check` must not take the second half — a suite brings its own
/// `app` — so it calls [`static_app_refusal`] directly.
pub(crate) fn app_rule(config: &Config, manifest: &Manifest) -> Result<(), String> {
    if let Some(message) = static_app_refusal(config, manifest) {
        return Err(message);
    }
    if !manifest.can_boot_without_app() && config.app.is_none() {
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
/// Stage 1 calls this too, immediately after its own `parse_and_validate` of the
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
/// sentence in both builds. So does a `[route.php]` route: it runs in a `view`
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
    if manifest.can_boot_without_app() {
        return None;
    }
    if let Some(app) = config.app.as_deref() {
        return Some(format!(
            "BLASTIT_APP ({app}): this build has no Python support (built without feature \
             \"python\"), so it cannot import a WSGI application; {INSTALL}"
        ));
    }
    // The first route that is not its own executor: a `[route.static]` root and
    // a `[route.php]` front controller both answer without an interpreter, so
    // only a route that declares neither is a WSGI view this build cannot fork.
    let route = manifest
        .routes()
        .iter()
        .find(|route| route.static_policy().is_none() && route.php().is_none())?;
    Some(format!(
        "route {:?}: this build has no Python support (built without feature \"python\"), \
         and the route declares no [route.static] block, so it needs a runtime this build \
         does not carry; {INSTALL}, or add a [route.static] block",
        route.name()
    ))
}

/// The distro file names a host NTS `libphp` is looked for under, newest first
/// (`ADR-0010` §3).
///
/// A fixed list rather than a directory scan: the order *is* the preference, so
/// a `libphp8.5.so` somebody dropped into `/usr/local/lib` never changes which
/// library an 8.4 host embeds. Both spellings the distros install are here: the
/// dotted Debian/Ubuntu name and the **packed** Alpine 8.3 one (`libphp83.so`;
/// Alpine's 8.4 and 8.5 state their minor in `/usr/lib/php84` and
/// `/usr/lib/php85` instead, `MB.3.59`), so a host with `BLASTIT_LIBPHP` unset
/// reaches Alpine's package without a copy.
///
/// The unversioned `libphp.so` is last here because on its own it states no
/// minor; the per-minor directory it sits in states one, which is why
/// [`LIBPHP_PROBES`] also tries it there, under that minor (`MB.3.61`).
pub(crate) const LIBPHP_CANDIDATES: [&str; 10] = [
    "libphp8.5.so",
    "libphp-8.5.so",
    "libphp85.so",
    "libphp8.4.so",
    "libphp-8.4.so",
    "libphp84.so",
    "libphp8.3.so",
    "libphp-8.3.so",
    "libphp83.so",
    "libphp.so",
];

/// One step of the discovery order: a file name, and which of
/// [`PhpSearch::lib_dirs`] it is looked for in (`MB.3.61`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LibphpProbe {
    /// The file name, as [`LIBPHP_CANDIDATES`] spells it.
    name: &'static str,
    /// `None`: every directory, in [`PhpSearch::lib_dirs`] order. `Some("php84")`:
    /// only the per-minor embed directories whose last component is that name, so
    /// a `libphp.so` there is the 8.4 library the *path* names.
    per_minor_dir: Option<&'static str>,
}

/// The order [`first_existing`] walks: **version-major**, then spelling
/// (`MB.3.61`).
///
/// Each minor's own spellings — dotted, hyphenated, packed, and the per-minor
/// `libphp.so` — are all tried before the next minor's, so the newest *library*
/// wins rather than the first name in a list. That is what makes a host with two
/// embed packages installed serve from the newest one: Alpine's `php83-embed`
/// puts `libphp83.so` in `/usr/lib` and `php85-embed` puts `libphp.so` in
/// `/usr/lib/php85`, and a name-major walk preferred `libphp83.so` — an older
/// library — while the 8.5 library beside it would serve.
///
/// The unversioned `libphp.so` in the plain directories stays last: its minor is
/// only known from the descriptor's link target or the object's own bytes, so it
/// is a fallback, not a preference.
const LIBPHP_PROBES: [LibphpProbe; 13] = [
    LibphpProbe {
        name: "libphp8.5.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp-8.5.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp85.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp.so",
        per_minor_dir: Some("php85"),
    },
    LibphpProbe {
        name: "libphp8.4.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp-8.4.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp84.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp.so",
        per_minor_dir: Some("php84"),
    },
    LibphpProbe {
        name: "libphp8.3.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp-8.3.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp83.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp.so",
        per_minor_dir: Some("php83"),
    },
    LibphpProbe {
        name: "libphp.so",
        per_minor_dir: None,
    },
];

/// The distributions' PHP embed packages, named once so every refusal says the
/// same thing (`ADR-0010` §4.1).
const PHP_PACKAGES: &str = "install the PHP embed package — Debian/Ubuntu: \
     apt install libphp-embed, Alpine: apk add php85-embed, Fedora/RHEL: \
     dnf install php-embedded";

/// The minors this build ships a shim for, as one sentence.
///
/// `docs/frameworks/php.md` states the same three minors and nothing else; a
/// host on another minor is refused with exactly this sentence
/// ([`PhpRefusal::UnsupportedVersion`]).
const SUPPORTED_PHP: &str = "PHP 8.3, 8.4 and 8.5";

/// The one override `ADR-0010` §3 allows.
const PHP_OVERRIDE: &str = "set BLASTIT_LIBPHP=/path/to/libphp.so";

/// The variable that tunes how long a stage-1 `[route.php]` probe may take
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
            "{PROBE_TIMEOUT_VAR}=0 would kill every [route.php] probe before it starts; set it \
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
    Ok(value)
}

/// `ADR-0010` §4.1's sentence for a **stage-1 probe** refusal.
///
/// The stage-0 gate builds the same shape from the same two halves
/// ([`present_refusal`]); this is the post-barrier twin, so a shim that will not
/// load, an ABI this build does not speak, a minor that disagrees with the
/// library, or a `worker_init` that hangs reads exactly like a discovery
/// refusal and names the same fix.
#[cfg(feature = "php")]
pub(crate) fn php_probe_refusal(
    route: &str,
    entry: &str,
    problem: &str,
    version: Option<PhpVersion>,
) -> String {
    present_refusal(
        &format!("route {route:?}: [route.php] entry {entry:?}: "),
        problem,
        version,
    )
}

/// The compile-time target's architecture, as the shim name spells it.
///
/// Deliberately a second spelling of the tuple in `crates/blastit-php-sapi`
/// (`Minor::shim_file()`): that crate is an *optional* dependency of this
/// binary (`php` feature) while this discovery gate compiles on every build, so
/// its tuple cannot be imported here unconditionally (and the no-load guard
/// below bans naming the loader crate at all).
/// `the_shim_name_matches_the_adr_literal` pins this spelling to the ADR-0010
/// §4.1 literals, so the two cannot drift.
const fn target_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    }
}

/// The compile-time target's libc, as the shim name spells it (`ADR-0010` §4).
///
/// Pinned by `the_shim_name_matches_the_adr_literal` for the same reason as
/// [`target_arch`].
const fn target_libc() -> &'static str {
    if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    }
}

/// `x86_64-gnu`: the half of the ABI tuple that is fixed at compile time.
fn target_tuple() -> String {
    format!("{}-{}", target_arch(), target_libc())
}

/// `x86_64-linux-gnu`: the multi-arch directory name a distro installs into.
fn multiarch_triplet() -> String {
    format!("{}-linux-{}", target_arch(), target_libc())
}

/// The shim for one host version: `libblastit_php84-x86_64-gnu.so`.
fn shim_name(version: PhpVersion) -> String {
    format!("libblastit_php{}-{}.so", version.number(), target_tuple())
}

/// The multi-arch library directories a distro may put `libphp` in, in order
/// (`ADR-0010` §3).
///
/// The per-minor directories come after the plain `/usr/lib` and before the
/// `/usr/local` half: Alpine installs 8.4 as `/usr/lib/php84/libphp.so` and 8.5
/// as `/usr/lib/php85/libphp.so`, and with `BLASTIT_LIBPHP` unset a fixed name
/// alone cannot reach them — the directory is half of the package's spelling
/// (`MB.3.59`). `/usr/local/lib/php8x` is the twin of a copy an operator put
/// there.
///
/// The directory order is *within* one minor: [`LIBPHP_PROBES`] picks the minor
/// first, so `/usr/lib/php85` outranks `/usr/lib/libphp84.so` even though it is
/// listed later (`MB.3.61`).
fn library_dirs() -> Vec<PathBuf> {
    let triplet = multiarch_triplet();
    [
        format!("/usr/lib/{triplet}"),
        "/usr/lib64".to_string(),
        "/usr/lib".to_string(),
        "/usr/lib/php85".to_string(),
        "/usr/lib/php84".to_string(),
        "/usr/lib/php83".to_string(),
        format!("/usr/local/lib/{triplet}"),
        "/usr/local/lib".to_string(),
        "/usr/local/lib/php85".to_string(),
        "/usr/local/lib/php84".to_string(),
        "/usr/local/lib/php83".to_string(),
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

/// Whether the discovery may look in the development shim directory.
///
/// `ADR-0010` §4 and `R8`: `BLASTIT_PHP_SHIM_DIR` and `target/php-sapi` are the
/// development and `check` seams. A deployed boot takes the shims the artifact
/// ships — beside the executable — and nothing an environment variable names,
/// because a shim is code that runs inside every PHP worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhpShimScope {
    /// `blastit`: beside the executable only.
    Deployment,
    /// `blastit dev` and `blastit check`: `BLASTIT_PHP_SHIM_DIR`, then beside
    /// the executable, then the `target/php-sapi` directory `make php-sapi`
    /// writes.
    Development,
}

/// Everything the `[route.php]` discovery looks at, injectable so a test never
/// depends on the host's `/usr/lib`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PhpSearch {
    /// `BLASTIT_LIBPHP`: when set, the only candidate.
    pub libphp: Option<PathBuf>,
    /// The directories a shim is looked for in, in order.
    pub shim_dirs: Vec<PathBuf>,
    /// The directories the fixed names in [`LIBPHP_CANDIDATES`] are looked for
    /// in, in order.
    pub lib_dirs: Vec<PathBuf>,
}

impl PhpSearch {
    /// The real directories for `scope`, with an explicit override.
    fn new(libphp: Option<PathBuf>, shim_dir: Option<PathBuf>, scope: PhpShimScope) -> Self {
        let mut shim_dirs = Vec::new();
        if scope == PhpShimScope::Development {
            shim_dirs.extend(shim_dir);
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            shim_dirs.push(dir.to_path_buf());
        }
        // `make php-sapi` writes `target/php-sapi`; finding it from the running
        // executable is what makes it independent of the working directory a
        // developer started the boot in.
        if scope == PhpShimScope::Development
            && let Ok(exe) = std::env::current_exe()
            && let Some(target) = exe
                .ancestors()
                .find(|dir| dir.file_name() == Some(OsStr::new("target")))
        {
            shim_dirs.push(target.join("php-sapi"));
        }
        Self {
            libphp,
            shim_dirs,
            lib_dirs: library_dirs(),
        }
    }

    /// What a boot's configuration names, and the host's fixed directories.
    fn from_config(config: &Config, scope: PhpShimScope) -> Self {
        Self::new(config.libphp.clone(), config.php_shim_dir.clone(), scope)
    }
}

/// What the static `.dynsym` inspection says about one shared object
/// (`ADR-0010` §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZendAbi {
    /// `executor_globals` is exported and `tsrm_startup` is not: the
    /// Non-Thread-Safe build, the only one an embedded worker may load.
    Nts,
    /// `tsrm_startup` is exported: a thread-safe (ZTS) build.
    Zts,
    /// Neither symbol is exported: not a `libphp` at all.
    NotLibphp,
}

/// Why the `[route.php]` gate refused, before the sentence names the route.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PhpRefusal {
    /// No `BLASTIT_LIBPHP` and no candidate file: nothing to embed.
    Missing {
        /// The directories that were searched.
        searched: Vec<PathBuf>,
    },
    /// `BLASTIT_LIBPHP` is not an absolute path.
    NotAbsolute {
        /// The value, as given.
        path: PathBuf,
    },
    /// The file could not be opened, or is not a regular file.
    NotAFile {
        /// The candidate.
        path: PathBuf,
        /// The kernel's reason.
        detail: String,
    },
    /// The file is not an ELF shared object this build can inspect.
    NotElf {
        /// The candidate.
        path: PathBuf,
        /// What the walk found instead.
        detail: String,
        /// The version its name states, when it states one.
        version: Option<PhpVersion>,
    },
    /// The object is a ZTS (thread-safe) `libphp`.
    ThreadSafe {
        /// The candidate.
        path: PathBuf,
        /// The version its name states, when it states one.
        version: Option<PhpVersion>,
    },
    /// The object exports neither Zend global, so it is not a `libphp`.
    NotLibphp {
        /// The candidate.
        path: PathBuf,
        /// The version its name states, when it states one.
        version: Option<PhpVersion>,
    },
    /// The object's `.dynsym` has no `php_version` **name**, so the worker
    /// cannot resolve the symbol it reads its minor from at load time
    /// (`MB.3.59`). An object that exports the name but whose body this walk
    /// cannot read is [`Self::NoVersion`], not this (`MB.3.61`).
    NoPhpVersion {
        /// The candidate.
        path: PathBuf,
        /// The version the path itself states, when it states one — named in
        /// the sentence so an operator sees that the file name does not help.
        named: Option<PhpVersion>,
    },
    /// The host's `libphp` is a PHP minor this build ships no shim for.
    ///
    /// The supported set is stated once, in [`SUPPORTED_PHP`], and the refusal
    /// is that sentence: a user does not have to learn `.dynsym` inspection to
    /// hear that their PHP is too old (or too new).
    UnsupportedVersion {
        /// The minor the object or its path states.
        version: PhpVersion,
    },
    /// The path states two different minors, or the path and the object's own
    /// bytes disagree, so no single shim can be chosen.
    VersionConflict {
        /// The candidate.
        path: PathBuf,
        /// Every minor the path states, with the spelling that stated it.
        stated: Vec<(PhpVersion, String)>,
        /// What the object's compile-time version string says, when readable.
        measured: Option<PhpVersion>,
    },
    /// `php_version` is exported in the `.dynsym`, but the version it returns
    /// could not be read from the object — no readable `return PHP_VERSION;`
    /// body, an `STT_NOTYPE` symbol, or `st_size == 0` — and the path states no
    /// minor either (`MB.3.61`).
    NoVersion {
        /// The candidate.
        path: PathBuf,
    },
    /// The host version is known but no shim for its tuple was found.
    NoShim {
        /// The NTS `libphp` that was found.
        libphp: PathBuf,
        /// Its version.
        version: PhpVersion,
        /// The file name that was looked for.
        shim: String,
        /// The directories that were searched.
        searched: Vec<PathBuf>,
    },
}

/// `(PHP 8.4, x86_64-gnu)` and the shim that tuple needs.
///
/// The two halves `ADR-0010` §4.1 requires every version-specific refusal to
/// name, built in one place so they cannot disagree.
fn abi_tuple(version: Option<PhpVersion>) -> Option<(String, String)> {
    let version = version?;
    Some((version.dotted(), shim_name(version)))
}

/// The sentence a `[route.php]` boot fails with (`ADR-0010` §4.1).
///
/// Both shapes name the route and its `entry`, the distro package that provides
/// the library, and `BLASTIT_LIBPHP`. The version-specific shape also names the
/// `(PHP X.Y, <arch>-<libc>)` tuple and the shim that tuple needs, because
/// "wrong minor" and "no shim" are the two failures an operator can fix by
/// installing a package, and neither is diagnosable without the tuple.
fn php_refusal_message(route: &str, entry: &str, refusal: &PhpRefusal) -> String {
    // Every shape names the route **and** its `entry`: a manifest with two PHP
    // routes must let the operator tell which front controller the host cannot
    // serve, and the entry is the only thing that distinguishes them.
    let prefix = format!("route {route:?}: [route.php] entry {entry:?}: ");
    match refusal {
        PhpRefusal::Missing { searched } => format!(
            "{prefix}no usable NTS libphp was found (searched BLASTIT_LIBPHP, then {} in {}); \
             {PHP_PACKAGES}, or {PHP_OVERRIDE}",
            LIBPHP_CANDIDATES.join(", "),
            display_paths(searched),
        ),
        PhpRefusal::NotAbsolute { path } => format!(
            "{prefix}BLASTIT_LIBPHP ({}) is not an absolute path, so the library the \
             boot inspected and the one a worker loads could differ; {PHP_PACKAGES}, or \
             {PHP_OVERRIDE}",
            path.display()
        ),
        PhpRefusal::NotAFile { path, detail } => format!(
            "{prefix}BLASTIT_LIBPHP ({}) cannot be opened or is not a regular file \
             ({detail}); {PHP_PACKAGES}, or {PHP_OVERRIDE}",
            path.display()
        ),
        PhpRefusal::NotElf {
            path,
            detail,
            version,
        } => present_refusal(
            &prefix,
            &format!("{} is not an ELF shared object ({detail})", path.display()),
            *version,
        ),
        PhpRefusal::ThreadSafe { path, version } => present_refusal(
            &prefix,
            &format!(
                "{} is a ZTS (thread-safe) libphp, and the embedded worker needs the NTS \
                 build of the same version",
                path.display()
            ),
            *version,
        ),
        PhpRefusal::NotLibphp { path, version } => present_refusal(
            &prefix,
            &format!(
                "{} exports neither executor_globals nor tsrm_startup, so it is not a PHP \
                 embed library",
                path.display()
            ),
            *version,
        ),
        PhpRefusal::NoPhpVersion { path, named } => {
            let name = named.map_or_else(String::new, |version| {
                format!(
                    " (its file name states PHP {}, but a file name cannot replace the symbol)",
                    version.dotted()
                )
            });
            format!(
                "{prefix}{} is an NTS libphp whose `.dynsym` exports no `php_version` — the \
                 symbol the embedded worker resolves before `php_module_startup` — so the worker \
                 cannot determine its minor at load time and no shim can be chosen for it{name}; \
                 {PHP_PACKAGES}, or {PHP_OVERRIDE}",
                path.display()
            )
        }
        PhpRefusal::UnsupportedVersion { version } => format!(
            "{prefix}PHP {} is not supported — Blastit supports {SUPPORTED_PHP}",
            version.dotted()
        ),
        PhpRefusal::VersionConflict {
            path,
            stated,
            measured,
        } => {
            let mut candidates: Vec<String> = stated
                .iter()
                .map(|(version, spelling)| format!("{spelling} states PHP {}", version.dotted()))
                .collect();
            if let Some(version) = measured {
                candidates.push(format!(
                    "the `php_version` string the object carries states PHP {}",
                    version.dotted()
                ));
            }
            format!(
                "{prefix}{} states more than one PHP minor — {} — so no single shim can be \
                 chosen for it; give it one versioned name (libphp8.4.so, libphp-8.4.so, …), \
                 keep the distro's directory and file name consistent, or {PHP_OVERRIDE}; \
                 {PHP_PACKAGES}",
                path.display(),
                candidates.join(", ")
            )
        }
        PhpRefusal::NoVersion { path } => format!(
            "{prefix}{} is an NTS libphp that exports `php_version` in its `.dynsym`, but the \
             version that symbol returns could not be read from its sections (no readable \
             `return PHP_VERSION;` body — a symbol typed `STT_NOTYPE` or sized `0` is one), and \
             neither its file name nor its directory states a PHP major.minor, so no shim can \
             be chosen for it; give it a versioned name (libphp8.4.so, libphp-8.4.so, …), or \
             {PHP_OVERRIDE}; {PHP_PACKAGES}",
            path.display()
        ),
        PhpRefusal::NoShim {
            libphp,
            version,
            shim,
            searched,
        } => format!(
            "{prefix}no shim {shim} was found for the NTS libphp {} (looked in {}); \
             this host is (PHP {}, {}), and {shim} is what that tuple needs; {PHP_PACKAGES}, \
             or {PHP_OVERRIDE}",
            libphp.display(),
            display_paths(searched),
            version.dotted(),
            target_tuple(),
        ),
    }
}

/// The version-specific half of [`php_refusal_message`]: the problem, then the
/// tuple, the shim, the package and the override.
fn present_refusal(prefix: &str, problem: &str, version: Option<PhpVersion>) -> String {
    match abi_tuple(version) {
        Some((dotted, shim)) => format!(
            "{prefix}{problem}; this host is (PHP {dotted}, {}), whose embedded shim \
             is {shim}; {PHP_PACKAGES}, or {PHP_OVERRIDE}",
            target_tuple(),
        ),
        None => format!(
            "{prefix}{problem}; this host is (PHP ?, {}), so no shim can be chosen \
             until the object's version is known; {PHP_PACKAGES}, or {PHP_OVERRIDE}",
            target_tuple(),
        ),
    }
}

/// The directories as one comma-separated list, for a refusal.
fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<String>>()
        .join(", ")
}

/// What a boot's `[route.php]` gate says before a listener may bind.
#[derive(Debug)]
pub(crate) enum PhpBoot {
    /// No route declares `[route.php]`: nothing to discover.
    Absent,
    /// The host can embed PHP: this is the NTS `libphp` and the shim every PHP
    /// route's worker will load, recorded as `(path, st_dev, st_ino)` in
    /// [`Config::php`] so stage 1 re-opens exactly these files (`ADR-0010` §3).
    Proceed(PhpRuntime),
    /// The host cannot embed PHP: the sentence names the route, the package,
    /// `BLASTIT_LIBPHP` and — when it is known — the `(PHP X.Y, <arch>-<libc>)`
    /// tuple.
    Refused(String),
}

impl PhpBoot {
    /// The sentence this gate refuses with, or `None` when it proceeds.
    #[cfg(test)]
    fn refusal(&self) -> Option<&str> {
        match self {
            Self::Absent | Self::Proceed(_) => None,
            Self::Refused(message) => Some(message),
        }
    }
}

/// `ADR-0010` §3/§4.1: the discovery gate a `[route.php]` manifest is resolved
/// by, before any listener binds.
///
/// Runs in the stage-0 image and never loads anything: the `libphp` is opened,
/// `fstat`ed and read as an ELF `.dynsym` table — `dlopen` happens inside a
/// view worker, never here.
///
/// The order is the order an operator can act on: an over-budget `workers` first
/// (nothing about the host can fix it), then the library (missing, a directory,
/// not an ELF, ZTS, no `php_version`, no version this boot can read) and the
/// shim for the tuple. Every one of those is fatal in **both** scopes:
/// `ADR-0010` §1 gives PHP one backend, so a host that cannot serve a declared
/// `[route.php]` route gets §4.1's package sentence — in `blastit dev` and
/// `blastit check` as much as in a deployment. There is no development mode
/// that serves a PHP route another way.
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
    // `blastit.toml`'s tree.
    let deployment = (scope == PhpShimScope::Deployment).then(|| Deployment {
        init_uid: current_uid(),
        manifest_dir: config.manifest_dir.clone(),
    });
    php_boot_in(
        manifest,
        &PhpSearch::from_config(config, scope),
        Some(config.view_worker_cap()),
        deployment.as_ref(),
    )
}

/// What a **deployment** boot checks that `dev` and `check` do not: the uid that
/// must own every path, and the directory the manifest was read from
/// (`ADR-0010` §3, design `R7`/`B2-R5`).
struct Deployment {
    /// The uid init runs as: root, or init's own in `rootless` mode.
    init_uid: u32,
    /// The directory `blastit.toml` came from — every PHP worker's `cwd` and
    /// `include_path` (`ADR-0010` §2).
    manifest_dir: PathBuf,
}

/// The same gate for a `dev`/`check` caller: no production provenance.
///
/// The unit tests use it as the seam they have always used; a boot reaches the
/// gate through [`php_boot_gate`], which decides the scope.
#[cfg(test)]
fn php_boot(manifest: &Manifest, search: &PhpSearch, cap: Option<u32>) -> PhpBoot {
    php_boot_in(manifest, search, cap, None)
}

/// The same gate for a caller with the search, the budget, and — for a
/// deployment — the provenance to authorise spelled out.
fn php_boot_in(
    manifest: &Manifest,
    search: &PhpSearch,
    cap: Option<u32>,
    deployment: Option<&Deployment>,
) -> PhpBoot {
    let Some((route, entry, _workers)) = first_php_route(manifest) else {
        return PhpBoot::Absent;
    };
    if let Some(cap) = cap
        && let Some(message) = php_worker_budget(manifest, cap)
    {
        return PhpBoot::Refused(message);
    }
    match discover_php(search) {
        Ok(runtime) => {
            // The authorisation comes after discovery — it needs the *resolved*
            // paths — and before the record is handed to stage 1, so a
            // deployment whose pair or tree a route uid could replace never
            // reaches `bind_view_sockets` (`R7`, `B2-R5`).
            if let Some(deployment) = deployment
                && let Some(message) =
                    deployment_refusal(manifest, deployment, &runtime, route, entry)
            {
                return PhpBoot::Refused(message);
            }
            PhpBoot::Proceed(runtime)
        }
        Err(refusal) => PhpBoot::Refused(php_refusal_message(route, entry, &refusal)),
    }
}

/// `R7`/`B2-R5`: the **deployment** half of the `[route.php]` gate, run over one
/// manifest directory and one pair — the seam the protected suite needs.
///
/// It runs the same function a deployment boot runs, `php_boot_in` with a
/// `Deployment`, because an unprivileged test process cannot reach a deployment
/// boot at all: `caps::detect` refuses it before `Config::from_env`
/// (`LESSONS.md`, `MB.1.6`). Driving [`crate::host::Provenance`] directly — which the
/// protected legs also do — pins the *decision* but not the *call*: with this
/// seam deleted from `php_boot_in`, `tests/critical_php_embed.rs`'s
/// `the_deployment_gate_is_invoked_behind_its_sentence` is the leg that fails,
/// and without it only this module's unit tests would.
///
/// `Ok(())` is "the gate proceeds" — `Proceed` from the discovery *and* a
/// manifest with no `[route.php]` route, which a deployment boots like any
/// other (the gate has nothing to authorise). `Err(message)` is a **refusal**:
/// the sentence the boot prints, the same one `php_boot`'s callers print.
///
/// Not part of the published API: the only caller is the protected suite, which
/// reaches it through `blastit::host::deployment_boot_check`.
pub fn deployment_boot_check(
    manifest_source: &str,
    manifest_dir: &Path,
    libphp: &Path,
    shim_dir: &Path,
    uid_base: u32,
) -> Result<(), String> {
    let (manifest, _warnings) = Manifest::parse_and_validate(manifest_source, uid_base)
        .map_err(|errors| errors.to_string())?;
    let search = PhpSearch {
        libphp: Some(libphp.to_path_buf()),
        shim_dirs: vec![shim_dir.to_path_buf()],
        lib_dirs: Vec::new(),
    };
    let deployment = Deployment {
        init_uid: current_uid(),
        manifest_dir: manifest_dir.to_path_buf(),
    };
    match php_boot_in(&manifest, &search, None, Some(&deployment)) {
        // Nothing to authorise is not a refusal: a manifest with no PHP route
        // boots (`MB.3.44` — reporting this as an error let a caller read a
        // *proceeding* manifest as a refused one, the confusion this seam
        // exists to prevent).
        PhpBoot::Proceed(_) | PhpBoot::Absent => Ok(()),
        PhpBoot::Refused(message) => Err(message),
    }
}

/// `R7`/`B2-R5`: the sentence a deployment boot refuses this pair and tree with.
///
/// The **pair** is checked first and keeps `ADR-0010` §4.1's shape — the route,
/// its `entry`, the offending path, the fix, the `(PHP X.Y, <arch>-<libc>)`
/// tuple, the distro package and `BLASTIT_LIBPHP` — because every way an
/// operator can fix a library is one of those names. The **tree** is a
/// different sentence: nothing about it is fixed by installing a package, so it
/// names what the path is (the directory holding `blastit.toml`, a
/// `[route.php] entry`, a `[route.static] root`), the directory itself, and the
/// `chown`/`chmod` that fixes it.
fn deployment_refusal(
    manifest: &Manifest,
    deployment: &Deployment,
    runtime: &PhpRuntime,
    route: &str,
    entry: &str,
) -> Option<String> {
    let objects = Provenance {
        init_uid: deployment.init_uid,
        objects: vec![
            Claim::new("the NTS libphp", runtime.libphp.clone()),
            Claim::new("the embedded shim", runtime.shim.clone()),
        ],
        trees: Vec::new(),
    };
    if let Err(detail) = objects.check_objects() {
        let prefix = format!("route {route:?}: [route.php] entry {entry:?}: ");
        return Some(present_refusal(
            &prefix,
            detail.as_str(),
            Some(runtime.version),
        ));
    }
    let trees = php_trees(manifest, deployment);
    // One claim at a time, in this order: the first refusal wins, exactly as
    // `check_trees` reports it, but a claim whose path is not there gets the
    // missing-path sentence (see [`missing_claim`]).
    for claim in &trees {
        if let Some(message) = missing_claim(claim) {
            return Some(message);
        }
        let checked = Provenance {
            init_uid: deployment.init_uid,
            trees: vec![claim.clone()],
            objects: Vec::new(),
        };
        if let Err(detail) = checked.check_trees() {
            return Some(detail);
        }
    }
    None
}

/// `R13`: the sentence a manifest-named path that is **not there** refuses with.
///
/// Asked before [`host::Provenance::check_trees`] for the same claim because
/// that check's sentence for a missing path — "it cannot be inspected (No such
/// file or directory)" — reads as a permission problem and sends the operator to
/// `chmod` a file that does not exist. A path the manifest names is *missing* or
/// it is *unprotected*; this is the missing half, and a path that is there but
/// unreadable still goes to the provenance check, which is the one that can say
/// what is wrong with it.
///
/// The test is [`std::fs::metadata`], the same call `host::facts_of` makes, so
/// "missing" here means exactly the `NotFound` that call reports — which
/// **includes a dangling symlink** (`metadata` follows the link, and the target
/// is what the boot would read). That is reported as missing on purpose: it is
/// the sentence an operator can act on, while "cannot be inspected" would name
/// the link rather than the file that is absent. Any other errno — `EACCES`, a
/// symlink loop — is *not* missing and stays with the provenance sentence,
/// which is the one that can say what is wrong with the path.
fn missing_claim(claim: &Claim) -> Option<String> {
    let missing = matches!(
        std::fs::metadata(&claim.path),
        Err(ref err) if err.kind() == std::io::ErrorKind::NotFound
    );
    missing.then(|| {
        format!(
            "{} ({}) is missing (no such file or directory): the manifest names a path this boot \
             cannot read, so a deployment would hand its workers nothing; create it, or correct \
             the path the manifest names",
            claim.what,
            claim.path.display()
        )
    })
}

/// The tree claims a deployment boot authorises: the directory the workers'
/// `cwd`/`include_path` is, every `[route.php] entry`, and every
/// `[route.static] root` (`ADR-0010` §3, design `B2-R5`).
///
/// A worker can `include` any file those paths reach, so a route uid that can
/// write one of them chooses PHP code the worker will run — which is why the
/// rule is the same one [`host::plain_is_protected`] applies to the binary a
/// child executes, minus the file-capability half (a tree is read, not loaded).
fn php_trees(manifest: &Manifest, deployment: &Deployment) -> Vec<Claim> {
    let mut claims = vec![Claim::new(
        "the directory holding blastit.toml (the PHP worker's cwd and include_path)",
        deployment.manifest_dir.clone(),
    )];
    for route in manifest.routes() {
        if let Some(php) = route.php() {
            claims.push(Claim::new(
                format!(
                    "route {:?}: [route.php] entry {:?}",
                    route.name(),
                    php.entry
                ),
                deployment.manifest_dir.join(&php.entry),
            ));
        }
        if let Some(policy) = route.static_policy() {
            claims.push(Claim::new(
                format!(
                    "route {:?}: [route.static] root {:?}",
                    route.name(),
                    policy.root
                ),
                deployment.manifest_dir.join(&policy.root),
            ));
        }
    }
    claims
}

/// `ADR-0010` §2: `workers` is a cap on **that route's** concurrency and it is
/// downward only, so every `[route.php]` route is checked, in manifest order,
/// and the first one above the deployment's per-view budget is refused naming
/// its own name and both numbers.
///
/// Checking only the route the discovery sentence would name leaves a manifest
/// whose *second* PHP route asks for more than the deployment allows booting
/// with an accepted budget; the cap is a manifest rule, so it is applied to the
/// whole manifest before any host question is asked.
fn php_worker_budget(manifest: &Manifest, cap: u32) -> Option<String> {
    manifest.routes().iter().find_map(|route| {
        let workers = route.php()?.workers?;
        (workers > cap).then(|| {
            format!(
                "route {:?}: [route.php] workers = {workers} is above this deployment's \
                     per-view budget of {cap} (BLASTIT_VIEW_MAX_WORKERS, from \
                     BLASTIT_MAX_WORKERS); lower workers to {cap} or raise the budget",
                route.name()
            )
        })
    })
}

/// The first `[route.php]` route: its name, its `entry` and its `workers` cap.
///
/// One route names the refusal, and it is the first one, so the sentence is
/// stable across a boot with several PHP routes.
fn first_php_route(manifest: &Manifest) -> Option<(&str, &str, Option<u32>)> {
    manifest.routes().iter().find_map(|route| {
        route
            .php()
            .map(|php| (route.name(), php.entry.as_str(), php.workers))
    })
}

/// Resolves the NTS `libphp` and its shim, or names why not (`ADR-0010` §3).
///
/// `BLASTIT_LIBPHP` is the only candidate when set — a boot never falls back to
/// a distro copy the operator did not name. Otherwise [`LIBPHP_PROBES`] is
/// walked version-major — every 8.5 spelling, then every 8.4 one, then 8.3 —
/// each in every directory of [`PhpSearch::lib_dirs`], so the *library's*
/// version wins over both its name and its directory (`MB.3.61`).
///
/// # Both records are the *resolved* path
///
/// Stage 0 may follow symlinks (`/usr/lib/libphp8.so -> libphp8.4.so` is a
/// normal distro layout), but stage 1 re-opens each object
/// `O_NOFOLLOW`-strictly so a link cannot be swapped in between the two
/// stages. Recording the path as given therefore makes a legitimate symlink
/// die in stage 1 with `ELOOP` — after the banner, with no §4.1 sentence. So
/// the record holds `canonicalize`'s answer for both objects: the two stages
/// open the same real file, a link in the distro keeps working, and a link
/// planted after stage 0 is still refused (`ENOENT`/`ELOOP` on the real name
/// is the inode record's job, not the link's).
fn discover_php(search: &PhpSearch) -> Result<PhpRuntime, PhpRefusal> {
    let path = match &search.libphp {
        Some(path) => {
            if !path.is_absolute() {
                return Err(PhpRefusal::NotAbsolute { path: path.clone() });
            }
            path.clone()
        }
        None => match first_existing(search) {
            Some(path) => path,
            None => {
                // No supported spelling was found. An installed *unsupported*
                // minor is a different failure from "no libphp at all", and the
                // fix is a different package — so say which one it is rather
                // than sending the operator to look for a library they have.
                if let Some(version) = installed_unsupported(search) {
                    return Err(PhpRefusal::UnsupportedVersion { version });
                }
                return Err(PhpRefusal::Missing {
                    searched: search.lib_dirs.clone(),
                });
            }
        },
    };
    let mut file = std::fs::File::open(&path).map_err(|err| PhpRefusal::NotAFile {
        path: path.clone(),
        detail: err.to_string(),
    })?;
    let metadata = file.metadata().map_err(|err| PhpRefusal::NotAFile {
        path: path.clone(),
        detail: err.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(PhpRefusal::NotAFile {
            path: path.clone(),
            detail: "not a regular file".to_string(),
        });
    }
    // The path stage 1 will re-open. A name that cannot be resolved is a §4.1
    // refusal here, never an `ELOOP` after the banner.
    let path = resolve(&path)?;
    // Three static statements, in the order `MB.3.59` fixes them: the file
    // name (the fast path), the path's per-minor directory, then the object's
    // own bytes. None of them loads anything.
    let named = version_of(&path, &file);
    let stated = path_versions(&path);
    let facts = inspect_object(&mut file).map_err(|detail| PhpRefusal::NotElf {
        path: path.clone(),
        detail,
        version: named.or_else(|| stated.first().map(|stated| stated.version)),
    })?;
    let hinted = named
        .or_else(|| stated.first().map(|stated| stated.version))
        .or(facts.version);
    match facts.abi {
        ZendAbi::Nts => {}
        ZendAbi::Zts => {
            return Err(PhpRefusal::ThreadSafe {
                path: path.clone(),
                version: hinted,
            });
        }
        ZendAbi::NotLibphp => {
            return Err(PhpRefusal::NotLibphp {
                path: path.clone(),
                version: hinted,
            });
        }
    }
    // A minor this build has no shim for is refused by name before the
    // `.dynsym` story. The object's own `php_version` body is the most
    // authoritative answer; the path's file name or per-minor directory is the
    // fallback for an object whose body the walk cannot read.
    let stated_version = facts
        .version
        .or(named)
        .or_else(|| stated.first().map(|stated| stated.version));
    if let Some(version) = stated_version
        && !version.supported()
    {
        return Err(PhpRefusal::UnsupportedVersion { version });
    }
    // The worker resolves `php_version` before it can start anything, so an
    // object whose `.dynsym` has no such **name** is refused here — whatever
    // its file name says (`MB.3.59`). Presence is the name alone: an odd object
    // that exports it as `STT_NOTYPE` or with no size is refused later, and for
    // the reason that is actually true (`MB.3.61`).
    if !facts.exports_php_version {
        return Err(PhpRefusal::NoPhpVersion {
            path: path.clone(),
            named,
        });
    }
    let version = choose_version(&path, &stated, facts.version)?;
    // One spelling, the `ADR-0010` §4.1 one: `make php-sapi` writes the same
    // `libblastit_php<minor>-<arch>-<libc>.so` that `Minor::shim_file()` names,
    // so a developer who has just built a shim is found here with no
    // `BLASTIT_PHP_SHIM_DIR` (`MB.3.35`). This lookup never accepts two
    // spellings for the same tuple.
    let shim = shim_name(version);
    let shim_path = search
        .shim_dirs
        .iter()
        .map(|dir| dir.join(&shim))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| PhpRefusal::NoShim {
            libphp: path.clone(),
            version,
            shim,
            searched: search.shim_dirs.clone(),
        })?;
    let shim_metadata = std::fs::metadata(&shim_path).map_err(|err| PhpRefusal::NotAFile {
        path: shim_path.clone(),
        detail: err.to_string(),
    })?;
    // The shim's own identity, recorded beside the library's: stage 1 re-opens
    // both and compares each against the record (`ADR-0010` §3), because a shim
    // is code that runs inside every PHP worker and a path is not an identity.
    let shim_path = resolve(&shim_path)?;
    Ok(PhpRuntime {
        libphp: path,
        dev: metadata.dev(),
        ino: metadata.ino(),
        version,
        shim: shim_path,
        shim_dev: shim_metadata.dev(),
        shim_ino: shim_metadata.ino(),
    })
}

/// The path a record stores: `canonicalize`'s answer, or a §4.1 refusal.
///
/// [`PhpRefusal::NotAFile`] carries the sentence an operator can act on — the
/// route, its `entry`, the path, the reason, the distro package and
/// `BLASTIT_LIBPHP` — which is the whole point: a link that cannot be resolved
/// must read as "this host cannot embed PHP", not as `errno` in a worker.
fn resolve(path: &Path) -> Result<PathBuf, PhpRefusal> {
    std::fs::canonicalize(path).map_err(|err| PhpRefusal::NotAFile {
        path: path.to_path_buf(),
        detail: format!("its path cannot be resolved ({err})"),
    })
}

/// The first probe of [`LIBPHP_PROBES`] that exists, in its own order.
///
/// Version-major, then spelling (`MB.3.61`): the *minor* is preferred, and the
/// name only breaks a tie within it, so a host carrying two embed packages
/// serves from the newer one instead of preferring whichever name came first.
fn first_existing(search: &PhpSearch) -> Option<PathBuf> {
    LIBPHP_PROBES.iter().find_map(|probe| {
        search
            .lib_dirs
            .iter()
            .filter(|dir| match probe.per_minor_dir {
                None => true,
                Some(minor) => dir.file_name() == Some(OsStr::new(minor)),
            })
            .map(|dir| dir.join(probe.name))
            .find(|path| path.is_file())
    })
}

/// The newest unsupported PHP minor installed in [`PhpSearch::lib_dirs`], when
/// no supported one was found.
///
/// This is a **refusal** scan, not a discovery one: it runs only after
/// [`first_existing`] found nothing, so it cannot change which library a host
/// with a supported package serves. Its one job is to turn "no usable NTS
/// `libphp` was found" into the true sentence for a host whose PHP is simply not
/// a minor this build ships — the version comes from the file name
/// (`libphp8.2.so`, `libphp82.so`) or, for an unversioned `libphp.so`, from the
/// per-minor directory it sits in (`/usr/lib/php82`).
fn installed_unsupported(search: &PhpSearch) -> Option<PhpVersion> {
    let mut found: Vec<PhpVersion> = Vec::new();
    for dir in &search.lib_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let directory_minor = dir
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(PhpVersion::parse_directory);
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let version = entry
                .file_name()
                .to_str()
                .and_then(PhpVersion::parse)
                .or(directory_minor);
            if let Some(version) = version
                && !version.supported()
            {
                found.push(version);
            }
        }
    }
    found.into_iter().max()
}

/// The version the file's name states, or the name its descriptor resolves to.
///
/// `libphp.so` is usually a symlink to `libphp8.4.so`, and a descriptor's link
/// target says so even when the path does not — a `readlink`, never a load.
/// Both spellings `PhpVersion::parse` knows are accepted, so Alpine's packed
/// `libphp83.so` is a name that states a version (`MB.3.59`).
fn version_of(path: &Path, file: &std::fs::File) -> Option<PhpVersion> {
    let named = path
        .file_name()
        .and_then(OsStr::to_str)
        .and_then(PhpVersion::parse);
    named.or_else(|| {
        let link = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()?;
        link.file_name()
            .and_then(OsStr::to_str)
            .and_then(PhpVersion::parse)
    })
}

/// One minor the **path** states, and the spelling that stated it — for the
/// conflict sentence, so an operator sees which half to rename.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StatedVersion {
    /// The minor.
    version: PhpVersion,
    /// The file name or directory component, as written.
    spelling: String,
}

/// Every PHP minor the path itself states (`MB.3.59`), deduplicated.
///
/// Two spellings are the distro's: the file name (`libphp83.so`,
/// `libphp8.4.so`) and a per-minor parent directory (`/usr/lib/php84`,
/// `/usr/local/lib/php83`). A path with the same minor twice keeps one entry;
/// a path with two different minors still lists both, so [`choose_version`]
/// can refuse rather than pick one.
fn path_versions(path: &Path) -> Vec<StatedVersion> {
    let mut stated: Vec<StatedVersion> = Vec::new();
    let mut push = |version: PhpVersion, spelling: String| {
        if !stated.iter().any(|known| known.version == version) {
            stated.push(StatedVersion { version, spelling });
        }
    };
    if let Some(name) = path.file_name().and_then(OsStr::to_str)
        && let Some(version) = PhpVersion::parse(name)
    {
        push(version, format!("`{name}`"));
    }
    // The immediate parent only: `…/php84/libphp.so` is Alpine's own shape,
    // while a `php84` further up a developer's tree is not a statement about
    // this object.
    if let Some(component) = path
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        && let Some(version) = PhpVersion::parse_directory(component)
    {
        push(version, format!("the directory `{component}`"));
    }
    stated
}

/// The one minor the path and the object's own bytes agree on, or the refusal
/// that names every candidate (`MB.3.59`).
///
/// Two different statements are never resolved by a preference: the shim is
/// chosen by this number, and a wrong one is a boot that loads a foreign ABI.
/// So disagreement is a refusal naming each spelling, the `.dynsym` fact and
/// `BLASTIT_LIBPHP`; stating nothing at all is the same sentence with no
/// candidates.
fn choose_version(
    path: &Path,
    stated: &[StatedVersion],
    measured: Option<PhpVersion>,
) -> Result<PhpVersion, PhpRefusal> {
    let mut distinct: Vec<PhpVersion> = stated.iter().map(|stated| stated.version).collect();
    if let Some(measured) = measured
        && !distinct.contains(&measured)
    {
        distinct.push(measured);
    }
    if distinct.len() > 1 {
        return Err(PhpRefusal::VersionConflict {
            path: path.to_path_buf(),
            stated: stated
                .iter()
                .map(|stated| (stated.version, stated.spelling.clone()))
                .collect(),
            measured,
        });
    }
    distinct
        .first()
        .copied()
        .ok_or_else(|| PhpRefusal::NoVersion {
            path: path.to_path_buf(),
        })
}

/// The largest section this walk will read into memory: a `libphp`'s `.dynsym`
/// is tens of kilobytes, and this is the bound that keeps a hostile file from
/// being an allocation.
const MAX_SECTION_BYTES: u64 = 64 * 1024 * 1024;

/// The ELF class a walk is reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ElfClass {
    /// 32-bit (`e_ident[EI_CLASS] == 1`).
    Elf32,
    /// 64-bit (`e_ident[EI_CLASS] == 2`).
    Elf64,
}

/// The header fields the `.dynsym` walk needs.
struct ElfHeader {
    class: ElfClass,
    /// `e_machine`: which decoder a `php_version` body needs.
    machine: u16,
    /// Offset of the section header table.
    shoff: u64,
    /// Size of one section header.
    shentsize: usize,
    /// Number of section headers.
    shnum: usize,
    /// The file's size, for the bounds checks.
    size: u64,
}

/// `SHT_DYNSYM`: the dynamic symbol table.
const SHT_DYNSYM: u32 = 11;
/// `SHT_STRTAB`: a string table.
const SHT_STRTAB: u32 = 3;
/// `EM_X86_64`: the machine whose `lea`-based `php_version` body is read.
const EM_X86_64: u16 = 62;
/// `EM_AARCH64`: the machine whose `adrp`/`add` body is read.
const EM_AARCH64: u16 = 183;

/// What the bounded, load-free walk says about one object (`ADR-0010` §3,
/// `MB.3.59`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObjectFacts {
    /// Which Zend build the `.dynsym` names.
    abi: ZendAbi,
    /// Whether `php_version` is exported — the symbol the worker resolves before
    /// `php_module_startup`.
    ///
    /// Presence comes from the **name** in the `.dynsym` and nothing else: an
    /// object that exports it as `STT_NOTYPE`, or with `st_size == 0`, does
    /// export it, and refusing it as "exports no `php_version`" was a false
    /// sentence (`MB.3.61`). What the body says is [`ObjectFacts::version`].
    exports_php_version: bool,
    /// The compile-time version `php_version()` returns, read from the string
    /// its body points at; `None` when the body is not the shape this walk
    /// understands.
    version: Option<PhpVersion>,
}

/// Reads the `.dynsym` of `file` and — when the path stated no minor — the
/// compile-time version the object carries (`ADR-0010` §3, `MB.3.59`).
///
/// No dependency, no `dlopen`, no `mmap`: `seek`/`read` of the section header
/// table, the dynamic symbol table and its string table, the `php_version`
/// body (bounded), and the string it returns (bounded), with every offset
/// bounds-checked. A file that is not a little-endian ELF, has no `.dynsym`, or
/// is truncated is an `Err` naming what was wrong.
fn inspect_object(file: &mut std::fs::File) -> Result<ObjectFacts, String> {
    let header = read_elf_header(file)?;
    let table_len = header
        .shentsize
        .checked_mul(header.shnum)
        .ok_or_else(|| "the section header table is too large".to_string())?;
    let table_end = header
        .shoff
        .checked_add(
            u64::try_from(table_len)
                .map_err(|_| "the section header table is too large".to_string())?,
        )
        .ok_or_else(|| "the section header table is too large".to_string())?;
    if table_end > header.size {
        return Err("the section header table is past the end of the file".to_string());
    }
    let table = read_at(file, header.shoff, table_len)?;
    let (dynsym, strtab_index) = find_dynsym(&header, &table)?;
    let strtab = find_strtab(&header, &table, strtab_index)?;
    let symbols = read_at(file, dynsym.offset, dynsym.len)?;
    let names = read_at(file, strtab.offset, strtab.len)?;
    let symbols = scan_symbols(&header.class, &symbols, dynsym.entsize, &names)?;
    let abi = match (symbols.executor_globals, symbols.tsrm_startup) {
        (true, false) => ZendAbi::Nts,
        (false, false) => ZendAbi::NotLibphp,
        (_, true) => ZendAbi::Zts,
    };
    let version = symbols
        .php_version_body
        .and_then(|symbol| php_version_string(file, &header, &table, symbol));
    Ok(ObjectFacts {
        abi,
        exports_php_version: symbols.exports_php_version,
        version,
    })
}

/// One section's location, bounded by [`MAX_SECTION_BYTES`].
#[derive(Clone, Copy, Debug)]
struct Section {
    offset: u64,
    len: usize,
    entsize: usize,
}

/// Parses and bounds-checks the ELF header.
fn read_elf_header(file: &mut std::fs::File) -> Result<ElfHeader, String> {
    let size = file
        .metadata()
        .map_err(|err| format!("the file could not be inspected ({err})"))?
        .len();
    if size < 16 {
        return Err("it is shorter than an ELF identification".to_string());
    }
    let ident = read_at(file, 0, 16)?;
    if ident.get(..4) != Some(b"\x7fELF") {
        return Err("no ELF magic".to_string());
    }
    let class = match ident.get(4) {
        Some(1) => ElfClass::Elf32,
        Some(2) => ElfClass::Elf64,
        _ => return Err("not an ELF file: unknown class".to_string()),
    };
    if ident.get(5) != Some(&1) {
        return Err("a big-endian ELF object: only little-endian objects are embedded".to_string());
    }
    let (head_len, shoff_off, shentsize_off, shnum_off, min_shentsize) = match class {
        ElfClass::Elf64 => (64usize, 0x28, 0x3a, 0x3c, 64usize),
        ElfClass::Elf32 => (52usize, 0x20, 0x2e, 0x30, 40usize),
    };
    if size < u64::try_from(head_len).map_err(|_| "the header is too large".to_string())? {
        return Err("the header is truncated".to_string());
    }
    let head = read_at(file, 0, head_len)?;
    let missing = || "the header is truncated".to_string();
    let machine = le_u16(&head, 0x12).ok_or_else(missing)?;
    let shoff = match class {
        ElfClass::Elf64 => le_u64(&head, shoff_off),
        ElfClass::Elf32 => le_u32(&head, shoff_off).map(u64::from),
    }
    .ok_or_else(missing)?;
    let shentsize = le_u16(&head, shentsize_off).ok_or_else(missing)?;
    let shnum = le_u16(&head, shnum_off).ok_or_else(missing)?;
    let shentsize = usize::from(shentsize);
    let shnum = usize::from(shnum);
    if shnum == 0 {
        return Err("it has no section headers".to_string());
    }
    if shentsize < min_shentsize {
        return Err("a section header table whose entries are too small".to_string());
    }
    Ok(ElfHeader {
        class,
        machine,
        shoff,
        shentsize,
        shnum,
        size,
    })
}

/// The `.dynsym` section and the index of its string table.
fn find_dynsym(header: &ElfHeader, table: &[u8]) -> Result<(Section, usize), String> {
    for index in 0..header.shnum {
        let at = index.saturating_mul(header.shentsize);
        let Some(kind) = le_u32(table, at.saturating_add(4)) else {
            continue;
        };
        if kind != SHT_DYNSYM {
            continue;
        }
        let Some((offset, size, link, entsize)) = section_fields(header.class, table, at) else {
            return Err("the .dynsym section header is truncated".to_string());
        };
        let len = bounded_section(offset, size, header.size)?;
        let entsize = usize::try_from(entsize)
            .map_err(|_| "the .dynsym entry size does not fit this build".to_string())?;
        return Ok((
            Section {
                offset,
                len,
                entsize,
            },
            usize::try_from(link).map_err(|_| {
                "the .dynsym string-table index does not fit this build".to_string()
            })?,
        ));
    }
    Err("it has no .dynsym section".to_string())
}

/// The string table `index` names, checked to be one.
fn find_strtab(header: &ElfHeader, table: &[u8], index: usize) -> Result<Section, String> {
    if index >= header.shnum {
        return Err("the .dynsym string-table index is past the section table".to_string());
    }
    let at = index.saturating_mul(header.shentsize);
    let kind = le_u32(table, at.saturating_add(4))
        .ok_or_else(|| "the .dynstr section header is truncated".to_string())?;
    if kind != SHT_STRTAB {
        return Err("the .dynsym string table is not a string table".to_string());
    }
    let (offset, size, _, _) = section_fields(header.class, table, at)
        .ok_or_else(|| "the .dynstr section header is truncated".to_string())?;
    let len = bounded_section(offset, size, header.size)?;
    Ok(Section {
        offset,
        len,
        entsize: 0,
    })
}

/// `(sh_offset, sh_size, sh_link, sh_entsize)` of the section at `at`.
fn section_fields(class: ElfClass, table: &[u8], at: usize) -> Option<(u64, u64, u32, u64)> {
    match class {
        ElfClass::Elf64 => Some((
            le_u64(table, at.saturating_add(0x18))?,
            le_u64(table, at.saturating_add(0x20))?,
            le_u32(table, at.saturating_add(0x28))?,
            le_u64(table, at.saturating_add(0x38))?,
        )),
        ElfClass::Elf32 => Some((
            u64::from(le_u32(table, at.saturating_add(0x10))?),
            u64::from(le_u32(table, at.saturating_add(0x14))?),
            le_u32(table, at.saturating_add(0x18))?,
            u64::from(le_u32(table, at.saturating_add(0x24))?),
        )),
    }
}

/// `sh_addr`: the address a section is mapped at, which turns a symbol's
/// `st_value` into a file offset (`MB.3.59`).
fn section_addr(class: ElfClass, table: &[u8], at: usize) -> Option<u64> {
    match class {
        ElfClass::Elf64 => le_u64(table, at.saturating_add(0x10)),
        ElfClass::Elf32 => le_u32(table, at.saturating_add(0x0c)).map(u64::from),
    }
}

/// `SHF_ALLOC`: the section is mapped into the process image.
const SHF_ALLOC: u64 = 0x2;
/// `SHF_WRITE`: the section is writable — never where a compile-time string
/// or a `return <constant>;` body is read from.
const SHF_WRITE: u64 = 0x1;

/// `sh_flags` of the section at `at`.
fn section_flags(class: ElfClass, table: &[u8], at: usize) -> Option<u64> {
    match class {
        ElfClass::Elf64 => le_u64(table, at.saturating_add(0x8)),
        ElfClass::Elf32 => le_u32(table, at.saturating_add(0x8)).map(u64::from),
    }
}

/// A section's length in memory, refused when it is absurd or past the file.
fn bounded_section(offset: u64, size: u64, file_size: u64) -> Result<usize, String> {
    if size > MAX_SECTION_BYTES {
        return Err(format!(
            "a {size}-byte section is larger than this build will inspect"
        ));
    }
    let end = offset
        .checked_add(size)
        .ok_or_else(|| "a section runs past the end of the file".to_string())?;
    if end > file_size {
        return Err("a section runs past the end of the file".to_string());
    }
    usize::try_from(size).map_err(|_| "a section is larger than this build can address".to_string())
}

/// The dynamic symbols the walk cares about (`ADR-0010` §3, `MB.3.59`).
struct DynSymbols {
    /// `executor_globals` is exported.
    executor_globals: bool,
    /// `tsrm_startup` is exported (a ZTS build).
    tsrm_startup: bool,
    /// The `.dynsym` exports the **name** `php_version`: the symbol the worker's
    /// loader resolves before `php_module_startup`. Presence is the name alone,
    /// whatever the symbol is typed or sized (`MB.3.61`).
    exports_php_version: bool,
    /// `php_version`'s `(st_value, st_size)` — its *body*, and a separate fact:
    /// set only for a symbol typed `STT_FUNC` whose fields the walk could read.
    php_version_body: Option<(u64, u64)>,
}

/// Which of the symbols above the dynamic table exports.
fn scan_symbols(
    class: &ElfClass,
    dynsym: &[u8],
    entsize: usize,
    strtab: &[u8],
) -> Result<DynSymbols, String> {
    let minimum = match class {
        ElfClass::Elf64 => 24usize,
        ElfClass::Elf32 => 16usize,
    };
    if entsize < minimum {
        return Err("a .dynsym entry is too small to be a symbol".to_string());
    }
    let mut symbols = DynSymbols {
        executor_globals: false,
        tsrm_startup: false,
        exports_php_version: false,
        php_version_body: None,
    };
    for index in 0..dynsym.len() / entsize {
        let at = index.saturating_mul(entsize);
        let Some(name) = le_u32(dynsym, at).and_then(|offset| {
            usize::try_from(offset)
                .ok()
                .map(|offset| c_str(strtab, offset))
        }) else {
            continue;
        };
        match name {
            b"executor_globals" => symbols.executor_globals = true,
            b"tsrm_startup" => symbols.tsrm_startup = true,
            b"php_version" => {
                // Presence is the name alone (`MB.3.61`): an object that exports
                // `php_version` as `STT_NOTYPE`, or with `st_size == 0`, still
                // exports it, and saying otherwise is a false sentence. The body
                // is a second fact, read only when it is the shape the version
                // decode understands.
                symbols.exports_php_version = true;
                if symbol_is_function(*class, dynsym, at)
                    && let (Some(value), Some(size)) = (
                        symbol_value(*class, dynsym, at),
                        symbol_size(*class, dynsym, at),
                    )
                {
                    symbols.php_version_body = Some((value, size));
                }
            }
            _ => {}
        }
    }
    Ok(symbols)
}

/// `st_info`'s type nibble: `STT_FUNC`, the only kind the worker can call.
fn symbol_is_function(class: ElfClass, dynsym: &[u8], at: usize) -> bool {
    let info = match class {
        ElfClass::Elf64 => dynsym.get(at.saturating_add(4)).copied(),
        ElfClass::Elf32 => dynsym.get(at.saturating_add(12)).copied(),
    };
    info.is_some_and(|info| info & 0x0f == 2)
}

/// `st_value`: where the symbol's code or data lives.
fn symbol_value(class: ElfClass, dynsym: &[u8], at: usize) -> Option<u64> {
    match class {
        ElfClass::Elf64 => le_u64(dynsym, at.checked_add(8)?),
        ElfClass::Elf32 => le_u32(dynsym, at.checked_add(4)?).map(u64::from),
    }
}

/// `st_size`: how many bytes the symbol's body occupies.
fn symbol_size(class: ElfClass, dynsym: &[u8], at: usize) -> Option<u64> {
    match class {
        ElfClass::Elf64 => le_u64(dynsym, at.checked_add(16)?),
        ElfClass::Elf32 => le_u32(dynsym, at.checked_add(8)?).map(u64::from),
    }
}

/// The compile-time version `php_version()` returns, read statically
/// (`MB.3.59`).
///
/// The function is `return PHP_VERSION;` in every PHP 8.3+ build: one
/// PC-relative `lea` (`x86_64`) or `adrp`/`add` pair (`aarch64`) whose target
/// is the string in the object's own read-only data — on Alpine it is the
/// `8.4.27` inside the `X-Powered-By: PHP/8.4.27` literal, which is why a
/// search for a bare version string would miss it. Decoding the body is
/// precise: it reads exactly the constant the loader will receive, and
/// anything this walk does not understand is `None` (a refusal), never a guess
/// at a neighbouring library's version.
fn php_version_string(
    file: &mut std::fs::File,
    header: &ElfHeader,
    table: &[u8],
    (value, size): (u64, u64),
) -> Option<PhpVersion> {
    // 32 bytes is more than the longest body either decoder reads; a bigger
    // symbol is not a `return <constant>;` and is left to the refusal.
    let len = usize::try_from(size.min(32)).ok().filter(|len| *len > 0)?;
    let body_offset = vaddr_to_offset(header, table, value)?;
    let body = read_at(file, body_offset, len).ok()?;
    let string_vaddr = match header.machine {
        EM_X86_64 => decode_x86_64(&body, value)?,
        EM_AARCH64 => decode_aarch64(&body, value)?,
        _ => return None,
    };
    let string_offset = vaddr_to_offset(header, table, string_vaddr)?;
    let available = usize::try_from(header.size.checked_sub(string_offset)?).ok()?;
    let wanted = available.min(32);
    if wanted == 0 {
        return None;
    }
    let bytes = read_at(file, string_offset, wanted).ok()?;
    let end = bytes.iter().position(|byte| *byte == 0)?;
    let text = std::str::from_utf8(bytes.get(..end)?).ok()?;
    version_prefix(text)
}

/// The file offset of `vaddr`, from the section that contains it.
///
/// A section's `sh_addr` is its load address and `sh_offset` its file offset;
/// the difference is the mapping, so no program header is needed.
fn vaddr_to_offset(header: &ElfHeader, table: &[u8], vaddr: u64) -> Option<u64> {
    for index in 0..header.shnum {
        let at = index.saturating_mul(header.shentsize);
        let flags = section_flags(header.class, table, at)?;
        // A body and the constant it returns live in mapped, read-only
        // sections; a pointer into `.data` or `.bss` is not the shape this
        // walk understands.
        if flags & SHF_ALLOC == 0 || flags & SHF_WRITE != 0 {
            continue;
        }
        let (offset, size, _, _) = section_fields(header.class, table, at)?;
        let addr = section_addr(header.class, table, at)?;
        let end = addr.checked_add(size)?;
        if vaddr >= addr && vaddr < end {
            return offset.checked_add(vaddr.checked_sub(addr)?);
        }
    }
    None
}

/// `"8.4.27"` -> PHP 8.4: the first two dot-separated components.
fn version_prefix(text: &str) -> Option<PhpVersion> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse::<u8>().ok()?;
    let minor = parts.next()?.parse::<u8>().ok()?;
    Some(PhpVersion::new(major, minor))
}

/// The target of `php_version`'s body on `x86_64`, when it is the
/// `lea reg,[rip+disp]` a `return <constant>;` compiles to.
fn decode_x86_64(body: &[u8], at: u64) -> Option<u64> {
    // `endbr64` is what `-fcf-protection` prefixes to the function.
    let cursor = if body.starts_with(&[0xf3, 0x0f, 0x1e, 0xfa]) {
        4
    } else {
        0
    };
    let rex = *body.get(cursor)?;
    if rex & 0xf8 != 0x48 {
        return None;
    }
    if *body.get(cursor.checked_add(1)?)? != 0x8d {
        return None;
    }
    let modrm = *body.get(cursor.checked_add(2)?)?;
    // mod = 00, rm = 101: `[rip + disp32]`.
    if modrm & 0xc7 != 0x05 {
        return None;
    }
    let at_disp = cursor.checked_add(3)?;
    let disp = i32::from_le_bytes(
        body.get(at_disp..at_disp.checked_add(4)?)?
            .try_into()
            .ok()?,
    );
    let next = at.checked_add(u64::try_from(at_disp.checked_add(4)?).ok()?)?;
    Some(next.wrapping_add_signed(i64::from(disp)))
}

/// The target of `php_version`'s body on `aarch64`: `adrp` + `add` with the
/// low 12 bits, the PIC sequence a `return <constant>;` compiles to.
fn decode_aarch64(body: &[u8], at: u64) -> Option<u64> {
    // `bti c` is what `-mbranch-protection` prefixes to the function.
    let mut cursor: usize = if u32::from_le_bytes(body.get(..4)?.try_into().ok()?) == 0xd503_245f {
        4
    } else {
        0
    };
    let adrp = u32::from_le_bytes(body.get(cursor..cursor.checked_add(4)?)?.try_into().ok()?);
    // ADRP: op=1, 10000 in bits 28..24, Rd, immhi, immlo.
    if adrp & 0x9f00_0000 != 0x9000_0000 {
        return None;
    }
    let rd = adrp & 0x1f;
    let immlo = i64::from((adrp >> 29) & 0x3);
    let immhi = i64::from((adrp >> 5) & 0x7_ffff);
    let pages = sign_extend((immhi << 2) | immlo, 21) << 12;
    let page = at.checked_add(u64::try_from(cursor).ok()?)? & !0xfff;
    let base = page.wrapping_add_signed(pages);
    cursor = cursor.checked_add(4)?;
    let add = u32::from_le_bytes(body.get(cursor..cursor.checked_add(4)?)?.try_into().ok()?);
    // ADD (immediate), 64-bit: sf=1, op=0, S=0, 100010, sh, imm12, Rn, Rd.
    if add & 0x7f80_0000 != 0x1100_0000 {
        return None;
    }
    if add & 0x1f != rd || (add >> 5) & 0x1f != rd {
        return None;
    }
    let shift = (add >> 22) & 1;
    let imm12 = u64::from((add >> 10) & 0xfff);
    base.checked_add(imm12.checked_shl(12 * shift)?)
}

/// `value` as a `bits`-wide two's-complement integer.
fn sign_extend(value: i64, bits: u32) -> i64 {
    let shift = 64u32.saturating_sub(bits);
    (value << shift) >> shift
}

/// The NUL-terminated string at `offset`, or an empty slice.
fn c_str(bytes: &[u8], offset: usize) -> &[u8] {
    let Some(rest) = bytes.get(offset..) else {
        return &[];
    };
    let end = rest
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(rest.len());
    rest.get(..end).unwrap_or_default()
}

/// `len` bytes at `offset`, from a file this process has open read-only.
fn read_at(file: &mut std::fs::File, offset: u64, len: usize) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset))
        .map_err(|err| format!("the file could not be read ({err})"))?;
    let mut bytes = vec![0u8; len];
    file.read_exact(&mut bytes)
        .map_err(|err| format!("the file is truncated ({err})"))?;
    Ok(bytes)
}

/// A little-endian `u16` at `offset`, or `None` past the end.
fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(slice))
}

/// A little-endian `u32` at `offset`, or `None` past the end.
fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

/// A little-endian `u64` at `offset`, or `None` past the end.
fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice: [u8; 8] = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(slice))
}

/// The one line a boot emits when `[route.php]` discovery resolved a runtime
/// (`MB.3.35`): the *resolved* `libphp`, the shim and this build's
/// `<arch>-<libc>` tuple.
///
/// It is what makes the native DX observable — a `dev` boot that found
/// `target/php-sapi/libblastit_php84-x86_64-gnu.so` must not read like one that
/// did not — and a `libphp8.so -> libphp8.4.so` symlink has to be visible as
/// the real file the workers will load. It names no alternative transport, so
/// `critical_php_backend.rs`'s "no fallback recipe" contract still holds for the
/// whole boot output.
fn php_runtime_line(runtime: &PhpRuntime) -> String {
    format!(
        "the [route.php] runtime is libphp {} and shim {} (PHP {}, {})",
        runtime.libphp.display(),
        runtime.shim.display(),
        runtime.version.dotted(),
        target_tuple()
    )
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
fn static_root_refusal(manifest: &Manifest, index: usize) -> String {
    let root = manifest
        .route(index)
        .and_then(|route| route.static_policy())
        .map_or_else(String::new, |policy| policy.root.clone());
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
    // `./blastit.toml` that file is what is missing, and `BLASTIT_APP` would
    // send the operator looking for an environment variable instead of a
    // project (`docs/troubleshooting.md`).
    let source = match std::fs::read_to_string(&config.manifest) {
        Ok(source) => source,
        Err(err) => fail(&emitter, &format!("{}: {err}", config.manifest.display())),
    };
    let (manifest, warnings) = match Manifest::parse_and_validate(&source, config.uid_base) {
        Ok(parsed) => parsed,
        Err(errors) => fail(&emitter, &errors.to_string()),
    };
    // `R13`: `BLASTIT_APP` is required unless every route is static. The
    // manifest is parsed first, because "is every route static" is a property of
    // the file, not of the environment — and a missing `./blastit.toml` is still
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
    match php_boot_gate(&config, &manifest, PhpShimScope::Deployment) {
        PhpBoot::Absent => {}
        PhpBoot::Refused(message) => fail(&emitter, &message),
        PhpBoot::Proceed(runtime) => {
            // One line naming what was resolved (`MB.3.35`): the same
            // sentence `dev` prints, so a boot log names the library and the
            // shim it selected instead of leaving discovery invisible.
            emitter.emit(&Event::now(
                Component::Init,
                EventKind::ManifestWarning(WarningEvent::new(php_runtime_line(&runtime))),
            ));
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
    // second (`docs/troubleshooting.md`).
    let source = match std::fs::read_to_string(&config.manifest) {
        Ok(source) => source,
        Err(err) => dev_fail(&format!("{}: {err}", config.manifest.display())),
    };
    let (manifest, warnings) = match Manifest::parse_and_validate(&source, config.uid_base) {
        Ok(parsed) => parsed,
        Err(errors) => dev_fail(&format!("{errors}")),
    };
    // `R13`: a site whose every route is static needs no `app`; anything else
    // still names the setting as before (`MB.1.6`), and a `BLASTIT_APP` that
    // could never run is refused rather than ignored.
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
    match php_boot_gate(&config, &manifest, PhpShimScope::Development) {
        PhpBoot::Absent => {}
        PhpBoot::Refused(message) => dev_fail(&message),
        PhpBoot::Proceed(runtime) => {
            // Descriptor 2, like every pre-banner line: stdout stays empty
            // until the boot is known-good (§1.1). Without this line `dev`
            // reads byte-identically with and without the built shim
            // (`MB.3.35`).
            startup.emit(&Event::now(
                Component::Init,
                EventKind::ManifestWarning(WarningEvent::new(php_runtime_line(&runtime))),
            ));
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

    let dbproxy = fork_dbproxy_dev(
        &startup,
        &boot,
        interpreter,
        &prepared,
        listeners,
        learn.as_ref().map(|(_, obs)| obs.as_raw_fd()),
    );

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

    exec_stage1_dev(&boot, options.host, prepared.app_env(), dbproxy, learn)
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
) -> ! {
    let document_value = match config_document(boot.config) {
        Ok(value) => value,
        Err(message) => fail_dev_root(boot.root, &message),
    };
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

    let stage1 = boot.host.stage1();
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
fn route_declares_proxy(manifest: &Manifest, index: usize, proxy: &ProxyName) -> bool {
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
        let mut source = String::from("version = 1\n");
        for index in 0..routes {
            source.push_str(&format!(
                "\n[[route]]\nname = \"r{index}\"\npath = ['/r{index}/']\n"
            ));
        }
        Manifest::parse_and_validate(&source, 20_000).unwrap().0
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
version = 1

[proxy.db]
protocol = \"postgresql\"
input = { url = \"DATABASE_URL\" }

[group.denied.db]
queries = []

[[route]]
name = \"locked\"
path = ['/locked/.*']
[route.db]
queries = []

[[route]]
name = \"grouped\"
path = ['/grouped/.*']
groups = [\"denied\"]

[[route]]
name = \"ungranted\"
path = ['/ungranted/.*']
";
        let (manifest, _) = Manifest::parse_and_validate(source, 20_000).unwrap();
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

    /// A static-only manifest: one `[route.static]` root, no app.
    const STATIC_ONLY: &str = "\
version = 1

[[route]]
name = \"docs\"
path = ['.*']

[route.static]
root = \"site\"
";

    /// One route that declares no executor of its own: a WSGI view.
    const WITH_A_VIEW: &str = "\
version = 1

[[route]]
name = \"web\"
path = ['.*']
";

    /// One PHP route and no app: its own executor (`ADR-0010` §2), so this
    /// build can serve it without an interpreter anywhere in the image.
    const PHP_ONLY: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"
";

    /// The page's shape: files from disk, everything else to the front
    /// controller. Still no interpreter.
    const STATIC_AND_PHP: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.static]
root = \"public\"

[route.php]
entry = \"public/index.php\"
";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_and_validate(source, 20_000).unwrap();
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
    fn a_route_with_no_static_block_is_refused_by_name() {
        let message = no_python_support(&Config::default(), &manifest(WITH_A_VIEW), false).unwrap();
        assert!(message.contains("route \"web\""), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "{message}"
        );
        assert!(message.contains("[route.static]"), "{message}");
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
                .starts_with("BLASTIT_APP: every route is static"),
        );
    }

    /// A PHP route runs in a `view` worker with no interpreter, so **this**
    /// gate and the python-less build must say nothing about installing
    /// Python (`MB.3.5.4`). The boot's own answer for such a route is
    /// [`php_boot_gate`]'s — a different gate, with a different sentence.
    ///
    /// The denied twin in the same test is the manifest that *is* WSGI-shaped:
    /// a route with no `[route.static]` and no `[route.php]` keeps the gate's
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
        assert!(message.contains("route \"web\""), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "a WSGI view keeps the sentence that names the artifact: {message}"
        );
    }
}

/// `ADR-0010` §3/§4.1: the `[route.php]` discovery gate and its two exact
/// sentences.
///
/// Every rule here has a denied case and an accepted twin: a host with no
/// `libphp` at all, one whose object is ZTS or not an ELF at all, one whose
/// version no shim matches, and a `workers` cap above the view budget — beside
/// the manifest that declares no PHP route and must be left alone, and the
/// versioned NTS object with its shim that resolves.
///
/// The library fixtures are written out byte by byte (an ELF64 header plus a
/// `.dynsym` and its `.dynstr`), so the inline suite needs no C toolchain and
/// every case chooses exactly the exported names it needs. The `cc`-built
/// constructor-canary stubs — proving zero stage-0 `dlopen` from the outside —
/// belong to the protected suite (`MB.3.13.4`).
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod php_boot_gate {
    use super::*;

    /// The page's shape: files from disk, the rest to the front controller.
    const STATIC_AND_PHP: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.static]
root = \"public\"

[route.php]
entry = \"public/index.php\"
";

    /// Two PHP routes: the gate names the **first** one, so the sentence is
    /// actionable and stable.
    const TWO_PHP_ROUTES: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"

[[route]]
name = \"api\"
path = ['/api/.*']

[route.php]
entry = \"api/index.php\"
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

    /// A PHP route with a cap the tests compare against the budget.
    fn php_with_workers(workers: u32) -> String {
        format!(
            "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n\n[route.php]\n\
             entry = \"public/index.php\"\nworkers = {workers}\n"
        )
    }

    /// Two PHP routes where **the second** carries the cap: the budget rule is a
    /// manifest rule, so it must not stop at the first route.
    fn two_php_routes_with_second_workers(workers: u32) -> String {
        format!(
            "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n\n[route.php]\n\
             entry = \"public/index.php\"\n\n[[route]]\nname = \"api\"\npath = ['/api/.*']\n\n\
             [route.php]\nentry = \"api/index.php\"\nworkers = {workers}\n"
        )
    }

    /// A PHP route with no cap at all: the deployment's budget is the budget.
    const PHP_ONLY: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"
";

    const STATIC_ONLY: &str = "\
version = 1

[[route]]
name = \"docs\"
path = ['.*']

[route.static]
root = \"site\"
";

    const WITH_A_VIEW: &str = "\
version = 1

[[route]]
name = \"web\"
path = ['.*']
";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_and_validate(source, 20_000).unwrap();
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
            matches!(
                php_boot(&manifest(PHP_ONLY), &search, Some(16)),
                PhpBoot::Proceed(_)
            ),
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

        let message = php_boot(
            &manifest(PHP_ONLY),
            &search_for(&link, &shims.dir),
            Some(16),
        )
        .refusal()
        .expect("a dangling link cannot be embedded")
        .to_string();
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("[route.php] entry \"public/index.php\""),
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
                matches!(
                    php_boot(&manifest(source), &search, Some(16)),
                    PhpBoot::Absent
                ),
                "{source}"
            );
        }
        let message = php_boot(&manifest(STATIC_AND_PHP), &search, Some(16))
            .refusal()
            .expect("a PHP route")
            .to_string();
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("[route.php] entry \"public/index.php\""),
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
        let two = php_boot(&manifest(TWO_PHP_ROUTES), &search, Some(16))
            .refusal()
            .expect("a PHP route")
            .to_string();
        assert!(two.contains("route \"app\""), "{two}");
        assert!(!two.contains("api/index.php"), "{two}");

        // The `dev` twin is the same refusal: `ADR-0010` §1 gives PHP one
        // backend, so a developer host with nothing installed gets the package
        // sentence too, not a boot that serves the route another way.
        let dev = php_boot(&manifest(PHP_ONLY), &search, Some(16))
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
        let message = php_boot(&manifest(PHP_ONLY), &search, Some(16))
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
            matches!(
                php_boot(&manifest(PHP_ONLY), &search, Some(16)),
                PhpBoot::Proceed(_)
            ),
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
            let message = php_boot(
                &manifest(PHP_ONLY),
                &search_for(&path, &shims.dir),
                Some(16),
            )
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
        let message = php_boot(
            &manifest(PHP_ONLY),
            &search_for(&directory, &shims.dir),
            Some(16),
        )
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
        let message = php_boot(
            &manifest(PHP_ONLY),
            &search_for(&unversioned, &shims.dir),
            Some(16),
        )
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
        let message = php_boot(
            &manifest(PHP_ONLY),
            &search_for(&conflicted, &shims.dir),
            Some(16),
        )
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
            Some(16),
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
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&old, &shims.dir), Some(16))
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
        let message = php_boot(&manifest(PHP_ONLY), &search, Some(16))
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
        let message = php_boot(&manifest(PHP_ONLY), &search, Some(16))
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

        let boot = php_boot(&manifest(PHP_ONLY), &search, Some(16));
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
            let message = php_boot(
                &manifest(PHP_ONLY),
                &search_for(&unversioned, &shims.dir),
                Some(16),
            )
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
            let message = php_boot(
                &manifest(PHP_ONLY),
                &search_for(&absent, &shims.dir),
                Some(16),
            )
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
        let message = php_boot(&manifest(PHP_ONLY), &search, Some(16))
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
        let dev = php_boot(&manifest(PHP_ONLY), &dev_search, Some(16))
            .refusal()
            .expect("dev refuses a missing shim too")
            .to_string();
        assert!(
            dev.contains(&format!("libblastit_php84-{}.so", target_tuple())),
            "{dev}"
        );
    }

    /// `ADR-0010` §2: `workers` is downward only. The error names both
    /// numbers, and comes before the discovery so the operator fixes the
    /// manifest first; `workers == cap` (and no `workers` at all) passes it.
    #[test]
    fn a_workers_cap_above_the_view_budget_names_both_numbers() {
        let scratch = Scratch::new("workers");
        let (_, resolved) = resolved_84(&scratch);

        let config = Config {
            max_workers: 8,
            view_max_workers: None,
            ..Config::default()
        };
        let gate = php_boot_gate(
            &config,
            &manifest(&php_with_workers(9)),
            PhpShimScope::Deployment,
        );
        let message = gate
            .refusal()
            .expect("9 is above the budget of 8")
            .to_string();
        assert!(message.contains("workers = 9"), "{message}");
        assert!(message.contains("budget of 8"), "{message}");
        assert!(message.contains("BLASTIT_VIEW_MAX_WORKERS"), "{message}");
        assert!(
            !message.contains("apt install"),
            "the budget is named before any discovery: {message}"
        );

        // The accepted twins: exactly the budget, and no cap at all.
        for source in [php_with_workers(8), PHP_ONLY.to_string()] {
            assert!(
                matches!(
                    php_boot(&manifest(&source), &resolved, Some(8)),
                    PhpBoot::Proceed(_)
                ),
                "{source}: the cap passed"
            );
        }
        // And the second route's over-budget cap is still refused below.
        let second = php_boot(
            &manifest(&two_php_routes_with_second_workers(20)),
            &resolved,
            Some(16),
        );
        let message = second
            .refusal()
            .expect("the second route's 20 is above the budget of 16")
            .to_string();
        assert!(message.contains("route \"api\""), "{message}");
        assert!(!message.contains("route \"app\""), "{message}");
        assert!(message.contains("workers = 20"), "{message}");
        assert!(message.contains("budget of 16"), "{message}");
        assert!(
            matches!(
                php_boot(
                    &manifest(&two_php_routes_with_second_workers(16)),
                    &resolved,
                    Some(16)
                ),
                PhpBoot::Proceed(_)
            ),
            "the second route at exactly the budget passes"
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
        let message = php_boot(
            &manifest(PHP_ONLY),
            &search_for(&unversioned, &shims.dir),
            Some(16),
        )
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
        let message = php_boot(&manifest(PHP_ONLY), &search, Some(16))
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
                    matches!(
                        php_boot(&manifest(PHP_ONLY), &search, None),
                        PhpBoot::Proceed(_)
                    ),
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
        let deployment = Deployment {
            init_uid: current_uid(),
            manifest_dir: protected.dir.clone(),
        };
        let message = php_boot_in(&manifest(PHP_ONLY), &search, Some(16), Some(&deployment))
            .refusal()
            .expect("a writable libphp directory cannot be a deployment")
            .to_string();
        // `ADR-0010` §4.1's shape: the route, its entry, the offending path, the
        // tuple and the shim, the package, and the override.
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("[route.php] entry \"public/index.php\""),
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
        let message = php_boot_in(&manifest(PHP_ONLY), &search, Some(16), Some(&deployment))
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
                php_boot_in(&manifest(PHP_ONLY), &good, Some(16), Some(&deployment)),
                PhpBoot::Proceed(_)
            ),
            "a 0755 pair plus a protected tree must not be refused"
        );
    }

    /// `B2-R5`: the tree the worker's `cwd`/`include_path` is — `blastit.toml`'s
    /// directory, each `[route.php] entry` and each `[route.static] root` — is
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
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"

[[route]]
name = \"docs\"
path = ['/assets/.*']

[route.static]
root = \"site\"
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
        let accepted = Deployment {
            init_uid: current_uid(),
            manifest_dir: protected.dir.clone(),
        };
        assert!(
            matches!(
                php_boot_in(
                    &manifest(manifest_source),
                    &search,
                    Some(16),
                    Some(&accepted)
                ),
                PhpBoot::Proceed(_)
            ),
            "a 0755 tree owned by init's uid is the accepted shape: {:?}",
            php_boot_in(
                &manifest(manifest_source),
                &search,
                Some(16),
                Some(&accepted)
            )
        );

        // 1. The directory the manifest was read from.
        let shared = Scratch::new("tree-shared");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let message = php_boot_in(
            &manifest(manifest_source),
            &search,
            Some(16),
            Some(&Deployment {
                init_uid: current_uid(),
                manifest_dir: shared.dir.clone(),
            }),
        )
        .refusal()
        .expect("a writable manifest directory cannot be a deployment")
        .to_string();
        assert!(
            message.contains("the directory holding blastit.toml"),
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
        let message = php_boot_in(
            &manifest(manifest_source),
            &search,
            Some(16),
            Some(&accepted),
        )
        .refusal()
        .expect("a writable front controller cannot be a deployment")
        .to_string();
        assert!(
            message.contains("route \"app\": [route.php] entry \"public/index.php\""),
            "the entry is named with its route: {message}"
        );
        assert!(message.contains(&entry.display().to_string()), "{message}");
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();

        // 3. A static root: the responder reads it, and the manifest declares
        // one next to the PHP route.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let message = php_boot_in(
            &manifest(manifest_source),
            &search,
            Some(16),
            Some(&accepted),
        )
        .refusal()
        .expect("a writable static root cannot be a deployment")
        .to_string();
        assert!(
            message.contains("route \"docs\": [route.static] root \"site\""),
            "the static root is named with its route: {message}"
        );
        assert!(message.contains(&root.display().to_string()), "{message}");

        // The same tree at `0755` is accepted: the bits, not the path.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(
                php_boot_in(
                    &manifest(manifest_source),
                    &search,
                    Some(16),
                    Some(&accepted)
                ),
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
        let deployment = Deployment {
            init_uid: current_uid(),
            manifest_dir: protected.dir.clone(),
        };
        // A PHP route and a static route, as in the tree test above: the gate
        // only runs for a manifest with a `[route.php]`, so the static half must
        // ride beside one.
        let source = |entry: &str, root: &str| {
            format!(
                "version = 1\n\n[[route]]\nname = \"app\"\npath = ['.*']\n\n[route.php]\nentry = \
                 \"{entry}\"\n\n[[route]]\nname = \"docs\"\npath = ['/assets/.*']\n\n\
                 [route.static]\nroot = \"{root}\"\n"
            )
        };
        let refuse = |source: &str| {
            php_boot_in(&manifest(source), &search, Some(16), Some(&deployment))
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
            message.contains("route \"docs\": [route.static] root \"site\""),
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
                    Some(16),
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
    /// A deployment whose manifest declares no `[route.php]` route has no pair
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

        // 1. No `[route.php]`: nothing to authorise, so the boot proceeds.
        let static_only = "\
version = 1

[[route]]
name = \"docs\"
path = ['.*']

[route.static]
root = \"site\"
";
        assert_eq!(
            deployment_boot_check(static_only, &protected.dir, &libphp, &protected.dir, 20_000),
            Ok(()),
            "a manifest with no [route.php] proceeds through the deployment gate"
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
            matches!(
                php_boot(&manifest(PHP_ONLY), &search, Some(16)),
                PhpBoot::Proceed(_)
            ),
            "`dev`/`check` must keep booting out of a world-writable checkout"
        );
        // And the production question about those same bytes is a refusal, so
        // this test cannot pass by the fixture being acceptable after all.
        let message = php_boot_in(
            &manifest(PHP_ONLY),
            &search,
            Some(16),
            Some(&Deployment {
                init_uid: current_uid(),
                manifest_dir: shared.dir.clone(),
            }),
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

/// `R13` after `ADR-0010` §2: a `[route.php]` route is its own executor, so the
/// app rule must never demand the WSGI callable for it (`MB.3.5.4`).
///
/// The old predicate was "every route is static", which read "not all-static"
/// as "needs an app" and refused a PHP-only manifest with
/// `BLASTIT_APP: the WSGI callable (…) is required` — advice that cannot make a
/// PHP route serve. What the rule actually asks is whether every route runs its
/// own executor.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod app_rule_executors {
    use super::*;

    /// A PHP route with no app: the app rule must not demand the callable.
    /// (What a `[route.php]` boot does need is a usable NTS `libphp`, and
    /// [`php_boot_gate`] is the gate that asks; that is a different sentence.)
    const PHP_ONLY: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"
";

    /// Files from disk, everything else to the front controller: also no app.
    const STATIC_AND_PHP: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.static]
root = \"public\"

[route.php]
entry = \"public/index.php\"
";

    /// One route that declares no executor: a WSGI view, which needs the app.
    const WITH_A_VIEW: &str = "\
version = 1

[[route]]
name = \"web\"
path = ['.*']
";

    /// A PHP route beside a WSGI view: the view still needs the callable, so one
    /// own-executor route cannot excuse the manifest.
    const PHP_AND_A_VIEW: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"

[[route]]
name = \"web\"
path = ['/api/.*']
";

    /// The sentence the rule refuses with, exactly.
    const REQUIRE_APP: &str = "BLASTIT_APP: the WSGI callable (`module:attribute`) is required";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_and_validate(source, 20_000).unwrap();
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
        for source in [
            WITH_A_VIEW,
            PHP_AND_A_VIEW,
            "version = 1\n",
            "version = 1\n\n[[route]]\nname = \"web\"\npath = ['.*']\n\
             [route.ingress]\nbody = \"json\"\n",
        ] {
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

        let static_only = "version = 1\n\n[[route]]\nname = \"docs\"\npath = ['.*']\n\
                           [route.static]\nroot = \"site\"\n";
        assert_eq!(app_rule(&Config::default(), &manifest(static_only)), Ok(()));
        let message = app_rule(&with_app, &manifest(static_only)).unwrap_err();
        assert!(
            message.starts_with("BLASTIT_APP: every route is static"),
            "{message}"
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
        let (manifest, _warnings) = Manifest::parse_and_validate(source, 20_000).unwrap();
        manifest
    }

    const REFUSED: &str = "\
version = 1
app = \"app:app\"

[[route]]
name = \"upload\"
path = ['/upload']
methods = [\"POST\"]

[route.ingress]
body = \"raw\"
";

    const ACCEPTED: &str = "\
version = 1
app = \"app:app\"

[[route]]
name = \"upload\"
path = ['/upload']
methods = [\"POST\"]

[route.ingress]
body = \"raw\"
allow_raw = true
";

    /// The gate has exactly one refusal and it names the route and the key; the
    /// acknowledgement opens it, which is what makes this a gate rather than a
    /// blanket refusal of the codec.
    #[test]
    fn an_unacknowledged_raw_route_is_refused_and_an_acknowledged_one_is_not() {
        let message = raw_ingress_gate(&manifest(REFUSED)).unwrap_err();
        assert!(message.contains("route \"upload\""), "{message}");
        assert!(message.contains("allow_raw = true"), "{message}");
        assert!(message.contains("[route.ingress]"), "{message}");
        assert_eq!(raw_ingress_gate(&manifest(ACCEPTED)), Ok(()));
    }

    /// A structured codec is untouched by the gate: only `raw` has no runtime
    /// validation for the acknowledgement to stand in for.
    #[test]
    fn every_other_codec_and_no_ingress_at_all_pass_the_gate() {
        for body in ["json", "form", "none"] {
            let source = format!(
                "version = 1\napp = \"app:app\"\n\n[[route]]\nname = \"upload\"\n\
                 path = ['/upload']\nmethods = [\"POST\"]\n\n[route.ingress]\nbody = \"{body}\"\n"
            );
            assert_eq!(
                raw_ingress_gate(&manifest(&source)),
                Ok(()),
                "body = {body} needs no acknowledgement"
            );
        }
        let no_ingress = "version = 1\napp = \"app:app\"\n\n[[route]]\nname = \"home\"\n\
                          path = ['.*']\n";
        assert_eq!(raw_ingress_gate(&manifest(no_ingress)), Ok(()));
    }

    /// Every offender is named, in route order, so a manifest edited in two
    /// places is fixed in one boot rather than one denial at a time.
    #[test]
    fn every_unacknowledged_route_is_named_in_route_order() {
        let source = REFUSED.replace("name = \"upload\"", "name = \"first\"")
            + "\n[[route]]\nname = \"second\"\npath = ['/second']\nmethods = [\"POST\"]\n\
               \n[route.ingress]\nbody = \"raw\"\n";
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
