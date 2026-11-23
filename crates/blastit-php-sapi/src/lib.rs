#![deny(unsafe_code)]
#![deny(missing_docs)]

//! Per-minor C SAPI shims for embedded PHP: the frozen `blastit_php_abi_v1`
//! surface, the toolchain that builds a shim, and the request-side types that
//! make the `php2shell` class structurally absent.
//!
//! # What this crate is
//!
//! Blastit runs `[route.php]` by embedding the host's Non-Thread-Safe
//! `libphp.so` inside a forked, unprivileged `view` worker (ADR-0010 §1). Rust
//! cannot declare `sapi_globals_struct` or `EG(bailout)` by hand and survive a
//! PHP minor bump (`docs/research/php-embed-sapi.md` §2), so each minor gets one
//! ~25 KiB Apache-2.0 C shim, `libblastit_php<minor>-<arch>-<libc>.so`,
//! compiled against that minor's headers. This crate holds:
//!
//! * [`toolchain`] — finds `php-config` (`PHP_CONFIG`, then `php-config` itself
//!   and one versioned name per minor declared in `packaging/languages.tsv`),
//!   refuses an unsupported minor with the package to install, and maps a
//!   version to a shim filename.
//! * [`request`] — [`ServerVars`] (a `$_SERVER` entry list where `PHP_VALUE`,
//!   `PHP_ADMIN_VALUE`, `REDIRECT_STATUS` and `HTTP_PROXY` cannot be
//!   constructed, and a `Proxy:` header can never become `HTTP_PROXY`) and
//!   [`IniPolicy`] (the `ZEND_INI_SYSTEM` lockdown, rendered once at startup).
//! * [`loader`] — the one `dlopen`/`dlsym` module: it opens the host's NTS
//!   `libphp.so` and the matching shim, verifies the ABI and the minor, and
//!   exposes `worker_init`/`request`/`worker_shutdown` as safe wrappers over a
//!   [`Sink`]. It is compiled on every host; nothing in it runs until a worker
//!   calls it.
//! * `abi` — the `#[repr(C)]` mirror of `c/include/blastit_php_abi_v1.h`.
//!   Pure declarations and layout pins, compiled on every host so the loader
//!   has a type to speak.
//!
//! The C shim lives in `c/blastit_php_sapi.c` and is built by `make php-sapi`,
//! never by this crate's `build.rs`: it is a separate `.so` with no `DT_NEEDED`
//! on `libphp`, `dlopen`ed by the worker after `libphp` (`RTLD_NOW |
//! RTLD_GLOBAL` then `RTLD_LOCAL`).
//!
//! # Honest degradation, and what a green `make check` does *not* prove
//!
//! A machine without PHP must still build this crate. `build.rs` never fails
//! the build: it sets `cfg(php_toolchain)` and `BLASTIT_PHP_CONFIG` /
//! `BLASTIT_PHP_VERSION` when a supported `php-config` exists, and otherwise
//! prints one warning naming the missing tool and the package, leaving the ABI
//! surface un-compiled ([`toolchain_present`] is then `false`).
//!
//! **A green `make check` on a PHP-free host proves that this Rust side degrades
//! honestly. It never proves that the C works.** The shim is built and exercised
//! only inside `e2e/php-sapi/` (`debian:trixie-slim` + `php8.4-dev` +
//! `libphp8.4-embed`), by `make php-sapi-e2e`, which `dlopen`s the real
//! `libphp.so`, runs real requests through the shim, and asserts the response
//! body, head, status, `cpu_ns`, the reserved-name drop, and that an `exit()` and
//! a fatal leave the worker able to serve the next request.
//!
//! ```
//! use blastit_php_sapi::{Minor, ReservedVar, ServerVar, ServerVars, SUPPORTED_MINORS};
//!
//! let mut vars = ServerVars::new();
//! vars.insert("REQUEST_METHOD", "GET").expect("allowed");
//! // httpoxy: a `Proxy:` header is dropped, never `HTTP_PROXY`.
//! vars.push_header("Proxy", "http://attacker.example");
//! assert!(vars.iter().all(|v| v.name() != "HTTP_PROXY"));
//! // And the CGI/FastCGI escalation names cannot be built at all.
//! assert_eq!(ServerVar::new("PHP_VALUE", "allow_url_include=1"), Err(ReservedVar::PhpValue.into()));
//! // The newest declared minor parses, and its shim carries this build's
//! // `(arch, libc)`, the tuple the boot selects by.
//! let newest = SUPPORTED_MINORS[0];
//! assert_eq!(Minor::parse(&format!("{}.1", newest.dotted())), Some(newest));
//! let libc = if cfg!(target_env = "musl") { "musl" } else { "gnu" };
//! assert_eq!(
//!     newest.shim_file(),
//!     format!("libblastit_{}-{}-{libc}.so", newest.slug(), std::env::consts::ARCH),
//! );
//! ```

#[allow(unsafe_code)]
pub mod abi;
pub mod loader;
pub mod request;
pub mod toolchain;

pub use loader::{FileSink, Installed, LoadError, Loaded, NoSink, SharedObject, Sink};
pub use request::{
    DISABLED_FUNCTIONS, DropReason, Egress, HeaderOutcome, IniError, IniPolicy, PINNED_INI,
    ReservedVar, ServerVar, ServerVarError, ServerVars,
};
pub use toolchain::{
    CONFIG_NAMES, Env, Minor, NEWEST_MINOR, ProcessEnv, SUPPORTED_MINORS, Toolchain,
    ToolchainError, supported_minors_sentence,
};

/// The C ABI this crate speaks: `blastit_php_abi_v1` is version 1.
///
/// A shim reporting another value is refused by the worker before any request
/// (ADR-0010 §1: the vtable is frozen per minor, versioned across changes).
pub const ABI_VERSION: u32 = 1;

/// Whether the build found a supported `php-config` (`cfg(php_toolchain)`).
///
/// `false` is a supported state: the crate compiles, the ABI surface does not,
/// and [`Toolchain::discover`] explains what to install.
pub fn toolchain_present() -> bool {
    cfg!(php_toolchain)
}

/// The `php-config --version` the crate was built against, when there was one.
///
/// Read from `BLASTIT_PHP_VERSION`, set by `build.rs`; `None` on a PHP-free
/// host. This is build metadata, not a runtime probe: the worker still asks the
/// loaded shim for `blastit_php_minor()`.
pub fn build_toolchain_version() -> Option<&'static str> {
    option_env!("BLASTIT_PHP_VERSION")
}

/// The `php-config` path the crate was built against, when there was one.
pub fn build_php_config() -> Option<&'static str> {
    option_env!("BLASTIT_PHP_CONFIG")
}
