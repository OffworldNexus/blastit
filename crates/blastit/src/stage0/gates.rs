use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

use blastit_dbproxy::ListenerSpec;
use blastit_events::{Component, Event, EventKind, Runtime};
use blastit_manifest::{Manifest, PgGrant, ProxyName, UidLayout};
use blastit_sys::{bind_unix_listener, make_private_dir};

use crate::config::{Config, view_dir};
// The two provenance types are the deployment gate's (`php` only): the rest of
// stage 0 names neither.
#[cfg(feature = "php")]
use crate::host::{Claim, Provenance};
use crate::logs::{Emitter, fail};
// The `entrypoint php` discovery gate (`MB.36.5`): a stage-0 dependency that
// names no SAPI type and no `dlopen`, so `init` can answer "can this host embed
// PHP?" without loading anything (`ADR-0010` §3, design §4.4 D5).
#[cfg(feature = "php")]
use blastit_view_php::discovery::{Deployment, PhpBoot, PhpSearch, PhpShimScope, php_boot_in};
// The discovery surface the `php_boot_gate` cases drive: their subjects are the
// crate's, so the names are the tests' only.
#[cfg(feature = "python")]
use crate::py::Python;

use super::current_uid;
use super::project_python::Venv;

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
    pub(super) fn clear(manifest: &Manifest) -> Result<Self, String> {
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
pub(super) fn probe_timeout() -> Result<Option<Duration>, String> {
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
pub(super) fn config_document(config: &Config) -> Result<serde_json::Value, String> {
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
pub(super) fn object_is_protected(init_uid: u32, what: &str, path: &Path) -> Result<(), String> {
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
pub(super) fn tree_is_protected(init_uid: u32, path: &Path) -> Result<(), String> {
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
pub(super) fn php_boot(manifest: &Manifest, search: &PhpSearch) -> PhpBoot {
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
pub(super) fn emit_runtime(
    emitter: &Emitter,
    config: &Config,
    interpreter: bool,
    venv: Option<&Venv>,
) {
    // A build without the `php` feature has no record to ask: no manifest that
    // reaches this point declares one (the gate refuses it first), so "no PHP
    // entrypoint" is the answer that cannot invent a line.
    #[cfg(feature = "php")]
    let entrypoint_php = config.php.is_some();
    #[cfg(not(feature = "php"))]
    let entrypoint_php = false;
    // A standalone `entrypoint python` boot has no interpreter in *this*
    // process, so [`runtime_shape`] honestly says nothing — but stage 0 found
    // the project's venv and matched it against the embedded CPython, and that
    // venv is exactly what the line exists to name (`MB.40.8`).
    let runtime = runtime_shape(entrypoint_php, config.app.is_some(), interpreter)
        .or_else(|| venv.map(embedded_runtime));
    if let Some(runtime) = runtime {
        emitter.emit(&Event::now(Component::Init, EventKind::Runtime(runtime)));
    }
}

/// The `runtime` line of a standalone `entrypoint python` boot (`MB.40.8`).
///
/// The version is the release `pyvenv.cfg` records (`3.14.10`), which is the
/// embedded CPython minor's patch level, or the matched `major.minor` when the
/// file names none; the environment is the venv root, the directory a developer
/// named with `uv venv`.
fn embedded_runtime(venv: &Venv) -> Runtime {
    Runtime::python(venv.release.clone(), venv.root.display().to_string())
}

/// The decision table behind [`emit_runtime`], as a value: which shape a boot
/// resolved, or `None` when the line is not this stage's to emit.
///
/// The three inputs are what actually decides it — a resolved library, a
/// callable, and an interpreter live in *this* process — so the table is one
/// testable function rather than three conditions spread over two boot paths.
pub(super) fn runtime_shape(
    entrypoint_php: bool,
    has_app: bool,
    interpreter: bool,
) -> Option<Runtime> {
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
pub(super) fn python_runtime(interpreter: bool) -> Option<Runtime> {
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
pub(super) fn python_runtime(_interpreter: bool) -> Option<Runtime> {
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
pub(super) fn listable(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => !matches!(entries.next(), Some(Err(_))),
        Err(_) => false,
    }
}

/// The declared root, as written, in the sentence a boot refuses with.
///
/// `index` is a **mount** index (§9.6), which is what `static_roots` returns.
pub(super) fn static_root_refusal(manifest: &Manifest, index: usize) -> String {
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
pub(super) fn layout_collision(
    manifest: &Manifest,
    layout: &UidLayout,
    uid: u32,
) -> Option<String> {
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
pub(super) fn bind_view_sockets(
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
