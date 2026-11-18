//! The `static` feature's boot refusal, **executed** rather than only compiled
//! (`MB.36.2`, ADR-0016 decision 6).
//!
//! A build without the responder compiles the mount dispatch out of
//! `scheduler.rs` and refuses a manifest that declares a mount at boot with
//! [`SchedulerError::StaticNotCompiled`]. The default build (and every other
//! suite in this package) has `static` on, so that branch would otherwise be a
//! compile-only claim: this file is `#![cfg(not(feature = "static"))]`, which
//! makes the default build compile it to nothing and
//! `cargo test -p blastit-scheduler --no-default-features --test
//! static_feature_off --locked` the one command that runs it.
//!
//! Two rows, both driving [`blastit_scheduler::run`] through the same in-process
//! harness every other suite uses:
//!
//! * the deny side — a `Blastfile` with one mount is refused, and the refusal's
//!   message names the fix without naming a path;
//! * the accepted twin — the same boot with no mount is **not** refused and goes
//!   on serving. Without it a refusal that fired on every manifest would pass,
//!   and the feature-less build would be one that cannot run a view at all.

#![cfg(not(feature = "static"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use blastit_scheduler::SchedulerError;
use common::{Options, World};

/// The smallest `Blastfile` the grammar allows that declares a mount: one
/// `static` block with its one required `root`, and nothing else. `run` refuses
/// before it opens any root, so the directory need not exist.
const MOUNTED: &str = "static \"/site\" {\n  root \"public\"\n}\n";

/// The same boot with the mount removed: there is nothing to refuse.
const UNMOUNTED: &str = "view app \"/*\" {}\n";

/// How long a boot that must be refused may take to say so. Generous enough for
/// a loaded CI host, short enough that a missing refusal fails the case instead
/// of hanging the suite.
const REFUSAL_DEADLINE: Duration = Duration::from_secs(2);

/// How long the accepted twin must keep serving before "not refused" is
/// observed. The run loop ends only when its peers go away, so `Ok(())` is not
/// the shape this case can assert.
const SERVING_WINDOW: Duration = Duration::from_millis(300);

#[tokio::test]
async fn a_manifest_with_a_mount_is_refused_before_any_channel_is_served() {
    let mut world = World::start(Options::new().blastfile(MOUNTED));
    let outcome = tokio::time::timeout(REFUSAL_DEADLINE, world.finish())
        .await
        .expect("a mount must be refused at boot, not served");
    let Err(error) = outcome else {
        panic!("a manifest with a mount must not boot in a `static`-less build");
    };
    assert!(
        matches!(error, SchedulerError::StaticNotCompiled),
        "the refusal must be the missing feature, not another error: {error:?}"
    );
    // The message names the fix—and only the fix: no root, no manifest content
    // (`ARCHITECTURE.md` §8.1's rule for every user-facing sentence).
    let message = error.to_string();
    assert!(
        message.contains("--features static"),
        "the refusal must name the fix: {message}"
    );
    assert!(
        !message.contains("public") && !message.contains("/site"),
        "the refusal must not echo the manifest: {message}"
    );
}

#[tokio::test]
async fn the_same_boot_without_a_mount_is_not_refused() {
    let mut world = World::start(Options::new().blastfile(UNMOUNTED));
    let outcome = tokio::time::timeout(SERVING_WINDOW, world.finish()).await;
    assert!(
        outcome.is_err(),
        "a mount-less boot must keep serving, not return: {outcome:?}"
    );
}
