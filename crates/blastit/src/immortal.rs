//! Heap immortalisation before the fork (ARCHITECTURE.md §9.2 #7, §4).
//!
//! On App Platform, gVisor breaks copy-on-write in chunks of about 2 MiB
//! (§9.2 #6), and CPython's reference counts write to every object a request
//! touches. Left alone, each view process ends up copying most of the
//! preloaded heap: 87 MiB per child against 17.8 MiB with nothing touched
//! (§9.2 #7). The fix is to make the preloaded heap **immortal** before
//! `gc.freeze()`, so `Py_INCREF` and `Py_DECREF` never write to it:
//!
//! ```text
//! gc.get_objects()                     every object the collector tracks
//!   └─ gc.get_referents() transitively  untracked str/int/bytes included
//! ob_refcnt low 32 bits = 0xFFFFFFFF    the immortal marker
//! ```
//!
//! The marker is the low half of the reference count, which is stable across
//! the versions blastit knows:
//!
//! * **3.12 and 3.13** — `Py_INCREF` adds one to the low 32 bits and skips the
//!   write when they wrap, and `Py_DECREF` checks the sign bit of the same
//!   half, so `0xFFFFFFFF` is never incremented and never reaches zero. The
//!   upper half is unused, so writing only the low half is the whole value.
//! * **3.14** — `_Py_IsImmortal` is exactly `(int32_t)refcnt < 0`, and the
//!   upper half holds `ob_overflow` and `ob_flags`, which must not be touched.
//!   The marker is what that test reads, but **not every decref on 3.14 checks
//!   it**: paths that only handle mortal objects count an immortal one down,
//!   `0xFFFFFFFF` → `0xFFFFFFFE` → …, and some later decref frees or corrupts
//!   it. Views then segfault in `_PyObject_Malloc` on their first request, so
//!   [`Immortalize::Auto`] does **not** mark a 3.14 heap, and
//!   `BLASTIT_IMMORTALIZE=1` forces it for experiments only.
//!
//! Anything else — 32-bit, big-endian, a CPython outside 3.12–3.14, or a
//! free-threaded build, whose counts live in `ob_ref_local` and
//! `ob_ref_shared` instead — is **skipped with a warning** rather than
//! half-done: the server starts and each view copies more of the preloaded
//! heap, which is the cost §9.2 #6 measured ([`decide`]).
//!
//! Immortal objects are never freed, which §9.2 #7 accepts for a heap that
//! lives as long as the container.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

use crate::config::Immortalize;

/// The immortal marker, as the low 32 bits of `ob_refcnt`.
const IMMORTAL_LOW: u32 = u32::MAX;

/// How many objects are handed to `gc.get_referents` in one call.
///
/// It takes `*objects` and returns one flat list, so batching cuts the number
/// of Python calls by three orders of magnitude without changing the walk.
const BATCH: usize = 1024;

/// What skipping the marker costs, appended to every [`Decision::Off`]: §9.2
/// #6's copy of the shared heap, per view process.
const SKIPPED_COST: &str = ", views will copy more of the preloaded heap";

/// The first CPython minor whose marker blastit knows, on `3.x`.
pub const MIN_MINOR: u8 = 12;

/// The last CPython minor whose marker blastit knows, on `3.x`: written on
/// 3.12, 3.13 and 3.14, the last of which only `BLASTIT_IMMORTALIZE=1` asks for
/// ([`Immortalize::Auto`] stops at [`AUTO_MAX_MINOR`]).
pub const MAX_MINOR: u8 = 14;

/// The last CPython minor [`Immortalize::Auto`] marks, on `3.x`.
///
/// 3.14 writes the same marker, but some of its decref paths do not check it
/// (see the module docs), so an auto pass stops at 3.13.
pub const AUTO_MAX_MINOR: u8 = 13;

