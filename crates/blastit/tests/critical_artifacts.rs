//! `MB.36.11` — the two artifact facts `MB.36` promised that nothing asserted.
//!
//! `docs/testing/AUDIT-MB.36.md` §5 lists R3 and R12 as the only accepted
//! requirements with no automated oracle, and both are facts about a *file*
//! rather than about a type, so neither has a compile-time witness:
//!
//! * **R12 — the shipped wheel's extension links no interpreter and no SAPI.**
//!   `scripts/build-wheel.sh` extracts each `blastit/_blastit*.so` and runs
//!   `scripts/check-portable-elf.sh --allow-origin-rpath` on it — once, during
//!   the build. A wheel is a *published* file: it can be rebuilt long after that
//!   script ran (a new dependency, a feature re-enabled, a hand-driven
//!   `maturin`), and `pyo3/extension-module` is the only thing keeping the
//!   extension off `libpython` (`LESSONS.md`, `MB.17`). This suite reads the
//!   same file a `pip install blastit` loads and applies the script's rule set:
//!   no `DT_NEEDED` on `libpython*`, none on `libphp*`, and no
//!   `DT_RUNPATH`/`DT_RPATH` that names a build host (a purely `$ORIGIN`-relative
//!   one is the musllinux `blastit.libs/` repair and stays allowed).
//! * **R3 — the deleted transport trait stays deleted.** `ADR-0016` decision 2
//!   deletes `WorkerRuntime`, `RequestIo`, `RequestEnd` and
//!   `RequestIo::flush_response`; `OutputStream`/`OUTPUT_LINE_CAP` move to
//!   `blastit-view::stream` and `serve_forever` takes the concrete
//!   `PhpEmbedRuntime`. A deleted name leaves nothing for the compiler to
//!   complain about, so the guard is a source scan — over the tree it walks,
//!   never `git ls-files`, because an *untracked* file is exactly what a
//!   re-introduced seam looks like before its first commit (`LESSONS.md`,
//!   `MB.23.3`).
//!
//! # The one exemption the scan carries: a recorded deviation from `R3`
//!
//! `R3`, design §4.2.1 and `ADR-0016` decision 2 — whose title is *"The worker
//! trait is deleted, not re-homed."* — say `RequestIo`, `RequestEnd` and
//! `flush_response` stop existing. The tree **deviates**: `MB.36.5` moved those
//! three into `crates/blastit-view-php/src/worker.rs`, the crate that is their
//! only implementer, because that worker needs a write contract for the
//! response it is serving, and re-exported the two types from that crate's
//! `src/lib.rs`. That is a *recorded* deviation from `R3`, not a reading of it —
//! `MB.36.5`'s `RESULT.md` records the move and `MB.36.11`'s own `RESULT.md`
//! records it as a deviation, and §4.2.1, §4.2.3 and §7 carry it after
//! `MB.36.11` corrected them to the tree. This suite does not pretend the
//! deviation is the requirement: it refuses everything the deviation does not
//! cover. `WorkerRuntime` and `flush_request` are refused in **every** file under
//! `crates/`; the per-request view is refused everywhere except exactly those
//! two files. The exemption is pinned from both sides: the declarations it covers
//! must still be there (a stale exemption is a failure, not a licence), and its
//! *path* is proved meaningful by planting `RequestIo` in another fixture path
//! and requiring a refusal.
//!
//! # Failing closed, without a new dependency
//!
//! `crates/blastit/Cargo.toml` is out of this node's `paths`, so the wheel is
//! read with `std` alone: a ZIP central-directory reader, an RFC 1951 inflater
//! (stored, fixed and dynamic Huffman blocks, with three embedded vectors as its
//! own fixture) and an ELF64/ELF32 dynamic-section reader. Shelling out to
//! `readelf`/`unzip` would have made a host without those tools either a silent
//! pass (the trap `LESSONS.md` records twice) or a red gate on a machine whose
//! only sin is a slim image; a reader that is ours cannot fail open. Every
//! unparseable byte string is a **red test**: a corrupt zip, a zip64 wheel, an
//! unknown compression method, a truncated ELF, a missing `PT_DYNAMIC` and a
//! missing `DT_STRTAB` are all errors, never skips.
//!
//! # Both directions
//!
//! * `dist/` with no wheel for the workspace version prints **one** line and
//!   passes, so `make check` stays offline and Docker-free — and exits non-zero
//!   when `BLASTIT_REQUIRE_WHEEL=1`, so CI can demand the artifact.
//! * `the_wheel_without_an_extension_is_refused` builds wheels whose extension
//!   is missing, links `libpython`, links `libphp`, or carries a build-host
//!   `RUNPATH`, and requires each to be refused — with a `$ORIGIN`-relative
//!   wheel and a plain `libc` extension as the allowed twins, so the check is
//!   never unconditionally red.
//! * `the_trait_scan_catches_a_planted_name` proves the source scan against a
//!   fixture tree: clean, planted, planted in a path other than the exemption's,
//!   and clean again for the exempted path itself.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The environment variable that turns "no wheel" from a pass into a failure.
const REQUIRE_WHEEL: &str = "BLASTIT_REQUIRE_WHEEL";

// ---------------------------------------------------------------------------
// R12 — the wheel's extension
// ---------------------------------------------------------------------------

/// The published artifact's own ELF, against `scripts/check-portable-elf.sh`.
#[test]
fn the_wheel_extension_links_no_interpreter_and_no_php() -> Result<(), String> {
    let root = workspace_root()?;
    let version = workspace_version(&root)?;
    let wheel = match select_wheel(&root, &version)? {
        Some(wheel) => wheel,
        None => {
            // One line, and the only thing that can turn this back into a
            // failure. `make check` is offline and Docker-free on a clean
            // checkout; a release gate sets BLASTIT_REQUIRE_WHEEL=1.
            println!(
                "no blastit-{version} wheel in {}: the R12 check needs a \
                 `scripts/build-wheel.sh` build (set {REQUIRE_WHEEL}=1 to require the artifact)",
                root.join("dist").display()
            );
            return if env::var(REQUIRE_WHEEL).as_deref() == Ok("1") {
                Err(format!(
                    "{REQUIRE_WHEEL}=1 is set but {} holds no blastit-{version} wheel: build it \
                     with `scripts/build-wheel.sh` (`make docs-test` and the CI wheel legs do), \
                     then re-run `cargo test -p blastit --test critical_artifacts --locked`",
                    root.join("dist").display()
                ))
            } else {
                Ok(())
            };
        }
    };
    let extensions = extension_objects(&wheel)?;
    let mut checked = Vec::new();
    for (member, elf) in &extensions {
        let facts = assert_portable(&format!("{}:{member}", wheel.display()), elf)?;
        checked.push(format!("{member} [{}]", facts.needed.join(" ")));
    }
    println!(
        "ok   {}: {} extension(s) with no libpython/libphp NEEDED and no build-host search path: {}",
        wheel.display(),
        checked.len(),
        checked.join(", ")
    );
    Ok(())
}

