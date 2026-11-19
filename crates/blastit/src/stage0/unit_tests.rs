#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::ffi::{CString, OsStr};
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

use blastit_dbproxy::{DbproxyConfig, ListenerSpec, ProxySpec, write_config};
use blastit_events::{
    Component, Event, EventKind, Level, Lifecycle, ManifestWarning as WarningEvent, Runtime, Stream,
};
use blastit_manifest::{Manifest, PgGrant, ProxyName, UidLayout};
use blastit_sys::{
    Fork, Pid, Rlimits, Signal, bind_unix_listener, current_signal_mask, exec, harden_child,
    make_private_dir, pipe, restore_signal_mask_in_child, set_cloexec, set_dumpable,
};

use crate::child::{ChildIo, Deadline, read_one, wait_readable};
use crate::cli::DevOptions;
use crate::config::{Config, LogFormat, Mode, Privilege, view_dir};
use crate::dev_learn::{DevRoot, PRIVILEGE};
// The two provenance types are the deployment gate's (`php` only): the rest of
// stage 0 names neither.
#[cfg(feature = "php")]
use crate::host::{Claim, Provenance};
use crate::host::{EXEC, Host, Install, Placement, Reexec};
use crate::logs::{Emitter, fail, output_event};
use crate::mem_usage;
// The `entrypoint php` discovery gate (`MB.36.5`): a stage-0 dependency that
// names no SAPI type and no `dlopen`, so `init` can answer "can this host embed
// PHP?" without loading anything (`ADR-0010` §3, design §4.4 D5).
#[cfg(feature = "php")]
use blastit_view_php::discovery::{Deployment, PhpBoot, PhpSearch, PhpShimScope, php_boot_in};
// The discovery surface the `php_boot_gate` cases drive: their subjects are the
// crate's, so the names are the tests' only.
#[cfg(feature = "python")]
#[cfg(all(test, feature = "php"))]
use blastit_view_php::discovery::{PhpVersion, discover_php, shim_name};

use crate::stage0::dev::*;
use crate::stage0::fork_exec::*;
use crate::stage0::gates::*;
use crate::stage0::*;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::Read;
    use std::time::Instant;

    use super::*;

    /// Everything an emitter wrote to `read_end`, once its peer is closed.
    fn captured(fd: OwnedFd) -> String {
        let mut file = std::fs::File::from(fd);
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    /// Drains `child_output` and returns what was logged, with how long the
    /// drain itself took.
    fn drain(child_output: &OwnedFd, stream: Stream) -> (String, Duration) {
        let (capture_read, capture_write) = pipe().unwrap();
        let emitter = Emitter::new(
            capture_write.as_raw_fd(),
            LogFormat::Json,
            Level::Info,
            false,
        );
        let started = Instant::now();
        emit_child_output(&emitter, child_output, stream);
        let elapsed = started.elapsed();
        drop(capture_write);
        (captured(capture_read), elapsed)
    }

    /// The read end of a pipe whose write end the caller keeps for itself.
    fn output_pipe() -> (OwnedFd, OwnedFd) {
        pipe().unwrap()
    }

    /// A manifest with `routes` routes, in the order they matter here.
    fn manifest_with(routes: usize) -> Manifest {
        let mut source = String::new();
        for index in 0..routes {
            source.push_str(&format!("view r{index} \"/r{index}/\" {{}}\n"));
        }
        Manifest::parse_blastfile(&source, 20_000).unwrap().0
    }

    #[test]
    fn the_layout_collision_check_knows_every_process_it_hands_a_uid_to() {
        // D6: init is an ordinary uid in rootless mode, so its own uid must not
        // be one the layout gives away.
        let manifest = manifest_with(3);
        let layout = UidLayout::new(20_000);
        assert_eq!(
            layout_collision(&manifest, &layout, 20_001).unwrap(),
            "the http process"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 20_002).unwrap(),
            "the scheduler"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 20_003).unwrap(),
            "dbproxy"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 21_000).unwrap(),
            "route `r0`"
        );
        assert_eq!(
            layout_collision(&manifest, &layout, 21_002).unwrap(),
            "route `r2`"
        );
    }

    #[test]
    fn a_uid_outside_the_layout_is_free() {
        let manifest = manifest_with(2);
        let layout = UidLayout::new(20_000);
        // The ordinary case: a service uid, and root.
        assert!(layout_collision(&manifest, &layout, 0).is_none());
        assert!(layout_collision(&manifest, &layout, 1000).is_none());
        // A view uid of a route this manifest does not have is free too.
        assert!(layout_collision(&manifest, &layout, 21_002).is_none());
        // And the whole check disappears when the base is a high, private one.
        let layout = UidLayout::new(100_000);
        assert!(layout_collision(&manifest, &layout, 20_001).is_none());
        assert_eq!(
            layout_collision(&manifest, &layout, 101_001).unwrap(),
            "route `r1`"
        );
    }

    #[test]
    fn the_stage_zero_boot_event_says_the_mode() {
        let root = stage0_lifecycle(Some(&Privilege {
            mode: Mode::Root,
            capabilities: Vec::new(),
            bounding_set_dropped: true,
        }));
        assert_eq!(root.stage(), Some("stage0"));
        assert_eq!(root.privilege(), Some("root"));
        assert!(root.capabilities().is_empty());
        assert_eq!(root.bounding_set(), Some(true));

        let rootless = stage0_lifecycle(Some(&Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_chown".to_string(), "cap_setuid".to_string()],
            bounding_set_dropped: false,
        }));
        assert_eq!(rootless.privilege(), Some("capabilities"));
        assert_eq!(rootless.capabilities(), ["cap_chown", "cap_setuid"]);
        assert_eq!(
            rootless.bounding_set(),
            Some(false),
            "M1: a deployment without cap_setpcap says so"
        );

        // A boot that could not read its own capability sets says nothing about
        // the mode rather than guessing one.
        let unknown = stage0_lifecycle(None);
        assert_eq!(unknown.stage(), Some("stage0"));
        assert_eq!(unknown.privilege(), None);
        assert_eq!(unknown.bounding_set(), None);
    }

    #[test]
    fn the_configuration_boot_event_says_the_mode_and_the_preset() {
        // E1: the first `boot` line is printed before the configuration is
        // read, so this is the one that says what the boot is: the mode init
        // detected, whether the filters are on, and which preset supplied which
        // defaults.
        let privilege = Privilege {
            mode: Mode::Capabilities,
            capabilities: vec!["cap_setuid".to_string()],
            bounding_set_dropped: true,
        };
        let mut config = Config::default();
        assert_eq!(
            config_lifecycle(&privilege, &config).stage(),
            Some("config")
        );
        assert_eq!(
            config_lifecycle(&privilege, &config).preset(),
            None,
            "no preset: the event says null, which the encoder writes"
        );

        config.preset = Some(crate::config::PresetChoice {
            name: "compose-traefik".to_string(),
            supplied: std::collections::BTreeMap::from([(
                "BLASTIT_CLIENT_IP_HEADER".to_string(),
                "x-forwarded-for".to_string(),
            )]),
            overridden: vec!["BLASTIT_LOG_FORMAT".to_string()],
        });
        let lifecycle = config_lifecycle(&privilege, &config);
        assert_eq!(lifecycle.stage(), Some("config"));
        assert_eq!(lifecycle.privilege(), Some("capabilities"));
        assert_eq!(lifecycle.capabilities(), ["cap_setuid"]);
        assert_eq!(lifecycle.seccomp(), Some(true), "the effective setting");
        assert_eq!(lifecycle.preset(), Some("compose-traefik"));
        assert_eq!(
            lifecycle.preset_settings().get("BLASTIT_CLIENT_IP_HEADER"),
            Some(&"x-forwarded-for".to_string())
        );
        assert_eq!(lifecycle.preset_overridden(), ["BLASTIT_LOG_FORMAT"]);

        // The filters being off is the configuration's word, not the
        // variable's: a `boot` line that disagrees with the running server is
        // the failure this field exists to prevent (§7.3).
        config.seccomp = false;
        assert_eq!(config_lifecycle(&privilege, &config).seccomp(), Some(false));
    }

    #[test]
    fn the_detected_mode_becomes_the_boot_field() {
        let root = privilege_from(&blastit_sys::Held {
            mode: blastit_sys::PrivilegeMode::Root,
            capabilities: Vec::new(),
            bounding_set_dropped: true,
        });
        assert_eq!(root.mode, Mode::Root);
        assert!(root.capabilities.is_empty());
        assert!(root.bounding_set_dropped);
        let rootless = privilege_from(&blastit_sys::Held {
            mode: blastit_sys::PrivilegeMode::Capabilities,
            capabilities: vec!["cap_chown", "cap_setuid"],
            bounding_set_dropped: false,
        });
        assert_eq!(rootless.mode, Mode::Capabilities);
        assert_eq!(rootless.capabilities, ["cap_chown", "cap_setuid"]);
        assert!(!rootless.bounding_set_dropped);
        assert_eq!(rootless.mode.as_str(), "capabilities");
    }
    /// A dbproxy that printed something and then hung holds its write end open:
    /// the drain has to read what it said without waiting for the process.
    #[test]
    fn a_live_child_that_printed_is_still_logged() {
        let (read_end, write_end) = output_pipe();
        rustix::io::write(&write_end, b"exec dbproxy: Permission denied\n").unwrap();

        let (text, elapsed) = drain(&read_end, Stream::Stdout);
        assert!(
            elapsed < Duration::from_secs(2),
            "the drain waited for a live child: {elapsed:?}"
        );
        assert!(text.contains("Permission denied"), "{text}");
        assert!(text.contains(r#""component":"dbproxy""#), "{text}");
        assert!(text.contains(r#""event":"app.output""#), "{text}");
    }

    /// And a live child that said *nothing* must not block the boot either: this
    /// is the timeout case, where the process is still alive and still holds the
    /// write end of both pipes.
    #[test]
    fn a_silent_live_child_does_not_block_the_drain() {
        let (read_end, _write_end) = output_pipe();

        let (text, elapsed) = drain(&read_end, Stream::Stderr);
        assert!(
            elapsed < Duration::from_secs(2),
            "the drain hung: {elapsed:?}"
        );
        assert!(text.is_empty(), "nothing was written: {text}");
    }

    /// The ordinary failure: dbproxy is gone, and what it printed on the way out
    /// is the only explanation of why.
    #[test]
    fn a_dead_child_output_becomes_app_output_events() {
        let (read_end, write_end) = output_pipe();
        rustix::io::write(&write_end, b"first\nsecond\n").unwrap();
        drop(write_end);

        let (text, _) = drain(&read_end, Stream::Stdout);
        assert!(text.contains("first"), "{text}");
        assert!(text.contains("second"), "{text}");
    }

    /// The wait itself: a dbproxy that never writes must expire the deadline —
    /// and quickly, which is why the timeout is a parameter.
    #[test]
    fn a_dbproxy_that_never_reports_ready_times_out() {
        let (ready, _write_end) = output_pipe();
        let started = Instant::now();
        let err = wait_ready(&ready, Duration::from_millis(50)).unwrap_err();
        assert!(err.contains("did not report ready within"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the deadline was not honoured: {:?}",
            started.elapsed()
        );
    }

    /// And a dbproxy that died first is reported as that, not as a timeout.
    #[test]
    fn a_dbproxy_that_exits_before_ready_is_reported_as_such() {
        let (ready, write_end) = output_pipe();
        drop(write_end);
        let err = wait_ready(&ready, Duration::from_secs(30)).unwrap_err();
        assert_eq!(err, "dbproxy exited before it was ready");
    }

    /// The arithmetic and the wording live in [`crate::mem_usage`]; this pins
    /// the wiring: `memory_refusal` compares the reading against *the config's*
    /// `BLASTIT_MEM_HIGH_WATERMARK`, the same ceiling the scheduler uses.
    #[test]
    fn memory_refusal_takes_its_ceiling_from_the_config() {
        let gib = 1024 * 1024 * 1024;
        let mut config = Config::for_dev().expect("a dev config");
        config.mem_high_watermark_percent = 85.0;
        let reading = |used: u64| crate::mem_usage::Usage::new(100 * gib, used, 0);
        assert!(memory_refusal(&reading(80 * gib), &config).is_none());
        assert!(
            memory_refusal(&reading(85 * gib), &config).is_none(),
            "the watermark is a ceiling, not a trigger"
        );
        let message = memory_refusal(&reading(86 * gib), &config).expect("above the watermark");
        assert!(message.contains("BLASTIT_MEM_HIGH_WATERMARK"), "{message}");
        assert!(message.contains("(85%)"), "the knob: {message}");
        assert!(message.contains("503"), "the symptom: {message}");
        // A 0% watermark can never be met: it is how a test forces the high arm
        // without faking `/proc`.
        config.mem_high_watermark_percent = 0.0;
        assert!(memory_refusal(&reading(1), &config).is_some());
        // An unreadable or empty reading is not pressure.
        assert!(memory_refusal(&reading(0), &config).is_none());
    }

    #[test]
    fn an_explicit_empty_grant_still_names_the_proxy_for_its_route() {
        let source = "\
postgres $DATABASE_URL

trait denied {
    postgres {}
}

view locked regex '/locked/.*' {
    postgres {}
}

view grouped regex '/grouped/.*' {
    use denied
}

view ungranted regex '/ungranted/.*' {}
";
        let (manifest, _) = Manifest::parse_blastfile(source, 20_000).unwrap();
        let db = ProxyName::new("db");
        assert!(
            route_declares_proxy(&manifest, 0, &db),
            "an inline `queries = []` is a declaration"
        );
        assert!(
            route_declares_proxy(&manifest, 1, &db),
            "an attached group that names the proxy is a declaration"
        );
        assert!(
            !route_declares_proxy(&manifest, 2, &db),
            "a route that names no proxy gets no listener"
        );
        // The denied twin: `effective_grants` is what drops the empty grant, so
        // the two functions disagree on `locked` on purpose.
        assert!(
            !manifest.effective_grants(0).contains_key(&db),
            "the manifest model still calls an empty grant empty"
        );
    }

    /// `MB.23.5.3`: the dev policy channel is one `AF_UNIX`/`SOCK_STREAM`
    /// socketpair, both ends close-on-exec and non-blocking, and the two ends
    /// really are one connected pair.
    #[test]
    fn the_dev_policy_channel_is_one_nonblocking_socketpair() {
        let emitter = Emitter::new(2, LogFormat::Json, Level::Info, false);
        let root = DevRoot::create().expect("a runtime root");
        let (stage1, dbproxy) = dev_policy_channel(&emitter, &root);
        for (name, end) in [("stage 1", &stage1), ("dbproxy", &dbproxy)] {
            let status = rustix::fs::fcntl_getfl(end).expect("the status flags");
            assert!(
                status.contains(rustix::fs::OFlags::NONBLOCK),
                "the {name} end is non-blocking: the supervisor writes through its poll set"
            );
            let flags = rustix::io::fcntl_getfd(end).expect("the descriptor flags");
            assert!(
                flags.contains(rustix::io::FdFlags::CLOEXEC),
                "the {name} end is close-on-exec at birth"
            );
        }
        // One connected pair: a byte written on one end is readable on the other.
        rustix::io::write(&stage1, b"x").expect("written");
        let mut byte = [0u8; 1];
        assert_eq!(rustix::io::read(&dbproxy, &mut byte).expect("read"), 1);
        assert_eq!(byte[0], b'x');
        root.cleanup();
    }

    /// `MB.25.2`: the `runtime` line's decision table. A resolved library is the
    /// probe's to report (only it ran `php_module_startup`), no callable means
    /// static mounts and no language runtime, and a callable is only nameable
    /// where an interpreter is already live — a wrong "yes" answers no line, not
    /// a panic, and never an invented version.
    #[test]
    fn the_runtime_shape_is_decided_by_what_the_boot_loads() {
        assert_eq!(
            runtime_shape(true, true, true),
            None,
            "PHP's line is the probe's, never stage 0's"
        );
        assert_eq!(
            runtime_shape(false, false, false),
            Some(Runtime::Static),
            "no library and no callable is a static-only file"
        );
        assert_eq!(
            runtime_shape(false, true, false),
            None,
            "no interpreter here: there is nothing honest to say"
        );
    }
}

/// `ADR-0009` §5: the boot gate of a build with no Python support (`MB.3.11`).
///
/// What it allows is as much a part of the gate as what it refuses: a
/// static-only manifest has to boot in the standalone archive, or the artifact
/// serves nothing at all.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod no_python_capability {
    use super::*;

    /// A static-only manifest: one `static` mount, no app.
    const STATIC_ONLY: &str = "\
static \"/\" {
    root \"site\"
}
";

    /// One route that declares no executor of its own: a WSGI view.
    const WITH_A_VIEW: &str = "\
view web regex '.*' {}
";

    /// One PHP route and no app: its own executor (`ADR-0010` §2), so this
    /// build can serve it without an interpreter anywhere in the image.
    const PHP_ONLY: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

    /// The page's shape: files from disk, everything else to the front
    /// controller. Still no interpreter.
    const STATIC_AND_PHP: &str = "\
entrypoint php \"public/index.php\"

static \"/\" {
    root \"public\"
}

view app regex '.*' {}
";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        manifest
    }

    #[test]
    fn a_static_only_boot_is_allowed() {
        assert_eq!(
            no_python_support(&Config::default(), &manifest(STATIC_ONLY), false),
            None
        );
    }

    #[test]
    fn an_app_is_refused_and_names_the_package_to_install() {
        let config = Config {
            app: Some("web.wsgi:app".to_string()),
            ..Config::default()
        };
        let message = no_python_support(&config, &manifest(WITH_A_VIEW), false).unwrap();
        assert!(message.contains("BLASTIT_APP (web.wsgi:app)"), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "{message}"
        );
    }

    #[test]
    fn a_view_no_mount_or_entrypoint_reaches_is_refused_by_name() {
        let message = no_python_support(&Config::default(), &manifest(WITH_A_VIEW), false).unwrap();
        assert!(message.contains("view \"web\""), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "{message}"
        );
        assert!(message.contains("`static` mount"), "{message}");
        assert!(message.contains("`entrypoint php`"), "{message}");
    }

    #[test]
    fn learn_needs_the_gate_and_says_so() {
        let message = no_python_support(&Config::default(), &manifest(STATIC_ONLY), true).unwrap();
        assert!(message.starts_with("dev --learn"), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "{message}"
        );
    }

    /// The one combination the gate deliberately leaves alone: a
    /// `BLASTIT_APP` on an all-static tree keeps `static_app_refusal`'s
    /// sentence in both builds, because installing the wheel would not make
    /// that app reachable either.
    #[test]
    fn an_app_on_a_static_tree_keeps_the_rule_that_already_explains_it() {
        let config = Config {
            app: Some("web.wsgi:app".to_string()),
            ..Config::default()
        };
        assert_eq!(
            no_python_support(&config, &manifest(STATIC_ONLY), false),
            None
        );
        assert!(
            static_app_refusal(&config, &manifest(STATIC_ONLY))
                .unwrap()
                .starts_with("BLASTIT_APP: the file serves static mounts and no views"),
        );
    }

    /// A PHP route runs in a `view` worker with no interpreter, so **this**
    /// gate and the python-less build must say nothing about installing
    /// Python (`MB.3.5.4`). The boot's own answer for such a route is
    /// [`php_boot_gate`]'s — a different gate, with a different sentence.
    ///
    /// The denied twin in the same test is the manifest that *is* WSGI-shaped:
    /// a route with no `[route.static]` and no `entrypoint php` keeps the gate's
    /// sentence, so the early return cannot have swallowed the whole rule.
    #[test]
    fn a_php_only_manifest_is_not_refused_by_the_capability_gate() {
        assert_eq!(
            no_python_support(&Config::default(), &manifest(PHP_ONLY), false),
            None
        );
        assert_eq!(
            no_python_support(&Config::default(), &manifest(STATIC_AND_PHP), false),
            None
        );

        let message = no_python_support(&Config::default(), &manifest(WITH_A_VIEW), false).unwrap();
        assert!(message.contains("view \"web\""), "{message}");
        assert!(
            message.contains("install the Python package: uv add blastit"),
            "a WSGI view keeps the sentence that names the artifact: {message}"
        );
    }
}

