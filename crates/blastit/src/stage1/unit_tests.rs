#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::io::Read;
use std::net::SocketAddr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
#[cfg(feature = "python")]
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::child::{ChildIo, Filter, Privileges, child_exit, install_filter};
use crate::config::{Config, LogFormat};
#[cfg(feature = "python")]
use crate::dev_reload::{MarkOutcome, Unavailable};
use crate::logs::{Emitter, output_event};
use crate::mem_usage;
use crate::roles::{HttpFds, SchedulerFds};
use crate::stage1::adopt::*;
use crate::stage1::boot::*;
use crate::stage1::serve::*;
use crate::stage1::spawn::*;
use crate::stage1::*;
use crate::supervise::{ComponentPipes, Parts, Supervisor};
#[cfg(feature = "python")]
use blastit_events::ManifestWarning;
#[cfg(feature = "php")]
use blastit_events::Runtime;
use blastit_events::{Component, Event, EventKind, Level, Lifecycle, Stream};
use blastit_frame::Limits;
use blastit_http::HttpConfig;
use blastit_manifest::{Manifest, RoutingTable, UidLayout};
#[cfg(feature = "python")]
use blastit_scheduler::derived_view_cap;
use blastit_scheduler::{MemoryProbe, SchedulerConfig, routes_from_manifest};
use blastit_sys::{
    Fork, Pid, SeccompRole, Signal, SignalFd, SignalMask, bind_tcp, drop_privileges, harden_child,
    pipe, restore_signal_mask_in_child, set_dumpable, socketpair_stream,
};

