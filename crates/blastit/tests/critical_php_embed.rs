//! `MB.3.13.4`: the `[route.php]` boot gate, the composed load, and the
//! embedded worker's containment — proved on a host with **no PHP at all**, no
//! Docker, and a stub ELF the test compiles with `cc`.
//!
//! `ADR-0010` §1 gives PHP exactly one backend and §3 puts the discovery in
//! stage 0 with **zero `dlopen`**, so the facts this suite pins are:
//!
//! * a `BLASTIT_LIBPHP` that is not a library, a ZTS (thread-safe) build, and a
//!   `libphp` whose inode moved between stage 0 and stage 1 each refuse the boot
//!   before a listener binds: exit `1`, and the sentence names the route, its
//!   `entry`, the distro package and `BLASTIT_LIBPHP`. The stage-0 refusals write
//!   nothing on stdout; a stage-1 refusal has already announced itself as an
//!   `app.output` event, so those legs assert "no `ready`/`spawned`/`request` and
//!   nothing listening" instead;
//! * **no stub is ever loaded while stage 0 is deciding.** Every stub carries an
//!   `__attribute__((constructor))` canary that appends the *pid and ppid* of the
//!   process that mapped it, so a canary that exists after a refusal is a
//!   `dlopen` in the boot; and a canary whose pid is the boot's own pid is a
//!   `libphp` above the worker (§3, `R11`). The ZTS leg is the proof: its stub
//!   *is* loadable, and stage 0 must still refuse it from `.dynsym` alone (a text
//!   file has no constructor, so the non-library leg makes no such claim);
//! * **the load happens only below the fork.** With the real pair the route
//!   answers `200`, init's `/proc/<pid>/maps` holds neither `libphp` nor
//!   `libblastit_php`, and a stub pair writes its canary with `ppid` equal to
//!   the boot's pid. The forked roles (`http`, `scheduler`, the view) are
//!   `PR_SET_DUMPABLE(0)`, so their maps are `EACCES` to every process —
//!   including this one — which is asserted rather than assumed;
//! * **`exit()`/`die()`/a fatal contain themselves in the worker.** One slot,
//!   two requests: the crash is answered with a `5xx` (never a `200`), is
//!   charged **at least the wall time its client waited** (`MB.3.13`'s own
//!   sentence, `R19`'s crash charge at one live thread) and is reported as a
//!   failed request — `outcome` is never `ok` — and the next request is served
//!   by the **only** live worker of that route — a new pid, a direct child of
//!   init, with the crashed process gone (`workers = 1` caps the slot, and the
//!   uid is one constant in single-UID `dev`, so it proves nothing). A third
//!   leg spends its 60 ms in `usleep`, so the CPU is a small fraction of the
//!   wall: a charge taken from `cpu_ns` cannot pass it, which is what makes the
//!   wall-charge assertion an oracle instead of a tautology.
//!
//! **Which outcome is required is decided by [`host_php`], an independent
//! probe**, exactly as `critical_php_backend.rs` does: raw `std` file access plus
//! the `ADR-0010` §4.1 names, never the code under test. A host with the pair
//! runs the containment and the maps legs for real; a host without it must
//! refuse with the package sentence. There is no branch on the boot's own exit
//! status, so a discovery regression cannot pass as "this host has no PHP".
//!
//! # CI coverage (`MB.3.13.4` review, follow-up for the CI owner)
//!
//! `make check` runs on a host without PHP, so every CI run exercises the refusal
//! branch **only**. The two real-pair legs (`no_libphp_is_mapped_above_the_view_worker`
//! and the `exit()`/fatal containment legs) need a host with a NTS `libphp` plus
//! `make php-sapi`; `.github/workflows/php-sapi.yml` is the only job that has
//! both (its image installs `php8.4-dev` + `libphp8.4-embed` and builds the
//! shim), but it runs the crate's probe and `make php-sapi-e2e`, never this
//! suite. Adding `cargo test -p blastit --locked --test critical_php_embed` to
//! that job (after its shim build) is what would cover them; until then the
//! containment leg has no CI witness.
//!
//! `cc` is a **hard requirement**: this suite builds its stub ELFs at test time,
//! so a missing compiler is a failure, never a skip.
//!
//! # The stage-1 probe child (`ADR-0010` §3, `MB.3.36`)
//!
//! Before anything listens, stage 1 forks one contained probe child per
//! `[route.php]` route, and that child `blastit_php_worker_init`s +
//! `blastit_php_worker_shutdown`s the pinned pair. So a shim whose
//! `blastit_php_minor()` disagrees with the host is a **boot** refusal
//! ([`a_shim_for_another_minor_refuses_the_boot_before_the_listener`]: exit `1`,
//! no `ready`/`spawned`/`request`, nothing listening), and a shim whose
//! `worker_init` never returns is killed at `BLASTIT_PROBE_TIMEOUT_MS`
//! ([`a_probe_that_hangs_is_killed_and_refuses_the_boot`]). The child is a fork
//! of init, so the canary still says the load happened only *below* the fork —
//! it no longer says nothing was loaded.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The route the fixture declares.
const ROUTE: &str = "app";

/// Its front controller, as `[route.php] entry` spells it.
const ENTRY: &str = "public/index.php";

/// `ADR-0010` §4.1's package half: the sentence every refusal carries.
const PACKAGES: &str = "install the PHP embed package";

/// How long a boot may take to bind, refuse, or answer.
const BOOT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long one request may take once the port accepts connections.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// One clock tick at `CONFIG_HZ = 1000`: below this a `0 ns` CPU reading is the
/// floor, not a broken measurement (`LESSONS.md`, `MB.3.12`).
const TICK_FLOOR_NS: u64 = 1_000_000;

/// How much smaller than the wall time a sleeping leg's CPU time must be for a
/// `cpu_ns`-based charge to be distinguishable from a wall-time one: at least a
/// twentieth, which a 60 ms `usleep` clears with two orders of magnitude to
/// spare (`MB.3.38`).
const CPU_SHARE: u64 = 20;

/// The documented page (`docs/frameworks/php.md`, `ADR-0010` §2): a `[route.php]`
/// route needs an `entry` and no `BLASTIT_APP`.
const PHP_MANIFEST: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"
";

/// One worker slot: the containment leg is about *that* slot surviving a crash.
const ONE_WORKER_MANIFEST: &str = "\
version = 1

[[route]]
name = \"app\"
path = ['.*']

[route.php]
entry = \"public/index.php\"
workers = 1
";

/// What the front controller echoes; the body the suite looks for.
const BODY: &str = "blastit-php-ok";

/// A script that answers normally.
const NORMAL_PHP: &str = "\
<?php
header('Content-Type: text/plain');
echo \"blastit-php-ok\\n\";
";

/// `exit()` deep enough into the script that `CLOCK_PROCESS_CPUTIME_ID` crosses
/// a tick (`LESSONS.md`, `MB.3.12`: a cheap script may legitimately report `0`).
const EXIT_PHP: &str = "\
<?php
for ($i = 0; $i < 500000; $i++) { $x = $i * $i; }
exit(0);
";

/// A contained fatal: `E_USER_ERROR` calls `zend_bailout()`.
const FATAL_PHP: &str = "\
<?php
for ($i = 0; $i < 500000; $i++) { $x = $i * $i; }
trigger_error('blastit fatal', E_USER_ERROR);
";

/// A contained bailout whose wall time is spent **sleeping**, so wall and CPU
/// cannot be confused: `usleep` parks the process for 60 ms and burns almost no
/// CPU doing it. A charge taken from `cpu_ns` is therefore a small fraction of
/// the wall time on this leg (~1/700 measured here), which is what makes
/// `cost_ns >= wall_ns` an oracle rather than a tautology: the `cpu_ns`-based
/// rule `MB.3.13.4` compared against fails it by orders of magnitude, not by a
/// few microseconds.
///
/// This is the same clock distinction `LESSONS.md` (`MB.3.12`) records from the
/// other side: a *fixture* must never use `usleep` when it needs CPU time,
/// because sleeping spends wall time, not CPU. That is exactly why it is the
/// right fixture for an assertion about the wall charge.
const SLEEP_PHP: &str = "\
<?php
usleep(60000);
exit(0);
";

// --------------------------------------------------------------- stub ELFs

/// The three sources this suite compiles. `BT_CANARY` and `BT_TAG` are `-D`
/// string literals, so the canary needs no environment at all — which matters,
/// because `blastit-env` strips every `BLASTIT_*` variable across the stage-1
/// `execve` (`ADR-0010` §3, `R12`).
const LIBPHP_STUB_C: &str = r#"
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

static void bt_canary(const char *tag)
{
    int fd = open(BT_CANARY, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd < 0) {
        return;
    }
    char line[96];
    int n = snprintf(line, sizeof line, "%s pid=%d ppid=%d\n",
                     tag, (int)getpid(), (int)getppid());
    if (n > 0) {
        ssize_t ignored = write(fd, line, (size_t)n);
        (void)ignored;
    }
    close(fd);
}

