//! Protected critical suite for the one filesystem watcher (`MB.23.3`).
//!
//! This is the **real-kernel** oracle: `make check` runs it as
//! `cargo test -p blastit-sys --test critical_fanotify --locked`, with no
//! feature flags, so it must not depend on `feature = "testutil"`. It proves
//! the measured `fanotify` facts the design froze
//! (`docs/design/dev-fanotify-reload.md` §4.2, research §1.2–§1.6) against the
//! kernel the suite is running on:
//!
//! * a create-write-close carries `FAN_CLOSE_WRITE`, an atomic editor save
//!   (`write tmp` + `rename`) carries `FAN_MOVED_TO` for the new name and
//!   `FAN_MOVED_FROM` for the old one, never a `FAN_CREATE`-only event for the
//!   saved name, an unlink carries `FAN_DELETE` and a `mkdir` carries
//!   `FAN_CREATE|FAN_ONDIR` — on a tmpfs **and** on this checkout's filesystem;
//! * masks are read as **bits**, never compared with `==`;
//! * a non-UTF-8 name arrives as bytes, not as a lossy string;
//! * marking a file is `ENOTDIR`, and a drained descriptor is `EAGAIN`;
//! * `FAN_MODIFY` is carried through when a test group asks for it, while the
//!   production mask is asserted never to contain it;
//! * an unprivileged `unshare(CLONE_NEWUSER|CLONE_NEWNS)` overlay mount proves
//!   the event's parent handle is the `AT_HANDLE_FID` handle of the marked
//!   directory, and that plain `flags = 0` is `EOPNOTSUPP` there — so a
//!   regression to `flags = 0` fails this suite.
//!
//! **A refused kernel is a failure, never a skip.** No test here is marked
//! ignored, none asks for root, and none returns silently: [`kernel_watcher`]
//! panics with the
//! errno and the fix, because a suite that quietly passes on a host where
//! `fanotify` is refused would certify nothing. The gate host and the CI
//! runners allow the unprivileged file-handle group; Docker's default seccomp
//! profile does not, and the message says so.
//!
//! The parser's malformed-record behaviour, the errno→[`WatcherError`] mapping,
//! the handle key and the read buffer's size/alignment are unit tests in
//! `src/fanotify.rs`, because they need nothing from the kernel.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use blastit_sys::fanotify::{
    Event, MARK_MASK, Op, ReadBuffer, Readout, Watcher, WatcherError, filesystem_type,
    parse_records,
};

/// How long a real filesystem event may take to reach the group.
const EVENT_DEADLINE: Duration = Duration::from_secs(10);

/// How long "nothing happened" is observed before it is asserted.
const QUIET_WINDOW: Duration = Duration::from_millis(150);

/// A per-process counter, so two tests never share a scratch directory.
static SCRATCH: AtomicU32 = AtomicU32::new(0);

/// The `errno` of the last failed call.
fn errno() -> Option<i32> {
    io::Error::last_os_error().raw_os_error()
}

