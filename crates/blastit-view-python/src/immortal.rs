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
//!
//! # `unsafe` in this module, and nowhere else in the crate
//!
//! Writing CPython's reference count and its `ob_flags` is raw memory, so this
//! module carries the crate's **one** `#[allow(unsafe_code)]` and the crate is
//! `deny(unsafe_code)` rather than `forbid`. Everything else here — the
//! version policy, the decision, the walk — is safe Rust, and the marker write
//! is a function whose documented precondition is "a live object with the GIL
//! held".
//!
//! `BLASTIT_IMMORTALIZE`'s parsing is the composition root's
//! (`blastit/src/config.rs`); [`Setting`] is the local spelling it maps into,
//! so this crate never names a root type.

#![allow(unsafe_code)]

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

/// When §9.2 #7's heap immortalisation runs.
///
/// This is the interpreter crate's own spelling of the operator setting; the
/// composition root maps `BLASTIT_IMMORTALIZE` (`blastit::config::Immortalize`)
/// into it in six lines, so nothing here reads the environment and nothing here
/// knows about the root's configuration document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Immortalise wherever the marker is known — CPython 3.12 to 3.14, as
    /// [`supported`] decides — and skip it everywhere else. The default.
    Auto,
    /// The same interpreters as [`Setting::Auto`], asked for by an operator
    /// rather than assumed: `1` says the pass must run, and the interpreter
    /// still has the last word about whether its marker is known at all.
    On,
    /// Never immortalise, whatever the interpreter.
    Off,
}

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
/// facts the probe reads, so every branch is unit-testable without that build
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
pub fn decide(interpreter: &Interpreter, setting: Setting) -> Decision {
    if setting == Setting::Off {
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
pub fn apply(py: Python<'_>, setting: Setting) -> PyResult<Outcome> {
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
/// question that cannot be asked means "not free-threaded"; [`supported`] still
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
