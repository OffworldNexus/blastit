#![cfg_attr(not(feature = "python"), allow(unused_imports))]

#[cfg(feature = "python")]
use blastit_events::ManifestWarning;
use blastit_events::{Component, Event, EventKind};
use blastit_manifest::{Manifest, RoutingTable};

#[cfg(feature = "php")]
use crate::dev_reload::Class;
use crate::dev_reload::{MarkOutcome, Reload, Unavailable};
use crate::logs::Emitter;
use crate::supervise::{Parts, Supervisor};

use super::boot::Serving;
#[cfg(feature = "python")]
use super::emit_ready;
#[cfg(feature = "python")]
use super::load::{self, Inputs, Loaded};

/// What the post-preload marking pass owes the boot line (§4.7, `T32`).
///
/// `reload` is a claim, and only a pass that actually marked may make it: a
/// watcher that latched `ENOSPC`/the cap already emitted the ladder's one
/// warning, and one that latched dead must emit nothing at all.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum BootLine {
    /// One `reload` event, with the marks the pass placed.
    Announce(usize),
    /// No line: the boot already owed its one warning.
    Silent,
    /// The ladder's one `dev.reload.unavailable`, then no line.
    Unavailable(Unavailable),
    /// A watcher that latched dead: the ladder's one warning, no line, and the
    /// `Reload` is dropped (`reload = None`).
    Stopped(Unavailable),
}

/// Turns one marking outcome into what the boot does about the boot line.
pub(super) fn after_preload_marks(
    outcome: MarkOutcome,
    marking_stopped: bool,
    dirs: usize,
) -> BootLine {
    match outcome {
        MarkOutcome::Marked if !marking_stopped => BootLine::Announce(dirs),
        MarkOutcome::Marked => BootLine::Silent,
        MarkOutcome::Exhausted(why) => BootLine::Unavailable(why),
        MarkOutcome::Dead(why) => BootLine::Stopped(why),
    }
}

/// Boots the one live-reload watcher of a `dev` boot with reload on, seeds the
/// paths only the boot knows, and settles it (`ADR-0014` §4.1).
///
/// `between` runs while the watcher exists and **before** the marking pass: the
/// WSGI boot's `preload` lives there, so the files it opens are reported and
/// marked, and whatever it returns comes back beside the watcher. The
/// `php`-only boot passes a no-op — its workers report their own files — so
/// both boot paths share one construction, one seeding pass, one marking pass
/// and one `reconcile`.
///
/// Returns `None` — and the ladder's one `dev.reload.unavailable` — exactly as
/// [`crate::supervise::boot_reload`] does. A production boot, `check` and a
/// `--no-reload` boot pass straight through with `None`: reload is a `dev`-only
/// path, and "off" means no watcher group, no mark, no report socket and no
/// audit hook at all (§1, `R1`).
pub(super) fn boot_and_seed_reload<T>(
    emitter: &Emitter,
    serving: &Serving,
    between: impl FnOnce(Option<&mut Reload>) -> T,
) -> (Option<Reload>, T) {
    let mut reload = match serving.dev.is_some() && serving.dev_reload {
        true => {
            crate::supervise::boot_reload(&serving.config, emitter, serving.reload_fault.as_deref())
        }
        false => None,
    };
    // A view reports its files in its first batch, and that batch is
    // what marks them — so a front controller edited **before** the first
    // request was not watched at all. Every PHP slot's entry and root are seeded
    // here, before the marking pass below, so a save at any point after boot is
    // caught.
    #[cfg(feature = "php")]
    if let Some(active) = reload.as_mut() {
        let tracked: Vec<std::path::PathBuf> = serving
            .php
            .iter()
            .flatten()
            .flat_map(|binding| [binding.entry.clone(), binding.document_root.clone()])
            .collect();
        active.policy().track_boot_files(&tracked, Class::Loaded);
    }
    let kept = between(reload.as_mut());
    settle_reload(&mut reload, serving, emitter);
    (reload, kept)
}

/// Boot order step 5 and §4.6 step 7: the marking pass, the one boot line, the
/// lost-save `reconcile`, and the drop of a watcher that latched dead.
///
/// `reload: on` is a claim that marking succeeded: a policy that latched
/// `ENOSPC`/the cap in `boot` already emitted its one warning, and printing the
/// line over it would be the lie the ladder exists to prevent (§4.7, `T32`).
/// The replacement half of a reload repeats neither the line nor `ready`: both
/// were the first image's, and claiming a boot that did not happen is exactly
/// what the observable contract must not do (§4.6 step 7).
fn settle_reload(reload: &mut Option<Reload>, serving: &Serving, emitter: &Emitter) {
    let Some(active) = reload.as_mut() else {
        return;
    };
    let outcome = active.sync_marks();
    let stopped = active.marking_stopped();
    let dirs = active.marked_dirs();
    match after_preload_marks(outcome, stopped, dirs) {
        BootLine::Announce(dirs) if serving.announce => {
            crate::supervise::announce_reload(emitter, dirs);
        }
        BootLine::Announce(_) | BootLine::Silent => {}
        BootLine::Unavailable(why) => crate::supervise::reload_unavailable(emitter, &why),
        BootLine::Stopped(why) => {
            crate::supervise::reload_stopped(emitter, &why);
            *reload = None;
        }
    }
    // §4.6 step 7: one `stat` per union path, once, so a save that landed while
    // no watcher existed is not lost. Never re-armed by a future mtime.
    if let Some(since) = serving.reload_since
        && let Some(active) = reload.as_mut()
    {
        let now = active.now();
        active.policy().reconcile(since, now);
    }
}

