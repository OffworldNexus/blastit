//! The upstream side of a `postgresql` proxy: libpq's `sslmode` semantics, the
//! CA bundle, and the startup parameters the proxy forces.
//!
//! pregresql takes a [`TlsClientOptions`] whose CA is a *file*, while init
//! hands dbproxy the CA as PEM text in the configuration; [`CaBundles`]
//! materialises the ones that are actually needed into a private directory
//! (see [`CaBundles::new`]).

use std::path::{Path, PathBuf};

use pregresql::proxy::{TlsClientOptions, UpstreamTls, VerifyMode};
use zeroize::Zeroizing;

use crate::error::DbproxyError;

/// The `options` startup parameter that turns `standard_conforming_strings` on
/// for the upstream session.
///
/// `blastit-sqlpolicy` and `ARCHITECTURE.md` §6 both assume it is on: with it
/// on, a `'…'` literal has no backslash escapes, which is how the allowlist
/// lexes it.
pub const FORCE_STANDARD_CONFORMING_STRINGS: &str = "-c standard_conforming_strings=on";

/// libpq's `sslmode`, reduced to what pregresql can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslPlan {
    /// `sslmode=disable`: never send an `SSLRequest`.
    Disable,
    /// `sslmode=prefer` or `allow` (and the default): try TLS, fall back to
    /// plaintext, and do not verify the server certificate — libpq does not
    /// verify in these modes either.
    Prefer,
    /// `sslmode=require`: TLS is mandatory; the server certificate is verified
    /// only when a CA was supplied.
    Require,
    /// `sslmode=verify-ca` or `verify-full`: TLS is mandatory and the server
    /// certificate is verified against the CA, which is therefore required.
    Verify,
}

impl SslPlan {
    /// Maps a libpq `sslmode` string (as it appears in a `postgresql://` URI
    /// or in the split inputs) onto a plan. `None` is libpq's default:
    /// `prefer`.
    ///
    /// An unknown mode is a configuration error, like libpq's own
    /// "invalid sslmode value".
    pub fn parse(sslmode: Option<&str>) -> Result<Self, DbproxyError> {
        match sslmode {
            None | Some("prefer") | Some("allow") => Ok(Self::Prefer),
            Some("disable") => Ok(Self::Disable),
            Some("require") => Ok(Self::Require),
            Some("verify-ca") | Some("verify-full") => Ok(Self::Verify),
            Some(other) => Err(DbproxyError::Config(format!(
                "unknown sslmode {other:?} (expected disable, allow, prefer, require, verify-ca or verify-full)"
            ))),
        }
    }

    /// Resolves the plan into pregresql's upstream TLS mode.
    ///
    /// `ca` is the materialised CA bundle, present only when one was
    /// configured *and* the plan verifies (or may verify) the server.
    ///
    /// `verify-ca` and `verify-full` share one mode: rustls' WebPKI verifier
    /// always checks the server name, so `verify-ca` here is stricter than
    /// libpq's (which accepts a certificate whose name does not match).
    pub fn resolve(self, ca: Option<PathBuf>) -> Result<UpstreamTls, DbproxyError> {
        fn options(ca: Option<PathBuf>, verify: VerifyMode) -> TlsClientOptions {
            TlsClientOptions {
                verify,
                ca_file: ca,
                client_cert: None,
            }
        }
        match self {
            Self::Disable => Ok(UpstreamTls::Disable),
            Self::Prefer => Ok(UpstreamTls::Prefer(options(None, VerifyMode::None))),
            Self::Require => Ok(UpstreamTls::Require(match ca {
                Some(ca) => options(Some(ca), VerifyMode::Full),
                None => options(None, VerifyMode::None),
            })),
            Self::Verify => match ca {
                Some(ca) => Ok(UpstreamTls::Require(options(Some(ca), VerifyMode::Full))),
                None => Err(DbproxyError::Config(
                    "sslmode=verify-ca or verify-full needs a ca_cert for the proxy".to_string(),
                )),
            },
        }
    }

    /// Whether this plan needs the CA bundle materialised on disk.
    pub fn needs_ca(self) -> bool {
        matches!(self, Self::Require | Self::Verify)
    }
}

