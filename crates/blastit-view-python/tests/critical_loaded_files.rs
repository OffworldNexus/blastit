//! The dev-only loaded-file report, end to end inside one interpreter
//! (`MB.23.4`, `docs/design/dev-fanotify-reload.md` §4.4, `ADR-0014`).
//!
//! What is proved here, in one process and one install:
//!
//! * the module closure reports a module's own `.py` and the template the
//!   application opened, and never a `__pycache__` path;
//! * a module the manifest's `preload` list names is in the **boot** batch —
//!   the closure is inherited across the fork, which is why no parent-side
//!   reporter exists;
//! * a namespace package reports its search directory, and no standard-library
//!   path is reported at all;
//! * `.pyc` becomes its `.py` (`importlib.util.source_from_cache`);
//! * a built-in (`sys`) and a frozen (`os`) module report nothing, and in
//!   particular a frozen module never falls back to `__file__`;
//! * an fd-based `open(3, "w")` — whose audit `path` is an `int` — does not
//!   crash the hook;
//! * an exception raised inside the hook never aborts the application's own
//!   `open` (the file is created), and the busy flag stops the hook recursing;
//! * a second harvest over unchanged `sys.modules` resolves nothing new and
//!   reports nothing new — asserted as a call count, never as a timer;
//! * the boot batch reaches the view's datagram socket with `boot`, `seq = 0`
//!   and the view's source, and decoding it yields the same paths.
//!
//! One `#[test]` touches the interpreter: it is process-wide and the tracker's
//! state is shared by design, so a second such test in this binary could only
//! observe the first one's consumed flushes. The other test reads
//! `stage1.rs`'s source and never attaches to Python.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Read;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use blastit_view::loaded::{LoadedReport, MAX_BATCH_BYTES, decode};
use blastit_view_python::{LoadedTracker, WsgiConfig, serve_reporting};
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// A deterministic fixture root under the build's temp directory.
fn fixture_root() -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("critical_loaded_files");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("templates")).expect("the fixture root");
    fs::create_dir_all(root.join("__pycache__")).expect("the bytecode directory");
    root
}

