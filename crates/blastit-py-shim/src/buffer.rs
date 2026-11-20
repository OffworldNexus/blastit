#![allow(unsafe_code)]

//! The one place a boundary reads a caller's bytes, writes a [`PyBufV1`], or
//! turns a panic into a refusal.
//!
//! Every buffer a shim returns is a Rust `Vec<u8>` whose pointer, length and
//! capacity travel out as [`PyBufV1`]. **The shim allocates, the caller frees**:
//! `out`/`err` must arrive zeroed (see [`PyBufV1`]'s ownership contract), a
//! non-null slot is overwritten rather than reinterpreted, and [`free`] is the
//! only reclaim path — the caller calls it through `free_buf`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use blastit_py_sapi::PyBufV1;
use blastit_sys::child::panic_message;
use serde::Serialize;

/// The refusal a boundary answers when its own body panicked (`70`).
///
/// Read by `lib.rs`'s `mod surface`, which is absent from a test build that
/// enabled `extension-module` (there is no `libpython` to link the `Py*`
/// references against), so `the_two_refusal_codes_are_the_frozen_ones` in the
/// tests below reads both here as well: a constant only a feature-gated module
/// uses is `-D dead-code` in the other target.
pub(crate) const PANIC: i32 = 70;

/// The refusal a boundary answers when it refused the request (`1`).
pub(crate) const REFUSED: i32 = 1;

/// The bytes of a caller's `(ptr, len)` request.
///
/// The lifetime is the caller's: `init_paths`/`preload`/`serve_view` hand a
/// descriptor-scoped slice that must outlive the whole call, exactly as the
/// frozen ABI requires. A null pointer is an empty request, never a panic.
pub(crate) fn input<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        return &[];
    }
    // SAFETY: the caller owns `len` bytes at `ptr` for the duration of the call
    // (design §1.2's `(req, req_len, out, err)` contract).
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// Writes `value` as JSON into `slot`.
pub(crate) fn write_json<T: Serialize>(slot: *mut PyBufV1, value: &T) {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| {
        Vec::from(&br#"{"error":"the shim could not serialise its answer"}"#[..])
    });
    store(slot, bytes);
}

