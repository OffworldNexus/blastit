//! Manifest parsing, validation and routing for blastit (ARCHITECTURE.md §5.1,
//! §6).
//!
//! The manifest is blastit's own routing table and capability list: which
//! requests go to which view process, and what each view may do. This crate
//! turns one TOML document into a typed [`Manifest`]:
//!
//! * [`Manifest::from_toml_str`] parses it strictly — an unknown key is an
//!   error, not a comment;
//! * [`Manifest::validate`] checks what needs the whole document (unknown
//!   groups, grants on unknown proxies, duplicate route names, path regexes,
//!   method tokens, the uid range, a `module:attribute` `app`, and a
//!   `queries` entry that inlines a value or a redacted `…`) and returns the
//!   non-fatal warnings;
//! * [`Manifest::app`] and [`Manifest::preload`] carry the two non-secret
//!   wiring keys, [`Manifest::name`] the application's name, [`Route::ingress`]
//!   the request-body codec a route declared, [`Manifest::mounts`] the static
//!   mounts a file serves, and [`Manifest::php`] the one front controller an
//!   embedded `libphp` worker runs;
//! * [`Route::static_policy`] and [`Route::php`] are the retired reader's
//!   per-route blocks: it fills them *and* [`Manifest::mounts`]/[`Manifest::php`]
//!   from the same document, so today's runtime reads one answer either way.
//!   The per-route pair is deleted with that reader;
//! * [`Manifest::is_all_static`], [`Manifest::static_roots`] and
//!   [`Manifest::check_static_roots`] answer the app-less static site: no `app`
//!   is needed once every route is static, and the roots are audited by an
//!   explicit call rather than by validation;
//! * [`Manifest::effective_grants`] unions the grants a route ends up with;
//! * [`Manifest::equivalent`] compares two manifests without their source
//!   positions, which is what a formatter or an editor's postcondition needs;
//! * [`edit::apply_edits`] and [`edit::write_atomic`] are how `--learn` writes
//!   the manifest back without reformatting it;
//! * [`UidLayout`] maps a route's sandbox to the uid its process runs under;
//! * [`RoutingTable`] matches `(method, path_info)` to a route, and
//!   [`MountTable`] matches a path to a static mount.
//!
//! Templates, input slots and the meaning of a statement belong to other
//! crates: this one keeps `input`, `output` and `queries` as raw strings
//! (§5.1, §5.2). A `postgresql` grant's `tables` and `functions` are kept as the
//! typed [`TableGrants`] `blastit-sqlpolicy` checks statements against (§6, D7).
//!
//! Errors render on one line: control characters are escaped and long values
//! are cut short, so a manifest problem never breaks the event stream (§8.1).
//!
//! ```
//! use blastit_manifest::{Manifest, Match, RoutingTable};
//!
//! let manifest = Manifest::from_toml_str(
//!     r#"
//!     version = 1
//!
//!     [proxy.db]
//!     protocol = "postgresql"
//!     input = { url = "DATABASE_URL" }
//!
//!     [[route]]
//!     name = "about"
//!     path = ['/about/']
//!     [route.db]
//!     queries = ['SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $1']
//!     "#,
//! )
//! # .unwrap()
//! ;
//! assert_eq!(manifest.validate(20_000).expect("valid"), vec![]);
//!
//! let table = RoutingTable::new(&manifest).expect("valid");
//! assert_eq!(table.route("GET", "/about/"), Some(Match { route: 0, rule: 0 }));
//! assert_eq!(table.route("GET", "/elsewhere"), None);
//!
//! // The model carries the Blastfile fields beside the per-route blocks the
//! // retired reader still fills: this document declares neither a mount nor a
//! // front controller, and it has no application name.
//! assert!(manifest.mounts().is_empty());
//! assert!(manifest.php().is_none());
//! assert_eq!(manifest.name(), None);
//! assert_eq!(manifest.route(0).map(|route| route.sandbox()), Some(0));
//! assert!(manifest.equivalent(&manifest.clone()));
//! ```
//!
//! The executor kind table a backend registry checks at boot is the crate
//! root's too:
//!
//! ```
//! use blastit_manifest::Executor;
//!
//! assert_eq!(Executor::View.kind(), "wsgi");
//! assert_eq!(Executor::ALL_KINDS, ["wsgi", "php"].as_slice());
//! ```
//!
//! `[route.fastcgi]` is retired: a manifest that writes it is refused like any
//! other unknown route key (`ADR-0010` §1: embedded PHP only, no fallback), and
//! `php` is the embedded worker ([`PhpPolicy`]), not the retired pool.
//!
//! [`blastfile::read`] is the one reader of a `Blastfile`: a regular file of at
//! most 1 MiB, refused by size from its metadata before a byte is read, and
//! never a FIFO or a device.

#![deny(missing_docs)]

mod error;
mod model;
mod parse;
mod routing;
mod uid;
mod validate;

pub mod blastfile;
pub mod edit;

pub use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};
pub use error::{Location, ManifestError, ManifestErrors, ManifestWarning, Position};
pub use model::{
    Executor, Grant, Group, Manifest, PgGrant, PhpPolicy, Protocol, Proxy, ProxyName, Route,
    StaticMount, StaticPolicy,
};
pub use routing::{
    Match, MountMatch, MountTable, REGEX_NEST_LIMIT, REGEX_SIZE_LIMIT, RoutingTable,
};
pub use uid::UidLayout;

// A route's `[route.ingress]` is part of the manifest, so the policy type it
// parses into is re-exported here: a caller that reads `Route::ingress` should
// not have to name a second crate to match on it.
pub use blastit_ingress::{BodyCodec, IngressPolicy};
