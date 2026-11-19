//! Shared test fixtures for `crates/blastit-dbproxy/tests/`.
//!
//! The suite drives the real proxy over real Unix sockets; the only simulated
//! peer is the PostgreSQL upstream the proxy dials, and it is a real loopback
//! server named after what it is.

pub mod pg_upstream;