/// `ADR-0010` §3/§4.1: the `entrypoint php` discovery gate and its two exact
/// sentences.
///
/// Every rule here has a denied case and an accepted twin: a host with no
/// `libphp` at all, one whose object is ZTS or not an ELF at all, one whose
/// version no shim matches — beside the manifest that declares no PHP route and
/// must be left alone, and the versioned NTS object with its shim that resolves.
/// (`workers` is not among them: `ADR-0012` §12.3 removed the keyword, so a
/// `Blastfile` can never carry a cap above a budget.)
///
/// The library fixtures are written out byte by byte (an ELF64 header plus a
/// `.dynsym` and its `.dynstr`), so the inline suite needs no C toolchain and
/// every case chooses exactly the exported names it needs. The `cc`-built
/// constructor-canary stubs — proving zero stage-0 `dlopen` from the outside —
/// belong to the protected suite (`MB.3.13.4`).
#[cfg(all(test, feature = "php"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod php_boot_gate {
    use std::path::PathBuf;

    use super::*;
    // The ELF facts the fixtures below write out byte by byte: the walker's own
    // constants, imported rather than respelled, so a fixture cannot drift from
    // the table it feeds (`MB.36.5`).
    use blastit_view_php::discovery::{
        EM_AARCH64, EM_X86_64, SHT_DYNSYM, SHT_STRTAB, decode_aarch64, decode_x86_64, library_dirs,
        php_refusal_message, target_tuple,
    };

    /// The page's shape: files from disk, the rest to the front controller.
    const STATIC_AND_PHP: &str = "\
entrypoint php \"public/index.php\"

static \"/\" {
    root \"public\"
}

view app regex '.*' {}
";

    /// Two PHP routes: the gate names the **first** one, so the sentence is
    /// actionable and stable.
    const TWO_PHP_ROUTES: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}

