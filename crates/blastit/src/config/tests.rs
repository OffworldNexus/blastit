//! The configuration's own unit tests: the value parsers, the document round
//! trip, the preset precedence and the environment read.
//!
//! Moved out of `config.rs` unchanged when the module was split; only the `use`
//! lines differ, because the names they import now live in sibling modules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::field_reassign_with_default
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use blastit_events::{Level, Lifecycle};
use blastit_frame::ClientIpConfig;
use blastit_scheduler::FairConfig;

use super::env::proxy_key;
use super::parse::{parse_bool, parse_duration, parse_immortalize, parse_percent, parse_size};
use super::*;

/// Serialises the tests that read or write the process environment.
///
/// [`Config::from_env`] reads every `BLASTIT_*` variable at once, so a test
/// that calls it must not run while another test is changing one.
static ENV: Mutex<()> = Mutex::new(());

/// A `BLASTIT_MANIFEST` that names no file in this tree.
///
/// [`Config::from_env`] falls back to `./Blastfile` when the variable is
/// unset (design §15.1), which would make every unit test below depend on the
/// directory `cargo test` happened to start in: a stray manifest there would
/// silently change `app` and `preload`. Pointing the variable at a path that
/// is never written makes discovery deterministic (`ADR-0002`).
const ABSENT_MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/absent-blastfile"
);

/// The lock described on [`ENV`], plus a deterministic `BLASTIT_MANIFEST`.
///
/// Dropping it restores whatever the test runner had, so a developer's own
/// environment is left exactly as it was.
struct EnvGuard {
    // Dropped after `Drop::drop` runs, so the restore below is still
    // serialised by the suite's mutex.
    _lock: MutexGuard<'static, ()>,
    previous_manifest: Option<OsString>,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: `_lock` is still held while this runs (fields drop after
        // the guard's own `Drop`), and every change to these variables in
        // this module goes through the same mutex.
        unsafe {
            match self.previous_manifest.take() {
                Some(value) => std::env::set_var("BLASTIT_MANIFEST", value),
                None => std::env::remove_var("BLASTIT_MANIFEST"),
            }
        }
    }
}

/// The lock described on [`ENV`], with `BLASTIT_MANIFEST` neutralised.
fn env() -> EnvGuard {
    let lock = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous_manifest = std::env::var_os("BLASTIT_MANIFEST");
    // SAFETY: `lock` is held for as long as the returned guard lives.
    unsafe { std::env::set_var("BLASTIT_MANIFEST", ABSENT_MANIFEST) };
    EnvGuard {
        _lock: lock,
        previous_manifest,
    }
}

/// An address, for the client-IP tests.
fn ip(text: &str) -> std::net::IpAddr {
    text.parse().unwrap()
}

#[test]
fn durations_accept_the_documented_spellings() {
    assert_eq!(
        parse_duration("BLASTIT_QUEUE_TIMEOUT", "10s").unwrap(),
        Duration::from_secs(10)
    );
    assert_eq!(
        parse_duration("BLASTIT_QUEUE_TIMEOUT", "500ms").unwrap(),
        Duration::from_millis(500)
    );
    assert_eq!(
        parse_duration("BLASTIT_QUEUE_TIMEOUT", "1.5m").unwrap(),
        Duration::from_secs(90)
    );
    assert_eq!(
        parse_duration("BLASTIT_QUEUE_TIMEOUT", "0").unwrap(),
        Duration::ZERO
    );
    assert!(parse_duration("BLASTIT_QUEUE_TIMEOUT", "soon").is_err());
}

#[test]
fn sizes_accept_the_documented_spellings() {
    assert_eq!(parse_size("V", "64 KiB").unwrap(), 64 * 1024);
    assert_eq!(parse_size("V", "100MiB").unwrap(), 100 * 1024 * 1024);
    assert_eq!(parse_size("V", "32 MiB").unwrap(), 32 * 1024 * 1024);
    assert_eq!(parse_size("V", "1GB").unwrap(), 1_000_000_000);
    assert_eq!(parse_size("V", "512").unwrap(), 512);
    assert_eq!(parse_size("V", "8 b").unwrap(), 8);
    assert!(parse_size("V", "much").is_err());
}

#[test]
fn percentages_accept_the_documented_spelling() {
    assert_eq!(parse_percent("V", "85%").unwrap(), 85.0);
    assert_eq!(parse_percent("V", "85").unwrap(), 85.0);
    assert!(parse_percent("V", "lots").is_err());
}

#[test]
fn booleans_accept_zero_and_one() {
    assert!(parse_bool("V", "1").unwrap());
    assert!(!parse_bool("V", "0").unwrap());
    assert!(parse_bool("V", "yes").unwrap());
    assert!(!parse_bool("V", "no").unwrap());
    assert!(parse_bool("V", "maybe").is_err());
}

#[test]
fn the_immortalisation_setting_takes_auto_and_the_booleans() {
    assert_eq!(parse_immortalize("V", "auto").unwrap(), Immortalize::Auto);
    assert_eq!(parse_immortalize("V", " AUTO ").unwrap(), Immortalize::Auto);
    assert_eq!(parse_immortalize("V", "1").unwrap(), Immortalize::On);
    assert_eq!(parse_immortalize("V", "on").unwrap(), Immortalize::On);
    assert_eq!(parse_immortalize("V", "true").unwrap(), Immortalize::On);
    assert_eq!(parse_immortalize("V", "0").unwrap(), Immortalize::Off);
    assert_eq!(parse_immortalize("V", "off").unwrap(), Immortalize::Off);
    assert_eq!(parse_immortalize("V", "false").unwrap(), Immortalize::Off);
    let err = parse_immortalize("BLASTIT_IMMORTALIZE", "sometimes").unwrap_err();
    assert!(err.to_string().contains("BLASTIT_IMMORTALIZE"), "{err}");
    assert!(err.to_string().contains("auto, 0 or 1"), "{err}");
}

#[test]
fn the_immortalisation_setting_is_read_from_the_environment() {
    let _env = env();
    // SAFETY: `env()` is held for the whole test, so no other test reads or
    // writes the environment while this one runs.
    unsafe {
        std::env::remove_var("BLASTIT_IMMORTALIZE");
        assert_eq!(
            Config::from_env(None).unwrap().immortalize,
            Immortalize::Auto,
            "the default is auto"
        );
        for (value, expected) in [
            ("1", Immortalize::On),
            ("on", Immortalize::On),
            ("0", Immortalize::Off),
            ("off", Immortalize::Off),
            ("auto", Immortalize::Auto),
        ] {
            std::env::set_var("BLASTIT_IMMORTALIZE", value);
            assert_eq!(
                Config::from_env(None).unwrap().immortalize,
                expected,
                "BLASTIT_IMMORTALIZE={value}"
            );
        }
        std::env::set_var("BLASTIT_IMMORTALIZE", "sometimes");
        let err = Config::from_env(None).unwrap_err();
        assert!(err.to_string().contains("BLASTIT_IMMORTALIZE"), "{err}");
        std::env::remove_var("BLASTIT_IMMORTALIZE");
    }
}

#[test]
fn the_boot_seccomp_flag_uses_the_configuration_spellings() {
    let _env = env();
    // SAFETY: `env()` is held for the whole test, so no other test reads or
    // writes the environment while this one runs; the variable is put back
    // before it returns.
    unsafe {
        // The `boot` event is printed before the configuration is parsed,
        // so the flag is read on its own — but through `parse_bool`, so it
        // cannot accept anything `Config::from_env` would reject.
        for value in ["0", "off", "false", "no", "False", " 0 "] {
            std::env::set_var("BLASTIT_SECCOMP", value);
            assert!(!seccomp_requested(), "{value} means off");
        }
        for value in ["1", "on", "yes", "true", "TRUE"] {
            std::env::set_var("BLASTIT_SECCOMP", value);
            assert!(seccomp_requested(), "{value} means on");
        }
        // Unset, and a value that is not a boolean at all: on here, and
        // `Config::from_env` rejects the second one right after.
        std::env::remove_var("BLASTIT_SECCOMP");
        assert!(seccomp_requested());
        std::env::set_var("BLASTIT_SECCOMP", "maybe");
        assert!(seccomp_requested());
        assert!(parse_bool("BLASTIT_SECCOMP", "maybe").is_err());
        std::env::remove_var("BLASTIT_SECCOMP");
    }
}

