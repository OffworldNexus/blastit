//! `blastit check`: audit a pytest suite against a `Blastfile` (R3..R5, R30).
//!
//! The command is a transaction with one place where it can fail and one place
//! where it writes:
//!
//! 1. **Before anything runs**, the CLI, the configuration, `./Blastfile`
//!    and every `[proxy.*]` input are validated. Any problem exits **`2`** with
//!    **nothing on stdout** — under `--json` stdout is reserved for exactly one
//!    document.
//! 2. **The credentials-holding role** is started in
//!    [`blastit_dbproxy::ListenerMode::DynamicCheck`] on one
//!    wildcard socket per proxy under `<runtime_dir>/pg`, so it never refuses a
//!    statement a missing grant would have refused (`R26`), and it writes every
//!    statement to an observation pipe.
//! 3. **`pytest -q -p no:blastit -p blastit._check_pytest`** runs unprivileged
//!    with the secret-free `swapped_env` whose `postgresql` outputs point at
//!    that socket directory. `-p no:blastit` disables the `0.1.0` `pytest11`
//!    entry point, so a project that still carries `blastit_manifest` in its
//!    `pyproject.toml` is audited by this gate and nothing else (`R30`,
//!    `ADR-0004`).
//! 4. **The audit** is [`evaluate`]'s: routing, effective grants, the allowlist
//!    and the human formatter are all its, so this module never re-implements
//!    one of them.
//! 5. **`--learn`** proposes routes and exact `queries` and writes them only
//!    when the suite passed, a request was observed, nothing was refused, this
//!    is not a dry run, and there is something to write (`R29`).
//!
//! # The module tree
//!
//! The command's own process lifecycle is `session` (the private runtime root),
//! `django` (the one-shot `pytest-django` handshake), `dbproxy` (one listener per
//! proxy and the run's drains), `pytest` (the suite and its interpreter) and
//! `report` (the document, the `learn:` line and route resolution). The grant
//! diff and its findings are `evaluate`, `ledger`, `diff`, `dead`, `ingress`,
//! `findings`, `human`, `learn`, `infer_routes`, `patterns`, `static_roots`,
//! `obs` and `error`.
//!
//! # Why the gate reports unrouted requests
//!
//! The ID's route is not known to the Python gate, and deciding it there would
//! mean re-implementing [`blastit_manifest::RoutingTable`]'s regex semantics in
//! Python — exactly the thing this command exists to avoid. The gate therefore
//! pushes each request as [`blastit_dbproxy::ObsTarget::Unrouted`] (the control protocol's
//! `push_unrouted`), and this module resolves the request against blastit's own
//! table afterwards (`report::resolve_targets`). A request that matches no route stays
//! unrouted, which is what `unrouted_request` and `--learn` want.
//!
//! # Stdout purity
//!
//! Under `--json` the pytest child's fd 1 is redirected onto this process's
//! stderr at the OS level (`Stdio`), so a `print()` in the suite cannot reach
//! stdout, and the audit document is printed only after the exit code is known
//! to be `0` or `1` (`R5`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::fd::AsRawFd;
use std::path::Path;

use blastit_manifest::{Manifest, RoutingTable};

use crate::cli::CheckOptions;
use crate::config::Config;
use crate::host::Host;
use crate::logs::{Colour, Emitter};
#[cfg(feature = "php")]
use crate::stage0::php_boot_gate;
use crate::stage0::{is_static_only, static_app_refusal, static_root_permission};
#[cfg(feature = "php")]
use blastit_view_php::discovery::{PhpBoot, PhpShimScope};

use self::dbproxy::{Dbproxy, EventDrain, Inherited, ObsReader, bind_listeners};
use self::django::{DjangoPreflight, Handshake, django_setup};
use self::pytest::{PytestSpawn, python_program, require_pytest, run_pytest};
use self::report::{emit_report, learn, resolve_targets};
use self::session::{CheckRoot, clear_cloexec, observation_error, socketpair};

mod dbproxy;
mod dead;
mod diff;
mod django;
mod error;
mod evaluate;
mod findings;
mod human;
/// Route inference for `--learn`: the views an unrouted request proposes.
pub mod infer_routes;
mod ingress;
mod learn;
mod ledger;
mod obs;
mod patterns;
mod pytest;
mod report;
mod session;
mod static_roots;

