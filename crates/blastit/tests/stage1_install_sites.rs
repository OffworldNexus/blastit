//! The dev boot's loaded-file hook, across every file the stage split put stage 1
//! in (`MB.40.4.3`).
//!
//! `crates/blastit-view-python/tests/critical_loaded_files.rs` reads
//! `blastit/src/stage1.rs` alone — the whole stage before it became the root
//! shim plus `src/stage1/*.rs` — and asserts there exactly one
//! `LoadedTracker::install` and no dead `if false` gate. Those two assertions
//! can no longer see the stage, so a second install site (which could sit
//! outside the dev gate and therefore *run in production*) or a dead branch
//! pretending to be that gate could live in a sibling file and pass silently.
//!
//! This suite restores the crate-wide guarantee without touching the protected
//! test:
//!
//! * the scanned set is proven equal to the directory's real `.rs` set, so a new
//!   module cannot quietly escape the scan;
//! * exactly one install site and no `if false` across the root and every file;
//! * the ordering obligations stay asserted in the file that actually holds
//!   `load` — the root shim, where the protected `include_str!` pins it.
//!
//! Like the protected leg it is a token-and-order witness, not behavioural
//! proof: it cannot see control flow, so it would still pass if the install were
//! moved into a branch that never runs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The stage-1 root module: the file that holds `load` and its install site.
const ROOT: &str = include_str!("../src/stage1.rs");

/// Every child file of the stage, with the name the completeness check compares.
const FILES: [(&str, &str); 6] = [
    ("adopt.rs", include_str!("../src/stage1/adopt.rs")),
    ("boot.rs", include_str!("../src/stage1/boot.rs")),
    ("python_env.rs", include_str!("../src/stage1/python_env.rs")),
    ("serve.rs", include_str!("../src/stage1/serve.rs")),
    ("spawn.rs", include_str!("../src/stage1/spawn.rs")),
    ("unit_tests.rs", include_str!("../src/stage1/unit_tests.rs")),
];

/// A panic when `source` has no `needle`, naming what the stage lost.
fn at(source: &str, file: &str, needle: &str) -> usize {
    source.find(needle).unwrap_or_else(|| {
        panic!("{file} no longer contains {needle:?}: the dev boot's ordering moved")
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
fn exactly_one_install_site_across_the_whole_stage() {
    let sites: Vec<&str> = stage_sources()
        .filter(|(_, source)| source.contains("LoadedTracker::install"))
        .map(|(name, _)| name)
        .collect();

    assert_eq!(
        sites,
        ["stage1.rs"],
        "exactly one tracker install site, in the file that holds `load`: a second \
         copy could sit outside the `if dev.is_some()` gate and keep this suite — and \
         the protected source scan — green while running in production"
    );

    for (file, source) in stage_sources() {
        assert_eq!(
            source.matches("LoadedTracker::install").count(),
            usize::from(file == "stage1.rs"),
            "exactly one install site in {file}, so a copy cannot hide behind a \
             shared helper"
        );
    }
}

#[test]
fn no_dead_branch_pretends_to_be_the_dev_gate() {
    for (file, source) in stage_sources() {
        assert!(
            !source.contains("if false"),
            "{file} must not carry a dead `if false` branch: one would let an install \
             site look gated while every boot runs it"
        );
    }
}

#[test]
fn the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup() {
    // `ROOT` is the file that holds `load`, which is where the protected
    // `include_str!` pins the ordering; the counts above keep the rest of the
    // stage from adding a second site.
    assert!(
        ROOT.contains("fn load("),
        "stage1.rs must still hold `load`: the protected `critical_loaded_files` \
         test reads this exact file"
    );

    // One install site: a second copy could otherwise sit outside the gate and
    // keep this test green while running in production.
    assert_eq!(
        ROOT.matches("LoadedTracker::install").count(),
        1,
        "exactly one tracker install site, inside the dev-only gate"
    );
    let install = at(ROOT, "stage1.rs", "LoadedTracker::install");
    let gate = at(ROOT, "stage1.rs", "let loaded = if dev.is_some() {");
    assert!(
        gate < install,
        "the install is inside the `if dev.is_some()` gate, not before it"
    );
    let gated = &ROOT[gate..install];
    assert!(
        !gated.contains("else"),
        "the install is in the gate's then-branch, not its else-branch"
    );
    assert!(
        install < at(ROOT, "stage1.rs", "import_callable(py, spec)"),
        "the tracker is installed before the application is imported, or the preload \
         closure is missed"
    );
    assert!(
        install < at(ROOT, "stage1.rs", "for module in &config.preload"),
        "the tracker is installed before the `preload` list is imported, or a \
         preloaded module is missed"
    );
    let harvest = at(ROOT, "stage1.rs", "loaded.harvest(py)");
    assert_eq!(
        ROOT.matches("loaded.harvest(py)").count(),
        1,
        "one baseline harvest, so the cost is paid once"
    );
    assert!(
        at(ROOT, "stage1.rs", "config.warmup.as_deref()") < harvest,
        "the harvest runs after `BLASTIT_WARMUP`, so the warmed closure is in the \
         boot batch"
    );
    assert!(
        harvest < at(ROOT, "stage1.rs", "py.run(CLOSE_AND_COLLECT, None, None)"),
        "the harvest runs before the heap is immortalised"
    );
}
