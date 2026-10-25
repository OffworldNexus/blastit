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
//! low 32 bits of ob_refcnt = marker    CPython's own, per version ([`marker`])
//! ```
//!
//! The marker is CPython's own, and 3.14 changed its shape:
//!
//! * **3.12 and 3.13** keep the reference count in the whole 64-bit `ob_refcnt`
//!   and incref it by adding one to the low 32-bit half, skipping the write
//!   when that half wraps to zero; `Py_DECREF` skips an object whose count has
//!   the sign bit set. `0xFFFFFFFF` is therefore never counted up and never
//!   reaches zero: no write, no copy.
//! * **3.14** splits the word into `ob_refcnt` (`u32` at offset 0),
//!   `ob_overflow` (`u16` at 4) and `ob_flags` (`u16` at 6), and its own
//!   `_Py_SetImmortalUntracked` writes `0xC0000000` and sets
//!   `_Py_IMMORTAL_FLAGS` (bit 0) in `ob_flags`. Both halves are needed there,
//!   because 3.14 has incref and decref paths that do not test immortality at
//!   all:
//!
//!   1. `0xFFFFFFFF` is a landmine. `LOAD_CONST_MORTAL`, `PyStackRef_DUP`
//!      (`LOAD_FAST`, `COPY`, `FOR_ITER`, …) and
//!      `PyStackRef_FromPyObjectNewMortal` incref through `_Py_INCREF_MORTAL`,
//!      which is a bare `op->ob_refcnt++` with no `_Py_IsImmortal` test. One
//!      more reference to a `0xFFFFFFFF` object wraps the count to `0`,
//!      `_Py_IsImmortal` then answers "mortal", and a later decref accepts that
//!      answer and frees an object that is still referenced: a use-after-free
//!      that corrupts the pymalloc freelist and segfaults in
//!      `_PyObject_Malloc`. `0xC0000000` leaves 2^30 of headroom both ways —
//!      increfs are omitted from `0xC0000000` up, decrefs from the sign bit up
//!      — so no mortal-only path can wrap it inside a container's lifetime.
//!   2. `PyStackRef_FromPyObjectSteal` decides whether a reference it is handed
//!      already has its own reference count by reading
//!      `ob_flags & _Py_IMMORTAL_FLAGS` (`Py_TAG_REFCNT`), not by reading the
//!      count, and `PyStackRef_CLOSE` on an untagged reference goes through
//!      `_Py_DECREF_MORTAL`, also unchecked. A count-only marker therefore
//!      drifts down by a unit per steal; the flag is what tells 3.14's stack
//!      that the object it points at is already immortal.
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

/// The low-32-bit marker on CPython 3.12 and 3.13, where `ob_refcnt` is the
/// whole 64-bit word and `0xFFFFFFFF` is `_Py_IMMORTAL_REFCNT`
/// (`UINT_MAX`) ([`marker`]).
const CLASSIC_REFCNT: u32 = u32::MAX;

/// The low-32-bit marker on CPython 3.14: `_Py_IMMORTAL_INITIAL_REFCNT`
/// (`3 << 30`), the count CPython's own `_Py_SetImmortalUntracked` writes
/// ([`marker`]).
const FLAGGED_REFCNT: u32 = 0xC000_0000;

/// `_Py_IMMORTAL_FLAGS`: the `ob_flags` bit 3.14's stack references read to
/// decide whether a reference is already immutable ([`Marker::immortal_flag`]).
const IMMORTAL_FLAGS: u16 = 0x0001;

/// Where `ob_flags` sits in a 3.14 `PyObject` on a little-endian build: after
/// `uint32_t ob_refcnt` (0), `uint16_t ob_overflow` (4), before `ob_type`.
const FLAGS_OFFSET: usize = 6;

/// The low 32 bits at or above which an object already counts as immortal: the
/// sign bit of the count half, which is exactly `_Py_IsImmortal` on the 64-bit
/// builds blastit marks.
const IMMORTAL_SIGN_BIT: u32 = 1 << 31;

