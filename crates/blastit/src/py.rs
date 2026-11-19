//! The composition root's handle on CPython, with no `pyo3` in sight (§1.2.1).
//!
//! Blastit's `blastit` crate never names an interpreter type. The shipped
//! binary links no `libpython`; stage 1 `dlopen`s the host's `libpython` (or,
//! for a wheel, only the per-minor shim) and every call from then on goes
//! through the frozen `blastit_py_abi_v1` vtable. [`blastit_py_sapi::PyRuntime`]
//! is that handle: a `Copy` value of three parts — the opaque zygote, the three
//! `PyOS_*Fork` hooks, and the vtable.
//!
//! # Three jobs, one module
//!
//! * the **token**: [`PyRuntime`]/[`PyForkHooks`] re-exported from
//!   `blastit-py-sapi`, or a phantom hooks type in a `--no-default-features`
//!   build (which serves no WSGI route and links no shim);
//! * the **fork hooks of an already-live interpreter** ([`runtime_hooks`]):
//!   a wheel's stage 0 runs inside CPython, so its forks must run CPython's
//!   protocol. Stage 0 may not `dlopen` (design §7 `R3`), so the hooks come
//!   from the objects already loaded, and a failure to resolve them is a
//!   refusal — never a bare `fork(2)` under a live interpreter;
//! * the **refusal text** of a shim boundary ([`refusal_message`]): the shim
//!   writes `{"error":"…"}`, and this is the one reader of that document.
//!
//! # `LiveInterpreter`
//!
//! [`crate::cli::main`] has to know whether the process already runs CPython,
//! without naming it: the wheel's `_blastit.main()` passes `Some(py)` and the
//! binary passes no interpreter. [`LiveInterpreter`] is that fact as a value —
//! implemented for `Option<T>` (the wheel) and for [`NoInterpreter`] (the
//! binary). One shape in both installations, and no interpreter type in a
//! signature.

/// The interpreter token a build with the `python` capability carries.
#[cfg(feature = "python")]
pub(crate) use blastit_py_sapi::{PyForkHooks, PyRuntime};

/// The token of a build that carries no shim at all.
///
/// There is nothing behind it: every WSGI route was refused before a listener
/// bound, so no fork ever runs a hook. It exists so `fork` keeps one shape.
#[cfg(not(feature = "python"))]
#[derive(Clone, Copy)]
pub(crate) struct PyForkHooks<'a>(std::marker::PhantomData<&'a ()>);

/// The three hooks a live interpreter in this image offers, or `None`.
///
/// `live` is the installation fact the caller already has: `true` when CPython
/// is running in this process (a wheel's stage 0/stage 1) and the caller holds
/// the GIL.
///
/// # Errors
///
/// One sentence when an interpreter is live but its `PyOS_*Fork` entry points
/// cannot be resolved. A bare `fork(2)` there would leave CPython's locks
/// describing a process that no longer exists, so the caller fails the boot.
#[cfg(feature = "python")]
pub(crate) fn runtime_hooks(live: bool) -> Result<Option<PyForkHooks<'static>>, String> {
    if !live {
        return Ok(None);
    }
    blastit_py_sapi::hosted_hooks().map(Some)
}

/// A build with no shim has nothing to stop or restart, and cannot host one.
#[cfg(not(feature = "python"))]
pub(crate) fn runtime_hooks(live: bool) -> Result<Option<PyForkHooks<'static>>, String> {
    match live {
        // Fail closed: this build cannot honour a live interpreter's forks.
        true => Err(
            "this blastit was built without the `python` capability, so it cannot \
             serve on a live interpreter; install the blastit that ships the CPython shim"
                .to_string(),
        ),
        false => Ok(None),
    }
}

/// The sentence behind a shim boundary's `{"error":"…"}` bytes.
///
/// A boundary that answered non-zero always wrote a JSON object; anything else
/// (an empty slot, a shim older than this reader) is reported as the bytes
/// themselves, lossily, so an operator still sees something actionable.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
pub(crate) fn refusal_message(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "the CPython shim refused without saying why".to_string();
    }
    if let Ok(serde_json::Value::Object(object)) =
        serde_json::from_slice::<serde_json::Value>(bytes)
        && let Some(message) = object.get("error").and_then(serde_json::Value::as_str)
    {
        return message.to_string();
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Whether this process already runs CPython, as a value (§1.2.1).
pub trait LiveInterpreter {
    /// `true` when the caller is a console script running on CPython.
    fn is_live(&self) -> bool;
}

/// The fact for a wheel's extension module: it was called *from* Python.
impl<T> LiveInterpreter for Option<T> {
    fn is_live(&self) -> bool {
        self.is_some()
    }
}

/// The fact for the standalone binary: nothing is running.
#[derive(Clone, Copy, Debug)]
pub struct NoInterpreter;

impl LiveInterpreter for NoInterpreter {
    fn is_live(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_interpreter_offers_no_hooks() {
        assert!(
            runtime_hooks(false)
                .expect("no interpreter is not a refusal")
                .is_none()
        );
        assert!(!NoInterpreter.is_live());
        assert!(!Option::<()>::None.is_live());
        assert!(Some(()).is_live());
    }

    #[test]
    fn a_refusal_document_names_the_shims_sentence() {
        assert_eq!(
            refusal_message(br#"{"error":"BLASTIT_APP (app:app): ModuleNotFoundError"}"#),
            "BLASTIT_APP (app:app): ModuleNotFoundError"
        );
        assert_eq!(
            refusal_message(b"not json"),
            "not json",
            "an unknown shape still reaches the operator"
        );
        assert!(!refusal_message(b"").is_empty());
    }
}