/// A fresh scratch directory under `CARGO_TARGET_TMPDIR` (this checkout's
/// filesystem), removed when the fixture is dropped.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn under(base: &Path, label: &str) -> Self {
        let serial = SCRATCH.fetch_add(1, Ordering::Relaxed);
        let root = base.join(format!(
            "blastit-fanotify-{label}-{}-{serial}",
            std::process::id()
        ));
        if let Err(err) = fs::create_dir_all(&root) {
            panic!(
                "cannot create the scratch directory {}: {err}",
                root.display()
            );
        }
        Self { root }
    }

    /// A scratch directory on this checkout's filesystem.
    fn on_this_filesystem(label: &str) -> Self {
        Self::under(
            Path::new(env!("CARGO_TARGET_TMPDIR")),
            &format!("worktree-{label}"),
        )
    }

    /// A scratch directory on a tmpfs this host actually has.
    ///
    /// The candidates are the tmpfs mounts a Linux host has by default; the
    /// first one that reports `tmpfs` wins, and a host with none fails loudly
    /// rather than silently testing one filesystem twice.
    fn on_tmpfs(label: &str) -> Self {
        let uid = unsafe { libc::getuid() };
        let candidates = [
            PathBuf::from("/dev/shm"),
            PathBuf::from(format!("/run/user/{uid}")),
            PathBuf::from("/run"),
            PathBuf::from("/tmp"),
        ];
        for candidate in &candidates {
            if filesystem_type(candidate) == Some("tmpfs") {
                return Self::under(candidate, &format!("tmpfs-{label}"));
            }
        }
        panic!(
            "no tmpfs mount among {:?}: this suite must exercise a tmpfs as well as {}; \
             mount one (or run make check on the host)",
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            env!("CARGO_TARGET_TMPDIR")
        );
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Best effort: a failed cleanup must not turn a green suite red.
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The group, or a failure that names the errno and the fix.
///
/// Never a skip: a host that refuses the unprivileged file-handle group cannot
/// certify this module, and pretending otherwise is how a broken watcher ships.
fn kernel_watcher() -> Watcher {
    match Watcher::new() {
        Ok(watcher) => watcher,
        Err(err) => {
            let detail = match WatcherError::of(&err) {
                Some(typed) => format!("{typed} (errno {})", typed.errno()),
                None => err.to_string(),
            };
            panic!(
                "the kernel refused the unprivileged fanotify file-handle group: {detail}. \
                 This suite needs a kernel that allows it: run `make check` on the host, or \
                 allow fanotify_init in the devcontainer's profile (Docker's default seccomp \
                 profile denies it)."
            );
        }
    }
}

/// A short description of every event seen, for a timeout message.
fn describe(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| format!("{}:{:#x}", String::from_utf8_lossy(&event.name), event.mask))
        .collect()
}

/// Read once, appending to `seen`; a read error is a failure of the watcher.
fn drain(watcher: &mut Watcher, seen: &mut Vec<Event>) -> Readout {
    match watcher.read(seen) {
        Ok(readout) => readout,
        Err(err) => panic!("Watcher::read failed: {err}"),
    }
}

/// Wait (bounded) for an event named `name` carrying `bit`, then return it.
fn wait_for(
    watcher: &mut Watcher,
    seen: &mut Vec<Event>,
    name: &[u8],
    bit: u64,
    label: &str,
) -> Event {
    let deadline = Instant::now() + EVENT_DEADLINE;
    loop {
        let readout = drain(watcher, seen);
        if let Some(event) = seen
            .iter()
            .find(|event| event.name == name && event.mask & bit == bit)
        {
            return event.clone();
        }
        if Instant::now() >= deadline {
            panic!(
                "{label}: no event with {bit:#x} for {:?} within {:?}; saw {:?}",
                String::from_utf8_lossy(name),
                EVENT_DEADLINE,
                describe(seen)
            );
        }
        if readout.events == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Every event seen for `name`.
fn named<'a>(seen: &'a [Event], name: &[u8]) -> Vec<&'a Event> {
    seen.iter().filter(|event| event.name == name).collect()
}

