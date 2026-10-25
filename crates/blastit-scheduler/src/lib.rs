//! The blastit scheduler: routing, admission, worker budgets and the view
//! process lifecycle (ARCHITECTURE.md §2.4).
//!
//! The scheduler sits between http's `http.chan` on one side, the view
//! processes' `view.chan`s on the other, and init's `ctl.chan` above them
//! all:
//!
//! ```text
//!   http ──① http.chan──► scheduler ──③ view.chan──► view process (per route)
//!                            │
//!                            └─② ctl.chan──► init: SPAWN / RETIRE, SPAWNED+fd
//! ```
//!
//! It is the only component that knows about every view process (§2.4). It
//! owns:
//!
//! * **Routing.** [`blastit_manifest::RoutingTable`] matches the request
//!   against the manifest's routes; the scheduler answers `404` itself when
//!   nothing matches, and its health path itself always.
//! * **Admission.** A global budget of [`SchedulerConfig::max_workers`] live
//!   request threads, a per-view cap, and one bounded FIFO queue. The budget
//!   counts threads, not answers: a slot is released when the view reports
//!   `DONE`, so a timed-out request still occupies one until its thread exits.
//! * **Lifecycle.** One live process per route at most, spawned on first use
//!   (or at startup for preforked routes and in eager mode), retired when idle
//!   or over its request cap, and recycled when it accumulates stuck threads.
//!
//! ```no_run
//! use std::sync::Arc;
//! use blastit_events::Event;
//! use blastit_manifest::{Manifest, RoutingTable, UidLayout};
//! use blastit_scheduler::{SchedulerConfig, UnixCtl, run, routes_from_manifest};
//! use tokio::net::UnixStream;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let manifest = Manifest::from_toml_str(
//!     "version = 1\n[[route]]\nname = \"about\"\npath = ['/about/']\n",
//! )?;
//! let table = RoutingTable::new(&manifest)?;
//! let routes = routes_from_manifest(&manifest, &UidLayout::default());
//! let emit = Arc::new(|event: Event| println!("{event:?}"));
//! // `http` and `ctl` are the scheduler's ends of the two socketpairs init
//! // made before the fork (§4).
//! let http: UnixStream = todo!();
//! let ctl = UnixCtl::new(todo!());
//! run(SchedulerConfig::default(), routes, table, http, ctl, emit).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Running it
//!
//! [`run`] does not set up a runtime: it is one future, and init is expected to
//! drive it on a tokio runtime alongside the other channels it serves.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod config;
mod ctl;
mod error;
mod path;
mod route;
mod scheduler;
mod task;

pub use crate::config::{SPAWN_BACKOFF, SchedulerConfig};
pub use crate::ctl::{CtlChannel, UnixCtl};
pub use crate::error::SchedulerError;
pub use crate::route::{RouteInfo, routes_from_manifest};
pub use crate::scheduler::{Emit, run};
