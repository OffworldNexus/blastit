//! The dev boot's loaded-file hook, across every file the stage split put it in
//! (`MB.40.4.3`, `MB.42.2.1.4`).
//!
//! `crates/blastit-view-python/tests/critical_loaded_files.rs` reads
//! `blastit/src/stage1.rs` alone — the whole stage before it became the root
//! shim plus `src/stage1/*.rs` — and asserts there exactly one
//! `LoadedTracker::install` and no dead `if false` gate. Those two assertions
//! can no longer see the stage, so a second install site (which could sit
//! outside the dev gate and therefore *run in production*) or a dead branch
//! pretending to be that gate could live in a sibling file and pass silently.
//!
//! Since the cut-over (`design/universal-distribution.md` §1.2.1) `crates/blastit`
//! has **no** interpreter, so it has no install site at all: the tracker is
//! installed inside the per-minor shim, pre-fork, by `vtable.preload`. This
//! suite therefore proves the move rather than the old ordering:
//!
//! * the scanned set is proven equal to the directory's real `.rs` set, so a new
//!   module cannot quietly escape the scan;
//! * zero install sites and no `if false` across the stage — `crates/blastit`
//!   cannot arm a tracker it has no interpreter for;
//! * exactly one install site in the shim's `boot.rs`, before the first import
//!   and after the dev-only report socket is adopted, so the instrumentation
//!   still cannot reach a production boot.
//!
//! Like the protected leg it is a token-and-order witness, not behavioural
//! proof: it cannot see control flow, so it would still pass if the install were
//! moved into a branch that never runs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The stage-1 root module.
const ROOT: &str = include_str!("../src/stage1.rs");

/// The shim's `preload`, which owns the tracker now (`MB.42.2.1.4`).
const SHIM_BOOT: &str = include_str!("../../blastit-py-shim/src/boot.rs");

/// Every child file of the stage, with the name the completeness check compares.
const FILES: [(&str, &str); 6] = [
    ("adopt.rs", include_str!("../src/stage1/adopt.rs")),
    ("boot.rs", include_str!("../src/stage1/boot.rs")),
    ("load.rs", include_str!("../src/stage1/load.rs")),
    ("serve.rs", include_str!("../src/stage1/serve.rs")),
    ("spawn.rs", include_str!("../src/stage1/spawn.rs")),
    ("unit_tests.rs", include_str!("../src/stage1/unit_tests.rs")),
];

/// A panic when `source` has no `needle`, naming what the stage lost.
fn at(source: &str, file: &str, needle: &str) -> usize {
    source.find(needle).unwrap_or_else(|| {
        panic!("{file} no longer contains {needle:?}: the preload ordering moved")
    })
}

/// Every stage-1 file's text, root first, for the crate-wide counts.
fn stage_sources() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once(("stage1.rs", ROOT)).chain(FILES.iter().copied())
}

#[test]
fn the_scan_covers_every_file_of_the_split_stage() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/stage1");
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .expect("src/stage1 is a readable directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".rs"))
        .collect();
    found.sort();

    let mut listed: Vec<String> = FILES.iter().map(|(name, _)| (*name).to_string()).collect();
    listed.sort();

    assert_eq!(
        listed, found,
        "every src/stage1/*.rs must be in this file's `FILES` list: a module that \
         is not scanned here could carry a second `LoadedTracker::install` (which \
         can run in production, outside the dev gate) or a dead `if false` branch, \
         and neither this suite nor the protected `critical_loaded_files` scan \
         would see it"
    );
}

#[test]
fn the_composition_root_arms_no_loaded_file_tracker() {
    for (file, source) in stage_sources() {
        assert_eq!(
            source.matches("LoadedTracker::install").count(),
            0,
            "{file} must arm no tracker: `crates/blastit` links no interpreter, and \
             the shim's `preload` is the one install site"
        );
        assert!(
            !source.contains("if false"),
            "{file} must not carry a dead `if false` branch"
        );
    }
}

#[test]
fn the_shim_installs_the_tracker_before_the_first_import() {
    assert_eq!(
        SHIM_BOOT.matches("LoadedTracker::install").count(),
        1,
        "exactly one tracker install site, inside the shim's `preload`"
    );
    // The call that arms it comes before the first import in `boot()`, so the
    // preload phase's own modules and file opens are reported; the socket check
    // inside `install_tracker` is what keeps a production boot silent (`S3`).
    let armed = at(SHIM_BOOT, "boot.rs", "install_tracker(py, req)");
    let first_import = at(SHIM_BOOT, "boot.rs", "import_callable(py, spec)");
    assert!(
        armed < first_import,
        "the tracker is armed **before** the application is imported"
    );
    let socket = at(SHIM_BOOT, "boot.rs", "req.report_fd >= 0");
    assert!(
        socket > armed,
        "the socket check is inside `install_tracker`, which `boot` called first"
    );
}
