//! `blastit check`: audit a pytest suite against a `Blastfile` (R3..R5, R30).
//!
//! The command is a transaction with one place where it can fail and one place
//! where it writes:
//!
//! 1. **Before anything runs**, the CLI, the configuration, `./Blastfile`
//!    and every `[proxy.*]` input are validated. Any problem exits **`2`** with
//!    **nothing on stdout** — under `--json` stdout is reserved for exactly one
//!    document.
//! 2. **`blastit-dbproxy`** is started in [`ListenerMode::DynamicCheck`] on one
//!    wildcard socket per proxy under `<runtime_dir>/pg`, so it never refuses a
//!    statement a missing grant would have refused (`R26`), and it writes every
//!    statement to an observation pipe.
//! 3. **`pytest -q -p no:blastit -p blastit._check_pytest`** runs unprivileged
//!    with the secret-free `swapped_env` whose `postgresql` outputs point at
//!    that socket directory. `-p no:blastit` disables the `0.1.0` `pytest11`
//!    entry point, so a project that still carries `blastit_manifest` in its
//!    `pyproject.toml` is audited by this gate and nothing else (`R30`,
//!    `ADR-0004`).
//! 4. **The audit** is [`crate::audit_v1`]'s: routing, effective grants, the
//!    allowlist and the human formatter are all its, so this module never
//!    re-implements one of them.
//! 5. **`--learn`** proposes routes and exact `queries` and writes them only
//!    when the suite passed, a request was observed, nothing was refused, this
//!    is not a dry run, and there is something to write (`R29`).
//!
//! # Why the gate reports unrouted requests
//!
//! The ID's route is not known to the Python gate, and deciding it there would
//! mean re-implementing [`blastit_manifest::RoutingTable`]'s regex semantics in
//! Python — exactly the thing this command exists to avoid. The gate therefore
//! pushes each request as [`ObsTarget::Unrouted`] (the control protocol's
//! `push_unrouted`), and this module resolves the request against blastit's own
//! table afterwards (the private `resolve_targets`). A request that matches no
//! route stays
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
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use blastit_dbproxy::{
    DbproxyConfig, ListenerMode, ListenerSpec, ObsTarget, ProxySpec, RoutePolicySpec,
    WILDCARD_ROUTE, WireSqlObs, write_config,
};
use blastit_events::Component;
use blastit_manifest::{Manifest, RoutingTable};
use serde_json::json;

use crate::audit_v1::{self, CheckObsV1, ObsError, Report};
use crate::cli::CheckOptions;
use crate::config::Config;
use crate::host::Host;
use crate::logs::{Colour, Emitter, Pipe};
#[cfg(feature = "php")]
use crate::stage0::php_boot_gate;
use crate::stage0::{is_static_only, static_app_refusal, static_root_permission};
#[cfg(feature = "php")]
use blastit_view_php::discovery::{PhpBoot, PhpShimScope};

/// The process exited cleanly: pytest passed and no finding fails the run.
const CLEAN: u8 = 0;

/// pytest passed, but the manifest is incomplete (`R4`).
const FINDINGS: u8 = 1;

/// Blastit could not start: CLI, config, manifest, missing input, missing
/// pytest, or an observation file that does not parse (`R4`).
const START_FAILURE: u8 = 2;

/// pytest itself failed, exit `5` included (`R4`).
const SUITE_FAILURE: u8 = 3;

/// How long dbproxy may take to report that its listeners are ready.
const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// How long dbproxy gets to leave after pytest, before it is killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

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
            emit_report(&audit_v1::Report::all_static(&manifest), true)?;
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

    let report = audit_v1::evaluate(&manifest, &observations).map_err(|error| error.to_string())?;
    // `R14`: a declared static root the suite never requested is a line on
    // stderr — never a finding, never `ok`, never the exit code. It is the
    // scheduler that answers those files, so the observations cannot see them.
    let mounts = blastit_manifest::MountTable::new(&manifest);
    for (prefix, root) in audit_v1::unrequested_static_roots(&manifest, &mounts, &observations) {
        eprintln!("{}", audit_v1::unrequested_static_root_line(&prefix, &root));
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

/// Refuses to start a suite whose interpreter cannot import pytest (`R4`).
///
/// A missing pytest is exit `2` ("Blastit could not start"), not `3`: nothing
/// ran, so there is no suite failure to report.
fn require_pytest(python: &Path, prepared: &blastit_env::Prepared) -> Result<(), String> {
    if imports(python, prepared, "pytest")? {
        return Ok(());
    }
    Err(format!(
        "{}: pytest is not installed in the interpreter blastit runs on; \
         install pytest: uv add --dev pytest",
        python.display()
    ))
}

/// Whether `module` imports in the suite's own secret-free environment.
///
/// The child is hermetic (`env_clear` plus [`Prepared::app_env`]) for the same
/// reason the suite is: a probe must not see a credential either. ImportError
/// is exit `1`, which is "absent"; anything else means the interpreter itself
/// could not run and is an error of its own.
fn imports(python: &Path, prepared: &blastit_env::Prepared, module: &str) -> Result<bool, String> {
    let mut command = Command::new(python);
    command
        .arg("-c")
        .arg(format!("import {module}"))
        .env_clear()
        .envs(prepared.app_env())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command
        .status()
        .map_err(|error| format!("{}: {error}", python.display()))?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        Some(code) => Err(format!(
            "{}: the check for {module} exited {code}, so the interpreter cannot run",
            python.display()
        )),
        None => Err(format!(
            "{}: the check for {module} was killed by a signal",
            python.display()
        )),
    }
}

/// Inputs needed to prepare the one-shot `pytest-django` handshake (`R30`).
struct DjangoPreflight<'a> {
    manifest: &'a Manifest,
    prepared: &'a blastit_env::Prepared,
    platform: &'a BTreeMap<String, String>,
    socket_dir: &'a str,
}

/// Writes the one-shot Django handshake when this is a `pytest-django` project
/// (`R30`, `ADR-0004`).
///
/// A `pytest-django` suite has to create its `test_*` database on the real
/// server before it can run, and every query after that must go through
/// dbproxy against that same test database. The real proxy inputs therefore
/// travel in a `0600` file that the Python gate reads and unlinks *before*
/// Django builds its settings; the suite's own environment keeps the
/// secret-free rewrite and the gate sends `{"op":"set_db"}` once the database
/// exists.
///
/// A project counts as Django when `pytest-django` imports **and** it names its
/// settings (`DJANGO_SETTINGS_MODULE`) or carries a `manage.py`: pytest-django
/// cannot run a suite without one of those, and a project that merely has the
/// package installed must keep the non-Django environment.
///
/// When the handshake cannot be prepared, this is a start failure (exit `2`):
/// running anyway would let Django's migrations and flushes reach the
/// developer's development database.
fn django_setup(
    preflight: &DjangoPreflight<'_>,
    python: &Path,
    runtime: &Path,
) -> Result<Option<PathBuf>, String> {
    if !django_project(preflight.platform) || !imports(python, preflight.prepared, "pytest_django")?
    {
        return Ok(None);
    }
    let document = django_document(
        preflight.manifest,
        preflight.prepared,
        preflight.platform,
        preflight.socket_dir,
    )?;
    let path = runtime.join("django-setup.json");
    let bytes =
        serde_json::to_vec(&document).map_err(|error| format!("the Django handshake: {error}"))?;
    write_private(&path, &bytes)?;
    Ok(Some(path))
}

/// Whether this directory is a Django project `pytest-django` could set up.
///
/// `pytest-django` refuses to start without settings, so a package that merely
/// happens to be installed in the same environment is not a Django suite.
fn django_project(platform: &BTreeMap<String, String>) -> bool {
    platform.contains_key("DJANGO_SETTINGS_MODULE") || Path::new("manage.py").is_file()
}

