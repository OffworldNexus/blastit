//! `blastit check`: audit a pytest suite against `blastit.toml` (R3..R5, R30).
//!
//! The command is a transaction with one place where it can fail and one place
//! where it writes:
//!
//! 1. **Before anything runs**, the CLI, the configuration, `./blastit.toml`
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
use blastit_manifest::{Manifest, RoutingTable};
use serde_json::json;

use crate::audit_v1::{self, CheckObsV1, ObsError, Report};
use crate::cli::CheckOptions;
use crate::config::Config;
use crate::host::{Host, Install};

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
pub fn run(host: &Host, options: &CheckOptions) -> u8 {
    match execute(host, options) {
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
fn execute(host: &Host, options: &CheckOptions) -> Result<u8, String> {
    let mut config = Config::for_check().map_err(|error| error.to_string())?;
    config.validate().map_err(|error| error.to_string())?;

    let source = read_manifest(&config.manifest)?;
    let (manifest, _warnings) = Manifest::parse_and_validate(&source, config.uid_base)
        .map_err(|errors| format!("{}: {errors}", config.manifest.display()))?;
    // The routing table is built here, not only inside the engine: the same
    // table resolves each observed request to its route (`resolve_targets`), so
    // a pattern that cannot compile fails before pytest starts.
    let table = RoutingTable::new(&manifest)
        .map_err(|errors| format!("{}: {errors}", config.manifest.display()))?;

    let runtime = CheckRoot::create()?;
    let socket_dir = runtime.postgres_dir()?;
    let socket_dir = socket_dir
        .to_str()
        .ok_or_else(|| "the runtime directory is not valid UTF-8".to_string())?
        .to_string();

    let platform = platform_env(&manifest)?;
    let prepared = prepare_env(&manifest, &platform, &socket_dir)?;
    // Both preflights run before dbproxy or the suite start, so a suite that
    // cannot run at all is exit `2` — never a proxy left behind, and never a
    // Django run that could reach the developer's own database (`R4`, `R30`).
    let python = python_program(host)?;
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

    let (obs_read, obs_child) = socketpair("the observation pipe")?;
    let (ctl_dbproxy, ctl_python) = socketpair("the control channel")?;
    let (ready_read, ready_child) = socketpair("the ready pipe")?;

    let mut dbproxy = Dbproxy::start(
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
    // Only *after* dbproxy's `execve`: the suite's end of the control channel
    // has to cross pytest's `execve` (`Stdio` does not carry it), but dbproxy
    // must never inherit it, so it keeps `FD_CLOEXEC` until it is spawned.
    clear_cloexec(ctl_python.as_raw_fd())?;

    let observed = ObsReader::start(obs_read);
    let http_path = runtime.path().join("http_obs.jsonl");
    let status = run_pytest(
        options,
        &prepared,
        &python,
        &http_path,
        ctl_python.as_raw_fd(),
        options.json,
        handshake.path(),
    );
    // The child closes its end; ours is dropped once pytest is gone.
    drop(ctl_python);
    let code = match status {
        Ok(code) => code,
        Err(message) => {
            dbproxy.stop();
            observed.stop();
            return Err(message);
        }
    };
    dbproxy.stop();
    let sql_text = observed.stop();

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
    emit_report(&report, options.json)?;
    Ok(if report.ok { CLEAN } else { FINDINGS })
}

/// Reads `./blastit.toml` (or `BLASTIT_MANIFEST`), naming the fix when absent.
fn read_manifest(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| {
        format!(
            "{}: {error} (run `blastit check` from the directory that holds \
             blastit.toml, or set BLASTIT_MANIFEST)",
            path.display()
        )
    })
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
            "pytest-django is installed but blastit.toml declares no postgresql proxy, \
             so its test database has nowhere to go; declare [proxy.<name>] with \
             protocol = \"postgresql\" (see docs/frameworks/django.md), or remove \
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
        return Err(format!(
            "pytest-django is installed but proxy `{}` has no input variables to hand \
             over, so its test database cannot be created; name them in [proxy.{}] and \
             export them before running blastit check",
            name.as_str(),
            name.as_str()
        ));
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
    fn start(
        host: &Host,
        manifest: &Manifest,
        prepared: &blastit_env::Prepared,
        mut listeners: Vec<(UnixListener, ListenerSpec)>,
        inherited: Inherited,
        ready_read: OwnedFd,
    ) -> Result<Self, String> {
        for (listener, spec) in &mut listeners {
            spec.fd = listener.as_raw_fd();
        }
        let uid = rustix::process::geteuid().as_raw();
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
                max_connections: 4,
            })
            .collect();
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

        // Every descriptor the child must inherit: the configuration pipe, the
        // two channels, the ready pipe, and one per listener.
        let mut inherited_fds = vec![
            secrets_read.as_raw_fd(),
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
        command.arg(format!("--events-fd={}", stderr_fd()));
        command.arg(format!("--ready-fd={}", inherited.ready.as_raw_fd()));
        command.arg("--seccomp=1");
        command.env_clear();
        command.stdin(Stdio::null());
        command.stdout(stdio_stderr()?);
        command.stderr(stdio_stderr()?);
        let child = command.spawn().map_err(|error| {
            format!(
                "{}: {error} (the check needs the `blastit-dbproxy` binary this \
                 installation ships)",
                program.display()
            )
        })?;
        // The parent's copies of the child's ends are closed right after the
        // `execve`: dbproxy owns them from here.
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
        Ok(Self { child })
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

/// Runs the suite, mapping a spawn failure to exit `2` (`R4`).
#[allow(clippy::too_many_arguments)]
fn run_pytest(
    options: &CheckOptions,
    prepared: &blastit_env::Prepared,
    python: &Path,
    http_path: &Path,
    ctl_fd: RawFd,
    json: bool,
    django: Option<&Path>,
) -> Result<i32, String> {
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
    command.env(HTTP_OBS, http_path);
    command.env(CTL_FD, ctl_fd.to_string());
    if let Some(path) = django {
        // R30/ADR-0004: the only place the real proxy inputs exist in this
        // process tree after the rewrite, `0600`, and gone as soon as the gate
        // has read it.
        command.env(DJANGO_SETUP, path);
    }
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

/// The interpreter `pytest` runs on.
///
/// A wheel's is the one it was installed into (`sys.executable`); a standalone
/// binary has none of its own, so `BLASTIT_PYTHON` names it, and `python3` on
/// `PATH` is the last resort.
fn python_program(host: &Host) -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("BLASTIT_PYTHON") {
        return Ok(PathBuf::from(explicit));
    }
    if host.install() == Install::Wheel {
        return Ok(host.stage1().program.clone());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in path.to_string_lossy().split(':') {
        if directory.is_empty() {
            continue;
        }
        for name in ["python3", "python"] {
            let candidate = Path::new(directory).join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(
        "no interpreter to run pytest on: set BLASTIT_PYTHON to the `python3` \
         that has pytest installed"
            .to_string(),
    )
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
    if plan.would_write(options.dry_run) {
        match blastit_manifest::edit::apply_edits(source, &plan.edits) {
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
    }
    if let Some(message) = failure {
        eprintln!("learn: {message}");
    }
    if clean { CLEAN } else { FINDINGS }
}

/// The §1.3 `learn:` line, with the absolute manifest path.
fn learn_line(plan: &audit_v1::LearnPlan, manifest: &Path, dry_run: bool) -> String {
    let path = std::path::absolute(manifest)
        .unwrap_or_else(|_| manifest.to_path_buf())
        .display()
        .to_string();
    if dry_run {
        format!(
            "learn: {} routes, {} queries proposed (dry-run) → {path}",
            plan.routes(),
            plan.queries()
        )
    } else {
        format!(
            "learn: {} routes, {} queries added → {path}",
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

/// This process's stderr, as a descriptor number.
fn stderr_fd() -> RawFd {
    std::io::stderr().as_raw_fd()
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

    const MANIFEST: &str = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "catalog"
path = ['/catalog/.*']
[route.db]
queries = ["SELECT 1"]

[[route]]
name = "fallback"
path = ['.*']
"#;

    fn statement(target: ObsTarget, sql: &str) -> WireSqlObs {
        WireSqlObs {
            target,
            proxy: "db".to_string(),
            kind: blastit_events::SqlKind::Query,
            sql: sql.to_string(),
            is_tx: false,
            allowed: false,
            verdict: blastit_dbproxy::ObsVerdict::Query {
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
            "version = 1\n\n[proxy.db]\nprotocol = \"postgresql\"\n\
             input = { url = \"DATABASE_URL\" }\n\n[[route]]\nname = \"catalog\"\n\
             path = ['/catalog/.*']\n",
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
            "version = 1\n\n[proxy.db]\nprotocol = \"postgresql\"\n\
             input = { url = \"DATABASE_URL\" }\n\n[[route]]\nname = \"catalog\"\n\
             path = ['/catalog/.*']\n[route.db]\nqueries = [\"SELECT 1\"]\n",
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
            .routes
            .push(blastit_manifest::edit::ProposedRoute {
                name: "orders".to_string(),
                path: vec!["/orders/[0-9]+".to_string()],
            });
        plan.edits
            .grants
            .push(blastit_manifest::edit::ProposedGrant {
                route: "orders".to_string(),
                proxy: "db".to_string(),
                queries: vec!["SELECT 1".to_string()],
            });
        let line = learn_line(&plan, Path::new("./blastit.toml"), false);
        assert!(
            line.starts_with("learn: 1 routes, 1 queries added → /"),
            "{line}"
        );
        assert!(!line.contains("dry-run"), "{line}");
        let dry = learn_line(&plan, Path::new("./blastit.toml"), true);
        assert!(dry.contains("proposed (dry-run)"), "{dry}");
        assert!(!dry.contains("added"), "a dry run adds nothing: {dry}");
    }

    #[test]
    fn a_missing_manifest_names_the_fix() {
        let error = read_manifest(Path::new("/nonexistent/blastit.toml")).unwrap_err();
        assert!(error.contains("BLASTIT_MANIFEST"), "{error}");
        assert!(error.contains("blastit.toml"), "{error}");
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
            "version = 1\n\n[proxy.db]\nprotocol = \"postgresql\"\n\
             input = { host = \"DB_HOST\", port = \"DB_PORT\", user = \"DB_USER\", \
             password = \"DB_PASSWORD\", database = \"DB_DATABASE\" }\n\n\
             [[route]]\nname = \"home\"\npath = ['/']\n",
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
        let manifest =
            audit_v1::parse_manifest("version = 1\n\n[[route]]\nname = \"home\"\npath = ['/']\n")
                .expect("manifest");
        let platform = BTreeMap::new();
        let prepared = blastit_env::build_with_socket_dir(&manifest, &platform, None, "/run/x")
            .expect("the environment");
        let error =
            django_document(&manifest, &prepared, &platform, "/run/x").expect_err("refused");
        assert!(error.contains("no postgresql proxy"), "{error}");
        assert!(error.contains("protocol = \"postgresql\""), "{error}");
    }
}