#[test]
fn proxy_variable_names_fold_to_one_key() {
    assert_eq!(proxy_key("db"), "db");
    assert_eq!(proxy_key("ANALYTICS"), "analytics");
    assert_eq!(proxy_key("analytics-db"), "analytics_db");
}

#[test]
fn the_json_document_round_trips() {
    let mut config = Config::default();
    config.port = 9000;
    config.app = Some("web.wsgi:application".to_string());
    config.preload = vec!["web.urls".to_string()];
    config.log_format = LogFormat::Pretty;
    config.log_level = Level::Warn;
    config.view_max_workers = Some(4);
    config.max_view_processes = Some(7);
    config.proxy_max_connections.insert("db".to_string(), 3);
    config.max_header_bytes = 4096;
    config.max_body_bytes = 1 << 20;
    config.health_path = "/health".to_string();
    config.warmup = Some("web.warmup:run".to_string());
    config.immortalize = Immortalize::Off;
    // D5: the fair queue and the one client-IP resolver.
    config.fair_queuing = true;
    config.fq_outer_prefix_v4 = 16;
    config.fq_outer_prefix_v6 = 64;
    config.fq_outer_max = 8;
    config.fq_inner_max_per_outer = 2;
    config.fq_inner_max = 5;
    config.fq_burst = Duration::from_millis(1);
    config.fq_max_debt = Duration::from_millis(2);
    config.fq_debt_ttl = Duration::from_millis(3);
    config.fq_inner_headers = vec!["x-api-key".to_string()];
    config.fq_inner_cookies = Vec::new();
    config.fq_inner_include_ip = false;
    config.client_ip_header = "do-connecting-ip".to_string();
    config.trusted_proxies = vec!["10.0.0.0/8".to_string(), "*".to_string()];
    config.warnings = vec![Warning::TrustForwardedAny];
    config.sys_path = vec![
        b"/app".to_vec(),
        b"/app/.venv/lib/python3.14/site-packages".to_vec(),
        vec![b'/', b'a', 0xff, b'p'],
    ];
    config.privilege = Some(Privilege {
        mode: Mode::Capabilities,
        capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
        bounding_set_dropped: true,
    });
    config.validate().unwrap();

    let json = config.to_json();
    let text = serde_json::to_string(&json).unwrap();
    assert!(!text.contains("BLASTIT_"), "{text}");
    assert_eq!(json["immortalize"], serde_json::json!("off"));
    assert_eq!(json["fair_queuing"], serde_json::json!(true));
    assert_eq!(json["fq_outer_prefix_v4"], serde_json::json!(16));
    assert_eq!(json["fq_burst_ms"], serde_json::json!(1.0));
    assert_eq!(
        json["client_ip_header"],
        serde_json::json!("do-connecting-ip")
    );
    let back = Config::from_json(&json).unwrap();
    assert_eq!(back, config);
    assert_eq!(back.fair_config().client_ip, config.client_ip());
}

#[test]
fn an_immortalisation_setting_the_document_cannot_carry_names_the_field() {
    let mut json = Config::default().to_json();
    assert_eq!(json["immortalize"], serde_json::json!("auto"));
    json["immortalize"] = serde_json::json!("sometimes");
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("immortalize"), "{err}");
}

#[test]
fn a_sys_path_entry_that_is_not_utf8_survives_the_document() {
    let mut config = Config::default();
    // `os.fsencode` bytes that are not a UTF-8 string at all.
    config.sys_path = vec![vec![b'/', b'a', 0xff, b'p'], b"/app".to_vec()];

    let json = config.to_json();
    assert_eq!(json["sys_path"][0], serde_json::json!("/a\u{ff}p"));
    let back = Config::from_json(&json).unwrap();
    assert_eq!(back.sys_path, config.sys_path);
}

#[test]
fn a_sys_path_entry_outside_latin1_is_refused() {
    let mut json = Config::default().to_json();
    json["sys_path"] = serde_json::json!(["/app/\u{20ac}"]);
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("sys_path"), "{err}");
}

#[test]
fn the_working_directory_blastit_started_in_travels_in_the_document() {
    // D2/§5.3: stage 1 puts it first on `sys.path`, so it is carried as
    // bytes like every other path — a directory that is not valid UTF-8 is
    // a directory like any other.
    let mut config = Config::default();
    assert_eq!(config.cwd, None, "nothing was started in a directory yet");
    assert_eq!(config.to_json()["cwd"], serde_json::Value::Null);
    config.cwd = Some(vec![b'/', b'a', 0xff, b'p']);
    let json = config.to_json();
    assert_eq!(json["cwd"], serde_json::json!("/a\u{ff}p"));
    assert_eq!(Config::from_json(&json).unwrap().cwd, config.cwd);

    // A directory outside latin-1, and a member that is not a string: a
    // document stage 0 did not write names the field.
    let mut json = Config::default().to_json();
    json["cwd"] = serde_json::json!("/app/\u{20ac}");
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("cwd"), "{err}");
    let mut json = Config::default().to_json();
    json["cwd"] = serde_json::json!(["/app"]);
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("cwd"), "{err}");
}

#[test]
fn the_privilege_mode_round_trips_and_says_what_it_is() {
    let mut config = Config::default();
    assert_eq!(config.privilege, None, "a default document says nothing");
    let json = config.to_json();
    assert_eq!(json["privilege"], serde_json::Value::Null);
    assert_eq!(json["capabilities"], serde_json::json!([]));
    assert_eq!(json["bounding_set"], serde_json::Value::Null);

    // Root: a mode, no list, and the bounding-set fact anyway (M1: root
    // empties it too, when the container has `cap_setpcap`).
    config.privilege = Some(Privilege {
        mode: Mode::Root,
        capabilities: Vec::new(),
        bounding_set_dropped: true,
    });
    let json = config.to_json();
    assert_eq!(json["privilege"], serde_json::json!("root"));
    assert_eq!(json["capabilities"], serde_json::json!([]));
    assert_eq!(json["bounding_set"], serde_json::json!(true));
    let back = Config::from_json(&json).unwrap();
    assert_eq!(back.privilege, config.privilege);
    assert_eq!(Mode::Root.as_str(), "root");

    // Rootless: the mode, the names init holds, and the one it could not
    // empty.
    config.privilege = Some(Privilege {
        mode: Mode::Capabilities,
        capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
        bounding_set_dropped: false,
    });
    let json = config.to_json();
    assert_eq!(json["privilege"], serde_json::json!("capabilities"));
    assert_eq!(
        json["capabilities"],
        serde_json::json!(["cap_chown", "cap_setuid"])
    );
    assert_eq!(json["bounding_set"], serde_json::json!(false));
    let back = Config::from_json(&json).unwrap();
    assert_eq!(back.privilege, config.privilege);
    assert_eq!(Mode::Capabilities.as_str(), "capabilities");
    assert_eq!(
        config.privilege.as_ref().unwrap().capabilities,
        ["cap_chown", "cap_setuid"]
    );
    assert!(!config.privilege.as_ref().unwrap().bounding_set_dropped);
    // A document that predates the field still reads as "dropped", which is
    // what the boot in force does unless it says otherwise.
    let mut older = json.clone();
    older.as_object_mut().unwrap().remove("bounding_set");
    assert!(
        Config::from_json(&older)
            .unwrap()
            .privilege
            .unwrap()
            .bounding_set_dropped
    );
}

#[test]
fn a_privilege_mode_the_document_cannot_carry_names_the_field() {
    let mut json = Config::default().to_json();
    json["privilege"] = serde_json::json!("sometimes");
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("privilege"), "{err}");
    json["privilege"] = serde_json::json!(1);
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("privilege"), "{err}");
    json["privilege"] = serde_json::json!("capabilities");
    json["bounding_set"] = serde_json::json!("maybe");
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("bounding_set"), "{err}");
}