/// The JSON the Python gate reads: the real inputs, the swapped ones, the
/// local dbproxy endpoint, and the test database to select (`ADR-0004`).
///
/// `real` never leaves this file; the gate applies it while Django loads its
/// settings and then restores `swapped`, so the credential barrier holds for
/// every test module and test body. Only the first `postgresql` proxy is handed
/// over: the supported Django shape is one proxied database.
fn django_document(
    manifest: &Manifest,
    prepared: &blastit_env::Prepared,
    platform: &BTreeMap<String, String>,
    socket_dir: &str,
) -> Result<serde_json::Value, String> {
    let Some((name, upstream)) = prepared.upstreams().first() else {
        return Err(
            "pytest-django is installed but the Blastfile declares no postgresql proxy, \
             so its test database has nowhere to go; declare a `postgres` store: \
             `postgres $DATABASE_URL` (see docs/frameworks/python/django.md), or remove \
             pytest-django from the environment blastit audits"
                .to_string(),
        );
    };
    let port = blastit_env::postgresql::local_port(0).ok_or_else(|| {
        format!(
            "{}: no local port left for the postgresql proxies",
            name.as_str()
        )
    })?;
    let mut real = BTreeMap::new();
    let mut swapped = BTreeMap::new();
    if let Some(proxy) = manifest.proxies().get(name) {
        for variable in proxy.input().values() {
            if let Some(value) = platform.get(variable) {
                real.insert(variable.clone(), value.clone());
            }
        }
    }
    if let Some(outputs) = prepared.outputs().get(name) {
        for variable in outputs {
            if let Some(value) = prepared.app_env().get(variable) {
                swapped.insert(variable.clone(), value.clone());
            }
            if let Some(value) = platform.get(variable) {
                real.entry(variable.clone())
                    .or_insert_with(|| value.clone());
            }
        }
    }
    if real.is_empty() || swapped.is_empty() {
        return Err(no_input_variables(name.as_str()));
    }
    Ok(json!({
        "proxy": name.as_str(),
        "database": format!("test_{}", upstream.database()),
        "socket_dir": socket_dir,
        "port": port,
        "user": blastit_env::postgresql::LOCAL_USER,
        "real": real,
        "swapped": swapped,
    }))
}

/// The refusal a store that hands the Django handshake no variable is printed
/// with: the block to write, as a `Blastfile` writes it, and never the retired
/// per-route table.
///
/// `real` and `swapped` are both filled for every store a `postgres` block can
/// declare, so this is the guard against a model that changes that — one
/// sentence, asserted on its own rather than through a manifest that cannot
/// reach it.
fn no_input_variables(store: &str) -> String {
    format!(
        "pytest-django is installed but proxy `{store}` has no input variables to hand \
         over, so its test database cannot be created; name them in that store's block: \
         `postgres {{ user $DB_USER; password $DB_PASSWORD }}` and export them before \
         running blastit check"
    )
}

/// Creates `path` `0600` and writes `bytes` into it, owned by this uid.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| {
            format!(
                "{}: {error} (the Django handshake needs a writable TMPDIR)",
                path.display()
            )
        })?;
    // The umask can only clear bits; force exactly what the gate requires.
    rustix::fs::fchmod(&file, rustix::fs::Mode::from_bits_truncate(0o600))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.flush()
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// One bound wildcard socket per declared `postgresql` proxy.
///
/// In `DynamicCheck` the listener is shared by every route: which route a
/// statement belongs to comes from the control channel, and the route policies
/// below are what the listener evaluates against (`§4.2 D.3`).
fn bind_listeners(
    prepared: &blastit_env::Prepared,
    socket_dir: &Path,
) -> Result<Vec<(UnixListener, ListenerSpec)>, String> {
    let uid = rustix::process::geteuid().as_raw();
    let mut listeners = Vec::new();
    for (index, (name, _upstream)) in prepared.upstreams().iter().enumerate() {
        let Some(port) = blastit_env::postgresql::local_port(index) else {
            return Err(format!(
                "{}: no local port left for the postgresql proxies",
                name.as_str()
            ));
        };
        let path = socket_dir.join(format!(".s.PGSQL.{port}"));
        let listener =
            UnixListener::bind(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        // `0600` and the caller's own uid: the suite, running as this user, is
        // the only client the check sockets accept (`R30`).
        rustix::fs::chmod(&path, rustix::fs::Mode::from_bits_truncate(0o600))
            .map_err(|error| format!("{}: {error}", path.display()))?;
        listeners.push((
            listener,
            ListenerSpec {
                fd: 0,
                route: WILDCARD_ROUTE.to_string(),
                proxy: name.as_str().to_string(),
                uid,
                queries: Vec::new(),
                transaction_control: false,
                unrestricted: false,
                tables: blastit_sqlpolicy::TableGrants::default(),
            },
        ));
    }
    Ok(listeners)
}

/// The route policies of every `(route, proxy)` pair (`§4.2 D.3`).
fn route_policies(manifest: &Manifest) -> Vec<RoutePolicySpec> {
    let mut policies = Vec::new();
    for index in 0..manifest.route_count() {
        let Some(route) = manifest.route(index) else {
            continue;
        };
        for (proxy, grant) in manifest.effective_grants(index) {
            let Some(pg) = grant.as_postgresql() else {
                continue;
            };
            policies.push(RoutePolicySpec {
                route: route.name().to_string(),
                proxy: proxy.as_str().to_string(),
                queries: pg.queries().to_vec(),
                transaction_control: pg.transaction_control(),
                unrestricted: pg.is_unrestricted(),
                tables: pg.tables().clone(),
            });
        }
    }
    policies
}

/// The inherited descriptors dbproxy adopts after its `execve`.
struct Inherited {
    obs: OwnedFd,
    ctl: OwnedFd,
    ready: OwnedFd,
}

/// The one-shot Django handshake file, removed however the run ends.
///
/// The suite unlinks it as soon as it has read it; this is the backstop for the
/// paths where the suite never started (a spawn failure, a dead dbproxy), so a
/// crash cannot leave the real proxy inputs in `/tmp` after `blastit check`
/// returns (`R30`).
struct Handshake {
    path: Option<PathBuf>,
}

impl Handshake {
    fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

impl Drop for Handshake {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The running `blastit-dbproxy` and the descriptors the parent still owns.
struct Dbproxy {
    child: Child,
}

impl Dbproxy {
    /// Forks and `execve`s `blastit-dbproxy` with an empty environment.
    ///
    /// Its events go to a pipe of their own rather than to stderr: `pytest -q`
    /// is drawing its progress line there, and a JSON line written into it
    /// corrupts the row an operator reads (A10.3). The returned reader is the
    /// pipe's read end, which the caller hands to an [`EventDrain`] for the
    /// whole of the suite. It stays a [`std::io::PipeReader`]: a pipe is not a
    /// socket, and reading it through a `UnixStream` fails with `ENOTSOCK`.
    fn start(
        host: &Host,
        manifest: &Manifest,
        prepared: &blastit_env::Prepared,
        mut listeners: Vec<(UnixListener, ListenerSpec)>,
        inherited: Inherited,
        ready_read: OwnedFd,
    ) -> Result<(Self, std::io::PipeReader), String> {
        for (listener, spec) in &mut listeners {
            spec.fd = listener.as_raw_fd();
        }
        let uid = rustix::process::geteuid().as_raw();
        let proxies: Vec<ProxySpec> = crate::dbproxy_wiring::proxy_specs(prepared, |_| 4);
        let mut document = DbproxyConfig::enforcing(
            proxies,
            listeners.iter().map(|(_, spec)| spec.clone()).collect(),
        );
        document.mode = ListenerMode::DynamicCheck;
        document.allow_root_listener = uid == 0;
        document.route_policies = route_policies(manifest);
        document.obs_fd = Some(inherited.obs.as_raw_fd());
        document.check_ctl_fd = Some(inherited.ctl.as_raw_fd());
        document
            .validate()
            .map_err(|error| format!("the dbproxy configuration: {error}"))?;

        let (secrets_read, mut secrets_write) =
            std::io::pipe().map_err(|error| format!("the dbproxy secrets pipe: {error}"))?;
        let (events_read, events_write) =
            std::io::pipe().map_err(|error| format!("the dbproxy events pipe: {error}"))?;

        // Every descriptor the child must inherit: the configuration pipe, the
        // two channels, the ready pipe, the events pipe, and one per listener.
        let mut inherited_fds = vec![
            secrets_read.as_raw_fd(),
            events_write.as_raw_fd(),
            inherited.obs.as_raw_fd(),
            inherited.ctl.as_raw_fd(),
            inherited.ready.as_raw_fd(),
        ];
        inherited_fds.extend(listeners.iter().map(|(listener, _)| listener.as_raw_fd()));
        for fd in &inherited_fds {
            clear_cloexec(*fd)?;
        }

        let program = dbproxy_program(host);
        let mut command = Command::new(&program);
        command.args(host.dbproxy().prefix.iter());
        command.arg("--role=dbproxy");
        command.arg(format!("--secrets-fd={}", secrets_read.as_raw_fd()));
        command.arg(format!("--events-fd={}", events_write.as_raw_fd()));
        command.arg(format!("--ready-fd={}", inherited.ready.as_raw_fd()));
        command.arg("--seccomp=1");
        command.env_clear();
        command.stdin(Stdio::null());
        command.stdout(stdio_stderr()?);
        command.stderr(stdio_stderr()?);
        // A coverage build boots dbproxy under the same profile file, so the
        // role's coverage is written when it exits (`-C instrument-coverage`).
        crate::coverage::inherit(&mut command);
        let child = command.spawn().map_err(|error| {
            format!(
                "{}: {error} (the check needs the `blastit-dbproxy` binary this \
                 installation ships)",
                program.display()
            )
        })?;
        // The parent's copies of the child's ends are closed right after the
        // `execve`: dbproxy owns them from here.
        drop(events_write);
        drop(inherited.obs);
        drop(inherited.ctl);
        drop(listeners);
        write_config(&document, &mut secrets_write)
            .map_err(|error| format!("the dbproxy configuration: {error}"))?;
        drop(secrets_write);
        drop(secrets_read);

        let mut ready = Ready::new(ready_read);
        if let Err(reason) = ready.wait() {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(reason);
        }
        Ok((Self { child }, events_read))
    }

    /// Stops dbproxy: `SIGTERM`, a short grace, then `SIGKILL`.
    fn stop(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        let pid = i32::try_from(self.child.id()).ok();
        if let Some(pid) = pid.and_then(rustix::process::Pid::from_raw) {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        }
        let deadline = std::time::Instant::now() + SHUTDOWN_GRACE;
        while std::time::Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Dbproxy {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Waits for dbproxy's ready byte.
struct Ready {
    stream: UnixStream,
}

impl Ready {
    fn new(fd: OwnedFd) -> Self {
        Self {
            stream: UnixStream::from(fd),
        }
    }

    fn wait(&mut self) -> Result<(), String> {
        self.stream
            .set_read_timeout(Some(READY_TIMEOUT))
            .map_err(|error| format!("the dbproxy ready pipe: {error}"))?;
        let mut byte = [0u8; 1];
        match self.stream.read(&mut byte) {
            Ok(0) => Err("dbproxy exited before it was ready".to_string()),
            Ok(_) => Ok(()),
            Err(error) => Err(format!(
                "dbproxy did not report ready: {error} (its own diagnostics are on stderr)"
            )),
        }
    }
}

/// Drains dbproxy's observation pipe on its own thread (`LESSONS.md`).
///
/// The writer blocks once the socketpair's buffer is full, so the reader has to
/// run for the whole of the suite rather than after it.
struct ObsReader {
    handle: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl ObsReader {
    fn start(read: OwnedFd) -> Self {
        let mut stream = UnixStream::from(read);
        let handle = std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buffer = [0u8; 16 * 1024];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => out.extend_from_slice(&buffer[..read]),
                    Err(_) => break,
                }
            }
            out
        });
        Self {
            handle: Some(handle),
        }
    }

    /// Waits for the pipe's end and returns what it held.
    fn stop(mut self) -> String {
        let Some(handle) = self.handle.take() else {
            return String::new();
        };
        let bytes = handle.join().unwrap_or_default();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Drains dbproxy's *events* pipe, so its lines never share pytest's row
/// (A10.3).
///
/// A `pg.refused` is one JSON line, and dbproxy writes it synchronously: pointed
/// at fd 2 it lands in the middle of the `-q` progress line pytest is drawing on
/// the same terminal. The pipe is drained here for the whole of the suite (the
/// writer blocks otherwise), and every line is forwarded through the run's own
/// [`Emitter`] once the suite is over — the same component restamp, the same
/// level filter (`BLASTIT_LOG_LEVEL=error` silences a `warn`) and the same
/// pretty or JSON shape as any other forwarded pipe.
///
/// [`Drop`] is the guarantee that "once the suite is over" is true on *every*
/// path out of [`execute`], including the `?` that abandons the run: a drain
/// that is only stopped by hand would take its thread and its bytes with it.
struct EventDrain {
    handle: Option<std::thread::JoinHandle<Vec<u8>>>,
    emitter: Emitter,
}

impl EventDrain {
    fn start(read: std::io::PipeReader, emitter: Emitter) -> Self {
        let mut stream = read;
        let handle = std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buffer = [0u8; 8 * 1024];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => out.extend_from_slice(&buffer[..read]),
                    Err(_) => break,
                }
            }
            out
        });
        Self {
            handle: Some(handle),
            emitter,
        }
    }

    /// Waits for the pipe's end and forwards every line. Idempotent, so an
    /// explicit stop and [`Drop`] never print a line twice.
    fn stop(mut self) {
        self.drain();
    }

    fn drain(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        let bytes = handle.join().unwrap_or_default();
        let pipe = Pipe::Events {
            component: Component::Dbproxy,
        };
        for line in String::from_utf8_lossy(&bytes).lines() {
            self.emitter.forward(&pipe, line);
        }
    }
}

impl Drop for EventDrain {
    fn drop(&mut self) {
        self.drain();
    }
}

/// Everything one `pytest` spawn reads (`MB.40.8`).
///
/// A struct rather than eight positional arguments: the output-stream flag, the
/// optional control descriptor and the optional paths read as one document, and
/// a caller cannot swap two of them by accident.
struct PytestSpawn<'a> {
    options: &'a CheckOptions,
    prepared: &'a blastit_env::Prepared,
    python: &'a Path,
    http_path: &'a Path,
    ctl_fd: Option<RawFd>,
    json: bool,
    django: Option<&'a Path>,
}

