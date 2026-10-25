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
//!
//! Immortal objects are never freed, which §9.2 #7 accepts for a heap that
//! lives as long as the container. Anything else — 32-bit, big-endian, an
//! unknown CPython, or a free-threaded build, whose counts live in
//! `ob_ref_local` and `ob_ref_shared` instead — is refused with a message
//! rather than half-done ([`check`]).

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

/// The immortal marker, as the low 32 bits of `ob_refcnt`.
const IMMORTAL_LOW: u32 = u32::MAX;

/// How many objects are handed to `gc.get_referents` in one call.
///
/// It takes `*objects` and returns one flat list, so batching cuts the number
/// of Python calls by three orders of magnitude without changing the walk.
const BATCH: usize = 1024;

/// The first CPython minor whose marker blastit writes, on `3.x`.
pub const MIN_MINOR: u8 = 12;

/// The last CPython minor whose marker blastit writes, on `3.x`.
pub const MAX_MINOR: u8 = 14;

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
/// #7: "blastit refuses to start on a version it doesn't know").
///
/// # Errors
///
/// A sentence for the log, naming what is unsupported and why.
pub fn check(py: Python<'_>) -> Result<(), String> {
    if !cfg!(target_pointer_width = "64") {
        return Err(format!(
            "heap immortalisation needs 64-bit CPython: this is a {}-bit build",
            usize::BITS
        ));
    }
    if !cfg!(target_endian = "little") {
        return Err("heap immortalisation needs a little-endian CPython".to_string());
    }
    let version = py.version_info();
    if (version.major, version.minor) < (3, MIN_MINOR)
        || (version.major, version.minor) > (3, MAX_MINOR)
    {
        return Err(format!(
            "CPython {}.{}.{}: §9.2 #7's immortal marker is known for 3.{MIN_MINOR} to 3.{MAX_MINOR} only",
            version.major, version.minor, version.patch
        ));
    }
    if free_threaded(py) {
        return Err(
            "the free-threaded CPython build (Py_GIL_DISABLED) keeps its reference counts in \
             ob_ref_local and ob_ref_shared, where §9.2 #7's marker means nothing"
                .to_string(),
        );
    }
    Ok(())
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
/// # Errors
///
/// Whatever importing `gc` or calling it raises. A caller that cannot
/// immortalise must not start: a view would then copy the shared heap.
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
    use pyo3::types::PyDict;

    use super::*;

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

    #[test]
    fn an_unsupported_interpreter_is_refused_with_a_reason() {
        Python::initialize();
        Python::attach(|py| match check(py) {
            Ok(()) => {
                let version = py.version_info();
                assert!(
                    (3, MIN_MINOR) <= (version.major, version.minor)
                        && (version.major, version.minor) <= (3, MAX_MINOR),
                    "accepted an unknown version: {}.{}",
                    version.major,
                    version.minor
                );
                assert!(!free_threaded(py), "accepted a free-threaded build");
            }
            Err(problem) => {
                assert!(
                    problem.contains("immortal") || problem.contains("CPython"),
                    "unclear refusal: {problem}"
                );
            }
        });
    }

    #[test]
    fn an_immortalised_object_stops_counting_references() {
        Python::initialize();
        Python::attach(|py| {
            if check(py).is_err() {
                // The refusal path is the test above; this one needs the walk.
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
        Python::initialize();
        Python::attach(|py| {
            if check(py).is_err() {
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
        Python::initialize();
        Python::attach(|py| {
            if check(py).is_err() {
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
