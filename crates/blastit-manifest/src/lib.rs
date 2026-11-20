//! Manifest parsing, validation and routing for blastit (ARCHITECTURE.md §5.1,
//! §6).
//!
//! The manifest is blastit's own routing table and capability list: which
//! requests go to which view process, and what each view may do. This crate
//! turns one `Blastfile` document into a typed [`Manifest`]:
//!
//! * [`Manifest::from_blastfile_str`] is the one reader: it parses and lowers
//!   the file, and an unknown keyword is an error, not a comment;
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
//! * [`Route::static_policy`] and [`Route::php`] are the retired per-route
//!   blocks: no `Blastfile` can set either (a static site is a top-level
//!   `static` mount and PHP is one `entrypoint php`), and they stay in the
//!   model until the two pages that still describe them are rewritten;
//! * [`Manifest::is_all_static`], [`Manifest::static_roots`] and
//!   [`Manifest::check_static_roots`] answer the app-less static site: no `app`
//!   is needed when a `static` mount answers every request and no view or
//!   `entrypoint php` runs, and the roots are audited by an explicit call rather
//!   than by validation;
//! * [`Manifest::effective_grants`] unions the grants a route ends up with;
//! * [`Manifest::equivalent`] compares two manifests without their source
//!   positions, which is what a formatter or an editor's postcondition needs;
//! * [`blastfile::apply_edits`] and [`edit::write_atomic`] are how `--learn`
//!   writes the manifest back without reformatting it;
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
//! let manifest = Manifest::from_blastfile_str(
//!     r#"
//!     postgres $DATABASE_URL
//!
//!     view about {
//!         GET "/about/"
//!         postgres {
//!             query 'SELECT "cms_page"."body" FROM "cms_page" WHERE "cms_page"."slug" = $1'
//!         }
//!     }
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
//! // This document declares no mount and no front controller, and it has no
//! // application name.
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
//! `fastcgi` is not a keyword: a `Blastfile` that writes it is refused like any
//! other unknown keyword (`ADR-0010` §1: embedded PHP only, no fallback), and
//! `php` is the embedded worker ([`PhpPolicy`]), not the retired pool.
//!
//! [`blastfile::read`] is the one reader of a `Blastfile`: a regular file of at
//! most 16 MiB, refused by size from its metadata before a byte is read, and
//! never a FIFO or a device.

#![deny(missing_docs)]

mod error;
mod model;
mod routing;
mod uid;
mod validate;

pub mod blastfile;
pub mod edit;

pub use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};
// The Quick Fix's rewrite, re-exported beside `TableGrant`/`Columns` on the
// same argument: a caller that asks the manifest for a `query`'s canonical `$?`
// text should not have to name a second crate (`ADR-0013`).
pub use blastit_sqlpolicy::parameterize;
pub use error::{Location, ManifestError, ManifestErrors, ManifestWarning, Position};
// §18's one line is the crate's: a caller that prints a path or a message of
// its own renders it with these rather than keeping a second copy.
pub use error::{clipped, escape_control};
pub use model::{
    BodyOwner, Executor, Grant, Group, Manifest, PgGrant, PhpPolicy, Protocol, Proxy, ProxyName,
    QuerySite, Route, StaticMount, StaticPolicy,
};
pub use routing::{
    Match, MountMatch, MountTable, REGEX_NEST_LIMIT, REGEX_SIZE_LIMIT, RoutingTable,
};
pub use uid::UidLayout;

// A route's `[route.ingress]` is part of the manifest, so the policy type it
// parses into is re-exported here: a caller that reads `Route::ingress` should
// not have to name a second crate to match on it.
pub use blastit_ingress::{BodyCodec, IngressPolicy};