view api regex '/api/.*' {}
";

    /// The ADR-0010 §4.1 name, pinned to literals rather than to this module's
    /// own helpers: `target_arch`/`target_libc` are the second spelling of the
    /// tuple (see their docs), so this test is what keeps the boot's name and
    /// `Minor::shim_file()`'s name in step for every supported target. The
    /// php-sapi half of the same literals lives in
    /// `crates/blastit-php-sapi/tests/critical_shim_contract.rs`.
    #[test]
    fn the_shim_name_matches_the_adr_literal() {
        #[cfg(all(target_arch = "x86_64", not(target_env = "musl")))]
        const SHIM_84: &str = "libblastit_php84-x86_64-gnu.so";
        #[cfg(all(target_arch = "x86_64", target_env = "musl"))]
        const SHIM_84: &str = "libblastit_php84-x86_64-musl.so";
        #[cfg(all(target_arch = "aarch64", not(target_env = "musl")))]
        const SHIM_84: &str = "libblastit_php84-aarch64-gnu.so";
        #[cfg(all(target_arch = "aarch64", target_env = "musl"))]
        const SHIM_84: &str = "libblastit_php84-aarch64-musl.so";

        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            assert_eq!(shim_name(PhpVersion::new(8, 4)), SHIM_84);
            // Only the infix moves between minors: the tuple is the build's.
            assert_eq!(
                shim_name(PhpVersion::new(8, 3)),
                SHIM_84.replace("php84-", "php83-")
            );
            assert_eq!(
                shim_name(PhpVersion::new(8, 5)),
                SHIM_84.replace("php84-", "php85-")
            );
            // A minor with no shim ABI keeps the same shape, so the refusal can
            // still name the file the host would need.
            assert_eq!(
                shim_name(PhpVersion::new(8, 6)),
                SHIM_84.replace("php84-", "php86-")
            );
        }
    }

    /// A PHP route with no cap at all: the deployment's budget is the budget.
    ///
    /// `Blastfile` has no `workers` keyword at all (`ADR-0012` §12.3: the
    /// scheduler owns parallelism), so this is the only PHP shape a file can
    /// declare, and the budget rule has no over-cap input to refuse.
    const PHP_ONLY: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

    const STATIC_ONLY: &str = "\
static \"/\" {
    root \"site\"
}
";

    const WITH_A_VIEW: &str = "\
view web regex '.*' {}
";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        manifest
    }

    /// A directory under the test temp dir, removed on drop.
    struct Scratch {
        dir: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("blastit-php-gate-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        /// Writes `contents` as `name` and returns the path.
        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.dir.join(name);
            std::fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// The two provenance callbacks a deployment boot supplies, over
    /// `crate::host` — literally the functions [`php_boot_gate`] hands the
    /// gate, so these cases measure the gate and not a stand-in.
    ///
    /// Both are needed, and both are real: the object decision is ownership +
    /// file capabilities + the zero-writable `DT_RPATH`/`DT_RUNPATH` scan, the
    /// tree decision is ownership only (design §4.4 D5). A no-op closure here
    /// would silently invert every deployment case below.
    /// One provenance callback as `Deployment` takes it: the object decision
    /// receives the claim's noun as well as its path, and the tree decision
    /// receives only the path.
    type ObjectCheck = Box<dyn Fn(&str, &Path) -> Result<(), String>>;
    type TreeCheck = Box<dyn Fn(&Path) -> Result<(), String>>;

    struct HostChecks {
        init_uid: u32,
        objects: ObjectCheck,
        trees: TreeCheck,
    }

    impl HostChecks {
        fn new(init_uid: u32) -> Self {
            Self {
                init_uid,
                objects: Box::new(move |what, path| object_is_protected(init_uid, what, path)),
                trees: Box::new(move |path| tree_is_protected(init_uid, path)),
            }
        }

        /// The `Deployment` a deployment boot would build for this directory.
        fn deployment(&self, manifest_dir: &Path) -> Deployment<'_> {
            Deployment {
                init_uid: self.init_uid,
                manifest_dir: manifest_dir.to_path_buf(),
                is_protected_object: self.objects.as_ref(),
                is_protected_tree: self.trees.as_ref(),
            }
        }
    }

    /// A little-endian ELF shared object whose `.dynsym` exports `names` plus
    /// the `php_version` symbol the worker needs (`MB.3.59`). The symbol is a
    /// function with **no body**, so the version comes from the file name — the
    /// fast path — exactly as a distro soname does.
    ///
    /// Layout: the 64-byte ELF header, the dynamic symbol table (a null symbol
    /// then one per name), its string table, then the null, `SHT_DYNSYM`,
    /// `SHT_STRTAB` section headers. Nothing is loaded: the gate reads these
    /// bytes the way it reads a distro `libphp`.
    fn shared_object(names: &[&str]) -> Vec<u8> {
        shared_object_with(names, None, true)
    }

    /// The same object, with a `php_version` body that points at `version` in a
    /// read-only section — the static fact `MB.3.59` reads when the path states
    /// no minor.
    fn shared_object_with_version(names: &[&str], version: &str) -> Vec<u8> {
        shared_object_with(names, Some(version), true)
    }

    /// An NTS object with **no** `php_version` in its `.dynsym` at all.
    fn shared_object_without_php_version(names: &[&str]) -> Vec<u8> {
        shared_object_with(names, None, false)
    }

    /// How the fixture types the `php_version` symbol (`MB.3.61`).
    ///
    /// Presence is read from the **name**, so every shape below is an object
    /// that exports `php_version`; only the body fact differs.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PhpVersionSymbol {
        /// `STT_FUNC` with the body's true size — what a distro build produces.
        Function,
        /// `STT_NOTYPE`, the body bytes still in `.text`: an odd but valid
        /// object that does export the symbol.
        NotType,
        /// `STT_FUNC` with `st_size == 0`: the body is there, its extent is not
        /// stated.
        ZeroSize,
    }

    fn shared_object_with(
        names: &[&str],
        version: Option<&str>,
        exports_php_version: bool,
    ) -> Vec<u8> {
        shared_object_in(
            names,
            version,
            exports_php_version,
            PhpVersionSymbol::Function,
        )
    }

    /// The same object with `php_version` typed/shaped by `symbol` (`MB.3.61`).
    fn shared_object_in(
        names: &[&str],
        version: Option<&str>,
        exports_php_version: bool,
        symbol: PhpVersionSymbol,
    ) -> Vec<u8> {
        let machine: u16 = if cfg!(target_arch = "aarch64") {
            EM_AARCH64
        } else {
            EM_X86_64
        };
        let mut exports: Vec<&str> = names.to_vec();
        if exports_php_version && !exports.contains(&"php_version") {
            exports.push("php_version");
        }
        let mut strtab = vec![0u8];
        let mut offsets = Vec::new();
        for name in &exports {
            offsets.push(u32::try_from(strtab.len()).unwrap());
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
        }
        let dynsym_off = 64usize;
        let dynsym_len = 24 * (1 + exports.len());
        let strtab_off = dynsym_off + dynsym_len;
        let text_off = (strtab_off + strtab.len() + 7) & !7;
        let code_len = if version.is_some() {
            if machine == EM_AARCH64 { 12 } else { 8 }
        } else {
            0
        };
        let rodata_off = (text_off + code_len + 7) & !7;
        let string: Vec<u8> = version
            .map(|version| {
                let mut bytes = version.as_bytes().to_vec();
                bytes.push(0);
                bytes
            })
            .unwrap_or_default();
        let shoff = (rodata_off + string.len() + 7) & !7;
        let mut file = vec![0u8; shoff + 5 * 64];
        file[..4].copy_from_slice(b"\x7fELF");
        file[4] = 2; // ELFCLASS64
        file[5] = 1; // ELFDATA2LSB
        file[6] = 1; // EV_CURRENT
        file[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        file[18..20].copy_from_slice(&machine.to_le_bytes());
        file[0x28..0x30].copy_from_slice(&u64::try_from(shoff).unwrap().to_le_bytes());
        file[0x34..0x36].copy_from_slice(&64u16.to_le_bytes()); // e_ehsize
        file[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
        file[0x3c..0x3e].copy_from_slice(&5u16.to_le_bytes()); // e_shnum

        let mut dynsym = vec![0u8; 24];
        for (index, offset) in offsets.iter().enumerate() {
            let mut entry = vec![0u8; 24];
            entry[..4].copy_from_slice(&offset.to_le_bytes());
            entry[4] = if exports[index] == "php_version" {
                // `STT_NOTYPE` (0) or `STB_GLOBAL | STT_FUNC` (0x12): both
                // export the *name*, which is the fact the gate reads.
                match symbol {
                    PhpVersionSymbol::NotType => 0x10,
                    PhpVersionSymbol::Function | PhpVersionSymbol::ZeroSize => 0x12,
                }
            } else {
                0x11 // STB_GLOBAL | STT_OBJECT
            };
            if exports[index] == "php_version" && version.is_some() {
                entry[6..8].copy_from_slice(&3u16.to_le_bytes()); // .text
                entry[8..16].copy_from_slice(&u64::try_from(text_off).unwrap().to_le_bytes());
                let stated = match symbol {
                    PhpVersionSymbol::ZeroSize => 0,
                    PhpVersionSymbol::Function | PhpVersionSymbol::NotType => code_len,
                };
                entry[16..24].copy_from_slice(&u64::try_from(stated).unwrap().to_le_bytes());
            }
            dynsym.extend_from_slice(&entry);
        }
        file[dynsym_off..dynsym_off + dynsym.len()].copy_from_slice(&dynsym);
        file[strtab_off..strtab_off + strtab.len()].copy_from_slice(&strtab);
        if version.is_some() {
            let code = php_version_body(machine, text_off, rodata_off);
            assert_eq!(code.len(), code_len, "the fixture's body is one size");
            file[text_off..text_off + code.len()].copy_from_slice(&code);
            file[rodata_off..rodata_off + string.len()].copy_from_slice(&string);
        }

        let header = |index: usize| shoff + index * 64;
        // The null header is already zeroed.
        let dynsym_header = header(1);
        file[dynsym_header + 4..dynsym_header + 8].copy_from_slice(&SHT_DYNSYM.to_le_bytes());
        file[dynsym_header + 0x18..dynsym_header + 0x20]
            .copy_from_slice(&u64::try_from(dynsym_off).unwrap().to_le_bytes());
        file[dynsym_header + 0x20..dynsym_header + 0x28]
            .copy_from_slice(&u64::try_from(dynsym.len()).unwrap().to_le_bytes());
        file[dynsym_header + 0x28..dynsym_header + 0x2c].copy_from_slice(&2u32.to_le_bytes());
        file[dynsym_header + 0x38..dynsym_header + 0x40].copy_from_slice(&24u64.to_le_bytes());
        let strtab_header = header(2);
        file[strtab_header + 4..strtab_header + 8].copy_from_slice(&SHT_STRTAB.to_le_bytes());
        file[strtab_header + 0x18..strtab_header + 0x20]
            .copy_from_slice(&u64::try_from(strtab_off).unwrap().to_le_bytes());
        file[strtab_header + 0x20..strtab_header + 0x28]
            .copy_from_slice(&u64::try_from(strtab.len()).unwrap().to_le_bytes());
        let text_header = header(3);
        file[text_header + 4..text_header + 8].copy_from_slice(&1u32.to_le_bytes()); // PROGBITS
        file[text_header + 0x8..text_header + 0x10].copy_from_slice(&6u64.to_le_bytes()); // ALLOC|EXEC
        file[text_header + 0x10..text_header + 0x18]
            .copy_from_slice(&u64::try_from(text_off).unwrap().to_le_bytes());
        file[text_header + 0x18..text_header + 0x20]
            .copy_from_slice(&u64::try_from(text_off).unwrap().to_le_bytes());
        file[text_header + 0x20..text_header + 0x28]
            .copy_from_slice(&u64::try_from(code_len).unwrap().to_le_bytes());
        let rodata_header = header(4);
        file[rodata_header + 4..rodata_header + 8].copy_from_slice(&1u32.to_le_bytes());
        file[rodata_header + 0x8..rodata_header + 0x10].copy_from_slice(&2u64.to_le_bytes()); // ALLOC
        file[rodata_header + 0x10..rodata_header + 0x18]
            .copy_from_slice(&u64::try_from(rodata_off).unwrap().to_le_bytes());
        file[rodata_header + 0x18..rodata_header + 0x20]
            .copy_from_slice(&u64::try_from(rodata_off).unwrap().to_le_bytes());
        file[rodata_header + 0x20..rodata_header + 0x28]
            .copy_from_slice(&u64::try_from(string.len()).unwrap().to_le_bytes());
        file
    }

    /// `return "PHP_VERSION";` as the host's architecture compiles it.
    fn php_version_body(machine: u16, text: usize, string: usize) -> Vec<u8> {
        if machine == EM_AARCH64 {
            let pages = (string >> 12) as i64 - (text >> 12) as i64;
            let imm = pages & 0x1f_ffff;
            let adrp =
                0x9000_0000u32 | (((imm >> 2) as u32 & 0x7_ffff) << 5) | ((imm as u32 & 0x3) << 29);
            let add = 0x9100_0000u32 | (((string & 0xfff) as u32) << 10);
            let mut code = Vec::new();
            code.extend_from_slice(&adrp.to_le_bytes());
            code.extend_from_slice(&add.to_le_bytes());
            code.extend_from_slice(&0xd65f_03c0u32.to_le_bytes()); // ret
            code
        } else {
            // `lea rax,[rip+disp]` lands on the byte *after* the instruction.
            let next = text + 7;
            let disp = i32::try_from(string as i64 - next as i64).unwrap();
            let mut code = vec![0x48, 0x8d, 0x05];
            code.extend_from_slice(&disp.to_le_bytes());
            code.push(0xc3); // ret
            code
        }
    }

    /// A search over one explicit NTS object and the shims in `shim_dir`.
    fn search_for(libphp: &Path, shim_dir: &Path) -> PhpSearch {
        PhpSearch {
            libphp: Some(libphp.to_path_buf()),
            shim_dirs: vec![shim_dir.to_path_buf()],
            lib_dirs: Vec::new(),
        }
    }

    /// An 8.4 NTS object and its correctly named shim, both under `scratch`.
    fn resolved_84(scratch: &Scratch) -> (PathBuf, PhpSearch) {
        let libphp = scratch.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        let shims = scratch.dir.join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::write(
            shims.join(shim_name(PhpVersion::new(8, 4))),
            b"the shim is not loaded here",
        )
        .unwrap();
        (
            libphp.clone(),
            PhpSearch {
                libphp: Some(libphp),
                shim_dirs: vec![shims],
                lib_dirs: Vec::new(),
            },
        )
    }

    /// A `BLASTIT_LIBPHP` that is a symlink — the distro's
    /// `libphp8.so -> libphp8.4.so` shape — is recorded as its **target**: stage
    /// 1 re-opens each object `O_NOFOLLOW`-strictly, so recording the link
    /// itself would kill the worker with `ELOOP` after the banner instead of
    /// loading the library the gate inspected.
    #[test]
    fn a_symlinked_libphp_and_shim_are_recorded_as_their_targets() {
        let scratch = Scratch::new("symlink");
        let real = scratch.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        let link = scratch.dir.join("libphp8.so");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let shims = Scratch::new("symlink-shims");
        let real_shim = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded here");
        let linked_dir = Scratch::new("symlink-shim-dir");
        let linked_shim = linked_dir.dir.join(shim_name(PhpVersion::new(8, 4)));
        std::os::unix::fs::symlink(&real_shim, &linked_shim).unwrap();

        let search = search_for(&link, &linked_dir.dir);
        let runtime = discover_php(&search).expect("a link to an NTS 8.4 object and its shim");
        assert_eq!(runtime.libphp, real, "the record is the resolved library");
        assert_eq!(runtime.shim, real_shim, "and the resolved shim");
        for path in [&runtime.libphp, &runtime.shim] {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            assert!(
                !metadata.file_type().is_symlink(),
                "{} is still a link, so stage 1's O_NOFOLLOW open would fail",
                path.display()
            );
        }
        assert!(
            matches!(php_boot(&manifest(PHP_ONLY), &search), PhpBoot::Proceed(_)),
            "a legitimately symlinked distro library keeps booting"
        );
    }

    /// The link that goes nowhere is a §4.1 refusal, not an `errno` in a
    /// worker: the route, its `entry`, the path, the package and
    /// `BLASTIT_LIBPHP`.
    #[test]
    fn a_dangling_symlink_is_refused_with_the_package_sentence() {
        let scratch = Scratch::new("dangling");
        let link = scratch.dir.join("libphp8.4.so");
        std::os::unix::fs::symlink(scratch.dir.join("not-installed.so"), &link).unwrap();
        let shims = Scratch::new("dangling-shims");

        let message = php_boot(&manifest(PHP_ONLY), &search_for(&link, &shims.dir))
            .refusal()
            .expect("a dangling link cannot be embedded")
            .to_string();
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("entrypoint php \"public/index.php\""),
            "{message}"
        );
        assert!(message.contains(&link.display().to_string()), "{message}");
        assert!(
            message.contains("install the PHP embed package"),
            "{message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(
            !message.contains("Too many levels of symbolic links"),
            "the refusal is the sentence, not a raw errno: {message}"
        );
    }

    /// A host with nothing to embed refuses **only** when a route asks for PHP.
    ///
    /// The refusal is `ADR-0010` §4.1's missing-library sentence: the route and
    /// its `entry`, the names that were searched and where, the three distro
    /// packages and `BLASTIT_LIBPHP`. A static tree and a WSGI-shaped manifest
    /// boot as before — the same `stage_0` must not refuse a site that never
    /// asked for PHP.
    #[test]
    fn a_host_with_no_libphp_refuses_only_when_a_php_route_exists() {
        let empty = Scratch::new("empty");
        let search = PhpSearch {
            libphp: None,
            shim_dirs: vec![empty.dir.clone()],
            lib_dirs: vec![empty.dir.clone()],
        };
        for source in [STATIC_ONLY, WITH_A_VIEW] {
            assert!(
                matches!(php_boot(&manifest(source), &search), PhpBoot::Absent),
                "{source}"
            );
        }
        let message = php_boot(&manifest(STATIC_AND_PHP), &search)
            .refusal()
            .expect("a PHP route")
            .to_string();
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("entrypoint php \"public/index.php\""),
            "{message}"
        );
        assert!(message.contains("no usable NTS libphp"), "{message}");
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        for package in [
            "apt install libphp-embed",
            "apk add php85-embed",
            "dnf install php-embedded",
        ] {
            assert!(
                message.contains(package),
                "the refusal names {package}: {message}"
            );
        }
        assert!(
            message.contains(&empty.dir.display().to_string()),
            "the refusal names the directories that were searched: {message}"
        );
        assert!(
            !message.contains("MB.3.13.3"),
            "a host with no library gets the package sentence, never the wiring one: {message}"
        );

        // The first PHP route is the one named, never the second.
        let two = php_boot(&manifest(TWO_PHP_ROUTES), &search)
            .refusal()
            .expect("a PHP route")
            .to_string();
        assert!(two.contains("route \"app\""), "{two}");
        assert!(!two.contains("api/index.php"), "{two}");

        // The `dev` twin is the same refusal: `ADR-0010` §1 gives PHP one
        // backend, so a developer host with nothing installed gets the package
        // sentence too, not a boot that serves the route another way.
        let dev = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("a PHP-free dev host is refused")
            .to_string();
        assert!(dev.contains("no usable NTS libphp"), "{dev}");
        assert!(
            dev.contains("apt install libphp-embed"),
            "dev names the package: {dev}"
        );
    }

    /// A ZTS build is refused with the tuple and the shim it would need — and
    /// the same object with `tsrm_startup` gone resolves.
    #[test]
    fn a_zts_libphp_is_refused_naming_the_tuple_and_the_shim() {
        let scratch = Scratch::new("zts");
        let zts = scratch.file(
            "libphp8.4.so",
            &shared_object(&["executor_globals", "tsrm_startup"]),
        );
        let shims = Scratch::new("zts-shims");
        let shim = shims.file(
            &shim_name(PhpVersion::new(8, 4)),
            b"never loaded in stage 0",
        );
        let search = search_for(&zts, &shims.dir);
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("a ZTS host is refused")
            .to_string();
        assert!(message.contains("ZTS (thread-safe)"), "{message}");
        assert!(
            message.contains(&format!("(PHP 8.4, {})", target_tuple())),
            "the tuple: {message}"
        );
        assert!(
            message.contains(&format!("libblastit_php84-{}.so", target_tuple())),
            "the shim that tuple needs: {message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");
        assert!(shim.is_file(), "the shim exists: ZTS is the refusal");

        // The accepted twin: the same file name without `tsrm_startup`.
        let nts = scratch.file("libphp-8.4.so", &shared_object(&["executor_globals"]));
        let search = search_for(&nts, &shims.dir);
        let runtime = discover_php(&search).expect("an NTS 8.4 object with its shim");
        assert_eq!(runtime.version, PhpVersion::new(8, 4));
        assert_eq!(runtime.libphp, nts);
        assert_eq!(runtime.shim, shim);
        assert!(runtime.ino > 0, "the inode is recorded for stage 1");
        assert!(
            matches!(php_boot(&manifest(PHP_ONLY), &search), PhpBoot::Proceed(_)),
            "the host resolves, so the boot proceeds to stage 1's binding"
        );
    }

    /// An object that is not an ELF — a text file, a directory, a path that is
    /// not there — is refused, never guessed at.
    #[test]
    fn a_libphp_that_is_not_an_elf_object_is_refused() {
        let scratch = Scratch::new("notelf");
        let shims = Scratch::new("notelf-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let cases = [
            (
                scratch.file("libphp8.4.so", b"this is not an ELF shared object"),
                "not an ELF shared object",
            ),
            (scratch.dir.join("libphp-8.4.so"), "cannot be opened"),
        ];
        for (path, expected) in cases {
            let message = php_boot(&manifest(PHP_ONLY), &search_for(&path, &shims.dir))
                .refusal()
                .expect("an unusable path is refused")
                .to_string();
            assert!(message.contains(expected), "{path:?}: {message}");
            assert!(
                message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
                "{message}"
            );
        }
        // A directory named like the library.
        let directory = scratch.dir.join("libphp8.3.so");
        std::fs::create_dir_all(&directory).unwrap();
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&directory, &shims.dir))
            .refusal()
            .expect("a directory is not a library")
            .to_string();
        assert!(message.contains("not a regular file"), "{message}");
    }

    /// The version comes from the file name (in either spelling), and a name
    /// that carries none is refused rather than guessed.
    #[test]
    fn the_version_comes_from_the_file_name() {
        let scratch = Scratch::new("version");
        let shims = Scratch::new("version-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 3)), b"never loaded");

        let unversioned = scratch.file("libphp.so", &shared_object(&["executor_globals"]));
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&unversioned, &shims.dir))
            .refusal()
            .expect("an unversioned name with no readable version cannot choose a shim")
            .to_string();
        assert!(
            message.contains("exports `php_version`"),
            "the `.dynsym` fact is named: {message}"
        );
        assert!(
            message.contains("could not be read from its sections"),
            "the static fact is named: {message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(
            !message.contains("names no PHP major.minor"),
            "the old file-name-only sentence is gone: {message}"
        );

        // The accepted twin, in the distro's hyphenated spelling.
        let hyphenated = scratch.file("libphp-8.3.so", &shared_object(&["executor_globals"]));
        let runtime =
            discover_php(&search_for(&hyphenated, &shims.dir)).expect("libphp-8.3.so and its shim");
        assert_eq!(runtime.version, PhpVersion::new(8, 3));
        assert_eq!(
            runtime.shim.file_name().unwrap().to_str().unwrap(),
            format!("libblastit_php83-{}.so", target_tuple())
        );
    }

    /// `MB.3.59`: Alpine's own package path states its minor in the **packed**
    /// file name (`libphp83.so`) or in the per-minor parent directory
    /// (`/usr/lib/php84/libphp.so`), and neither is a versioned name.
    #[test]
    fn the_packed_name_and_the_alpine_directory_state_the_minor() {
        let scratch = Scratch::new("alpine-spellings");
        let shims = Scratch::new("alpine-spellings-shims");
        for minor in [3u8, 4] {
            let _ = shims.file(&shim_name(PhpVersion::new(8, minor)), b"never loaded");
        }

        // `libphp83.so`: the name states 8.3 with no dot.
        let packed = scratch.file("libphp83.so", &shared_object(&["executor_globals"]));
        let runtime = discover_php(&search_for(&packed, &shims.dir)).expect("libphp83.so");
        assert_eq!(runtime.version, PhpVersion::new(8, 3));

        // `/usr/lib/php84/libphp.so`: the name states nothing, the directory
        // states 8.4.
        std::fs::create_dir_all(scratch.dir.join("php84")).unwrap();
        let directory = scratch.file("php84/libphp.so", &shared_object(&["executor_globals"]));
        let runtime = discover_php(&search_for(&directory, &shims.dir)).expect("php84/libphp.so");
        assert_eq!(runtime.version, PhpVersion::new(8, 4));
        assert_eq!(
            runtime.shim.file_name().unwrap().to_str().unwrap(),
            format!("libblastit_php84-{}.so", target_tuple())
        );
    }

    /// `MB.3.59`: the object's own bytes name the minor when the path states
    /// none — the `php_version` body's string, read statically.
    #[test]
    fn the_object_bytes_name_the_minor_when_the_path_does_not() {
        let scratch = Scratch::new("object-version");
        let shims = Scratch::new("object-version-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let unversioned = scratch.file(
            "libphp.so",
            &shared_object_with_version(&["executor_globals"], "8.4.27"),
        );
        let runtime =
            discover_php(&search_for(&unversioned, &shims.dir)).expect("the object names 8.4");
        assert_eq!(runtime.version, PhpVersion::new(8, 4));
        assert_eq!(
            runtime.shim.file_name().unwrap().to_str().unwrap(),
            format!("libblastit_php84-{}.so", target_tuple())
        );
    }

    /// `MB.3.59`: two statements that disagree are a refusal naming both, never
    /// a preference — the shim is chosen by this number.
    #[test]
    fn two_disagreeing_minors_are_refused_naming_both() {
        let scratch = Scratch::new("version-conflict");
        let shims = Scratch::new("version-conflict-shims");
        for minor in [3u8, 4] {
            let _ = shims.file(&shim_name(PhpVersion::new(8, minor)), b"never loaded");
        }
        // The file name states 8.3, the directory states 8.4.
        std::fs::create_dir_all(scratch.dir.join("php84")).unwrap();
        let conflicted = scratch.file("php84/libphp83.so", &shared_object(&["executor_globals"]));
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&conflicted, &shims.dir))
            .refusal()
            .expect("two minors cannot both choose a shim")
            .to_string();
        assert!(message.contains("more than one PHP minor"), "{message}");
        assert!(message.contains("PHP 8.3"), "{message}");
        assert!(message.contains("PHP 8.4"), "{message}");
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");
    }

    /// `MB.3.59`: an NTS object whose `.dynsym` carries no `php_version` is
    /// refused naming the symbol, and a file name that **does** carry a
    /// supported minor does not change the sentence.
    #[test]
    fn a_libphp_without_php_version_is_refused_naming_the_symbol() {
        let scratch = Scratch::new("no-php-version");
        let shims = Scratch::new("no-php-version-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let versioned_name = scratch.file(
            "libphp8.4.so",
            &shared_object_without_php_version(&["executor_globals"]),
        );
        let message = php_boot(
            &manifest(PHP_ONLY),
            &search_for(&versioned_name, &shims.dir),
        )
        .refusal()
        .expect("a libphp without php_version cannot be embedded")
        .to_string();
        assert!(
            message.contains("exports no `php_version`"),
            "the missing symbol is named: {message}"
        );
        assert!(
            message.contains("its file name states PHP 8.4"),
            "the name is named, but does not rescue it: {message}"
        );
        assert!(
            !message.contains("names no PHP major.minor"),
            "a versioned name must not change the sentence: {message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");
    }

    /// An installed minor this build ships no shim for is refused with the
    /// support sentence, not with the `.dynsym` story of a symbol it may or may
    /// not export — and the same holds when `BLASTIT_LIBPHP` names it.
    #[test]
    fn an_unsupported_minor_is_refused_with_the_support_sentence() {
        let scratch = Scratch::new("unsupported-minor");
        let shims = Scratch::new("unsupported-minor-shims");
        let old = scratch.file(
            "libphp8.2.so",
            &shared_object_without_php_version(&["executor_globals"]),
        );
        let sentence = "PHP 8.2 is not supported — Blastit supports PHP 8.3, 8.4 and 8.5";

        // `BLASTIT_LIBPHP` names it: the object is opened, and the version its
        // own name states is refused before the symbol walk.
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&old, &shims.dir))
            .refusal()
            .expect("an unsupported minor cannot be embedded")
            .to_string();
        assert!(message.ends_with(sentence), "{message}");
        assert!(
            !message.contains("exports no `php_version`"),
            "the support sentence replaces the symbol story: {message}"
        );

        // No supported spelling exists, and the only installed library is the
        // unsupported one: the refusal scan finds it and says the same thing.
        let only_old = Scratch::new("only-unsupported");
        let _ = only_old.file(
            "libphp8.2.so",
            &shared_object_without_php_version(&["executor_globals"]),
        );
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        search.lib_dirs = vec![only_old.dir.clone()];
        search.shim_dirs = vec![shims.dir.clone()];
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("an 8.2-only host is refused")
            .to_string();
        assert!(message.ends_with(sentence), "{message}");

        // The per-minor directory spelling states the same minor, so an
        // unversioned `libphp.so` in `/usr/lib/php82` is refused the same way.
        let per_minor = Scratch::new("only-unsupported-per-minor");
        std::fs::create_dir_all(per_minor.dir.join("php82")).unwrap();
        let _ = per_minor.file(
            "php82/libphp.so",
            &shared_object_without_php_version(&["executor_globals"]),
        );
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        search.lib_dirs = vec![per_minor.dir.join("php82")];
        search.shim_dirs = vec![shims.dir.clone()];
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("the per-minor directory states the minor")
            .to_string();
        assert!(message.ends_with(sentence), "{message}");
    }

    /// `MB.3.59`: the discovery directories include Alpine's per-minor embed
    /// directories, so an unset `BLASTIT_LIBPHP` can reach its own package.
    #[test]
    fn the_alpine_embed_directories_are_searched() {
        let dirs = library_dirs();
        for wanted in [
            "/usr/lib/php85",
            "/usr/lib/php84",
            "/usr/lib/php83",
            "/usr/local/lib/php85",
            "/usr/local/lib/php84",
            "/usr/local/lib/php83",
        ] {
            assert!(
                dirs.iter().any(|dir| dir == Path::new(wanted)),
                "{wanted} is missing from {dirs:?}"
            );
        }
        // Order is preference: the plain `/usr/lib` comes before a per-minor
        // directory, and `/usr/local` after both.
        let position = |wanted: &str| {
            dirs.iter()
                .position(|dir| dir == Path::new(wanted))
                .unwrap_or(usize::MAX)
        };
        assert!(position("/usr/lib") < position("/usr/lib/php85"));
        assert!(position("/usr/lib/php85") < position("/usr/local/lib"));
    }

    /// `MB.3.61`: a host carrying **two** embed packages must serve from the
    /// newer library, not from the first name in the list.
    ///
    /// The reproduced shape is Alpine's: `php83-embed` installs
    /// `/usr/lib/libphp83.so` (packed name) and `php85-embed` installs
    /// `/usr/lib/php85/libphp.so` (per-minor directory). With
    /// `BLASTIT_LIBPHP` unset a name-major walk found `libphp83.so` first and
    /// served 8.3, although the 8.5 library was right there.
    #[test]
    fn two_embed_packages_serve_from_the_newer_library() {
        let scratch = Scratch::new("two-packages");
        let shims = Scratch::new("two-packages-shims");
        for minor in [3u8, 5] {
            let _ = shims.file(&shim_name(PhpVersion::new(8, minor)), b"never loaded");
        }
        // The two packages' own paths, in the directories `library_dirs()`
        // names — the same layout an Alpine host gets.
        std::fs::create_dir_all(scratch.dir.join("php85")).unwrap();
        let old = scratch.file("libphp83.so", &shared_object(&["executor_globals"]));
        let new = scratch.file("php85/libphp.so", &shared_object(&["executor_globals"]));
        assert!(
            old.is_file() && new.is_file(),
            "both packages are installed"
        );

        // `BLASTIT_LIBPHP` unset: `PhpSearch::new(None, …)` is exactly the
        // library half of that, and the directories are the host's order.
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        assert!(
            search.libphp.is_none(),
            "an unset BLASTIT_LIBPHP is no override at all"
        );
        search.lib_dirs = vec![scratch.dir.clone(), scratch.dir.join("php85")];
        search.shim_dirs = vec![shims.dir.clone()];

        let boot = php_boot(&manifest(PHP_ONLY), &search);
        let PhpBoot::Proceed(runtime) = boot else {
            panic!("two packages, and the 8.5 library serves: {boot:?}");
        };
        assert_eq!(runtime.version, PhpVersion::new(8, 5), "the newer library");
        assert_eq!(
            runtime.libphp, new,
            "the served library is the 8.5 one, not the 8.3 name that came first"
        );

        // The single-package twin: the packed 8.3 name alone still serves.
        let only_old = Scratch::new("only-8.3");
        let old = only_old.file("libphp83.so", &shared_object(&["executor_globals"]));
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        search.lib_dirs = vec![only_old.dir.clone()];
        search.shim_dirs = vec![shims.dir.clone()];
        let runtime = discover_php(&search).expect("8.3 alone still serves");
        assert_eq!(runtime.version, PhpVersion::new(8, 3));
        assert_eq!(runtime.libphp, old);

        // And the 8.5 package alone still serves, from its own directory.
        let only_new = Scratch::new("only-8.5");
        std::fs::create_dir_all(only_new.dir.join("php85")).unwrap();
        let new = only_new.file("php85/libphp.so", &shared_object(&["executor_globals"]));
        let mut search = PhpSearch::new(None, None, PhpShimScope::Development);
        search.lib_dirs = vec![only_new.dir.join("php85")];
        search.shim_dirs = vec![shims.dir.clone()];
        let runtime = discover_php(&search).expect("8.5 alone still serves");
        assert_eq!(runtime.version, PhpVersion::new(8, 5));
        assert_eq!(runtime.libphp, new);
    }

    /// `MB.3.61`: `php_version` **presence** is the name in the `.dynsym`, so an
    /// odd object that exports it as `STT_NOTYPE` or with `st_size == 0` is not
    /// told it "exports no `php_version`" — a sentence about a symbol it has.
    /// The body is a separate fact: a versioned path still serves, and with no
    /// minor anywhere the refusal names the unreadable body (`NoVersion`).
    #[test]
    fn php_version_presence_is_the_name_not_the_body_shape() {
        let shims = Scratch::new("odd-symbols-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");

        for (shape, label) in [
            (PhpVersionSymbol::NotType, "STT_NOTYPE"),
            (PhpVersionSymbol::ZeroSize, "st_size-zero"),
        ] {
            let scratch = Scratch::new(&format!("odd-symbols-{label}"));
            // A versioned name: the symbol's presence is enough, and the minor
            // comes from the path because the body states nothing.
            let versioned = scratch.file(
                "libphp8.4.so",
                &shared_object_in(&["executor_globals"], None, true, shape),
            );
            let runtime = discover_php(&search_for(&versioned, &shims.dir))
                .unwrap_or_else(|refusal| panic!("{label}: refused as {refusal:?}"));
            assert_eq!(runtime.version, PhpVersion::new(8, 4), "{label}");

            // The unversioned twin: no body version, no minor in the path, so
            // the only true sentence is the one that names the body it cannot
            // read — never that the symbol is absent.
            let unversioned = scratch.file(
                "libphp.so",
                &shared_object_in(&["executor_globals"], None, true, shape),
            );
            let message = php_boot(&manifest(PHP_ONLY), &search_for(&unversioned, &shims.dir))
                .refusal()
                .unwrap_or_else(|| panic!("{label}: no version anywhere cannot choose a shim"))
                .to_string();
            assert!(
                !message.contains("exports no `php_version`"),
                "{label}: the object does export it, so that sentence is false: {message}"
            );
            assert!(
                message.contains("exports `php_version` in its `.dynsym`"),
                "{label}: the real cause is the unreadable body: {message}"
            );
            assert!(
                message.contains("could not be read from its sections"),
                "{label}: {message}"
            );

            // The genuinely missing symbol keeps its own sentence: the two
            // refusals stay distinguishable.
            let absent = scratch.file(
                "libphp8.4.so",
                &shared_object_without_php_version(&["executor_globals"]),
            );
            let message = php_boot(&manifest(PHP_ONLY), &search_for(&absent, &shims.dir))
                .refusal()
                .expect("no php_version name at all is refused")
                .to_string();
            assert!(
                message.contains("exports no `php_version`"),
                "{label}: the absence sentence still belongs to the absent symbol: {message}"
            );
        }
    }

    /// The two `php_version` bodies the resolver decodes, on both architectures
    /// — the aarch64 half cannot run on an x86_64 host, so it is pinned by
    /// bytes rather than left to an arm runner.
    #[test]
    fn the_version_body_is_decoded_on_both_architectures() {
        let disp = i32::try_from(0x2000i64 - 0x1007).unwrap();
        let mut x86 = vec![0x48, 0x8d, 0x05];
        x86.extend_from_slice(&disp.to_le_bytes());
        x86.push(0xc3);
        assert_eq!(decode_x86_64(&x86, 0x1000), Some(0x2000));
        // `endbr64` is skipped, not mistaken for the body: the displacement of
        // the body after it is relative to *its* end, not the function's start.
        let mut protected = vec![0xf3, 0x0f, 0x1e, 0xfa];
        let disp = i32::try_from(0x2000i64 - 0x100bi64).unwrap();
        protected.extend_from_slice(&[0x48, 0x8d, 0x05]);
        protected.extend_from_slice(&disp.to_le_bytes());
        protected.push(0xc3);
        assert_eq!(decode_x86_64(&protected, 0x1000), Some(0x2000));
        // Anything else is not this shape: a refusal, never a guess.
        assert_eq!(
            decode_x86_64(&[0xb8, 0x2b, 0x3a, 0x01, 0x00, 0xc3], 0x1000),
            None
        );

        // `adrp x0, page; add x0, x0, #0x345; ret` reaches 0x2000 + 0x345.
        let adrp = 0x9000_0000u32 | (1 << 29);
        let add = 0x9100_0000u32 | (0x345 << 10);
        let mut arm = Vec::new();
        arm.extend_from_slice(&adrp.to_le_bytes());
        arm.extend_from_slice(&add.to_le_bytes());
        arm.extend_from_slice(&0xd65f_03c0u32.to_le_bytes());
        assert_eq!(decode_aarch64(&arm, 0x1000), Some(0x2345));
        assert_eq!(decode_aarch64(&[0x1f, 0x20, 0x03, 0xd5], 0x1000), None);
    }

    /// Nothing matched to the tuple is a refusal that names the shim, the
    /// tuple, the directories and the package; a shim in a later directory is
    /// the accepted twin.
    #[test]
    fn the_shim_is_chosen_by_the_tuple_and_a_missing_one_names_it() {
        let scratch = Scratch::new("shim");
        let libphp = scratch.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        let first = Scratch::new("shim-first");
        let second = Scratch::new("shim-second");
        let search = PhpSearch {
            libphp: Some(libphp.clone()),
            shim_dirs: vec![first.dir.clone(), second.dir.clone()],
            lib_dirs: Vec::new(),
        };
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("no shim for the tuple")
            .to_string();
        assert!(
            message.contains(&format!("no shim libblastit_php84-{}.so", target_tuple())),
            "{message}"
        );
        assert!(
            message.contains(&format!("(PHP 8.4, {})", target_tuple())),
            "{message}"
        );
        assert!(
            message.contains(&first.dir.display().to_string())
                && message.contains(&second.dir.display().to_string()),
            "the refusal names every directory it looked in: {message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");

        // The accepted twin: the shim in the second directory is found.
        let shim = second.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let runtime = discover_php(&search).expect("the shim in the second directory");
        assert_eq!(runtime.shim, shim);

        // And the `dev` twin of the missing shim: `make php-sapi` is optional in
        // a development checkout, and `ADR-0010` §1 gives PHP one backend — so
        // `dev` refuses a missing shim exactly as a deployment does, naming the
        // tuple and the `make php-sapi` artifact that fixes it.
        let empty = Scratch::new("shim-dev-empty");
        let dev_search = PhpSearch {
            libphp: Some(libphp),
            shim_dirs: vec![empty.dir.clone()],
            lib_dirs: Vec::new(),
        };
        let dev = php_boot(&manifest(PHP_ONLY), &dev_search)
            .refusal()
            .expect("dev refuses a missing shim too")
            .to_string();
        assert!(
            dev.contains(&format!("libblastit_php84-{}.so", target_tuple())),
            "{dev}"
        );
    }

    /// `ADR-0010` §2's `workers` cap was downward only, and `ADR-0012` §12.3
    /// removed the keyword from the language: a `Blastfile` carries no per-view
    /// cap, so the deployment's budget is the only one there is and this gate
    /// has nothing to compare it against. The accepted case is the whole rule —
    /// the sibling that refused an over-cap `workers`, and the parameter that
    /// carried the budget, are deleted with the keyword.
    #[test]
    fn a_php_boot_with_no_workers_cap_passes_any_budget() {
        let scratch = Scratch::new("workers");
        let (_, resolved) = resolved_84(&scratch);
        assert!(
            matches!(
                php_boot(&manifest(PHP_ONLY), &resolved),
                PhpBoot::Proceed(_)
            ),
            "a view declares no cap, so the deployment's budget is the budget"
        );
    }

    /// The version-less refusal still carries the `ADR-0010` §4.1 tuple shape,
    /// with the version marked unknown; a name that states no `major.minor` is
    /// exactly that case.
    #[test]
    fn a_version_less_refusal_still_names_the_tuple_with_a_question_mark() {
        let scratch = Scratch::new("no-version");
        let shims = Scratch::new("no-version-shims");
        let _ = shims.file(&shim_name(PhpVersion::new(8, 4)), b"never loaded");
        let unversioned = scratch.file("libphp.so", b"this is not an ELF shared object");
        let message = php_boot(&manifest(PHP_ONLY), &search_for(&unversioned, &shims.dir))
            .refusal()
            .expect("not an ELF, and its name states no version")
            .to_string();
        assert!(
            message.contains(&format!("(PHP ?, {})", target_tuple())),
            "the tuple shape survives an unknown version: {message}"
        );
        assert!(message.contains("apt install libphp-embed"), "{message}");
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );
    }

    /// `Config::validate` refuses a relative `BLASTIT_LIBPHP` before a boot
    /// reaches the gate, and the gate refuses it too: `php_boot` is a pure
    /// function and the search is injectable, so the defence is tested where it
    /// lives rather than assumed.
    #[test]
    fn a_relative_libphp_is_refused_by_the_gate_too() {
        let search = PhpSearch {
            libphp: Some(PathBuf::from("libphp.so")),
            shim_dirs: Vec::new(),
            lib_dirs: Vec::new(),
        };
        let message = php_boot(&manifest(PHP_ONLY), &search)
            .refusal()
            .expect("a relative override cannot be the library the boot inspected")
            .to_string();
        assert!(message.contains("BLASTIT_LIBPHP"), "{message}");
        assert!(message.contains("not an absolute path"), "{message}");
        assert!(message.contains("apt install libphp-embed"), "{message}");
    }

    /// The native twin: on this host the real distro library is inspected
    /// statically and resolves, and on a host without one the same gate names
    /// the package. Both branches are an assertion — neither is a skip.
    #[test]
    fn the_host_library_resolves_or_the_refusal_names_the_package() {
        let shims = Scratch::new("host-shims");
        for minor in [2u8, 3, 4] {
            let _ = shims.file(
                &shim_name(PhpVersion::new(8, minor)),
                b"never loaded at boot",
            );
        }
        let search = PhpSearch {
            libphp: None,
            shim_dirs: vec![shims.dir.clone()],
            lib_dirs: library_dirs(),
        };
        match discover_php(&search) {
            Ok(runtime) => {
                assert!(runtime.libphp.is_file(), "{:?}", runtime.libphp);
                assert!(runtime.dev > 0 && runtime.ino > 0);
                // The minor comes from the name, the per-minor directory or
                // the object's own bytes (`MB.3.59`); the shim is the one that
                // minor needs.
                assert_eq!(runtime.version.major(), 8);
                assert!(
                    (2..=4).contains(&runtime.version.minor()),
                    "{} is not a supported minor",
                    runtime.version.dotted()
                );
                assert!(
                    runtime
                        .shim
                        .file_name()
                        .and_then(OsStr::to_str)
                        .is_some_and(|name| name.contains(&runtime.version.number())),
                    "{} is not the shim for PHP {}",
                    runtime.shim.display(),
                    runtime.version.dotted()
                );
                assert!(
                    matches!(php_boot(&manifest(PHP_ONLY), &search), PhpBoot::Proceed(_)),
                    "this host resolves the pair"
                );
            }
            Err(refusal) => {
                let message = php_refusal_message("app", "public/index.php", &refusal);
                assert!(message.contains("apt install libphp-embed"), "{message}");
                assert!(message.contains("BLASTIT_LIBPHP"), "{message}");
            }
        }
    }

    /// `ADR-0010` §3: zero `dlopen` in stage 0. The gate opens, `fstat`s and
    /// reads the ELF; the load happens inside a view worker. A constructor
    /// canary proves this from the outside (`MB.3.13.4`); this proves it by
    /// construction, so no one adds the call while that suite is not looking.
    #[test]
    fn the_gate_reads_the_elf_and_never_loads_it() {
        // The stage-0 production modules, one file each since `MB.40.4.3`:
        // every one of them, so a call added to any file is seen here.
        const SOURCES: [&str; 4] = [
            include_str!("mod.rs"),
            include_str!("gates.rs"),
            include_str!("fork_exec.rs"),
            include_str!("dev.rs"),
        ];
        // The needles are split so this test's own source does not match them.
        for banned in [
            concat!("dl", "open("),
            concat!("dlm", "open("),
            concat!("lib", "loading"),
            concat!("blastit_php", "_sapi"),
            concat!("Loaded", "::load"),
        ] {
            for source in SOURCES {
                assert!(
                    !source.contains(banned),
                    "stage 0 must not load the library: found `{banned}`"
                );
            }
        }
    }

    // ------------------------------------------- production provenance (R7)

    /// A `Scratch` under a directory only root or this process's uid can write.
    ///
    /// **A panic, never a `None`**: a `None` returned into a test body that then
    /// `return`s early is a test that asserts nothing on exactly the hosts where
    /// the rule is hardest to check (the reviewer's MINOR on `MB.3.40`). A host
    /// with no such directory is a host a deployment boot could not run on
    /// either, so the call-site tests would prove nothing there — and every
    /// rejected candidate is named with its reason, so the panic says why.
    ///
    /// Not [`Scratch`] itself: that one lives in `temp_dir()`, and `/tmp` is
    /// `1777` — a `0755` fixture *under* it is still a tree another uid could
    /// replace, which is exactly what these tests measure (`LESSONS.md`,
    /// `MB.3.40`). The candidates are the protected suite's
    /// (`tests/critical_php_embed.rs::protected_root`); the fixture is a
    /// [`Scratch`], so it is removed on the panic path too.
    fn protected_scratch(tag: &str) -> Scratch {
        use std::os::unix::fs::PermissionsExt;

        let mut candidates = Vec::new();
        if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
            candidates.push(PathBuf::from(dir));
        }
        candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"));
        if let Some(dir) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(dir));
        }
        for root in ["/opt", "/usr/local", "/srv"] {
            candidates.push(PathBuf::from(root));
        }
        let mut tried = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        for root in candidates {
            if !root.is_dir() {
                tried.push(format!("{}: not a directory", root.display()));
                continue;
            }
            let root = std::fs::canonicalize(&root).unwrap_or(root);
            if seen.contains(&root) {
                continue;
            }
            seen.push(root.clone());
            let dir = root.join(format!(
                "blastit-php-gate-protected-{}-{tag}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            if let Err(err) = std::fs::create_dir_all(&dir) {
                tried.push(format!("{}: create_dir_all failed ({err})", dir.display()));
                continue;
            }
            if let Err(err) = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
            {
                tried.push(format!("{}: chmod 0755 failed ({err})", dir.display()));
                let _ = std::fs::remove_dir_all(&dir);
                continue;
            }
            let probe = dir.join("probe");
            let reason = match std::fs::write(&probe, b"#!/bin/sh\n") {
                Err(err) => Some(format!("a probe file cannot be written ({err})")),
                Ok(()) => match crate::host::facts_of(&probe) {
                    Err(err) => Some(format!("the probe cannot be inspected ({err})")),
                    Ok(facts) => crate::host::protected(&facts, current_uid()).err(),
                },
            };
            let _ = std::fs::remove_file(&probe);
            match reason {
                Some(reason) => {
                    tried.push(format!("{}: {reason}", dir.display()));
                    let _ = std::fs::remove_dir_all(&dir);
                }
                None => return Scratch { dir },
            }
        }
        panic!(
            "no directory on this host is one uid's alone (tried [{}]), so a deployment boot \
             could not run on this host either and these tests would assert nothing",
            tried.join("; ")
        );
    }

    /// The accepted pair: a NTS stub `libphp` and the shim for its tuple.
    fn stub_pair(scratch: &Scratch) -> PhpSearch {
        let libphp = scratch.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        scratch.file(&shim_name(PhpVersion::new(8, 4)), &shared_object(&[]));
        search_for(&libphp, &scratch.dir)
    }

    /// `R7`/`B2-R5`: the deployment gate refuses a pair another uid could
    /// replace, keeping `ADR-0010` §4.1's sentence — and the *same* pair in a
    /// directory only root or init's uid can write proceeds, so the permission
    /// bits are the decision and not the path.
    #[test]
    fn a_deployment_boot_refuses_a_pair_a_route_uid_could_replace() {
        use std::os::unix::fs::PermissionsExt;

        let protected = protected_scratch("pair");
        let good = stub_pair(&protected);
        // `PHP_ONLY`'s front controller, at the accepted shape: a tree claim
        // that does not exist is a refusal of its own (`facts_of`'s error).
        std::fs::create_dir_all(protected.dir.join("public")).unwrap();
        let entry = protected.dir.join("public/index.php");
        std::fs::write(&entry, b"<?php echo 1;\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        let runtime = discover_php(&good).expect("a stub pair this host accepts");
        assert!(
            crate::host::Provenance {
                init_uid: current_uid(),
                objects: vec![
                    Claim::new("the NTS libphp", runtime.libphp.clone()),
                    Claim::new("the embedded shim", runtime.shim.clone()),
                ],
                trees: Vec::new(),
            }
            .check_objects()
            .is_ok(),
            "a 0755 pair owned by init's uid is the accepted shape"
        );

        // The library's directory, writable by another uid.
        let shared = Scratch::new("pair-shared");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let search = stub_pair(&shared);
        let checks = HostChecks::new(current_uid());
        let deployment = checks.deployment(&protected.dir);
        let message = php_boot_in(&manifest(PHP_ONLY), &search, Some(&deployment))
            .refusal()
            .expect("a writable libphp directory cannot be a deployment")
            .to_string();
        // `ADR-0010` §4.1's shape: the route, its entry, the offending path, the
        // tuple and the shim, the package, and the override.
        assert!(message.contains("route \"app\""), "{message}");
        assert!(
            message.contains("entrypoint php \"public/index.php\""),
            "{message}"
        );
        assert!(message.contains("the NTS libphp"), "{message}");
        assert!(
            message.contains(&search.libphp.as_ref().unwrap().display().to_string()),
            "the offending library is named: {message}"
        );
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );
        assert!(message.contains("chown"), "and the fix: {message}");
        assert!(message.contains("(PHP 8.4, "), "{message}");
        assert!(
            message.contains(&shim_name(PhpVersion::new(8, 4))),
            "{message}"
        );
        assert!(
            message.contains("install the PHP embed package"),
            "{message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );

        // The shim is the pair's other half: a protected library with a shim in
        // a writable directory is still a pair a route uid owns.
        let shims = protected.dir.join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::set_permissions(&shims, std::fs::Permissions::from_mode(0o777)).unwrap();
        let shim = shims.join(shim_name(PhpVersion::new(8, 4)));
        std::fs::write(&shim, shared_object(&[])).unwrap();
        let search = search_for(&protected.dir.join("libphp8.4.so"), &shims);
        let message = php_boot_in(&manifest(PHP_ONLY), &search, Some(&deployment))
            .refusal()
            .expect("a writable shim directory cannot be a deployment")
            .to_string();
        assert!(message.contains("the embedded shim"), "{message}");
        assert!(
            message.contains(&shim.display().to_string()),
            "the offending shim is named: {message}"
        );
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );

        // And the same pair, once its directories are the accepted shape, is
        // what a deployment boots: nothing in `dev`'s answer changed.
        assert!(
            matches!(
                php_boot_in(&manifest(PHP_ONLY), &good, Some(&deployment)),
                PhpBoot::Proceed(_)
            ),
            "a 0755 pair plus a protected tree must not be refused"
        );
    }

    /// `B2-R5`: the tree the worker's `cwd`/`include_path` is — the `Blastfile`'s
    /// directory, each `entrypoint php` and each `static` root — is
    /// refused when a route uid could write it, in its own sentence (this one is
    /// fixed by `chown`/`chmod`, not by installing a package).
    #[test]
    fn a_deployment_boot_refuses_a_tree_a_route_uid_could_write() {
        use std::os::unix::fs::PermissionsExt;

        let protected = protected_scratch("tree");
        let search = stub_pair(&protected);
        // One PHP route and one static route, on disjoint trees: a manifest that
        // declares both blocks on *one* route must keep the entry under the
        // root, which would make the two legs interfere.
        let manifest_source = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}

