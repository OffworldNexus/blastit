//! `ADR-0010` §3: the two PHP settings and the resolved backend the stage-1
//! document carries (`MB.3.13.2`).
//!
//! The denied cases are the ones a boot must not guess at: a relative
//! `BLASTIT_LIBPHP` (the file inspected and the file loaded could differ), a
//! version a file name does not state, and a shim directory outside the
//! absolute-path rule. The accepted twins are the distro spellings and a full
//! round trip of a resolved `libphp` + shim.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use blastit_view_php::discovery::{PhpRuntime, PhpVersion};

use super::*;

/// A relative path is refused by the configuration, before any discovery
/// could open it; absolute ones are what the boot documents.
#[test]
fn the_libphp_paths_must_be_absolute() {
    let mut config = Config {
        libphp: Some(PathBuf::from("libphp.so")),
        ..Config::default()
    };
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("BLASTIT_LIBPHP"), "{err}");
    config.libphp = Some(PathBuf::from("/usr/lib/libphp8.4.so"));
    config
        .validate()
        .expect("an absolute path is what is documented");

    let mut config = Config {
        php_shim_dir: Some(PathBuf::from("target/php-sapi")),
        ..Config::default()
    };
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("BLASTIT_PHP_SHIM_DIR"), "{err}");
    config.php_shim_dir = Some(PathBuf::from("/opt/blastit/shims"));
    config.validate().expect("an absolute directory");
}

/// The document carries both settings and the resolved `(path, dev, ino,
/// version, shim)`, and a document written before them still reads.
#[test]
fn the_resolved_backend_round_trips_and_an_older_document_still_reads() {
    let mut config = Config {
        libphp: Some(PathBuf::from("/usr/lib/libphp8.4.so")),
        php_shim_dir: Some(PathBuf::from("/opt/blastit/shims")),
        ..Config::default()
    };
    assert_eq!(config.to_json()["php"], serde_json::Value::Null);
    config.php = Some(PhpRuntime {
        libphp: PathBuf::from("/usr/lib/libphp8.4.so"),
        dev: 66306,
        ino: 123_456,
        version: PhpVersion::new(8, 4),
        shim: PathBuf::from("/opt/blastit/shims/libblastit_php84-x86_64-gnu.so"),
        shim_dev: 66306,
        shim_ino: 654_321,
    });
    for config in [config.clone(), Config::default()] {
        let json = config.to_json();
        assert_eq!(Config::from_json(&json).unwrap(), config);
    }
    let json = config.to_json();
    assert_eq!(json["libphp"], serde_json::json!("/usr/lib/libphp8.4.so"));
    assert_eq!(json["php"]["version"], serde_json::json!("8.4"));
    assert_eq!(json["php"]["ino"], serde_json::json!(123_456));
    assert_eq!(json["php"]["shim_ino"], serde_json::json!(654_321));

    // A document that predates the fields reads as "nothing was resolved".
    let mut older = json.clone();
    let object = older.as_object_mut().unwrap();
    for key in ["libphp", "php_shim_dir", "php"] {
        object.remove(key);
    }
    let back = Config::from_json(&older).unwrap();
    assert_eq!(back.libphp, None);
    assert_eq!(back.php_shim_dir, None);
    assert_eq!(back.php, None);

    // A version the document cannot carry names the field.
    let mut broken = json;
    broken["php"]["version"] = serde_json::json!("8");
    let err = Config::from_json(&broken).unwrap_err();
    assert!(err.to_string().contains("php"), "{err}");
}