__attribute__((constructor)) static void bt_libphp_canary(void) { bt_canary(BT_TAG); }

/* What stage 0 reads from .dynsym: a NTS build exports the executor globals and
 * no TSRM. `php_version` is what the worker's loader asks for after `dlopen`. */
int executor_globals = 0;
const char *php_version(void) { return BT_VERSION; }
"#;

/// The ZTS twin: `tsrm_startup` is the one symbol `ADR-0010` §3 says must be
/// absent, and stage 0 must refuse on it without loading the file.
const ZTS_STUB_C: &str = r#"
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

static void bt_canary(const char *tag)
{
    int fd = open(BT_CANARY, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd < 0) {
        return;
    }
    char line[96];
    int n = snprintf(line, sizeof line, "%s pid=%d ppid=%d\n",
                     tag, (int)getpid(), (int)getppid());
    if (n > 0) {
        ssize_t ignored = write(fd, line, (size_t)n);
        (void)ignored;
    }
    close(fd);
}

__attribute__((constructor)) static void bt_zts_canary(void) { bt_canary(BT_TAG); }

int executor_globals = 0;
int tsrm_startup(void) { return 0; }
const char *php_version(void) { return BT_VERSION; }
"#;

/// The shim: the five symbols the worker resolves, with `BT_MINOR` the code
/// `blastit_php_minor()` reports. A mismatch is `BT_MINOR` disagreeing with the
/// `php_version()` the stub above returns.
const SHIM_STUB_C: &str = r#"
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

static void bt_canary(const char *tag)
{
    int fd = open(BT_CANARY, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (fd < 0) {
        return;
    }
    char line[96];
    int n = snprintf(line, sizeof line, "%s pid=%d ppid=%d\n",
                     tag, (int)getpid(), (int)getppid());
    if (n > 0) {
        ssize_t ignored = write(fd, line, (size_t)n);
        (void)ignored;
    }
    close(fd);
}

__attribute__((constructor)) static void bt_shim_canary(void) { bt_canary(BT_TAG); }

unsigned blastit_php_abi_version(void) { return 1u; }
unsigned blastit_php_minor(void) { return BT_MINOR; }
int blastit_php_worker_init(const void *config) { (void)config; return 0; }
int blastit_php_request(const void *request, const void *sink, void *out)
{
    (void)request; (void)sink; (void)out;
    return -1;
}
void blastit_php_worker_shutdown(void) {}
"#;

/// A shim that reports its own containment and then never returns: the leg that
/// proves the stage-1 probe's timeout (`ADR-0010` §3, `BLASTIT_PROBE_TIMEOUT_MS`).
///
/// It writes two lines to `BT_SHAPE` — the `dlopen` canary, and the shape of the
/// process `worker_init` runs in — so the suite can check the containment the
/// timeout killed instead of trusting it: `pid`/`ppid` place the load below the
/// fork, `pgid == pid` is the child's own process group, `uid`/`euid` are the
/// route's, and `inet` is `EPERM` because `EmbeddedView { egress: false }`
/// refuses `socket(AF_INET)`.
const HANG_SHIM_STUB_C: &str = r#"
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static void bt_note(const char *line)
{
    int fd = open(BT_SHAPE, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0600);
    if (fd < 0) {
        return;
    }
    ssize_t ignored = write(fd, line, strlen(line));
    (void)ignored;
    close(fd);
}

__attribute__((constructor)) static void bt_hang_canary(void)
{
    char line[96];
    int n = snprintf(line, sizeof line, "loaded pid=%d ppid=%d\n",
                     (int)getpid(), (int)getppid());
    if (n > 0) {
        bt_note(line);
    }
}

unsigned blastit_php_abi_version(void) { return 1u; }
unsigned blastit_php_minor(void) { return 804; }

int blastit_php_worker_init(const void *config)
{
    (void)config;
    int inet = socket(AF_INET, SOCK_STREAM, 0);
    int refused = inet < 0 ? errno : 0;
    if (inet >= 0) {
        close(inet);
    }
    char line[192];
    int n = snprintf(line, sizeof line,
                     "probe pid=%d ppid=%d pgid=%d uid=%d euid=%d inet=%d\n",
                     (int)getpid(), (int)getppid(), (int)getpgid(0),
                     (int)getuid(), (int)geteuid(), refused);
    if (n > 0) {
        bt_note(line);
    }
    for (;;) {
        pause();
    }
}

int blastit_php_request(const void *request, const void *sink, void *out)
{
    (void)request; (void)sink; (void)out;
    return -1;
}
void blastit_php_worker_shutdown(void) {}
"#;

/// The shim's `libblastit_php<major><minor>-<arch>-<libc>.so` spelling
/// (`ADR-0010` §4.1) for this build's target.
fn shim_name(minor: u32) -> String {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    };
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    format!("libblastit_php{minor}-{arch}-{libc}.so")
}

/// Every stub ELF this suite compiles, resolved once per test binary.
///
/// `Clone` because [`stubs`] hands out a copy: the fixture is **rebuildable**, so
/// a test that runs after the shared root was removed (which is what
/// `--test-threads=1` does at the end of every test) gets a fresh build instead
/// of paths that no longer exist.
#[derive(Clone)]
struct Stubs {
    /// The root all of this build's stub directories live under.
    root: PathBuf,
    /// `libphp8.4.so` exporting `executor_globals` and `php_version`.
    nts_libphp: PathBuf,
    /// The `84` shim reporting `804`: the accepted pair.
    nts_shim: PathBuf,
    /// The NTS stub's canary.
    nts_libphp_canary: PathBuf,
    /// The `84` shim's canary.
    nts_shim_canary: PathBuf,
    /// `libphp8.4.so` exporting `tsrm_startup`.
    zts_libphp: PathBuf,
    /// The ZTS stub's canary.
    zts_canary: PathBuf,
    /// `libphp8.4.so` for the mismatch leg.
    mismatch_libphp: PathBuf,
    /// A shim *named* `84` that reports `803`.
    mismatch_shim: PathBuf,
    /// Their canaries.
    mismatch_libphp_canary: PathBuf,
    /// The mismatch shim's canary.
    mismatch_shim_canary: PathBuf,
    /// `libphp8.4.so` for the timeout leg.
    hang_libphp: PathBuf,
    /// A shim named `84` whose `worker_init` never returns.
    hang_shim: PathBuf,
    /// Where that shim writes the shape of the child it hangs in.
    hang_shape: PathBuf,
}

/// The stub set, built on first use and **rebuilt whenever its root is gone**.
///
/// The root is removed when the last live test of a run exits, and a
/// `--test-threads=1` run looks exactly like a one-test run to that counter, so
/// a one-shot cache would hand the next test paths that no longer exist and fail
/// `NotFound` on the first CI run (which runs every binary serially). The cache
/// therefore rebuilds when [`stub_root`] is absent, under the lock
/// [`StubRootGuard`] also takes to delete the root — the cache and the directory
/// are one invariant.
///
/// Every caller holds a [`StubRootGuard`], so the counter can never reach zero
/// (and delete the root) while a test is inside this function.
fn stubs() -> Stubs {
    let mut slot = STUBS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // The cached paths are only usable while their root is there: a serialized
    // run deletes it at the end of *every* test.
    if slot.as_ref().is_none_or(|set| !set.root.is_dir()) {
        *slot = Some(build_stubs());
    }
    slot.clone().expect("the stub set was built or reused")
}

/// The directory every stub of this test binary lives under.
fn stub_root() -> PathBuf {
    std::env::temp_dir().join(format!("blastit-php-embed-stubs-{}", std::process::id()))
}

/// How many tests of this binary are running right now.
static LIVE_TESTS: AtomicUsize = AtomicUsize::new(0);

/// The cached stub set, shared with [`StubRootGuard`].
///
/// One lock guards both the cache and the root: `stubs()` holds it while it
/// stats the root and clones the paths, and the last guard holds it while it
/// deletes the root and empties the cache, so the two can never interleave
/// (a thread inside `stubs()` cannot stat a present root that is then deleted
/// under its clone, and a deleting guard cannot race a rebuild).
static STUBS: Mutex<Option<Stubs>> = Mutex::new(None);

/// Removes the shared stub root when the **last** test that was running exits.
///
/// A `Drop` on the cached value never runs (it lives in a static), so the guard
/// is the thing that is actually dropped — by the test's own frame, on the way
/// out of a pass *or* a panic — and the last guard to fall takes the directory
/// with it.
struct StubRootGuard;