pub use self::error::ObsError;
pub use self::evaluate::{evaluate, parse_manifest};
pub use self::findings::{Finding, Fix, ProxyCounts, Report, RouteCounts, Severity};
pub use self::human::{human, plural};
pub use self::learn::{LearnPlan, plan_learn, plan_learn_json};
pub use self::obs::{CheckObsV1, HttpObsLine, SCHEMA_VERSION, parse_http_obs, parse_sql_obs};
pub use self::patterns::pattern_is_writable;
pub use self::static_roots::{unrequested_static_root_line, unrequested_static_roots};

/// How many distinct test node ids and paths one report entry keeps.
const EXAMPLES: usize = 5;

/// The label a request made outside any test is reported under.
const NO_TEST: &str = "(no test)";

/// The process exited cleanly: pytest passed and no finding fails the run.
const CLEAN: u8 = 0;

/// pytest passed, but the manifest is incomplete (`R4`).
const FINDINGS: u8 = 1;

/// Blastit could not start: CLI, config, manifest, missing input, missing
/// pytest, or an observation file that does not parse (`R4`).
const START_FAILURE: u8 = 2;

/// pytest itself failed, exit `5` included (`R4`).
const SUITE_FAILURE: u8 = 3;

/// The variable naming the JSONL file the WSGI gate appends to.
///
/// Internal to this command: it is added to the already-secret-free
/// `swapped_env`, and it names a path, never a value.
pub const HTTP_OBS: &str = "BLASTIT_CHECK_HTTP_OBS";

/// The variable naming the inherited descriptor of dbproxy's control channel.
///
/// Internal to this command, like [`HTTP_OBS`]: a descriptor number, never a
/// value. The gate pushes one target per request through it.
pub const CTL_FD: &str = "BLASTIT_CHECK_CTL_FD";

/// The variable naming the one-shot Django handshake file (`R30`, `ADR-0004`).
///
/// Only set when `pytest-django` is importable: the file holds the real proxy
/// inputs, is `0600`, and `_check_pytest` reads and unlinks it before Django
/// builds its settings. Nothing else in the suite's environment carries them.
pub const DJANGO_SETUP: &str = "BLASTIT_CHECK_DJANGO_SETUP";

/// Runs `blastit check` (or its alias `blastit test`) and returns its exit code.
///
/// Every failure this module can name is exit `2` with a one-line diagnostic on
/// stderr and nothing on stdout. `pytest` never writes to stdout under `--json`.
#[must_use]
pub fn run(host: &Host, options: &CheckOptions, colour: Colour) -> u8 {
    match execute(host, options, colour) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("blastit check: {message}");
            START_FAILURE
        }
    }
}

