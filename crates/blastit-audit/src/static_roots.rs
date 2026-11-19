//! The declared static mounts no observed request reached (`R14`).
//!
//! A static answer is written by the scheduler and never reaches the WSGI gate,
//! so the observations cannot show whether the files were requested at all.
//! `check` prints one `stderr` line per pair and leaves the verdict alone: the
//! root exists (or the boot refused it), so this is a hint that the suite never
//! touched the mount, not a completeness error.

use blastit_manifest::{Manifest, MountTable};

use crate::obs::CheckObsV1;
/// The declared static mounts no observed request reached, in mount order, as
/// `(prefix, root)` (`R14`).
///
/// A static answer is written by the scheduler and never reaches the WSGI gate,
/// so the observations cannot show whether the files were requested at all.
/// `check` prints one stderr line per pair and leaves the verdict alone: the
/// root exists (or the boot refused it), so this is a hint that the suite never
/// touched the mount, not a completeness error. Suppressed when the suite
/// observed no request at all — [`crate::Finding::NoObservations`] already says that.
///
/// A mount is **requested** when an observed request's path the mount table
/// would answer for: longest prefix, on a segment boundary, the same rule the
/// scheduler serves with (§9.2). Nothing is `stat`ed here — whether a root
/// exists is the boot's audit (§9.6), and at `check` time a build output may not
/// have been written yet.
///
/// The result names the mount by its prefix, because that is what a `Blastfile`
/// writes and what the developer edits; a mount has no route name.
pub fn unrequested_static_roots(
    manifest: &Manifest,
    mounts: &MountTable,
    observations: &CheckObsV1,
) -> Vec<(String, String)> {
    if observations.http().is_empty() {
        return Vec::new();
    }
    let mut requested = vec![false; manifest.mounts().len()];
    for line in observations.http() {
        if let Some(matched) = mounts.lookup(&line.path) {
            requested[matched.mount] = true;
        }
    }
    manifest
        .mounts()
        .iter()
        .enumerate()
        .filter(|(index, _)| !requested[*index])
        .map(|(_, mount)| (mount.prefix.clone(), mount.policy.root.clone()))
        .collect()
}

/// The one stderr line `check` prints for a mount no observation reached
/// (`R14`).
///
/// It names the **mount's prefix**, the thing a `Blastfile` writes and the
/// developer edits, and its `root`; there is no route name to print (§9.2). The
/// leading two spaces line it up with the other `check` hints, and the line is
/// never a finding: [`unrequested_static_roots`] is a hint, so `ok` and the exit
/// code stay what the audit said.
pub fn unrequested_static_root_line(prefix: &str, root: &str) -> String {
    format!("  static \"{prefix}\": root \"{root}\" was never requested")
}