/// Writes `message` into `slot` as `{"error": "..."}`.
pub(crate) fn write_error(slot: *mut PyBufV1, message: &str) {
    let bytes = serde_json::to_vec(&serde_json::json!({ "error": message }))
        .unwrap_or_else(|_| Vec::from(&br#"{"error":"refused"}"#[..]));
    store(slot, bytes);
}

/// Hands `bytes` to the caller and forgets the allocation.
///
/// The slot is **overwritten, never freed**: the shim cannot reconstruct a
/// `Vec` from a struct it may not have allocated, and the frozen contract says
/// the caller frees every buffer through `free_buf` before reusing its slot
/// ([`PyBufV1`]'s ownership section). A null slot is unreachable — every
/// boundary validates its `out`/`err` first — and is ignored rather than
/// dereferenced.
fn store(slot: *mut PyBufV1, bytes: Vec<u8>) {
    if slot.is_null() {
        return;
    }
    let mut bytes = std::mem::ManuallyDrop::new(bytes);
    // SAFETY: `slot` is non-null, so it is a writable `PyBufV1` owned by the
    // caller (design §1.2). The allocation is forgotten here and reclaimed by
    // `free` alone.
    unsafe {
        *slot = PyBufV1 {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        };
    }
}

/// Reclaims a buffer this shim allocated (`free_buf`).
///
/// A null pointer or a zero capacity is ignored: an empty answer is a dangling
/// pointer by `Vec`'s own convention, and freeing it would corrupt the heap. A
/// `len` larger than `cap` is refused the same way — a caller that lies about a
/// buffer's length must not be able to hand `from_raw_parts` a slice outside its
/// allocation.
pub(crate) fn free(buf: PyBufV1) {
    if buf.ptr.is_null() || buf.cap == 0 || buf.len > buf.cap {
        return;
    }
    // SAFETY: the pointer arrived from `store`, with a length no larger than its
    // capacity, and `free_buf` is the one caller.
    unsafe { drop(Vec::from_raw_parts(buf.ptr, buf.len, buf.cap)) };
}

/// Runs `body`, turning a panic into `refusal` plus one `err` sentence.
///
/// A panic may never unwind into C (`extern "C"` is not unwind-safe), so every
/// boundary is wrapped; the sentence is `blastit_sys::child::panic_message`, the
/// one spelling the child sequence uses.
pub(crate) fn guarded<F>(err: *mut PyBufV1, refusal: i32, body: F) -> i32
where
    F: FnOnce() -> i32,
{
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(code) => code,
        Err(payload) => {
            write_error(
                err,
                &format!("the Python shim panicked: {}", panic_message(&payload)),
            );
            refusal
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Answer {
        ok: bool,
    }

    fn empty() -> PyBufV1 {
        PyBufV1 {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    #[test]
    fn a_written_answer_round_trips_and_is_reclaimed_once() {
        let mut slot = empty();
        write_json(&mut slot as *mut PyBufV1, &Answer { ok: true });
        // SAFETY: `store` just wrote `len` initialised bytes at `ptr`.
        let bytes = unsafe { std::slice::from_raw_parts(slot.ptr, slot.len) };
        assert_eq!(bytes, br#"{"ok":true}"#);
        free(slot);
        // A null buffer is a no-op, not a second free of the same allocation.
        free(empty());
    }

    #[test]
    fn the_caller_reclaims_a_buffer_before_reusing_its_slot() {
        // The contract in `abi.rs`: the shim allocates, the caller frees, and a
        // slot handed back for a second answer is zeroed first.
        let mut slot = empty();
        write_json(&mut slot as *mut PyBufV1, &Answer { ok: true });
        free(slot);
        slot = empty();
        write_json(&mut slot as *mut PyBufV1, &Answer { ok: false });
        // SAFETY: the second `store` owns the slot now.
        let bytes = unsafe { std::slice::from_raw_parts(slot.ptr, slot.len) };
        assert_eq!(bytes, br#"{"ok":false}"#);
        free(slot);
    }

    #[test]
    fn a_null_slot_is_never_dereferenced() {
        // Boundaries refuse a null `out`/`err` with a sentence; `store` must
        // still not dereference one if that check is ever bypassed.
        write_json(std::ptr::null_mut(), &Answer { ok: true });
        write_error(std::ptr::null_mut(), "nothing to write to");
    }

    #[test]
    fn a_lying_length_is_refused_instead_of_corrupting_the_heap() {
        // A caller must not be able to hand `from_raw_parts` a slice that runs
        // past the allocation: `len > cap` is ignored, not trusted.
        let dangling = std::ptr::dangling_mut::<u8>();
        free(PyBufV1 {
            ptr: dangling,
            len: 3,
            cap: 1,
        });
        free(PyBufV1 {
            ptr: dangling,
            len: 4,
            cap: 0,
        });
    }

    #[test]
    fn an_error_is_json_on_the_error_channel() {
        let mut slot = empty();
        write_error(&mut slot as *mut PyBufV1, "no interpreter");
        // SAFETY: as above.
        let bytes = unsafe { std::slice::from_raw_parts(slot.ptr, slot.len) };
        let parsed: serde_json::Value = serde_json::from_slice(bytes).expect("valid JSON");
        assert_eq!(parsed["error"], "no interpreter");
        free(slot);
    }

    #[test]
    fn a_panic_becomes_a_refusal_and_a_sentence() {
        let mut slot = empty();
        let code = guarded(&mut slot as *mut PyBufV1, -1, || panic!("boom"));
        assert_eq!(code, -1);
        // SAFETY: the `guarded` failure path wrote the sentence.
        let bytes = unsafe { std::slice::from_raw_parts(slot.ptr, slot.len) };
        let parsed: serde_json::Value = serde_json::from_slice(bytes).expect("valid JSON");
        assert!(
            parsed["error"]
                .as_str()
                .is_some_and(|text| text.contains("boom")),
            "{parsed}"
        );
        free(slot);
    }

    #[test]
    fn a_null_request_is_empty_and_never_dereferenced() {
        assert_eq!(input(std::ptr::null(), 0), b"");
        assert_eq!(input(std::ptr::null(), 9), b"", "a null pointer wins");
        let bytes = [1u8, 2, 3];
        assert_eq!(input(bytes.as_ptr(), 2), &[1, 2]);
    }

    #[test]
    fn the_two_refusal_codes_are_the_frozen_ones() {
        // `mod surface` answers these (`serve_view` is `guarded(err, PANIC, …)`
        // and returns `REFUSED`), and that module is compiled out of a test build
        // that enabled `extension-module`: without a reader in this target the
        // pair is dead code under `-D dead-code`. The numbers are the frozen ABI
        // (`1` an ordinary refusal, `70` a panic), so they are asserted, not just
        // touched.
        assert_eq!(PANIC, 70, "the panic code is the frozen 70");
        assert_eq!(REFUSED, 1, "an ordinary refusal is the frozen 1");
    }
}
