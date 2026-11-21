//! The composition root's handle on CPython, with no `pyo3` in sight (§1.2.1).
//!
//! Blastit's `blastit` crate never names an interpreter type. The shipped
//! binary links no `libpython`; stage 1 `dlopen`s the host's `libpython` (or,
//! for a wheel, only the per-minor shim) and every call from then on goes
//! through the frozen `blastit_py_abi_v1` vtable. [`blastit_py_sapi::PyRuntime`]
//! is that handle: a `Copy` value of three parts — the opaque zygote, the three
//! `PyOS_*Fork` hooks, and the vtable.
//!
//! # Two jobs, one module
//!
//! * the **token**: [`PyRuntime`]/[`PyForkHooks`] re-exported from
//!   `blastit-py-sapi`, or a phantom hooks type in a `--no-default-features`
//!   build (which serves no WSGI route and links no shim);
//! * the **refusal text** of a shim boundary ([`refusal_message`]): the shim
//!   writes `{"error":"…"}`, and this is the one reader of that document.
//!
//! Nothing here asks whether CPython is already running: the universal binary is
//! the only installation, it starts no interpreter, and stage 1's `dlopen` is the
//! one place an interpreter enters this process tree.

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