/// A wheel the check must refuse, and the two shapes it must accept.
#[test]
fn the_wheel_without_an_extension_is_refused() -> Result<(), String> {
    let scratch = Scratch::new("wheel")?;

    // (1) A wheel with no `blastit/_blastit*.so` at all: the fixture the node
    //     asks for, refused by name instead of silently accepted. A real wheel
    //     built with `python` off has exactly this shape.
    let bare = scratch.path("bare.whl");
    write_zip(
        &bare,
        &[
            ("blastit/__init__.py", b"# a wheel with no extension\n"),
            ("blastit-0.2.0.dist-info/WHEEL", b"Wheel-Version: 1.0\n"),
        ],
    )?;
    match extension_objects(&bare) {
        Ok(found) => {
            return Err(format!(
                "{} carries no blastit/_blastit*.so, so there is no extension to check, but the \
                 reader accepted it and returned {} member(s): a wheel without the extension is a \
                 build that lost its python capability",
                bare.display(),
                found.len()
            ));
        }
        Err(error) => assert!(
            error.contains("carries no blastit/_blastit*.so"),
            "the refusal must name the missing member, got: {error}"
        ),
    }

    // (2) The allowed twin: a wheel whose extension is a plain `libc` object.
    //     Without this, a reader that refused everything would look green here.
    let good = scratch.path("good.whl");
    let plain = synthetic_elf(&["libc.so.6", "libm.so.6"], None);
    write_zip(
        &good,
        &[(
            "blastit/_blastit.cpython-314-x86_64-linux-gnu.so",
            plain.as_slice(),
        )],
    )?;
    let extensions = extension_objects(&good)?;
    assert_eq!(extensions.len(), 1, "one extension member");
    assert_portable("good.whl:the extension", &extensions[0].1)?;

    // (3) A `$ORIGIN`-relative search path is the musllinux repair and passes,
    //     exactly as `scripts/check-portable-elf.sh --allow-origin-rpath` does.
    let origin = synthetic_elf(&["libc.so.6"], Some("$ORIGIN/../blastit.libs"));
    assert_portable("origin:the extension", &origin)?;

    // (4) `libpython`: the capability must never be a link dependency.
    let python = synthetic_elf(&["libpython3.14.so.1.0", "libc.so.6"], None);
    expect_refusal("linking libpython", &python, "links an interpreter")?;

    // (5) `libphp`: the SAPI is the host's, reached through a verified
    //     descriptor, so a `NEEDED` is a second copy nothing can pin.
    let php = synthetic_elf(&["libphp8.4.so", "libc.so.6"], None);
    expect_refusal("linking libphp", &php, "links libphp")?;

    // (6) A build-host path — the uv cache a `PYO3_PYTHON` build writes.
    let host = synthetic_elf(&["libc.so.6"], Some("/home/someone/.local/share/uv/lib"));
    expect_refusal("a build-host RUNPATH", &host, "library search path")?;

    // (7) An ELF this reader cannot parse is a red test, never a skip.
    let mut truncated = synthetic_elf(&["libc.so.6"], None);
    truncated.truncate(40);
    match assert_portable("truncated:the extension", &truncated) {
        Ok(_) => return Err(String::from("a 40-byte ELF header must not be accepted")),
        Err(error) => assert!(
            error.contains("truncated:the extension"),
            "an unparseable ELF must name itself in the refusal, got: {error}"
        ),
    }
    let garbage = plain_zip_member(&scratch, &[0u8; 64])?;
    match assert_portable("garbage:the extension", &garbage) {
        Ok(_) => return Err(String::from("a non-ELF member must not be accepted")),
        Err(error) => assert!(
            error.contains("ELF magic"),
            "a member with no ELF magic must be refused by name, got: {error}"
        ),
    }

    // (8) `DT_STRSZ` bounds the whole table, not just the first byte of each
    //     string: a name that starts inside it and ends outside it is refused,
    //     never read out of bytes the file says are not strings.
    let understated = synthetic(&Synthetic {
        class: 2,
        data: 1,
        needed: &["libc.so.6"],
        search: None,
        strsz: Some(5),
    });
    expect_refusal("an understated DT_STRSZ", &understated, "DT_STRSZ")?;

    // (9) A file with no `DT_NEEDED` at all is refused: a reader that found
    //     nothing never read the section, and would certify every file.
    let no_needed = synthetic(&Synthetic {
        class: 2,
        data: 1,
        needed: &[],
        search: None,
        strsz: None,
    });
    expect_refusal("an ELF with no DT_NEEDED", &no_needed, "names no DT_NEEDED")?;

    // (10) `ELFCLASS32` is parsed, not skipped: the same fixture is accepted
    //      with `libc` and refused with `libpython`, for the same reason.
    let elf32 = synthetic(&Synthetic {
        class: 1,
        data: 1,
        needed: &["libc.so.6"],
        search: None,
        strsz: None,
    });
    assert_portable("elf32:the extension", &elf32)?;
    let elf32_python = synthetic(&Synthetic {
        class: 1,
        data: 1,
        needed: &["libpython3.14.so.1.0"],
        search: None,
        strsz: None,
    });
    expect_refusal(
        "a 32-bit ELF linking libpython",
        &elf32_python,
        "links an interpreter",
    )?;

    // (11) A big-endian file cannot be read, so it cannot be certified.
    let big_endian = synthetic(&Synthetic {
        class: 2,
        data: 2,
        needed: &["libc.so.6"],
        search: None,
        strsz: None,
    });
    expect_refusal("a big-endian ELF", &big_endian, "not little-endian")?;

    // (12) A `DT_NEEDED` whose only text is the empty string names nothing: the
    //      section would resolve to `[""]` and certify clean while proving
    //      nothing, so an empty dynamic string is refused instead.
    let empty_name = synthetic(&Synthetic {
        class: 2,
        data: 1,
        needed: &[""],
        search: None,
        strsz: None,
    });
    expect_refusal(
        "a DT_NEEDED with an empty name",
        &empty_name,
        "names no library",
    )?;
    Ok(())
}

