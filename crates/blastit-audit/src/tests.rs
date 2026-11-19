//! The `audit` presenter's suites, moved with the code.
//!
//! Each file holds one area: the row-lock and grant rules, the request/statement
//! diff, the observations schema's refusals, `connection_setup`, egress and the
//! global groups. They are `audit`'s own tests — the `blastit check` document
//! has its own (`check_tests`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
mod egress;
mod groups;
mod locking;
mod requests;
mod schema;
mod setup;