/// Runs the suite, mapping a spawn failure to exit `2` (`R4`).
fn run_pytest(spawn: PytestSpawn<'_>) -> Result<i32, String> {
    let PytestSpawn {
        options,
        prepared,
        python,
        http_path,
        ctl_fd,
        json,
        django,
    } = spawn;
    let mut command = Command::new(python);
    command.arg("-m").arg("pytest").arg("-q");
    // R30: `-p no:blastit` disables the installed `0.1.0` entry point, and the
    // gate is loaded by module name instead.
    command.arg("-p").arg("no:blastit");
    command.arg("-p").arg("blastit._check_pytest");
    command.args(&options.pytest_args);
    command.env_clear();
    for (name, value) in prepared.app_env() {
        command.env(name, value);
    }
    // `MB.40.8`: the bundle `make install` stages carries the gate itself at
    // `<exe_dir>/../python`, so `-p blastit._check_pytest` imports even when
    // `blastit` was never installed into the application's own venv. An existing
    // `PYTHONPATH` keeps its entries, after the bundle's.
    if let Some(directory) = bundled_python() {
        let existing = prepared
            .app_env()
            .get("PYTHONPATH")
            .filter(|value| !value.is_empty())
            .cloned()
            .unwrap_or_default();
        let joined = if existing.is_empty() {
            directory
        } else {
            format!("{directory}:{existing}")
        };
        command.env("PYTHONPATH", joined);
    }
    command.env(HTTP_OBS, http_path);
    // A store-less app runs no dbproxy, so there is no channel to push a target
    // into and no `BLASTIT_CHECK_CTL_FD` to name: the gate records the request
    // and pushes nothing (`_check_pytest._send`).
    if let Some(fd) = ctl_fd {
        command.env(CTL_FD, fd.to_string());
    }
    if let Some(path) = django {
        // R30/ADR-0004: the only place the real proxy inputs exist in this
        // process tree after the rewrite, `0600`, and gone as soon as the gate
        // has read it.
        command.env(DJANGO_SETUP, path);
    }
    crate::coverage::inherit(&mut command);
    command.stdin(Stdio::null());
    if json {
        // R5: the child's fd 1 *is* our stderr, so no `print()` can reach the
        // document's stream.
        command.stdout(stdio_stderr()?);
    } else {
        command.stdout(Stdio::inherit());
    }
    command.stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|error| {
        format!(
            "{}: {error} (install pytest in the environment blastit runs on: \
             `uv add --dev pytest`)",
            python.display()
        )
    })?;
    let status = child
        .wait()
        .map_err(|error| format!("waiting for pytest: {error}"))?;
    Ok(status.code().unwrap_or(-1))
}