/// `assert_portable` must refuse `elf`, with `needle` in the sentence.
fn expect_refusal(what: &str, elf: &[u8], needle: &str) -> Result<(), String> {
    match assert_portable("fixture:the extension", elf) {
        Ok(_) => Err(format!(
            "{what} was accepted: the R12 rule set is not enforced"
        )),
        Err(error) => {
            assert!(
                error.contains(needle),
                "the refusal for {what} must name the rule ({needle:?}), got: {error}"
            );
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// R3 — the deleted transport trait
// ---------------------------------------------------------------------------

/// `WorkerRuntime`, `RequestIo`, `RequestEnd`, `flush_response` stay gone.
#[test]
fn the_deleted_transport_trait_stays_deleted() -> Result<(), String> {
    let crates = workspace_root()?.join("crates");
    assert_is_dir(&crates)?;

    // The names: what is refused, why, and the only files allowed to name it
    // (relative to `crates/`).
    let rules = [
        Rule {
            name: "WorkerRuntime",
            why: "the deleted worker trait (`ADR-0016` decision 2)",
            allowed: &[],
        },
        Rule {
            name: "flush_request",
            why: "`R3`: the sender flushes, the runtime never does",
            allowed: &[],
        },
        Rule {
            name: "RequestIo",
            why: "the deleted per-request trait (`ADR-0016` decision 2)",
            allowed: PHP_SOLE_IMPLEMENTER,
        },
        Rule {
            name: "RequestEnd",
            why: "the deleted per-request end type (`ADR-0016` decision 2)",
            allowed: PHP_SOLE_IMPLEMENTER,
        },
        Rule {
            name: "flush_response",
            why: "`RequestIo::flush_response`, deleted with the trait",
            allowed: PHP_SOLE_IMPLEMENTER,
        },
    ];
    let findings = scan(&crates, &rules)?;
    assert!(
        findings.is_empty(),
        "the deleted transport seam is back under {}:\n  {}\n  fix: `ADR-0016` decision 2 deletes \
         `WorkerRuntime`/`RequestIo`/`RequestEnd`/`flush_response`; the `view.chan` frame \
         vocabulary is the contract, so a new file naming them is a trait boundary that was never \
         drawn. Only the PHP worker (`{}`) may carry the per-request view, because it is its only \
         implementer.",
        crates.display(),
        findings.join("\n  "),
        PHP_SOLE_IMPLEMENTER.join("`, `"),
    );

    assert_transport_layout(&crates)
}

/// `OutputStream`/`OUTPUT_LINE_CAP` are the transport's, and the seam has one home.
fn assert_transport_layout(crates: &Path) -> Result<(), String> {
    let stream = "blastit-view/src/stream.rs";
    // Both of `R3`'s moves are pinned **once**: a second declaration is a copy the
    // PHP worker could drift against, and the re-export in
    // `crates/blastit-view/src/lib.rs` is not a declaration (it names the item,
    // not `pub enum`/`pub const`).
    for declaration in ["pub enum OutputStream", "pub const OUTPUT_LINE_CAP"] {
        let declarations = files_containing(crates, declaration)?;
        if declarations.len() != 1 || declarations[0] != stream {
            return Err(format!(
                "`{declaration}` must be declared once, in `crates/{stream}` (`R3`: it moved \
                 there so the PHP worker formats `app.output` lines through the transport), but \
                 it is declared in: {}",
                if declarations.is_empty() {
                    String::from("no file")
                } else {
                    declarations.join(", ")
                }
            ));
        }
    }

    // The module the trait lived in is deleted, not emptied.
    let runtime = crates.join("blastit-view/src/runtime.rs");
    if runtime.exists() {
        return Err(format!(
            "{} exists again: `ADR-0016` decision 2 deletes `runtime.rs` (the `WorkerRuntime` \
             trait's home), so recreating the module re-opens the seam",
            runtime.display()
        ));
    }

    // The trait's replacement is the concrete runtime, and the exemption this
    // suite carries must still describe the file it exempts.
    let worker = crates.join("blastit-view-php/src/worker.rs");
    let text = fs::read_to_string(&worker)
        .map_err(|error| format!("cannot read {}: {error}", worker.display()))?;
    assert!(
        text.contains("runtime: PhpEmbedRuntime"),
        "`serve_forever` must take the concrete `PhpEmbedRuntime` (`R3`), and {} no longer \
         declares `runtime: PhpEmbedRuntime`",
        worker.display()
    );
    for required in [
        "pub trait RequestIo",
        "pub struct RequestEnd",
        "fn flush_response",
    ] {
        if !text.contains(required) {
            return Err(format!(
                "{} is exempt from the scan for `RequestIo`/`RequestEnd`/`flush_response`, but it \
                 no longer declares `{required}`: the exemption is stale, and a stale exemption is \
                 how a guard quietly stops guarding. Delete it from `PHP_SOLE_IMPLEMENTER`.",
                worker.display()
            ));
        }
    }
    Ok(())
}

/// The scan has to go red on a planted name, in the right place and only there.
#[test]
fn the_trait_scan_catches_a_planted_name() -> Result<(), String> {
    let scratch = Scratch::new("scan")?;
    let rules = [
        Rule {
            name: "WorkerRuntime",
            why: "the deleted worker trait",
            allowed: &[],
        },
        Rule {
            name: "RequestIo",
            why: "the deleted per-request trait",
            allowed: PHP_SOLE_IMPLEMENTER,
        },
    ];

    // (1) A clean tree: the scan is not unconditionally red.
    scratch.write("src/lib.rs", b"pub enum OutputStream { Stdout }\n")?;
    assert!(
        scan(&scratch.root, &rules)?.is_empty(),
        "a tree that names no forbidden identifier must scan clean"
    );

    // (2) A planted name is found, with its path, line and text.
    scratch.write(
        "src/planted.rs",
        b"//! a file nobody committed\npub trait WorkerRuntime {\n    fn serve(&self);\n}\n",
    )?;
    let found = scan(&scratch.root, &rules)?;
    assert_eq!(found.len(), 1, "one planted name, one finding: {found:?}");
    assert!(
        found[0].contains("planted.rs:2") && found[0].contains("WorkerRuntime"),
        "the finding must name the path, the line and the identifier, got: {}",
        found[0]
    );

    // (3) The exemption is a *path*: the same name in another file is refused.
    scratch.write(
        "blastit/tests/other.rs",
        b"fn f(io: &mut dyn RequestIo) {}\n",
    )?;
    let found = scan(&scratch.root, &rules)?;
    assert_eq!(
        found.len(),
        2,
        "the planted names, and only those: {found:?}"
    );
    assert!(
        found.iter().any(|finding| finding.contains("other.rs:1")),
        "`RequestIo` outside the PHP worker must be refused, got: {found:?}"
    );

    // (4) ...and the exempted path itself passes, so the tree is clean again.
    scratch.write(
        "blastit-view-php/src/worker.rs",
        b"pub trait RequestIo {}\npub struct RequestEnd;\n",
    )?;
    let found = scan(&scratch.root, &rules)?;
    assert!(
        found.iter().all(|finding| !finding.contains("worker.rs")),
        "the sole implementer must not be refused for a name it owns, got: {found:?}"
    );
    assert_eq!(
        found.len(),
        2,
        "only the two planted names remain refused: {found:?}"
    );
    Ok(())
}

/// The files allowed to name the per-request view, relative to `crates/`.
///
/// A **recorded deviation from `R3`** (§4.2.1, §4.2.3, §7, `ADR-0016` decision
/// 2): `MB.36.5` moved `RequestIo`/`RequestEnd`/`flush_response` out of the
/// deleted `blastit-view/src/runtime.rs` into the crate that is their only
/// implementer — which needs a write contract for the response it is serving —
/// and re-exports the two types from that crate's root. `ADR-0016` decision 2
/// says “the worker trait is deleted, not re-homed”, so this list is the
/// deviation made explicit and *narrow*: `WorkerRuntime` and `flush_request`
/// have no exemption at all, and `assert_transport_layout` refuses a stale
/// entry here.
const PHP_SOLE_IMPLEMENTER: &[&str] = &[
    "blastit-view-php/src/lib.rs",
    "blastit-view-php/src/worker.rs",
];

/// The guard names the deleted names, so it is the one file the scan exempts
/// itself from. Its own negative twin is `the_trait_scan_catches_a_planted_name`,
/// which plants the names in a fixture tree, so this cannot hide a regression.
const GUARD_FILE: &str = "blastit/tests/critical_artifacts.rs";

/// One forbidden identifier and the files allowed to contain it.
struct Rule {
    /// The identifier as it is spelled in source.
    name: &'static str,
    /// What deleting it was for, quoted in the refusal.
    why: &'static str,
    /// Paths relative to the scanned root that may name it.
    allowed: &'static [&'static str],
}

/// How many occurrences of one name in one file are reported.
const MAX_FINDINGS_PER_FILE: usize = 5;

/// The deepest directory the walk follows before it calls a loop a failure.
const MAX_DEPTH: usize = 32;

/// Every file under `root` that names a forbidden identifier, as findings.
///
/// `root` is a parameter rather than a constant so the negative twin can point
/// the same code at a fixture tree. The walk reads every regular file, not just
/// `.rs`: the design's oracle is `grep -rn` over `crates/`, and a name that only
/// survives in a `.md` or a fixture is still a name a reader will copy. An
/// unreadable file, an unreadable directory, a device node and a symlink loop
/// are all errors — a tree that cannot be walked cannot be certified.
fn scan(root: &Path, rules: &[Rule]) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    walk(root, &mut files, 0)?;
    files.sort();
    let mut findings = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .map_err(|error| {
                format!(
                    "{} is not under {}: {error}",
                    path.display(),
                    root.display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes =
            fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        for rule in rules {
            let exempt =
                relative == GUARD_FILE || rule.allowed.iter().any(|allowed| *allowed == relative);
            if exempt {
                continue;
            }
            let needle = rule.name.as_bytes();
            let mut from = 0;
            let mut reported = 0;
            while reported < MAX_FINDINGS_PER_FILE {
                let tail = bytes.get(from..).unwrap_or(&[]);
                let Some(at) = find(tail, needle) else {
                    break;
                };
                let at = from + at;
                let (line, text) = line_of(&bytes, at);
                findings.push(format!(
                    "{path}:{line}: `{name}` ({why}): {text}",
                    path = path.display(),
                    name = rule.name,
                    why = rule.why,
                ));
                reported += 1;
                from = at + needle.len();
            }
        }
    }
    Ok(findings)
}

/// The first index of `needle` in `haystack`, or `None`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The 1-based line number and text of the byte at `at`.
fn line_of(bytes: &[u8], at: usize) -> (usize, String) {
    let head = bytes.get(..at).unwrap_or(bytes);
    let line = 1 + head.iter().filter(|byte| **byte == b'\n').count();
    let start = head
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let tail = bytes.get(at..).unwrap_or_default();
    let end = tail
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |index| at + index);
    (
        line,
        String::from_utf8_lossy(&bytes[start..end])
            .trim()
            .to_owned(),
    )
}

/// Every regular file under `dir`, depth-bound so a symlink loop is an error.
fn walk(dir: &Path, files: &mut Vec<PathBuf>, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "{} is nested deeper than {MAX_DEPTH} directories: a symlink loop cannot be walked, \
             and a tree that cannot be walked cannot be certified",
            dir.display()
        ));
    }
    let entries =
        fs::read_dir(dir).map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot read an entry of {}: {error}", dir.display()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if kind.is_dir() {
            // A `target/` or `.git/` inside `crates/` is a build tree, not
            // source, and walking one would certify the build machine's output.
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "target" || name == ".git" {
                continue;
            }
            walk(&path, files, depth + 1)?;
        } else if kind.is_file() {
            files.push(path);
        } else if kind.is_symlink() {
            // Follow it: a symlink to a source file is source. A dangling one is
            // a file the scan cannot read, which is an error rather than a gap.
            let target = fs::metadata(&path)
                .map_err(|error| format!("cannot follow {}: {error}", path.display()))?;
            if target.is_dir() {
                walk(&path, files, depth + 1)?;
            } else {
                files.push(path);
            }
        } else {
            return Err(format!(
                "{} is neither a file, a directory nor a symlink: the scan cannot decide what to \
                 read there",
                path.display()
            ));
        }
    }
    Ok(())
}

/// The files (relative to `root`) that contain `needle`, sorted.
fn files_containing(root: &Path, needle: &str) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    walk(root, &mut files, 0)?;
    let mut found = Vec::new();
    for path in files {
        let bytes =
            fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|error| {
                format!(
                    "{} is not under {}: {error}",
                    path.display(),
                    root.display()
                )
            })?
            .to_string_lossy()
            .into_owned();
        if relative != GUARD_FILE && find(&bytes, needle.as_bytes()).is_some() {
            found.push(relative);
        }
    }
    found.sort();
    Ok(found)
}

/// A directory that is removed when the test ends, panic or not.
struct Scratch {
    /// The directory itself.
    root: PathBuf,
}

impl Scratch {
    /// A fresh directory named after this process and `label`.
    fn new(label: &str) -> Result<Self, String> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        // `CARGO_TARGET_TMPDIR` is cargo's own scratch space for integration
        // tests; the fallback is only reached outside cargo.
        let base = option_env!("CARGO_TARGET_TMPDIR").map_or_else(env::temp_dir, PathBuf::from);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = base.join(format!(
            "blastit-artifacts-{}-{label}-{serial}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root)
                .map_err(|error| format!("cannot clear {}: {error}", root.display()))?;
        }
        fs::create_dir_all(&root)
            .map_err(|error| format!("cannot create {}: {error}", root.display()))?;
        Ok(Self { root })
    }