/// One `blastit check` run.
///
/// # Errors
///
/// A one-line message for everything that is exit `2`: the manifest, the
/// configuration, a proxy input, `pytest` missing, the runtime directory, or an
/// observation line that does not parse.
fn execute(host: &Host, options: &CheckOptions, colour: Colour) -> Result<u8, String> {
    let mut config = Config::for_check().map_err(|error| error.to_string())?;
    config.validate().map_err(|error| error.to_string())?;

    let source = read_manifest(&config.manifest)?;
    let (manifest, _warnings) =
        Manifest::parse_blastfile(&source, config.uid_base).map_err(|errors| {
            format!(
                "{}: {errors}",
                crate::config::display_path(&config.manifest)
            )
        })?;
    // The routing table is built here, not only inside the engine: the same
    // table resolves each observed request to its route (`resolve_targets`), so
    // a pattern that cannot compile fails before pytest starts.
    let table = RoutingTable::new(&manifest)
        .map_err(|errors| format!("{}: {errors}", config.manifest.display()))?;
    // `R13`: a `BLASTIT_APP` no route of an all-static manifest could reach is
    // refused by name. `check` does not demand an app otherwise — the suite
    // brings its own WSGI callable — so this half of the boot's rule is the
    // only one it applies.
    if let Some(message) = static_app_refusal(&config, &manifest) {
        return Err(message);
    }
    // `ADR-0010` §3, §4.1: the same `entrypoint php` discovery gate the boot runs,
    // so `blastit check` reaches the package sentence rather than handing pytest
    // a manifest no worker of this host could serve. `check` looks where `dev`
    // does (`BLASTIT_PHP_SHIM_DIR`, the `target/php-sapi` `make php-sapi`
    // writes), because it runs on the machine that builds the site. A
    // configuration error is exit `2` with nothing on stdout.
    #[cfg(feature = "php")]
    match php_boot_gate(&config, &manifest, PhpShimScope::Development) {
        PhpBoot::Absent | PhpBoot::Proceed(_) => {}
        PhpBoot::Refused(message) => return Err(message),
    }
    // A build without the `php` feature carries no gate to ask: the same one
    // sentence the boot prints, so `check` fails for the same reason the server
    // would (`R13`).
    #[cfg(not(feature = "php"))]
    if let Some(message) = crate::stage0::no_php_support(&manifest) {
        return Err(message);
    }
    manifest
        .check_static_roots(&config.manifest_dir)
        .map_err(|errors| {
            format!(
                "{}: {errors}",
                crate::config::display_path(&config.manifest)
            )
        })?;
    if let Some(message) = static_root_permission(
        &manifest,
        &config.manifest_dir,
        rustix::process::geteuid().as_raw(),
    ) {
        return Err(message);
    }
    // `R14`: an all-static site has nothing to exercise — no pytest, no
    // dbproxy, no observation file. Every root is audited above; the report is
    // the same document a run with an app prints, with the counts at zero.
    if is_static_only(&manifest) {
        if options.learn {
            return Err(
                "nothing to learn: the file answers every request from static mounts".to_string(),
            );
        }
        // `--json` prints exactly one document; the human form prints nothing
        // at all, because every root was audited and nothing ran.
        if options.json {
            emit_report(&Report::all_static(&manifest), true)?;
        }
        return Ok(CLEAN);
    }

    let runtime = CheckRoot::create()?;
    let socket_dir = runtime.postgres_dir()?;
    let socket_dir = socket_dir
        .to_str()
        .ok_or_else(|| "the runtime directory is not valid UTF-8".to_string())?
        .to_string();

    // One emitter for dbproxy's own events, built here at the command's root:
    // the run's configured level and format, on stderr — `check`'s stdout stays
    // one document under `--json`, and a `BLASTIT_LOG_LEVEL=error` run must not
    // print a `warn`.
    let drain_emitter = Emitter::new(
        std::io::stderr().as_raw_fd(),
        config.log_format,
        config.log_level,
        colour.stderr,
    );

    let platform = platform_env(&manifest)?;
    let prepared = prepare_env(&manifest, &platform, &socket_dir)?;
    // Both preflights run before dbproxy or the suite start, so a suite that
    // cannot run at all is exit `2` — never a proxy left behind, and never a
    // Django run that could reach the developer's own database (`R4`, `R30`).
    let python = python_program(&config.manifest_dir)?;
    require_pytest(&python, &prepared)?;
    let handshake = Handshake::new(django_setup(
        &DjangoPreflight {
            manifest: &manifest,
            prepared: &prepared,
            platform: &platform,
            socket_dir: &socket_dir,
        },
        &python,
        runtime.path(),
    )?);
    let listeners = bind_listeners(&prepared, Path::new(&socket_dir))?;
    // A store-less app (a hello-world with no `postgres` store) declares no
    // `postgresql` proxy, so there is no socket to hand dbproxy and no statement
    // to attribute: dbproxy is not started at all and the gate records the HTTP
    // requests alone, which is exactly what the audit of such an app reads. The
    // control channel is not opened either, so nothing can be pushed into a
    // channel nobody reads.
    let mut dbproxy = None;
    let mut events = None;
    let mut observed = None;
    let mut ctl_python = None;
    if !listeners.is_empty() {
        let (obs_read, obs_child) = socketpair("the observation pipe")?;
        let (ctl_dbproxy, python_end) = socketpair("the control channel")?;
        let (ready_read, ready_child) = socketpair("the ready pipe")?;
        let (proxy, events_read) = Dbproxy::start(
            host,
            &manifest,
            &prepared,
            listeners,
            Inherited {
                obs: obs_child,
                ctl: ctl_dbproxy,
                ready: ready_child,
            },
            ready_read,
        )?;
        dbproxy = Some(proxy);
        events = Some(EventDrain::start(events_read, drain_emitter));
        // Only *after* dbproxy's `execve`: the suite's end of the control
        // channel has to cross pytest's `execve` (`Stdio` does not carry it), but
        // dbproxy must never inherit it, so it keeps `FD_CLOEXEC` until it is
        // spawned.
        clear_cloexec(python_end.as_raw_fd())?;
        observed = Some(ObsReader::start(obs_read));
        ctl_python = Some(python_end);
    }

    let http_path = runtime.path().join("http_obs.jsonl");
    let status = run_pytest(PytestSpawn {
        options,
        prepared: &prepared,
        python: &python,
        http_path: &http_path,
        ctl_fd: ctl_python.as_ref().map(AsRawFd::as_raw_fd),
        json: options.json,
        django: handshake.path(),
    });
    // The child closes its end; ours is dropped once pytest is gone.
    drop(ctl_python);
    let code = match status {
        Ok(code) => code,
        Err(message) => {
            if let Some(proxy) = dbproxy.as_mut() {
                proxy.stop();
            }
            if let Some(drain) = events.take() {
                drain.stop();
            }
            if let Some(reader) = observed.take() {
                let _ = reader.stop();
            }
            return Err(message);
        }
    };
    if let Some(proxy) = dbproxy.as_mut() {
        proxy.stop();
    }
    // The proxy's write end is closed, so the drain's reader sees EOF: every
    // line dbproxy emitted is forwarded here, before the report and on every
    // path (`Drop` repeats this for the paths that return early).
    if let Some(drain) = events.take() {
        drain.stop();
    }
    let sql_text = observed.map(ObsReader::stop).unwrap_or_default();

    if code != 0 {
        // R4: pytest failed, exit 5 included. No audit report is emitted and
        // `--learn` writes nothing.
        return Ok(SUITE_FAILURE);
    }

    let http_text = match std::fs::read_to_string(&http_path) {
        Ok(text) => text,
        // A suite that never calls the application writes no line at all: that
        // is zero requests, which the audit reports as `no_observations` and
        // `--learn` as `"wrote": false` (`R35`, `R29`), not a start failure.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!(
                "{}: {error} (the WSGI gate writes it)",
                http_path.display()
            ));
        }
    };
    let mut observations = CheckObsV1::from_jsonl(&http_text, &sql_text)
        .map_err(|error| observation_error(&runtime, error))?;
    resolve_targets(&table, &manifest, &mut observations);

    if options.learn {
        return Ok(learn(&config, &source, &manifest, &observations, options));
    }

    let report = evaluate(&manifest, &observations).map_err(|error| error.to_string())?;
    // `R14`: a declared static root the suite never requested is a line on
    // stderr — never a finding, never `ok`, never the exit code. It is the
    // scheduler that answers those files, so the observations cannot see them.
    let mounts = blastit_manifest::MountTable::new(&manifest);
    for (prefix, root) in unrequested_static_roots(&manifest, &mounts, &observations) {
        eprintln!("{}", unrequested_static_root_line(&prefix, &root));
    }
    emit_report(&report, options.json)?;
    Ok(if report.ok { CLEAN } else { FINDINGS })
}