/// The interpreter facts §9.2 #7's marker depends on.
///
/// Every one of them decides either where the reference count sits or what
/// `0xFFFFFFFF` means to `Py_INCREF` and `Py_DECREF`, so [`supported`] and
/// [`decide`] are pure functions of exactly these: each refusal can be
/// unit-tested without such a build being installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interpreter {
    /// CPython major version.
    pub major: u8,
    /// CPython minor version.
    pub minor: u8,
    /// Pointer width of the interpreter, in bits.
    pub pointer_width: u32,
    /// Whether a 32-bit store at offset zero means the low half of `ob_refcnt`:
    /// little-endian for both the build that writes the marker and the
    /// interpreter that owns the object.
    pub little_endian: bool,
    /// Free-threaded build (`Py_GIL_DISABLED`), whose counts live in
    /// `ob_ref_local` and `ob_ref_shared` instead of `ob_refcnt`.
    pub free_threaded: bool,
    /// `Py_TRACE_REFS` build, which keeps two pointers *before* `ob_refcnt`.
    pub trace_refs: bool,
}

/// What one immortalisation pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// How many objects had the marker written. Objects that were immortal
    /// already — small integers, interned strings, the interpreter's own
    /// singletons — are walked but not counted.
    pub objects: u64,
    /// How long the walk took.
    pub elapsed: Duration,
}

/// Refuses an interpreter whose immortal marker blastit does not know (§9.2
/// #7). This is the pure half of [`decide`].
///
/// A refusal is not fatal: [`decide`] turns it into [`Decision::Off`] and stage
/// 1 warns instead of starting a server that cannot be immortalised.
///
/// # Errors
///
/// A sentence for the log, naming what is unsupported and why.
pub fn supported(interpreter: &Interpreter) -> Result<(), String> {
    if interpreter.pointer_width != 64 {
        return Err(format!(
            "heap immortalisation needs 64-bit CPython: this is a {}-bit one",
            interpreter.pointer_width
        ));
    }
    if !interpreter.little_endian {
        return Err(
            "heap immortalisation needs a little-endian CPython: the marker is the low 32 bits \
             of the reference count, which is a 32-bit store only on a little-endian object"
                .to_string(),
        );
    }
    if (interpreter.major, interpreter.minor) < (3, MIN_MINOR)
        || (interpreter.major, interpreter.minor) > (3, MAX_MINOR)
    {
        return Err(format!(
            "CPython {}.{}: §9.2 #7's immortal marker is known for 3.{MIN_MINOR} to 3.{MAX_MINOR} only",
            interpreter.major, interpreter.minor
        ));
    }
    if interpreter.free_threaded {
        return Err(
            "the free-threaded CPython build (Py_GIL_DISABLED) keeps its reference counts in \
             ob_ref_local and ob_ref_shared, where §9.2 #7's marker means nothing"
                .to_string(),
        );
    }
    if interpreter.trace_refs {
        return Err(
            "a Py_TRACE_REFS CPython build (sys.getobjects) puts two pointers before ob_refcnt, \
             so §9.2 #7's marker would land in the middle of that list"
                .to_string(),
        );
    }
    Ok(())
}

/// Whether §9.2 #7's pass runs for one interpreter and one
/// `BLASTIT_IMMORTALIZE` setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Write the marker over the preloaded heap.
    On,
    /// Leave the heap alone, for this reason: the sentence the `warn` event
    /// and the preload `boot` event carry.
    Off(String),
}

impl Decision {
    /// Whether the pass runs.
    pub const fn is_on(&self) -> bool {
        matches!(self, Self::On)
    }

    /// The reason the pass does not run, when it does not.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::On => None,
            Self::Off(reason) => Some(reason),
        }
    }
}

