//! Proxy inputs, the template engine and the app environment (ARCHITECTURE.md §5).
//!
//! blastit takes its configuration from environment variables, and init
//! rewrites that environment once, in stage 0, before the `execve` barrier
//! (§4). This crate is that rewrite, as a pure function:
//!
//! * [`postgresql`] turns one `[proxy.<name>]` declaration into an
//!   [`Upstream`] — host, port, role, password, database, TLS mode and CA —
//!   for dbproxy to connect with (§2.2, §5.2);
//! * [`Template`] renders the app-side variables that point at dbproxy's local
//!   socket: `{{ field }}` and `{{ field | urlencode }}`, nothing else (§5.1);
//! * [`build`] removes every secret from the platform environment, renders the
//!   proxies' outputs into it, injects process-safety defaults and hands back
//!   both halves in a [`Prepared`] (§5.1, §5.3).
//!
//! **Secrets cannot escape.** [`Prepared::upstreams`] carries the real
//! credentials wrapped in [`Secret`], which has no `Display`, zeroizes on drop
//! and prints `Secret(<redacted>)`; [`Prepared::app_env`] never holds one,
//! because every proxy input variable is removed and the fields a template may
//! read are only the local endpoint and two non-secret facts. Every error in
//! [`EnvError`] names the variable involved, never its value.
//!
//! **`HOME` and `TMPDIR` are not here.** §5.3 lists them among the injected
//! defaults, but each view gets a private scratch directory that only the
//! process launcher knows, so they are set per view later; [`build`] leaves
//! them out and documents it.
//!
//! **What this crate does not do.** It carries a proxy's `postgresql` query
//! parameters through untouched in [`postgresql::Upstream::options`]. Forcing
//! `standard_conforming_strings = on` on every upstream connection is
//! dbproxy's job — the SQL policy depends on it (§2.2) — and not this crate's.
//!
//! ```
//! use std::collections::BTreeMap;
//! use blastit_env::build;
//! use blastit_manifest::Manifest;
//!
//! let manifest = Manifest::from_toml_str(
//!     "version = 1\n\
//!      [proxy.db]\n\
//!      protocol = \"postgresql\"\n\
//!      input = { url = \"DATABASE_URL\", ca_cert = \"DATABASE_CA_CERT\" }\n",
//! )
//! .unwrap();
//!
//! let platform = BTreeMap::from([
//!     (
//!         "DATABASE_URL".to_string(),
//!         "postgresql://doadmin:s3cr3t@db.example.com:25060/defaultdb?sslmode=require"
//!             .to_string(),
//!     ),
//!     ("DATABASE_CA_CERT".to_string(), "-----BEGIN CERTIFICATE-----".to_string()),
//!     ("PYTHONUNBUFFERED".to_string(), "1".to_string()),
//! ]);
//!
//! let prepared = build(&manifest, &platform, None).unwrap();
//!
//! // The app gets the local endpoint, under the same variable name.
//! assert_eq!(
//!     prepared.app_env().get("DATABASE_URL").map(String::as_str),
//!     Some("postgresql://blastit@%2Fproc%2Fself%2Fcwd:5432/defaultdb"),
//! );
//! // The CA material is gone from the app's environment…
//! assert_eq!(prepared.app_env().get("DATABASE_CA_CERT"), None);
//! // …and the process-safety defaults are in.
//! assert_eq!(prepared.app_env().get("LC_ALL").map(String::as_str), Some("C.UTF-8"));
//!
//! // dbproxy still holds the real credentials.
//! let upstream = prepared.upstream("db").unwrap();
//! assert_eq!(upstream.password().expose(), "s3cr3t");
//! ```

#![deny(missing_docs)]

mod env;
mod error;
pub mod postgresql;
mod secret;
mod template;

pub use env::{
    INJECTED_DEFAULTS, Prepared, RESERVED_PREFIX, STRIP_VAR, build, build_from_process,
    platform_env_from_os,
};
pub use error::{EnvError, EnvErrors};
pub use postgresql::Upstream;
pub use secret::Secret;
pub use template::{Fields, Filter, Template, TemplateError, urlencode};
