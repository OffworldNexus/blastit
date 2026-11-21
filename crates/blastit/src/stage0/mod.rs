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

use std::os::unix::ffi::OsStringExt;
use std::time::Duration;

use blastit_events::{
    Component, Event, EventKind, Level, Lifecycle, ManifestWarning as WarningEvent, Stream,
};
use blastit_manifest::{Manifest, UidLayout};

use crate::config::{Config, LogFormat, Mode, Privilege};
// The two provenance types are the deployment gate's (`php` only): the rest of
// stage 0 names neither.
use crate::host::{EXEC, FILE_NOUN, Host};
use crate::logs::{Colour, Emitter, fail, output_event};
// The `entrypoint php` discovery gate: a stage-0 dependency that
// names no SAPI type and no `dlopen`, so `init` can answer "can this host embed
// PHP?" without loading anything (`ADR-0010` §3, design §4.4 D5).
#[cfg(feature = "php")]
use blastit_view_php::discovery::{PhpBoot, PhpShimScope};

mod dev;
mod fork_exec;
mod gates;
#[cfg(feature = "python")]
mod project_python;

pub use dev::run_dev;

#[cfg(feature = "php")]
pub use gates::deployment_boot_check;
#[cfg(not(feature = "php"))]
pub(crate) use gates::no_php_support;
#[cfg(not(feature = "python"))]
pub(crate) use gates::no_python_support;
#[cfg(feature = "php")]
pub(crate) use gates::php_boot_gate;
pub(crate) use gates::{PROBE_TIMEOUT_DEFAULT, PROBE_TIMEOUT_MEMBER, RESTART_MEMBER};
#[cfg(debug_assertions)]
pub(crate) use gates::{TEST_RELOAD_FAULT_MEMBER, TEST_SWAP_POINT_MEMBER};
pub(crate) use gates::{
    app_rule, is_static_only, raw_ingress_gate, route_declares_proxy, static_app_refusal,
    static_root_permission,
};

use fork_exec::{exec_stage1, fork_dbproxy};
use gates::{ServeGates, bind_view_sockets, emit_runtime, layout_collision};

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
/// §1.1: a person at a terminal reads the refusal as prose on stderr and stdout
/// stays empty (a startup error writes zero bytes there); a pipeline that asked
/// for `json` gets the lines that waited, as JSON, and the error after them.
/// `human` is the composition root's decision ([`crate::logs::text_refusals`]),
/// taken once before anything can fail.
fn fail_early(early: &mut Early, human: bool, message: &str) -> ! {
    if human {
        crate::logs::refuse_text(message);
    }
    // JSON needs no settings and takes no colour.
    let json = Emitter::stdout(LogFormat::Json, Level::Info, Colour::PLAIN);
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
fn capability_advice(err: &std::io::Error) -> String {
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
             A rootless deployment gives them to a *dedicated copy* of the {FILE_NOUN} instead:\n  \
             {}\n  \
             To run the same tree locally without root, use `blastit dev`; it is a \
             single-uid simulation, not a sandbox",
            copy_recipe()
        );
    }
    format!(
        "init runs as uid {uid} and is missing {missing}.\n  \
         A rootless deployment gives the capabilities to a *dedicated copy* of the {FILE_NOUN}, \
         never to the shared one:\n  {}\n  \
         To run the same tree locally without root, use `blastit dev`; it is a \
         single-uid simulation, not a sandbox",
        copy_recipe()
    )
}

