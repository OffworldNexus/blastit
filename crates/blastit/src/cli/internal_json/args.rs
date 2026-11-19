//! The argument decoders of design §3's twelve rows.
//!
//! One function per JSON shape the contract names: a string, a list of strings,
//! a boolean (absent means `false`), a non-negative whole number, and the one
//! bytes-on-the-wire field `probe_ingress` needs (standard base64, written by
//! `base64.b64encode` in `_blastit.py`).
//!
//! Every refusal names the argument and never echoes a value: a request document
//! can carry a manifest, and a manifest can carry a credential's variable name.

use serde_json::Value;

/// One required string argument.
pub(super) fn string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("`{key}` must be a string"))
}

/// One list-of-strings argument.
pub(super) fn strings(args: &Value, key: &str) -> Result<Vec<String>, String> {
    let wrong = || format!("`{key}` must be a list of strings");
    args.get(key)
        .and_then(Value::as_array)
        .ok_or_else(wrong)?
        .iter()
        .map(|item| item.as_str().map(ToOwned::to_owned).ok_or_else(wrong))
        .collect()
}

/// One boolean argument, `false` when it is absent.
pub(super) fn boolean(args: &Value, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("`{key}` must be true or false")),
    }
}

/// One non-negative whole-number argument.
pub(super) fn integer(args: &Value, key: &str) -> Result<u64, String> {
    let missing = || format!("`{key}` must be a non-negative whole number");
    args.get(key).and_then(Value::as_u64).ok_or_else(missing)
}

/// Decodes standard base64 (`A–Z a–z 0–9 + /`, `=` padding) without a dependency.
///
/// The alphabet is the standard one because the encoder is Python's; anything
/// else — a URL-safe `-`/`_`, a misplaced `=`, a trailing group of one digit — is
/// refused rather than guessed at.
pub(super) fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let bad = || "`prefix_b64` is not base64".to_string();
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for chunk in text.as_bytes().chunks(4) {
        let mut values = [0_u8; 4];
        let mut kept = 4;
        for (index, byte) in chunk.iter().enumerate() {
            values[index] = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if index >= 2 => {
                    kept = index;
                    break;
                }
                _ => return Err(bad()),
            };
        }
        if chunk.len() != 4 && kept == 4 {
            return Err(bad());
        }
        let bits = (u32::from(values[0]) << 18)
            | (u32::from(values[1]) << 12)
            | (u32::from(values[2]) << 6)
            | u32::from(values[3]);
        let quad = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        out.extend_from_slice(&quad[..kept - 1]);
    }
    Ok(out)
}