/// §9.2 #7's decision for one interpreter and one setting.
///
/// This is the whole policy, with no interpreter in sight, so every branch is
/// unit-testable:
///
/// | `BLASTIT_IMMORTALIZE` | 3.12/3.13 | 3.14 | anything else |
/// |---|---|---|---|
/// | `auto` (default) | on | **off**: unsafe there | off |
/// | `1` | on | on, for experiments | off |
/// | `0` | off | off | off |
///
/// The setting decides before the interpreter does, so an operator who turned
/// the pass off is told that, and only then what a version could not do. Every
/// [`Decision::Off`] ends with what skipping costs, so the sentence reads as
/// one line in the log whatever the reason.
pub fn decide(interpreter: &Interpreter, setting: Immortalize) -> Decision {
    if setting == Immortalize::Off {
        return Decision::Off(format!(
            "BLASTIT_IMMORTALIZE=0: heap immortalisation is off{SKIPPED_COST}"
        ));
    }
    if let Err(problem) = supported(interpreter) {
        return Decision::Off(format!("{problem}{SKIPPED_COST}"));
    }
    if setting == Immortalize::Auto
        && !(interpreter.major == 3 && (MIN_MINOR..=AUTO_MAX_MINOR).contains(&interpreter.minor))
    {
        return Decision::Off(format!(
            "CPython {}.{}: heap immortalisation is unsafe on this version{SKIPPED_COST}",
            interpreter.major, interpreter.minor
        ));
    }
    Decision::On
}

/// §9.2 #7's outcome for the running interpreter: the decision, and what the
/// pass did when it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// On, or off with the sentence that says why.
    pub decision: Decision,
    /// What the pass did — `Some` exactly when the decision is
    /// [`Decision::On`].
    pub stats: Option<Stats>,
}

impl Outcome {
    /// How many objects carry the marker afterwards: the pass's count, or zero
    /// when the pass was skipped.
    pub fn objects(&self) -> u64 {
        self.stats.map_or(0, |stats| stats.objects)
    }

    /// How long the pass took, in milliseconds, when it ran.
    ///
    /// A skipped pass has no duration: there was no walk to time.
    pub fn millis(&self) -> Option<f64> {
        self.stats.map(|stats| stats.elapsed.as_secs_f64() * 1000.0)
    }
}

/// [`decide`] for the interpreter that is running, and the pass when it says
/// to run it.
///
/// # Errors
///
/// Whatever importing `gc` or calling it raises, when the decision was to run
/// the pass. A caller that decided to run and cannot must not start: a view
/// would then copy the shared heap. A skipped pass never fails.
pub fn apply(py: Python<'_>, setting: Immortalize) -> PyResult<Outcome> {
    let decision = decide(&probe(py), setting);
    if !decision.is_on() {
        return Ok(Outcome {
            decision,
            stats: None,
        });
    }
    Ok(Outcome {
        decision,
        stats: Some(immortalize(py)?),
    })
}

/// Reads the facts [`supported`] needs from the running interpreter.
fn probe(py: Python<'_>) -> Interpreter {
    let version = py.version_info();
    Interpreter {
        major: version.major,
        minor: version.minor,
        pointer_width: pointer_width(py),
        // Both halves matter: the marker is a 32-bit store written by this
        // build into an object laid out by the interpreter.
        little_endian: cfg!(target_endian = "little") && sys_byteorder_is_little(py),
        free_threaded: free_threaded(py),
        trace_refs: trace_refs(py),
    }
}

/// The interpreter's own pointer width, in bits.
///
/// `struct.calcsize("P")` is the width CPython reports for itself, which is the
/// one the object layout follows. A platform where the question cannot be asked
/// is this build's own width.
fn pointer_width(py: Python<'_>) -> u32 {
    let bytes = py
        .import("struct")
        .and_then(|module| module.call_method1("calcsize", ("P",)))
        .and_then(|size| size.extract::<u32>());
    match bytes {
        Ok(bytes) => bytes.saturating_mul(8),
        Err(_) => usize::BITS,
    }
}

/// Whether the interpreter calls its own byte order little-endian.
fn sys_byteorder_is_little(py: Python<'_>) -> bool {
    let order = py
        .import("sys")
        .and_then(|sys| sys.getattr("byteorder"))
        .and_then(|order| order.extract::<String>());
    match order {
        Ok(order) => order == "little",
        Err(_) => cfg!(target_endian = "little"),
    }
}