/// The frozen public constants, and the flags this kernel must accept.
#[test]
fn the_frozen_flags_and_mask_are_the_measured_ones() {
    assert_eq!(Watcher::init_flags(), 0x0c03);
    assert_eq!(Watcher::mark_mask(), MARK_MASK);
    assert_eq!(
        Watcher::init_flags() & 0x0c00,
        0x0c00,
        "FAN_REPORT_DFID_NAME"
    );
    assert_ne!(MARK_MASK & libc::FAN_CLOSE_WRITE, 0);
    assert_ne!(MARK_MASK & libc::FAN_MOVED_TO, 0);
    assert_ne!(MARK_MASK & libc::FAN_MOVED_FROM, 0);
    assert_ne!(MARK_MASK & libc::FAN_CREATE, 0);
    assert_ne!(MARK_MASK & libc::FAN_DELETE, 0);
    assert_ne!(MARK_MASK & libc::FAN_ONDIR, 0);
    assert_ne!(MARK_MASK & libc::FAN_EVENT_ON_CHILD, 0);
    // The production mask never asks for a write-without-close.
    assert_eq!(MARK_MASK & libc::FAN_MODIFY, 0);
    assert_eq!(MARK_MASK & libc::FAN_ACCESS, 0);
    assert_eq!(MARK_MASK & libc::FAN_RENAME, 0);

    // `AT_HANDLE_FID` is Linux 6.5+; this host must be one of them, because the
    // overlay leg below depends on it and a `flags = 0` regression must be red.
    assert_eq!(
        Watcher::probe_flags(),
        libc::AT_HANDLE_FID as u32,
        "probe_flags must be AT_HANDLE_FID on a kernel >= 6.5 (this host runs {})",
        kernel_release()
    );
    // And the group itself must exist, or the failure names its errno and fix.
    let watcher = kernel_watcher();
    assert!(watcher.fd().as_raw_fd() >= 0);
}

/// `uname(2)`'s release, for a failure message.
fn kernel_release() -> String {
    // SAFETY: `utsname` is plain old data, so all-zero is a valid value, and
    // `uname` fills it before it is read.
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: `uname` takes one writable pointer to a `utsname`.
    if unsafe { libc::uname(&mut uts) } == -1 {
        return "an unknown kernel".to_string();
    }
    // SAFETY: `uname` filled `release` with a NUL-terminated string.
    let release = unsafe { std::ffi::CStr::from_ptr(uts.release.as_ptr()) };
    release.to_string_lossy().into_owned()
}