/// Before D6 the stage-1 document had none of the three privilege members.
///
/// `Privilege::from_json` read them with `get`, so an absent key always meant
/// "this boot detected nothing" and never "this document is broken": the members
/// are optional, and a document a pre-D6 build wrote still boots.
#[test]
fn a_document_without_the_privilege_members_reads_as_nothing_detected() {
    let config = Config::default();
    assert_eq!(config.privilege, None);

    // All three absent at once, which is what a pre-D6 document looks like.
    let mut json = config.to_json();
    let object = json.as_object_mut().unwrap();
    object.remove("privilege");
    object.remove("capabilities");
    object.remove("bounding_set");
    let back = Config::from_json(&json).unwrap();
    assert_eq!(back.privilege, None);
    assert_eq!(back, config);

    // And the two D6 additions on their own: a mode with no list is rootless
    // with no capabilities, and no bounding-set fact is "dropped", which is
    // what the boot in force does unless it says otherwise.
    let mut json = config.to_json();
    json["privilege"] = serde_json::json!("capabilities");
    let object = json.as_object_mut().unwrap();
    object.remove("capabilities");
    object.remove("bounding_set");
    let back = Config::from_json(&json).unwrap();
    assert_eq!(
        back.privilege,
        Some(Privilege {
            mode: Mode::Capabilities,
            capabilities: Vec::new(),
            bounding_set_dropped: true,
        })
    );
}

/// The fail-closed side of the wire structs: `capabilities` is type-checked in
/// both privilege modes (`MB.40.4.1`).
///
/// The hand-written accessor only read the list in `capabilities` mode — in root
/// mode the container's own set is the interesting one, so the document's list
/// was ignored — which carried a broken member silently. Refusing it is
/// deliberate, and stage 0 always writes an array, so no boot changes.
#[test]
fn a_capability_list_that_is_not_strings_is_refused_in_either_mode() {
    let mut json = Config::default().to_json();
    for mode in ["root", "capabilities"] {
        json["privilege"] = serde_json::json!(mode);
        for bad in [serde_json::json!([7]), serde_json::json!("cap_chown")] {
            json["capabilities"] = bad.clone();
            let err = Config::from_json(&json).unwrap_err();
            assert!(
                err.to_string().contains("capabilities"),
                "{mode} {bad}: {err}"
            );
        }
    }
}

/// A member of the wrong shape keeps the sentence the accessor gave it, and no
/// `serde` type name reaches an operator.
#[test]
fn a_wrong_shaped_member_keeps_its_own_sentence() {
    let json = Config::default().to_json();

    // `proxy_max_connections` was read with `get`: an absent member is "not an
    // object", not "the field is missing".
    let mut short = json.clone();
    short
        .as_object_mut()
        .unwrap()
        .remove("proxy_max_connections");
    let err = Config::from_json(&short).unwrap_err();
    assert_eq!(
        err.to_string(),
        "configuration document: proxy_max_connections: must be an object"
    );

    // `php` is the resolved backend or null, and nothing else.
    #[cfg(feature = "php")]
    {
        let mut broken = json.clone();
        broken["php"] = serde_json::json!(5);
        let text = Config::from_json(&broken).unwrap_err().to_string();
        assert_eq!(
            text,
            "configuration document: php: must be an object or null"
        );
        assert!(
            !text.contains("PhpWire") && !text.contains("invalid type"),
            "{text}"
        );
    }

    // A value that is not an object has none of D6's members: "nothing was
    // detected", never a description of a Rust type.
    assert_eq!(
        Privilege::from_json(&serde_json::json!({"nothing": true})).unwrap(),
        None
    );
    assert_eq!(Privilege::from_json(&serde_json::json!(5)).unwrap(), None);
}

#[test]
fn the_privilege_fields_reach_a_lifecycle_event() {
    let privilege = Privilege {
        mode: Mode::Capabilities,
        capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
        bounding_set_dropped: true,
    };
    let lifecycle = privilege.lifecycle(Lifecycle::default().with_stage("stage0"));
    assert_eq!(lifecycle.privilege(), Some("capabilities"));
    assert_eq!(lifecycle.capabilities(), ["cap_chown", "cap_setuid"]);
    assert_eq!(lifecycle.bounding_set(), Some(true));
    let root = Privilege {
        mode: Mode::Root,
        capabilities: Vec::new(),
        bounding_set_dropped: false,
    }
    .lifecycle(Lifecycle::default().with_stage("stage0"));
    assert_eq!(root.privilege(), Some("root"));
    assert_eq!(root.bounding_set(), Some(false), "M1 says which happened");
    assert!(root.capabilities().is_empty());
}

#[test]
fn a_document_missing_a_field_names_it() {
    let mut json = Config::default().to_json();
    json.as_object_mut().unwrap().remove("max_workers");
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("max_workers"), "{err}");
}

#[test]
fn a_health_path_must_be_absolute() {
    let mut config = Config::default();
    config.health_path = "health".to_string();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("BLASTIT_HEALTH_PATH"), "{err}");
}

#[test]
fn an_app_must_be_module_colon_attribute() {
    let mut config = Config::default();
    config.app = Some("web.wsgi".to_string());
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("BLASTIT_APP"), "{err}");
}

#[test]
fn frame_max_is_validated() {
    let mut config = Config::default();
    config.frame_max = 4;
    assert!(config.validate().is_err());
    config.frame_max = blastit_frame::MAX_FRAME_DEFAULT;
    assert!(config.validate().is_ok());
}

#[test]
fn a_view_cap_of_zero_is_refused() {
    // §8 Sizing: a cap of zero could never fork a view process, so every
    // request would time out waiting for one.
    let mut config = Config::default();
    config.max_view_processes = Some(0);
    let err = config.validate().unwrap_err();
    assert!(
        err.to_string().contains("BLASTIT_MAX_VIEW_PROCESSES"),
        "{err}"
    );
    config.max_view_processes = Some(1);
    assert!(config.validate().is_ok());
    config.max_view_processes = None;
    assert!(config.validate().is_ok());
}

#[test]
fn a_view_mem_estimate_of_zero_is_refused() {
    // §8 Sizing: the estimate is the divisor of the derived cap.
    let mut config = Config::default();
    config.view_mem_estimate = 0;
    let err = config.validate().unwrap_err();
    assert!(
        err.to_string().contains("BLASTIT_VIEW_MEM_ESTIMATE"),
        "{err}"
    );
    config.view_mem_estimate = 1;
    assert!(config.validate().is_ok());
}

#[test]
fn the_fair_queuing_defaults_are_the_scheduler_s() {
    // §8.2 (D5): the `BLASTIT_FQ_*` defaults are granian's, and they are
    // `FairConfig`'s, because that is what the scheduler queues with.
    let config = Config::default();
    assert!(!config.fair_queuing, "fair queuing is off by default");
    assert_eq!(config.fair_config(), FairConfig::default());
    assert_eq!(config.fq_outer_prefix_v4, 24);
    assert_eq!(config.fq_outer_prefix_v6, 56);
    assert_eq!(config.fq_outer_max, 65_536);
    assert_eq!(config.fq_inner_max_per_outer, 1_024);
    assert_eq!(config.fq_inner_max, 262_144);
    assert_eq!(config.fq_burst, Duration::from_millis(20));
    assert_eq!(config.fq_max_debt, Duration::from_secs(5));
    assert_eq!(config.fq_debt_ttl, Duration::from_secs(30));
    assert!(config.fq_inner_include_ip);
    assert_eq!(
        config.fq_inner_headers,
        [
            "user-agent",
            "accept-language",
            "sec-ch-ua",
            "sec-ch-ua-platform",
            "sec-ch-ua-mobile",
            "authorization"
        ]
    );
    assert_eq!(config.fq_inner_cookies, ["sessionid", "session"]);
}

#[test]
fn the_default_trusted_proxies_are_the_resolver_s() {
    // The list is spelled out in this module because the document carries
    // values, so this is what keeps it equal to the library's default.
    let config = Config::default();
    assert_eq!(config.client_ip(), ClientIpConfig::default());
    assert_eq!(config.client_ip_header, "x-forwarded-for");
    assert_eq!(config.trusted_proxies, ["127.0.0.1", "::1"]);
    assert!(
        config.warnings.is_empty(),
        "nothing is deprecated by default"
    );
}