/// Whether the running interpreter is a `Py_TRACE_REFS` build.
///
/// `sys.getobjects` exists only there, and such a build keeps
/// `_PyObject_HEAD_EXTRA` — two pointers — in front of `ob_refcnt` (§9.2 #7's
/// offset-zero store would overwrite the middle of that list).
fn trace_refs(py: Python<'_>) -> bool {
    py.import("sys")
        .and_then(|sys| sys.getattr("getobjects"))
        .is_ok()
}

/// Whether the running interpreter is the free-threaded build.
///
/// Two questions cover the three ways of asking: the build configuration
/// (`sysconfig.get_config_var("Py_GIL_DISABLED")` — the same flag pyo3's
/// `Py_GIL_DISABLED` cfg is derived from, readable here without a build script
/// of our own) and the runtime switch (`sys._is_gil_enabled`, 3.13+). A
/// question that cannot be asked means "not free-threaded"; [`check`] still
/// refuses any version it does not know.
fn free_threaded(py: Python<'_>) -> bool {
    let from_sysconfig = || -> Option<bool> {
        let sysconfig = py.import("sysconfig").ok()?;
        let value = sysconfig
            .call_method1("get_config_var", ("Py_GIL_DISABLED",))
            .ok()?;
        value.is_truthy().ok()
    };
    if from_sysconfig() == Some(true) {
        return true;
    }
    let from_sys = || -> Option<bool> {
        let sys = py.import("sys").ok()?;
        let enabled = sys.getattr("_is_gil_enabled").ok()?;
        let enabled = enabled.call0().ok()?;
        Some(!enabled.is_truthy().ok()?)
    };
    from_sys().unwrap_or(false)
}

/// Makes every object reachable from `gc.get_objects()` immortal (§9.2 #7).
///
/// The walk is a breadth-first search over `gc.get_referents()`, keyed by
/// object address, so untracked objects — `str`, `int`, `bytes` — are reached
/// as well. Objects are marked as they are discovered, which is also what keeps
/// the address set meaningful for the whole walk: an immortal object is never
/// freed, so no later object can reuse its address.
///
/// It is a **whole-interpreter** pass: `gc.get_objects()` is every tracked
/// object in the process, so anything another thread allocates or holds while
/// this runs is immortalised too. init satisfies that trivially — stage 1 is
/// single-threaded and nothing else in the process runs Python (§2.1, §4) — but
/// a caller that shares its interpreter with other Python threads must
/// serialise around this call.
///
/// # Errors
///
/// Whatever importing `gc` or calling it raises. A caller that cannot
/// immortalise must not start: a view would then copy the shared heap. Callers
/// go through [`apply`], which decides whether to run this at all.
pub fn immortalize(py: Python<'_>) -> PyResult<Stats> {
    let gc = py.import("gc")?;
    let get_objects = gc.getattr("get_objects")?;
    let get_referents = gc.getattr("get_referents")?;

    let started = Instant::now();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut queue: VecDeque<Py<PyAny>> = VecDeque::new();
    let mut objects = 0_u64;

    for object in get_objects.call0()?.try_iter()? {
        discover(&mut seen, &mut queue, &object?, &mut objects);
    }
    while !queue.is_empty() {
        let mut batch: Vec<Py<PyAny>> = Vec::with_capacity(BATCH);
        while batch.len() < BATCH {
            match queue.pop_front() {
                Some(object) => batch.push(object),
                None => break,
            }
        }
        let args = PyTuple::new(py, batch.iter().map(|object| object.bind(py)))?;
        for referent in get_referents.call1(args)?.try_iter()? {
            discover(&mut seen, &mut queue, &referent?, &mut objects);
        }
        // `batch` is dropped here. Every object in it was marked immortal when
        // it was discovered, so dropping the last reference to one of them does
        // not free it, and the walk stays valid.
    }
    Ok(Stats {
        objects,
        elapsed: started.elapsed(),
    })
}

