//! The uid table (§2) and the range check that goes with it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{Manifest, ManifestError, ManifestErrors, UidLayout};

use common::{section_6, validate_errors};

#[test]
fn the_default_layout() {
    let layout = UidLayout::default();
    assert_eq!(layout, UidLayout::new(UidLayout::DEFAULT_BASE));
    assert_eq!(layout.base, 20_000);
    assert_eq!(layout.http(), 20_001);
    assert_eq!(layout.scheduler(), 20_002);
    assert_eq!(layout.dbproxy(), 20_003);
    assert_eq!(layout.view(0), 21_000);
    assert_eq!(layout.view(1), 21_001);
    assert_eq!(layout.view(3), 21_003);
}

#[test]
fn a_custom_base_moves_everything() {
    let layout = UidLayout::new(30_000);
    assert_eq!(layout.http(), 30_001);
    assert_eq!(layout.scheduler(), 30_002);
    assert_eq!(layout.dbproxy(), 30_003);
    assert_eq!(layout.view(7), 31_007);
}

#[test]
fn a_huge_base_saturates_instead_of_panicking() {
    let layout = UidLayout::new(u32::MAX);
    assert_eq!(layout.http(), u32::MAX);
    assert_eq!(layout.scheduler(), u32::MAX);
    assert_eq!(layout.dbproxy(), u32::MAX);
    assert_eq!(layout.view(0), u32::MAX);
    assert_eq!(layout.view(usize::MAX), u32::MAX);
}

#[test]
fn views_are_numbered_in_manifest_order() {
    let manifest = section_6();
    let layout = UidLayout::default();
    for (index, route) in manifest.routes().iter().enumerate() {
        assert_eq!(
            layout.view(index),
            21_000 + index as u32,
            "{}",
            route.name()
        );
    }
    assert_eq!(layout.view(3), 21_003, "the last of the four routes");
}

#[test]
fn the_uid_range_is_checked_against_the_base() {
    // The shortest document there is: an empty `Blastfile` declares nothing.
    assert_eq!(
        validate_errors("", 65_533),
        ManifestErrors::from(ManifestError::UidBaseTooHigh {
            base: 65_533,
            uid: 65_536,
            max: 65_536,
        })
    );
    Manifest::from_blastfile_str("")
        .unwrap()
        .validate(65_532)
        .expect("dbproxy at 65535 is still in range");
}

#[test]
fn the_last_route_uid_is_the_boundary() {
    let route = "view r regex '.*' {\n}\n";
    // 64535 + 1000 = 65535: in range.
    Manifest::from_blastfile_str(route)
        .unwrap()
        .validate(64_535)
        .expect("in range");
    // 64536 + 1000 = 65536: out of range.
    assert_eq!(
        validate_errors(route, 64_536),
        ManifestErrors::from(ManifestError::TooManyRoutes {
            index: 0,
            uid: 65_536,
            base: 64_536,
            max: 65_536,
        })
    );
}

/// A route runs under the uid of its **sandbox**, not of its position in the
/// table: `lower` numbers one sandbox per `view`, and the §6 application has one
/// endpoint per view, so `sandbox(route.sandbox())` is the number today's table
/// gives. A view with two endpoints carries that view's sandbox on both of its
/// routes and therefore one uid (§2.3, §7.3).
///
/// The shared-sandbox and range-check halves are `src/uid.rs`'s own tests: only
/// the crate can build a `Route` with a chosen sandbox, and `tests/` never calls
/// a `pub(crate)` item (design §20).
#[test]
fn a_route_runs_under_the_uid_of_its_sandbox() {
    let manifest = section_6();
    let layout = UidLayout::default();
    for (index, route) in manifest.routes().iter().enumerate() {
        assert_eq!(route.sandbox(), index as u32, "{}", route.name());
        assert_eq!(
            layout.sandbox(route.sandbox()),
            layout.view(index),
            "{}",
            route.name()
        );
        assert_eq!(layout.sandbox(route.sandbox()), 21_000 + index as u32);
    }
}

/// The next sandbox takes the next uid, and a huge one saturates instead of
/// panicking.
#[test]
fn the_next_sandbox_takes_the_next_uid() {
    let layout = UidLayout::default();
    assert_eq!(layout.sandbox(0), 21_000);
    assert_eq!(layout.sandbox(1), 21_001);
    assert_eq!(layout.sandbox(3), 21_003);
    assert_eq!(layout.sandbox(u32::MAX), u32::MAX);
}