#[test]
fn the_fair_queuing_settings_are_read_from_the_environment() {
    let _env = env();
    // SAFETY: `env()` is held for the whole test, so no other test reads or
    // writes the environment while this one runs; every variable is removed
    // again before it returns.
    unsafe {
        let vars = [
            "BLASTIT_FAIR_QUEUING",
            "BLASTIT_FQ_OUTER_PREFIX_V4",
            "BLASTIT_FQ_OUTER_PREFIX_V6",
            "BLASTIT_FQ_OUTER_MAX",
            "BLASTIT_FQ_INNER_MAX_PER_OUTER",
            "BLASTIT_FQ_INNER_MAX",
            "BLASTIT_FQ_BURST",
            "BLASTIT_FQ_MAX_DEBT",
            "BLASTIT_FQ_DEBT_TTL",
            "BLASTIT_FQ_INNER_HEADERS",
            "BLASTIT_FQ_INNER_COOKIES",
            "BLASTIT_FQ_INNER_INCLUDE_IP",
        ];
        for name in vars {
            std::env::remove_var(name);
        }
        assert!(!Config::from_env(None).unwrap().fair_queuing);

        std::env::set_var("BLASTIT_FAIR_QUEUING", "on");
        std::env::set_var("BLASTIT_FQ_OUTER_PREFIX_V4", "16");
        std::env::set_var("BLASTIT_FQ_OUTER_PREFIX_V6", "48");
        std::env::set_var("BLASTIT_FQ_OUTER_MAX", "10");
        std::env::set_var("BLASTIT_FQ_INNER_MAX_PER_OUTER", "3");
        std::env::set_var("BLASTIT_FQ_INNER_MAX", "9");
        std::env::set_var("BLASTIT_FQ_BURST", "0");
        std::env::set_var("BLASTIT_FQ_MAX_DEBT", "250ms");
        std::env::set_var("BLASTIT_FQ_DEBT_TTL", "2m");
        std::env::set_var("BLASTIT_FQ_INNER_HEADERS", "user-agent, x-api-key");
        std::env::set_var("BLASTIT_FQ_INNER_COOKIES", "");
        std::env::set_var("BLASTIT_FQ_INNER_INCLUDE_IP", "0");
        let config = Config::from_env(None).unwrap();
        assert!(config.fair_queuing);
        assert_eq!(config.fq_outer_prefix_v4, 16);
        assert_eq!(config.fq_outer_prefix_v6, 48);
        assert_eq!(config.fq_outer_max, 10);
        assert_eq!(config.fq_inner_max_per_outer, 3);
        assert_eq!(config.fq_inner_max, 9);
        assert_eq!(config.fq_burst, Duration::ZERO);
        assert_eq!(config.fq_max_debt, Duration::from_millis(250));
        assert_eq!(config.fq_debt_ttl, Duration::from_secs(120));
        assert_eq!(config.fq_inner_headers, ["user-agent", "x-api-key"]);
        assert!(
            config.fq_inner_cookies.is_empty(),
            "an empty list hashes no cookie, and is not the default list"
        );
        assert!(!config.fq_inner_include_ip);
        // The scheduler gets exactly what was read.
        let fair = config.fair_config();
        assert_eq!(fair.outer_prefix_v4, 16);
        assert_eq!(fair.inner_headers, ["user-agent", "x-api-key"]);
        assert!(!fair.inner_include_ip);

        // Out-of-range values are clamped, not refused, and the boot
        // warning says so (D5, as the reference implementation does).
        std::env::set_var("BLASTIT_FQ_OUTER_PREFIX_V4", "33");
        std::env::set_var("BLASTIT_FQ_INNER_MAX", "0");
        std::env::set_var("BLASTIT_FQ_DEBT_TTL", "250ms");
        let clamped = Config::from_env(None).unwrap();
        assert_eq!(clamped.fq_outer_prefix_v4, 32);
        assert_eq!(clamped.fq_inner_max, 1);
        assert_eq!(clamped.fq_debt_ttl, MIN_DEBT_TTL);
        assert_eq!(
            clamped.warnings,
            vec![
                Warning::OuterPrefixV4Clamped,
                Warning::InnerMaxClamped,
                Warning::DebtTtlFloored
            ]
        );
        for warning in &clamped.warnings {
            assert!(
                warning.message().contains("BLASTIT_FQ_"),
                "the warning names the variable: {:?}",
                warning.message()
            );
        }
        std::env::set_var("BLASTIT_FQ_OUTER_PREFIX_V4", "16");
        std::env::set_var("BLASTIT_FQ_INNER_MAX", "9");
        std::env::set_var("BLASTIT_FQ_DEBT_TTL", "2m");

        // A value that is not a number at all *is* refused: there is no
        // nearest usable value to a word.
        std::env::set_var("BLASTIT_FQ_INNER_MAX", "many");
        let err = Config::from_env(None).unwrap_err();
        assert!(err.to_string().contains("BLASTIT_FQ_INNER_MAX"), "{err}");
        std::env::set_var("BLASTIT_FQ_INNER_MAX", "9");

        std::env::set_var("BLASTIT_FAIR_QUEUING", "sometimes");
        let err = Config::from_env(None).unwrap_err();
        assert!(err.to_string().contains("BLASTIT_FAIR_QUEUING"), "{err}");

        for name in vars {
            std::env::remove_var(name);
        }
    }
}

#[test]
fn the_client_ip_settings_are_read_from_the_environment() {
    let _env = env();
    // SAFETY: see the test above.
    unsafe {
        for name in [
            "BLASTIT_CLIENT_IP_HEADER",
            "BLASTIT_TRUSTED_PROXIES",
            "BLASTIT_TRUST_FORWARDED",
        ] {
            std::env::remove_var(name);
        }
        let default = Config::from_env(None).unwrap();
        assert_eq!(default.client_ip(), ClientIpConfig::default());

        // App Platform's setting: the edge's own header, believed from the
        // ingress networks only.
        std::env::set_var("BLASTIT_CLIENT_IP_HEADER", "Do-Connecting-IP");
        std::env::set_var("BLASTIT_TRUSTED_PROXIES", "10.0.0.0/8, 100.64.0.0/10");
        let config = Config::from_env(None).unwrap();
        assert_eq!(
            config.client_ip_header, "do-connecting-ip",
            "a header name is case-insensitive"
        );
        let client_ip = config.client_ip();
        assert_eq!(client_ip.header, "do-connecting-ip");
        let headers = vec![(b"do-connecting-ip".to_vec(), b"203.0.113.7".to_vec())];
        assert_eq!(
            client_ip.resolve("10.244.3.9:1", &headers).unwrap().ip(),
            ip("203.0.113.7"),
            "the ingress is trusted"
        );
        assert_eq!(
            client_ip.resolve("8.8.8.8:1", &headers).unwrap().ip(),
            ip("8.8.8.8"),
            "anywhere else the header is not read"
        );

        // `*`, and the list that trusts nobody.
        std::env::set_var("BLASTIT_TRUSTED_PROXIES", "*");
        assert!(Config::from_env(None).unwrap().client_ip().trusted.is_any());
        std::env::set_var("BLASTIT_TRUSTED_PROXIES", " ");
        let none = Config::from_env(None).unwrap();
        assert!(none.trusted_proxies.is_empty());
        assert!(
            !none.client_ip().trusted.contains(&ip("127.0.0.1")),
            "an empty list trusts nobody"
        );

        for name in ["BLASTIT_CLIENT_IP_HEADER", "BLASTIT_TRUSTED_PROXIES"] {
            std::env::remove_var(name);
        }
    }
}