impl StubRootGuard {
    /// Registers this test as running; the stubs are cleaned up by whoever
    /// leaves last.
    fn start() -> Self {
        LIVE_TESTS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for StubRootGuard {
    fn drop(&mut self) {
        if LIVE_TESTS.fetch_sub(1, Ordering::SeqCst) == 1 {
            // The same lock `stubs()` takes: the cache and the directory are one
            // invariant, and the cache must not survive the directory it names.
            let mut slot = STUBS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = std::fs::remove_dir_all(stub_root());
            *slot = None;
        }
    }
}

/// Compiles the stub ELFs, or fails: `cc` is not optional here.
fn build_stubs() -> Stubs {
    let root = stub_root();
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["nts", "zts", "mismatch", "hang"] {
        std::fs::create_dir_all(root.join(dir)).expect("a stub directory");
    }
    std::fs::write(root.join("libphp_stub.c"), LIBPHP_STUB_C).expect("writing the stub source");
    std::fs::write(root.join("zts_stub.c"), ZTS_STUB_C).expect("writing the ZTS source");
    std::fs::write(root.join("shim_stub.c"), SHIM_STUB_C).expect("writing the shim source");
    std::fs::write(root.join("hang_shim_stub.c"), HANG_SHIM_STUB_C)
        .expect("writing the hanging shim source");

    let nts_libphp = root.join("nts/libphp8.4.so");
    let nts_shim = root.join("nts").join(shim_name(84));
    let zts_libphp = root.join("zts/libphp8.4.so");
    let mismatch_libphp = root.join("mismatch/libphp8.4.so");
    let mismatch_shim = root.join("mismatch").join(shim_name(84));
    let hang_libphp = root.join("hang/libphp8.4.so");
    let hang_shim = root.join("hang").join(shim_name(84));

    compile(
        &root.join("libphp_stub.c"),
        &nts_libphp,
        &[
            (
                "BT_CANARY",
                quote(&root.join("nts/libphp.canary").display().to_string()),
            ),
            ("BT_TAG", quote("libphp")),
            ("BT_VERSION", quote("8.4.27")),
        ],
    );
    compile(
        &root.join("shim_stub.c"),
        &nts_shim,
        &[
            (
                "BT_CANARY",
                quote(&root.join("nts/shim.canary").display().to_string()),
            ),
            ("BT_TAG", quote("shim84")),
            ("BT_MINOR", "804".to_string()),
        ],
    );
    compile(
        &root.join("zts_stub.c"),
        &zts_libphp,
        &[
            (
                "BT_CANARY",
                quote(&root.join("zts/libphp.canary").display().to_string()),
            ),
            ("BT_TAG", quote("zts")),
            ("BT_VERSION", quote("8.4.27")),
        ],
    );
    compile(
        &root.join("libphp_stub.c"),
        &mismatch_libphp,
        &[
            (
                "BT_CANARY",
                quote(&root.join("mismatch/libphp.canary").display().to_string()),
            ),
            ("BT_TAG", quote("libphp")),
            ("BT_VERSION", quote("8.4.27")),
        ],
    );
    compile(
        &root.join("shim_stub.c"),
        &mismatch_shim,
        &[
            (
                "BT_CANARY",
                quote(&root.join("mismatch/shim.canary").display().to_string()),
            ),
            ("BT_TAG", quote("shim83")),
            ("BT_MINOR", "803".to_string()),
        ],
    );
    compile(
        &root.join("libphp_stub.c"),
        &hang_libphp,
        &[
            (
                "BT_CANARY",
                quote(&root.join("hang/libphp.canary").display().to_string()),
            ),
            ("BT_TAG", quote("libphp")),
            ("BT_VERSION", quote("8.4.27")),
        ],
    );
    compile(
        &root.join("hang_shim_stub.c"),
        &hang_shim,
        &[(
            "BT_SHAPE",
            quote(&root.join("hang/shape").display().to_string()),
        )],
    );

    let canaries = {
        let root = root.clone();
        move |name: &str| root.join(name)
    };
    Stubs {
        root,
        nts_libphp,
        nts_shim,
        nts_libphp_canary: canaries("nts/libphp.canary"),
        nts_shim_canary: canaries("nts/shim.canary"),
        zts_libphp,
        zts_canary: canaries("zts/libphp.canary"),
        mismatch_libphp,
        mismatch_shim,
        mismatch_libphp_canary: canaries("mismatch/libphp.canary"),
        mismatch_shim_canary: canaries("mismatch/shim.canary"),
        hang_libphp,
        hang_shim,
        hang_shape: canaries("hang/shape"),
    }
}

/// A `-D` string literal for the compiler.
fn quote(value: &str) -> String {
    format!("\"{value}\"")
}

/// `cc -shared -fPIC` of `source` into `out`, with the given `-D` defines.
///
/// A missing compiler or a failed compile is a panic: the stub ELFs are the
/// fixture, and a suite that skipped them would assert nothing.
fn compile(source: &Path, out: &Path, defines: &[(&str, String)]) {
    let mut command = Command::new("cc");
    command.args(["-shared", "-fPIC", "-O0", "-o"]);
    command.arg(out).arg(source).arg("-lc");
    for (name, value) in defines {
        command.arg(format!("-D{name}={value}"));
    }
    let output = command.output().unwrap_or_else(|err| {
        panic!(
            "this suite compiles its stub ELFs with `cc`, and `cc` could not be run: {err}; \
             install a C toolchain (this is a hard failure, never a skip)"
        )
    });
    assert!(
        output.status.success(),
        "cc failed for {}: {}",
        out.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ------------------------------------------------------------- host probe

/// The host's NTS `libphp` and the shim for its tuple, resolved **without** the
/// code under test: raw `std` file access, the `ADR-0010` §4.1 candidate names,
/// the `.dynsym` strings read as bytes, and the shim's file-name spelling.
struct HostPhp {
    /// The real file a boot is handed (`canonicalize`d).
    libphp: PathBuf,
    /// The shim for its `(major, minor)`.
    shim: PathBuf,
}

/// The library and shim this host can embed, or `None`.
fn host_php() -> Option<HostPhp> {
    let libphp = libphp_path()?;
    // A `.dynsym` name is a plain string in the file; reading the bytes beats
    // duplicating an ELF walk, and it is still independent of `blastit`.
    let bytes = std::fs::read(&libphp).ok()?;
    if !contains(&bytes, b"executor_globals") || contains(&bytes, b"tsrm_startup") {
        return None;
    }
    let (major, minor) = minor_of(&libphp)?;
    let shim = shim_path(major, minor)?;
    Some(HostPhp { libphp, shim })
}

/// Whether `haystack` holds `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// `BLASTIT_LIBPHP` when it is a file, else the distro's fixed names, newest
/// first, in the multi-arch directories (`ADR-0010` §3).
fn libphp_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BLASTIT_LIBPHP") {
        let path = PathBuf::from(path);
        return path.is_file().then(|| path.canonicalize().ok()).flatten();
    }
    const NAMES: [&str; 7] = [
        "libphp8.4.so",
        "libphp-8.4.so",
        "libphp8.3.so",
        "libphp-8.3.so",
        "libphp8.2.so",
        "libphp-8.2.so",
        "libphp.so",
    ];
    let triplet = format!("{}-linux-gnu", std::env::consts::ARCH);
    let dirs = [
        format!("/usr/lib/{triplet}"),
        "/usr/lib64".to_string(),
        "/usr/lib".to_string(),
        format!("/usr/local/lib/{triplet}"),
        "/usr/local/lib".to_string(),
    ];
    NAMES
        .iter()
        .flat_map(|name| dirs.iter().map(move |dir| Path::new(dir).join(name)))
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

/// The `(major, minor)` a `libphp<name>` states: `libphp8.4.so`, `libphp-8.4.so`.
fn minor_of(path: &Path) -> Option<(u32, u32)> {
    let name = path.file_name()?.to_str()?.strip_prefix("libphp")?;
    let mut rest = name.strip_prefix('-').unwrap_or(name);
    let major = digits(&mut rest)?;
    rest = rest.strip_prefix('.')?;
    let minor = digits(&mut rest)?;
    Some((major, minor))
}

/// The leading decimal run, consumed from `rest`.
fn digits(rest: &mut &str) -> Option<u32> {
    let len = rest.bytes().take_while(u8::is_ascii_digit).count();
    let value = rest.get(..len)?.parse().ok()?;
    *rest = rest.get(len..)?;
    Some(value)
}

/// `libblastit_php<major><minor>-<arch>-<libc>.so` in the directories a `dev`
/// boot looks in: `BLASTIT_PHP_SHIM_DIR`, the executable's directory, then the
/// `target/php-sapi` that `make php-sapi` writes.
fn shim_path(major: u32, minor: u32) -> Option<PathBuf> {
    let name = shim_name(major * 10 + minor);
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_blastit"));
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("BLASTIT_PHP_SHIM_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = exe.parent() {
        dirs.push(dir.to_path_buf());
    }
    if let Some(target) = exe
        .ancestors()
        .find(|dir| dir.file_name() == Some(std::ffi::OsStr::new("target")))
    {
        dirs.push(target.join("php-sapi"));
    }
    dirs.into_iter()
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

// ---------------------------------------------------------------- fixture

/// A temp directory holding one case's `blastit.toml`, its front controller and
/// (for the stub legs) the stub pair, removed on drop.
struct Scratch {
    /// The directory `blastit dev` is started in.
    dir: PathBuf,
}

impl Scratch {
    /// Creates `dir` with `blastit.toml` written.
    fn new(name: &str, manifest: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("blastit-php-embed-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("public")).expect("a fixture directory");
        std::fs::write(dir.join("blastit.toml"), manifest).expect("the manifest");
        Self { dir }
    }

    /// Writes `contents` to `relative`, creating its parent.
    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a parent directory");
        }
        std::fs::write(&path, contents).expect("a fixture file");
        path
    }

    /// Copies `from` to `relative`, returning the destination.
    fn copy(&self, from: &Path, relative: &str) -> PathBuf {
        let path = self.dir.join(relative);
        std::fs::copy(from, &path).expect("copying a stub");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ----------------------------------------------------------------- a boot

/// A rename to perform the instant stage 0 announces the pair it inspected: the
/// only way to land a swap between stage 0's `fstat` and stage 1's re-open.
struct Swap {
    /// The line that means stage 0 has recorded `(dev, ino)` and moved on.
    needle: &'static str,
    /// The file to rename over `to`.
    from: PathBuf,
    /// The `libphp` path stage 1 will re-open.
    to: PathBuf,
}

/// A `blastit dev` boot under test: its process, both streams, and its port.
struct Boot {
    child: Child,
    /// Every stdout line, in order (JSON events under `BLASTIT_LOG_FORMAT=json`).
    stdout: Arc<Mutex<Vec<String>>>,
    /// Everything stderr has said.
    stderr: Arc<Mutex<Vec<String>>>,
    readers: Vec<JoinHandle<()>>,
    /// Whether the swap was performed, when one was asked for.
    swapped: Option<Arc<AtomicBool>>,
    /// One boot at a time: an ephemeral port handed back by one test must not be
    /// bound by the next boot before this one has bound it.
    #[allow(dead_code)]
    serial: std::sync::MutexGuard<'static, ()>,
    port: u16,
}

/// The process-wide lock every boot takes, poisoning-proof: a failing test must
/// not turn every later test into a lock failure.
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Boot {
    /// Starts a boot in `dir` with `env` on top of an empty environment.
    ///
    /// `env_clear` is what a developer's shell is not: every `BLASTIT_*` a test
    /// needs is named here, so nothing ambient — in particular no
    /// `BLASTIT_LIBPHP` — can make the probe and the boot disagree.
    fn start(dir: &Path, env: &[(&str, &str)]) -> Self {
        Self::launch(dir, env, None)
    }

    /// The same, renaming `swap.from` over `swap.to` the instant `swap.needle`
    /// appears on stderr.
    fn start_swapping(dir: &Path, env: &[(&str, &str)], swap: Swap) -> Self {
        Self::launch(dir, env, Some(swap))
    }

    fn launch(dir: &Path, env: &[(&str, &str)], swap: Option<Swap>) -> Self {
        let serial = boot_lock();
        // A port nobody listens on: `dev` binds it, and a refusal never gets
        // that far.
        let held = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let port = held.local_addr().expect("the port").port();
        drop(held);

        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .env_clear()
            // A loaded host must not turn this into the memory-watermark refusal.
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .env("BLASTIT_LOG_FORMAT", "json")
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawning `blastit dev`");

        let stdout_lines = Arc::new(Mutex::new(Vec::new()));
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        let swapped = swap.as_ref().map(|_| Arc::new(AtomicBool::new(false)));

        let collected = Arc::clone(&stdout_lines);
        let stdout = child.stdout.take().expect("stdout");
        let readers = vec![std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                collected.lock().expect("the stdout buffer").push(line);
            }
        })];

        let collected = Arc::clone(&stderr_lines);
        let trigger = swap.map(|swap| (swap, Arc::clone(swapped.as_ref().expect("a swap flag"))));
        let stderr = child.stderr.take().expect("stderr");
        let mut readers = readers;
        readers.push(std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                collected
                    .lock()
                    .expect("the stderr buffer")
                    .push(line.clone());
                if let Some((swap, done)) = &trigger
                    && line.contains(swap.needle)
                    && !done.swap(true, Ordering::SeqCst)
                {
                    let _ = std::fs::rename(&swap.from, &swap.to);
                }
            }
        }));

