#![cfg_attr(not(feature = "python"), allow(unused_imports))]

#[cfg(feature = "python")]
use blastit_events::ManifestWarning;
use blastit_events::{Component, Event, EventKind};
use blastit_manifest::{Manifest, RoutingTable};

#[cfg(feature = "python")]
use crate::dev_reload::{MarkOutcome, Unavailable};
use crate::logs::Emitter;
use crate::py::Python;
use crate::supervise::{Parts, Supervisor};

use super::boot::Serving;
#[cfg(feature = "python")]
use super::emit_ready;
#[cfg(feature = "python")]
use super::{Boot, load};

/// What the post-preload marking pass owes the boot line (§4.7, `T32`).
///
/// `reload: on` is a claim, and only a pass that actually marked may make it: a
/// watcher that latched `ENOSPC`/the cap already emitted the ladder's one
/// warning, and one that latched dead must print nothing at all.
#[cfg(feature = "python")]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum BootLine {
    /// `reload: on (N directories)`, with the marks the pass placed.
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
#[cfg(feature = "python")]
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

/// Loads the application, emits `ready`, and supervises. Never returns.
#[cfg(feature = "python")]
pub(super) fn serve(py: Python<'_>, emitter: &Emitter, serving: Serving) -> ! {
    // `MB.23.5`: the one live-reload watcher of this boot, created **before** the
    // first import so the `preload` phase's modules and file opens are watched
    // too (§4.1 boot order). A production boot, `check` and a `dev` boot with
    // `--no-reload` never reach it: reload is a `dev`-only path, and "off" means
    // no watcher group, no mark, no report socket and no audit hook at all —
    // `--no-reload` is not a watcher that ignores events (§1, `R1`).
    let mut reload = match serving.dev.is_some() && serving.dev_reload {
        true => {
            crate::supervise::boot_reload(&serving.config, emitter, serving.reload_fault.as_deref())
        }
        false => None,
    };
    // The `preload` phase reports on source `0xffff`; its paths are `pre_fork`,
    // so a change to one takes the reload cycle (§4.4). This socket is also the
    // **gate** for the loaded-file hook: no socket, no hook, no marks (`S3`).
    let preload = match reload.as_mut() {
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
    let app = load(
        py,
        &serving.config,
        serving.declared_routes,
        emitter,
        serving.memory,
        serving.hosted,
        Boot {
            dev: serving.dev.as_ref(),
            report: preload,
            announce: serving.announce,
        },
    );
    // Boot order step 5: the parents the preload batch just added are marked,
    // then the one boot line — pretty stdout, never an event (§4.7, §4.9). A
    // marking pass that ran out of room (`ENOSPC`, or the marks cap) emits the
    // ladder's one warning and prints no boot line, and a watcher that latched
    // dead drops the reload entirely: neither may claim `reload: on`.
    //
    // The replacement half of a reload repeats neither the line nor `ready`:
    // both were the first image's, and claiming a boot that did not happen is
    // exactly what the observable contract must not do (§4.6 step 7).
    if let Some(active) = reload.as_mut() {
        let outcome = active.sync_marks();
        let stopped = active.marking_stopped();
        let dirs = active.marked_dirs();
        match after_preload_marks(outcome, stopped, dirs) {
            // `reload: on` is a claim that marking succeeded: a policy that
            // latched `ENOSPC`/the cap in `boot` already emitted its one
            // warning, and printing the line over it would be the lie the
            // ladder exists to prevent (§4.7, `T32`).
            BootLine::Announce(dirs) if serving.announce => {
                crate::supervise::announce_reload(serving.config.log_format, dirs);
            }
            BootLine::Announce(_) => {}
            BootLine::Silent => {}
            BootLine::Unavailable(why) => crate::supervise::reload_unavailable(emitter, &why),
            BootLine::Stopped(why) => {
                crate::supervise::reload_stopped(emitter, &why);
                reload = None;
            }
        }
        // §4.6 step 7: one `stat` per union path, once, so a save that landed
        // while no watcher existed is not lost. Never re-armed by a future
        // mtime.
        if let Some(since) = serving.reload_since
            && let Some(active) = reload.as_mut()
        {
            let now = active.now();
            active.policy().reconcile(since, now);
        }
    }
    if serving.announce {
        emit_ready(emitter, &serving.config, serving.declared_routes);
    }
    let mut supervisor = Supervisor::new(
        Parts {
            cfg: serving.config,
            limits: serving.limits,
            routes: serving.routes,
            #[cfg(feature = "php")]
            php: serving.php,
            egress: serving.egress,
            plans: serving.plans,
            app,
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
    supervisor.run(py)
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