#[test]
fn a_trusted_proxy_that_is_not_a_network_names_its_variable() {
    let _env = env();
    // SAFETY: see the test above.
    unsafe {
        std::env::remove_var("BLASTIT_TRUST_FORWARDED");
        for value in [
            "127.0.0.1:8080",
            "example.com",
            "10.0.0.0/33",
            "10.0.0.0/x",
            "2001:db8::/129",
        ] {
            std::env::set_var("BLASTIT_TRUSTED_PROXIES", value);
            let err = Config::from_env(None).unwrap_err();
            let text = err.to_string();
            assert!(
                text.contains("BLASTIT_TRUSTED_PROXIES"),
                "{value:?}: {text}"
            );
            assert!(
                !text.contains(value.trim()),
                "{value:?}: the error must not quote the value: {text}"
            );
        }
        // A port is the mistake worth naming, because it looks like one
        // peer and is not a network at all.
        std::env::set_var("BLASTIT_TRUSTED_PROXIES", "127.0.0.1:8080");
        assert!(
            Config::from_env(None)
                .unwrap_err()
                .to_string()
                .contains("port"),
            "the port is named as the likely mistake"
        );
        std::env::remove_var("BLASTIT_TRUSTED_PROXIES");
    }
}

#[test]
fn a_client_ip_header_that_is_not_a_header_name_names_its_variable() {
    let _env = env();
    // SAFETY: see the test above.
    unsafe {
        std::env::remove_var("BLASTIT_TRUST_FORWARDED");
        for value in ["x forwarded for", "x-forwarded-for:", "host:8080", ""] {
            std::env::set_var("BLASTIT_CLIENT_IP_HEADER", value);
            let err = Config::from_env(None).unwrap_err();
            assert!(
                err.to_string().contains("BLASTIT_CLIENT_IP_HEADER"),
                "{value:?}: {err}"
            );
        }
        std::env::remove_var("BLASTIT_CLIENT_IP_HEADER");
    }
}

#[test]
fn the_deprecated_trust_forwarded_is_translated_and_warned_about() {
    let _env = env();
    // SAFETY: see the test above.
    unsafe {
        for name in [
            "BLASTIT_CLIENT_IP_HEADER",
            "BLASTIT_TRUSTED_PROXIES",
            "BLASTIT_TRUST_FORWARDED",
        ] {
            std::env::remove_var(name);
        }

        // `0` is what it always was: trust nobody, so the peer is the
        // client.
        std::env::set_var("BLASTIT_TRUST_FORWARDED", "0");
        let config = Config::from_env(None).unwrap();
        assert!(config.trusted_proxies.is_empty());
        assert_eq!(
            config.client_ip().resolve("1.2.3.4:1", &[]).unwrap().ip(),
            ip("1.2.3.4")
        );
        assert_eq!(
            config.warnings,
            vec![Warning::TrustForwardedNone],
            "`0` is translated, and said out loud"
        );
        assert!(
            Warning::TrustForwardedNone.message().contains("deprecated"),
            "the message names the variable and its replacement"
        );

        // `1` is the old rule as well: the last entry of anybody's header,
        // which is `*` here — every peer is trusted and the rightmost entry
        // wins.
        std::env::set_var("BLASTIT_TRUST_FORWARDED", "1");
        let config = Config::from_env(None).unwrap();
        assert_eq!(config.trusted_proxies, ["*"]);
        assert_eq!(config.warnings, vec![Warning::TrustForwardedAny]);
        assert!(config.client_ip().trusted.is_any());

        // The new variable wins, and says so.
        std::env::set_var("BLASTIT_TRUSTED_PROXIES", "10.0.0.0/8");
        let config = Config::from_env(None).unwrap();
        assert_eq!(config.trusted_proxies, ["10.0.0.0/8"]);
        assert_eq!(config.warnings, vec![Warning::TrustForwardedIgnored]);
        assert!(
            Warning::TrustForwardedIgnored.message().contains("ignored"),
            "an ignored setting is named as ignored"
        );

        // A value that is not a boolean at all is an error, not a
        // warning.
        std::env::remove_var("BLASTIT_TRUSTED_PROXIES");
        std::env::set_var("BLASTIT_TRUST_FORWARDED", "maybe");
        let err = Config::from_env(None).unwrap_err();
        assert!(err.to_string().contains("BLASTIT_TRUST_FORWARDED"), "{err}");

        std::env::remove_var("BLASTIT_TRUST_FORWARDED");
    }
}

#[test]
fn a_fair_queue_entry_that_is_not_a_name_is_refused() {
    // §3.1: a header name is a token, and so is a cookie name (RFC 6265). An
    // entry that is a *value* — or that carries a space or a colon — would
    // be hashed against a name no request can have, so it is refused rather
    // than quietly doing nothing.
    let mut config = Config::default();
    for bad in [
        "user agent",
        "user-agent:",
        "a=b",
        "",
        "a\nb",
        "Bearer s3cret",
        "cookie=sessionid",
    ] {
        config.fq_inner_headers = vec![bad.to_string()];
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("BLASTIT_FQ_INNER_HEADERS"),
            "{bad:?}: {err}"
        );
        assert!(err.to_string().contains("header name"), "{bad:?}: {err}");
    }
    for bad in ["session id", "sessionid=", "sessionid; Path=/", ""] {
        config.fq_inner_headers = vec!["user-agent".to_string()];
        config.fq_inner_cookies = vec![bad.to_string()];
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("BLASTIT_FQ_INNER_COOKIES"),
            "{bad:?}: {err}"
        );
        assert!(err.to_string().contains("cookie name"), "{bad:?}: {err}");
    }

    // The names HTTP actually uses pass, including the defaults and the
    // empty-but-for-that case of hashing no header at all.
    config.fq_inner_headers = vec!["x-api-key".to_string(), "sec-ch-ua".to_string()];
    config.fq_inner_cookies = vec!["sessionid".to_string(), "csrf.token".to_string()];
    config.validate().unwrap();
    config.fq_inner_headers = Vec::new();
    config.fq_inner_cookies = Vec::new();
    config.validate().unwrap();
}

#[test]
fn the_fair_queue_knobs_are_clamped_and_reported() {
    // D5, as the reference implementation does it: a prefix that cannot be
    // longer than the address it masks, a table that holds nothing, and a
    // debt TTL under a second all have a nearest usable value, so none of
    // them stops the server from starting.
    let mut config = Config::default();
    config.fq_outer_prefix_v4 = 33;
    config.fq_outer_prefix_v6 = 129;
    config.fq_outer_max = 0;
    config.fq_inner_max_per_outer = 0;
    config.fq_inner_max = 0;
    config.fq_debt_ttl = Duration::from_millis(250);
    config.validate().unwrap();
    assert_eq!(config.fq_outer_prefix_v4, 32);
    assert_eq!(config.fq_outer_prefix_v6, 128);
    assert_eq!(config.fq_outer_max, 1);
    assert_eq!(config.fq_inner_max_per_outer, 1);
    assert_eq!(config.fq_inner_max, 1);
    assert_eq!(config.fq_debt_ttl, MIN_DEBT_TTL);
    assert_eq!(
        config.warnings,
        vec![
            Warning::OuterPrefixV4Clamped,
            Warning::OuterPrefixV6Clamped,
            Warning::OuterMaxClamped,
            Warning::InnerMaxPerOuterClamped,
            Warning::InnerMaxClamped,
            Warning::DebtTtlFloored,
        ]
    );
    // The queue is configured with the clamped values, not with the raw
    // ones, and clamping is not a way of turning fair queuing on.
    let fair = config.fair_config();
    assert_eq!(fair.outer_prefix_v4, 32);
    assert_eq!(fair.outer_max, 1);
    assert_eq!(fair.debt_ttl, MIN_DEBT_TTL);
    assert!(!config.fair_queuing);

    // In range, or exactly on the boundary, nothing is adjusted and
    // nothing is reported.
    let mut config = Config::default();
    config.fq_debt_ttl = MIN_DEBT_TTL;
    config.fq_outer_prefix_v4 = 32;
    config.fq_outer_prefix_v6 = 128;
    config.validate().unwrap();
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    assert_eq!(config.fq_outer_prefix_v6, 128);
}