/// The interpreter `pytest` runs on (`MB.40.8`, `MB.42.2.2.2`).
///
/// A wheel's console script hands the command line to the universal binary, so
/// there is no interpreter in this process to inherit: it records the one it ran
/// on in `BLASTIT_WHEEL_PYTHON` (dropped from the application's environment by
/// `blastit-env` like every other `BLASTIT_*` variable) and that is the project's
/// own environment. Otherwise the search is what a developer's shell answers:
/// `BLASTIT_PYTHON`, the activated `$VIRTUAL_ENV`, the project's `.venv`,
/// `BLASTIT_WHEEL_PYTHON`, `PATH` — so an activated venv always wins.
fn python_program(manifest_dir: &Path) -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("BLASTIT_PYTHON") {
        return Ok(PathBuf::from(explicit));
    }
    if let Some(found) = std::env::var_os("VIRTUAL_ENV")
        .map(PathBuf::from)
        .filter(|directory| !directory.as_os_str().is_empty())
        .and_then(|directory| venv_python(&directory))
    {
        return Ok(found);
    }
    if let Some(found) = venv_python(&manifest_dir.join(".venv")) {
        return Ok(found);
    }
    if let Some(found) = std::env::var_os("BLASTIT_WHEEL_PYTHON")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
    {
        return Ok(found);
    }
    if let Some(found) = path_python() {
        return Ok(found);
    }
    Err(
        "no interpreter to run pytest on: set BLASTIT_PYTHON to the `python3` \
         that has pytest installed"
            .to_string(),
    )
}

