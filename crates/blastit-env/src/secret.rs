//! A value that is never formatted, only exposed on request (§5.1).

use std::fmt;

use zeroize::Zeroize;

/// A secret string: upstream credentials and CA material (§5.1, §5.2).
///
/// A `Secret` is wrapped so that the value cannot reach a log, an event or an
/// error by accident:
///
/// * [`fmt::Debug`] prints `Secret(<redacted>)`;
/// * there is **no** [`fmt::Display`], so `{}`, `format!` and every encoder
///   that uses `Display` fail to compile on it;
/// * the bytes are zeroized when the value is dropped;
/// * the only way to read the value is the explicitly named
///   [`Secret::expose`].
///
/// ```
/// use blastit_env::Secret;
///
/// let secret = Secret::new("s3cr3t");
/// assert_eq!(secret.expose(), "s3cr3t");
/// assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
/// ```
#[derive(Clone, Default)]
pub struct Secret(String);

impl Secret {
    /// Wraps a value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the value. The one place a secret is read.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// True when the secret is the empty string.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    /// Always `<redacted>`; the value never reaches a formatter.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl Drop for Secret {
    /// Overwrites the value before the allocation goes away.
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::Secret;

    #[test]
    fn debug_redacts_the_value() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert!(!format!("{secret:?}").contains("hunter2"));
    }

    #[test]
    fn expose_returns_the_value_and_empty_is_empty() {
        assert_eq!(Secret::new("hunter2").expose(), "hunter2");
        assert!(Secret::new("").is_empty());
        assert!(!Secret::new("x").is_empty());
    }

    #[test]
    fn cloned_values_are_independent() {
        let secret = Secret::new("hunter2");
        let copy = secret.clone();
        assert_eq!(copy.expose(), "hunter2");
        drop(secret);
        assert_eq!(copy.expose(), "hunter2");
    }
}