#[test]
fn the_configuration_warnings_travel_to_stage_1() {
    // The document is what stage 1 reads, and it is stage 1 that prints the
    // warning (§8.1), so the decision has to survive the round trip.
    let mut config = Config::default();
    config.warnings = vec![Warning::TrustForwardedIgnored, Warning::DebtTtlFloored];
    let json = config.to_json();
    assert_eq!(
        json["warnings"],
        serde_json::json!(["trust_forwarded_ignored", "debt_ttl_floored"])
    );
    assert_eq!(Config::from_json(&json).unwrap().warnings, config.warnings);

    // Every token round-trips, and a token this build does not know is a
    // document error that names the field.
    for warning in [
        Warning::TrustForwardedAny,
        Warning::TrustForwardedNone,
        Warning::TrustForwardedIgnored,
    ] {
        assert_eq!(Warning::parse(warning.as_str()).unwrap(), warning);
        assert!(
            warning.message().contains("BLASTIT_TRUST_FORWARDED")
                && warning.message().contains("BLASTIT_TRUSTED_PROXIES"),
            "the message says what to do instead"
        );
    }
    for warning in [
        Warning::OuterPrefixV4Clamped,
        Warning::OuterPrefixV6Clamped,
        Warning::OuterMaxClamped,
        Warning::InnerMaxPerOuterClamped,
        Warning::InnerMaxClamped,
        Warning::DebtTtlFloored,
    ] {
        assert_eq!(Warning::parse(warning.as_str()).unwrap(), warning);
        assert!(
            warning.message().contains("BLASTIT_FQ_") && warning.message().contains("read as"),
            "the message says what it was read as: {:?}",
            warning.message()
        );
    }
    let mut json = json;
    json["warnings"] = serde_json::json!(["something_else"]);
    let err = Config::from_json(&json).unwrap_err();
    assert!(err.to_string().contains("warnings"), "{err}");
}

/// The environment variables a preset could supply, cleared, so that a test
/// reads what the preset says rather than what the shell had.
///
/// # Safety
///
/// The caller holds [`ENV`].
unsafe fn clear_preset_variables() {
    unsafe { std::env::remove_var("BLASTIT_PRESET") };
    for preset in PRESETS {
        for (name, _) in preset.settings() {
            unsafe { std::env::remove_var(name) };
        }
    }
}

/// [`clear_preset_variables`], for as long as it lives (E1).
///
/// A test that leaves `BLASTIT_PRESET`, or a variable a preset supplies,
/// behind — an assertion that failed half-way is enough — would make every
/// test after it read a configuration nobody asked for. Dropping the guard
/// clears them again, and guards drop before every other local of the test
/// that made them.
struct NoPreset;

impl NoPreset {
    /// # Safety
    ///
    /// The caller must hold [`ENV`]: this writes the process environment.
    unsafe fn new() -> Self {
        unsafe { clear_preset_variables() };
        Self
    }
}

impl Drop for NoPreset {
    fn drop(&mut self) {
        // SAFETY: the caller of `new` holds `ENV`, and this runs before the
        // guard on it is dropped.
        unsafe { clear_preset_variables() };
    }
}

#[test]
fn each_preset_supplies_the_documented_defaults() {
    // E1: the table in §8.2. The expectations are spelled out here rather
    // than read from `PRESETS`, so that a value that changes has to change
    // in the documentation too.
    let expected: &[(&str, &str, &[&str], bool)] = &[
        ("digitalocean-paas", "do-connecting-ip", &["*"], true),
        ("compose-traefik", "x-forwarded-for", &["*"], true),
    ];
    let _env = env();
    // SAFETY: `env()` is held for the whole test, so no other test reads or
    // writes the environment while this one runs.
    unsafe {
        let _clean = NoPreset::new();
        for (name, header, trusted, fair) in expected {
            std::env::set_var("BLASTIT_PRESET", name);
            let config = Config::from_env(None).unwrap();
            assert_eq!(config.client_ip_header, *header, "{name}");
            assert_eq!(config.trusted_proxies, *trusted, "{name}");
            assert_eq!(config.fair_queuing, *fair, "{name}");
            assert_eq!(config.log_format, LogFormat::Json, "{name}");
            // The resolved resolver agrees: every peer is trusted, so a
            // client-IP header is read from anywhere (§2.3).
            assert!(config.client_ip().trusted.is_any(), "{name}");
            // The queue is configured with the preset's knobs, and the
            // switch is on: granian's defaults, none of the `FQ_*`
            // variables busy. The client-IP settings are the preset's, so
            // they are carried through to the queue as well.
            assert_eq!(
                config.fair_config(),
                FairConfig {
                    client_ip: config.client_ip(),
                    ..FairConfig::default()
                },
                "granian's defaults, {name}"
            );
        }
        std::env::remove_var("BLASTIT_PRESET");
    }
}

#[test]
fn no_preset_leaves_the_documented_defaults_alone() {
    // E1: a deployment with neither the flag nor the variable is exactly
    // the deployment §8.2 describes, and the boot says `preset: null`.
    let _env = env();
    // SAFETY: `env()` is held for the whole test.
    unsafe {
        std::env::remove_var("BLASTIT_PRESET");
        let _clean = NoPreset::new();
        let config = Config::from_env(None).unwrap();
        assert!(config.preset.is_none());
        assert_eq!(config.client_ip_header, ClientIpConfig::default().header);
        assert_eq!(
            config.trusted_proxies,
            TRUSTED_PROXIES_DEFAULT
                .iter()
                .map(|entry| (*entry).to_string())
                .collect::<Vec<String>>()
        );
        assert!(!config.fair_queuing);
    }
}

#[test]
fn an_explicit_variable_overrides_the_preset() {
    // E1's precedence: built-in default < preset < explicit variable. A
    // preset is a set of *defaults*, so `BLASTIT_PRESET=…` is always safe
    // to add next to variables an operator already set.
    let _env = env();
    // SAFETY: `env()` is held for the whole test.
    unsafe {
        let _clean = NoPreset::new();
        std::env::set_var("BLASTIT_PRESET", "compose-traefik");
        std::env::set_var("BLASTIT_LOG_FORMAT", "pretty");
        std::env::set_var("BLASTIT_FAIR_QUEUING", "0");
        std::env::set_var("BLASTIT_TRUSTED_PROXIES", "127.0.0.1");
        let config = Config::from_env(None).unwrap();
        assert_eq!(config.log_format, LogFormat::Pretty);
        assert!(!config.fair_queuing);
        assert_eq!(config.trusted_proxies, vec!["127.0.0.1".to_string()]);
        assert!(!config.client_ip().trusted.is_any());
        // The one the preset still supplies.
        assert_eq!(config.client_ip_header, "x-forwarded-for");

        let choice = config.preset.expect("the preset is in force");
        assert_eq!(choice.name, "compose-traefik");
        assert_eq!(
            choice.supplied,
            BTreeMap::from([(
                "BLASTIT_CLIENT_IP_HEADER".to_string(),
                "x-forwarded-for".to_string()
            )])
        );
        assert_eq!(
            choice.overridden,
            vec![
                "BLASTIT_TRUSTED_PROXIES".to_string(),
                "BLASTIT_FAIR_QUEUING".to_string(),
                "BLASTIT_LOG_FORMAT".to_string(),
            ]
        );

        // A variable that is set to nothing is set: an empty
        // `BLASTIT_TRUSTED_PROXIES` means "trust nobody", and the preset's
        // `*` may not come back through the empty value.
        std::env::set_var("BLASTIT_TRUSTED_PROXIES", "");
        let config = Config::from_env(None).unwrap();
        assert!(config.trusted_proxies.is_empty());
        assert!(!config.client_ip().trusted.is_any());

        for name in [
            "BLASTIT_PRESET",
            "BLASTIT_LOG_FORMAT",
            "BLASTIT_FAIR_QUEUING",
            "BLASTIT_TRUSTED_PROXIES",
        ] {
            std::env::remove_var(name);
        }
    }
}