/// Loads the application through the frozen vtable, emits `ready`, and
/// supervises. Never returns.
///
/// Every boot with a Python view pinned its runtime in stage 0; this function
/// `dlopen`s it **after** the `http`/scheduler forks, so no half-initialised
/// interpreter is inherited.
#[cfg(feature = "python")]
pub(super) fn serve(emitter: &Emitter, mut serving: Serving) -> ! {
    // The watcher exists **before** the first import, so the `preload` phase's
    // modules and file opens are watched too (§4.1 boot order), and the marking
    // pass runs once the load has reported what it opened.
    let (reload, loaded) = boot_and_seed_reload(emitter, &serving, |reload| {
        // `R13`: an all-static site (and a PHP-only one) has no application to
        // import — stage 0 pinned no interpreter at all (`Config::python` is
        // `None`), so this image loads nothing and forks no WSGI view.
        if serving.config.python.is_none() {
            return Ok::<Option<Loaded>, String>(None);
        }
        // The `preload` phase reports on source `0xffff`; its paths are
        // `pre_fork`, so a change to one takes the reload cycle (§4.4). This
        // socket is also the **gate** for the loaded-file hook: no socket, no
        // hook, no marks (`S3`).
        let preload = match reload {
            Some(reload) => match crate::supervise::open_preload_report(reload) {
                Ok(reporter) => Some(reporter),
                Err(err) => {
                    emitter.emit(&Event::now(
                        Component::Init,
                        EventKind::ManifestWarning(ManifestWarning::new(format!(
                            "live reload: the preload report socket: {err}"
                        ))),
                    ));
                    None
                }
            },
            None => None,
        };
        let report_fd = preload.as_ref().map_or(-1, std::os::fd::AsRawFd::as_raw_fd);
        let inputs = Inputs {
            config: &serving.config,
            routes: serving.declared_routes,
            emitter,
            dev: serving.dev.is_some(),
            report_fd,
            memory: serving.memory,
            announce: serving.announce,
        };
        // The universal binary is the only installation: when stage 0 pinned a
        // runtime, this image `dlopen`s it (`Embed`); otherwise the manifest
        // needs no interpreter at all.
        let result = match &serving.py_fds {
            Some(pair) => load::embed(&inputs, pair).map(Some),
            None => Err(
                "this boot has no pinned Python runtime to serve with (a blastit bug — \
                 report the boot log above)"
                    .to_string(),
            ),
        };
        // The shim adopted the report socket (or nothing did): either way this
        // process must not close a descriptor the shim now writes through.
        if let Some(socket) = preload {
            std::mem::forget(socket);
        }
        result
    });
    let loaded: Option<Loaded> = match loaded {
        Ok(loaded) => loaded,
        Err(message) => super::fail(emitter, serving.dev.as_ref(), &message),
    };
    if serving.announce {
        emit_ready(emitter, &serving.config, serving.declared_routes);
    }
    // §1.2.1 item 6: the pinned `(libpython, shim)` descriptors are held only
    // when a `dev` cycle can re-exec from them; every other boot drops them the
    // moment the `dlopen` above finished (`take` releases them here, since this
    // frame never returns).
    let keep_py_fds = serving.reload_source.is_some();
    let py_fds = serving.py_fds.take().filter(|_| keep_py_fds);
    // The handle borrows the loader, and the supervisor borrows both: declared
    // in this order so the supervisor drops first. A boot with no pinned
    // runtime (static-only, PHP-only) has neither.
    let runtime = loaded
        .as_ref()
        .map(|loaded| loaded.python.runtime(loaded.zygote));
    let mut supervisor = Supervisor::new(
        Parts {
            cfg: serving.config,
            limits: serving.limits,
            routes: serving.routes,
            #[cfg(feature = "php")]
            php: serving.php,
            egress: serving.egress,
            plans: serving.plans,
            runtime,
            py_fds,
            ctl: serving.ctl,
            signals: serving.signals,
            mask: serving.mask,
            components: serving.components,
            dev: serving.dev,
            learn: serving.learn,
            reload,
            reload_source: serving.reload_source,
            chan: serving.chan,
            policy: serving.policy,
            learn_fds: serving.learn_fds,
            boot_pairs: serving.boot_pairs,
        },
        emitter,
    );
    supervisor.run()
}

/// Builds the routing table for `dev --learn`, reserving slot `0` for the
/// scheduler's unrouted-request fallback (`R26`, `R27`).
///
/// Slot `0` carries no pattern — [`RoutingTable::route`] can never select it —
/// and declared routes `0..N` are their manifest entries, shifted to `1..=N`
/// in exact manifest order with their original methods and patterns intact.
/// The table is built from the model [`RoutingTable::for_learn`] already holds:
/// no document is rendered and re-parsed.
pub(super) fn learn_routing_table(manifest: &Manifest) -> Result<RoutingTable, String> {
    RoutingTable::for_learn(manifest).map_err(|errors| errors.to_string())
}