#[test]
fn the_loaded_file_report_is_exact() {
    let root = fixture_root();
    let helper = root.join("helper.py");
    let preloaded = root.join("preloaded.py");
    let template = root.join("templates/index.html");
    let legacy = root.join("legacy.py");
    let legacy_pyc = root.join("__pycache__/legacy.cpython-313.pyc");
    let opened = root.join("opened_by_the_app.txt");
    // A namespace package: a directory with no `__init__.py`, which is what
    // Flask's `flask.sansio` is (research §2.1 row 4).
    let namespace = root.join("ns_pkg");
    fs::create_dir_all(&namespace).expect("the namespace package");
    fs::write(&helper, "GREETING = 'helper'\n").expect("the module");
    fs::write(&preloaded, "PRELOADED = True\n").expect("the preloaded module");
    fs::write(&template, "<p>hello</p>\n").expect("the template");
    fs::write(&legacy, "LEGACY = 1\n").expect("the source of a cached module");
    fs::write(&legacy_pyc, b"\0\0\0\0").expect("a bytecode file");

    Python::initialize();
    Python::attach(|py| {
        assert!(
            LoadedTracker::adopt(py).is_none(),
            "a production view adopts nothing, because nothing installed a tracker"
        );
        let tracker = LoadedTracker::install(py, &root).expect("the tracker installs");
        assert!(
            LoadedTracker::adopt(py).is_some(),
            "a forked view finds the tracker in sys.modules"
        );
        assert!(
            tracker.harvest(py).expect("a harvest") > 0,
            "the cold scan resolves the interpreter's own closure"
        );

        // The application's own imports and one file read. `preload` is
        // modelled by importing before the boot harvest, exactly as the dev
        // boot imports the manifest's `preload` list before `BLASTIT_WARMUP`.
        py_run(
            py,
            format!(
                "import sys\n\
                 sys.path.insert(0, {root:?})\n\
                 import preloaded\n\
                 import helper\n\
                 import ns_pkg\n\
                 open({template:?}).read()\n",
                root = root.to_string_lossy(),
                template = template.to_string_lossy(),
            ),
        );
    });

    // The boot batch, over a real per-view datagram socket: what a freshly
    // forked view sends before it announces `READY`. It runs with the GIL
    // released here, because the view thread has to acquire it itself — a main
    // thread that held it while waiting for the datagram would deadlock.
    // `view.chan` is held open until the datagram has arrived and then dropped,
    // which is how the view is told to leave.
    let (worker_socket, supervisor_socket) = UnixDatagram::pair().expect("a socketpair");
    let (worker_chan, mut supervisor_chan) = UnixStream::pair().expect("view.chan");
    let view = std::thread::spawn(move || {
        Python::attach(|py| {
            serve_reporting(
                py,
                worker_chan,
                py.None(),
                WsgiConfig::for_route("web"),
                Some(LoadedReport {
                    socket: worker_socket,
                    source: 3,
                }),
            )
        })
    });

    supervisor_socket
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("a read timeout");
    let mut buf = vec![0u8; MAX_BATCH_BYTES];
    let n = supervisor_socket
        .recv(&mut buf)
        .expect("the view sends its boot batch");
    let boot = decode(&buf[..n]).expect("the boot batch decodes");
    assert!(boot.boot, "the first batch replaces the view's set");
    assert!(!boot.resync, "a clean boot asks for no resync");
    assert_eq!(boot.seq, 0, "a boot batch is the start of the stream");
    assert_eq!(boot.source, 3, "the batch names the reporting view");

    for (what, path) in [
        ("the module's own .py", &helper),
        ("a `preload` module", &preloaded),
        ("the template the app opened", &template),
        ("a namespace package's search directory", &namespace),
    ] {
        let path = path.to_string_lossy().into_owned();
        assert!(
            boot.paths.contains(&path),
            "{what} is in the boot harvest: {:?}",
            boot.paths
        );
    }
    assert!(
        !boot.paths.iter().any(|path| path.contains("/__pycache__/")),
        "no bytecode cache path leaks: {:?}",
        boot.paths
    );
    assert!(
        !boot.paths.iter().any(|path| path.ends_with("/os.py")),
        "a frozen module never reports its __file__: {:?}",
        boot.paths
    );
    assert!(
        !boot.paths.iter().any(|path| path == "sys"),
        "a built-in module names no file at all: {:?}",
        boot.paths
    );

    // The view writes `READY` on `view.chan` right after the boot batch, so
    // reading one frame is how the test knows it has finished announcing
    // itself. Without this the channel can be closed while `READY` is still on
    // its way, and `serve` then ends with an `EPIPE` instead of the EOF it is
    // written for — a test that fails on how fast the machine is.
    supervisor_chan
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("a read timeout");
    let mut ready = [0u8; 64];
    assert!(
        supervisor_chan
            .read(&mut ready)
            .expect("the view announces itself")
            > 0,
        "the view writes READY after the boot batch"
    );

    // Closing the supervisor's end is the view's exit: `serve` returns once
    // the channel is over.
    drop(supervisor_chan);
    view.join()
        .expect("the view thread")
        .expect("serve returns");

    // The rest needs the tracker's Python state, so it runs in its own attach
    // on the same interpreter.
    Python::attach(|py| {
        let tracker = LoadedTracker::adopt(py).expect("the tracker from the install above");

        // Warm: unchanged `sys.modules` resolves nothing and reports nothing.
        let before = resolved(py, &tracker);
        assert_eq!(
            tracker.harvest(py),
            Some(0),
            "the warm scan resolves no new module"
        );
        assert_eq!(
            resolved(py, &tracker),
            before,
            "and the resolved set is the same one"
        );
        let (paths, resync) = tracker.flush(py).expect("a flush");
        assert!(paths.is_empty(), "nothing new to report: {paths:?}");
        assert!(!resync);

        // `.pyc` becomes its `.py`, from the hook and from a module origin.
        py_run(
            py,
            format!("open({:?}).read()\n", legacy_pyc.to_string_lossy()),
        );
        let (paths, resync) = tracker.flush(py).expect("a flush");
        assert!(!resync);
        assert_eq!(
            paths,
            vec![legacy.to_string_lossy().into_owned()],
            "the .pyc is reported as the source it was compiled from"
        );

        // An fd-based `open(3, "w")`: the audit `path` is an `int`, which the
        // hook drops instead of crashing on.
        let target = root.join("fd_target.txt");
        py_run(
            py,
            format!(
                "import os\n\
                 fd = os.open({target:?}, os.O_CREAT | os.O_WRONLY, 0o600)\n\
                 open(fd, 'w')\n",
                target = target.to_string_lossy(),
            ),
        );

        let dict = module_dict(py, &tracker);
        // The standard library is never reported (research §2.1's flush step):
        // its prefix lives in the tracker's own state, and every stdlib module
        // the interpreter imported is already in `sys.modules`.
        let stdlib: String = dict
            .get_item("_stdlib")
            .expect("the stdlib lookup")
            .expect("`arm` recorded the standard library")
            .extract()
            .expect("a stdlib path");
        let under_stdlib =
            |path: &String| path == &stdlib || path.starts_with(&format!("{stdlib}/"));
        assert!(
            !boot.paths.iter().any(under_stdlib),
            "no standard-library path is reported ({stdlib}): {:?}",
            boot.paths
        );
        let original = dict
            .get_item("_record_open")
            .expect("the hook body")
            .expect("the hook body exists");

        // An exception inside the hook never aborts the application's own open.
        py_run_in(
            py,
            &dict,
            "def _record_open(args):\n    raise RuntimeError('boom')\n".to_string(),
        );
        let created = root.join("created_despite_a_raising_hook.txt");
        py_run(
            py,
            format!("open({:?}, 'w').close()\n", created.to_string_lossy()),
        );
        assert!(
            created.is_file(),
            "the file is created even though the hook raised"
        );

        // The busy flag stops the hook recursing: a body that opens a file is
        // delivered once, not once per nested open.
        let inner = root.join("opened_by_the_hook.txt");
        py_run_in(
            py,
            &dict,
            format!(
                "seen = []\n\
                 def _record_open(args):\n\
                 \x20   seen.append(True)\n\
                 \x20   open({inner:?}, 'w').close()\n",
                inner = inner.to_string_lossy(),
            ),
        );
        let outer = root.join("opened_around_the_hook.txt");
        py_run(
            py,
            format!("open({:?}, 'w').close()\n", outer.to_string_lossy()),
        );
        assert!(outer.is_file(), "the outer open created its file");
        assert!(inner.is_file(), "the hook's own open created its file");
        let seen = dict
            .get_item("seen")
            .expect("the list")
            .expect("the list exists")
            .len()
            .expect("a length");
        assert_eq!(
            seen, 1,
            "the nested open is not delivered to the hook again"
        );

        // The real hook back, and one ordinary application open: the report
        // names it, and nothing the hook tests created.
        dict.set_item("_record_open", original)
            .expect("the hook body is restored");
        py_run(
            py,
            format!("open({:?}, 'w').close()\n", opened.to_string_lossy()),
        );
        let (paths, resync) = tracker.flush(py).expect("a flush");
        assert!(!resync);
        // `os.open` raises its own `open` event with the string path, so the
        // fd target is a real, named open too — and the files the hook tests
        // created are not, because no real hook body saw them.
        let mut expected = vec![
            opened.to_string_lossy().into_owned(),
            target.to_string_lossy().into_owned(),
        ];
        expected.sort();
        assert_eq!(
            paths, expected,
            "exactly the files the application opened by name, and nothing else"
        );
    });

    let _ = fs::remove_dir_all(&root);
}