view docs regex '/assets/.*' {}

static \"/assets\" {
    root \"site\"
}
";
        // The accepted shape this test starts from: the entry at `0644`, the
        // static root at `0755`, both owned by init's uid.
        std::fs::create_dir_all(protected.dir.join("public")).unwrap();
        let entry = protected.dir.join("public/index.php");
        std::fs::write(&entry, b"<?php echo 1;\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        let root = protected.dir.join("site");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let checks = HostChecks::new(current_uid());
        let accepted = checks.deployment(&protected.dir);
        assert!(
            matches!(
                php_boot_in(&manifest(manifest_source), &search, Some(&accepted)),
                PhpBoot::Proceed(_)
            ),
            "a 0755 tree owned by init's uid is the accepted shape: {:?}",
            php_boot_in(&manifest(manifest_source), &search, Some(&accepted))
        );

        // 1. The directory the manifest was read from.
        let shared = Scratch::new("tree-shared");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let checks = HostChecks::new(current_uid());
        let message = php_boot_in(
            &manifest(manifest_source),
            &search,
            Some(&checks.deployment(&shared.dir)),
        )
        .refusal()
        .expect("a writable manifest directory cannot be a deployment")
        .to_string();
        assert!(
            message.contains("the directory holding the Blastfile"),
            "the tree is named for what it is: {message}"
        );
        assert!(
            message.contains(&shared.dir.display().to_string()),
            "{message}"
        );
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );
        assert!(message.contains("chown"), "the fix: {message}");
        assert!(
            !message.contains("install the PHP embed package"),
            "a tree is not fixed by a package: {message}"
        );

        // 2. The front controller itself, world-writable inside a protected tree.
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o666)).unwrap();
        let message = php_boot_in(&manifest(manifest_source), &search, Some(&accepted))
            .refusal()
            .expect("a writable front controller cannot be a deployment")
            .to_string();
        assert!(
            message.contains("route \"app\": entrypoint php \"public/index.php\""),
            "the entry is named with its route: {message}"
        );
        assert!(message.contains(&entry.display().to_string()), "{message}");
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();

        // 3. A static root: the responder reads it, and the manifest declares
        // one next to the PHP route.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let message = php_boot_in(&manifest(manifest_source), &search, Some(&accepted))
            .refusal()
            .expect("a writable static root cannot be a deployment")
            .to_string();
        assert!(
            message.contains("static \"/assets\": root \"site\""),
            "the static root is named with its route: {message}"
        );
        assert!(message.contains(&root.display().to_string()), "{message}");

        // The same tree at `0755` is accepted: the bits, not the path.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(
                php_boot_in(&manifest(manifest_source), &search, Some(&accepted)),
                PhpBoot::Proceed(_)
            ),
            "the same tree at 0755 is what a deployment boots"
        );
    }

    /// `R13`: a path a PHP manifest names that is **not there** is refused with
    /// the path and "no such file or directory", not with the provenance
    /// sentence's "it cannot be inspected (No such file or directory)" — which
    /// reads as a permissions problem and sends the operator to `chmod` a file
    /// that does not exist (the reviewer's MINOR on `MB.3.40`).
    #[test]
    fn a_missing_entry_or_static_root_names_the_missing_path() {
        use std::os::unix::fs::PermissionsExt;

        let protected = protected_scratch("missing");
        let search = stub_pair(&protected);
        std::fs::create_dir_all(protected.dir.join("public")).unwrap();
        let entry = protected.dir.join("public/index.php");
        std::fs::write(&entry, b"<?php echo 1;\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        let checks = HostChecks::new(current_uid());
        let deployment = checks.deployment(&protected.dir);
        // A PHP route and a static route, as in the tree test above: the gate
        // only runs for a manifest with an `entrypoint php`, so the static half must
        // ride beside one.
        let source = |entry: &str, root: &str| {
            format!(
                "entrypoint php \"{entry}\"\n\nview app regex '.*' {{}}\n\nview docs regex \
                 '/assets/.*' {{}}\n\nstatic \"/assets\" {{\n    root \"{root}\"\n}}\n"
            )
        };
        let refuse = |source: &str| {
            php_boot_in(&manifest(source), &search, Some(&deployment))
                .refusal()
                .expect("a manifest naming a path that is not there cannot be a deployment")
                .to_string()
        };

        // 1. The front controller.
        let absent = "public/missing.php";
        let missing = protected.dir.join(absent);
        std::fs::create_dir_all(protected.dir.join("site")).unwrap();
        std::fs::set_permissions(
            protected.dir.join("site"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let message = refuse(&source(absent, "site"));
        assert!(message.contains(absent), "the entry is named: {message}");
        assert!(
            message.contains(&missing.display().to_string()),
            "and the path it resolves to: {message}"
        );
        assert!(
            message.contains("no such file or directory"),
            "a missing path says so: {message}"
        );
        assert!(
            !message.contains("cannot be inspected"),
            "a path that is not there is not an unreadable one: {message}"
        );

        // 2. A `[route.static] root`.
        std::fs::remove_dir_all(protected.dir.join("site")).unwrap();
        let site = protected.dir.join("site");
        let message = refuse(&source("public/index.php", "site"));
        assert!(
            message.contains("static \"/assets\": root \"site\""),
            "the static root is named with its route: {message}"
        );
        assert!(
            message.contains(&site.display().to_string()),
            "and the path it resolves to: {message}"
        );
        assert!(
            message.contains("no such file or directory"),
            "a missing path says so: {message}"
        );
        assert!(
            !message.contains("cannot be inspected"),
            "a path that is not there is not an unreadable one: {message}"
        );

        // 3. A **dangling symlink** is the same answer, and that is what
        // `fs::metadata` means by `NotFound`: it follows the link, so the file
        // the boot would read is the one that is not there.
        let dangling = protected.dir.join("public/dangling.php");
        std::os::unix::fs::symlink(protected.dir.join("public/gone.php"), &dangling)
            .expect("a dangling symlink");
        let message = refuse(&source("public/dangling.php", "site"));
        assert!(
            message.contains("public/dangling.php") && message.contains("is missing"),
            "a dangling symlink is missing, not unreadable: {message}"
        );
        assert!(!message.contains("cannot be inspected"), "{message}");
        std::fs::remove_file(&dangling).unwrap();

        // And the same manifest with both paths created is what a deployment
        // boots, so the two refusals are the missing path and nothing else.
        std::fs::create_dir_all(&site).unwrap();
        std::fs::set_permissions(&site, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(
                php_boot_in(
                    &manifest(&source("public/index.php", "site")),
                    &search,
                    Some(&deployment)
                ),
                PhpBoot::Proceed(_)
            ),
            "the same manifest with both paths there is what a deployment boots"
        );
    }

    /// The seam's contract (`MB.3.44`): `Ok(())` is "the gate proceeds" and
    /// `Err` is a **refusal**.
    ///
    /// A deployment whose manifest declares no `entrypoint php` has no pair
    /// and no entry to authorise — `php_boot_in` answers `Absent` and the boot
    /// proceeds, exactly as `dev`/`check` do. Mapping that to `Err` let a caller
    /// read a *proceeding* manifest as a refused one, which is the confusion the
    /// seam exists to prevent, so both arms are pinned here.
    #[test]
    fn the_deployment_seam_proceeds_on_an_absent_gate_and_refuses_a_bad_pair() {
        use std::os::unix::fs::PermissionsExt;

        let protected = protected_scratch("seam");
        std::fs::create_dir_all(protected.dir.join("public")).unwrap();
        let entry = protected.dir.join("public/index.php");
        std::fs::write(&entry, b"<?php echo 1;\n").unwrap();
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _pair = stub_pair(&protected);
        let libphp = protected.dir.join("libphp8.4.so");

        // 1. No `entrypoint php`: nothing to authorise, so the boot proceeds.
        let static_only = "\
static \"/\" {
    root \"site\"
}
";
        assert_eq!(
            deployment_boot_check(static_only, &protected.dir, &libphp, &protected.dir, 20_000),
            Ok(()),
            "a manifest with no entrypoint php proceeds through the deployment gate"
        );

        // 2. A refused pair still produces `ADR-0010` §4.1's sentence, so arm 1
        // is not passing because the seam never looks.
        let shared = Scratch::new("seam-shared");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let writable = shared.file("libphp8.4.so", &shared_object(&["executor_globals"]));
        shared.file(&shim_name(PhpVersion::new(8, 4)), &shared_object(&[]));
        let message =
            deployment_boot_check(PHP_ONLY, &protected.dir, &writable, &shared.dir, 20_000)
                .expect_err("a pair another uid could replace must refuse the boot");
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );
        assert!(
            message.contains("install the PHP embed package"),
            "{message}"
        );
        assert!(
            message.contains("BLASTIT_LIBPHP=/path/to/libphp.so"),
            "{message}"
        );

        // 3. And the accepted pair at 0755 is `Ok`, so the refusal above is the
        // permission bits and nothing else.
        assert_eq!(
            deployment_boot_check(PHP_ONLY, &protected.dir, &libphp, &protected.dir, 20_000),
            Ok(()),
            "a 0755 pair owned by init's uid is what a deployment boots"
        );
    }

    /// The other half of the scope rule: a `dev`/`check` boot is handed the very
    /// same paths and must not ask the production question at all — the shim a
    /// developer just built lives in a checkout that uid writes.
    #[test]
    fn a_development_boot_does_not_apply_the_production_rule() {
        use std::os::unix::fs::PermissionsExt;

        let shared = Scratch::new("dev-scope");
        std::fs::set_permissions(&shared.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let search = stub_pair(&shared);
        assert!(
            matches!(php_boot(&manifest(PHP_ONLY), &search), PhpBoot::Proceed(_)),
            "`dev`/`check` must keep booting out of a world-writable checkout"
        );
        // And the production question about those same bytes is a refusal, so
        // this test cannot pass by the fixture being acceptable after all.
        let checks = HostChecks::new(current_uid());
        let message = php_boot_in(
            &manifest(PHP_ONLY),
            &search,
            Some(&checks.deployment(&shared.dir)),
        )
        .refusal()
        .expect("the same bytes are refused by a deployment")
        .to_string();
        assert!(
            message.contains("could be replaced by another uid"),
            "{message}"
        );
    }
}

/// `R13` after `ADR-0010` §2: an `entrypoint php` file is its own executor, so the
/// app rule must never demand the WSGI callable for it (`MB.3.5.4`).
///
/// The old predicate read "not all-static" as "needs an app" and refused a
/// PHP-only manifest with `BLASTIT_APP: the WSGI callable (…) is required` —
/// advice that cannot make a view serve. What the rule actually asks is
/// [`Manifest::can_boot_without_app`]: does every request reach an executor the
/// file owns?
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod app_rule_executors {
    use super::*;

    /// A PHP route with no app: the app rule must not demand the callable.
    /// (What an `entrypoint php` boot needs is a usable NTS `libphp`, and
    /// [`php_boot_gate`] is the gate that asks; that is a different sentence.)
    const PHP_ONLY: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

    /// Files from disk, everything else to the front controller: also no app.
    const STATIC_AND_PHP: &str = "\
entrypoint php \"public/index.php\"

static \"/\" {
    root \"public\"
}

view app regex '.*' {}
";

    /// One route that declares no executor: a WSGI view, which needs the app.
    const WITH_A_VIEW: &str = "\
view web regex '.*' {}
";

    /// The sentence the rule refuses with, exactly.
    const REQUIRE_APP: &str = "BLASTIT_APP: the WSGI callable (`module:attribute`) is required";

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        manifest
    }

    /// Every own-executor shape needs no `app`: PHP only, and the page's
    /// static-plus-PHP route.
    #[test]
    fn a_php_manifest_needs_no_app() {
        for source in [PHP_ONLY, STATIC_AND_PHP] {
            assert_eq!(
                app_rule(&Config::default(), &manifest(source)),
                Ok(()),
                "{source}"
            );
        }
    }

    /// The refusal case did not vanish: a manifest with no `app` and no route
    /// that runs its own executor still refuses, with the sentence that names
    /// the setting — and never the Python-free build's `uv add` advice, which
    /// would send a PHP developer to install a package the route cannot use.
    #[test]
    fn a_manifest_with_no_executor_still_requires_the_callable() {
        // A `Blastfile` declares one entrypoint for the whole file (§12.2), so
        // "a PHP view beside a WSGI view" is not a shape the language offers;
        // the refusal is measured on the shapes that need a callable.
        for source in [WITH_A_VIEW, "", "view web regex '.*' {\n    body json\n}\n"] {
            let message = app_rule(&Config::default(), &manifest(source)).unwrap_err();
            assert_eq!(message, REQUIRE_APP, "{source}");
            assert!(
                !message.contains("uv add"),
                "the app rule never names the package: {message}"
            );
        }
    }

    /// `BLASTIT_APP` still satisfies the rule for a WSGI view, and the
    /// all-static-with-an-app refusal is untouched by the wider predicate.
    #[test]
    fn the_two_halves_of_the_rule_still_hold() {
        let with_app = Config {
            app: Some("web.wsgi:app".to_string()),
            ..Config::default()
        };
        assert_eq!(app_rule(&with_app, &manifest(WITH_A_VIEW)), Ok(()));

        let static_only = "static \"/\" {\n    root \"site\"\n}\n";
        assert_eq!(app_rule(&Config::default(), &manifest(static_only)), Ok(()));
        let message = app_rule(&with_app, &manifest(static_only)).unwrap_err();
        assert!(
            message.starts_with("BLASTIT_APP: the file serves static mounts and no views"),
            "{message}"
        );
    }

    /// §9.5/§12.2: an `entrypoint php` no view reaches needs **no**
    /// `BLASTIT_APP`. This is the arm the boot's own predicate used to disagree
    /// on: it demanded at least one view for the PHP half, so a file that is a
    /// mount and a front controller was refused with "the WSGI callable is
    /// required" — advice that cannot make it serve. The boot now asks the
    /// model's one answer.
    ///
    /// `EntrypointNotReached` (the reminder for such a file) is `MB.21.3.0`'s
    /// warning and lives in `validate.rs`; it is not in the model yet
    /// (`critical_blastfile_entrypoint.rs` records the same gap), so what this
    /// fixture set pins is the half this leaf owns: no missing app, and not a
    /// static file.
    #[test]
    fn a_php_entrypoint_no_view_reaches_needs_no_app() {
        // The bare shape is the discriminating one: no view and no mount, so the
        // boot's old "PHP needs a view" reading demanded the callable while the
        // model already said this file is its own executor.
        let source = "entrypoint php \"public/index.php\"\n";
        let (front_controller, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        assert!(
            front_controller.can_boot_without_app(),
            "§12.2 answers true"
        );
        assert!(
            !front_controller.is_all_static(),
            "a front controller is not static"
        );
        assert_eq!(app_rule(&Config::default(), &front_controller), Ok(()));

        // The page's shape — a mount beside the front controller — is bootable
        // without an app too.
        let with_mount = "entrypoint php \"public/index.php\"\n\
                          static \"/assets\" {\n    root \"public/assets\"\n}\n";
        assert_eq!(app_rule(&Config::default(), &manifest(with_mount)), Ok(()));

        // The deny side is the same view with no entrypoint and no mount: nothing
        // answers it, so the callable is required again.
        let without_entrypoint = "view web regex '.*' {}\n";
        assert_eq!(
            app_rule(&Config::default(), &manifest(without_entrypoint)),
            Err(REQUIRE_APP.to_string())
        );
    }
}