#[test]
fn the_flag_names_the_preset_and_wins_over_the_variable() {
    // E1: `--preset` is the more explicit of the two — it is typed at the
    // process that is being started — so it wins, and the boot says which
    // preset it settled on. When the two disagree, that is worth a line of
    // its own.
    let _env = env();
    // SAFETY: `env()` is held for the whole test.
    unsafe {
        let _clean = NoPreset::new();
        std::env::set_var("BLASTIT_PRESET", "compose-traefik");

        let config = Config::from_env(Some("digitalocean-paas")).unwrap();
        let choice = config.preset.expect("the preset is in force");
        assert_eq!(choice.name, "digitalocean-paas");
        assert_eq!(config.client_ip_header, "do-connecting-ip");
        assert_eq!(config.warnings, vec![Warning::PresetFlagWins]);

        // The same name twice is not a disagreement.
        let config = Config::from_env(Some("compose-traefik")).unwrap();
        assert_eq!(
            config.preset.expect("the preset is in force").name,
            "compose-traefik"
        );
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);

        // The flag alone, with no variable: no warning either.
        std::env::remove_var("BLASTIT_PRESET");
        let config = Config::from_env(Some("compose-traefik")).unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.client_ip_header, "x-forwarded-for");
    }
}

#[test]
fn an_unknown_preset_name_is_refused_and_lists_the_known_ones() {
    // E1: a name that is not a preset is a startup error, and the error
    // says what the presets are — an operator who typo'd `compose-trafik`
    // needs the list more than they need the name they wrote.
    let _env = env();
    // SAFETY: `env()` is held for the whole test.
    unsafe {
        let _clean = NoPreset::new();
        let err = Config::from_env(Some("compose-trafik")).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--preset"), "{text}");
        assert!(text.contains("compose-trafik"), "{text}");
        for preset in PRESETS {
            assert!(text.contains(preset.name()), "{text}");
        }

        std::env::set_var("BLASTIT_PRESET", "compose-trafik");
        let err = Config::from_env(None).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("BLASTIT_PRESET"), "{text}");
        assert!(text.contains("compose-traefik"), "{text}");

        // The flag is checked before the variable, so a bad flag is
        // reported even when the variable is fine — and a bad variable is
        // reported when there is no flag.
        std::env::set_var("BLASTIT_PRESET", "compose-traefik");
        assert!(Config::from_env(Some("nope")).is_err());
        assert!(Config::from_env(Some("compose-traefik")).is_ok());
        std::env::remove_var("BLASTIT_PRESET");

        // An empty variable is "unset", not "the preset called ''": it is
        // how a deployment clears an inherited value.
        std::env::set_var("BLASTIT_PRESET", "");
        assert!(Config::from_env(None).unwrap().preset.is_none());
        std::env::remove_var("BLASTIT_PRESET");
    }
}

#[test]
fn the_preset_name_is_trimmed_however_it_arrives() {
    // E1: the variable is read the way the flag is — an env file, a compose
    // `environment:` entry or a CI variable is easy to pad by accident — and
    // a value that is only whitespace is an unset variable.
    let _env = env();
    // SAFETY: `env()` is held for the whole test.
    unsafe {
        let _clean = NoPreset::new();

        std::env::set_var("BLASTIT_PRESET", "  compose-traefik  ");
        let config = Config::from_env(None).unwrap();
        assert_eq!(
            config.preset.as_ref().map(|choice| choice.name.as_str()),
            Some("compose-traefik")
        );
        assert_eq!(config.client_ip_header, "x-forwarded-for");

        // A padded name still matches the flag, so a padded variable is not
        // a disagreement worth a warning.
        let config = Config::from_env(Some("compose-traefik")).unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(
            config.preset.as_ref().map(|choice| choice.name.as_str()),
            Some("compose-traefik")
        );

        for empty in ["", " ", "\t", "\n ", " \t"] {
            std::env::set_var("BLASTIT_PRESET", empty);
            let config = Config::from_env(None).unwrap();
            assert!(config.preset.is_none(), "{empty:?} is unset");
        }
    }
}

#[test]
fn the_stage_one_document_carries_the_preset() {
    // E1: the preset travels in the configuration document like everything
    // else, because stage 1's environment is the app's and has no
    // `BLASTIT_*` variable in it at all (§4, §5.3).
    let _env = env();
    // SAFETY: `env()` is held for the whole test.
    unsafe {
        let _clean = NoPreset::new();
        std::env::set_var("BLASTIT_PRESET", "compose-traefik");
        std::env::set_var("BLASTIT_LOG_FORMAT", "pretty");
        let config = Config::from_env(None).unwrap();
        let json = config.to_json();
        assert_eq!(json["preset"]["name"], "compose-traefik");
        assert_eq!(
            json["preset"]["supplied"]["BLASTIT_CLIENT_IP_HEADER"],
            "x-forwarded-for"
        );
        assert_eq!(
            json["preset"]["overridden"],
            serde_json::json!(["BLASTIT_LOG_FORMAT"])
        );
        assert_eq!(Config::from_json(&json).unwrap(), config);

        // The preset member names the variable that was overridden, and
        // never the value that won: that one came from the environment, and
        // §8.1 keeps environment values out of the log and out of the
        // document's preset record alike. (The *effective* value is in the
        // document, of course — that is what the document is for.)
        let recorded = json["preset"].to_string();
        assert!(!recorded.contains("pretty"), "{recorded}");
        assert!(recorded.contains("BLASTIT_LOG_FORMAT"), "{recorded}");

        // No preset: `null`, and the round trip keeps that.
        std::env::remove_var("BLASTIT_PRESET");
        std::env::remove_var("BLASTIT_LOG_FORMAT");
        let none = Config::from_env(None).unwrap();
        let json = none.to_json();
        assert!(json["preset"].is_null(), "{json}");
        let back = Config::from_json(&json).unwrap();
        assert!(back.preset.is_none());
        assert_eq!(back, none);

        // A document stage 0 did not write is refused, naming the field.
        let mut broken = json;
        broken["preset"] = serde_json::json!({"name": 7});
        let err = Config::from_json(&broken).unwrap_err();
        assert!(err.to_string().contains("preset"), "{err}");
        let mut broken = none.to_json();
        broken["preset"] = serde_json::json!("compose-traefik");
        let err = Config::from_json(&broken).unwrap_err();
        assert!(err.to_string().contains("preset"), "{err}");
        let mut missing = none.to_json();
        missing.as_object_mut().unwrap().remove("preset");
        let err = Config::from_json(&missing).unwrap_err();
        assert!(err.to_string().contains("preset"), "{err}");
    }
}

#[test]
fn a_preset_whose_assumption_does_not_hold_is_named_but_never_refused() {
    // E1, §2.1: the mode is detected rather than configured, so a preset
    // used where its platform's identity does not hold says so, and never
    // refuses: root in a rootless deployment still works. Stage 0 prints the
    // line itself, which is why this returns the notice instead of adding it
    // to `warnings` for stage 1.
    let chosen = |preset: &str| Config {
        preset: Some(PresetChoice {
            name: preset.to_string(),
            supplied: BTreeMap::new(),
            overridden: Vec::new(),
        }),
        ..Config::default()
    };

    let digitalocean = chosen("digitalocean-paas");
    let warning = digitalocean
        .preset_assumption_warning(Mode::Capabilities)
        .expect("a root preset booted without capabilities");
    assert_eq!(warning, Warning::PresetExpectsRoot);
    assert!(warning.message().contains("root"));
    assert_eq!(
        Warning::parse(warning.as_str()).unwrap(),
        Warning::PresetExpectsRoot,
        "the token round-trips even though stage 0 prints the sentence"
    );
    assert!(
        digitalocean.warnings.is_empty(),
        "the notice is printed at the boot, not carried to stage 1: {:?}",
        digitalocean.warnings
    );

    // The mode the preset assumes says nothing at all, and asking twice
    // cannot say anything twice.
    assert_eq!(digitalocean.preset_assumption_warning(Mode::Root), None);
    assert_eq!(
        digitalocean.preset_assumption_warning(Mode::Capabilities),
        Some(Warning::PresetExpectsRoot)
    );

    // And the other way round: a rootless preset booted as root.
    let rootless = chosen("compose-traefik");
    let warning = rootless
        .preset_assumption_warning(Mode::Root)
        .expect("a rootless preset booted as root");
    assert_eq!(warning, Warning::PresetExpectsRootless);
    assert!(warning.message().contains("rootless"));
    assert_eq!(
        Warning::parse(warning.as_str()).unwrap(),
        Warning::PresetExpectsRootless
    );
    assert_eq!(rootless.preset_assumption_warning(Mode::Capabilities), None);

    // No preset, no notice — whatever mode the boot is in — and a name this
    // build does not know has nothing to assume.
    let none = Config::default();
    assert_eq!(none.preset_assumption_warning(Mode::Root), None);
    assert_eq!(none.preset_assumption_warning(Mode::Capabilities), None);
    assert_eq!(
        chosen("gone-in-a-later-build").preset_assumption_warning(Mode::Root),
        None
    );
}