/// Reads the discovered manifest (design §15.1), naming the fix when absent.
///
/// Discovery is [`crate::config::manifest_path`]'s — the same function `dev`,
/// the boot and `fmt` call — so `check` can never read a different file, and
/// the read is [`blastit_manifest::blastfile::read`]'s, so a FIFO cannot block
/// it. The missing-file sentence is §15.1's one sentence, produced exactly once
/// in [`crate::config::missing_manifest`]; every other refusal keeps the
/// reader's `path: reason` shape with the path escaped.
fn read_manifest(path: &Path) -> Result<String, String> {
    crate::config::read_manifest(path).map_err(|error| error.sentence(path))
}

/// The platform environment the manifest's inputs come from.
fn platform_env(manifest: &Manifest) -> Result<BTreeMap<String, String>, String> {
    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    blastit_env::platform_env_from_os(manifest, &vars).map_err(|errors| errors.to_string())
}

/// The secret-free application environment, rendered against `socket_dir`.
fn prepare_env(
    manifest: &Manifest,
    platform: &BTreeMap<String, String>,
    socket_dir: &str,
) -> Result<blastit_env::Prepared, String> {
    blastit_env::build_with_socket_dir(manifest, platform, None, socket_dir)
        .map_err(|errors| errors.to_string())
}