        Self {
            child,
            stdout: stdout_lines,
            stderr: stderr_lines,
            readers,
            swapped,
            serial,
            port,
        }
    }

    /// The boot's own pid (it `execve`s into stage 1 in place, so it is init's).
    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Waits for the boot to either accept a connection or exit.
    ///
    /// A gate that refuses never binds — every §4.1 gate runs before the
    /// listener — so "the port accepts" and "the process is gone" is a complete
    /// partition of the two `ADR-0010` §1 outcomes.
    fn await_boot(&mut self) -> Option<Option<i32>> {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("waiting for the boot") {
                return Some(status.code());
            }
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return None;
            }
            assert!(
                Instant::now() < deadline,
                "`blastit dev` neither served nor refused within {BOOT_TIMEOUT:?}:\n{}\n{}",
                self.stderr_text(),
                self.stdout_text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `GET <path>` over a fresh connection, as the raw response.
    fn get(&self, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connecting");
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .expect("a read timeout");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .expect("writing the request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("the response");
        String::from_utf8_lossy(&response).into_owned()
    }

    /// Every stdout line parsed as an event, ignoring anything else.
    fn events(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lock()
            .expect("the stdout buffer")
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect()
    }

    /// The first event matching `wanted`, waiting up to `within` for it.
    fn wait_event<F>(&self, within: Duration, wanted: F) -> Option<serde_json::Value>
    where
        F: Fn(&serde_json::Value) -> bool,
    {
        let deadline = Instant::now() + within;
        loop {
            if let Some(event) = self.events().into_iter().find(|event| wanted(event)) {
                return Some(event);
            }
            if Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The `request` event for `path`.
    fn request_event(&self, path: &str) -> serde_json::Value {
        self.wait_event(REQUEST_TIMEOUT, |event| {
            event["event"] == "request" && event["path"] == path
        })
        .unwrap_or_else(|| panic!("no `request` event for {path}: {}", self.stdout_text()))
    }

    /// The pids forked by init, from `/proc/<pid>/task/<pid>/children`.
    fn children(&self) -> Vec<u32> {
        let path = format!("/proc/{}/task/{}/children", self.pid(), self.pid());
        let Ok(text) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        text.split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect()
    }

    /// Whether the requested swap actually happened.
    fn swapped(&self) -> bool {
        self.swapped
            .as_ref()
            .is_some_and(|done| done.load(Ordering::SeqCst))
    }

    fn stdout_text(&self) -> String {
        self.stdout.lock().expect("the stdout buffer").join("\n")
    }

    fn stderr_text(&self) -> String {
        self.stderr.lock().expect("the stderr buffer").join("\n")
    }

    /// Waits for the readers to reach end of file (the process is gone).
    fn join_readers(&mut self) {
        for reader in std::mem::take(&mut self.readers) {
            let _ = reader.join();
        }
    }

    /// Stops the boot: `SIGTERM` first (so `dev` removes its runtime root and
    /// drains its children), then `SIGKILL`, then the readers are joined.
    fn stop(&mut self) {
        if self.child.try_wait().expect("the boot's state").is_none() {
            let _ = Command::new("kill")
                .args(["-TERM", &self.pid().to_string()])
                .status();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if self.child.try_wait().expect("the boot's state").is_some() {
                    break;
                }
                if Instant::now() > deadline {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.join_readers();
    }
}

impl Drop for Boot {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// --------------------------------------------------------------- assertions

/// The refusal a **stage-1** boot prints, as raw text.
///
/// Stage 1's `fail` emits each line as an `app.output` event on stdout
/// (`logs::fail`, `LESSONS.md` `MB.1.6`), where the quotes of
/// `route "app"` arrive JSON-escaped — so the sentence is read back out of the
/// events rather than grepped for in the encoded line.
fn refusal_text(boot: &Boot) -> String {
    boot.events()
        .into_iter()
        .filter(|event| event["event"] == "app.output")
        .filter_map(|event| event["line"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `ADR-0010` §4.1's sentence shape, for the stage-0 refusals: the route, its
/// `entry`, the package and the override.
fn assert_package_sentence(text: &str, what: &str) {
    assert!(
        text.contains(&format!("route {ROUTE:?}")),
        "{what}: the route is named: {text}"
    );
    assert!(
        text.contains(&format!("[route.php] entry {ENTRY:?}")),
        "{what}: the entry is named: {text}"
    );
    assert!(
        text.contains(PACKAGES),
        "{what}: the package sentence is the fix: {text}"
    );
    assert!(
        text.contains("BLASTIT_LIBPHP"),
        "{what}: the override is named, so an out-of-tree library is reachable: {text}"
    );
}

/// One backend, one sentence (`ADR-0010` §1): a refusal must not name an
/// alternative transport — even to deny it — because that reads as a recipe.
///
/// `libphp` is deliberately allowed: `BLASTIT_LIBPHP` is the name an operator
/// has to set, so every §4.1 sentence contains it.
fn assert_no_fallback_recipe(text: &str) {
    let lower = text.to_ascii_lowercase();
    for banned in ["php-fpm", "fastcgi", "route.fastcgi", "wsgi", "meanwhile"] {
        assert!(
            !lower.contains(banned),
            "the refusal names a fallback recipe ({banned}): {text}"
        );
    }
}

/// A boot that got past the gates emits `boot`/`ready`; a refusal emits neither
/// and leaves no listener behind.
fn assert_no_serving_surface(boot: &mut Boot, what: &str) {
    let events = boot.events();
    for name in ["ready", "process.spawned", "request"] {
        assert!(
            !events.iter().any(|event| event["event"] == name),
            "{what}: a refused boot must not emit `{name}`: {}",
            boot.stdout_text()
        );
    }
    assert!(
        TcpStream::connect(("127.0.0.1", boot.port)).is_err(),
        "{what}: a refused boot left something listening on port {}",
        boot.port
    );
}

/// The canary evidence: every line a stub wrote, as `(pid, ppid)`.
fn canary_writes(path: &Path) -> Vec<(u32, u32)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut pid = None;
            let mut ppid = None;
            for field in line.split_whitespace() {
                if let Some(value) = field.strip_prefix("pid=") {
                    pid = value.parse().ok();
                }
                if let Some(value) = field.strip_prefix("ppid=") {
                    ppid = value.parse().ok();
                }
            }
            Some((pid?, ppid?))
        })
        .collect()
}

/// Removes a canary so the next boot's writes are the only ones in the file.
fn clear(canary: &Path) {
    let _ = std::fs::remove_file(canary);
}

/// `PPid:` from `/proc/<pid>/status`.
///
/// `status` stays readable for a `PR_SET_DUMPABLE(0)` process (only `maps`,
/// `environ` and `mem` are restricted), so this is how a *worker* can be tied to
/// the boot that forked it. `None` when the process is gone.
fn ppid_of(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("PPid:")
            .and_then(|value| value.trim().parse().ok())
    })
}

/// This process's effective uid, from `/proc/self/status` (`Uid: real effective
/// saved fs`).
///
/// Root can read a `PR_SET_DUMPABLE(0)` role's maps, so a test that asserts
/// `EACCES` on them has to know whether it is root — and `ci.yml` runs every
/// test binary as uid 0. Standard library only: there is no `geteuid` here.
fn effective_uid() -> u32 {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|fields| fields.split_whitespace().nth(1)?.parse().ok())
        .unwrap_or(u32::MAX)
}

/// Stage 0 must never load what it inspects (`ADR-0010` §3, `R11`): a canary
/// that exists after a refusal is a `dlopen` in the boot.
fn assert_never_loaded(canaries: &[&Path], what: &str) {
    for canary in canaries {
        let writes = canary_writes(canary);
        assert!(
            writes.is_empty(),
            "{what}: {} holds {writes:?}, so a stub was loaded before the boot's gate: \
             `dlopen` belongs to the view worker alone (ADR-0010 §3)",
            canary.display()
        );
    }
}

/// A canary is proof of the *fork*, not of the boot, only when every writer is a
/// different process whose parent is the boot.
fn assert_loaded_only_below_the_fork(canaries: &[&Path], init: u32, what: &str) -> Vec<(u32, u32)> {
    let mut writers = Vec::new();
    for canary in canaries {
        writers.extend(canary_writes(canary));
    }
    assert!(
        !writers.is_empty(),
        "{what}: no stub was loaded at all, so nothing proves the load happened in a view"
    );
    for (pid, ppid) in &writers {
        assert_ne!(
            *pid, init,
            "{what}: the boot itself ({init}) mapped a stub; the load belongs to the forked view"
        );
        assert_eq!(
            *ppid, init,
            "{what}: a stub was mapped by pid {pid}, whose parent is {ppid} and not the boot \
             ({init}); only a forked view may load it"
        );
    }
    writers
}

// ------------------------------------------------------------------- tests

/// The denied twin that every host can run: a `BLASTIT_LIBPHP` that is not a
/// library is stage 0's `.dynsym` walk refusing a file it never loads.
///
/// A text file carries no `__attribute__((constructor))`, so no canary can
/// observe a `dlopen` of **this** file: the zero-`dlopen` proof belongs to
/// [`a_zts_libphp_refuses_before_anything_loads`], whose stub *is* loadable and
/// therefore must never be loaded. This leg asserts the refusal and its sentence
/// only, and it says so rather than checking a canary that nothing could write.
#[test]
fn a_non_library_blastit_libphp_refuses_before_anything_loads() {
    let _root = StubRootGuard::start();
    let scratch = Scratch::new("not-a-library", PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    let text = scratch.write("libphp8.4.so", "this is not an ELF shared object");

    let mut boot = Boot::start(&scratch.dir, &[("BLASTIT_LIBPHP", &text.to_string_lossy())]);
    let code = boot
        .await_boot()
        .expect("a boot with a non-library BLASTIT_LIBPHP must refuse, not serve");
    boot.stop();
    let stdout = boot.stdout_text();
    let stderr = boot.stderr_text();

    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert_package_sentence(&stderr, "not a library");
    assert!(
        stderr.contains("is not an ELF shared object"),
        "the refusal says what the file is: {stderr}"
    );
    assert!(
        stderr.contains(&text.display().to_string()),
        "the refusal names the file the operator pointed at: {stderr}"
    );
    assert_no_fallback_recipe(&stderr);
    assert_no_serving_surface(&mut boot, "not a library");
    println!("refused (stage 0; the zero-dlopen proof is the ZTS leg): {stderr}");
}

/// A ZTS build must be refused from `.dynsym` alone (`tsrm_startup` exported):
/// loading it to ask would be the very bug this gate exists to prevent.
///
/// This is the suite's zero-`dlopen` proof: the file *is* a loadable shared
/// object whose constructor writes a canary, and stage 0 must decide against it
/// without ever mapping it.
#[test]
fn a_zts_libphp_refuses_before_anything_loads() {
    let _root = StubRootGuard::start();
    let stubs = stubs();
    clear(&stubs.zts_canary);
    let scratch = Scratch::new("zts", PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    let libphp = scratch.copy(&stubs.zts_libphp, "libphp8.4.so");

    let mut boot = Boot::start(
        &scratch.dir,
        &[("BLASTIT_LIBPHP", &libphp.to_string_lossy())],
    );
    let code = boot
        .await_boot()
        .expect("a ZTS libphp must refuse the boot, not serve");
    boot.stop();
    let stdout = boot.stdout_text();
    let stderr = boot.stderr_text();

    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    assert_package_sentence(&stderr, "ZTS");
    assert!(
        stderr.contains("is a ZTS (thread-safe) libphp"),
        "the refusal names the ABI and the fix: {stderr}"
    );
    assert!(
        stderr.contains("(PHP 8.4, ") && stderr.contains(&shim_name(84)),
        "the version-specific shape names the tuple and its shim: {stderr}"
    );
    assert_no_fallback_recipe(&stderr);
    assert_no_serving_surface(&mut boot, "ZTS");
    assert_never_loaded(&[&stubs.zts_canary], "ZTS");
    println!("refused (stage 0, zero dlopen): {stderr}");
}

/// A shim whose `blastit_php_minor()` disagrees with the host refuses the boot
/// before the listener binds (`ADR-0010` §3).
///
/// This is the shape that used to be a first-request `500`: the pair loads, the
/// shim reports `PHP 8.3` for a `8.4.27` library, and the stage-1 probe child
/// says so — `exit 1`, no `ready`/`spawned`/`request`, and nothing listening on
/// the port. A boot that served a route here would read `EG(bailout)` at the
/// wrong offset and turn `exit()` into a `SIGSEGV`.
///
/// The load itself still happens **only below the fork**: the probe child is a
/// fork of init, so both canaries name a process whose parent is the boot
/// (stage 1 prints on stdout, so the old "nothing on stdout" assertion does not
/// belong to this leg).
#[test]
fn a_shim_for_another_minor_refuses_the_boot_before_the_listener() {
    let _root = StubRootGuard::start();
    let stubs = stubs();
    clear(&stubs.mismatch_libphp_canary);
    clear(&stubs.mismatch_shim_canary);
    let scratch = Scratch::new("mismatch", PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    let libphp = scratch.copy(&stubs.mismatch_libphp, "libphp8.4.so");
    scratch.copy(&stubs.mismatch_shim, &shim_name(84));

    let mut boot = Boot::start(
        &scratch.dir,
        &[
            ("BLASTIT_LIBPHP", &libphp.to_string_lossy()),
            ("BLASTIT_PHP_SHIM_DIR", &scratch.dir.to_string_lossy()),
        ],
    );
    let init = boot.pid();
    let code = boot
        .await_boot()
        .expect("a shim of another minor must refuse the boot, not serve a route");
    boot.stop();
    let stdout = boot.stdout_text();
    let stderr = boot.stderr_text();
    let all = format!("{stdout}\n{stderr}");

    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    let refusal = refusal_text(&boot);
    assert_package_sentence(&refusal, "shim minor");
    assert_no_fallback_recipe(&refusal);
    assert!(
        all.contains("the shim is built for PHP 8.3"),
        "the refusal names the shim's minor: {all}"
    );
    assert!(
        all.contains("reports 8.4.27"),
        "and the host version `libphp` reported: {all}"
    );
    assert!(
        all.contains("libblastit_php83"),
        "and the shim to install: {all}"
    );
    assert!(
        all.contains("(PHP 8.4, ") && all.contains(&shim_name(84)),
        "and the `(PHP X.Y, <arch>-<libc>)` tuple this build needed: {all}"
    );
    // The refusal is the boot's: no listener, no worker, no request.
    assert_no_serving_surface(&mut boot, "shim minor");
    // The load happened, but only in the probe child: both stubs' canaries name
    // a process whose parent is the boot.
    let writes = assert_loaded_only_below_the_fork(
        &[&stubs.mismatch_libphp_canary, &stubs.mismatch_shim_canary],
        init,
        "shim minor",
    );
    println!("refused at boot (stage-1 probe), load in a child only: {writes:?}\n{refusal}");
}

/// The timeout: a probe child whose `worker_init` never returns is killed at
/// `BLASTIT_PROBE_TIMEOUT_MS` and the boot refuses — it never hangs.
///
/// The stub shim writes the shape of the process it hangs in before it hangs, so
/// this leg checks the containment the timeout killed rather than trusting it:
/// a child of the boot (`pid != init`, `ppid == init`), its **own** process
/// group (`pgid == pid`), the route's uid (`dev` runs as the caller, so that is
/// the caller's), and `EPERM` on `socket(AF_INET)` — `SeccompRole::EmbeddedView
/// { egress: false }` is installed before `worker_init` runs.
#[test]
fn a_probe_that_hangs_is_killed_and_refuses_the_boot() {
    let _root = StubRootGuard::start();
    let stubs = stubs();
    clear(&stubs.hang_shape);
    let scratch = Scratch::new("hang", PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    let libphp = scratch.copy(&stubs.hang_libphp, "libphp8.4.so");
    scratch.copy(&stubs.hang_shim, &shim_name(84));
    // The route's uid in `dev`: the caller's, which is who created the scratch.
    let route_uid = std::fs::metadata(&scratch.dir).expect("the scratch").uid();

    let started = Instant::now();
    let mut boot = Boot::start(
        &scratch.dir,
        &[
            ("BLASTIT_LIBPHP", &libphp.to_string_lossy()),
            ("BLASTIT_PHP_SHIM_DIR", &scratch.dir.to_string_lossy()),
            ("BLASTIT_PROBE_TIMEOUT_MS", "250"),
        ],
    );
    let init = boot.pid();
    let code = boot
        .await_boot()
        .expect("a probe that never returns must refuse the boot, not hang it");
    let elapsed = started.elapsed();
    boot.stop();
    let stdout = boot.stdout_text();
    let stderr = boot.stderr_text();
    let all = format!("{stdout}\n{stderr}");

    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert!(
        elapsed < Duration::from_secs(10),
        "the 250 ms probe timeout did not bound the boot: it took {elapsed:?}:\n{all}"
    );
    let refusal = refusal_text(&boot);
    assert_package_sentence(&refusal, "probe timeout");
    assert_no_fallback_recipe(&refusal);
    assert!(
        all.contains("250 ms"),
        "the refusal names the timeout it applied: {all}"
    );
    assert!(
        all.contains("BLASTIT_PROBE_TIMEOUT_MS"),
        "and the knob that would raise it: {all}"
    );
    assert_no_serving_surface(&mut boot, "probe timeout");

    let shape = std::fs::read_to_string(&stubs.hang_shape).unwrap_or_else(|err| {
        panic!(
            "no probe shape was written ({}): {err}\n{all}",
            stubs.hang_shape.display()
        )
    });
    let mut fields = std::collections::BTreeMap::new();
    for field in shape
        .lines()
        .find(|line| line.starts_with("probe "))
        .unwrap_or_else(|| panic!("no `probe` line in the shape file: {shape}"))
        .split_whitespace()
    {
        if let Some((key, value)) = field.split_once('=') {
            fields.insert(key.to_string(), value.parse::<u32>().expect("a u32 field"));
        }
    }
    let pid = fields["pid"];
    assert_ne!(pid, init, "the probe is a fork, not the boot: {shape}");
    assert_eq!(fields["ppid"], init, "the probe is init's child: {shape}");
    assert_eq!(
        fields["pgid"], pid,
        "the probe leads its own process group, which is what `kill(-pgid)` names: {shape}"
    );
    assert_eq!(
        fields["uid"], route_uid,
        "the probe runs as the route's uid: {shape}"
    );
    assert_eq!(fields["euid"], route_uid, "and owns nothing more: {shape}");
    assert_eq!(
        fields["inet"], 1,
        "`EmbeddedView` must answer `socket(AF_INET)` with EPERM (1) before `worker_init`: {shape}"
    );
    // The timeout must also be a *reap*, not a refusal that leaves the parked
    // child behind: a boot that exits while its `pause()` child lives on is a
    // leak that the next boot would inherit (and a `kill(-pgid)` that missed).
    let reaped = Instant::now() + Duration::from_secs(2);
    while Path::new(&format!("/proc/{pid}")).exists() && Instant::now() < reaped {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "the boot refused but its probe child (pid {pid}, pgid {} uid {}) is still in \
         `/proc`, so `BLASTIT_PROBE_TIMEOUT_MS` did not reap what it killed:\n{all}",
        fields["pgid"],
        fields["uid"]
    );
    println!(
        "killed at BLASTIT_PROBE_TIMEOUT_MS=250 after {elapsed:?}; probe pid {pid} ppid {} \
         pgid {} uid {} seccomp EPERM, gone from /proc\n{refusal}",
        fields["ppid"], fields["pgid"], fields["uid"]
    );
}

/// The inode pin (`ADR-0010` §3, `R7`): a `libphp` that changes identity
/// between stage 0's `fstat` and stage 1's `O_NOFOLLOW` re-open must refuse the
/// boot — the worker would otherwise load a file nobody inspected.
///
/// The swap is driven from the boot's own announcement of the pair it inspected,
/// because that line is the only observable end of stage 0: `src/` is not this
/// node's path, so there is no seam to open the window deterministically. The
/// window is a race between two stages of one process, so the test retries —
/// **bounded** to 4 attempts and 20 s — and only a *lost race* is retried:
///
/// * stage 0 never announces the pair → a broken fixture or a regressed gate,
///   which panics at once instead of being retried;
/// * the boot refuses with a different sentence → a real failure, which panics
///   at once;
/// * the boot **serves** → the rename landed after stage 1 had already opened
///   the pinned file, which is the harness losing the race and is the only
///   reason to try again; if every attempt serves, the failure says exactly that
///   the window was missed **and** that a removed inode pin produces the same
///   four served attempts, so it can never read as harness noise when it is a
///   regression.
///
/// This is the one refusal in this suite that stage **1** makes, so unlike a
/// stage-0 refusal the `dev` boot has already confirmed itself on stdout: the
/// assertions are that it exits `1`, that no `ready`, `spawned` or `request`
/// event exists, that nothing listens on the port, that the sentence names the
/// file and the fix, and that no stub was loaded.
#[test]
fn a_libphp_replaced_between_the_stages_refuses_the_boot() {
    let _root = StubRootGuard::start();
    let stubs = stubs();
    let mut served = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut attempt = 0;
    while attempt < 4 && Instant::now() < deadline {
        attempt += 1;
        clear(&stubs.nts_libphp_canary);
        clear(&stubs.nts_shim_canary);
        let scratch = Scratch::new("swap", PHP_MANIFEST);
        scratch.write(ENTRY, NORMAL_PHP);
        let libphp = scratch.copy(&stubs.nts_libphp, "libphp8.4.so");
        scratch.copy(&stubs.nts_shim, &shim_name(84));
        let spare = scratch.copy(&stubs.nts_libphp, "replacement.so");

        let mut boot = Boot::start_swapping(
            &scratch.dir,
            &[
                ("BLASTIT_LIBPHP", &libphp.to_string_lossy()),
                ("BLASTIT_PHP_SHIM_DIR", &scratch.dir.to_string_lossy()),
            ],
            Swap {
                needle: "[route.php] runtime is",
                from: spare,
                to: libphp.clone(),
            },
        );
        let code = boot.await_boot();
        let swapped = boot.swapped();
        boot.stop();
        let stdout = boot.stdout_text();
        let stderr = boot.stderr_text();
        let all = format!("{stdout}\n{stderr}");

        if !swapped {
            panic!(
                "attempt {attempt}: stage 0 never announced the pair it inspected, so the swap \
                 could not be placed at all — this is not the timing race, it is the fixture or \
                 the gate:\n{all}"
            );
        }
        if !all.contains("is not the file stage 0 inspected") {
            match code {
                // A refusal with another sentence is a real failure.
                Some(code) => panic!(
                    "attempt {attempt}: the boot refused (exit {code:?}) with a sentence that is \
                     not the inode pin's, which is not the timing race:\n{all}"
                ),
                // Served: the rename landed after stage 1 opened the file.
                None => served.push(format!("attempt {attempt}: {all}")),
            }
            continue;
        }
        assert_eq!(
            code,
            Some(Some(1)),
            "the inode refusal is a boot refusal (exit 1): {all}"
        );
        assert!(
            !boot.events().iter().any(|event| event["event"] == "ready"),
            "no listener is bound before stage 1 verifies the inode: {all}"
        );
        assert_no_serving_surface(&mut boot, "inode swap");
        assert!(
            all.contains(&libphp.display().to_string()),
            "the refusal names the library that moved: {all}"
        );
        assert!(
            all.contains("restore the package and restart blastit"),
            "and the fix: {all}"
        );
        assert_never_loaded(
            &[&stubs.nts_libphp_canary, &stubs.nts_shim_canary],
            "inode swap",
        );
        println!("refused (stage 1 inode pin) on attempt {attempt}: {all}");
        return;
    }
    assert!(
        !served.is_empty(),
        "the retry bound (4 attempts, 20 s) ended after {attempt} attempt(s) without a boot to \
         judge"
    );
    panic!(
        "the stage-0→stage-1 window was missed in {attempt} attempt(s) inside 20 s: the rename \
         landed after stage 1 had already opened the pinned file, so every attempt served the \
         route and the inode pin was never exercised. This is a timing race in the harness (the \
         swap is triggered by stage 0's own announcement; there is no deterministic seam without \
         a `src/` change) — BUT a **removed inode pin** produces exactly these same served \
         attempts, so do not dismiss this as harness noise before checking that stage 1 still \
         compares the re-opened file's `(st_dev, st_ino)` against stage 0's record \
         (`php_view::boot_bindings`/`open_pinned`). If that comparison is gone, this is the \
         regression the test exists to catch, not a lost race:\n{}",
        served.join("\n")
    );
}

/// `R11`: no `libphp` above the worker. With a real pair the route answers `200`
/// and init — the only process whose address space is readable — maps neither the
/// library nor the shim.
///
/// The forked roles keep `PR_SET_DUMPABLE(0)` (`ADR-0010` §2), so their maps are
/// `EACCES` to every process, this one included; that is asserted rather than
/// assumed, and the load's *placement* is proved by the stub canary in
/// [`a_shim_for_another_minor_refuses_the_boot_before_the_listener`].
#[test]
fn no_libphp_is_mapped_above_the_view_worker() {
    let _root = StubRootGuard::start();
    let Some(host) = host_php() else {
        refused_without_a_host("maps", "no-host-maps");
        return;
    };
    let scratch = Scratch::new("maps", PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);

    let mut boot = Boot::start(
        &scratch.dir,
        &[
            ("BLASTIT_LIBPHP", &host.libphp.to_string_lossy()),
            (
                "BLASTIT_PHP_SHIM_DIR",
                &host
                    .shim
                    .parent()
                    .expect("a shim directory")
                    .to_string_lossy(),
            ),
        ],
    );
    assert!(
        boot.await_boot().is_none(),
        "this host has {} and {}, so the route must be served: {}",
        host.libphp.display(),
        host.shim.display(),
        boot.stderr_text()
    );
    let response = boot.get("/");
    assert!(
        response.starts_with("HTTP/1.1 200") && response.contains(BODY),
        "the embedded worker did not answer: {response}"
    );
    // The boot names the pair it opened, so the served leg and the probe agree.
    let stderr = boot.stderr_text();
    assert!(
        stderr.contains(&host.libphp.display().to_string())
            && stderr.contains(&host.shim.display().to_string()),
        "the boot must name the pair the probe found: {stderr}"
    );

    let init = boot.pid();
    let maps = std::fs::read_to_string(format!("/proc/{init}/maps")).expect("init's maps");
    assert!(
        !maps.contains("libphp"),
        "init mapped {}: the load belongs to the worker alone:\n{maps}",
        host.libphp.display()
    );
    assert!(
        !maps.contains("libblastit_php"),
        "init mapped a PHP shim: the load belongs to the worker alone:\n{maps}"
    );

    let children = boot.children();
    assert!(
        !children.is_empty(),
        "`dev` forks http and scheduler from init; a boot with no children cannot place the load"
    );
    // The route's view is the one role that legitimately maps `libphp`; every
    // other child of init (`http`, `scheduler`, `dbproxy`) must not, so those are
    // the roles whose map content is asserted here. As root — how CI runs every
    // test binary (`ci.yml`'s "run every test binary as root" step) — a
    // non-dumpable role's maps are readable, so the same witness exists there
    // instead of being skipped.
    let view_pids: Vec<u32> = boot
        .events()
        .into_iter()
        .filter(|event| event["event"] == "process.spawned" && event["route"] == ROUTE)
        .filter_map(|event| event["pid"].as_u64())
        .map(|pid| pid as u32)
        .collect();
    let above_view: Vec<u32> = children
        .iter()
        .copied()
        .filter(|pid| !view_pids.contains(pid))
        .collect();
    assert!(
        !above_view.is_empty(),
        "no role above the view among init's children {children:?} (view pids {view_pids:?}), so \
         this test would assert nothing"
    );
    let euid = effective_uid();
    let (mut checked, mut denied, mut gone) = (0, 0, 0);
    for pid in &above_view {
        match std::fs::read_to_string(format!("/proc/{pid}/maps")) {
            // `PR_SET_DUMPABLE(0)`: an unprivileged caller cannot read a forked
            // role's maps, which the count below asserts.
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => denied += 1,
            // A role that retired between the listing and the read.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => gone += 1,
            Err(err) => panic!("reading /proc/{pid}/maps: {err}"),
            Ok(text) => {
                checked += 1;
                assert!(
                    !text.contains("libphp") && !text.contains("libblastit_php"),
                    "/proc/{pid}/maps (a role above the view) holds a PHP image:\n{text}"
                );
            }
        }
    }
    boot.stop();
    if euid == 0 {
        // Root reads every map, so the witness has to be complete: each role
        // above the view was either content-checked or gone.
        assert_eq!(
            checked + gone,
            above_view.len(),
            "as root every role above the view must be content-checked ({checked} checked, \
             {denied} denied, {gone} gone of {}): {above_view:?}",
            above_view.len()
        );
    } else {
        assert!(
            denied > 0,
            "the forked roles must keep PR_SET_DUMPABLE(0) (ADR-0010 §2): {denied} denied of {} \
             above the view",
            above_view.len()
        );
    }
    println!(
        "init pid {init}: no libphp/shim; above the view {checked} map(s) content-checked, \
         {denied} denied (EACCES), {gone} gone, of {} children; euid {euid}",
        children.len()
    );
}

/// `exit()` is contained: the worker retires, its slot serves the next request,
/// and the crashed request is charged **at least its wall time**.
#[test]
fn an_exit_is_contained_and_the_next_request_is_served() {
    let _root = StubRootGuard::start();
    containment(EXIT_PHP, "exit()", "containment-exit", None);
}

/// The fatal twin: `trigger_error(..., E_USER_ERROR)` reaches `zend_bailout()`,
/// which `php_execute_script` swallows (`LESSONS.md`, `MB.3.12`).
#[test]
fn a_fatal_is_contained_and_the_next_request_is_served() {
    let _root = StubRootGuard::start();
    containment(FATAL_PHP, "fatal", "containment-fatal", None);
}

/// The same bailout with almost no CPU in it: 60 ms of `usleep` and an `exit()`.
///
/// This is the leg that makes `cost_ns >= wall_ns` an oracle: a charge taken
/// from `cpu_ns` is a small fraction of the wall time here (this host measures
/// ~1/700 of it), so it misses the epic's sentence by orders of magnitude rather
/// than by a rounding error. See [`SLEEP_PHP`].
#[test]
fn a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time() {
    let _root = StubRootGuard::start();
    containment(SLEEP_PHP, "usleep()", "containment-sleep", Some(60));
}

/// One slot, two requests: the crash answers `5xx` with its cost, the next
/// request is served by a fresh process in the **same** slot — the only live
/// worker of the route, forked by init, with the crashed one gone.
///
/// `sleeps_ms` is set by a leg whose wall time is spent sleeping rather than
/// computing: it then also asserts that the sleep shows up in the wall time and
/// that the reported CPU is a small fraction of it, which is what proves the
/// charge came from the wall and not from the CPU.
fn containment(crash: &str, what: &str, name: &str, sleeps_ms: Option<u64>) {
    let Some(host) = host_php() else {
        refused_without_a_host(what, name);
        return;
    };
    let scratch = Scratch::new(name, ONE_WORKER_MANIFEST);
    scratch.write(ENTRY, crash);

    let mut boot = Boot::start(
        &scratch.dir,
        &[
            ("BLASTIT_LIBPHP", &host.libphp.to_string_lossy()),
            (
                "BLASTIT_PHP_SHIM_DIR",
                &host
                    .shim
                    .parent()
                    .expect("a shim directory")
                    .to_string_lossy(),
            ),
        ],
    );
    assert!(
        boot.await_boot().is_none(),
        "this host has {} and {}, so the route must be served: {}",
        host.libphp.display(),
        host.shim.display(),
        boot.stderr_text()
    );

    let crashed = boot.get("/crash");
    assert!(
        !crashed.starts_with("HTTP/1.1 2"),
        "{what}: a contained bailout must never be a `200`: {crashed}"
    );
    let event = boot.request_event("/crash");
    let status = event["status"]
        .as_u64()
        .unwrap_or_else(|| panic!("{what}: no status: {event}"));
    assert!(
        status >= 500,
        "{what}: the crash is reported as a server failure, not success: {event}"
    );
    // The label and the bill are one decision (`MB.3.38`): a request a view
    // ended with a `5xx` failed, so §8.1's closed outcome list reports it as
    // `view_died` — never `ok`, which is what it was before the charge was
    // fixed.
    let outcome = event["outcome"]
        .as_str()
        .unwrap_or_else(|| panic!("{what}: no outcome: {event}"));
    assert_ne!(
        outcome, "ok",
        "{what}: a {status} bailout is a failed request, not `ok`: {event}"
    );
    let cpu_ns = event["cpu_ns"]
        .as_u64()
        .unwrap_or_else(|| panic!("{what}: the worker reported no CPU cost: {event}"));
    let wall_ns = (event["duration_ms"]
        .as_f64()
        .unwrap_or_else(|| panic!("{what}: no duration: {event}"))
        * 1e6) as u64;
    let cost_ns = event["cost_ns"]
        .as_u64()
        .unwrap_or_else(|| panic!("{what}: nothing was charged: {event}"));
    assert!(
        cpu_ns <= wall_ns,
        "{what}: cpu_ns {cpu_ns} > wall_ns {wall_ns}: one process's CPU time cannot exceed \
         its wall time: {event}"
    );
    assert!(
        cpu_ns > 0 || wall_ns < TICK_FLOOR_NS,
        "{what}: a {wall_ns} ns request that reported 0 ns of CPU should have crossed a tick: \
         {event}"
    );
    // `MB.3.13`'s own sentence: "the crashed request is charged at least its
    // wall time", where the wall time is the event's `duration_ms` — what the
    // client waited. `cost_ns >= cpu_ns` was the weaker rule that let an 18 ms
    // `exit()` be charged 3 ms, so a crash loop cost a fifth of what it spent.
    // `R19`'s ceiling at the single live thread such a worker has *is* this
    // wall, and no kernel floor can exceed it.
    assert!(
        cost_ns > 0 && cost_ns >= wall_ns,
        "{what}: a crashed request must be charged at least its wall time (cost {cost_ns}, \
         wall {wall_ns}, cpu {cpu_ns}): {event}"
    );
    if let Some(sleep_ms) = sleeps_ms {
        // The discriminator: the sleep is in the wall time, and the CPU is a
        // small fraction of it, so a `cpu_ns`-based charge cannot satisfy the
        // assertion above on this leg.
        let sleep_ns = sleep_ms.saturating_mul(1_000_000);
        assert!(
            wall_ns >= sleep_ns,
            "{what}: {sleep_ms} ms of sleeping is not in the {wall_ns} ns wall time: {event}"
        );
        assert!(
            cpu_ns.saturating_mul(CPU_SHARE) <= wall_ns,
            "{what}: the CPU ({cpu_ns} ns) must be at most a {CPU_SHARE}th of the wall \
             ({wall_ns} ns), or a `cpu_ns`-based charge could not be told from a wall one: \
             {event}"
        );
    }

    // The worker retires itself (a clean `0`), which is what lets the supervisor
    // respawn into the same slot instead of treating the crash as a kill.
    let crashed_pid = boot
        .wait_event(BOOT_TIMEOUT, |event| {
            event["event"] == "process.exited" && event["status"] == 0
        })
        .unwrap_or_else(|| {
            panic!(
                "{what}: the crashed worker never exited: {}",
                boot.stdout_text()
            )
        });
    let crashed_pid = crashed_pid["pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("{what}: a worker exit without a pid: {crashed_pid}"))
        as u32;

    // The slot still answers: the same route, a fresh process, the same uid.
    scratch.write(ENTRY, NORMAL_PHP);
    let after = boot.get("/after");
    assert!(
        after.starts_with("HTTP/1.1 200") && after.contains(BODY),
        "{what}: the slot stopped serving after the crash: {after}"
    );
    let after_event = boot.request_event("/after");
    assert_eq!(after_event["status"], 200, "{what}: {after_event}");

    let spawned: Vec<serde_json::Value> = boot
        .events()
        .into_iter()
        .filter(|event| event["event"] == "process.spawned" && event["route"] == ROUTE)
        .collect();
    assert!(
        spawned.len() >= 2,
        "{what}: the slot must be respawned, not abandoned: {}",
        boot.stdout_text()
    );
    let first = &spawned[0];
    let second = &spawned[spawned.len() - 1];
    assert_eq!(
        first["pid"].as_u64(),
        Some(u64::from(crashed_pid)),
        "{what}: the worker that crashed is the first spawn: {first}"
    );
    assert_ne!(
        first["pid"], second["pid"],
        "{what}: the crash retired the worker, so the next request needs a new process: {second}"
    );

    // "Same slot" cannot be uid equality: `blastit dev` runs every route as the
    // caller, so the uid is one constant for the whole boot. The slot is the
    // route's **single** live view, and the proof is that the process serving
    // the second request is a direct child of init, that the process that
    // crashed is gone, and that exactly one worker of this route is alive.
    let init = boot.pid();
    let second_pid = second["pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("{what}: a spawn without a pid: {second}"))
        as u32;
    assert_eq!(
        ppid_of(second_pid),
        Some(init),
        "{what}: the respawn is init's child ({init}), so it is the same slot and not a second \
         pool: {second}"
    );
    assert_ne!(
        ppid_of(crashed_pid),
        Some(init),
        "{what}: the crashed worker (pid {crashed_pid}) is gone, so nothing was left serving"
    );
    let live: Vec<u32> = spawned
        .iter()
        .filter_map(|event| event["pid"].as_u64())
        .map(|pid| pid as u32)
        .filter(|pid| ppid_of(*pid) == Some(init))
        .collect();
    assert_eq!(
        live,
        vec![second_pid],
        "{what}: exactly one worker of route {ROUTE:?} may be live after the crash, and it is \
         the respawn ({second_pid}): {live:?}"
    );
    boot.stop();
    println!(
        "{what}: status {status}  outcome={outcome}  cpu_ns={cpu_ns}  wall_ns={wall_ns}  \
         cost_ns={cost_ns}  retired pid {crashed_pid}  respawned pid {second_pid} (ppid {init}) \
         uid {}  live={live:?}",
        second["uid"]
    );
}

/// The other branch of the two-outcome rule: no usable pair on this host, so the
/// documented page must refuse at boot with the package sentence — never serve.
fn refused_without_a_host(what: &str, name: &str) {
    let scratch = Scratch::new(name, PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    let mut boot = Boot::start(&scratch.dir, &[]);
    let code = boot
        .await_boot()
        .expect("a host with no usable libphp cannot serve a [route.php] route");
    boot.stop();
    let stdout = boot.stdout_text();
    let stderr = boot.stderr_text();
    assert_eq!(code, Some(1), "{what}: stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "{what}: a startup error writes nothing on stdout (§1.1)"
    );
    assert_package_sentence(&stderr, what);
    assert_no_fallback_recipe(&stderr);
    assert_no_serving_surface(&mut boot, what);
    println!("refused (no usable libphp + shim on this host): {stderr}");
}