/// The CA bundles a configuration needs, materialised as private files.
///
/// pregresql's [`TlsClientOptions::ca_file`] is a path, so a CA that only
/// exists as PEM text in the configuration has to be written somewhere before
/// the upstream TLS handshake can verify against it.
///
/// The bundles live in a **fresh** directory per process —
/// `<tmp>/blastit-dbproxy-<pid>-<nonce>`, created with `mkdir(0700)` so its
/// permissions are right from the first instant and an existing path is never
/// reused — and each file is created with `O_CREAT|O_EXCL|O_NOFOLLOW` and mode
/// `0600`, so a symlink or a pre-existing file at the path is an error rather
/// than something dbproxy would write through. A certificate is public
/// material, not a credential, and the files are not removed on shutdown: a
/// container restart replaces the directory, and there is no shutdown step
/// whose failure could matter.
#[derive(Debug, Default)]
pub struct CaBundles {
    directory: Option<PathBuf>,
    /// Distinguishes the bundles of proxies whose names sanitise alike.
    written: usize,
}

impl CaBundles {
    /// Creates the private directory lazily; nothing is written until
    /// [`CaBundles::write`] is called.
    pub fn new() -> Self {
        Self::default()
    }

    /// Materialises one proxy's CA bundle and returns its path.
    pub fn write(&mut self, proxy: &str, pem: &Zeroizing<String>) -> Result<PathBuf, DbproxyError> {
        let directory = match &self.directory {
            Some(directory) => directory.clone(),
            None => {
                let directory = create_private_directory()?;
                self.directory = Some(directory.clone());
                directory
            }
        };
        // Every bundle gets its own file: two proxy names can sanitise to the
        // same file name, and `O_EXCL` would then fail the boot.
        let path = directory.join(format!("{}-{}.pem", sanitise(proxy), self.written));
        write_private_file(&path, pem.as_bytes())?;
        self.written += 1;
        Ok(path)
    }
}