/// The four commands that make the capable copy, in this installation's words
/// (D6/B1, B2).
fn copy_recipe() -> String {
    format!(
        "cp <the blastit binary> <the binary>-init\n  \
         chown <the uid init runs as>:<its gid> <the copy>\n  chmod 0500 <the copy>\n  \
         setcap '{}+ep' <the copy>\n  \
         then start <the copy>, and set {EXEC}=<the blastit binary> so that \
         the database proxy runs the capability-free copy",
        blastit_sys::caps::Capability::SETCAP,
    )
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
/// Runs stage 0. Never returns: it `execve`s stage 1 or exits non-zero.
///
/// `host` says how blastit was installed: which program the barrier and
/// dbproxy's own `execve` start (§4).
///
/// `preset` is `--preset` when the command line carried one, and it wins over
/// `BLASTIT_PRESET` (E1, [`Config::from_env`]).
///
/// `colour` is the terminal decision the command line's composition root made
/// once (`A-4`): this image never asks its descriptors again.
pub fn run(host: &Host, preset: Option<&str>, colour: Colour) -> ! {
    // §8.1: the log format is one of the *settings*, and stage 0's first lines
    // are made before there is a configuration to read it from — so they are
    // kept here, in the order they were made, and written out once the settings
    // (preset included) have been resolved. A boot whose configuration cannot be
    // read at all writes them as JSON instead, followed by the error: JSON is
    // the one format that needs no settings to print.
    let mut early = Early::default();
    // §1.1/A10.3: a boot that has not read its settings yet still owes a person
    // at a terminal a sentence they can read, and a `json` pipeline the format
    // it asked for. Read once here, by the composition root.
    let human_refusals = crate::logs::text_refusals(colour);
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
            fail_early(&mut early, human_refusals, &capability_advice(&err));
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
            fail_early(&mut early, human_refusals, &message);
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
        Err(err) => fail_early(&mut early, human_refusals, &err.to_string()),
    };
    // D6: init's mode belongs in the configuration, because stage 1's preload
    // `boot` event reports the same thing stage 0's does. It is set here, before
    // anything can fail with it, so the `boot` line below can say it — and so
    // E1's preset assumption can be checked against it.
    config.privilege = Some(privilege.clone());
    let emitter = Emitter::stdout(config.log_format, config.log_level, colour);
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
    // named before the setting.
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
            // (`php_view::probe_bindings`): it is the process that
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
    // its finding.
    //
    // The gate returns the proof `bind_view_sockets` demands, so this
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
    ) {
        fail(&emitter, &message);
    }
    for warning in &warnings {
        emitter.emit(&Event::now(
            Component::Init,
            EventKind::ManifestWarning(WarningEvent::new(warning.to_string())),
        ));
    }
    // Design §2.1/§2.4: a Python WSGI boot resolves its interpreter,
    // `libpython` and shim here and pins both descriptors; a PHP-only or
    // static-only boot returns before any search (`plan`). The three refusal
    // families are `blastit-py-sapi`'s sentences, printed here, before any
    // listener binds.
    #[cfg(feature = "python")]
    let python = match project_python::boot(&config, &manifest, project_python::Boot::Serve) {
        Ok(python) => python,
        Err(message) => crate::logs::fail_sentence(&emitter, &message),
    };
    #[cfg(feature = "python")]
    let runtime_line = python.as_ref().map(project_python::PythonBoot::runtime);
    #[cfg(not(feature = "python"))]
    let runtime_line = None;
    #[cfg(feature = "python")]
    if let Some(python) = &python {
        config.python = Some(python.handoff.clone());
    }
    // §8.1: one `runtime` line saying what this boot will run. PHP's is the
    // probe's (below the fork, after `php_module_startup`); the other two are
    // decided here.
    emit_runtime(&emitter, &config, runtime_line);

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
    let dbproxy = fork_dbproxy(&emitter, &config, &placement, &prepared, listeners);

    // The interpreter's import path travels in the configuration, not in the
    // environment: stage 1 is `execve`'d as an interpreter of its own (§4), and
    // a wheel re-executes it with `-I`, which drops `PYTHONPATH` and the
    // working directory. Empty for the standalone binary — whose project's
    // `site-packages` stage 0 discovered above.
    config.sys_path = host.sys_path().to_vec();
    #[cfg(feature = "python")]
    if let Some(python) = &python {
        for entry in python.handoff.sys_path.iter().rev() {
            if !config.sys_path.iter().any(|known| known == entry) {
                config.sys_path.insert(0, entry.clone());
            }
        }
    }
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