/// `ADR-0011` (`MB.3.53`): the production path's ingress rule, as the serve path
/// calls it.
///
/// The stage-1 half is measured by a real production process
/// (`crates/blastit/tests/critical_raw_ingress.rs`); this half is the rule the
/// serve path calls, and it is a unit test because an unprivileged `cargo test`
/// cannot reach a stage-0 gate that lives after the manifest parse — stage 0
/// reads its capability sets first and refuses an unprivileged boot there
/// (`blastit_sys::caps::detect`).
#[cfg(test)]
mod raw_ingress_gate_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{Manifest, ServeGates, raw_ingress_gate};

    fn manifest(source: &str) -> Manifest {
        let (manifest, _warnings) = Manifest::parse_blastfile(source, 20_000).unwrap();
        manifest
    }

    const REFUSED: &str = "\
app \"app:app\"

view upload POST \"/upload\" {
    body raw
}
";

    const ACCEPTED: &str = "\
app \"app:app\"

view upload POST \"/upload\" {
    body raw unvalidated
}
";

    /// The gate has exactly one refusal, and it is §10.2's sentence: it names
    /// the view, what was declared (`body raw`) and the node to write
    /// (`body raw unvalidated`); the acknowledgement opens it, which is what
    /// makes this a gate rather than a blanket refusal of the codec.
    #[test]
    fn an_unacknowledged_raw_view_is_refused_and_an_acknowledged_one_is_not() {
        let message = raw_ingress_gate(&manifest(REFUSED)).unwrap_err();
        assert_eq!(
            message,
            "view \"upload\": body raw hands the request body to the application unvalidated, and \
             the production path refuses it without the acknowledgement; write `body raw \
             unvalidated` in that view, or declare the codec it takes (json, form, none)",
            "§10.2, verbatim"
        );
        assert_eq!(raw_ingress_gate(&manifest(ACCEPTED)), Ok(()));
    }

    /// A structured codec is untouched by the gate: only `raw` has no runtime
    /// validation for the acknowledgement to stand in for.
    #[test]
    fn every_other_codec_and_no_ingress_at_all_pass_the_gate() {
        for body in ["json", "form", "none"] {
            let source = format!(
                "app \"app:app\"\n\nview upload POST \"/upload\" {{\n    body {body}\n}}\n"
            );
            assert_eq!(
                raw_ingress_gate(&manifest(&source)),
                Ok(()),
                "body {body} needs no acknowledgement"
            );
        }
        let no_ingress = "app \"app:app\"\n\nview home regex '.*' {}\n";
        assert_eq!(raw_ingress_gate(&manifest(no_ingress)), Ok(()));
    }

    /// Every offender is named, in route order, so a manifest edited in two
    /// places is fixed in one boot rather than one denial at a time.
    #[test]
    fn every_unacknowledged_route_is_named_in_route_order() {
        let source = REFUSED.replace("view upload", "view first")
            + "\nview second POST \"/second\" {\n    body raw\n}\n";
        let message = raw_ingress_gate(&manifest(&source)).unwrap_err();
        assert_eq!(message.lines().count(), 2, "{message}");
        assert!(
            message.lines().next().unwrap().contains("\"first\""),
            "{message}"
        );
        assert!(
            message.lines().nth(1).unwrap().contains("\"second\""),
            "{message}"
        );
    }

    /// `MB.3.54` (item 3): the serve path cannot reach a bound view socket
    /// without the gate. The ordering half is [`bind_view_sockets`]'s
    /// `&ServeGates` parameter — a compile error, not a test, which is why it is
    /// the witness an unprivileged suite cannot be. This is the half a test can
    /// hold: the proof is withheld from exactly the manifest the gate refuses,
    /// and granted to the acknowledged one.
    #[test]
    fn the_serve_path_proof_is_withheld_from_a_refused_manifest() {
        assert!(
            ServeGates::clear(&manifest(REFUSED)).is_err(),
            "an unacknowledged raw route must not hand out the proof the bind needs"
        );
        assert!(
            ServeGates::clear(&manifest(ACCEPTED)).is_ok(),
            "the acknowledgement is what opens the gate"
        );
    }
}