/// The dev boot's ordering, read from the **shim's** `preload` source text.
///
/// The owner moved with `MB.42.2.1.4`: `crates/blastit` holds no interpreter any
/// more, so the tracker is armed inside `blastit_py_abi_v1`'s `preload`
/// (`crates/blastit-py-shim/src/boot.rs`) — the tibia every view is forked
/// from — rather than in stage 1's `load`. What this witnesses, and only that:
///
/// * exactly **one** `LoadedTracker::install` site, and it is inside
///   `install_tracker`, after the shim's dev-only gate — the report descriptor
///   (`req.report_fd >= 0`) and the let-else that returns no tracker when there
///   is none. That gate is the shim's equivalent of stage 1's old
///   `if dev.is_some()`: a production boot hands no report fd, so it installs
///   nothing (design §1.2.1 item 3: installed iff `report_fd >= 0`);
/// * `boot` calls `install_tracker` **before** the first application import
///   (`import_callable(py, spec)`) and before the `for module in &req.preload`
///   loop, so the preload phase's modules and opens are seen;
/// * exactly **one** `tracker.harvest(py)`, inside `report_boot`, and `boot`
///   calls `report_boot` after `req.warmup.as_deref()` and before
///   `py.run(CLOSE_AND_COLLECT, None, None)` — so the cold `sys.modules` scan is
///   paid once, in the immortal parent every view is forked from.
///
/// This is **a token-and-order witness, not behavioural proof**: it cannot see
/// control flow, so it would still pass if the install were moved inside a dead
/// branch, and it deliberately does not pin what `--no-reload` adds to the gate.
/// It does pin the two edits that are cheap to make by accident — a second
/// install site outside the gate, and an install moved after the first import.
/// Neither order is observable from a view's own process (by the time a view
/// runs, both have happened), which is why the source is read at all; the
/// runtime half is the boot batch in `the_loaded_file_report_is_exact`.
#[test]
fn the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup() {
    // `MB.42.2.1.4`: the owner is the shim's `preload`, not stage 1.
    let src = include_str!("../../blastit-py-shim/src/boot.rs");
    let at = |needle: &str| {
        src.find(needle)
            .unwrap_or_else(|| panic!("the shim's boot.rs no longer contains {needle:?}"))
    };

    // One install site: a second copy could otherwise sit outside the gate and
    // keep this test green while running in production.
    assert_eq!(
        src.matches("LoadedTracker::install").count(),
        1,
        "exactly one tracker install site, inside the shim's dev-only gate"
    );
    let install = at("LoadedTracker::install");
    // The single site lives in `install_tracker`, between its own delimiters.
    let inside = at("fn install_tracker");
    let after_inside = at("fn report_boot");
    assert!(
        inside < install && install < after_inside,
        "the one install site is `install_tracker`'s body, not a helper beside it"
    );
    // The dev-only gate: the report descriptor decides whether there is a
    // tracker at all, and the let-else returns without one when it does not.
    let gate = at("let socket = (req.report_fd >= 0).then(|| {");
    let no_socket = at("return Ok((None, None))");
    assert!(
        gate < no_socket && no_socket < install,
        "the install is inside the `report_fd >= 0` then-path, after the \
         no-descriptor return — a production boot installs nothing"
    );
    let gated = &src[no_socket..install];
    assert!(
        !gated.contains("if false"),
        "no dead branch pretends to be the dev gate"
    );
    assert!(
        !src.contains("if false"),
        "no dead branch pretends to be the dev gate"
    );
    // `boot` arms it before either import, or the preload closure is missed.
    let armed = at("install_tracker(py, req)");
    assert!(
        armed < at("import_callable(py, spec)"),
        "the tracker is installed before the application is imported, or the preload closure is missed"
    );
    assert!(
        armed < at("for module in &req.preload"),
        "the tracker is installed before the `preload` list is imported, or a preloaded module is missed"
    );
    // One baseline harvest, after the warm-up and before the collection.
    let harvest = at("tracker.harvest(py)");
    assert_eq!(
        src.matches("tracker.harvest(py)").count(),
        1,
        "one baseline harvest, so the cost is paid once"
    );
    let reported = at("report_boot(py, tracker.as_ref(), socket)");
    assert!(
        at("req.warmup.as_deref()") < reported,
        "the harvest runs after `BLASTIT_WARMUP`, so the warmed closure is in the boot batch"
    );
    assert!(
        reported < at("py.run(CLOSE_AND_COLLECT, None, None)"),
        "the harvest runs before the heap is immortalised"
    );
    assert!(
        at("fn report_boot") < harvest,
        "the one harvest is the boot batch's own, not a second scan"
    );
}

/// Runs one Python snippet in the tracker interpreter's main namespace.
fn py_run(py: Python<'_>, code: String) {
    let code = std::ffi::CString::new(code).expect("no NUL in the snippet");
    py.run(&code, None, None).expect("the snippet runs");
}

/// Runs one Python snippet in the tracker module's namespace.
fn py_run_in(py: Python<'_>, dict: &Bound<'_, PyDict>, code: String) {
    let code = std::ffi::CString::new(code).expect("no NUL in the snippet");
    py.run(&code, Some(dict), None).expect("the snippet runs");
}

/// How many modules the tracker has resolved so far, read from its own state.
fn resolved(py: Python<'_>, tracker: &LoadedTracker) -> usize {
    tracker
        .object(py)
        .getattr("_resolved")
        .expect("the state dict")
        .len()
        .expect("a length")
}

/// The tracker module's own `__dict__`, so a test can replace the hook body.
fn module_dict<'py>(py: Python<'py>, tracker: &LoadedTracker) -> Bound<'py, PyDict> {
    tracker
        .object(py)
        .getattr("__dict__")
        .expect("a module has a __dict__")
        .cast_into::<PyDict>()
        .expect("the module dict")
}