/// `<venv>/bin/python3` (or `bin/python`), when it exists.
fn venv_python(venv: &Path) -> Option<PathBuf> {
    for name in ["python3", "python"] {
        let candidate = venv.join("bin").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// The first `python3` (or `python`) on `PATH`.
fn path_python() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in path.to_string_lossy().split(':') {
        if directory.is_empty() {
            continue;
        }
        for name in ["python3", "python"] {
            let candidate = Path::new(directory).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// `<exe_dir>/../python`, or the wheel root two levels up, when it holds the
/// staged `blastit` gate (`MB.40.8`, `MB.42.2.2.2`, `MB.42.11`, `MB.42.3.3`).
///
/// Two layouts, one marker pair. `make install` puts the binary at
/// `<prefix>/share/blastit/lib/blastit` and the package at
/// `<prefix>/share/blastit/python/blastit`, so `<exe_dir>/../python` is the
/// package root. The published `py3-none-*` wheel (`MB.42.3.3`) puts the binary
/// at `<site-packages>/blastit/_bin/blastit` with the package's own modules
/// beside it, so `<exe_dir>/../..` is `<site-packages>` — the root that holds
/// `blastit/`. (`MB.42.11`'s `../../..` resolved one level *above*
/// `site-packages`, which can never hold the gate: it was a dead fallback, and
/// `MB.42.3.3` replaced it with the root the repacked wheel actually has.)
///
/// Both markers have to be there — `_check_pytest.py` is the gate `-p` imports
/// and `__main__.py` is the package's entry point, and every bundle ships both —
/// so a `cargo` build tree whose `target/<profile>` sits beside the repository's
/// own `python/` can never be mistaken for one. `_bin` is deliberately not a
/// marker: it is a *directory name*, and the wheel's root is two levels above it.
fn bundled_python() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    bundle_root(exe.parent()?)
}

/// The `blastit` package root beside `exe_dir` that carries the staged gate.
fn bundle_root(exe_dir: &Path) -> Option<String> {
    let roots = [exe_dir.join("../python"), exe_dir.join("../..")];
    roots
        .into_iter()
        .find(|root| {
            let package = root.join("blastit");
            package.join("_check_pytest.py").is_file() && package.join("__main__.py").is_file()
        })
        .map(|root| root.to_string_lossy().into_owned())
}

/// The `blastit-dbproxy` program this installation runs.
fn dbproxy_program(host: &Host) -> PathBuf {
    let program = &host.dbproxy().program;
    if program.is_file() {
        return program.clone();
    }
    std::env::current_exe()
        .map(|exe| exe.with_file_name("blastit-dbproxy"))
        .unwrap_or_else(|_| program.clone())
}

/// Turns every observed request into the target blastit's own table resolves.
///
/// The gate pushes `Unrouted { method, path }` for every request, because the
/// route is blastit's decision and re-implementing it in Python would be a
/// second, divergent router. Here the same [`RoutingTable`] `evaluate` uses
/// names the route; a path no route matches stays unrouted (`R32`).
fn resolve_targets(table: &RoutingTable, manifest: &Manifest, observations: &mut CheckObsV1) {
    let resolved: Vec<WireSqlObs> = observations
        .sql()
        .iter()
        .map(|statement| {
            let ObsTarget::Unrouted { method, path } = &statement.target else {
                return statement.clone();
            };
            let Some(matched) = table.route(method, path) else {
                return statement.clone();
            };
            let Some(route) = manifest.route(matched.route) else {
                return statement.clone();
            };
            WireSqlObs {
                target: ObsTarget::Routed {
                    route: route.name().to_string(),
                },
                ..statement.clone()
            }
        })
        .collect();
    *observations = match CheckObsV1::new(
        observations.schema_version(),
        observations.http().to_vec(),
        resolved,
    ) {
        Ok(updated) => updated,
        // Unreachable: the version was the accepted one a moment ago.
        Err(_) => return,
    };
}

/// Writes the report, human or one JSON document (`§1.3`, `R5`).
fn emit_report(report: &Report, json: bool) -> Result<(), String> {
    if json {
        let document = report.to_json().map_err(|error| error.to_string())?;
        println!("{document}");
    } else {
        print!("{}", audit_v1::human(report));
    }
    Ok(())
}

/// `check --learn`: the proposal, its hints, and the one write (`R29`).
fn learn(
    config: &Config,
    source: &str,
    manifest: &Manifest,
    observations: &CheckObsV1,
    options: &CheckOptions,
) -> u8 {
    let plan = match audit_v1::plan_learn(manifest, observations) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("blastit check: {error}");
            return START_FAILURE;
        }
    };
    plan.emit_hints();
    for refusal in &plan.refused {
        eprintln!("learn: refused {refusal}");
    }

    let mut wrote = false;
    let mut failure = None;
    // `--dry-run` writes nothing, so the proposal is the output: the very
    // string the write below hands to `write_atomic` (one editor, no second
    // renderer), and the same refusal sentence a real run reports
    // (`docs/ci.md`: "print the proposal, write nothing").
    let mut proposal = None;
    if options.dry_run && plan.would_write(false) {
        match plan.proposal(source) {
            Ok(text) => proposal = Some(text),
            Err(error) => failure = Some(format!("{}: {error}", config.manifest.display())),
        }
    }
    if plan.would_write(options.dry_run) {
        match plan.proposal(source) {
            Ok(edited) if edited != source => {
                match blastit_manifest::edit::write_atomic(&config.manifest, &edited) {
                    Ok(()) => wrote = true,
                    Err(error) => failure = Some(format!("{}: {error}", config.manifest.display())),
                }
            }
            Ok(_) => {}
            Err(error) => failure = Some(format!("{}: {error}", config.manifest.display())),
        }
    }

    let clean = plan.requests > 0 && plan.refused.is_empty() && failure.is_none();
    if options.json {
        // §1.3: the `--learn --json` document, intentionally without
        // `schema_version`, and never `wrote: true` for a write that failed.
        println!(
            "{}",
            json!({
                "proposed": {"routes": plan.routes(), "queries": plan.queries()},
                "wrote": wrote,
            })
        );
        if clean {
            eprintln!("{}", learn_line(&plan, &config.manifest, options.dry_run));
        }
    } else {
        println!("{}", learn_line(&plan, &config.manifest, options.dry_run));
        if let Some(text) = proposal {
            println!("learn: proposed Blastfile (nothing written):");
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
        }
    }
    if let Some(message) = failure {
        eprintln!("learn: {message}");
    }
    if clean { CLEAN } else { FINDINGS }
}

/// The §1.3 `learn:` line, with the absolute manifest path.
///
/// The two counts are written in their own number — `1 route`, `2 routes`,
/// `1 query`, `3 queries` — because this is a sentence a developer reads. The
/// `--json` document keeps `{"proposed": {"routes": N, "queries": M}}`: a
/// machine key never pluralises.
fn learn_line(plan: &audit_v1::LearnPlan, manifest: &Path, dry_run: bool) -> String {
    let path = std::path::absolute(manifest)
        .unwrap_or_else(|_| manifest.to_path_buf())
        .display()
        .to_string();
    let routes = audit_v1::plural(plan.routes(), "route");
    let queries = audit_v1::plural(plan.queries(), "query");
    if dry_run {
        format!(
            "learn: {} {routes}, {} {queries} proposed (dry-run) → {path}",
            plan.routes(),
            plan.queries()
        )
    } else {
        format!(
            "learn: {} {routes}, {} {queries} added → {path}",
            plan.routes(),
            plan.queries()
        )
    }
}

/// Names the file an observation line came from (`T2-M11`).
fn observation_error(runtime: &CheckRoot, error: ObsError) -> String {
    let http = runtime.path().join("http_obs.jsonl");
    match error {
        ObsError::Malformed { file, line, reason } => format!("{file}: line {line}: {reason}"),
        other => {
            let hint = if http.is_file() {
                format!(" (the file this run's gate wrote is {})", http.display())
            } else {
                String::new()
            };
            format!("{other}{hint}")
        }
    }
}

/// This run's private `0700` runtime root, removed when it is dropped.
struct CheckRoot {
    path: PathBuf,
}

impl CheckRoot {
    /// Creates `$TMPDIR/blastit-check-<uid>-<pid>-<nonce>`, `0700`, owned by
    /// this uid.
    fn create() -> Result<Self, String> {
        let uid = rustix::process::geteuid().as_raw();
        let base = std::env::temp_dir();
        let mut last = None;
        for attempt in 0..16u32 {
            let name = format!(
                "blastit-check-{uid}-{}-{}-{attempt}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.subsec_nanos())
                    .unwrap_or(0)
            );
            let path = base.join(name);
            match rustix::fs::mkdir(&path, rustix::fs::Mode::from_bits_truncate(0o700)) {
                Ok(()) => {
                    verify_private(&path, uid)?;
                    return Ok(Self { path });
                }
                Err(error) => last = Some(error),
            }
        }
        Err(format!(
            "{}: could not create a private runtime directory: {}",
            base.display(),
            last.map(|error| error.to_string()).unwrap_or_default()
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Creates and returns `<root>/pg`, the socket directory `swapped_env`
    /// renders.
    fn postgres_dir(&self) -> Result<PathBuf, String> {
        let dir = self.path.join("pg");
        rustix::fs::mkdir(&dir, rustix::fs::Mode::from_bits_truncate(0o700))
            .map_err(|error| format!("{}: {error}", dir.display()))?;
        verify_private(&dir, rustix::process::geteuid().as_raw())?;
        Ok(dir)
    }
}

impl Drop for CheckRoot {
    fn drop(&mut self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        if !metadata.file_type().is_dir() || metadata.uid() != rustix::process::geteuid().as_raw() {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Checks that `path` is a `0700` directory owned by `uid`.
fn verify_private(path: &Path, uid: u32) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.file_type().is_dir() || metadata.uid() != uid {
        return Err(format!(
            "{}: not a directory owned by uid {uid}",
            path.display()
        ));
    }
    if metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(format!("{}: is not 0700", path.display()));
    }
    Ok(())
}

/// A socketpair, with a one-line error naming the channel.
fn socketpair(channel: &str) -> Result<(OwnedFd, OwnedFd), String> {
    let (a, b) = UnixStream::pair().map_err(|error| format!("{channel}: {error}"))?;
    Ok((OwnedFd::from(a), OwnedFd::from(b)))
}

/// Clears `FD_CLOEXEC` so the child inherits `fd` across its `execve`.
fn clear_cloexec(fd: RawFd) -> Result<(), String> {
    // SAFETY: `fd` is one of this run's own descriptors and this is the only
    // place that changes its flags.
    let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    rustix::io::fcntl_setfd(borrowed, rustix::io::FdFlags::empty())
        .map_err(|error| format!("descriptor {fd}: {error}"))
}

/// A duplicate of stderr, for a child whose stdout must not be stdout.
fn stdio_stderr() -> Result<Stdio, String> {
    let duplicated = rustix::io::dup(std::io::stderr())
        .map_err(|error| format!("duplicating stderr: {error}"))?;
    Ok(Stdio::from(duplicated))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::audit_v1::{HttpObsLine, SCHEMA_VERSION};
    use blastit_ingress::BodySample;

    const MANIFEST: &str = r#"
postgres $DATABASE_URL

view catalog regex '/catalog/.*' {
    postgres {
        query 'SELECT 1'
    }
}

view fallback regex '.*' {}
"#;

    fn statement(target: ObsTarget, sql: &str) -> WireSqlObs {
        WireSqlObs {
            target,
            proxy: "db".to_string(),
            kind: blastit_events::SqlKind::Query,
            sql: sql.to_string(),
            is_tx: false,
            allowed: false,
            verdict: blastit_sqlpolicy::LearnVerdict::Query {
                normalized: sql.to_string(),
            },
        }
    }

    fn observations(sql: Vec<WireSqlObs>) -> CheckObsV1 {
        CheckObsV1::new(
            SCHEMA_VERSION,
            vec![HttpObsLine {
                method: "GET".to_string(),
                path: "/catalog/1".to_string(),
                content_type: None,
                body_len: 0,
                sample: None,
            }],
            sql,
        )
        .expect("observations")
    }

    #[test]
    fn an_observed_request_is_resolved_by_blastits_own_table() {
        let manifest = audit_v1::parse_manifest(MANIFEST).expect("manifest");
        let table = RoutingTable::new(&manifest).expect("table");
        // Adversarial: the fallback route matches anything, and it is the
        // second route, so a naive "any match" would name it.
        let mut observed = observations(vec![statement(
            ObsTarget::Unrouted {
                method: "GET".to_string(),
                path: "/catalog/1".to_string(),
            },
            "SELECT 1",
        )]);
        resolve_targets(&table, &manifest, &mut observed);
        assert_eq!(
            observed.sql()[0].target,
            ObsTarget::Routed {
                route: "catalog".to_string()
            }
        );

        // Denied twin: a path the table routes to the catch-all is a *routed*
        // request too — it is not an `unrouted_request`, only a shallower route.
        let mut elsewhere = observations(vec![statement(
            ObsTarget::Unrouted {
                method: "GET".to_string(),
                path: "/robots.txt".to_string(),
            },
            "SELECT 1",
        )]);
        resolve_targets(&table, &manifest, &mut elsewhere);
        assert_eq!(
            elsewhere.sql()[0].target,
            ObsTarget::Routed {
                route: "fallback".to_string()
            }
        );

        // Denied twin: no route at all stays unrouted, so `--learn` can
        // propose one and the audit can report it.
        let manifest = audit_v1::parse_manifest(
            "postgres $DATABASE_URL\n\nview catalog regex '/catalog/.*' {}\n",
        )
        .expect("manifest");
        let table = RoutingTable::new(&manifest).expect("table");
        let mut unknown = observations(vec![statement(
            ObsTarget::Unrouted {
                method: "GET".to_string(),
                path: "/robots.txt".to_string(),
            },
            "SELECT 1",
        )]);
        resolve_targets(&table, &manifest, &mut unknown);
        assert_eq!(
            unknown.sql()[0].target,
            ObsTarget::Unrouted {
                method: "GET".to_string(),
                path: "/robots.txt".to_string()
            }
        );

        // A statement already attributed is never rewritten.
        let mut routed = observations(vec![statement(
            ObsTarget::Routed {
                route: "catalog".to_string(),
            },
            "SELECT 1",
        )]);
        resolve_targets(&table, &manifest, &mut routed);
        assert_eq!(
            routed.sql()[0].target,
            ObsTarget::Routed {
                route: "catalog".to_string()
            }
        );
    }

    #[test]
    fn a_resolved_statement_is_granted_where_an_unrouted_one_would_be_missing() {
        // The whole point of resolving: the same statement is a `missing_grant`
        // under `Unrouted` in the raw stream and not one once resolved.
        let manifest = audit_v1::parse_manifest(
            "postgres $DATABASE_URL\n\nview catalog regex '/catalog/.*' {\n    postgres {\n        query 'SELECT 1'\n    }\n}\n",
        )
        .expect("manifest");
        let table = RoutingTable::new(&manifest).expect("table");
        let mut observed = observations(vec![statement(
            ObsTarget::Unrouted {
                method: "GET".to_string(),
                path: "/catalog/1".to_string(),
            },
            "SELECT 1",
        )]);
        resolve_targets(&table, &manifest, &mut observed);
        let report = audit_v1::evaluate(&manifest, &observed).expect("audit");
        assert!(report.ok, "{:?}", report.findings);
        assert_eq!(report.statements, 1);

        // Denied twin: without the rewrite the grant is reported dead, which is
        // the wrong answer the resolution exists to prevent.
        let raw = observations(vec![statement(
            ObsTarget::Unrouted {
                method: "GET".to_string(),
                path: "/catalog/1".to_string(),
            },
            "SELECT 1",
        )]);
        let report = audit_v1::evaluate(&manifest, &raw).expect("audit");
        assert!(
            report
                .findings
                .iter()
                .any(|finding| format!("{finding:?}").contains("DeadGrant")),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn pytest_exit_codes_map_to_the_documented_ones() {
        // A helper mirroring `execute`'s two branches.
        fn code(pytest: i32) -> u8 {
            if pytest == 0 { 0 } else { SUITE_FAILURE }
        }
        assert_eq!(code(0), CLEAN);
        assert_eq!(code(1), SUITE_FAILURE, "a failing assertion is exit 3");
        assert_eq!(code(5), SUITE_FAILURE, "no tests collected is exit 3");
        assert_eq!(code(2), SUITE_FAILURE, "a pytest usage error is exit 3");
        assert_eq!(code(-1), SUITE_FAILURE, "a signalled child is exit 3");
    }

    #[test]
    fn the_learn_line_names_dry_runs_and_absolute_paths() {
        let mut plan = audit_v1::LearnPlan::default();
        plan.edits
            .views
            .push(blastit_manifest::blastfile::ProposedView {
                name: "orders".to_string(),
                methods: Vec::new(),
                patterns: vec!["/orders/{id:int}".to_string()],
            });
        plan.edits
            .grants
            .push(blastit_manifest::blastfile::ProposedGrant {
                view: "orders".to_string(),
                store: None,
                queries: vec!["SELECT 1".to_string()],
                tables: Default::default(),
                functions: Default::default(),
            });
        // One of each: the sentence says `1 route`, never `1 routes`.
        let line = learn_line(&plan, Path::new("./Blastfile"), false);
        assert!(
            line.starts_with("learn: 1 route, 1 query added → /"),
            "{line}"
        );
        assert!(!line.contains("dry-run"), "{line}");
        let dry = learn_line(&plan, Path::new("./Blastfile"), true);
        assert!(
            dry.starts_with("learn: 1 route, 1 query proposed (dry-run) → /"),
            "{dry}"
        );
        assert!(!dry.contains("added"), "a dry run adds nothing: {dry}");

        // Many of each: each noun is pluralised on its own count. `--json` is
        // untouched — `{"proposed": {"routes": N, "queries": M}}` stays a key.
        let mut many = audit_v1::LearnPlan::default();
        for name in ["orders", "items"] {
            many.edits
                .views
                .push(blastit_manifest::blastfile::ProposedView {
                    name: name.to_string(),
                    methods: Vec::new(),
                    patterns: vec![format!("/{name}/")],
                });
        }
        many.edits
            .grants
            .push(blastit_manifest::blastfile::ProposedGrant {
                view: "orders".to_string(),
                store: None,
                queries: vec![
                    "SELECT 1".to_string(),
                    "SELECT 2".to_string(),
                    "SELECT 3".to_string(),
                ],
                tables: Default::default(),
                functions: Default::default(),
            });
        let many_line = learn_line(&many, Path::new("./Blastfile"), true);
        assert!(
            many_line.starts_with("learn: 2 routes, 3 queries proposed (dry-run) → /"),
            "{many_line}"
        );
        assert!(
            plan.json(true).contains("\"routes\""),
            "{}",
            plan.json(true)
        );
        assert!(
            !plan.json(true).contains("\"route\""),
            "a machine key never pluralises: {}",
            plan.json(true)
        );
    }

    /// A `Blastfile` with a dead grant, a missing grant and the two body
    /// findings, so both renderings have every actionable field to carry.
    const PARITY_MANIFEST: &str = r#"
postgres $DATABASE_URL

postgres analytics $ANALYTICS_URL

view catalog regex '/catalog/.*' {
    postgres {
        query 'SELECT 1'
        query 'SELECT 2'
    }
}

view form regex '/form/.*' {
    body json
}

view upload regex '/upload/.*' {}
"#;

    fn http_line(
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body_len: u64,
        sample: Option<BodySample>,
    ) -> HttpObsLine {
        HttpObsLine {
            method: method.to_string(),
            path: path.to_string(),
            content_type: content_type.map(str::to_string),
            body_len,
            sample,
        }
    }

    #[test]
    fn every_fact_the_json_document_carries_is_in_the_human_report() {
        // One fixture rendered both ways: the human form and the `--json`
        // document of the same report. A field the human path throws away fails
        // here instead of in a developer's terminal.
        let manifest = audit_v1::parse_manifest(PARITY_MANIFEST).expect("manifest");
        let observed = CheckObsV1::new(
            SCHEMA_VERSION,
            vec![
                http_line("GET", "/catalog/1", None, 0, None),
                http_line(
                    "POST",
                    "/form/1",
                    Some("application/x-www-form-urlencoded"),
                    10,
                    Some(BodySample {
                        mime: "application/x-www-form-urlencoded".to_string(),
                        truncated: false,
                        valid_json: false,
                        valid_form: true,
                    }),
                ),
                http_line(
                    "POST",
                    "/upload/1",
                    Some("multipart/form-data; boundary=x"),
                    20,
                    Some(BodySample {
                        mime: "multipart/form-data".to_string(),
                        truncated: false,
                        valid_json: false,
                        valid_form: false,
                    }),
                ),
            ],
            vec![
                statement(
                    ObsTarget::Routed {
                        route: "catalog".to_string(),
                    },
                    "SELECT 3",
                ),
                statement(
                    ObsTarget::Routed {
                        route: "catalog".to_string(),
                    },
                    "SELECT 2",
                ),
            ],
        )
        .expect("observations");
        let report = audit_v1::evaluate(&manifest, &observed).expect("audit");
        let human = audit_v1::human(&report);
        let document: serde_json::Value =
            serde_json::from_str(&report.to_json().expect("json")).expect("the document parses");

        let mut facts = 0;
        for finding in document["findings"].as_array().expect("findings") {
            for field in ["sql", "proxy"] {
                if let Some(value) = finding.get(field).and_then(serde_json::Value::as_str) {
                    assert!(
                        human.contains(value),
                        "the human report drops {field} `{value}`:\n{human}"
                    );
                    facts += 1;
                }
            }
            if let Some(to) = finding["fix"]["to"].as_str() {
                assert!(
                    human.contains(to),
                    "the human report drops fix.to `{to}`:\n{human}"
                );
                facts += 1;
            }
            for mime in finding["observed"].as_array().into_iter().flatten() {
                let mime = mime.as_str().expect("a MIME string");
                assert!(
                    human.contains(mime),
                    "the human report drops observed `{mime}`:\n{human}"
                );
                facts += 1;
            }
        }
        assert_eq!(report.findings.len(), 5, "{:?}", report.findings);
        assert!(
            facts >= 9,
            "the fixture must exercise the actionable fields: {facts}"
        );
        // One `fix:` line per finding, `proxy_unused` and `no_observations`
        // included: the field-by-field loop above can only assert a `fix.to`
        // that exists, so the count is what catches a finding with no fix line
        // at all (the gap that hid `proxy_unused`).
        assert_eq!(
            human.matches("\n      fix: ").count(),
            report.findings.len(),
            "one fix line per finding:\n{human}"
        );
        // The file's own nouns: a retired `route`/`group` id names nothing a
        // `Blastfile` writes.
        assert!(!human.contains("route "), "{human}");
        assert!(!human.contains("group "), "{human}");
    }

    #[test]
    fn a_dry_run_proposes_the_bytes_learn_would_write() {
        // The dry run's output is the writer's own splice — the same string
        // `learn()` hands to `write_atomic` — so a paste and a `--learn` run
        // cannot disagree: no second renderer, no `fmt` of the whole document.
        let manifest = audit_v1::parse_manifest(MANIFEST).expect("manifest");
        let observed = observations(vec![statement(
            ObsTarget::Unrouted {
                method: "POST".to_string(),
                path: "/cart/".to_string(),
            },
            "SELECT 1",
        )]);
        let plan = audit_v1::plan_learn(&manifest, &observed).expect("plan");
        let view = plan.edits.views.first().expect("a view").name.clone();
        let proposed = plan.proposal(MANIFEST).expect("the plan proposes a file");
        let written =
            blastit_manifest::blastfile::apply_edits(MANIFEST, &plan.edits).expect("the edit");
        assert_eq!(
            proposed, written,
            "the dry run must print exactly what --learn writes"
        );
        assert!(proposed.contains(&format!("view {view} ")), "{proposed}");
        assert!(proposed.contains("query 'SELECT 1'"), "{proposed}");
        assert_ne!(proposed, MANIFEST, "the proposal adds something");
        // Denied twin: an editor that refuses is reported, never swallowed into
        // a dry run that prints nothing (a real run prints `learn: <error>`).
        let broken = "view cart \"/cart/\" {\n    body xml\n}\n";
        let refused = plan.proposal(broken).expect_err("the editor refuses");
        assert!(!refused.is_empty(), "{refused}");
    }

    #[test]
    fn a_project_venvs_interpreter_is_preferred_over_a_bare_one() {
        let root = CheckRoot::create().expect("a runtime root");
        let venv = root.path().join("venv");
        let bin = venv.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        // Denied twin: a directory with no interpreter is not a venv.
        assert!(venv_python(&venv).is_none());
        std::fs::write(bin.join("python"), b"#!/bin/sh\n").expect("python");
        assert_eq!(venv_python(&venv), Some(bin.join("python")));
        // The `python3` name wins over `python` when both are there.
        std::fs::write(bin.join("python3"), b"#!/bin/sh\n").expect("python3");
        assert_eq!(venv_python(&venv), Some(bin.join("python3")));
    }

    #[test]
    fn an_installed_bundle_is_a_gate_root_and_a_cargo_tree_is_not() {
        // A `make install` bundle: seven modules under
        // `<prefix>/share/blastit/python/blastit`, the binary in `lib/`, and the
        // `bin/blastit` symlink the operator runs — no `_bin`, which is the
        // retired wheel layout `check-bundle-layout.sh` refuses.
        let root = CheckRoot::create().expect("a runtime root");
        let prefix = root.path().join("prefix");
        let share = prefix.join("share/blastit");
        let package = share.join("python/blastit");
        std::fs::create_dir_all(&package).expect("package");
        for module in [
            "__init__",
            "__main__",
            "_blastit",
            "_check_pytest",
            "fold",
            "logs",
            "pytest_plugin",
        ] {
            std::fs::write(package.join(format!("{module}.py")), b"").expect("module");
        }
        let lib = share.join("lib");
        std::fs::create_dir_all(&lib).expect("lib");
        std::fs::write(lib.join("blastit"), b"#!/bin/sh\n").expect("binary");
        std::fs::create_dir_all(prefix.join("bin")).expect("bin");
        std::os::unix::fs::symlink("../share/blastit/lib/blastit", prefix.join("bin/blastit"))
            .expect("symlink");
        // `current_exe` resolves `bin/blastit`, so a bundle's `exe_dir` is the
        // `lib/` directory the symlink names.
        assert_eq!(
            std::fs::read_link(prefix.join("bin/blastit")).expect("link"),
            Path::new("../share/blastit/lib/blastit")
        );
        let expected = std::fs::canonicalize(share.join("python")).expect("canonical package");
        let found = bundle_root(&lib).expect("a bundle root");
        assert_eq!(
            std::fs::canonicalize(&found).expect("canonical root"),
            expected
        );

        // Denied twin: the pair is belt-and-braces, so a tree with only one
        // marker is not the gate root (and neither marker is `_bin`).
        let entry = package.join("__main__.py");
        std::fs::remove_file(&entry).expect("remove");
        assert_eq!(bundle_root(&lib), None, "a package with no entry point");
        std::fs::write(&entry, b"").expect("restore");
        assert_eq!(
            std::fs::canonicalize(bundle_root(&lib).expect("restored gate")).expect("canonical"),
            expected
        );
        let gate = package.join("_check_pytest.py");
        std::fs::remove_file(&gate).expect("remove");
        assert_eq!(bundle_root(&lib), None, "a package with no gate");

        // A `cargo` tree: `target/<profile>/deps` beside the repository's own
        // `python/`. Its two candidate roots are `target/<profile>/python` and
        // the repository itself, and neither holds a `blastit` package — so a
        // build tree is never mistaken for a bundle.
        let repo = root.path().join("repo");
        let deps = repo.join("target/debug/deps");
        std::fs::create_dir_all(&deps).expect("deps");
        let repo_package = repo.join("python/blastit");
        std::fs::create_dir_all(&repo_package).expect("repository package");
        for module in ["__init__", "__main__", "_check_pytest"] {
            std::fs::write(repo_package.join(format!("{module}.py")), b"").expect("module");
        }
        assert_eq!(bundle_root(&deps), None);
    }

    /// The published `py3-none-*` wheel (`MB.42.3.3`): `<site-packages>/blastit/_bin/blastit`
    /// beside the package's own modules, so `<exe_dir>/../..` is the root the gate
    /// lives under. This is the layout the dead `../../..` could never match, and the
    /// one `blastit check` runs in when the CLI was `pip install`ed.
    #[test]
    fn a_repacked_wheel_layout_finds_its_gate() {
        let root = CheckRoot::create().expect("a runtime root");
        let site = root.path().join("lib/python3.13/site-packages");
        let package = site.join("blastit");
        std::fs::create_dir_all(&package).expect("package");
        for module in [
            "__init__",
            "__main__",
            "_blastit",
            "_check_pytest",
            "fold",
            "logs",
            "pytest_plugin",
        ] {
            std::fs::write(package.join(format!("{module}.py")), b"").expect("module");
        }
        let bin = package.join("_bin");
        std::fs::create_dir_all(&bin).expect("_bin");
        std::fs::write(bin.join("blastit"), b"#!/bin/sh\n").expect("binary");

        let found = bundle_root(&bin).expect("the wheel root");
        assert_eq!(
            std::fs::canonicalize(&found).expect("canonical"),
            std::fs::canonicalize(&site).expect("canonical site-packages"),
            "the repacked wheel's gate root is `<site-packages>`, two levels above `_bin`"
        );

        // Denied twin: a `blastit` package one level **above** `site-packages`
        // (`lib/python3.13/blastit`) must not shadow the wheel's own — that is the
        // exact place `MB.42.11`'s third root looked, and it is why that root was
        // dead rather than merely redundant.
        let decoy = site.join("../../blastit");
        std::fs::create_dir_all(&decoy).expect("decoy");
        for module in ["__main__", "_check_pytest"] {
            std::fs::write(decoy.join(format!("{module}.py")), b"").expect("module");
        }
        assert_eq!(
            std::fs::canonicalize(bundle_root(&bin).expect("the wheel root")).expect("canonical"),
            std::fs::canonicalize(&site).expect("canonical site-packages"),
            "a package above site-packages must not shadow the wheel's own"
        );

        // Denied twin: a wheel whose gate module is missing is not a gate root.
        std::fs::remove_file(package.join("_check_pytest.py")).expect("remove");
        assert_eq!(bundle_root(&bin), None, "a wheel with no staged gate");
    }

    #[test]
    fn a_missing_manifest_names_the_fix() {
        let error = read_manifest(Path::new("/nonexistent/Blastfile")).unwrap_err();
        assert!(error.contains("BLASTIT_MANIFEST"), "{error}");
        assert!(error.contains("Blastfile"), "{error}");
    }

    #[test]
    fn the_route_policies_are_the_effective_grants() {
        let manifest = audit_v1::parse_manifest(MANIFEST).expect("manifest");
        let policies = route_policies(&manifest);
        let catalog = policies
            .iter()
            .find(|policy| policy.route == "catalog")
            .expect("the catalog route has a policy");
        assert_eq!(catalog.queries, vec!["SELECT 1".to_string()]);
        assert!(!catalog.unrestricted);
        // Denied twin: a route the manifest grants nothing on has no policy, so
        // nothing can attribute a statement to a grant that does not exist.
        assert!(
            !policies.iter().any(|policy| policy.route == "fallback"),
            "{policies:?}"
        );
        // And every policy names a route the manifest really declares.
        for policy in &policies {
            assert!(
                manifest
                    .routes()
                    .iter()
                    .any(|route| route.name() == policy.route),
                "{policy:?}"
            );
        }
    }

    #[test]
    fn the_runtime_root_is_private_and_removed() {
        let root = CheckRoot::create().expect("a runtime root");
        let path = root.path().to_path_buf();
        let metadata = std::fs::symlink_metadata(&path).expect("metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        let pg = root.postgres_dir().expect("the socket directory");
        assert!(pg.is_dir());
        assert_eq!(
            std::fs::symlink_metadata(&pg)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        drop(root);
        assert!(!path.exists(), "the root is removed");
    }

    #[test]
    fn the_django_handshake_carries_the_real_inputs_and_the_test_database() {
        let manifest = audit_v1::parse_manifest(MANIFEST).expect("manifest");
        let real = "postgresql://real-user:real-secret@127.0.0.1:5432/app?sslmode=disable";
        let platform = BTreeMap::from([("DATABASE_URL".to_string(), real.to_string())]);
        let prepared =
            blastit_env::build_with_socket_dir(&manifest, &platform, None, "/run/blastit-check")
                .expect("the environment");
        let document = django_document(&manifest, &prepared, &platform, "/run/blastit-check")
            .expect("document");

        // Allowed: the handshake is the one place the real inputs live, and it
        // names both the local dbproxy endpoint and the database Django's
        // fixtures will create.
        assert_eq!(document["proxy"], "db");
        assert_eq!(document["database"], "test_app");
        assert_eq!(document["socket_dir"], "/run/blastit-check");
        assert_eq!(document["port"], 5432);
        assert_eq!(document["user"], "blastit");
        assert_eq!(document["real"]["DATABASE_URL"], real);
        assert_eq!(
            document["swapped"]["DATABASE_URL"],
            prepared.app_env()["DATABASE_URL"]
        );
        // Denied twin: the environment the suite keeps after the handshake has
        // no credential in it, so a leaked `real` map cannot come from here.
        let swapped = document["swapped"]["DATABASE_URL"].as_str().expect("url");
        assert!(!swapped.contains("real-secret"), "{swapped}");
        assert!(!swapped.contains("127.0.0.1"), "{swapped}");

        // Split-form proxy inputs work too: `DB_PASSWORD` has no output slot in
        // `swapped` and the local socket endpoint is still explicit.
        let split_manifest = audit_v1::parse_manifest(
            "postgres {\n    host $DB_HOST\n    port $DB_PORT\n    user $DB_USER\n    password $DB_PASSWORD\n    database $DB_DATABASE\n}\n\nview home \"/\" {}\n",
        )
        .expect("split manifest");
        let split_platform = BTreeMap::from([
            ("DB_HOST".to_string(), "127.0.0.1".to_string()),
            ("DB_PORT".to_string(), "5432".to_string()),
            ("DB_USER".to_string(), "real-user".to_string()),
            ("DB_PASSWORD".to_string(), "real-secret".to_string()),
            ("DB_DATABASE".to_string(), "app".to_string()),
        ]);
        let split_prepared = blastit_env::build_with_socket_dir(
            &split_manifest,
            &split_platform,
            None,
            "/run/blastit-check",
        )
        .expect("split environment");
        let split_doc = django_document(
            &split_manifest,
            &split_prepared,
            &split_platform,
            "/run/blastit-check",
        )
        .expect("split document");
        assert_eq!(split_doc["socket_dir"], "/run/blastit-check");
        assert_eq!(split_doc["port"], 5432);
        assert_eq!(split_doc["real"]["DB_PASSWORD"], "real-secret");
        assert!(split_doc["swapped"].get("DB_PASSWORD").is_none());
    }

    #[test]
    fn the_handshake_file_is_owner_only_and_never_reused() {
        let root = CheckRoot::create().expect("a runtime root");
        let path = root.path().join("django-setup.json");
        write_private(&path, b"{\"x\":1}").expect("written");
        let metadata = std::fs::symlink_metadata(&path).expect("metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
        // Denied twin: a second handshake never silently reuses the path — a
        // stale file would hand the previous run's credentials to this one.
        let error = write_private(&path, b"{\"x\":2}").expect_err("refused");
        assert!(error.contains("File exists"), "{error}");
    }

    #[test]
    fn a_store_less_app_binds_no_listener_and_a_proxied_one_binds_one() {
        // Allowed: a hello-world with no store has nothing to proxy, so the
        // check binds no socket and starts no dbproxy (`MB.40.2`).
        let root = CheckRoot::create().expect("a runtime root");
        let pg = root.postgres_dir().expect("the socket directory");
        let store_less = audit_v1::parse_manifest("view hello \"/*\" {}\n").expect("manifest");
        let platform = BTreeMap::new();
        let prepared =
            blastit_env::build_with_socket_dir(&store_less, &platform, None, "/run/blastit-check")
                .expect("the environment");
        let listeners = bind_listeners(&prepared, &pg).expect("no listener");
        assert!(listeners.is_empty(), "{listeners:?}");
        assert_eq!(
            std::fs::read_dir(&pg)
                .expect("the socket directory")
                .count(),
            0,
            "a store-less run must not leave a socket it never serves"
        );

        // The other half: one store is one real wildcard socket, `0600` and
        // owned by this uid.
        let proxied = audit_v1::parse_manifest("postgres $DATABASE_URL\n\nview hello \"/*\" {}\n")
            .expect("manifest");
        let platform = BTreeMap::from([(
            "DATABASE_URL".to_string(),
            "postgresql://user:secret@127.0.0.1:5432/app".to_string(),
        )]);
        let prepared =
            blastit_env::build_with_socket_dir(&proxied, &platform, None, "/run/blastit-check")
                .expect("the environment");
        let listeners = bind_listeners(&prepared, &pg).expect("one listener");
        assert_eq!(listeners.len(), 1);
        let port = blastit_env::postgresql::local_port(0).expect("a local port");
        let socket = pg.join(format!(".s.PGSQL.{port}"));
        let metadata = std::fs::symlink_metadata(&socket).expect("the socket exists");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
    }

    #[test]
    fn the_handshake_file_is_removed_however_the_run_ends() {
        let root = CheckRoot::create().expect("a runtime root");
        let path = root.path().join("django-setup.json");
        write_private(&path, b"{}").expect("written");
        {
            let handshake = Handshake::new(Some(path.clone()));
            assert_eq!(handshake.path(), Some(path.as_path()));
        }
        // Allowed: a run whose suite never started leaves no real input behind.
        assert!(!path.exists(), "the guard removed the handshake file");
        // Denied twin: no Django run has no file, and dropping it is a no-op.
        let absent = Handshake::new(None);
        assert!(absent.path().is_none());
    }

    #[test]
    fn a_pytest_django_project_without_a_postgresql_proxy_is_a_start_failure() {
        let manifest = audit_v1::parse_manifest("view home \"/\" {}\n").expect("manifest");
        let platform = BTreeMap::new();
        let prepared = blastit_env::build_with_socket_dir(&manifest, &platform, None, "/run/x")
            .expect("the environment");
        let error =
            django_document(&manifest, &prepared, &platform, "/run/x").expect_err("refused");
        assert!(error.contains("no postgresql proxy"), "{error}");
        // The fix names the store a `Blastfile` writes, not the retired table.
        assert!(error.contains("postgres $DATABASE_URL"), "{error}");
        assert!(!error.contains("[proxy."), "{error}");
    }

    #[test]
    fn a_store_with_no_input_variables_is_a_start_failure_that_names_the_block() {
        // The guard fires for a model that hands the handshake nothing — no
        // `postgres` block any build accepts reaches it — so the sentence is
        // asserted here: it names the store's block, as a `Blastfile` writes it,
        // and never the retired per-route table.
        let error = no_input_variables("db");
        assert!(error.contains("no input variables"), "{error}");
        assert!(
            error.contains("postgres { user $DB_USER; password $DB_PASSWORD }"),
            "{error}"
        );
        assert!(!error.contains("[proxy."), "{error}");
        assert!(!error.contains("protocol ="), "{error}");
    }
}