#[test]
fn every_preset_table_is_well_formed() {
    // Every preset is shipped, so its table has to be usable: the names are
    // §8.2 variables, the values parse, and the help text names them all.
    for preset in PRESETS {
        let name = preset.name();
        assert!(!name.is_empty(), "{name}");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "`{name}` is a slug an operator can type"
        );
        assert!(!preset.summary().is_empty(), "{name}");
        let mut names: Vec<&str> = Vec::new();
        for (variable, value) in preset.settings() {
            assert!(
                variable.starts_with("BLASTIT_"),
                "{name} sets `{variable}`, which is not a BLASTIT_* variable"
            );
            assert!(!names.contains(variable), "{name} sets {variable} twice");
            names.push(variable);
            assert!(
                !value.is_empty() && !value.contains(char::is_whitespace),
                "{name}: `{variable}` is set to a value with whitespace in it"
            );
            assert_eq!(
                preset.get(variable),
                Some(*value),
                "{name} must be able to look up {variable}"
            );
        }
        // The tables this build ships are exactly the two §8.2 documents;
        // anything else would be a preset nobody measured.
        let known = ["digitalocean-paas", "compose-traefik"];
        assert!(known.contains(&name), "`{name}` is not a documented preset");
        assert!(
            preset_list().contains(name) && preset_help().contains(name),
            "{name} is missing from the help text"
        );
        assert!(preset_help().lines().count() == PRESETS.len());
        // A preset this build does not know is not a preset.
        assert!(Preset::find(name).is_some());
    }
    assert!(Preset::find("nope").is_none());
    assert_eq!(PRESETS.len(), 2);
}

/// The stage-0 → stage-1 document is the `--config-fd` re-exec contract.
///
/// Stage 0 writes it, `execve` hands the pipe to stage 1, and stage 1 reads it
/// back through the wire structs. So the key set, the nesting and the members an
/// older document may lack are pinned here: a field that is renamed, dropped or
/// made mandatory fails this test rather than a boot.
#[test]
fn the_document_key_set_is_the_stage_0_contract() {
    let mut config = Config::default();
    config.app = Some("web.wsgi:application".to_string());
    config.libphp = Some(PathBuf::from("/usr/lib/libphp8.4.so"));
    config.php_shim_dir = Some(PathBuf::from("/opt/blastit/shims"));
    config.proxy_max_connections.insert("db".to_string(), 4);
    config
        .proxy_outputs
        .insert("db".to_string(), vec!["BLASTIT_PROXY_DB_URL".to_string()]);
    config.sys_path = vec![b"/app".to_vec()];
    config.cwd = Some(b"/srv/app".to_vec());
    config.warnings = vec![Warning::SeccompDisabled];
    config.privilege = Some(Privilege {
        mode: Mode::Capabilities,
        capabilities: vec!["cap_chown".to_string()],
        bounding_set_dropped: true,
    });
    config.preset = Some(PresetChoice {
        name: "compose-traefik".to_string(),
        supplied: BTreeMap::from([(
            "BLASTIT_CLIENT_IP_HEADER".to_string(),
            "x-forwarded-for".to_string(),
        )]),
        overridden: Vec::new(),
    });

    let json = config.to_json();
    let mut keys = json
        .as_object()
        .expect("the document is an object")
        .keys()
        .cloned()
        .collect::<Vec<String>>();
    keys.sort();
    let mut expected: Vec<&str> = vec![
        "port",
        "server_port",
        "app",
        "preload",
        "manifest",
        "manifest_dir",
        "libphp",
        "php_shim_dir",
        "script_name",
        "log_format",
        "log_level",
        "uid_base",
        "max_workers",
        "view_max_workers",
        "queue_size",
        "queue_timeout_ms",
        "request_timeout_ms",
        "stuck_threads",
        "view_spawn",
        "view_idle_timeout_ms",
        "view_max_requests",
        "max_view_processes",
        "view_mem_estimate",
        "mem_high_watermark_percent",
        "immortalize",
        "max_header_bytes",
        "max_body_bytes",
        "header_timeout_ms",
        "keepalive_timeout_ms",
        "fair_queuing",
        "fq_outer_prefix_v4",
        "fq_outer_prefix_v6",
        "fq_outer_max",
        "fq_inner_max_per_outer",
        "fq_inner_max",
        "fq_burst_ms",
        "fq_max_debt_ms",
        "fq_debt_ttl_ms",
        "fq_inner_headers",
        "fq_inner_cookies",
        "fq_inner_include_ip",
        "client_ip_header",
        "trusted_proxies",
        "warnings",
        "health_path",
        "proxy_max_connections",
        "env_strip",
        "seccomp",
        "graceful_timeout_ms",
        "warmup",
        "frame_max",
        "backlog",
        "max_connections",
        "proxy_outputs",
        "privilege",
        "capabilities",
        "bounding_set",
        "sys_path",
        "cwd",
        "preset",
    ];
    // `php` is written by a build that has the feature, and stage 1 of the same
    // build reads it; a build without it writes no member at all.
    #[cfg(feature = "php")]
    expected.push("php");
    expected.sort_unstable();
    assert_eq!(keys, expected, "the re-exec document key set");

    // The nesting stage 1 reads: D6's three members, the preset's three, and
    // the resolved backend.
    assert_eq!(json["privilege"], serde_json::json!("capabilities"));
    assert_eq!(json["capabilities"], serde_json::json!(["cap_chown"]));
    assert_eq!(json["bounding_set"], serde_json::json!(true));
    assert_eq!(json["preset"]["name"], "compose-traefik");
    assert_eq!(
        json["preset"]["supplied"]["BLASTIT_CLIENT_IP_HEADER"],
        "x-forwarded-for"
    );
    assert_eq!(json["preset"]["overridden"], serde_json::json!([]));
    #[cfg(feature = "php")]
    assert_eq!(json["php"], serde_json::Value::Null);

    // Read back through the wire structs: the same configuration, and a
    // document that serialises to the same bytes.
    let text = serde_json::to_string(&json).expect("the document is JSON");
    let back = Config::from_json(&json).expect("a document stage 0 wrote");
    assert_eq!(back, config);
    assert_eq!(
        serde_json::to_string(&back.to_json()).expect("the document is JSON"),
        text,
        "the writer and the reader are inverses"
    );

    // The members a document written before them simply does not have: absent
    // means "nothing was resolved" (or, for D6, "nothing was detected"), and
    // never an error.
    let mut older = json.clone();
    let object = older.as_object_mut().expect("the document is an object");
    object.remove("libphp");
    object.remove("php_shim_dir");
    object.remove("privilege");
    object.remove("capabilities");
    object.remove("bounding_set");
    #[cfg(feature = "php")]
    object.remove("php");
    let back = Config::from_json(&older).expect("a pre-D6, pre-libphp document still reads");
    assert_eq!(back.libphp, None);
    assert_eq!(back.php_shim_dir, None);
    assert_eq!(back.privilege, None, "a pre-D6 document detected nothing");
    let mut base_shaped = config.clone();
    base_shaped.libphp = None;
    base_shaped.php_shim_dir = None;
    base_shaped.privilege = None;
    assert_eq!(back, base_shaped);

    // A required member is not optional: its absence names it.
    let mut short = json;
    short
        .as_object_mut()
        .expect("the document is an object")
        .remove("request_timeout_ms");
    let err = Config::from_json(&short).expect_err("a member is missing");
    assert!(err.to_string().contains("request_timeout_ms"), "{err}");
}