/// The measured event semantics, on a tmpfs and on this checkout's filesystem.
///
/// Every mask is tested as bits (`mask & bit == bit`), never with `==`: one
/// create-write-close legitimately arrives as `FAN_CREATE|FAN_CLOSE_WRITE`.
#[test]
fn create_close_rename_delete_and_a_non_utf8_name_on_both_filesystems() {
    let worktree = Fixture::on_this_filesystem("events");
    let tmpfs = Fixture::on_tmpfs("events");
    for (label, fixture) in [("worktree", &worktree), ("tmpfs", &tmpfs)] {
        let dir = fixture.path();
        let mut watcher = kernel_watcher();
        if let Err(err) = watcher.mark_dir(dir) {
            panic!(
                "{label}: mark_dir({}) failed: {err} ({:?})",
                dir.display(),
                WatcherError::of(&err)
            );
        }

        // 1. create, write, close -> FAN_CLOSE_WRITE, resolved to the marked dir.
        let app = dir.join("app.py");
        fs::write(&app, b"print(1)\n")
            .unwrap_or_else(|err| panic!("{label}: cannot write {}: {err}", app.display()));
        let mut seen = Vec::new();
        let closed = wait_for(
            &mut watcher,
            &mut seen,
            b"app.py",
            libc::FAN_CLOSE_WRITE,
            label,
        );
        assert_eq!(
            closed.dir, dir,
            "{label}: the event's parent handle must resolve to the marked directory"
        );
        assert_ne!(closed.mask & libc::FAN_CLOSE_WRITE, 0);

        // 2. An atomic editor save: write a temporary, rename it over the name.
        let temp = dir.join("app.py.tmp");
        fs::write(&temp, b"print(2)\n")
            .unwrap_or_else(|err| panic!("{label}: cannot write {}: {err}", temp.display()));
        let before_rename = seen.len();
        fs::rename(&temp, &app)
            .unwrap_or_else(|err| panic!("{label}: cannot rename {}: {err}", temp.display()));
        let moved_to = wait_for(
            &mut watcher,
            &mut seen,
            b"app.py",
            libc::FAN_MOVED_TO,
            label,
        );
        assert_eq!(moved_to.dir, dir);
        let moved_from = wait_for(
            &mut watcher,
            &mut seen,
            b"app.py.tmp",
            libc::FAN_MOVED_FROM,
            label,
        );
        assert_eq!(moved_from.dir, dir);
        // The saved name never arrives as a partial create-only event: a
        // reload decision that only looked at `FAN_CREATE` would restart on a
        // half-written file. Only the events of the save itself are inspected;
        // the earlier create of `app.py` legitimately carried `FAN_CREATE`.
        for event in seen.get(before_rename..).unwrap_or(&[]) {
            if event.name == b"app.py" && event.mask & libc::FAN_CREATE == libc::FAN_CREATE {
                assert_ne!(
                    event.mask & libc::FAN_MOVED_TO,
                    0,
                    "{label}: a FAN_CREATE-only event for the saved name: {:?}",
                    describe(&seen)
                );
            }
        }

        // 3. An unlink -> FAN_DELETE for the name.
        fs::remove_file(&app)
            .unwrap_or_else(|err| panic!("{label}: cannot remove {}: {err}", app.display()));
        let deleted = wait_for(&mut watcher, &mut seen, b"app.py", libc::FAN_DELETE, label);
        assert_eq!(deleted.dir, dir);

        // 4. A mkdir -> FAN_CREATE|FAN_ONDIR (both bits, never one alone).
        let pkg = dir.join("pkg");
        fs::create_dir(&pkg)
            .unwrap_or_else(|err| panic!("{label}: cannot create {}: {err}", pkg.display()));
        let created = wait_for(&mut watcher, &mut seen, b"pkg", libc::FAN_CREATE, label);
        assert_ne!(
            created.mask & libc::FAN_ONDIR,
            0,
            "{label}: a directory event"
        );
        assert_eq!(created.dir, dir);

        // 5. A non-UTF-8 name arrives as bytes, not as a lossy string.
        let raw_name = b"caf\xe9.py";
        let raw_path = dir.join(std::ffi::OsStr::from_bytes(raw_name));
        fs::write(&raw_path, b"x")
            .unwrap_or_else(|err| panic!("{label}: cannot write {}: {err}", raw_path.display()));
        let raw = wait_for(
            &mut watcher,
            &mut seen,
            raw_name,
            libc::FAN_CLOSE_WRITE,
            label,
        );
        assert_eq!(raw.name, raw_name);
        assert_ne!(
            raw.name,
            String::from_utf8_lossy(raw_name).as_bytes(),
            "{label}: the name must not be a lossy conversion"
        );

        // 6. A write without a close is not a trigger: FAN_MODIFY is not asked
        // for, so nothing at all arrives until the descriptor is closed.
        let held = dir.join("held.py");
        let mut open = fs::File::create(&held)
            .unwrap_or_else(|err| panic!("{label}: cannot create {}: {err}", held.display()));
        io::Write::write_all(&mut open, b"partial")
            .unwrap_or_else(|err| panic!("{label}: cannot write {}: {err}", held.display()));
        io::Write::flush(&mut open)
            .unwrap_or_else(|err| panic!("{label}: cannot flush {}: {err}", held.display()));
        // The create itself is a trigger; the write is not.
        let _ = wait_for(&mut watcher, &mut seen, b"held.py", libc::FAN_CREATE, label);
        std::thread::sleep(QUIET_WINDOW);
        drain(&mut watcher, &mut seen);
        for event in named(&seen, b"held.py") {
            assert_eq!(
                event.mask & libc::FAN_MODIFY,
                0,
                "{label}: the production mask must never ask for FAN_MODIFY"
            );
            assert_eq!(
                event.mask & libc::FAN_CLOSE_WRITE,
                0,
                "{label}: a write without close must not look like a saved file"
            );
        }
        drop(open);
        let _ = wait_for(
            &mut watcher,
            &mut seen,
            b"held.py",
            libc::FAN_CLOSE_WRITE,
            label,
        );

        // 7. Marking a file is ENOTDIR: the walk marks directories only.
        let file = dir.join("plain.py");
        fs::write(&file, b"x")
            .unwrap_or_else(|err| panic!("{label}: cannot write {}: {err}", file.display()));
        let refused = watcher.mark_dir(&file);
        let err = match refused {
            Ok(()) => panic!("{label}: marking a file must be refused"),
            Err(err) => err,
        };
        assert_eq!(
            WatcherError::of(&err),
            Some(&WatcherError::Unavailable {
                op: Op::Mark,
                errno: libc::ENOTDIR
            }),
            "{label}: a file mark must be ENOTDIR, got {:?}",
            WatcherError::of(&err)
        );

        // 8. A drained descriptor is EAGAIN, and the wrapper reports an empty,
        // non-overflow readout rather than an error.
        let mut quiet = Vec::new();
        loop {
            let readout = drain(&mut watcher, &mut quiet);
            if readout.events == 0 {
                break;
            }
        }
        let mut buffer = ReadBuffer::new();
        let capacity = buffer.as_bytes().len();
        // SAFETY: the buffer is `capacity` writable bytes and the descriptor is
        // open; the call only writes into the buffer.
        let raw = unsafe {
            libc::read(
                watcher.fd().as_raw_fd(),
                buffer.as_mut_slice().as_mut_ptr().cast(),
                capacity,
            )
        };
        assert_eq!(raw, -1, "{label}: a drained descriptor must not block");
        assert_eq!(
            errno(),
            Some(libc::EAGAIN),
            "{label}: a drained group must be EAGAIN"
        );
        let mut empty = Vec::new();
        assert_eq!(
            drain(&mut watcher, &mut empty),
            Readout {
                events: 0,
                overflow: false,
                skipped: 0
            },
            "{label}: EAGAIN is an empty readout, not an error"
        );
    }
}