    /// `rel`, resolved inside the scratch directory.
    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Write `bytes` at `rel`, creating the parent directories.
    fn write(&self, rel: &str, bytes: &[u8]) -> Result<PathBuf, String> {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        fs::write(&path, bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        Ok(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// ---------------------------------------------------------------------------
// The wheel: which file is the artifact, and what it contains
// ---------------------------------------------------------------------------

/// The workspace root: two directories above this crate's manifest.
fn workspace_root() -> Result<PathBuf, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            format!(
                "{} has no grandparent to be a workspace root",
                manifest.display()
            )
        })?
        .to_path_buf();
    assert_is_dir(&root)?;
    Ok(root)
}

/// Fail when `path` is not a directory.
fn assert_is_dir(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(format!("{} is not a directory", path.display()))
    }
}

/// `[workspace.package] version` — the version every artifact of this tree has.
///
/// `python/tests/runnable_docs/harness.py`'s `planned_version()` reads the same
/// line, and the wheel is named after it.
fn workspace_version(root: &Path) -> Result<String, String> {
    let path = root.join("Cargo.toml");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut in_workspace_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_workspace_package = line == "[workspace.package]";
            continue;
        }
        if !in_workspace_package {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "version" {
            continue;
        }
        let value = value.trim();
        let version = value
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or_default();
        if version.is_empty() || !version.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(format!(
                "{}: `[workspace.package] version` is not a quoted `x.y.z`: {line}",
                path.display()
            ));
        }
        return Ok(version.to_owned());
    }
    Err(format!(
        "{} has no `[workspace.package] version`, so the wheel the check reads cannot be named",
        path.display()
    ))
}

/// The one wheel for `version` in `dist/`, or `None` when there is none.
///
/// `harness.py`'s rule: the workspace version decides, never mtime and never the
/// alphabet, and two wheels for one version are a failure rather than a guess —
/// the check cannot tell which of them a user installs.
fn select_wheel(root: &Path, version: &str) -> Result<Option<PathBuf>, String> {
    let dist = root.join("dist");
    let prefix = format!("blastit-{version}-");
    let mut candidates = Vec::new();
    let mut others = Vec::new();
    match fs::read_dir(&dist) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|error| {
                    format!("cannot read an entry of {}: {error}", dist.display())
                })?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".whl") || !name.starts_with("blastit-") {
                    continue;
                }
                if name.starts_with(&prefix) {
                    candidates.push(entry.path());
                } else {
                    others.push(name);
                }
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", dist.display())),
    }
    candidates.sort();
    others.sort();
    match candidates.len() {
        0 => {
            if !others.is_empty() {
                println!(
                    "{} holds no blastit-{version} wheel; it holds {}: the version decides which \
                     wheel is the artifact",
                    dist.display(),
                    others.join(", ")
                );
            }
            Ok(None)
        }
        1 => Ok(candidates.pop()),
        count => {
            let names: Vec<String> = candidates
                .iter()
                .map(|path| {
                    path.file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
                })
                .collect();
            Err(format!(
                "{} holds {count} wheels for version {version}: {}. The check cannot tell which \
                 one is the artifact — delete the ones this run did not build (`dist/` is \
                 generated) and re-run `scripts/build-wheel.sh`",
                dist.display(),
                names.join(", ")
            ))
        }
    }
}

/// The wheel's `blastit/_blastit*.so` members, inflated, with their names.
///
/// The member rule is `scripts/build-wheel.sh`'s: `blastit/`, an `_blastit`
/// infix and a `.so` suffix. No such member is a failure, never an empty list:
/// that is the wheel a `python`-off build produces, and it cannot serve a WSGI
/// route at all.
fn extension_objects(wheel: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let label = wheel.display().to_string();
    let archive = fs::read(wheel).map_err(|error| format!("cannot read {label}: {error}"))?;
    let members = zip_members(&archive, &label)?;
    let mut objects = Vec::new();
    for member in members {
        let wanted = member.name.starts_with("blastit/")
            && member.name.contains("_blastit")
            && member.name.ends_with(".so");
        if !wanted {
            continue;
        }
        let bytes = zip_member(&archive, &member, &label)?;
        objects.push((member.name, bytes));
    }
    if objects.is_empty() {
        return Err(format!(
            "{label} carries no blastit/_blastit*.so: there is no extension to check"
        ));
    }
    Ok(objects)
}

/// The R12 rule set, as `scripts/check-portable-elf.sh` applies it.
///
/// `Ok` carries the entries the reader resolved, so a caller can print them and
/// a reader that silently found nothing cannot pass for a certified file. `Err`
/// is either a violation (with the script's sentence and its fix) or an ELF this
/// reader cannot parse. There is no third outcome: an unreadable artifact is red.
fn assert_portable(label: &str, elf: &[u8]) -> Result<DynamicFacts, String> {
    let facts = dynamic_facts(elf, label)?;
    let mut problems = Vec::new();
    for needed in &facts.needed {
        if needed.contains("libpython") {
            problems.push(format!(
                "error: {label} links an interpreter, so it only runs where CPython is installed:\n  \
                 NEEDED [{needed}]\n  fix: build the wheel with `pyo3/extension-module` \
                 (`scripts/build-wheel.sh` does). A Python extension is loaded by the interpreter \
                 that imports it and must not link `libpython`."
            ));
        }
        if needed.contains("libphp") {
            problems.push(format!(
                "error: {label} links libphp, so it carries its own PHP instead of loading the \
                 host's:\n  NEEDED [{needed}]\n  fix: build without a crate that links it; a view \
                 `dlopen`s libphp (`ADR-0010` §1), so the wheel's extension reaches PHP through \
                 the verified descriptor and never through a link."
            ));
        }
    }
    for (kind, value) in &facts.search_paths {
        if carries_build_host_path(value) {
            problems.push(format!(
                "error: {label} carries a library search path from its build host:\n  {kind} \
                 [{value}]\n  fix: the manylinux wheel links the system C library and needs no \
                 search path; only a `$ORIGIN`-relative one (the musllinux `blastit.libs/` \
                 repair) is acceptable, and an interpreter cache here means the python capability \
                 leaked into a shipped file."
            ));
        }
    }
    if problems.is_empty() {
        // Anti-vacuity: a shared object the wheel ships resolves at least the C
        // library, so a reader that found no `DT_NEEDED` at all has not read the
        // dynamic section and must not report a pass.
        if facts.needed.is_empty() {
            return Err(format!(
                "{label}: the dynamic section names no DT_NEEDED at all, which no extension the \
                 wheel ships does: this reader did not read the section, so the check would pass \
                 without looking and is refused instead"
            ));
        }
        Ok(facts)
    } else {
        Err(problems.join("\n"))
    }
}

/// The script's relaxation: strip `$ORIGIN<non-separator>`, then refuse `$`/`/`.
fn carries_build_host_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut at = 0;
    let mut rest = String::new();
    while at < bytes.len() {
        if value[at..].starts_with("$ORIGIN") {
            at += "$ORIGIN".len();
            while at < bytes.len() && bytes[at] != b' ' && bytes[at] != b':' {
                at += 1;
            }
        } else {
            let Some(character) = value[at..].chars().next() else {
                break;
            };
            rest.push(character);
            at += character.len_utf8();
        }
    }
    rest.contains('$') || rest.contains('/')
}

// ---------------------------------------------------------------------------
// A dependency-free ELF reader: the dynamic section, the way `readelf -d` sees it
// ---------------------------------------------------------------------------