/// Marks `object` immortal if it is not already, and queues it for the walk
/// when it has not been seen.
fn discover(
    seen: &mut HashSet<usize>,
    queue: &mut VecDeque<Py<PyAny>>,
    object: &Bound<'_, PyAny>,
    objects: &mut u64,
) {
    if !seen.insert(object.as_ptr() as usize) {
        return;
    }
    // SAFETY: `object` is a live Python object and the GIL is held, so its
    // reference count is ours to write.
    if unsafe { write_marker(object.as_ptr()) } {
        *objects += 1;
    }
    queue.push_back(object.clone().unbind());
}

/// Writes §9.2 #7's marker into the object's reference count, and returns
/// whether it wrote anything.
///
/// Only the low 32 bits are touched: on 3.12 and 3.13 they are the whole
/// reference count, and on 3.14 the upper 32 bits hold `ob_overflow` and
/// `ob_flags`, which a 64-bit store would clobber.
///
/// # Safety
///
/// `object` must point at a live Python object, and the caller must hold the
/// GIL (`PyObject` is not `Sync`).
unsafe fn write_marker(object: *mut ffi::PyObject) -> bool {
    // SAFETY: a `PyObject` is 8-byte aligned (it starts with the reference
    // count and a pointer), so the first four bytes are a valid, aligned
    // `u32`; the GIL means nothing else is writing it.
    let low = object.cast::<u32>();
    if unsafe { low.read_volatile() } == IMMORTAL_LOW {
        return false;
    }
    // SAFETY: as above. The store is volatile so the compiler cannot cache the
    // old count across the Python calls that follow.
    unsafe { low.write_volatile(IMMORTAL_LOW) };
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use pyo3::types::PyDict;

    use super::*;

    /// Serialises the tests in this binary that touch the interpreter heap.
    ///
    /// These tests are threads of one process and share one interpreter, and
    /// [`immortalize`] is a whole-interpreter pass: `gc.get_objects()` is what
    /// the collector tracks, so a pass running in a parallel test marks the
    /// objects another test is holding — including one it has just created to
    /// check that a *new* object is left alone. The window is real: CPython
    /// releases the GIL at bytecode boundaries, so a pass can run in the middle
    /// of another test's `py.run`, where the executing frame makes everything
    /// that snippet bound reachable. Every test here holds this lock for its
    /// whole body, so each one sees a heap only its own pass has touched.
    static HEAP: Mutex<()> = Mutex::new(());

    /// The lock described on [`HEAP`]; a poisoned lock still runs the next test.
    fn heap() -> MutexGuard<'static, ()> {
        HEAP.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the object already carries §9.2 #7's marker.
    fn is_immortal(object: *mut ffi::PyObject) -> bool {
        // SAFETY: the caller passes a live object and holds the GIL.
        unsafe { object.cast::<u32>().read_volatile() == IMMORTAL_LOW }
    }

    /// `sys.getrefcount`.
    fn getrefcount(py: Python<'_>, object: &Bound<'_, PyAny>) -> i64 {
        py.import("sys")
            .unwrap()
            .call_method1("getrefcount", (object,))
            .unwrap()
            .extract()
            .unwrap()
    }

    /// Runs `code` in a fresh namespace and returns it.
    fn namespace<'py>(py: Python<'py>, code: &std::ffi::CStr) -> Bound<'py, PyDict> {
        let globals = PyDict::new(py);
        py.run(code, Some(&globals), None).unwrap();
        globals
    }

    /// Whether this interpreter can carry §9.2 #7's marker at all.
    fn walkable(py: Python<'_>) -> bool {
        supported(&probe(py)).is_ok()
    }

    /// The interpreter facts to test a decision against, at `minor`.
    fn interpreter(minor: u8) -> Interpreter {
        Interpreter {
            minor,
            ..supported_interpreter()
        }
    }

    #[test]
    fn a_supported_interpreter_is_accepted() {
        assert!(supported(&supported_interpreter()).is_ok());
        assert!(
            supported(&Interpreter {
                minor: MIN_MINOR,
                ..supported_interpreter()
            })
            .is_ok()
        );
        assert!(
            supported(&Interpreter {
                minor: MAX_MINOR,
                ..supported_interpreter()
            })
            .is_ok()
        );
    }

    #[test]
    fn a_32_bit_interpreter_is_refused() {
        let problem = supported(&Interpreter {
            pointer_width: 32,
            ..supported_interpreter()
        })
        .unwrap_err();
        assert!(problem.contains("64-bit"), "{problem}");
        assert!(problem.contains("32-bit"), "{problem}");
    }

    #[test]
    fn a_big_endian_interpreter_is_refused() {
        let problem = supported(&Interpreter {
            little_endian: false,
            ..supported_interpreter()
        })
        .unwrap_err();
        assert!(problem.contains("little-endian"), "{problem}");
    }

    #[test]
    fn versions_outside_3_12_to_3_14_are_refused() {
        for (major, minor) in [(3, 11), (3, 15), (3, 0), (2, 7), (4, 0)] {
            let problem = supported(&Interpreter {
                major,
                minor,
                ..supported_interpreter()
            })
            .unwrap_err();
            assert!(
                problem.contains(&format!("CPython {major}.{minor}")),
                "unclear refusal for {major}.{minor}: {problem}"
            );
            assert!(problem.contains("3.12 to 3.14"), "{problem}");
        }
    }

    #[test]
    fn a_free_threaded_build_is_refused() {
        let problem = supported(&Interpreter {
            free_threaded: true,
            ..supported_interpreter()
        })
        .unwrap_err();
        assert!(problem.contains("free-threaded"), "{problem}");
        assert!(problem.contains("Py_GIL_DISABLED"), "{problem}");
    }

    #[test]
    fn a_trace_refs_build_is_refused() {
        let problem = supported(&Interpreter {
            trace_refs: true,
            ..supported_interpreter()
        })
        .unwrap_err();
        assert!(problem.contains("Py_TRACE_REFS"), "{problem}");
        assert!(problem.contains("sys.getobjects"), "{problem}");
        assert!(problem.contains("ob_refcnt"), "{problem}");
    }

    #[test]
    fn the_running_interpreter_is_probed() {
        let _heap = heap();
        Python::initialize();
        Python::attach(|py| {
            let interpreter = probe(py);
            let version = py.version_info();
            assert_eq!(
                (interpreter.major, interpreter.minor),
                (version.major, version.minor),
                "probed version"
            );
            assert_eq!(interpreter.pointer_width, usize::BITS, "probed width");
            assert_eq!(
                interpreter.little_endian,
                cfg!(target_endian = "little"),
                "probed endianness"
            );
            assert!(!interpreter.trace_refs, "a Py_TRACE_REFS build is running");
            if walkable(py) {
                assert!(
                    (3, MIN_MINOR) <= (version.major, version.minor)
                        && (version.major, version.minor) <= (3, MAX_MINOR),
                    "accepted an unknown version: {}.{}",
                    version.major,
                    version.minor
                );
                assert!(!free_threaded(py), "accepted a free-threaded build");
            } else {
                let problem = supported(&interpreter).unwrap_err();
                assert!(
                    problem.contains("immortal") || problem.contains("CPython"),
                    "unclear refusal: {problem}"
                );
            }
        });
    }

    #[test]
    fn an_unsupported_interpreter_is_refused_by_supported_alone() {
        // §9.2 #7 used to refuse to start here; the refusal is now only a
        // reason for `decide`, and every one of them can be turned into a skip.
        for unsupported in [
            Interpreter {
                pointer_width: 32,
                ..supported_interpreter()
            },
            Interpreter {
                little_endian: false,
                ..supported_interpreter()
            },
            Interpreter {
                minor: 15,
                ..supported_interpreter()
            },
            Interpreter {
                free_threaded: true,
                ..supported_interpreter()
            },
            Interpreter {
                trace_refs: true,
                ..supported_interpreter()
            },
        ] {
            let problem = supported(&unsupported).unwrap_err();
            for setting in [Immortalize::Auto, Immortalize::On] {
                let decision = decide(&unsupported, setting);
                assert!(!decision.is_on(), "{unsupported:?} with {setting:?}");
                let reason = decision.reason().unwrap();
                assert!(reason.contains(&problem), "{reason}");
                assert!(
                    reason.ends_with("views will copy more of the preloaded heap"),
                    "{reason}"
                );
            }
        }
    }

    #[test]
    fn the_version_and_the_setting_decide_the_pass() {
        // §9.2 #7: the marker is safe on 3.12 and 3.13, unsafe on 3.14.
        for minor in [MIN_MINOR, 13] {
            assert_eq!(decide(&interpreter(minor), Immortalize::Auto), Decision::On);
            assert_eq!(decide(&interpreter(minor), Immortalize::On), Decision::On);
        }

        // `auto` on 3.14: skipped, with the reason the log carries.
        let unsafe_here = decide(&interpreter(MAX_MINOR), Immortalize::Auto);
        assert!(!unsafe_here.is_on());
        assert_eq!(
            unsafe_here.reason().unwrap(),
            "CPython 3.14: heap immortalisation is unsafe on this version, views will copy more \
             of the preloaded heap"
        );
        // ... and `1` forces it there, for experiments.
        assert_eq!(
            decide(&interpreter(MAX_MINOR), Immortalize::On),
            Decision::On
        );

        // `0` never runs the pass, and says so before anything else.
        for minor in [MIN_MINOR, 13, MAX_MINOR] {
            let off = decide(&interpreter(minor), Immortalize::Off);
            assert!(!off.is_on());
            assert!(
                off.reason().unwrap().contains("BLASTIT_IMMORTALIZE=0"),
                "{off:?}"
            );
        }

        // `Decision::On` has no reason to give.
        assert_eq!(Decision::On.reason(), None);
    }

    /// The facts of a CPython every blastit build is tested against.
    fn supported_interpreter() -> Interpreter {
        Interpreter {
            major: 3,
            minor: 13,
            pointer_width: usize::BITS,
            little_endian: true,
            free_threaded: false,
            trace_refs: false,
        }
    }

    #[test]
    fn a_skipped_pass_walks_nothing() {
        let _heap = heap();
        Python::initialize();
        Python::attach(|py| {
            // `BLASTIT_IMMORTALIZE=0` never walks, whatever interpreter this is:
            // `apply` is the whole decision, and the heap is untouched.
            let globals = namespace(py, c"HOLD = {'marker': 'blastit' * 32}\n");
            let held = globals.get_item("HOLD").unwrap().unwrap();
            let marker = held.get_item("marker").unwrap();
            assert!(!is_immortal(marker.as_ptr()));

            let outcome = apply(py, Immortalize::Off).unwrap();
            assert!(!outcome.decision.is_on());
            assert_eq!(outcome.stats, None);
            assert_eq!(outcome.objects(), 0);
            assert_eq!(outcome.millis(), None);
            assert!(!is_immortal(marker.as_ptr()), "the heap was walked");
        });
    }

    #[test]
    fn the_running_interpreter_follows_the_same_decision() {
        let _heap = heap();
        Python::initialize();
        Python::attach(|py| {
            let version = py.version_info();
            // On this interpreter, `auto` runs the walk exactly when the pure
            // decision says it should; on 3.14 that is "no walk", which is the
            // point of the whole exercise.
            let outcome = apply(py, Immortalize::Auto).unwrap();
            let expected = decide(&probe(py), Immortalize::Auto);
            assert_eq!(outcome.decision, expected);
            assert_eq!(outcome.stats.is_some(), expected.is_on());
            if expected.is_on() {
                assert!(walkable(py), "an accepted interpreter can be walked");
                assert!(
                    version.major == 3 && (MIN_MINOR..=AUTO_MAX_MINOR).contains(&version.minor),
                    "only the interpreters auto trusts ran the walk: {}.{}",
                    version.major,
                    version.minor
                );
                assert!(outcome.objects() > 0, "nothing was immortalised");
                let millis = outcome.millis().unwrap();
                assert!(millis >= 0.0 && millis.is_finite(), "{millis}");
            } else {
                assert_eq!(outcome.objects(), 0);
                let reason = outcome.decision.reason().unwrap();
                assert!(reason.contains("views will copy"), "{reason}");
            }
        });
    }

    #[test]
    fn an_immortalised_object_stops_counting_references() {
        let _heap = heap();
        Python::initialize();
        Python::attach(|py| {
            if !walkable(py) {
                // The skip path is tested above; this one needs the walk.
                return;
            }
            let globals = namespace(
                py,
                c"HELD = {'marker': 'blastit' * 32, 'number': 12345678, 'nested': [1, 2, 3]}
",
            );
            let held = globals.get_item("HELD").unwrap().unwrap();
            let marker = held.get_item("marker").unwrap();
            assert!(!is_immortal(marker.as_ptr()), "immortal before the walk");

            let stats = immortalize(py).unwrap();
            assert!(stats.objects > 0, "nothing was immortalised");

            // §9.2 #7: the preloaded object is immortal, and taking and
            // dropping references does not move its count — no write, no copy.
            assert!(is_immortal(marker.as_ptr()));
            let before = getrefcount(py, &marker);
            assert!(
                before >= i64::from(IMMORTAL_LOW),
                "the marker is not in the count: {before}"
            );
            for _ in 0..1000 {
                let taken = held.get_item("marker").unwrap();
                drop(taken);
            }
            assert_eq!(getrefcount(py, &marker), before, "the count moved");

            // `gc.get_referents` reaches the containers too.
            assert!(is_immortal(held.as_ptr()), "a dict was missed");
            let nested = held.get_item("nested").unwrap();
            assert!(is_immortal(nested.as_ptr()), "a list was missed");
        });
    }

    #[test]
    fn an_object_created_afterwards_is_unaffected() {
        let _heap = heap();
        Python::initialize();
        Python::attach(|py| {
            if !walkable(py) {
                return;
            }
            immortalize(py).unwrap();

            let fresh = namespace(py, c"FRESH = {'a': [1, 2, 3]}\n");
            let fresh = fresh.get_item("FRESH").unwrap().unwrap();
            // A new allocation must not inherit the marker, and its count must
            // still work: incref and decref move it by one either way.
            assert!(!is_immortal(fresh.as_ptr()), "a new object was immortal");
            let before = getrefcount(py, &fresh);
            let taken = fresh.clone();
            assert_eq!(getrefcount(py, &fresh), before + 1);
            drop(taken);
            assert_eq!(getrefcount(py, &fresh), before);
        });
    }

    #[test]
    fn the_walk_reaches_untracked_objects() {
        let _heap = heap();
        Python::initialize();
        Python::attach(|py| {
            if !walkable(py) {
                return;
            }
            // A `str` and an `int` are not tracked by the collector, so they
            // can only be reached through a referent of a tracked object.
            let globals = namespace(
                py,
                c"MARKER = 'blastit-untracked-marker-string' * 3\nNUMBER = 12345678\nHOLDER = [MARKER, NUMBER]\n",
            );
            let marker = globals.get_item("MARKER").unwrap().unwrap();
            let number = globals.get_item("NUMBER").unwrap().unwrap();
            assert!(!is_immortal(marker.as_ptr()));

            let stats = immortalize(py).unwrap();
            assert!(stats.objects > 0);
            assert!(is_immortal(marker.as_ptr()), "a str was missed");
            assert!(is_immortal(number.as_ptr()), "an int was missed");
        });
    }
}