/// `FAN_MODIFY` is carried through unchanged when a group asks for it.
///
/// The production mask never asks for it (asserted above and in the crate's
/// unit tests), but the parser must not silently drop a bit a *test* group
/// requested — that would make every mask assertion in the policy untestable.
#[test]
fn modify_is_carried_when_a_group_asks_for_it() {
    let fixture = Fixture::on_this_filesystem("modify");
    let dir = fixture.path();
    let c_dir = CString::new(dir.as_os_str().as_bytes())
        .unwrap_or_else(|_| panic!("{}: the scratch path has a NUL byte", dir.display()));

    // SAFETY: `fanotify_init` takes two integers and returns a descriptor.
    let fd = unsafe { libc::fanotify_init(Watcher::init_flags(), libc::O_RDONLY as libc::c_uint) };
    if fd == -1 {
        panic!("fanotify_init failed: {}", io::Error::last_os_error());
    }
    let mask = libc::FAN_MODIFY | libc::FAN_CLOSE_WRITE | libc::FAN_EVENT_ON_CHILD;
    // SAFETY: the descriptor is open, `c_dir` is NUL-terminated, and the kernel
    // copies the path.
    let marked = unsafe {
        libc::fanotify_mark(
            fd,
            libc::FAN_MARK_ADD | libc::FAN_MARK_ONLYDIR,
            mask,
            libc::AT_FDCWD,
            c_dir.as_ptr(),
        )
    };
    if marked == -1 {
        let err = io::Error::last_os_error();
        // SAFETY: the descriptor was created above and nothing else owns it.
        unsafe { libc::close(fd) };
        panic!("fanotify_mark with FAN_MODIFY failed: {err}");
    }

    let written = dir.join("modify.py");
    fs::write(&written, b"x")
        .unwrap_or_else(|err| panic!("cannot write {}: {err}", written.display()));

    let mut buffer = ReadBuffer::new();
    let capacity = buffer.as_bytes().len();
    let deadline = Instant::now() + EVENT_DEADLINE;
    let mut events = Vec::new();
    loop {
        // SAFETY: the buffer is `capacity` writable bytes, the descriptor is
        // open, and the call only writes into the buffer.
        let read = unsafe { libc::read(fd, buffer.as_mut_slice().as_mut_ptr().cast(), capacity) };
        if read > 0 {
            let bytes = buffer.as_bytes().get(..read as usize).unwrap_or(&[]);
            let mut parsed = Vec::new();
            match parse_records(bytes, &mut parsed) {
                Ok(readout) => {
                    assert_eq!(readout.skipped, 0, "the kernel's own event must parse");
                    events.extend(parsed);
                }
                Err(err) => panic!("parse_records refused the kernel's event: {err}"),
            }
        } else if read < 0 && errno() != Some(libc::EAGAIN) {
            panic!("read failed: {}", io::Error::last_os_error());
        }
        if events
            .iter()
            .any(|event| event.name == b"modify.py" && event.mask & libc::FAN_MODIFY != 0)
        {
            break;
        }
        if Instant::now() >= deadline {
            // SAFETY: the descriptor was created above and nothing else owns it.
            unsafe { libc::close(fd) };
            panic!(
                "no FAN_MODIFY event for modify.py within {EVENT_DEADLINE:?}; saw {:?}",
                describe(&events)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // SAFETY: the descriptor was created above and nothing else owns it.
    unsafe { libc::close(fd) };

    let modify = events
        .iter()
        .find(|event| event.name == b"modify.py" && event.mask & libc::FAN_MODIFY != 0);
    let modify = match modify {
        Some(event) => event,
        None => panic!("FAN_MODIFY vanished from the parsed events"),
    };
    assert_eq!(
        modify.mask & libc::FAN_MODIFY,
        libc::FAN_MODIFY,
        "the bit must be carried unchanged"
    );
    // And the production mask still never asks for it.
    assert_eq!(MARK_MASK & libc::FAN_MODIFY, 0);
    assert_eq!(Watcher::mark_mask() & libc::FAN_MODIFY, 0);
}

/// An unprivileged overlay mount, where the parent handle must be the
/// `AT_HANDLE_FID` handle and `flags = 0` must be `EOPNOTSUPP`.
///
/// The leg runs in a forked child because `unshare(CLONE_NEWUSER)` changes the
/// whole process's namespaces and requires a single-threaded caller. The child
/// body is deliberately short and mostly syscall-shaped — the same contract
/// `tests/critical_embedded_seccomp.rs` keeps for its forked children — and it
/// ends in `_exit` without unwinding.
#[test]
fn an_overlay_parent_handle_is_the_at_handle_fid_handle() {
    let fixture = Fixture::on_tmpfs("overlay");
    let base = fixture.path();
    let lower = base.join("lower");
    let upper = base.join("upper");
    let work = base.join("work");
    let merged = base.join("merged");
    for dir in [&lower, &upper, &work, &merged] {
        fs::create_dir(dir).unwrap_or_else(|err| panic!("cannot create {}: {err}", dir.display()));
    }
    fs::write(lower.join("a.py"), b"x")
        .unwrap_or_else(|err| panic!("cannot write the lower fixture: {err}"));

    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    let options = format!(
        "lowerdir={},upperdir={},workdir={}",
        lower.display(),
        upper.display(),
        work.display()
    );
    let merged_c = CString::new(merged.as_os_str().as_bytes())
        .unwrap_or_else(|_| panic!("{}: the scratch path has a NUL byte", merged.display()));
    let options_c = CString::new(options.as_bytes())
        .unwrap_or_else(|_| panic!("the overlay options have a NUL byte"));

    in_child("overlay", || {
        // SAFETY: `unshare` takes one flag word and creates namespaces for this
        // process only; the child is the process being changed.
        if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) } == -1 {
            return child_refused(10, "unshare(CLONE_NEWUSER|CLONE_NEWNS)", errno());
        }
        // `unshare(CLONE_NEWUSER)` leaves an empty id map and uid 65534 until
        // this process writes its own single-line map; without it the mount is
        // `EPERM` and the whole leg is meaningless.
        for (path, line) in [
            ("/proc/self/setgroups", "deny".to_string()),
            ("/proc/self/uid_map", format!("0 {uid} 1")),
            ("/proc/self/gid_map", format!("0 {gid} 1")),
        ] {
            if let Err(err) = fs::write(path, line) {
                return child_refused(11, path, err.raw_os_error());
            }
        }
        // SAFETY: the two path pointers are valid NUL-terminated strings and the
        // options pointer is a valid NUL-terminated string for the call; a
        // private mount namespace means no other process sees the mount.
        let mounted = unsafe {
            libc::mount(
                c"overlay".as_ptr(),
                merged_c.as_ptr(),
                c"overlay".as_ptr(),
                0,
                options_c.as_ptr().cast(),
            )
        };
        if mounted == -1 {
            return child_refused(12, "mount(overlay)", errno());
        }

        // `AT_HANDLE_FID` must succeed, and `flags = 0` must not: this is the
        // regression oracle for a watcher that stopped asking for the flag.
        if let Err(err) = name_to_handle(&merged_c, libc::AT_HANDLE_FID) {
            return child_refused(13, "name_to_handle_at(AT_HANDLE_FID)", err.raw_os_error());
        }
        match name_to_handle(&merged_c, 0) {
            Ok(()) => return child_refused(14, "name_to_handle_at(flags=0) succeeded", None),
            Err(err) if err.raw_os_error() == Some(libc::EOPNOTSUPP) => {}
            Err(err) => {
                return child_refused(15, "name_to_handle_at(flags=0)", err.raw_os_error());
            }
        }

        // The watcher must mark the overlay directory (which is only possible
        // through `AT_HANDLE_FID`) and resolve an event back to it.
        let mut watcher = match Watcher::new() {
            Ok(watcher) => watcher,
            Err(err) => {
                return child_refused(16, "Watcher::new", watcher_errno(&err));
            }
        };
        if let Err(err) = watcher.mark_dir(&merged) {
            return child_refused(17, "Watcher::mark_dir(overlay)", watcher_errno(&err));
        }
        let entry = merged.join("hello.py");
        if let Err(err) = fs::write(&entry, b"x") {
            return child_refused(18, "write through the overlay", err.raw_os_error());
        }
        let deadline = Instant::now() + EVENT_DEADLINE;
        let mut seen = Vec::new();
        loop {
            match watcher.read(&mut seen) {
                Ok(_) => {}
                Err(err) => return child_refused(19, "Watcher::read", watcher_errno(&err)),
            }
            if seen
                .iter()
                .any(|event| event.name == b"hello.py" && event.dir == merged)
            {
                return 0;
            }
            if Instant::now() >= deadline {
                return child_refused(20, "no resolvable overlay event", None);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
}

/// `name_to_handle_at(dir, flags)`, discarding the handle.
fn name_to_handle(path: &CString, flags: libc::c_int) -> io::Result<()> {
    /// `file_handle` plus a full payload, in one aligned buffer.
    #[repr(C, align(8))]
    struct Storage {
        handle_bytes: u32,
        handle_type: i32,
        handle: [u8; libc::MAX_HANDLE_SZ as usize],
    }
    let mut storage = Storage {
        handle_bytes: libc::MAX_HANDLE_SZ as u32,
        handle_type: 0,
        handle: [0; libc::MAX_HANDLE_SZ as usize],
    };
    let mut mount_id: libc::c_int = 0;
    // SAFETY: `storage` is `#[repr(C, align(8))]` with `file_handle`'s layout
    // (the crate's unit tests assert the offsets), `path` is NUL-terminated, and
    // the kernel writes only into the two pointers given.
    let probed = unsafe {
        libc::name_to_handle_at(
            libc::AT_FDCWD,
            path.as_ptr(),
            std::ptr::from_mut(&mut storage).cast::<libc::file_handle>(),
            &mut mount_id,
            flags,
        )
    };
    if probed == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The errno a [`Watcher`] failure names, classified or not.
///
/// The watcher's failures carry [`WatcherError`] *inside* the `io::Error`
/// (design §4.2), and an error built that way has no `raw_os_error()`: reading
/// only the raw errno would print "errno ?" for exactly the failures a user
/// needs to act on. Ask the typed detail first, then fall back to a plain
/// `io::Error` from a syscall that was never classified.
fn watcher_errno(err: &io::Error) -> Option<i32> {
    WatcherError::of(err)
        .map(WatcherError::errno)
        .or_else(|| err.raw_os_error())
}

/// Report a child's refusal on stderr and return its exit code.
///
/// `libc::write` (not `eprintln!`) so the body stays allocation-light and never
/// touches a lock another thread could have held at `fork` time.
fn child_refused(code: i32, what: &str, err: Option<i32>) -> i32 {
    let mut digits = [0u8; 10];
    let mut len = 0usize;
    if let Some(mut number) = err.map(|errno| u32::try_from(errno).unwrap_or(0)) {
        loop {
            if let Some(slot) = digits.get_mut(len) {
                *slot = b'0' + (number % 10) as u8;
                len += 1;
            }
            number /= 10;
            if number == 0 || len == digits.len() {
                break;
            }
        }
    }
    let mut reversed = [0u8; 10];
    for index in 0..len {
        if let (Some(source), Some(target)) = (digits.get(len - 1 - index), reversed.get_mut(index))
        {
            *target = *source;
        }
    }
    // SAFETY: four writes of this process's own stack buffers to stderr; the
    // lengths are the buffers' own lengths.
    unsafe {
        libc::write(2, what.as_ptr().cast(), what.len());
        if err.is_some() {
            libc::write(2, b": errno ".as_ptr().cast(), 8);
            libc::write(2, reversed.as_ptr().cast(), len);
        }
        libc::write(2, b"\n".as_ptr().cast(), 1);
    }
    code
}

/// Run `body` in a forked child and require a normal exit code 0.
///
/// Forking from the harness means the child is a copy of a multi-threaded
/// process. The child bodies here are short and mostly syscall-shaped, which is
/// what keeps that sound in practice — the same contract
/// `tests/critical_embedded_seccomp.rs` and `src/testutil.rs` keep.
fn in_child(label: &str, body: impl FnOnce() -> i32) {
    // SAFETY: the child is a copy of a multi-threaded process, so its body is
    // kept short and mostly syscall-shaped, holds no lock of ours, and ends in
    // `_exit` without unwinding.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => panic!("{label}: fork failed: {}", io::Error::last_os_error()),
        0 => {
            let code = body();
            // SAFETY: `_exit` never returns and runs no destructors.
            unsafe { libc::_exit(code) }
        }
        pid => {
            let mut status = 0;
            loop {
                // SAFETY: the status word is valid storage for this one child.
                let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
                if waited == pid {
                    break;
                }
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                panic!("{label}: waitpid failed: {err}");
            }
            assert!(
                libc::WIFEXITED(status),
                "{label}: the child died from signal {}",
                libc::WTERMSIG(status)
            );
            assert_eq!(
                libc::WEXITSTATUS(status),
                0,
                "{label}: the child refused (exit {}); its stderr names the call and the errno. \
                 This host must allow unprivileged user namespaces and an overlay mount on a \
                 tmpfs; run `make check` on the host if it does not.",
                libc::WEXITSTATUS(status)
            );
        }
    }
}
