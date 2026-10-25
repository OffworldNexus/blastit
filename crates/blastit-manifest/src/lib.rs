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
//!   method tokens, the uid range) and returns the non-fatal warnings;
//! * [`Manifest::effective_grants`] unions the grants a route ends up with;
//! * [`UidLayout`] maps a route index to the uid its process runs under;
//! * [`RoutingTable`] matches `(method, path_info)` to a route.
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
//! ```

#![deny(missing_docs)]

mod error;
mod model;
mod parse;
mod routing;
mod uid;
mod validate;

pub use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};
pub use error::{Location, ManifestError, ManifestErrors, ManifestWarning};
pub use model::{Grant, Group, Manifest, PgGrant, Protocol, Proxy, ProxyName, Route};
pub use routing::{Match, REGEX_NEST_LIMIT, REGEX_SIZE_LIMIT, RoutingTable};
pub use uid::UidLayout;