/// What `readelf -d` prints for the three tags this check cares about.
#[derive(Debug, Default)]
struct DynamicFacts {
    /// `DT_NEEDED` entries, in order.
    needed: Vec<String>,
    /// `DT_RPATH`/`DT_RUNPATH` entries, with the tag that carried them.
    search_paths: Vec<(&'static str, String)>,
}

/// PT_LOAD, PT_DYNAMIC and the `DT_*` tags this reader resolves.
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_STRTAB: u64 = 5;
const DT_STRSZ: u64 = 10;
const DT_RPATH: u64 = 15;
const DT_RUNPATH: u64 = 29;

/// Read the dynamic section of an ELF32/ELF64 little-endian file.
///
/// Big-endian, a bad class, no program headers, a program header table outside
/// the file, no `PT_DYNAMIC`, a `DT_NEEDED` with no `DT_STRTAB` and a string
/// outside the file are all errors: each one would otherwise let the check pass
/// by failing to look.
fn dynamic_facts(elf: &[u8], label: &str) -> Result<DynamicFacts, String> {
    if elf.len() < 52 {
        return Err(format!(
            "{label}: {} bytes is shorter than an ELF header (52), so the file cannot be checked",
            elf.len()
        ));
    }
    if &elf[..4] != b"\x7fELF" {
        return Err(format!(
            "{label}: no ELF magic at the start of the member, so it is not the extension this \
             check has to certify"
        ));
    }
    let wide = match elf[4] {
        2 => true,
        1 => false,
        other => {
            return Err(format!(
                "{label}: ELF class {other} is neither 32 (1) nor 64 (2), so its headers cannot be \
                 read"
            ));
        }
    };
    if elf[5] != 1 {
        return Err(format!(
            "{label}: ELF data encoding {} is not little-endian (1); this reader refuses rather \
             than guess where the dynamic strings are",
            elf[5]
        ));
    }

    let (phoff, phentsize, phnum, phentmin) = if wide {
        (
            u64_at(elf, 0x20, label)? as usize,
            u16_at(elf, 0x36, label)? as usize,
            u16_at(elf, 0x38, label)? as usize,
            56,
        )
    } else {
        (
            u32_at(elf, 0x1c, label)? as usize,
            u16_at(elf, 0x2a, label)? as usize,
            u16_at(elf, 0x2c, label)? as usize,
            32,
        )
    };
    if phnum == 0 || phoff == 0 {
        return Err(format!(
            "{label}: the ELF header declares no program header table, so there is no way to find \
             its dynamic section (fail closed)"
        ));
    }
    if phentsize < phentmin {
        return Err(format!(
            "{label}: the program header entry size {phentsize} is below the {phentmin} a class {} \
             file needs",
            if wide { 64 } else { 32 }
        ));
    }

    let mut loads = Vec::new();
    let mut dynamic = None;
    for index in 0..phnum {
        let base = phoff
            .checked_add(index * phentsize)
            .ok_or_else(|| format!("{label}: program header {index} overflows the file offset"))?;
        let kind = u32_at(elf, base, label)?;
        let (offset, filesz, vaddr) = if wide {
            (
                u64_at(elf, base + 8, label)?,
                u64_at(elf, base + 32, label)?,
                u64_at(elf, base + 16, label)?,
            )
        } else {
            (
                u64::from(u32_at(elf, base + 4, label)?),
                u64::from(u32_at(elf, base + 16, label)?),
                u64::from(u32_at(elf, base + 8, label)?),
            )
        };
        match kind {
            PT_LOAD => loads.push((vaddr, filesz, offset)),
            PT_DYNAMIC => {
                dynamic = Some((
                    usize::try_from(offset).map_err(|_| {
                        format!("{label}: PT_DYNAMIC's offset {offset} does not fit this host")
                    })?,
                    filesz,
                ));
            }
            _ => {}
        }
    }
    let Some((dynamic_at, dynamic_size)) = dynamic else {
        return Err(format!(
            "{label}: no PT_DYNAMIC segment, so its NEEDED and RPATH/RUNPATH entries cannot be \
             read (fail closed)"
        ));
    };
    let entry = if wide { 16 } else { 8 };
    let size = usize::try_from(dynamic_size)
        .map_err(|_| format!("{label}: PT_DYNAMIC's size {dynamic_size} does not fit this host"))?;
    if dynamic_at
        .checked_add(size)
        .is_none_or(|end| end > elf.len())
    {
        return Err(format!(
            "{label}: the dynamic section at {dynamic_at}+{size} is past the end of the {} byte \
             file",
            elf.len()
        ));
    }

    let mut needed_offsets = Vec::new();
    let mut search_offsets: Vec<(u64, &'static str)> = Vec::new();
    let mut strtab = None;
    let mut strsz = None;
    for index in 0..size / entry {
        let at = dynamic_at + index * entry;
        let (tag, value) = if wide {
            (u64_at(elf, at, label)?, u64_at(elf, at + 8, label)?)
        } else {
            (
                u64::from(u32_at(elf, at, label)?),
                u64::from(u32_at(elf, at + 4, label)?),
            )
        };
        match tag {
            DT_NULL => break,
            DT_NEEDED => needed_offsets.push(value),
            DT_STRTAB => strtab = Some(value),
            DT_STRSZ => strsz = Some(value),
            DT_RPATH => search_offsets.push((value, "RPATH")),
            DT_RUNPATH => search_offsets.push((value, "RUNPATH")),
            _ => {}
        }
    }

    let mut facts = DynamicFacts::default();
    if needed_offsets.is_empty() && search_offsets.is_empty() {
        return Ok(facts);
    }
    let strtab = strtab.ok_or_else(|| {
        format!(
            "{label}: the dynamic section names a library or a search path but carries no \
             DT_STRTAB, so the strings cannot be resolved (fail closed)"
        )
    })?;
    let table = vaddr_to_offset(&loads, strtab, label)?;
    let limit = match strsz {
        Some(size) => usize::try_from(size)
            .map_err(|_| format!("{label}: DT_STRSZ {size} does not fit this host"))?,
        None => usize::MAX,
    };
    for offset in needed_offsets {
        facts
            .needed
            .push(string_at(elf, table, limit, offset, label)?);
    }
    for (offset, kind) in search_offsets {
        facts
            .search_paths
            .push((kind, string_at(elf, table, limit, offset, label)?));
    }
    Ok(facts)
}

/// Where a `DT_STRTAB` virtual address lives in the file, via `PT_LOAD`.
fn vaddr_to_offset(loads: &[(u64, u64, u64)], vaddr: u64, label: &str) -> Result<usize, String> {
    for &(base, filesz, offset) in loads {
        if vaddr >= base && vaddr < base.saturating_add(filesz) {
            let at = offset
                .checked_add(vaddr - base)
                .ok_or_else(|| format!("{label}: vaddr {vaddr:#x} overflows its segment offset"))?;
            return usize::try_from(at).map_err(|_| {
                format!("{label}: vaddr {vaddr:#x} maps past what this host can address")
            });
        }
    }
    Err(format!(
        "{label}: DT_STRTAB's vaddr {vaddr:#x} is in no PT_LOAD segment's file range, so the \
         dynamic strings cannot be located (fail closed)"
    ))
}

/// The NUL-terminated string at `offset` in the dynamic string table.
fn string_at(
    elf: &[u8],
    table: usize,
    limit: usize,
    offset: u64,
    label: &str,
) -> Result<String, String> {
    let offset = usize::try_from(offset)
        .map_err(|_| format!("{label}: a dynamic string offset does not fit this host"))?;
    if offset >= limit {
        return Err(format!(
            "{label}: dynamic string offset {offset} is past DT_STRSZ ({limit}), so the string \
             cannot be trusted"
        ));
    }
    let start = table
        .checked_add(offset)
        .ok_or_else(|| format!("{label}: dynamic string offset {offset} overflows the file"))?;
    let tail = elf.get(start..).ok_or_else(|| {
        format!("{label}: dynamic string offset {offset} is past the end of the file")
    })?;
    let end = tail.iter().position(|byte| *byte == 0).ok_or_else(|| {
        format!("{label}: the dynamic string at offset {offset} is not NUL-terminated")
    })?;
    // A dynamic string is never empty: a `DT_NEEDED` that names no library
    // resolves nothing (and an empty `DT_RUNPATH` entry names the current
    // directory), so an empty name would let an artifact certify clean while the
    // section proved nothing.
    if end == 0 {
        return Err(format!(
            "{label}: the dynamic string at offset {offset} is empty, so that entry names no \
             library — a NEEDED or a search path with no text proves nothing about the artifact, \
             and the check refuses to certify it (fail closed)"
        ));
    }
    // `DT_STRSZ` bounds the *whole* table, so the string's last byte has to be
    // inside it — not just its first. Without this, an understated `DT_STRSZ`
    // lets a name be read out of bytes the file says are not strings.
    let stops = offset
        .checked_add(end)
        .ok_or_else(|| format!("{label}: the dynamic string at offset {offset} overflows"))?;
    if stops >= limit {
        return Err(format!(
            "{label}: the dynamic string at offset {offset} ends at {stops}, which is not inside \
             the DT_STRSZ table of {limit} bytes, so the string is not in the table the file \
             declares (fail closed)"
        ));
    }
    String::from_utf8(tail[..end].to_vec())
        .map_err(|_| format!("{label}: the dynamic string at offset {offset} is not UTF-8"))
}

/// `width` bytes at `at`, or an error naming the field and the file's size.
fn checked<'a>(bytes: &'a [u8], at: usize, width: usize, label: &str) -> Result<&'a [u8], String> {
    bytes.get(at..at.saturating_add(width)).ok_or_else(|| {
        format!(
            "{label}: a {width}-byte field at {at} runs past the {} byte file",
            bytes.len()
        )
    })
}