/// What §9.2 #7 writes over one object to make it immortal ([`marker`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    /// The low 32 bits of `ob_refcnt` — the whole reference count on 3.12 and
    /// 3.13, and the count half of the split word on 3.14.
    pub refcnt: u32,
    /// Whether `_Py_IMMORTAL_FLAGS` is OR-ed into `ob_flags`, the `u16` at
    /// offset 6, too. False on 3.12 and 3.13, which have no `ob_flags`.
    pub immortal_flag: bool,
}

/// §9.2 #7's marker for one interpreter: CPython's own value for the version
/// that will read it.
///
/// This is the whole version policy of the pass, and a pure function of the
/// facts [`probe`] reads, so every branch is unit-testable without that build
/// being installed.
///
/// The count alone is the marker on 3.12 and 3.13: `Py_INCREF` adds one to the
/// low 32 bits and skips the write when they wrap, so the count never moves
/// again. 3.14 needs both halves — the count and the flag — because two of its
/// paths (`_Py_INCREF_MORTAL`, `_Py_DECREF_MORTAL`) do not test immortality at
/// all; the module docs work through both.
///
/// Only 3.12–3.14 reach a write: [`supported`] refuses every other interpreter
/// and [`decide`] turns the refusal into a skip. A version past 3.14, if it is
/// ever accepted, gets 3.14's split word, which is the shape that version
/// introduced.
pub const fn marker(interpreter: &Interpreter) -> Marker {
    if interpreter.major > 3 || (interpreter.major == 3 && interpreter.minor >= 14) {
        Marker {
            refcnt: FLAGGED_REFCNT,
            immortal_flag: true,
        }
    } else {
        Marker {
            refcnt: CLASSIC_REFCNT,
            immortal_flag: false,
        }
    }
}

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
/// 3.12, 3.13 and 3.14, the last of which writes its own split-word marker
/// instead ([`marker`]).
pub const MAX_MINOR: u8 = 14;

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
/// | `BLASTIT_IMMORTALIZE` | 3.12–3.14 | anything else |
/// |---|---|---|
/// | `auto` (default) | on | off: the interpreter is unsupported |
/// | `1` | on | off: the interpreter is unsupported |
/// | `0` | off | off |
///
/// Each version has a marker that holds *its* reference counts still, 3.14
/// included ([`marker`]), so the version only decides whether blastit knows the
/// interpreter at all: `1` and `auto` say the same thing, and differ only in
/// what an operator asked for. The setting decides before the interpreter does,
/// so an operator who turned the pass off is told that, and only then what a
/// version could not do. Every [`Decision::Off`] ends with what skipping costs,
/// so the sentence reads as one line in the log whatever the reason.
pub fn decide(interpreter: &Interpreter, setting: Immortalize) -> Decision {
    if setting == Immortalize::Off {
        return Decision::Off(format!(
            "BLASTIT_IMMORTALIZE=0: heap immortalisation is off{SKIPPED_COST}"
        ));
    }
    if let Err(problem) = supported(interpreter) {
        return Decision::Off(format!("{problem}{SKIPPED_COST}"));
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
/// The marker is the one this interpreter reads ([`marker`]): `0xFFFFFFFF` on
/// 3.12 and 3.13, `0xC0000000` plus `ob_flags`'s `_Py_IMMORTAL_FLAGS` on 3.14.
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
    let marker = marker(&probe(py));

    let started = Instant::now();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut queue: VecDeque<Py<PyAny>> = VecDeque::new();
    let mut objects = 0_u64;

    for object in get_objects.call0()?.try_iter()? {
        discover(&mut seen, &mut queue, &object?, marker, &mut objects);
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
            discover(&mut seen, &mut queue, &referent?, marker, &mut objects);
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
    marker: Marker,
    objects: &mut u64,
) {
    if !seen.insert(object.as_ptr() as usize) {
        return;
    }
    // SAFETY: `object` is a live Python object and the GIL is held, so its
    // reference count is ours to write.
    if unsafe { write_marker(object.as_ptr(), marker) } {
        *objects += 1;
    }
    queue.push_back(object.clone().unbind());
}

/// Writes `marker` into the object's reference count, and returns whether it
/// wrote anything.
///
/// The low 32 bits always are the count: the whole reference count on 3.12 and
/// 3.13, and the count half of 3.14's split word, whose upper half holds
/// `ob_overflow` and `ob_flags` that a 64-bit store would clobber. When the
/// marker carries a flag, `_Py_IMMORTAL_FLAGS` is OR-ed into `ob_flags` as
/// well, so every other bit of it — `_Py_STATICALLY_ALLOCATED_FLAG` in
/// particular — survives.
///
/// An object whose count already has the sign bit set is left alone: it is
/// immortal (`_Py_IsImmortal`), so it already has whatever marker and flags
/// CPython gave it, and this pass has nothing to add.
///
/// # Safety
///
/// `object` must point at a live Python object, and the caller must hold the
/// GIL (`PyObject` is not `Sync`). `marker` must be [`marker`]'s value for the
/// running interpreter: on 3.12 and 3.13, offset 6 is still part of the
/// reference count, and a flag written there would corrupt it.
unsafe fn write_marker(object: *mut ffi::PyObject, marker: Marker) -> bool {
    // SAFETY: a `PyObject` is 8-byte aligned (it starts with the reference
    // count and a pointer), so the first four bytes are a valid, aligned
    // `u32`; the GIL means nothing else is writing it.
    let low = object.cast::<u32>();
    if unsafe { low.read_volatile() } & IMMORTAL_SIGN_BIT != 0 {
        return false;
    }
    // SAFETY: as above. The store is volatile so the compiler cannot cache the
    // old count across the Python calls that follow.
    unsafe { low.write_volatile(marker.refcnt) };
    if marker.immortal_flag {
        // SAFETY: `ob_flags` is the `u16` at offset 6 — 2-byte aligned inside
        // the 8-byte-aligned object, and past both `ob_refcnt` and
        // `ob_overflow` on 3.14 — so it is a valid, aligned `u16` that the GIL
        // keeps to ourselves.
        let flags = unsafe { object.cast::<u8>().add(FLAGS_OFFSET) }.cast::<u16>();
        let updated = unsafe { flags.read_volatile() } | IMMORTAL_FLAGS;
        // SAFETY: as above. Volatile for the same reason as the count store.
        unsafe { flags.write_volatile(updated) };
    }
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

    /// Whether the object already carries §9.2 #7's marker: its count is in the
    /// immortal range, which is what `_Py_IsImmortal` reads.
    fn is_immortal(object: *mut ffi::PyObject) -> bool {
        low32(object) & IMMORTAL_SIGN_BIT != 0
    }

    /// The low 32 bits of the object's reference count, as a marker writes it.
    fn low32(object: *mut ffi::PyObject) -> u32 {
        // SAFETY: the caller passes a live object and holds the GIL.
        unsafe { object.cast::<u32>().read_volatile() }
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
    ///
    /// Fixture strings are built from a name (`WORD * 32`), never written as a
    /// constant expression: CPython folds `'x' * 32` into one string object and
    /// interns it, so every snippet that spells it out shares that single
    /// object, and a pass another test has run immortalises it — which makes
    /// "not immortal before the walk" below depend on the order libtest
    /// happened to run the tests in. A string built at runtime is a fresh
    /// object no pass can have seen yet.
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
        // §9.2 #7: every version blastit knows has a marker that holds its
        // reference counts still, 3.14 included, so both spellings run it.
        for minor in [MIN_MINOR, 13, MAX_MINOR] {
            assert_eq!(decide(&interpreter(minor), Immortalize::Auto), Decision::On);
            assert_eq!(decide(&interpreter(minor), Immortalize::On), Decision::On);
        }

        // A version outside the range is skipped with the interpreter's own
        // reason, whichever spelling asked for the pass.
        for minor in [MIN_MINOR - 1, MAX_MINOR + 1] {
            for setting in [Immortalize::Auto, Immortalize::On] {
                let off = decide(&interpreter(minor), setting);
                assert!(!off.is_on(), "3.{minor} with {setting:?}");
                let reason = off.reason().unwrap();
                assert!(reason.contains(&format!("CPython 3.{minor}")), "{reason}");
                assert!(reason.contains("views will copy"), "{reason}");
            }
        }

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

    #[test]
    fn the_marker_follows_the_version() {
        // 3.12 and 3.13 count in the whole word, and `0xFFFFFFFF` —
        // `_Py_IMMORTAL_REFCNT` there — is what `Py_INCREF`'s wrap-around
        // never lets past its own skip.
        for minor in [MIN_MINOR, 13] {
            assert_eq!(
                marker(&interpreter(minor)),
                Marker {
                    refcnt: u32::MAX,
                    immortal_flag: false,
                },
                "3.{minor} takes the classic marker alone"
            );
        }

        // 3.14 splits the word: `_Py_IMMORTAL_INITIAL_REFCNT` (`3 << 30`) in
        // the count half, and `_Py_IMMORTAL_FLAGS` in `ob_flags`, which is
        // what `_Py_SetImmortalUntracked` — CPython's own immortalisation —
        // writes there.
        assert_eq!(
            marker(&interpreter(MAX_MINOR)),
            Marker {
                refcnt: 0xC000_0000,
                immortal_flag: true,
            },
            "3.14 takes CPython's split-word marker"
        );

        // The split word is the shape from 3.14 on: a version past it (refused
        // before it could be marked) and an unknown major keep it, and
        // anything older than 3.12 — also refused — has the whole-word count.
        assert_eq!(
            marker(&interpreter(MAX_MINOR + 1)),
            marker(&interpreter(MAX_MINOR))
        );
        assert_eq!(
            marker(&interpreter(MIN_MINOR - 1)),
            Marker {
                refcnt: u32::MAX,
                immortal_flag: false,
            }
        );
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
            let globals = namespace(py, c"WORD = 'blastit'\nHOLD = {'marker': WORD * 32}\n");
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
            // A fresh object, created while this test holds the heap lock: no
            // pass another test ran can have marked it, so it says whether
            // *this* one walked. (The count cannot: a heap a previous test
            // immortalised is walked and matches nothing.)
            let globals = namespace(py, c"WORD = 'blastit'\nHELD = {'marker': WORD * 32}\n");
            let marker = globals.get_item("HELD").unwrap().unwrap();
            assert!(!is_immortal(marker.as_ptr()), "immortal before the pass");

            // On this interpreter, `auto` runs the walk exactly when the pure
            // decision says it should: on every version blastit knows, which is
            // the point of the whole exercise — 3.14 included since C3c.
            let outcome = apply(py, Immortalize::Auto).unwrap();
            let expected = decide(&probe(py), Immortalize::Auto);
            assert_eq!(outcome.decision, expected);
            assert_eq!(outcome.stats.is_some(), expected.is_on());
            if expected.is_on() {
                assert!(walkable(py), "an accepted interpreter can be walked");
                assert!(
                    version.major == 3 && (MIN_MINOR..=MAX_MINOR).contains(&version.minor),
                    "only the interpreters blastit knows ran the walk: {}.{}",
                    version.major,
                    version.minor
                );
                assert!(is_immortal(marker.as_ptr()), "the heap was not walked");
                assert_eq!(
                    low32(marker.as_ptr()),
                    super::marker(&probe(py)).refcnt,
                    "the walk wrote the marker this interpreter reads"
                );
                let millis = outcome.millis().unwrap();
                assert!(millis >= 0.0 && millis.is_finite(), "{millis}");
            } else {
                assert_eq!(outcome.objects(), 0);
                assert_eq!(outcome.millis(), None);
                assert!(!is_immortal(marker.as_ptr()), "the heap was walked");
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
                c"WORD = 'blastit'\nHELD = {'marker': WORD * 32, 'number': 12345678, 'nested': [1, 2, 3]}
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
                before >= i64::from(IMMORTAL_SIGN_BIT),
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
                c"WORD = 'blastit-untracked-marker-string'\nMARKER = WORD * 3\nNUMBER = 12345678\nHOLDER = [MARKER, NUMBER]\n",
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

    /// The environment variable that turns this test binary into the marker
    /// child ([`marker_child`]).
    ///
    /// It names the marker to write: `classic` for the `0xFFFFFFFF` §9.2 #7
    /// wrote everywhere before C3c, and anything else for [`marker`]'s choice
    /// for the running interpreter.
    const CHILD_MARKER: &str = "BLASTIT_IMMORTAL_CHILD";

    /// [`marker_child`]'s name as libtest knows it, for the child's `--exact`
    /// filter: libtest names a unit test by its module path, with the crate
    /// name that [`module_path!`] starts with left off.
    const CHILD_TEST: &str = concat!(module_path!(), "::marker_child");

    /// How many times the child walks its marked objects in the round the
    /// report calls `many`: enough that a count moving per walk leaves the
    /// immortal range in plain sight, and that a count that settled after the
    /// first walk is seen not to move again.
    const MANY_WALKS: usize = 32;

    /// The child's namespace: three objects, and the interpreter traffic
    /// §9.2 #7 has to survive.
    ///
    /// `probe` is called by name from inside `drive`, and hands its object
    /// back: the shape C3c's report bisected to. A `for` loop over marked
    /// objects, two locals per iteration (`PyStackRef_DUP`, and with it
    /// `_Py_INCREF_MORTAL`) and one Python call per object. In a plain CPython
    /// 3.14 that shape wraps a `0xFFFFFFFF` count to zero, frees an object that
    /// is still referenced and dies in `_PyObject_Malloc` — five of five
    /// processes — where five of five carrying CPython's own marker lived; the
    /// same traffic took a real Django app's views down on their first request.
    /// Inside this embedded interpreter the same shape is gentler and no less
    /// final: it is the classic count that moves, one decref at a time (see
    /// [`the_3_14_wrap_is_survived_by_the_marker`]).
    const CHILD_SETUP: &std::ffi::CStr = c"
class C:
    pass


SEQ = [C(), C(), C()]


def probe(o):
    return o


def drive(seq):
    for o in seq:
        y = o
        z = o
        probe(o)
";

    /// Prints one line of the child's report, and gets it out before the next
    /// step can kill this process.
    fn report(line: serde_json::Value) {
        println!("{line}");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }

    /// `sys._is_immortal` for each object, on the interpreters that have it
    /// (3.14); `None` where the question cannot be asked.
    fn immortal_flags(py: Python<'_>, objects: &[Bound<'_, PyAny>]) -> Option<Vec<bool>> {
        let is_immortal = py.import("sys").ok()?.getattr("_is_immortal").ok()?;
        objects
            .iter()
            .map(|object| is_immortal.call1((object,)).ok()?.is_truthy().ok())
            .collect()
    }

    /// Stops a child the walk kills from writing a core file: the signal is the
    /// evidence, the dump is not.
    fn no_core_dumps() {
        let none = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `setrlimit` only lowers this process's own core-file limit,
        // and the return value is the kernel's answer about a limit nothing
        // here depends on.
        unsafe { libc::setrlimit(libc::RLIMIT_CORE, &none) };
    }

    /// [`the_3_14_wrap_is_survived_by_the_marker`]'s child: this same test
    /// binary, re-run with [`CHILD_MARKER`] naming the marker to write.
    ///
    /// A child *process*, because a marker that does not hold frees a live
    /// object and takes the interpreter down with it: the parent has to be able
    /// to read the wreckage instead of dying with it. The child prints what it
    /// wrote *before* the walk, so even a child the walk kills leaves evidence,
    /// and reads every count itself so no Python traffic stands between the
    /// marker and the reading.
    #[test]
    #[ignore = "the child of the_3_14_wrap_is_survived_by_the_marker: this binary, re-run with BLASTIT_IMMORTAL_CHILD"]
    fn marker_child() {
        let Ok(which) = std::env::var(CHILD_MARKER) else {
            // `cargo test -- --ignored` by hand: there is no marker to write.
            return;
        };
        no_core_dumps();
        Python::initialize();
        Python::attach(|py| {
            let marker = if which == "classic" {
                // What §9.2 #7 wrote everywhere before C3c: the classic count,
                // and no flag, whatever interpreter is running.
                Marker {
                    refcnt: u32::MAX,
                    immortal_flag: false,
                }
            } else {
                marker(&probe(py))
            };
            let globals = namespace(py, CHILD_SETUP);
            let sequence = globals.get_item("SEQ").unwrap().unwrap();
            let objects: Vec<Bound<'_, PyAny>> = sequence
                .try_iter()
                .unwrap()
                .collect::<PyResult<Vec<_>>>()
                .unwrap();

            let mut marked = 0;
            let mut before = Vec::with_capacity(objects.len());
            for object in &objects {
                // SAFETY: `object` is a live Python object and this holds the
                // GIL, which is what `write_marker` asks for.
                if unsafe { write_marker(object.as_ptr(), marker) } {
                    marked += 1;
                }
                before.push(low32(object.as_ptr()));
            }
            let version = py.version_info();
            report(serde_json::json!({
                "stage": "marked",
                "version": [version.major, version.minor],
                "refcnt": marker.refcnt,
                "immortal_flag": marker.immortal_flag,
                "count": objects.len(),
                "marked": marked,
                "before": before,
            }));

            // One walk, then many: the first is where 3.14's unchecked incref
            // wraps a `0xFFFFFFFF` count, the rest say whether what the marker
            // left behind keeps moving.
            let walk = || {
                py.run(c"drive(SEQ)\n", Some(&globals), None).unwrap();
                objects
                    .iter()
                    .map(|object| low32(object.as_ptr()))
                    .collect::<Vec<u32>>()
            };
            let one = walk();
            let mut many = one.clone();
            for _ in 1..MANY_WALKS {
                many = walk();
            }
            report(serde_json::json!({
                "stage": "after",
                "one": one,
                "many": many,
                "immortal": immortal_flags(py, &objects),
            }));
        });
    }

    /// One run of [`marker_child`], as the parent sees it.
    struct ChildRun {
        /// How the child ended.
        status: std::process::ExitStatus,
        /// Everything the child printed, its report lines included.
        stdout: String,
        /// Everything it printed on stderr.
        stderr: String,
    }

    impl ChildRun {
        /// The child's report for `stage`, JSON-parsed, when it printed one.
        fn stage(&self, stage: &str) -> Option<serde_json::Value> {
            self.stdout
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .find(|value| value["stage"].as_str() == Some(stage))
        }

        /// How the child ended, for a failure message.
        fn ending(&self) -> String {
            use std::os::unix::process::ExitStatusExt;
            match (self.status.code(), self.status.signal()) {
                (Some(code), _) => format!("exit {code}"),
                (None, Some(signal)) => format!("killed by signal {signal}"),
                (None, None) => "ended without a status".to_string(),
            }
        }
    }

    /// Re-runs this test binary as [`marker_child`], with `which` in
    /// [`CHILD_MARKER`].
    fn run_marker_child(which: &str) -> ChildRun {
        let binary = std::env::current_exe().expect("this test binary's own path");
        // `module_path!` starts with the crate name; libtest's own test names
        // do not.
        let name = CHILD_TEST
            .strip_prefix(concat!(env!("CARGO_PKG_NAME"), "::"))
            .unwrap_or(CHILD_TEST);
        let output = std::process::Command::new(binary)
            .args([name, "--exact", "--ignored", "--nocapture"])
            .env(CHILD_MARKER, which)
            .output()
            .expect("re-running this test binary as the marker child");
        ChildRun {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// A count array from one of the child's report fields.
    fn counts(value: &serde_json::Value) -> Vec<u32> {
        value
            .as_array()
            .unwrap_or_else(|| panic!("not an array of counts: {value}"))
            .iter()
            .map(|count| {
                count
                    .as_u64()
                    .unwrap_or_else(|| panic!("not a count: {count}")) as u32
            })
            .collect()
    }

    /// What one child's report says about the marker it wrote.
    #[derive(Debug, PartialEq, Eq)]
    enum Survived {
        /// The child was killed before it could report: under that marker, a
        /// live object was freed.
        Killed,
        /// Every object stayed in the immortal range, nothing counted it up,
        /// and its count was the same after one walk and after [`MANY_WALKS`].
        Stable,
        /// Anything else, in one line.
        Broke(String),
    }

    impl Survived {
        /// Reads `child`'s report; [`Survived::Stable`] is a marker that did
        /// its job — a count §9.2 #7 can promise never moves — and the only
        /// outcome the pass may produce.
        fn of(child: &ChildRun) -> Self {
            let Some(after) = child.stage("after") else {
                return Self::Killed;
            };
            let one = counts(&after["one"]);
            let many = counts(&after["many"]);
            if let Some(count) = one.iter().find(|count| *count & IMMORTAL_SIGN_BIT == 0) {
                return Self::Broke(format!("a count left the immortal range: {count:#x}"));
            }
            if let Some(flags) = after["immortal"].as_array()
                && flags.iter().any(|flag| flag.as_bool() != Some(true))
            {
                return Self::Broke(format!("sys._is_immortal says no: {flags:?}"));
            }
            if let Some(before) = child
                .stage("marked")
                .map(|marked| counts(&marked["before"]))
                && one != before
            {
                return Self::Broke(format!(
                    "the walk wrote a count it should not have: {before:x?} then {one:x?}"
                ));
            }
            if one != many {
                return Self::Broke(format!(
                    "the count kept moving between one walk and {MANY_WALKS}: {one:x?} then {many:x?}"
                ));
            }
            Self::Stable
        }
    }

    /// This interpreter's version and marker, read while holding the heap lock.
    ///
    /// `None` when §9.2 #7 does not know the interpreter, which is the whole
    /// reason `apply` would skip it and there would be no marker to look at.
    fn this_interpreter() -> Option<(u8, u8, Marker)> {
        let _heap = heap();
        Python::initialize();
        Python::attach(|py| {
            if !walkable(py) {
                return None;
            }
            let version = py.version_info();
            Some((version.major, version.minor, marker(&probe(py))))
        })
    }

    /// §9.2 #7 on 3.14: three objects marked with CPython's own marker come
    /// through the interpreter's stack-reference traffic with their counts
    /// exactly where the marker left them, and still immortal; the same traffic
    /// writes the classic marker's count — the first step of the wrap that ends
    /// with a live object freed and a process killed.
    ///
    /// Each marker gets a child process of its own ([`marker_child`]), because
    /// a marker that does not hold can free a live object, and that is not
    /// something a test can watch from inside the process it happens to. The
    /// lock is held only for the moment this interpreter's own facts are read:
    /// the children bring their own interpreters.
    #[test]
    fn the_3_14_wrap_is_survived_by_the_marker() {
        let Some((major, minor, expected)) = this_interpreter() else {
            return;
        };

        // The marker blastit writes for this interpreter, which has to hold —
        // on 3.13 the classic count, on 3.14 CPython's split word.
        let auto = run_marker_child("auto");
        let marked = auto.stage("marked").unwrap_or_else(|| {
            panic!(
                "the auto child printed no report ({}):\n{}\n{}",
                auto.ending(),
                auto.stdout,
                auto.stderr
            )
        });
        assert_eq!(
            marked["version"],
            serde_json::json!([major, minor]),
            "{marked}"
        );
        assert_eq!(marked["count"].as_u64(), Some(3), "{marked}");
        assert_eq!(
            marked["marked"].as_u64(),
            marked["count"].as_u64(),
            "{marked}"
        );
        assert_eq!(
            marked["refcnt"].as_u64(),
            Some(u64::from(expected.refcnt)),
            "{marked}"
        );
        assert_eq!(
            marked["immortal_flag"].as_bool(),
            Some(expected.immortal_flag),
            "{marked}"
        );
        assert_eq!(
            counts(&marked["before"]),
            [expected.refcnt; 3],
            "the marker as written: {marked}"
        );
        assert_eq!(
            Survived::of(&auto),
            Survived::Stable,
            "the marker did not hold on {major}.{minor}:\n{}\n{}",
            auto.stdout,
            auto.stderr
        );

        // `0xFFFFFFFF` — what §9.2 #7 wrote before C3c — has to do the
        // opposite there. Only 3.14 has the unchecked stack-reference paths
        // that write and wrap it, so only there does the contrast mean
        // anything; the report this step is built on measured a segfault in
        // `_PyObject_Malloc` on 5 of 5 processes.
        if minor >= MAX_MINOR {
            let classic = run_marker_child("classic");
            assert_ne!(
                Survived::of(&classic),
                Survived::Stable,
                "0xFFFFFFFF held on {major}.{minor}, so the run above proves nothing:\n{}\n{}",
                classic.stdout,
                classic.stderr
            );
        }
    }
}
