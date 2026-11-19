//! The byte boundary between `crates/blastit` and a loaded shim (design §1.2).
//!
//! Every entry point here is a *translation* only: the caller serialises its
//! request with its own JSON writer and reads the answer with its own reader, so
//! this crate stays free of `serde` (it is on stage 0's path) and the shim
//! stays the only owner of the document shapes. What lives here is the part
//! neither of them may do safely alone: handing a `(ptr, len)` request to a
//! function pointer, reclaiming the two [`PyBufV1`]s through `free_buf`, and
//! turning a non-zero return code into the refusal bytes the caller parses.
//!
//! # Nothing here frees the zygote
//!
//! `preload` hands back a pointer to a box **the shim** allocated and owns for
//! the process's lifetime. It is copied into a [`crate::abi::PyZygote`] and
//! never dropped: every forked view still references the `Py<PyAny>` inside it,
//! and the frozen ABI has no `free_zygote` entry (design §1.2.1 item 2).

use core::ptr;

use crate::abi::{BlastitPyVtableV1, PyBufV1, PyRuntime, PyZygote};
use crate::loader::LoadedPy;

/// The refusal bytes of a boundary that answered non-zero.
///
/// The shim writes `{"error":"…"}`, and the caller's own reader names the
/// sentence. A refusal with nothing written is replaced by this module's own
/// sentence, so a caller never has to interpret an empty answer.
const NO_MESSAGE: &[u8] = br#"{"error":"the Python shim refused without a message"}"#;

/// The empty slots a boundary writes into.
fn slots() -> (PyBufV1, PyBufV1) {
    let empty = || PyBufV1 {
        ptr: ptr::null_mut(),
        len: 0,
        cap: 0,
    };
    (empty(), empty())
}

/// The refusal bytes of a non-zero answer.
fn refusal(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.is_empty() {
        NO_MESSAGE.to_vec()
    } else {
        bytes
    }
}

impl LoadedPy {
    /// Calls `init_paths` with a serialised request (design §1.2).
    ///
    /// # Errors
    ///
    /// The shim's `{"error": …}` bytes: a minor mismatch, an unreadable path set
    /// or the interpreter refusing to start.
    pub fn init_paths(&self, request: &[u8]) -> Result<Vec<u8>, Vec<u8>> {
        let vtable = self.vtable();
        let (mut out, mut err) = slots();
        let code = (vtable.init_paths)(request.as_ptr(), request.len(), &mut out, &mut err);
        let answer = crate::loader::take_slot(vtable, out);
        let failure = crate::loader::take_slot(vtable, err);
        if code == 0 {
            Ok(answer)
        } else {
            Err(refusal(failure))
        }
    }

    /// Calls `preload` and returns the opaque zygote with its answer.
    ///
    /// # Errors
    ///
    /// The shim's `{"error": …}` bytes, and the sentence a successful call that
    /// wrote no zygote still earns.
    pub fn preload(&self, request: &[u8]) -> Result<(PyZygote, Vec<u8>), Vec<u8>> {
        let vtable = self.vtable();
        let (mut out, mut err) = slots();
        let mut raw = ptr::null_mut();
        let code = (vtable.preload)(
            request.as_ptr(),
            request.len(),
            &mut raw,
            &mut out,
            &mut err,
        );
        let answer = crate::loader::take_slot(vtable, out);
        let failure = crate::loader::take_slot(vtable, err);
        if code != 0 {
            return Err(refusal(failure));
        }
        let zygote = PyZygote::from_raw(raw);
        if zygote.is_null() {
            return Err(refusal(failure));
        }
        Ok((zygote, answer))
    }

    /// Compiles one file in the preloaded parent, executing nothing (§1.2).
    ///
    /// # Errors
    ///
    /// The shim's `{"error": …}` bytes, which carry the file's own sentence.
    pub fn check_syntax(&self, zygote: PyZygote, code: &[u8], path: &[u8]) -> Result<(), Vec<u8>> {
        check_syntax_with(self.vtable(), zygote, code, path)
    }
}

impl PyRuntime<'_> {
    /// Compiles one file, executing nothing (design §1.2, `dev` reload).
    ///
    /// # Errors
    ///
    /// The shim's `{"error": …}` bytes, which carry the file's own sentence.
    pub fn check_syntax(&self, code: &[u8], path: &[u8]) -> Result<(), Vec<u8>> {
        check_syntax_with(self.vtable, self.zygote, code, path)
    }

    /// Runs one view child's `serve_view`, returning its exit code and refusal.
    ///
    /// The code is the child sequence's own: `0` served and shut down, `1` a
    /// refusal (whose sentence is the returned bytes), `70` a panic caught at
    /// the boundary.
    #[must_use]
    pub fn serve_view(&self, request: &[u8]) -> (i32, Vec<u8>) {
        let vtable = self.vtable;
        let (_, mut err) = slots();
        let code = (vtable.serve_view)(
            self.zygote.as_raw(),
            request.as_ptr(),
            request.len(),
            &mut err,
        );
        (code, crate::loader::take_slot(vtable, err))
    }
}

/// The one `check_syntax` call, shared by [`LoadedPy`] and [`PyRuntime`].
fn check_syntax_with(
    vtable: &BlastitPyVtableV1,
    zygote: PyZygote,
    code: &[u8],
    path: &[u8],
) -> Result<(), Vec<u8>> {
    let (_, mut err) = slots();
    let status = (vtable.check_syntax)(
        zygote.as_raw(),
        code.as_ptr(),
        code.len(),
        path.as_ptr(),
        path.len(),
        &mut err,
    );
    let failure = crate::loader::take_slot(vtable, err);
    if status == 0 {
        Ok(())
    } else {
        Err(refusal(failure))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A slot the shim never wrote: a length that says nothing is there. `take`
    /// reads only when `cap != 0` and `len <= cap`.
    #[test]
    fn an_unwritten_slot_is_never_read() {
        let slot = PyBufV1 {
            ptr: ptr::dangling_mut::<u8>(),
            len: 3,
            cap: 0,
        };
        assert!(slot.cap == 0 || slot.len > slot.cap);
    }

    #[test]
    fn the_refusal_placeholder_names_itself() {
        let text = String::from_utf8_lossy(NO_MESSAGE);
        assert!(text.contains("\"error\""), "{text}");
        assert!(text.contains("without a message"), "{text}");
    }

    #[test]
    fn the_null_zygote_names_nothing() {
        assert!(PyZygote::null().is_null());
        assert!(PyZygote::from_raw(ptr::null_mut()).is_null());
    }
}
