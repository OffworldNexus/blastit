//! Guard: the PostgreSQL upstream harness exists twice.
//!
//! A test file can only `mod` modules inside its own crate's `tests/` tree, so
//! `PgUpstream` lives in both `crates/blastit/tests/common/pg_upstream.rs` and
//! `crates/blastit-dbproxy/tests/common/pg_upstream.rs`. `MB.40.3.3` forbids two
//! divergent copies: the first edit to one of them would silently fork the two
//! protected suites onto different wire peers while both stayed green.
//!
//! This is the guard, and it is a test rather than a source-grep script for the
//! same reason the suites are real sockets — it compares the actual bytes the
//! compiler was handed, so it fails on the edit, not on a regex approximation.

/// The two copies must be byte-identical; copy the edited file verbatim across.
#[test]
fn both_crates_carry_the_same_upstream_harness() {
    let here = include_str!("common/pg_upstream.rs");
    let sibling = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../blastit-dbproxy/tests/common/pg_upstream.rs"
    ));
    assert!(
        here == sibling,
        "the PostgreSQL upstream harness has forked between crates: keep \
         `crates/blastit/tests/common/pg_upstream.rs` and \
         `crates/blastit-dbproxy/tests/common/pg_upstream.rs` byte-identical by \
         copying the edited file verbatim into the other crate in the same change"
    );
}
