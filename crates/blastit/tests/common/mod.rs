//! Shared test fixtures for `crates/blastit/tests/`.
//!
//! Only modules the suites actually need live here: how tests are built and run
//! is the policy's business, and every helper here is a real loopback server or
//! a fixture directory.

pub mod mock_pg;