/// A little-endian `u16` at `at`, or an error naming the segment.
fn u16_at(bytes: &[u8], at: usize, label: &str) -> Result<u16, String> {
    let slice = checked(bytes, at, 2, label)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

/// A little-endian `u32` at `at`, or an error naming the segment.
fn u32_at(bytes: &[u8], at: usize, label: &str) -> Result<u32, String> {
    let slice = checked(bytes, at, 4, label)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// A little-endian `u64` at `at`, or an error naming the segment.
fn u64_at(bytes: &[u8], at: usize, label: &str) -> Result<u64, String> {
    let slice = checked(bytes, at, 8, label)?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

/// One fixture ELF's shape.
struct Synthetic<'a> {
    /// `ELFCLASS32` (1) or `ELFCLASS64` (2).
    class: u8,
    /// `ELFDATA2LSB` (1) or `ELFDATA2MSB` (2).
    data: u8,
    /// The `DT_NEEDED` names, in order.
    needed: &'a [&'a str],
    /// A `DT_RUNPATH` value.
    search: Option<&'a str>,
    /// An understated `DT_STRSZ`, for the fixture that must be refused.
    strsz: Option<u64>,
}

/// A little-endian ELF64 the fixture wheels carry: one `PT_LOAD`, one `PT_DYNAMIC`.
///
/// It is a real, parseable ELF (its `DT_NEEDED`, `DT_RUNPATH` and `DT_STRTAB`
/// land where this reader looks for them), which is what lets the R12 rule set
/// be proved refused *and* accepted without building a wheel on every run.
fn synthetic_elf(needed: &[&str], search: Option<&str>) -> Vec<u8> {
    synthetic(&Synthetic {
        class: 2,
        data: 1,
        needed,
        search,
        strsz: None,
    })
}

/// The same fixture at any class and byte order, so every refusal branch of the
/// reader has a case: `ELFCLASS32` must parse, `ELFDATA2MSB` must be refused, and
/// an understated `DT_STRSZ` must not let a name be read outside the table.
fn synthetic(spec: &Synthetic<'_>) -> Vec<u8> {
    let wide = spec.class == 2;
    let big = spec.data == 2;
    // The string table: a leading NUL, then each name, as the dynamic linker
    // lays it out. Offsets are relative to `DT_STRTAB`.
    let mut strings: Vec<u8> = vec![0];
    let mut needed_at = Vec::new();
    for name in spec.needed {
        needed_at.push(strings.len() as u64);
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
    }
    let search_at = spec.search.map(|path| {
        let at = strings.len() as u64;
        strings.extend_from_slice(path.as_bytes());
        strings.push(0);
        at
    });

    let header_size = if wide { 64 } else { 52 };
    let program_header = if wide { 56 } else { 32 };
    let entry_size = if wide { 16 } else { 8 };
    let entries = spec.needed.len() + usize::from(spec.search.is_some()) + 3;
    let dynamic_at = header_size + 2 * program_header;
    let strings_at = dynamic_at + entries * entry_size;
    let total = strings_at + strings.len();

    let mut header = vec![0u8; header_size];
    header[..4].copy_from_slice(b"\x7fELF");
    header[4] = spec.class;
    header[5] = spec.data;
    header[6] = 1; // EV_CURRENT
    patch(&mut header, 16, 3, 2, big); // e_type: ET_DYN
    patch(&mut header, 18, if wide { 62 } else { 3 }, 2, big); // EM_X86_64 / EM_386
    // ELF64 puts e_version at 20 (4 bytes), e_entry at 24 (8) and e_phoff at 32;
    // ELF32 has the same e_version and e_entry offsets, and e_phoff at 28.
    if wide {
        patch(&mut header, 20, 1, 4, big);
        patch(&mut header, 24, 0, 8, big);
        patch(&mut header, 32, header_size as u64, 8, big);
        patch(&mut header, 52, header_size as u64, 2, big);
        patch(&mut header, 54, program_header as u64, 2, big);
        patch(&mut header, 56, 2, 2, big);
    } else {
        patch(&mut header, 20, 1, 4, big);
        patch(&mut header, 24, 0, 4, big);
        patch(&mut header, 28, header_size as u64, 4, big);
        patch(&mut header, 40, header_size as u64, 2, big);
        patch(&mut header, 42, program_header as u64, 2, big);
        patch(&mut header, 44, 2, 2, big);
    }

    let mut load = vec![0u8; program_header];
    write_segment(&mut load, wide, big, PT_LOAD, 4, 0, total);
    let mut dynamic_header = vec![0u8; program_header];
    write_segment(
        &mut dynamic_header,
        wide,
        big,
        PT_DYNAMIC,
        6,
        dynamic_at,
        entries * entry_size,
    );

    let mut elf = header;
    elf.extend_from_slice(&load);
    elf.extend_from_slice(&dynamic_header);
    for offset in needed_at {
        push_dynamic(&mut elf, wide, big, DT_NEEDED, offset);
    }
    if let Some(offset) = search_at {
        push_dynamic(&mut elf, wide, big, DT_RUNPATH, offset);
    }
    push_dynamic(&mut elf, wide, big, DT_STRTAB, strings_at as u64);
    push_dynamic(
        &mut elf,
        wide,
        big,
        DT_STRSZ,
        spec.strsz.unwrap_or(strings.len() as u64),
    );
    push_dynamic(&mut elf, wide, big, DT_NULL, 0);
    elf.extend_from_slice(&strings);
    assert_eq!(elf.len(), total, "the synthetic ELF's layout is fixed");
    elf
}

/// One program header: `PT_LOAD` at vaddr 0 over the whole file, or `PT_DYNAMIC`.
fn write_segment(
    out: &mut [u8],
    wide: bool,
    big: bool,
    kind: u32,
    flags: u32,
    offset: usize,
    size: usize,
) {
    let word = if wide { 8 } else { 4 };
    patch(out, 0, u64::from(kind), 4, big);
    if wide {
        patch(out, 4, u64::from(flags), 4, big);
        patch(out, 8, offset as u64, 8, big);
        patch(out, 16, offset as u64, 8, big);
        patch(out, 32, size as u64, 8, big);
        patch(out, 40, size as u64, 8, big);
        patch(out, 48, 0x1000, 8, big);
    } else {
        patch(out, 4, offset as u64, word, big);
        patch(out, 8, offset as u64, word, big);
        patch(out, 16, size as u64, word, big);
        patch(out, 20, size as u64, word, big);
        patch(out, 24, u64::from(flags), 4, big);
        patch(out, 28, 0x1000, 4, big);
    }
}

/// One `DT_*` entry, 16 bytes wide on ELF64 and 8 on ELF32.
fn push_dynamic(out: &mut Vec<u8>, wide: bool, big: bool, tag: u64, value: u64) {
    let word = if wide { 8 } else { 4 };
    let mut entry = vec![0u8; word * 2];
    patch(&mut entry, 0, tag, word, big);
    patch(&mut entry, word, value, word, big);
    out.extend_from_slice(&entry);
}

/// Write `value` as a `width`-byte field at `at`, in `big` order.
fn patch(out: &mut [u8], at: usize, value: u64, width: usize, big: bool) {
    let bytes = if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    // Big-endian keeps the field in the *last* `width` bytes of the 8-byte
    // array, little-endian in the first.
    let field = if big {
        bytes.get(8 - width..)
    } else {
        bytes.get(..width)
    };
    if let (Some(field), Some(slot)) = (field, out.get_mut(at..at + width)) {
        slot.copy_from_slice(field);
    }
}

// ---------------------------------------------------------------------------
// A dependency-free ZIP reader (central directory + RFC 1951 inflate)
// ---------------------------------------------------------------------------

/// One central-directory entry, as far as this check needs it.
struct Member {
    /// The stored name, `/`-separated.
    name: String,
    /// The compression method: 0 (stored) or 8 (deflate).
    method: u16,
    /// The compressed size, from the central directory.
    compressed: usize,
    /// The size the entry claims to inflate to.
    uncompressed: usize,
    /// Where the local header starts.
    local: usize,
}

/// The central directory of a ZIP archive, refusing zip64 rather than guessing.
fn zip_members(archive: &[u8], label: &str) -> Result<Vec<Member>, String> {
    let eocd = find_eocd(archive).ok_or_else(|| {
        format!(
            "{label}: no end-of-central-directory record, so this is not a zip archive the check \
             can open (a wheel is a zip by definition)"
        )
    })?;
    let entries = u16_at(archive, eocd + 10, label)?;
    if entries == u16::MAX {
        return Err(format!(
            "{label}: a zip64 central directory is beyond this reader; the check refuses rather \
             than skip the extension"
        ));
    }
    let size = u32_at(archive, eocd + 12, label)? as usize;
    let offset = u32_at(archive, eocd + 16, label)? as usize;
    if offset
        .checked_add(size)
        .is_none_or(|end| end > archive.len())
    {
        return Err(format!(
            "{label}: the central directory at {offset}+{size} is past the end of the {} byte file",
            archive.len()
        ));
    }
    let mut members = Vec::new();
    let mut at = offset;
    for _ in 0..entries {
        if u32_at(archive, at, label)? != 0x0201_4b50 {
            return Err(format!(
                "{label}: the central directory entry at {at} has no `PK\\x01\\x02` signature"
            ));
        }
        let method = u16_at(archive, at + 10, label)?;
        let compressed = u32_at(archive, at + 20, label)? as usize;
        let uncompressed = u32_at(archive, at + 24, label)? as usize;
        let name_len = u16_at(archive, at + 28, label)? as usize;
        let extra_len = u16_at(archive, at + 30, label)? as usize;
        let comment_len = u16_at(archive, at + 32, label)? as usize;
        let local = u32_at(archive, at + 42, label)? as usize;
        if compressed == 0xffff_ffff || uncompressed == 0xffff_ffff || local == 0xffff_ffff {
            return Err(format!(
                "{label}: a zip64 entry is beyond this reader; the check refuses rather than skip \
                 the extension"
            ));
        }
        let name_start = at
            .checked_add(46)
            .ok_or_else(|| format!("{label}: a central directory entry overflows"))?;
        let name = checked(archive, name_start, name_len, label).map_err(|_| {
            format!("{label}: the entry name at {name_start} runs past the end of the file")
        })?;
        members.push(Member {
            name: String::from_utf8_lossy(name).into_owned(),
            method,
            compressed,
            uncompressed,
            local,
        });
        at = name_start
            .checked_add(name_len + extra_len + comment_len)
            .ok_or_else(|| format!("{label}: the central directory overflows the file offset"))?;
    }
    Ok(members)
}

/// The offset of the end-of-central-directory record.
fn find_eocd(archive: &[u8]) -> Option<usize> {
    const SIGNATURE: &[u8] = b"PK\x05\x06";
    const RECORD: usize = 22;
    const MAX_COMMENT: usize = 65_535;
    if archive.len() < RECORD {
        return None;
    }
    let last = archive.len() - RECORD;
    let first = last.saturating_sub(MAX_COMMENT);
    (first..=last).rev().find(|&at| {
        archive
            .get(at..at + 4)
            .is_some_and(|signature| signature == SIGNATURE)
    })
}

/// The bytes of one member, inflating it when the entry is deflated.
fn zip_member(archive: &[u8], member: &Member, label: &str) -> Result<Vec<u8>, String> {
    let local = member.local;
    if u32_at(archive, local, label)? != 0x0403_4b50 {
        return Err(format!(
            "{label}: the local header of `{}` at {local} has no `PK\\x03\\x04` signature",
            member.name
        ));
    }
    let name_len = u16_at(archive, local + 26, label)? as usize;
    let extra_len = u16_at(archive, local + 28, label)? as usize;
    let start = local
        .checked_add(30 + name_len + extra_len)
        .ok_or_else(|| format!("{label}: the local header of `{}` overflows", member.name))?;
    let data = checked(archive, start, member.compressed, label).map_err(|_| {
        format!(
            "{label}: `{}`'s {} compressed bytes at {start} run past the end of the file",
            member.name, member.compressed
        )
    })?;
    let bytes = match member.method {
        0 => data.to_vec(),
        8 => inflate::inflate(data, member.uncompressed, &member.name)?,
        other => {
            return Err(format!(
                "{label}: `{}` uses compression method {other}, which this reader cannot open \
                 (only stored and deflate); refusing rather than skipping the extension",
                member.name
            ));
        }
    };
    if bytes.len() != member.uncompressed {
        return Err(format!(
            "{label}: `{}` inflates to {} bytes, not the {} the central directory declares",
            member.name,
            bytes.len(),
            member.uncompressed
        ));
    }
    Ok(bytes)
}

/// A synthetic member that is not an ELF at all, for the fail-closed fixture.
fn plain_zip_member(scratch: &Scratch, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let wheel = scratch.path("garbage.whl");
    write_zip(
        &wheel,
        &[("blastit/_blastit.cpython-314-x86_64-linux-gnu.so", bytes)],
    )?;
    let mut objects = extension_objects(&wheel)?;
    assert_eq!(objects.len(), 1, "one member, one object");
    Ok(objects.remove(0).1)
}

/// Write a stored-entry zip archive — a wheel-shaped fixture, nothing more.
///
/// Stored entries keep the fixture independent of the deflater: the deflate path
/// is proved by `deflate_vectors_decode` and by the real wheel.
fn write_zip(path: &Path, members: &[(&str, &[u8])]) -> Result<(), String> {
    let mut body: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in members {
        let offset = u32::try_from(body.len())
            .map_err(|_| String::from("the fixture archive is larger than a zip32 offset"))?;
        let size = u32::try_from(data.len())
            .map_err(|_| String::from("the fixture member is larger than a zip32 size"))?;
        let name_len = u16::try_from(name.len())
            .map_err(|_| String::from("the fixture member name is longer than 64 KiB"))?;
        // Local header: signature, version, flags, method (stored), time, date,
        // CRC (the reader checks the declared size instead), sizes, name lengths.
        body.extend_from_slice(b"PK\x03\x04");
        body.extend_from_slice(&20u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&21u16.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(&name_len.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(name.as_bytes());
        body.extend_from_slice(data);

        // Central directory entry: the same facts plus the local offset.
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&21u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_size = u32::try_from(central.len())
        .map_err(|_| String::from("the fixture central directory is larger than 4 GiB"))?;
    let central_offset = u32::try_from(body.len())
        .map_err(|_| String::from("the fixture archive is larger than 4 GiB"))?;
    let count = u16::try_from(members.len())
        .map_err(|_| String::from("the fixture archive holds more than 65535 members"))?;
    body.extend_from_slice(&central);
    body.extend_from_slice(b"PK\x05\x06");
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&central_size.to_le_bytes());
    body.extend_from_slice(&central_offset.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    fs::write(path, &body).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// RFC 1951, as much of it as a wheel uses: stored, fixed and dynamic blocks.
mod inflate {
    use std::cmp;

    /// A byte-aligned reader over the low bits of each byte, as DEFLATE packs them.
    struct Bits<'a> {
        data: &'a [u8],
        at: usize,
        buffer: u32,
        have: u32,
    }

    impl Bits<'_> {
        /// A reader at the first byte of `data`.
        fn new(data: &[u8]) -> Bits<'_> {
            Bits {
                data,
                at: 0,
                buffer: 0,
                have: 0,
            }
        }

        /// The next `count` bits, least significant first.
        fn take(&mut self, count: u32) -> Result<u32, String> {
            while self.have < count {
                let byte = *self.data.get(self.at).ok_or_else(|| {
                    String::from("the deflate stream ended in the middle of a symbol")
                })?;
                self.at += 1;
                self.buffer |= u32::from(byte) << self.have;
                self.have += 8;
            }
            let mask = if count == 0 { 0 } else { (1u32 << count) - 1 };
            let value = self.buffer & mask;
            self.buffer >>= count;
            self.have -= count;
            Ok(value)
        }

        /// Discard the bits up to the next byte boundary.
        fn align(&mut self) {
            let drop = self.have % 8;
            self.buffer >>= drop;
            self.have -= drop;
        }
    }

    /// A canonical Huffman code, as DEFLATE writes it.
    struct Huffman {
        counts: [u16; 16],
        symbols: Vec<u16>,
    }

    impl Huffman {
        /// Build the code from per-symbol code lengths (0 = unused).
        fn new(lengths: &[u16]) -> Result<Self, String> {
            let mut counts = [0u16; 16];
            for &length in lengths {
                let slot = usize::from(length);
                let counter = counts.get_mut(slot).ok_or_else(|| {
                    format!("a code length of {length} is longer than DEFLATE's 15")
                })?;
                *counter += 1;
            }
            let mut left: i32 = 1;
            for &count in counts.iter().skip(1) {
                left <<= 1;
                left -= i32::from(count);
                if left < 0 {
                    return Err(String::from(
                        "the deflate Huffman code is over-subscribed, so the stream is corrupt",
                    ));
                }
            }
            let mut offsets = [0u16; 16];
            for length in 1..15 {
                offsets[length + 1] = offsets[length] + counts[length];
            }
            let mut symbols = vec![0u16; lengths.len()];
            for (symbol, &length) in lengths.iter().enumerate() {
                if length != 0 {
                    let slot = usize::from(offsets[usize::from(length)]);
                    let placed = symbols
                        .get_mut(slot)
                        .ok_or_else(|| String::from("a Huffman symbol index runs off its table"))?;
                    *placed = u16::try_from(symbol)
                        .map_err(|_| String::from("a Huffman symbol does not fit in u16"))?;
                    offsets[usize::from(length)] += 1;
                }
            }
            Ok(Huffman { counts, symbols })
        }

        /// Decode one symbol.
        fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, String> {
            let mut code: i32 = 0;
            let mut first: i32 = 0;
            let mut index: i32 = 0;
            for length in 1..16 {
                code |= i32::try_from(bits.take(1)?)
                    .map_err(|_| String::from("a decoded bit does not fit in i32"))?;
                let count = i32::from(self.counts[length]);
                if code - first < count {
                    let at = usize::try_from(index + (code - first))
                        .map_err(|_| String::from("a Huffman index is negative"))?;
                    return self
                        .symbols
                        .get(at)
                        .copied()
                        .ok_or_else(|| String::from("a Huffman symbol is outside its table"));
                }
                index += count;
                first = (first + count) << 1;
                code <<= 1;
            }
            Err(String::from(
                "a deflate symbol matched no code, so the stream's Huffman table is incomplete",
            ))
        }
    }

    /// Length bases for symbols 257..=285.
    const LENGTH_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    /// Extra bits for symbols 257..=285.
    const LENGTH_EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    /// Distance bases for symbols 0..=29.
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    /// Extra bits for distance symbols 0..=29.
    const DIST_EXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    /// The order a dynamic block writes its code-length code in.
    const CODE_LENGTH_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];

    /// Inflate `data`, refusing to grow past `expected` bytes.
    pub fn inflate(data: &[u8], expected: usize, label: &str) -> Result<Vec<u8>, String> {
        let mut bits = Bits::new(data);
        let mut out: Vec<u8> = Vec::with_capacity(cmp::min(expected, 64 << 20));
        loop {
            let last = bits.take(1)?;
            let kind = bits.take(2)?;
            match kind {
                0 => stored(&mut bits, &mut out, expected, label)?,
                1 => {
                    let literals = fixed_literals()?;
                    let distances = fixed_distances()?;
                    codes(&mut bits, &mut out, &literals, &distances, expected, label)?;
                }
                2 => dynamic(&mut bits, &mut out, expected, label)?,
                other => {
                    return Err(format!(
                        "{label}: block type {other} is reserved, so the deflate stream is corrupt"
                    ));
                }
            }
            if last == 1 {
                break;
            }
        }
        Ok(out)
    }

    /// A stored (uncompressed) block.
    fn stored(
        bits: &mut Bits<'_>,
        out: &mut Vec<u8>,
        expected: usize,
        label: &str,
    ) -> Result<(), String> {
        bits.align();
        let length = bits.take(16)? as usize;
        let complement = bits.take(16)?;
        if u32::try_from(length).ok() != Some(!complement & 0xffff) {
            return Err(format!(
                "{label}: a stored block's length and its complement disagree"
            ));
        }
        if out.len().saturating_add(length) > expected {
            return Err(format!(
                "{label}: a stored block adds {length} bytes past the {expected} the wheel declares"
            ));
        }
        for _ in 0..length {
            let byte = *bits.data.get(bits.at).ok_or_else(|| {
                format!("{label}: a stored block runs past the end of the stream")
            })?;
            bits.at += 1;
            out.push(byte);
        }
        Ok(())
    }

    /// A fixed-Huffman block's literal/length code.
    fn fixed_literals() -> Result<Huffman, String> {
        let mut lengths = vec![0u16; 288];
        for (symbol, length) in lengths.iter_mut().enumerate() {
            *length = match symbol {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        Huffman::new(&lengths)
    }

    /// A fixed-Huffman block's distance code.
    fn fixed_distances() -> Result<Huffman, String> {
        Huffman::new(&[5u16; 32])
    }

    /// A dynamic-Huffman block: its own code-length code, then its two tables.
    fn dynamic(
        bits: &mut Bits<'_>,
        out: &mut Vec<u8>,
        expected: usize,
        label: &str,
    ) -> Result<(), String> {
        let literals_count = bits.take(5)? as usize + 257;
        let distances_count = bits.take(5)? as usize + 1;
        let code_lengths_count = bits.take(4)? as usize + 4;
        let total = literals_count + distances_count;

        let mut code_length_lengths = [0u16; 19];
        for &slot in CODE_LENGTH_ORDER.iter().take(code_lengths_count) {
            code_length_lengths[slot] = bits.take(3)? as u16;
        }
        let code_length_code = Huffman::new(&code_length_lengths)?;

        let mut lengths: Vec<u16> = Vec::with_capacity(total);
        while lengths.len() < total {
            let symbol = code_length_code.decode(bits)?;
            match symbol {
                0..=15 => lengths.push(symbol),
                16 => {
                    let last = *lengths.last().ok_or_else(|| {
                        format!("{label}: a deflate repeat with no previous code length")
                    })?;
                    let repeat = 3 + bits.take(2)? as usize;
                    lengths.extend(std::iter::repeat_n(last, repeat));
                }
                17 => {
                    let repeat = 3 + bits.take(3)? as usize;
                    lengths.extend(std::iter::repeat_n(0, repeat));
                }
                18 => {
                    let repeat = 11 + bits.take(7)? as usize;
                    lengths.extend(std::iter::repeat_n(0, repeat));
                }
                other => {
                    return Err(format!(
                        "{label}: code-length symbol {other} is not a code length or a repeat"
                    ));
                }
            }
            if lengths.len() > total {
                return Err(format!(
                    "{label}: the deflate code-length table runs past its {total} entries"
                ));
            }
        }

        let literals = Huffman::new(&lengths[..literals_count])?;
        let distances = Huffman::new(&lengths[literals_count..])?;
        codes(bits, out, &literals, &distances, expected, label)
    }

    /// Decode symbols until the end-of-block marker.
    fn codes(
        bits: &mut Bits<'_>,
        out: &mut Vec<u8>,
        literals: &Huffman,
        distances: &Huffman,
        expected: usize,
        label: &str,
    ) -> Result<(), String> {
        loop {
            let symbol = literals.decode(bits)?;
            match symbol {
                0..=255 => out.push(symbol as u8),
                256 => return Ok(()),
                _ => {
                    let index = usize::from(symbol - 257);
                    let (base, extra) = LENGTH_BASE
                        .get(index)
                        .copied()
                        .zip(LENGTH_EXTRA.get(index).copied())
                        .ok_or_else(|| {
                            format!("{label}: length symbol {symbol} is not in the DEFLATE table")
                        })?;
                    let length = usize::from(base) + bits.take(extra)? as usize;

                    let distance_symbol = usize::from(distances.decode(bits)?);
                    let (distance_base, distance_extra) = DIST_BASE
                        .get(distance_symbol)
                        .copied()
                        .zip(DIST_EXTRA.get(distance_symbol).copied())
                        .ok_or_else(|| {
                            format!(
                                "{label}: distance symbol {distance_symbol} is not in the DEFLATE \
                                 table"
                            )
                        })?;
                    let distance = usize::from(distance_base) + bits.take(distance_extra)? as usize;
                    if distance == 0 || distance > out.len() {
                        return Err(format!(
                            "{label}: a back-reference of {distance} points before the {} bytes \
                             already decoded",
                            out.len()
                        ));
                    }
                    for _ in 0..length {
                        // Byte by byte on purpose: a distance of 1 is a run, and
                        // the byte it copies was pushed by this same loop.
                        let byte = out[out.len() - distance];
                        out.push(byte);
                    }
                }
            }
            if out.len() > expected {
                return Err(format!(
                    "{label}: the deflate stream decodes to more than the {expected} bytes the \
                     wheel declares"
                ));
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Decode the hex a vector is written in.
        fn unhex(hex: &str) -> Vec<u8> {
            let digits = hex.as_bytes();
            let mut bytes = Vec::with_capacity(digits.len() / 2);
            for pair in digits.chunks(2) {
                let high = char::from(pair[0]).to_digit(16).unwrap_or(0);
                let low = pair
                    .get(1)
                    .map_or(Some(0), |digit| char::from(*digit).to_digit(16));
                bytes.push(((high << 4) | low.unwrap_or(0)) as u8);
            }
            bytes
        }

        #[test]
        fn deflate_vectors_decode() {
            // One stream per block type (the block type is the two bits after
            // `BFINAL`), produced by zlib, so all three branches are exercised
            // on a host with no wheel to read.
            let mut dynamic_text = String::new();
            for index in 0..20 {
                dynamic_text.push_str(&format!("symbol-{index};"));
            }
            let vectors = [
                (
                    0,
                    "011800e7ff73746f72656420626c6f636b3a2030313233343536373839",
                    "stored block: 0123456789",
                ),
                (
                    1,
                    "2bc94855282ccd4cce56482aca2fcf5348cbaf50c82acd2d2856c82f4b2d5228014ae724565\
                     52aa4e4a70300",
                    "the quick brown fox jumps over the lazy dog",
                ),
                (
                    2,
                    "45cdbb0d803000c4d0899030ff28db508352a4627b0a74a67b95dd9ffb6cd730d6fe81600ae66009d6600bf6\
                     e0088ac13f6d1be358c73cf67180075c50ea0b",
                    dynamic_text.as_str(),
                ),
            ];
            for (expected_kind, hex, expected) in vectors {
                let stream = unhex(hex);
                assert_eq!(
                    (stream[0] >> 1) & 0b11,
                    expected_kind,
                    "the vector must be the block type it is labelled with"
                );
                match inflate(&stream, 4096, "vector") {
                    Ok(bytes) => assert_eq!(
                        String::from_utf8_lossy(&bytes),
                        expected,
                        "block type {expected_kind} must inflate to its text"
                    ),
                    Err(error) => panic!("block type {expected_kind}: {error}"),
                }
            }
        }

        #[test]
        fn a_stream_that_grows_past_its_declared_size_is_refused() {
            let stream = unhex("011800e7ff73746f72656420626c6f636b3a2030313233343536373839");
            match inflate(&stream, 4, "declared 4") {
                Ok(bytes) => {
                    panic!("a 24-byte block must not inflate under a 4-byte bound: {bytes:?}")
                }
                Err(error) => assert!(
                    error.contains("past the 4 the wheel declares"),
                    "the refusal must name the declared size, got: {error}"
                ),
            }
        }
    }
}