/// Creates a directory only this process can read, atomically.
///
/// `mkdir` with mode `0700` never widens an existing directory's permissions,
/// and a colliding path (a leftover from a recycled pid) is retried with
/// another nonce rather than reused.
fn create_private_directory() -> Result<PathBuf, DbproxyError> {
    use std::os::unix::fs::DirBuilderExt;

    let base = std::env::temp_dir();
    for _ in 0..8 {
        let path = base.join(format!(
            "blastit-dbproxy-{}-{}",
            std::process::id(),
            nonce()
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(DbproxyError::Config(
        "could not create a private directory for the CA bundles".to_string(),
    ))
}

/// A short, unpredictable name component.
fn nonce() -> String {
    let mut bytes = [0u8; 8];
    let random = std::fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
        .is_ok();
    if random {
        let mut hex = String::with_capacity(16);
        for byte in bytes {
            hex.push_str(&format!("{byte:02x}"));
        }
        return hex;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// Writes `contents` to a **new** `path` with mode `0600`.
fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), DbproxyError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        // `O_CREAT|O_EXCL`: never write through an existing file.
        .create_new(true)
        .mode(0o600)
        // ... and never follow a symlink that appeared at the path.
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)?;
    file.write_all(contents)?;
    file.flush()?;
    Ok(())
}

/// Keeps a proxy name usable as a file name: only ASCII alphanumerics, `-`,
/// `_` and `.` survive, everything else becomes `_`.
fn sanitise(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Builds the extra upstream startup parameters for one proxy.
///
/// The proxy's own `options` are passed through (they override whatever the
/// client asked for, which is pregresql's documented behaviour for hook
/// parameters), except that `user` and `database` are dropped: those come from
/// the proxy's real identity, and a client must never be able to steer them.
/// `standard_conforming_strings=on` is appended to the `options` value, so a
/// caller's own `options` are preserved.
pub fn upstream_parameters(options: &[(String, String)]) -> Vec<(String, String)> {
    let mut parameters: Vec<(String, String)> = options
        .iter()
        .filter(|(name, _)| name != "user" && name != "database")
        .cloned()
        .collect();
    match parameters.iter_mut().find(|(name, _)| name == "options") {
        Some((_, value)) => {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(FORCE_STANDARD_CONFORMING_STRINGS);
        }
        None => parameters.push((
            "options".to_string(),
            FORCE_STANDARD_CONFORMING_STRINGS.to_string(),
        )),
    }
    parameters
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn plan(mode: Option<&str>) -> SslPlan {
        SslPlan::parse(mode).expect("a known sslmode")
    }

    #[test]
    fn default_is_prefer() {
        assert_eq!(plan(None), SslPlan::Prefer);
        assert_eq!(plan(Some("prefer")), SslPlan::Prefer);
        assert_eq!(plan(Some("allow")), SslPlan::Prefer);
        assert_eq!(plan(Some("disable")), SslPlan::Disable);
        assert_eq!(plan(Some("require")), SslPlan::Require);
        assert_eq!(plan(Some("verify-ca")), SslPlan::Verify);
        assert_eq!(plan(Some("verify-full")), SslPlan::Verify);
    }

    #[test]
    fn unknown_sslmode_is_rejected_without_echoing_it() {
        let error = SslPlan::parse(Some("yes")).expect_err("unknown mode");
        assert!(error.to_string().contains("unknown sslmode"), "{error}");
    }

    #[test]
    fn disable_never_uses_tls() {
        assert_eq!(
            plan(Some("disable")).resolve(None).expect("ok"),
            UpstreamTls::Disable
        );
        assert!(!plan(Some("disable")).needs_ca());
    }

    #[test]
    fn prefer_tries_tls_without_verification() {
        let tls = plan(None).resolve(None).expect("ok");
        assert!(matches!(tls, UpstreamTls::Prefer(_)));
        assert!(!tls.is_required());
        assert_eq!(tls.options().expect("options").verify, VerifyMode::None);
    }

    #[test]
    fn prefer_ignores_a_ca_because_libpq_does() {
        let tls = plan(Some("prefer"))
            .resolve(Some(PathBuf::from("/tmp/ca.pem")))
            .expect("ok");
        assert_eq!(tls.options().expect("options").verify, VerifyMode::None);
    }

    #[test]
    fn require_verifies_only_with_a_ca() {
        let without = plan(Some("require")).resolve(None).expect("ok");
        assert!(without.is_required());
        assert_eq!(without.options().expect("options").verify, VerifyMode::None);
        assert_eq!(without.options().expect("options").ca_file, None);

        let with = plan(Some("require"))
            .resolve(Some(PathBuf::from("/tmp/ca.pem")))
            .expect("ok");
        assert!(with.is_required());
        assert_eq!(with.options().expect("options").verify, VerifyMode::Full);
        assert_eq!(
            with.options().expect("options").ca_file,
            Some(PathBuf::from("/tmp/ca.pem"))
        );
    }

    #[test]
    fn verify_needs_a_ca_and_verifies_when_it_has_one() {
        let error = plan(Some("verify-full"))
            .resolve(None)
            .expect_err("a CA is required");
        assert!(error.to_string().contains("needs a ca_cert"), "{error}");

        let tls = plan(Some("verify-ca"))
            .resolve(Some(PathBuf::from("/tmp/ca.pem")))
            .expect("ok");
        assert!(tls.is_required());
        assert_eq!(tls.options().expect("options").verify, VerifyMode::Full);
        assert!(plan(Some("verify-full")).needs_ca());
    }

    #[test]
    fn ca_bundles_are_written_privately_and_named_after_the_proxy() {
        use std::os::unix::fs::PermissionsExt;

        let mut bundles = CaBundles::new();
        let pem = Zeroizing::new(
            "-----BEGIN CERTIFICATE-----\nnot really a certificate\n-----END CERTIFICATE-----\n"
                .to_string(),
        );
        let path = bundles.write("../weird name", &pem).expect("written");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(".._weird_name-0.pem")
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("readable"),
            pem.as_str()
        );
        let file = std::fs::metadata(&path).expect("metadata");
        assert_eq!(file.permissions().mode() & 0o777, 0o600);
        let directory = path.parent().expect("a directory");
        let mode = std::fs::metadata(directory)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
        std::fs::remove_dir_all(directory).expect("cleaned up");
    }

    #[test]
    fn upstream_parameters_force_standard_conforming_strings() {
        let parameters = upstream_parameters(&[]);
        assert_eq!(
            parameters,
            [(
                "options".to_string(),
                FORCE_STANDARD_CONFORMING_STRINGS.to_string()
            )]
        );

        let parameters = upstream_parameters(&[
            ("options".to_string(), "-c timezone=UTC".to_string()),
            ("application_name".to_string(), "blastit".to_string()),
            ("user".to_string(), "not-the-real-user".to_string()),
            ("database".to_string(), "not-the-real-database".to_string()),
        ]);
        assert_eq!(
            parameters,
            [
                (
                    "options".to_string(),
                    format!("-c timezone=UTC {FORCE_STANDARD_CONFORMING_STRINGS}")
                ),
                ("application_name".to_string(), "blastit".to_string()),
            ]
        );
    }
}
