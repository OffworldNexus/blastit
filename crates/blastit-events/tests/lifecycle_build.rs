#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `Lifecycle::build` is the `preload` `boot` event's alone (`R23`).

use blastit_events::{
    Component, Event, EventKind, Lifecycle, decode_json, encode_json, encode_pretty,
};

const BUILD: &str = env!("CARGO_PKG_VERSION");

fn boot(lifecycle: Lifecycle) -> Event {
    Event::new(Component::Init, EventKind::Boot(lifecycle))
}

#[test]
fn build_round_trips_through_json_on_the_preload_boot() {
    let event = boot(Lifecycle::default().with_stage("preload").with_build(BUILD));
    let line = encode_json(&event);
    assert!(line.contains(&format!(r#""build":"{BUILD}""#)), "{line}");

    let decoded = decode_json(&line).expect("the line this crate wrote decodes");
    let EventKind::Boot(lifecycle) = decoded.kind() else {
        panic!("a boot line decodes as a boot: {line}");
    };
    assert_eq!(lifecycle.build(), Some(BUILD));
}

#[test]
fn build_is_absent_from_every_other_lifecycle_event() {
    for (name, event) in [
        (
            "stage0 boot",
            boot(Lifecycle::default().with_stage("stage0")),
        ),
        (
            "config boot",
            boot(Lifecycle::default().with_stage("config")),
        ),
        (
            "ready",
            Event::new(
                Component::Init,
                EventKind::Ready(Lifecycle::default().with_stage("serving")),
            ),
        ),
        (
            "shutdown",
            Event::new(
                Component::Init,
                EventKind::Shutdown(Lifecycle::default().with_stage("drain")),
            ),
        ),
    ] {
        let json = encode_json(&event);
        assert!(!json.contains(r#""build""#), "{name}: {json}");
        let pretty = encode_pretty(&event, false);
        assert!(!pretty.contains("build="), "{name}: {pretty}");
    }
}

#[test]
fn pretty_names_the_build_on_the_preload_boot() {
    let line = encode_pretty(
        &boot(Lifecycle::default().with_stage("preload").with_build(BUILD)),
        false,
    );
    assert!(line.contains("build="), "{line}");
    assert!(line.contains(BUILD), "{line}");
}

#[test]
fn a_lifecycle_never_carries_a_port() {
    for event in [
        boot(Lifecycle::default().with_stage("preload").with_build(BUILD)),
        Event::new(
            Component::Init,
            EventKind::Ready(Lifecycle::default().with_stage("serving")),
        ),
    ] {
        assert!(!encode_json(&event).contains(r#""port""#));
        assert!(!encode_pretty(&event, false).contains("port="));
    }
}

#[test]
fn the_unprivileged_dev_privilege_is_a_plain_string() {
    let line = encode_json(&boot(
        Lifecycle::default()
            .with_stage("stage0")
            .with_privilege("unprivileged-dev"),
    ));
    assert!(line.contains(r#""privilege":"unprivileged-dev""#), "{line}");
}