#[cfg(all(test, feature = "python"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs::File;
    use std::os::unix::ffi::OsStrExt;
    use std::time::Duration;

    use super::*;
    use crate::config::Immortalize;

    #[test]
    fn learn_routing_table_reserves_slot_zero_and_preserves_declared_matches() {
        let source = "\
view a GET, POST regex '/a/[0-9]+', regex '/café/1' {}\n\
\n\
view b regex '/robots\\.txt' {}\n";
        let (manifest, _) = Manifest::parse_blastfile(source, 20_000).unwrap();
        let table = learn_routing_table(&manifest).unwrap();
        assert_eq!(
            table.route("GET", "/a/42"),
            Some(blastit_manifest::Match { route: 1, rule: 0 })
        );
        assert_eq!(
            table.route("POST", "/café/1"),
            Some(blastit_manifest::Match { route: 1, rule: 1 })
        );
        assert_eq!(table.route("DELETE", "/a/42"), None);
        assert_eq!(
            table.route("GET", "/robots.txt"),
            Some(blastit_manifest::Match { route: 2, rule: 0 })
        );
        // Slot 0 is the never-matching fallback: no request is routed into it,
        // and it contributes no method to a `405`.
        assert_eq!(table.route("GET", "/unrouted"), None);
        assert_eq!(table.route("GET", "/"), None);
        assert_eq!(table.methods_for("/unrouted"), None);
    }

    /// `blastit` keeps `blastit-ingress` as a direct dependency (`MA.2.1.1`):
    /// the supervisor and `check` read the same codec the manifest writes.
    #[test]
    fn the_ingress_types_are_importable_and_speak_the_manifest_spelling() {
        use blastit_ingress::{BodyCodec, IngressPolicy};

        let policy = IngressPolicy {
            body: BodyCodec::Json,
            allow_raw: false,
        };
        assert_eq!(
            serde_json::to_value(policy).unwrap(),
            serde_json::json!({"body": "json", "allow_raw": false})
        );
        assert_eq!(
            serde_json::to_value(BodyCodec::Raw).unwrap(),
            serde_json::json!("raw")
        );
    }

    /// Finding: the post-preload `sync_marks` can come back `Dead` — the boot's
    /// own pass succeeded and a *later* one died — and that row used to have no
    /// test at all. It must drop the `Reload` **and** keep `reload: on` off the
    /// transcript: printing it would claim a watcher that is gone.
    ///
    /// Grey-box: the real `Reload`, a real `Policy`/`ReportTable` and a scripted
    /// watcher whose second marking pass fails `EIO`.
    #[test]
    fn a_watcher_that_dies_after_the_preload_keeps_the_boot_line_off() {
        use std::os::fd::OwnedFd;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use crate::dev_reload::{
            Class, FakeWatcher, MAX_PATHS, Reload, ReloadConfig, TestClock, report_pair,
        };

        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "blastit-stage1-marks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app/views")).unwrap();
        let loaded = root.join("app/views/cart.py");
        std::fs::write(&loaded, "TAG = 1\n").unwrap();

        // `Default`, not the constructor named `new`: the no-inotify guard
        // counts that call in `src/` and allows exactly one, in `dev_reload.rs`.
        let mut watcher = FakeWatcher::default();
        // Call 1 is the boot's app-root mark, the only one it needs; the
        // preload pass is call 2 and dies.
        watcher.fail_from = Some((2, libc::EIO));
        let poll = std::os::unix::net::UnixStream::pair().unwrap().0;
        let mut reload = Reload::boot(
            ReloadConfig::new(root.clone(), root.join("Blastfile"), "tmpfs".to_string()),
            Box::new(watcher),
            OwnedFd::from(poll),
            Box::new(TestClock::new(std::time::Instant::now())),
        )
        .expect("the boot's own marking pass succeeds");
        assert_eq!(
            reload.marked_dirs(),
            1,
            "the app root, and nothing else yet"
        );

        // The preload batch adds a parent the boot had not marked.
        let _write = report_pair(reload.policy().reports(), 0, Class::Loaded, true).unwrap();
        let absorbed =
            reload
                .policy()
                .reports()
                .absorb(0, Class::Loaded, true, &[loaded], true, MAX_PATHS);
        assert!(!absorbed.refused(), "the loaded file joined the union");
        assert_eq!(absorbed.added, 1);

        let outcome = reload.sync_marks();
        assert!(reload.is_dead(), "the second pass latched the watcher dead");
        let dirs = reload.marked_dirs();
        match after_preload_marks(outcome, false, dirs) {
            BootLine::Stopped(why) => assert_eq!(why.errno_name(), "EIO"),
            other => panic!(
                "a watcher that dies after the preload prints no boot line and drops the \
                 reload, got {other:?}"
            ),
        }
        // The positive control: a pass that marked prints the line it claims.
        assert_eq!(
            after_preload_marks(MarkOutcome::Marked, false, 3),
            BootLine::Announce(3)
        );
        assert_eq!(
            after_preload_marks(MarkOutcome::Marked, true, 3),
            BootLine::Silent,
            "a boot that already owed its one warning never prints the line"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // ------------------------------------------- the adoption document (§4.6)

    /// Three open pipe read ends and their keepers, for a document that must
    /// pass the open-descriptor check.
    struct OpenFds {
        fds: [RawFd; 4],
        _keep: Vec<std::io::PipeWriter>,
    }

    fn open_fds() -> OpenFds {
        let mut keep = Vec::new();
        let mut fds = [0 as RawFd; 4];
        for slot in &mut fds {
            let (read, write) = std::io::pipe().expect("a pipe");
            *slot = std::os::fd::IntoRawFd::into_raw_fd(read);
            keep.push(write);
        }
        OpenFds { fds, _keep: keep }
    }

    /// Reads `document` through [`Adoption::read`] exactly as a boot would.
    fn read_document(document: &[u8]) -> Result<Adoption, String> {
        use std::io::Write as _;
        let (read, mut write) = std::io::pipe().expect("a pipe");
        write.write_all(document).expect("the document");
        drop(write);
        Adoption::read(std::os::fd::IntoRawFd::into_raw_fd(read))
    }

    fn component_json(fds: [RawFd; 3], pid: i32) -> serde_json::Value {
        serde_json::json!({
            "pid": pid,
            "stdout": fds[0],
            "stderr": fds[1],
            "events": fds[2],
        })
    }

    /// Every refusal in the adoption contract is a boot sentence, and each one
    /// is a distinct reason: an unknown key, a missing key, the wrong version,
    /// a descriptor this image does not hold, and the size bound.
    #[test]
    fn the_adoption_document_refuses_every_shape_it_cannot_honour() {
        let open = open_fds();
        let http = component_json([open.fds[0], open.fds[1], open.fds[2]], 4242);
        let chan = open.fds[3];
        let cases: Vec<(serde_json::Value, &str)> = vec![
            (
                serde_json::json!({"version": 1, "http": http, "chan": chan, "bogus": 0}),
                "unknown key `bogus`",
            ),
            (
                serde_json::json!({"version": 1, "http": http, "chan": chan, "dbproxy": {"pid": 1, "stdout": 999_999, "stderr": 999_999, "events": 999_999}}),
                "descriptor 999999 is not open",
            ),
            (
                serde_json::json!({"version": 2, "http": http, "chan": chan}),
                "`version` must be 1",
            ),
            (
                serde_json::json!({"version": 1, "chan": chan}),
                "missing `http`",
            ),
            (
                serde_json::json!({"version": 1, "http": http}),
                "missing `chan`",
            ),
            (
                serde_json::json!({"version": 1, "http": component_json([999_999, open.fds[1], open.fds[2]], 7), "chan": chan}),
                "descriptor 999999 is not open",
            ),
        ];
        for (document, expected) in cases {
            let bytes = serde_json::to_vec(&document).expect("a document");
            let error = read_document(&bytes).expect_err("this document must be refused");
            assert!(
                error.contains(expected),
                "expected {expected:?} from {document}, got {error:?}"
            );
        }

        // The size bound is checked before parsing, so an oversized document
        // never reaches `serde_json`.
        let oversized = vec![b' '; MAX_ADOPTION_DOCUMENT + 1];
        let error = read_document(&oversized).expect_err("an oversized document is refused");
        assert!(error.contains("more than the 4096-byte limit"), "{error}");

        // The positive control: these descriptors and this shape are accepted.
        let good = serde_json::to_vec(&serde_json::json!({
            "version": 1, "http": http, "chan": chan,
        }))
        .expect("a document");
        let adoption = read_document(&good).expect("a well-formed document is adopted");
        assert_eq!(adoption.http.pid, 4242);
        assert_eq!(adoption.chan, chan);
    }

    /// The stage-1 program prefix is collected in full and refused past eight
    /// tokens — never silently truncated (§4.6 step 6).
    #[test]
    fn a_stage1_prefix_past_eight_tokens_is_refused_not_truncated() {
        let long: Vec<String> = (0..9).map(|n| format!("--arg{n}")).collect();
        let error = stage1_program(Some(&long)).expect_err("nine tokens is refused");
        assert!(error.contains("9 token(s), not 1..=8"), "{error}");
        let ok = stage1_program(Some(&long[..8])).expect("eight tokens are accepted");
        assert_eq!(ok.len(), 8);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod child_plans {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A `blastit dev` root, removed when dropped.
    ///
    /// [`crate::dev_learn::Dev::adopt`] refuses anything that is not a `0700`
    /// directory owned by the caller, so the fixture builds exactly that and
    /// takes it away again even when an assertion fails.
    struct DevRoot(std::path::PathBuf);

    impl DevRoot {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "blastit-view-child-plans-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("the fixture's dev root");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("the fixture's dev root mode");
            Self(path)
        }

        fn dev(&self) -> crate::dev_learn::Dev {
            let host = std::net::IpAddr::V4(std::net::Ipv4Addr::from([127, 0, 0, 1]));
            crate::dev_learn::Dev::adopt(self.0.clone(), host)
                .expect("a 0700 dev root owned by the caller")
        }
    }

    impl Drop for DevRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Two routes with **different** uids and different egress, so “every route
    /// gets its own identity and its own reach” is asserted rather than implied
    /// by a single-route fixture.
    fn fixture() -> (Vec<blastit_scheduler::RouteInfo>, Vec<bool>) {
        (
            vec![
                blastit_scheduler::RouteInfo::new("shop", false, 20_001),
                blastit_scheduler::RouteInfo::new("api", false, 20_002),
            ],
            vec![false, true],
        )
    }

    /// D1, row 1: a production boot drops each view to **its own** uid and
    /// installs its own `View` filter.
    ///
    /// This is the pin the design asks for: inverting the boot-mode branch (or
    /// swapping two routes' uids) fails here.
    #[test]
    fn a_production_boot_drops_each_view_to_its_own_uid_and_filters_it() {
        let (routes, egress) = fixture();
        let plans = view_child_plans(&routes, &egress, None, true);
        assert_eq!(
            plans.privileges.len(),
            routes.len(),
            "one identity per route"
        );
        assert_eq!(plans.filter.len(), routes.len(), "one filter per route");
        assert_eq!(
            plans.privileges,
            vec![Privileges::DropTo(20_001), Privileges::DropTo(20_002)],
            "each route drops to its own uid, in routing order"
        );
        assert_eq!(
            plans.filter,
            vec![
                Filter::Seccomp(SeccompRole::View { egress: false }),
                Filter::Seccomp(SeccompRole::View { egress: true }),
            ],
            "each route's filter keeps its own egress"
        );
    }

    /// D1, row 2: a `blastit dev` boot shares the caller's uid and still
    /// installs the filter — `dev` is filtered today (`Config::seccomp`
    /// defaults to on).
    ///
    /// The shared variant is asserted as "not a per-route drop": `Privileges`
    /// has exactly two variants, and the census that proves no view crate
    /// constructs the shared one greps this directory — so the literal spelling
    /// lives in `view_child_plans` alone and a test does not become a false
    /// positive there.
    #[test]
    fn a_dev_boot_shares_the_uid_and_still_filters() {
        let root = DevRoot::new("dev");
        let dev = root.dev();
        let (routes, egress) = fixture();
        let plans = view_child_plans(&routes, &egress, Some(&dev), true);
        assert_eq!(plans.privileges.len(), routes.len());
        assert!(
            plans
                .privileges
                .iter()
                .all(|privileges| !matches!(privileges, Privileges::DropTo(_))),
            "dev shares the caller's uid, whatever the layout says: {:?}",
            plans.privileges
        );
        assert_eq!(
            plans.filter,
            vec![
                Filter::Seccomp(SeccompRole::View { egress: false }),
                Filter::Seccomp(SeccompRole::View { egress: true }),
            ],
            "dev is filtered: only the explicit BLASTIT_SECCOMP=0 is not"
        );
    }

    /// D1, row 3: only the explicit `BLASTIT_SECCOMP=0` switches the filter
    /// off, and it does so without changing the identity. Same spelling rule as
    /// row 2: `Filter` has exactly two variants, so "not a `Seccomp`" *is* the
    /// off variant, and the census stays clean.
    #[test]
    fn an_explicit_off_switch_installs_no_filter_but_keeps_the_identity() {
        let (routes, egress) = fixture();
        let plans = view_child_plans(&routes, &egress, None, false);
        assert_eq!(
            plans.privileges,
            vec![Privileges::DropTo(20_001), Privileges::DropTo(20_002)],
            "BLASTIT_SECCOMP=0 is not an identity switch"
        );
        assert_eq!(plans.filter.len(), routes.len());
        assert!(
            plans
                .filter
                .iter()
                .all(|filter| !matches!(filter, Filter::Seccomp(_))),
            "no route may keep a filter under BLASTIT_SECCOMP=0: {:?}",
            plans.filter
        );
    }
}
