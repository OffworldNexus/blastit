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
//!   bailed request — `outcome` is `bailed` (`MB.3.41`), never `ok` and never
//!   `view_died` — and the next request is served
//!   by a fresh worker of that route — a new pid, a direct child of
//!   init, with the crashed process gone (views are forked on demand and one
//!   request is in flight at a time, and the uid is one constant in single-UID
//!   `dev`, so it proves nothing about the slot). A third
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
//! # CI coverage (`MB.3.13.4` review, closed by `MB.3.39`)
//!
//! `make check` runs on a host without PHP, so the real-pair legs can only take
//! their refusal branch there. The served half needs a NTS `libphp` plus the
//! shim, and `.github/workflows/php-sapi.yml`'s `shim` job has both: `make
//! rs-test-php-image` builds these binaries on the runner (a `--no-run` `cargo
//! test -p blastit --locked` over `Makefile`'s `PHP_SUITE_ARGS`, i.e. `--test
//! critical_php_backend --test critical_php_embed`) and runs them in the
//! `php-sapi-e2e` image against its `libphp8.4.so` and shim. A `guard: the pair
//! legs must serve, not refuse` step then fails that job if either suite prints
//! its no-pair line (`refused (no usable libphp + shim on this host` or
//! `refused: … no usable NTS libphp was found`) or if the served evidence is
//! missing: `served: HTTP/1.1 200 OK` (the backend suite) and `retired pid …
//! respawned pid` (the containment leg here). The trigger is `push` or
//! `pull_request` on `develop` (plus `workflow_dispatch`), so a release-branch
//! push is not covered; the PR is the gate.
//!
//! So this job is the CI witness for `no_libphp_is_mapped_above_the_view_worker`,
//! the containment legs (`an_exit_is_contained_and_the_next_request_is_served`,
//! `a_fatal_is_contained_and_the_next_request_is_served`,
//! `a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time`),
//! `a_blocking_script_is_charged_cpu_time_not_wall_time` and
//! `a_form_body_reaches_the_front_controller`, plus `critical_php_backend`'s
//! two pair-gated legs —
//! `a_php_route_is_served_or_the_boot_refuses_naming_the_package` and
//! `a_symlinked_blastit_libphp_is_resolved_before_it_is_served` — while the
//! stub-refusal and probe legs, the provenance legs and the `egress` twin run
//! everywhere, and `make rs-test-php` is the same pair legs on a host that has
//! the pair.
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
//!
//! # The four legs `MB.3.37` added, and the mutation each one kills
//!
//! `AUDIT-MB.3.13` found four rules this suite claimed but did not own, each a
//! deletion or inversion that left `make check` green:
//!
//! * [`a_shim_replaced_between_the_stages_refuses_the_boot`] — the shim's half
//!   of the `(path, st_dev, st_ino)` pin. Dropping the shim comparison in
//!   `php_view::open_pinned` made the boot serve instead;
//! * [`an_egress_route_lets_the_probe_open_a_socket`] — the `egress = true`
//!   direction, which `probe_bindings`' `unwrap_or(false)` hardcodes to
//!   `false`: the probe's `socket(AF_INET)` would be `EPERM` while the same
//!   route's INI says `allow_url_fopen=1`;
//! * [`a_blocking_script_is_charged_cpu_time_not_wall_time`] — `cpu_ns` is
//!   `CLOCK_PROCESS_CPUTIME_ID`, not the request's duration: a shim that
//!   reported the wall clock as `cpu_ns` is red here and green everywhere else;
//! * [`a_form_body_reaches_the_front_controller`] — a request body reaches
//!   `$_POST` (`R12`): a `RequestV1` without its body pointer would serve every
//!   form as an empty `$_POST`.
//!
//! The first two need no host PHP; the last two are real-pair legs and take the
//! [`refused_without_a_host`] branch on a PHP-free host, like every other
//! real-pair leg here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use blastit::host::{Claim, Provenance};

#[path = "common/libphp_probes.rs"]
mod libphp_probes;

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

/// The documented page (`docs/frameworks/php.md`, `ADR-0010` §2): a PHP file
/// needs an `entrypoint` and no `BLASTIT_APP`.
const PHP_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {}
";

/// What the front controller echoes; the body the suite looks for.
const BODY: &str = "blastit-php-ok";

/// A script that answers normally.
const NORMAL_PHP: &str = "\
<?php
header('Content-Type: text/plain');
echo \"blastit-php-ok\\n\";
";

/// A decoy front controller (`MB.3.56`): a body carrying this marker is a decoy
/// that ran, from an inode the boot never recorded.
const DECOY_PHP: &str = "\
<?php
header('Content-Type: text/plain');
echo \"DECOY-RAN\\n\";
";

/// `exit()` deep enough into the script that `CLOCK_PROCESS_CPUTIME_ID` crosses
/// a tick (`LESSONS.md`, `MB.3.12`: a cheap script may legitimately report `0`).
///
/// These are crash **bodies**, not pages: [`crash_page`] puts one behind a
/// `/crash` guard in the one front controller, because a containment leg may not
/// rewrite the entry between its two requests — an in-place rewrite is refused
/// by `MB.3.56`'s pin, and a leg that needs one would be testing the refusal.
const EXIT_BODY: &str = "\
for ($i = 0; $i < 500000; $i++) { $x = $i * $i; }
exit(0);";

/// A contained fatal: `E_USER_ERROR` calls `zend_bailout()`.
const FATAL_BODY: &str = "\
for ($i = 0; $i < 500000; $i++) { $x = $i * $i; }
trigger_error('blastit fatal', E_USER_ERROR);";

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
const SLEEP_BODY: &str = "\
usleep(60000);
exit(0);";

/// The containment legs' front controller: `body` runs for `/crash` and nothing
/// else, so the second request reaches the **same pinned file** as the first.
///
/// Rewriting the entry to a normal page between the two requests was the old
/// shape; it cannot exist under `MB.3.56`'s pin, and it was never the point of
/// the leg (the point is that the respawned worker serves). The guard is also
/// what lets the crash legs share one page with the charge assertions: only
/// `/crash` crashes.
fn crash_page(body: &str) -> String {
    format!(
        "\
<?php
header('Content-Type: text/plain');
if (($_SERVER['PATH_INFO'] ?? '/') === '/crash') {{
{body}
}}
echo \"blastit-php-ok\\n\";
"
    )
}

/// How long the `cpu_ns`-vs-wall leg parks its front controller: long enough
/// that the request's wall time is dominated by the sleep, and a `cpu_ns` taken
/// from the clock the C shim documents cannot reach a quarter of it.
const BLOCK_MS: u64 = 200;

/// The fraction of the wall time the leg above allows the CPU reading: a
/// `CLOCK_PROCESS_CPUTIME_ID` delta over a 200 ms `usleep` is a few milliseconds
/// of PHP startup, so a quarter of the wall is a wide margin for a loaded CI
/// host — and a `cpu_ns` that reported the request's duration misses it by the
/// whole block.
const CPU_WALL_SHARE: u64 = 4;

/// A front controller that **blocks** and then answers normally: the wall time
/// is spent parked, so the CPU reading and the request's duration are different
/// numbers by construction (`ADR-0010` §1.1, `R19`).
///
/// Every other fixture in this suite is a busy loop, where `cpu_ns ≈ wall_ns`
/// and an implementation that reported the request's duration as `cpu_ns`
/// satisfies every existing assertion (`cpu_ns <= wall_ns`, `cpu_ns > 0`,
/// `cost_ns >= cpu_ns`). This is the fixture that tells the two clocks apart.
///
/// Built from [`BLOCK_MS`] rather than spelling the number again: `200000`
/// written here let the constant document one block while the fixture slept
/// another (the reviewer's MINOR on `MB.3.37`), and a leg whose wall-time guard
/// moves with the constant must sleep what the constant says.
fn blocking_php() -> String {
    format!(
        "\
<?php
usleep({});
header('Content-Type: text/plain');
echo \"blastit-php-ok\\n\";
",
        BLOCK_MS.saturating_mul(1000)
    )
}

/// The form leg's front controller: whatever reached `$_POST` is what answers,
/// so an empty `$_POST` — a `RequestV1` without its body — is visible in the
/// response instead of being inferred.
const FORM_PHP: &str = "\
<?php
header('Content-Type: text/plain');
echo 'x=' . (isset($_POST['x']) ? $_POST['x'] : 'absent') . \"\\n\";
";

/// The documented manifest plus its ingress codec (`docs/frameworks/php.md`,
/// `MB.3.13`'s gap G4): a `form` route refuses a body of another type with
/// `400`, so the `Content-Type` the leg posts is part of the fixture.
const FORM_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {
    body form
}
";

/// The worker's whole-body budget for the form leg (`BLASTIT_MAX_BODY_BYTES`).
///
/// It is deliberately small: `BodyPlan::Buffer` bounds the buffered body with
/// this same number in the scheduler and in the view (`R14`/`R15`), so one
/// 8 KiB post is past the cap both of them apply.
const FORM_BODY_MAX: usize = 4096;

/// `PHP_MANIFEST` with `egress any` (`ADR-0010` §3, design §7.3): the view
/// keeps `AF_INET`/`AF_INET6` in its filter, and the INI renders
/// `allow_url_fopen=1` for the same view. The two must never disagree.
const EGRESS_MANIFEST: &str = "\
entrypoint php \"public/index.php\"

view app regex '.*' {
    egress any
}
";

/// A file with **no** `entrypoint php`: the deployment gate has nothing to
/// authorise, so the boot proceeds exactly as `dev`/`check` do (`MB.3.44`).
const STATIC_ONLY_MANIFEST: &str = "\
static \"/\" {
    root \"site\"
}
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
    compile_linked(source, out, defines, &[]);
}

/// The same, plus linker arguments — the `-Wl,-rpath,…` that writes a
/// `DT_RPATH`/`DT_RUNPATH` into the object it produces.
fn compile_linked(source: &Path, out: &Path, defines: &[(&str, String)], linker: &[String]) {
    let mut command = Command::new("cc");
    command.args(["-shared", "-fPIC", "-O0", "-o"]);
    command.arg(out).arg(source).arg("-lc").args(linker);
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
    // An object that exports no `php_version` — the symbol the worker resolves
    // before `php_module_startup` — is refused by stage 0 (`MB.3.59`). The exact
    // name needs its NUL: `php_version_compare` exists in every version and
    // must not count as the symbol.
    if !contains(&bytes, b"php_version\0") {
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

/// The discovery order this probe walks: a file name, and the per-minor
/// directory it is looked for in (`None`: every directory of [`libphp_path`]'s
/// list, in its order).
///
/// Version-major, then spelling (`MB.3.61`): the *minor* is preferred and the
/// name only breaks a tie within it, so a host with two embed packages probes the
/// library the boot will serve — the newest — instead of the first name in a
/// list. A name-major walk would probe `/usr/lib/libphp83.so` before a host's
/// `/usr/lib/php85/libphp.so`, and serve the older minor.
///
/// It is a **copy** of `stage0.rs`'s `LIBPHP_PROBES`, because this suite may not
/// lean on the code under test; [`the_probe_table_matches_stage0`] is what keeps
/// the copy honest (`MB.3.62`).
const PROBES: [(&str, Option<&str>); 13] = [
    ("libphp8.5.so", None),
    ("libphp-8.5.so", None),
    ("libphp85.so", None),
    ("libphp.so", Some("php85")),
    ("libphp8.4.so", None),
    ("libphp-8.4.so", None),
    ("libphp84.so", None),
    ("libphp.so", Some("php84")),
    ("libphp8.3.so", None),
    ("libphp-8.3.so", None),
    ("libphp83.so", None),
    ("libphp.so", Some("php83")),
    ("libphp.so", None),
];

/// `MB.3.62`: [`PROBES`] is `stage0.rs`'s discovery order, read from that source
/// at compile time and compared entry by entry — a change to either side fails
/// here instead of letting this probe walk a host differently from the boot.
///
/// The check's own oracle (a dropped per-minor constraint, the fallback turned
/// into a constrained probe, two entries out of order) is
/// `crates/blastit/tests/common/libphp_probes.rs`'s `a_changed_entry_is_reported_at_its_index`,
/// which runs in this binary too.
#[test]
fn the_probe_table_matches_stage0() {
    if let Err(drift) = libphp_probes::assert_matches(&PROBES, include_str!("../src/stage0.rs")) {
        panic!("{drift}");
    }
}

/// `BLASTIT_LIBPHP` when it is a file, else the version-major probe order in the
/// library directories (`ADR-0010` §3) — the resolved path.
///
/// The names, directories and order are the ones `blastit` itself uses,
/// including Alpine's packed `libphp83.so` and its per-minor `/usr/lib/php84`
/// (`MB.3.59`), and they are tried minor-by-minor rather than name-by-name
/// (`MB.3.61`). The table below is a **copy** of `stage0.rs`'s `LIBPHP_PROBES` —
/// this probe may not lean on the code under test — so
/// [`the_probe_table_matches_stage0`] reads that source at compile time and
/// fails when an entry differs, in order or in its per-minor constraint
/// (`MB.3.62`).
fn libphp_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BLASTIT_LIBPHP") {
        let path = PathBuf::from(path);
        return path.is_file().then(|| path.canonicalize().ok()).flatten();
    }
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    let triplet = format!("{}-linux-{libc}", std::env::consts::ARCH);
    let dirs = [
        format!("/usr/lib/{triplet}"),
        "/usr/lib64".to_string(),
        "/usr/lib".to_string(),
        "/usr/lib/php85".to_string(),
        "/usr/lib/php84".to_string(),
        "/usr/lib/php83".to_string(),
        format!("/usr/local/lib/{triplet}"),
        "/usr/local/lib".to_string(),
        "/usr/local/lib/php85".to_string(),
        "/usr/local/lib/php84".to_string(),
        "/usr/local/lib/php83".to_string(),
    ];
    for (name, minor) in PROBES {
        let found = dirs
            .iter()
            .filter(|dir| match minor {
                Some(minor) => {
                    Path::new(dir).file_name().and_then(|name| name.to_str()) == Some(minor)
                }
                None => true,
            })
            .map(|dir| Path::new(dir).join(name))
            .find(|path| path.is_file());
        if let Some(path) = found {
            return path.canonicalize().ok();
        }
    }
    None
}

/// The `(major, minor)` the path states: a dotted name (`libphp8.4.so`,
/// `libphp-8.4.so`), a packed name (`libphp83.so`), or a per-minor directory
/// (`/usr/lib/php84`, `MB.3.59`).
fn minor_of(path: &Path) -> Option<(u32, u32)> {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(dotted) = dotted_of(name)
    {
        return Some(dotted);
    }
    let packed = |component: &str| -> Option<(u32, u32)> {
        let rest = component
            .strip_prefix("libphp")
            .or_else(|| component.strip_prefix("php"))?;
        let stem = rest.split('.').next()?;
        let bytes = stem.as_bytes();
        if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some((
            u32::from(bytes.first()? - b'0'),
            u32::from(bytes.get(1)? - b'0'),
        ))
    };
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(packed) = packed(name)
    {
        return Some(packed);
    }
    path.ancestors()
        .skip(1)
        .filter_map(Path::file_name)
        .filter_map(|component| component.to_str())
        .find_map(packed)
}

/// The dotted spelling: `libphp8.4.so`, `libphp-8.4.so`, `libphp8.4.so.0`.
fn dotted_of(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("libphp")?;
    let mut rest = rest.strip_prefix('-').unwrap_or(rest);
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

/// A temp directory holding one case's `Blastfile`, its front controller and
/// (for the stub legs) the stub pair, removed on drop.
struct Scratch {
    /// The directory `blastit dev` is started in.
    dir: PathBuf,
}

impl Scratch {
    /// Creates `dir` with `Blastfile` written.
    fn new(name: &str, manifest: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("blastit-php-embed-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("public")).expect("a fixture directory");
        std::fs::write(dir.join("Blastfile"), manifest).expect("the manifest");
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

/// A rename to perform while the boot is paused at the test-only rendezvous
/// (`MB.3.64`): the one way to land a swap between stage 0's `fstat` and stage
/// 1's re-open, because the boot says when it is there.
struct Swap {
    /// The rendezvous path `BLASTIT_TEST_SWAP_POINT` names. Stage 1 creates
    /// `<point>.ready` after stage 0 has recorded the pair and before it
    /// re-opens it; writing `<point>.go` releases the boot.
    point: PathBuf,
    /// The file to rename over `to`.
    from: PathBuf,
    /// The pinned path stage 1 will re-open: the `libphp` or the shim.
    to: PathBuf,
}

/// `MB.3.64`: the seam the two inode-pin legs share — `<point>.ready` means
/// "stage 1 is between stage 0's record and the re-open", `<point>.go` releases
/// it.
fn swap_point_sidecar(point: &Path, suffix: &str) -> PathBuf {
    let mut name = point.as_os_str().to_os_string();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
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

    /// The same, renaming `swap.from` over `swap.to` while the boot is paused at
    /// the test-only rendezvous `<point>.ready`.
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
        // `MB.3.64`: stage 0 reads this before the `execve` whose environment has
        // every `BLASTIT_*` stripped, and carries it in the configuration
        // document — so the point reaches stage 1 without the seam needing to
        // cross a barrier that exists to stop exactly that.
        if let Some(swap) = &swap {
            command.env("BLASTIT_TEST_SWAP_POINT", &swap.point);
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
            }
        }));

        // The other half of the rendezvous: nothing here watches a log line. The
        // swap waits for stage 1 to say it is paused and then says go, so the
        // rename is inside the window because both sides say so.
        if let Some((swap, done)) = swap.zip(swapped.clone()) {
            readers.push(std::thread::spawn(move || {
                let released = swap_point_sidecar(&swap.point, "go");
                let deadline = Instant::now() + BOOT_TIMEOUT;
                let ready = swap_point_sidecar(&swap.point, "ready");
                while !ready.exists() {
                    if Instant::now() > deadline {
                        // Never reached the point: release a boot that somehow
                        // is waiting, and let `swapped()` report the miss.
                        let _ = std::fs::write(&released, b"go\n");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                if std::fs::rename(&swap.from, &swap.to).is_ok() {
                    done.store(true, Ordering::SeqCst);
                }
                let _ = std::fs::write(&released, b"go\n");
            }));
        }

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

    /// Waits for the boot to either announce `ready` or exit.
    ///
    /// A gate that refuses never binds — every §4.1 gate runs before the
    /// listener — so "the boot says it is serving" and "the process is gone" is a
    /// complete partition of the two `ADR-0010` §1 outcomes. The first half is
    /// [`Boot::ready`] and not a connect to the port, which this harness picked
    /// by binding and releasing it: a foreign listener on that port made a
    /// refusing boot read as served (`MB.3.40` follow-up 4), and a boot that
    /// serves is the one that has the standing to say so (`MB.3.48`).
    fn await_boot(&mut self) -> Option<Option<i32>> {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("waiting for the boot") {
                return Some(status.code());
            }
            if self.ready() {
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

    /// `POST <path>` with `content_type` and `body` over a fresh connection, as
    /// the raw response.
    ///
    /// A body the scheduler refuses past `BLASTIT_MAX_BODY_BYTES` is answered
    /// before the client has finished writing, so a write error here is the
    /// refusal arriving early and not a harness failure; the response is read
    /// either way and asserted by the caller.
    fn post(&self, path: &str, content_type: &str, body: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connecting");
        stream
            .set_read_timeout(Some(REQUEST_TIMEOUT))
            .expect("a read timeout");
        let _ = write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: {content_type}\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(body.as_bytes());
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
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

    /// Whether init has announced `ready`.
    ///
    /// This is the "served" half of [`Boot::await_boot`]'s partition, not the
    /// port: the port is an ephemeral one this harness picks by binding and
    /// releasing it, so a *foreign* listener that takes it in the window makes
    /// "something accepts on 127.0.0.1" true for a boot that refuses — observed
    /// once in ~12 in `a_non_library_blastit_libphp_refuses_before_anything_loads`
    /// (`MB.3.40` follow-up 4). `ready` is emitted by init after the listener is
    /// bound and the roles are forked, so it is the boot answering for itself and
    /// also the moment a `get` can succeed (`MB.3.48`).
    fn ready(&self) -> bool {
        self.events().iter().any(|event| event["event"] == "ready")
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
        text.contains(&format!("entrypoint php {ENTRY:?}")),
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

/// The text of the shape file a hanging shim writes, and the fields of its
/// `probe` line — `pid`, `ppid`, `pgid`, `uid`, `euid`, `inet` — from the path
/// `HANG_SHIM_STUB_C` bakes in at compile time.
///
/// The shape is read here once for **both** probe legs (the denied
/// `EmbeddedView { egress: false }` and its `egress = true` twin), so the field
/// names and the parse cannot drift between two stubs: a shape the C stub
/// changed fails both legs at once.
fn probe_shape(stubs: &Stubs, all: &str) -> (String, BTreeMap<String, u32>) {
    let shape = std::fs::read_to_string(&stubs.hang_shape).unwrap_or_else(|err| {
        panic!(
            "no probe shape was written ({}): {err}\n{all}",
            stubs.hang_shape.display()
        )
    });
    let mut fields = BTreeMap::new();
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
    (shape, fields)
}

/// The containment both probe twins must show — asserted **once**, because the
/// two legs run one C shim source through one fork path, and a containment claim
/// that drifted between them would be two weaker assertions wearing one name
/// (the reviewer's MINOR on `MB.3.37`).
///
/// The load is below the fork (`pid != init`, `ppid == init`), the child leads
/// its **own** process group (`pgid == pid`, which is what `kill(-pgid)` names),
/// it runs as the route's uid and owns nothing more, and the boot **reaped** it
/// instead of leaving the parked child behind — a boot that exits while its
/// `pause()` child lives on is a leak the next boot would inherit. `inet` is the
/// one field the two legs disagree about, so it stays in each of them.
fn assert_probe_containment(
    fields: &BTreeMap<String, u32>,
    init: u32,
    route_uid: u32,
    shape: &str,
    all: &str,
) {
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
    // The timeout must also be a *reap*, not a refusal that leaves the parked
    // child behind: `/proc/<pid>` gone is the observable end of it.
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
}

/// The lock the two probe legs take for their whole body.
///
/// `HANG_SHIM_STUB_C` bakes **one** shape path in (`BT_SHAPE`), so the hang leg
/// and its `egress = true` twin write the same file: they must not clear or
/// read it while the other's boot is running. `Boot`'s own lock is not enough —
/// it is taken *after* the `clear` and released only when the `Boot` drops,
/// after the read — so this closes exactly that window, poisoned-proof like
/// [`boot_lock`].
fn hang_shape_lock() -> std::sync::MutexGuard<'static, ()> {
    static SHAPE: Mutex<()> = Mutex::new(());
    SHAPE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

/// A PHP minor this build ships no shim for is refused with one sentence naming
/// the supported set — not with the `.dynsym` story of a symbol the object may
/// or may not export.
///
/// This is the 8.2 host a user actually has: the object is a valid NTS `libphp`,
/// and the only thing wrong with it is that Blastit does not support its minor.
/// The sentence is asserted verbatim, so a reworded or re-hedged refusal fails
/// here.
#[test]
fn an_unsupported_php_minor_refuses_the_boot_naming_the_supported_set() {
    let _root = StubRootGuard::start();
    let scratch = Scratch::new("unsupported-minor", PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    let source = scratch.write("libphp82_stub.c", LIBPHP_STUB_C);
    let libphp = scratch.dir.join("libphp8.2.so");
    compile(
        &source,
        &libphp,
        &[
            (
                "BT_CANARY",
                quote(&scratch.dir.join("libphp.canary").display().to_string()),
            ),
            ("BT_TAG", quote("libphp82")),
            ("BT_VERSION", quote("8.2.29")),
        ],
    );

    let mut boot = Boot::start(
        &scratch.dir,
        &[("BLASTIT_LIBPHP", &libphp.to_string_lossy())],
    );
    let code = boot
        .await_boot()
        .expect("an unsupported PHP minor must refuse the boot, not serve");
    boot.stop();
    let stdout = boot.stdout_text();
    let stderr = boot.stderr_text();

    assert_eq!(code, Some(1), "stdout={stdout:?} stderr={stderr:?}");
    assert_eq!(
        stdout, "",
        "a startup error writes nothing on stdout (§1.1)"
    );
    let sentence = "PHP 8.2 is not supported — Blastit supports PHP 8.3, 8.4 and 8.5";
    assert!(
        stderr.contains(sentence),
        "the refusal is exactly the support sentence: {stderr}"
    );
    assert!(
        !stderr.contains("exports no `php_version`"),
        "an unsupported minor is not explained as a missing symbol: {stderr}"
    );
    assert_no_fallback_recipe(&stderr);
    assert_no_serving_surface(&mut boot, "unsupported minor");
    println!("refused (unsupported minor): {stderr}");
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
    // The `egress = true` twin writes the same shape file; see [`probe_shape`].
    let _shape_guard = hang_shape_lock();
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

    let (shape, fields) = probe_shape(&stubs, &all);
    assert_probe_containment(&fields, init, route_uid, &shape, &all);
    let pid = fields["pid"];
    assert_eq!(
        fields["inet"], 1,
        "`EmbeddedView` must answer `socket(AF_INET)` with EPERM (1) before `worker_init`: {shape}"
    );
    println!(
        "killed at BLASTIT_PROBE_TIMEOUT_MS=250 after {elapsed:?}; probe pid {pid} ppid {} \
         pgid {} uid {} seccomp EPERM, gone from /proc\n{refusal}",
        fields["ppid"], fields["pgid"], fields["uid"]
    );
}

/// The **allowed** twin of the leg above (`ADR-0010` §3, design §7.3): the same
/// hanging shim under `egress = true`, so the probe child's
/// `SeccompRole::EmbeddedView` keeps `AF_INET` and `socket(AF_INET)` succeeds.
///
/// The denied half alone is satisfied by an implementation that hardcodes
/// `egress: false` — which is exactly `probe_bindings`'s
/// `egress.get(slot).copied().unwrap_or(false)` — while the INI for the same
/// route renders `allow_url_fopen=1`. That pair (a filter that denies and an
/// INI that permits) is the disagreement the epic's Egress doctrine forbids and
/// the one that silently breaks every PHP route that legitimately needs the
/// network. This leg is therefore a shape assertion on the filter and not a
/// second containment test: the boot must still refuse at the probe timeout,
/// naming it, with nothing bound.
#[test]
fn an_egress_route_lets_the_probe_open_a_socket() {
    let _root = StubRootGuard::start();
    // The same shape file the denied twin writes; see [`probe_shape`].
    let _shape_guard = hang_shape_lock();
    let stubs = stubs();
    clear(&stubs.hang_shape);
    let scratch = Scratch::new("egress-hang", EGRESS_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    let libphp = scratch.copy(&stubs.hang_libphp, "libphp8.4.so");
    scratch.copy(&stubs.hang_shim, &shim_name(84));
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
        "the 250 ms probe timeout did not bound the egress boot either: it took {elapsed:?}:\n{all}"
    );
    let refusal = refusal_text(&boot);
    assert_package_sentence(&refusal, "egress probe timeout");
    assert_no_fallback_recipe(&refusal);
    assert!(
        all.contains("250 ms"),
        "the refusal names the timeout it applied: {all}"
    );
    assert!(
        all.contains("BLASTIT_PROBE_TIMEOUT_MS"),
        "and the knob that would raise it: {all}"
    );
    assert_no_serving_surface(&mut boot, "egress probe timeout");

    let (shape, fields) = probe_shape(&stubs, &all);
    assert_probe_containment(&fields, init, route_uid, &shape, &all);
    let pid = fields["pid"];
    // The route's `Egress` reaches the filter: `socket(AF_INET)` returns a
    // descriptor (0), not EPERM (1). A hardcoded `egress: false` answers 1 here
    // while the same route's INI says `allow_url_fopen=1`.
    assert_eq!(
        fields["inet"], 0,
        "`egress = true` must let the probe child open an `AF_INET` socket (0), so the \
         filter and the route's INI cannot disagree: {shape}"
    );
    println!(
        "killed at BLASTIT_PROBE_TIMEOUT_MS=250 after {elapsed:?}; probe pid {pid} ppid {} \
         pgid {} uid {} egress socket allowed (inet 0), gone from /proc\n{refusal}",
        fields["ppid"], fields["pgid"], fields["uid"]
    );
}

/// The inode pin (`ADR-0010` §3, `R7`): a `libphp` that changes identity
/// between stage 0's `fstat` and stage 1's `O_NOFOLLOW` re-open must refuse the
/// boot — the worker would otherwise load a file nobody inspected.
///
/// The rename is placed **deterministically** (`MB.3.64`): stage 0 reads
/// `BLASTIT_TEST_SWAP_POINT` before the `execve` that strips every `BLASTIT_*`
/// variable and carries the path in the configuration document, and stage 1
/// pauses at that point — after stage 0 recorded `(path, st_dev, st_ino)`, before
/// [`php_view::boot_bindings`] re-opens either object. The boot creates
/// `<point>.ready`; this test waits for it, renames, and writes `<point>.go`.
/// Neither side can be early, so the window does not depend on how fast the
/// runner is — and a boot that **serves** is not a lost race but the regression
/// this test exists to catch (a stage 1 that no longer compares the re-opened
/// file's `(st_dev, st_ino)` against stage 0's record).
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
            point: scratch.dir.join("swap-point"),
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

    assert!(
        swapped,
        "stage 1 never reached the swap point, so nothing was renamed — this is the fixture or \
         the gate, not a race (the seam is a rendezvous, not a timing window):\n{all}"
    );
    let Some(inode) = all
        .lines()
        .find(|line| line.contains("is not the file stage 0 inspected"))
    else {
        match code {
            // A refusal with another sentence is a real failure.
            Some(code) => panic!(
                "the boot refused (exit {code:?}) with a sentence that is not the inode pin's:\n{all}"
            ),
            // Served: the seam held the boot and the rename still served, which
            // only a removed (or miscompared) pin can do.
            None => panic!(
                "the boot served the route even though the seam paused it between stage 0's \
                 record and stage 1's re-open: the rename landed inside the window by \
                 construction, so this is **not** a lost race. A boot that serves here means \
                 stage 1 no longer compares the re-opened `libphp`'s `(st_dev, st_ino)` against \
                 stage 0's record (`php_view::boot_bindings`/`open_pinned`) — the regression this \
                 test exists to catch:\n{all}"
            ),
        }
    };
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
        inode.contains(&libphp.display().to_string()),
        "the refusal names the library that moved: {inode}"
    );
    assert!(
        inode.contains("restore the package and restart blastit"),
        "and the fix: {all}"
    );
    assert_never_loaded(
        &[&stubs.nts_libphp_canary, &stubs.nts_shim_canary],
        "inode swap",
    );
    println!("refused (stage 1 inode pin): {all}");
}

/// The **shim** half of the same pin (`ADR-0010` §3, `R7`): stage 0 records
/// `(path, st_dev, st_ino)` for **both** objects — the library and the
/// `libblastit_php<minor>-<arch>-<libc>.so` shim — and stage 1 re-opens each and
/// compares it against its **own** record.
///
/// The library's half is the leg above; without this one, deleting the shim
/// comparison leaves every **other** protected assertion green, because the
/// library's pin still refuses its own swap and nothing else looks at the shim.
/// The shim is code that runs inside every PHP worker, so its identity is the
/// same boundary the library's is.
///
/// The miscompare — the shim re-opened and compared against the **library's**
/// `(st_dev, st_ino)`, the bug `MB.3.13.3`'s first run found — is a different
/// mutation and this leg does **not** own it: it refuses *every* boot, so this
/// leg still finds the sentence it looks for and stays green (measured by
/// `MB.3.37`'s reviewer). What catches that one is the legs that require a
/// route to be **served** (a refused boot serves nothing), plus the probe twins,
/// which require the timeout's own sentence rather than any refusal.
///
/// The fixture is the same shape as the library leg, with the two objects
/// exchanged: a **copy of the same stub** is renamed over the recorded shim
/// between stage 0's record and stage 1's re-open — the rendezvous of
/// [`a_libphp_replaced_between_the_stages_refuses_the_boot`] says exactly when —
/// so the file's content, and therefore the pair's validity, is unchanged and
/// only the inode moves. That is what makes "served" the signature of a removed
/// pin rather than of a fixture that could not have loaded.
#[test]
fn a_shim_replaced_between_the_stages_refuses_the_boot() {
    let _root = StubRootGuard::start();
    let stubs = stubs();
    clear(&stubs.nts_libphp_canary);
    clear(&stubs.nts_shim_canary);
    let scratch = Scratch::new("swap-shim", PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    let libphp = scratch.copy(&stubs.nts_libphp, "libphp8.4.so");
    let shim = scratch.copy(&stubs.nts_shim, &shim_name(84));
    let spare = scratch.copy(&stubs.nts_shim, "replacement-shim.so");
    // The premise the assertion below rests on: the replacement is the
    // recorded shim byte for byte, so the refusal can only be the inode.
    assert_eq!(
        std::fs::read(&spare).expect("the spare shim"),
        std::fs::read(&shim).expect("the recorded shim"),
        "the replacement must be a copy of the same stub, not a different shim"
    );
    // Stage 0 records the path it resolved, which is the canonical one.
    let recorded = std::fs::canonicalize(&shim).expect("the shim stage 0 records");

    let mut boot = Boot::start_swapping(
        &scratch.dir,
        &[
            ("BLASTIT_LIBPHP", &libphp.to_string_lossy()),
            ("BLASTIT_PHP_SHIM_DIR", &scratch.dir.to_string_lossy()),
        ],
        Swap {
            point: scratch.dir.join("swap-point"),
            from: spare,
            to: shim.clone(),
        },
    );
    let code = boot.await_boot();
    let swapped = boot.swapped();
    boot.stop();
    let stdout = boot.stdout_text();
    let stderr = boot.stderr_text();
    let all = format!("{stdout}\n{stderr}");

    assert!(
        swapped,
        "stage 1 never reached the swap point, so the shim swap could not be placed at all — \
         this is the fixture or the gate, not a race (the seam is a rendezvous, not a timing \
         window):\n{all}"
    );
    // The sentence that separates the shim's pin from the library's: it names
    // the shim, and only the shim.
    let Some(inode) = all
        .lines()
        .find(|line| line.contains("is not the file stage 0 inspected"))
    else {
        match code {
            // A refusal with another sentence is a real failure.
            Some(code) => panic!(
                "the boot refused (exit {code:?}) with a sentence that is not the inode pin's:\n{all}"
            ),
            // Served: the seam held the boot and the rename still served, which
            // only a removed (or miscompared) pin can do.
            None => panic!(
                "the boot served the route even though the seam paused it between stage 0's \
                 record and stage 1's re-open: the rename landed inside the window by \
                 construction, so this is **not** a lost race. A boot that serves here means \
                 stage 1 no longer compares the re-opened **shim**'s `(st_dev, st_ino)` against \
                 stage 0's record (`php_view::boot_bindings`/`open_pinned`, the `shim` call's \
                 `record.shim_dev`/`record.shim_ino`), or compares it against the library's pair \
                 — the regression this test exists to catch:\n{all}"
            ),
        }
    };
    assert_eq!(
        code,
        Some(Some(1)),
        "the shim's inode refusal is a boot refusal (exit 1): {all}"
    );
    assert!(
        inode.contains(&format!("shim {}", recorded.display())),
        "the refusal names the **shim** that moved ({}) and not the library: {inode}",
        recorded.display()
    );
    assert!(
        !inode.contains(&libphp.display().to_string()),
        "the refusal is the shim's comparison, not the library's (the library did not \
         move): {inode}"
    );
    assert!(
        inode.contains("restore the package and restart blastit"),
        "and the fix: {inode}"
    );
    assert!(
        !boot.events().iter().any(|event| event["event"] == "ready"),
        "no listener is bound before stage 1 verifies the shim's inode: {all}"
    );
    assert_no_serving_surface(&mut boot, "shim inode swap");
    assert_never_loaded(
        &[&stubs.nts_libphp_canary, &stubs.nts_shim_canary],
        "shim inode swap",
    );
    println!("refused (stage 1 shim inode pin): {all}");
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
    containment(EXIT_BODY, "exit()", "containment-exit", None);
}

/// The fatal twin: `trigger_error(..., E_USER_ERROR)` reaches `zend_bailout()`,
/// which `php_execute_script` swallows (`LESSONS.md`, `MB.3.12`).
#[test]
fn a_fatal_is_contained_and_the_next_request_is_served() {
    let _root = StubRootGuard::start();
    containment(FATAL_BODY, "fatal", "containment-fatal", None);
}

/// The same bailout with almost no CPU in it: 60 ms of `usleep` and an `exit()`.
///
/// This is the leg that makes `cost_ns >= wall_ns` an oracle: a charge taken
/// from `cpu_ns` is a small fraction of the wall time here (this host measures
/// ~1/700 of it), so it misses the epic's sentence by orders of magnitude rather
/// than by a rounding error. See [`SLEEP_BODY`].
#[test]
fn a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time() {
    let _root = StubRootGuard::start();
    containment(SLEEP_BODY, "usleep()", "containment-sleep", Some(60));
}

/// One slot, two requests: the crash answers `5xx` with its cost, the next
/// request is served by a fresh process — forked on demand, a direct child of
/// init, with the crashed one gone.
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
    let scratch = Scratch::new(name, PHP_MANIFEST);
    scratch.write(ENTRY, &crash_page(crash));

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
    // `MB.3.38`'s label, narrowed by `MB.3.41`: the handler did not return, so
    // §8.1 reports the request as `bailed` — never `ok` (the request failed),
    // and never `view_died` (a contained bailout leaves the view process alive,
    // and that word is for a process that died mid-request). The status alone
    // cannot say this, which is why `DONE` carries it.
    let outcome = event["outcome"]
        .as_str()
        .unwrap_or_else(|| panic!("{what}: no outcome: {event}"));
    assert_eq!(
        outcome, "bailed",
        "{what}: a {status} contained bailout is a handler that never returned: {event}"
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

    // The slot still answers: the same route, a fresh process, the same uid —
    // and, because the entry is pinned (`MB.3.56`), the same page the crashed
    // worker ran: the fixture dispatches on the path instead of being rewritten.
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

    // "Same slot" cannot be uid equality: `blastit dev` runs every view as the
    // caller, so the uid is one constant for the whole boot. Views are forked on
    // demand (`Blastfile` has no eager spawn and this view declares no
    // `prefork`), so one request in flight means one worker: the proof is that
    // the process serving the second request is a direct child of init, that the
    // process that crashed is gone, and that exactly one worker of this view is
    // alive.
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

/// `ADR-0010` §1.1 / `R19`: the `cpu_ns` on a completion is the worker's
/// **process CPU** (`CLOCK_PROCESS_CPUTIME_ID`), not the request's duration.
///
/// Every other fixture in this suite computes, so `cpu_ns ≈ wall_ns` by
/// construction and `cpu_ns = duration` satisfies all of them
/// (`cpu_ns <= wall_ns`, `cpu_ns > 0`, `cost_ns >= cpu_ns`). This leg's front
/// controller parks for 200 ms in `usleep`, which spends wall time and almost no
/// CPU, so the two clocks are orders of magnitude apart and the assertion is a
/// real discriminator: a shim that reported the request's wall clock fails it by
/// the whole block.
///
/// The wall time is the `request` event's `duration_ms` — arrival to last byte,
/// the number `LESSONS.md` (`MB.3.38`) records as *not* the dispatch span — and
/// never `cost_ns`, which is the charge.
#[test]
fn a_blocking_script_is_charged_cpu_time_not_wall_time() {
    let _root = StubRootGuard::start();
    let Some(host) = host_php() else {
        refused_without_a_host("blocking script", "cpu-vs-wall");
        return;
    };
    let scratch = Scratch::new("cpu-vs-wall", PHP_MANIFEST);
    scratch.write(ENTRY, &blocking_php());

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
    let started = Instant::now();
    let response = boot.get("/block");
    let elapsed = started.elapsed();
    assert!(
        response.starts_with("HTTP/1.1 200") && response.contains(BODY),
        "the blocking front controller did not answer: {response}"
    );
    let event = boot.request_event("/block");
    assert_eq!(event["status"], 200, "the event agrees: {event}");
    let cpu_ns = event["cpu_ns"]
        .as_u64()
        .unwrap_or_else(|| panic!("the worker reported no CPU cost: {event}"));
    let wall_ns = (event["duration_ms"]
        .as_f64()
        .unwrap_or_else(|| panic!("no duration: {event}"))
        * 1e6) as u64;
    // The block really is in the wall time this leg compares against: without
    // it, a `cpu_ns` that was the duration would be indistinguishable again.
    let block_ns = BLOCK_MS.saturating_mul(1_000_000);
    assert!(
        wall_ns >= block_ns,
        "the {BLOCK_MS} ms block is not in the {wall_ns} ns wall time this leg asserts against: \
         {event}"
    );
    assert!(
        cpu_ns.saturating_mul(CPU_WALL_SHARE) < wall_ns,
        "`cpu_ns` ({cpu_ns} ns) must be under a {CPU_WALL_SHARE}th of the wall time \
         ({wall_ns} ns) for a {BLOCK_MS} ms `usleep`: the reading is one process's \
         `CLOCK_PROCESS_CPUTIME_ID` delta, and a shim that reported the request's own duration \
         here is charging wall time as CPU time: {event}"
    );
    boot.stop();
    println!(
        "blocked {BLOCK_MS} ms in usleep: status 200  cpu_ns={cpu_ns}  wall_ns={wall_ns}  \
         cpu/wall=1/{}  answered in {elapsed:?}",
        wall_ns / cpu_ns.max(1)
    );
}

/// `R12` / the documented `[route.ingress] body = "form"` page
/// (`docs/frameworks/php.md`): a request body reaches PHP's `$_POST`.
///
/// The transport buffers the whole body and hands `RequestV1` a pointer and a
/// length (`blastit-view/src/php.rs`), and `php2shell`'s class is request data
/// reaching PHP — so a `RequestV1` that dropped its body would serve every form
/// as an empty `$_POST` with nothing in this suite failing. The echo is the
/// witness: the front controller answers with what it actually received.
///
/// The second half is the cap. `BLASTIT_MAX_BODY_BYTES` is one number applied
/// in two places — the scheduler's `BodyPlan::Buffer` bound (`R14`/`R15`, which
/// answers first) and the worker's own `max_body_bytes` — so a body past it is a
/// `4xx`/`5xx`, and the slot must still serve the request after it.
#[test]
fn a_form_body_reaches_the_front_controller() {
    let _root = StubRootGuard::start();
    let Some(host) = host_php() else {
        refused_without_a_host("form body", "form-body");
        return;
    };
    const FORM: &str = "application/x-www-form-urlencoded";
    let scratch = Scratch::new("form-body", FORM_MANIFEST);
    scratch.write(ENTRY, FORM_PHP);
    let max_body = FORM_BODY_MAX.to_string();

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
            ("BLASTIT_MAX_BODY_BYTES", &max_body),
        ],
    );
    assert!(
        boot.await_boot().is_none(),
        "this host has {} and {}, so the route must be served: {}",
        host.libphp.display(),
        host.shim.display(),
        boot.stderr_text()
    );

    // The allowed path: the body arrives, the codec accepts it, and PHP parses
    // it into `$_POST`.
    let response = boot.post("/form", FORM, "x=1");
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "a `form` route must answer a canonical form body: {response}"
    );
    assert!(
        response.contains("x=1"),
        "the body did not reach `$_POST`: the front controller echoed {response:?}"
    );
    let event = boot.request_event("/form");
    assert_eq!(event["status"], 200, "the event agrees: {event}");
    assert_eq!(
        event["method"], "POST",
        "the event names the method: {event}"
    );
    assert!(
        event["bytes_in"].as_u64().unwrap_or(0) >= 3,
        "the request's body bytes reached the scheduler: {event}"
    );

    // The denied path: one byte past the worker's budget. The scheduler buffers
    // the body under the same number and refuses it before dispatch, so the
    // answer is its `413` (any `4xx`/`5xx` is the contract, and it must not be a
    // `200` carrying a truncated `$_POST`).
    let oversized = format!("x={}", "a".repeat(FORM_BODY_MAX * 2));
    let refused = boot.post("/form", FORM, &oversized);
    assert!(
        refused.starts_with("HTTP/1.1 4") || refused.starts_with("HTTP/1.1 5"),
        "a body past the {FORM_BODY_MAX}-byte cap must be refused, not served: {refused}"
    );
    assert!(
        !refused.starts_with("HTTP/1.1 200"),
        "and never truncated into a `200`: {refused}"
    );

    // The slot survives the refusal: the next request is answered by the same
    // route, with its body parsed again.
    let after = boot.post("/form", FORM, "x=2");
    assert!(
        after.starts_with("HTTP/1.1 200") && after.contains("x=2"),
        "the slot stopped serving after an oversized body: {after}"
    );
    // And the scheduler agrees it served two of them: the refused body was
    // never dispatched, so the route's `200`s are the two form requests.
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        let served = boot
            .events()
            .into_iter()
            .filter(|event| {
                event["event"] == "request" && event["path"] == "/form" && event["status"] == 200
            })
            .count();
        if served >= 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "only {served} form request(s) were reported served: {}",
            boot.stdout_text()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    boot.stop();
    println!(
        "form body reached `$_POST` (x=1, 200); a {}-byte body was refused with `{}`; the slot \
         still answered x=2",
        oversized.len(),
        refused.lines().next().unwrap_or_default()
    );
}

/// `MB.3.56`, `ADR-0010` §3: the boot records `[route.php] entry` beside the
/// pair's and every request re-checks it — so a decoy **inside `manifest_dir`**
/// is refused instead of executed.
///
/// This is the case `MB.3.7` measured and left as an `info` line: moving
/// `public/` aside to a decoy *outside* the root already fails closed through
/// `open_basedir`, but nothing recorded the entry, so a swap to another file
/// inside the root answered `200 DECOY-RAN`. The pin is what makes the ADR
/// sentence true, and the marker is the witness: if `DECOY-RAN` reaches the
/// client, the check did not run.
///
/// The swap is a **rename** of a different inode over the same path — the shape
/// the identity half catches; the in-place shape has its own leg below, because a
/// witness that only covers what the mechanism happens to catch is how attempt
/// 1's gap survived. Here the original is renamed aside first, so the restore
/// puts the recorded inode back.
///
/// The accepted twin is the **restart**, not the same worker: `rename(2)` stamps
/// the moved file's own `st_ctime`, so putting the inode back moves the record
/// too — and the sentence's fix ("restart blastit to run the file as it is now")
/// is then what serves. A refusal that outlives a restore-rename is the pin
/// being honest, not a stuck worker.
///
/// Mutations this leg kills: dropping the identity comparison makes it answer
/// `200 DECOY-RAN`; dropping only the content half leaves it green (that is the
/// in-place leg's job).
#[test]
fn a_decoy_inside_the_manifest_dir_is_refused_and_a_restart_serves() {
    let _root = StubRootGuard::start();
    let Some(host) = host_php() else {
        refused_without_a_host("entry pin", "entry-pin");
        return;
    };
    let scratch = Scratch::new("entry-pin", PHP_MANIFEST);
    let entry = scratch.write(ENTRY, NORMAL_PHP);
    let pinned = std::fs::metadata(&entry).expect("the entry's metadata");
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

    // Accepted twin, first half: the untouched tree serves from the pinned inode.
    let served = boot.get("/users/42");
    assert!(
        served.starts_with("HTTP/1.1 200") && served.contains(BODY),
        "an untouched tree must serve its own entry: {served}"
    );

    // The swap: another inode at the same path, inside `manifest_dir`.
    let decoy = scratch.write("public/decoy.php", DECOY_PHP);
    let aside = scratch.dir.join("public/index.php.pinned");
    std::fs::rename(&entry, &aside).expect("the real entry aside");
    std::fs::rename(&decoy, &entry).expect("the decoy into place");
    let swapped = std::fs::metadata(&entry).expect("the swapped entry's metadata");
    assert!(
        swapped.dev() != pinned.dev() || swapped.ino() != pinned.ino(),
        "the fixture did not actually swap the inode, so nothing is being measured"
    );

    let refused = boot.get("/users/42");
    assert!(
        refused.starts_with("HTTP/1.1 5"),
        "a decoy inside `manifest_dir` must be refused, not served: {refused}"
    );
    assert!(
        !refused.contains("DECOY-RAN"),
        "the decoy ran: the entry was re-resolved instead of checked against the boot's \
         `(st_dev, st_ino)`: {refused}"
    );
    assert!(
        !refused.contains(BODY),
        "the refusal answered with the app's own body: {refused}"
    );

    // The sentence names the route, the entry and the fix, and stays out of the
    // response (it is an `app.output` line).
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        let logged = refusal_text(&boot);
        if logged.contains("changed since this boot recorded it")
            && logged.contains(&entry.display().to_string())
            && logged.contains("restart blastit to run the file as it is now")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the worker never said which entry moved and how to fix it: {logged:?}\n{}",
            boot.stdout_text()
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // The documented fix, and the accepted twin: a restart records the tree as
    // it is and serves. The rename round-trip itself is *not* enough — `rename`
    // moves the file's own `st_ctime`, so the record it was pinned to is gone.
    std::fs::rename(&aside, &entry).expect("the real entry back");
    let back = std::fs::metadata(&entry).expect("the restored entry's metadata");
    assert_eq!(
        (back.dev(), back.ino()),
        (pinned.dev(), pinned.ino()),
        "the restore must put the recorded inode back"
    );
    let still = boot.get("/users/42");
    assert!(
        still.starts_with("HTTP/1.1 5"),
        "a rename moves the entry's ctime, so the recorded file is gone: the route must keep \
         refusing until a boot records it again: {still}"
    );
    boot.stop();
    drop(boot);
    let mut fresh = Boot::start(
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
        fresh.await_boot().is_none(),
        "a boot of the restored tree must serve: {}",
        fresh.stderr_text()
    );
    let again = fresh.get("/users/42");
    assert!(
        again.starts_with("HTTP/1.1 200") && again.contains(BODY),
        "a restart must serve the restored entry (the sentence's fix): {again}"
    );
    fresh.stop();
    println!(
        "a decoy inside manifest_dir is refused; the restored tree serves after a restart \
         ({} -> {} -> {})",
        served.lines().next().unwrap_or_default(),
        refused.lines().next().unwrap_or_default(),
        again.lines().next().unwrap_or_default()
    );
}

/// `MB.3.56` attempt 2, the review's MAJOR: a **rewrite in place** keeps the
/// inode and changes only the bytes — `cp`, `tar -x` and a deploy script all do
/// it — so an identity-only pin executes the new content (`200
/// IN-PLACE-DECOY-RAN`, measured on a live route). `st_size` and `st_ctime` are
/// the content half of the record; the held `O_PATH` descriptor is the identity
/// half that makes a freed inode number un-reusable.
///
/// The decoy is **the same length** as the page it replaces, so `st_size` alone
/// cannot catch it and `st_ctime` is what must: that is the leg an identity-only
/// pin fails, and the reason the record carries both numbers.
///
/// Mutations this leg kills: dropping the content comparison (attempt 1) makes
/// it answer `200` with the marker; dropping the `O_PATH` descriptor alone does
/// not (no `rm` here), which is why that half is asserted where it can be seen —
/// `php_view`'s unit test reads `/proc/self/fd/<entry>` through the kept
/// descriptor and the child-sequence test proves it survives `close_fds_except`.
#[test]
fn an_entry_rewritten_in_place_is_refused_and_a_restart_serves_the_new_file() {
    let _root = StubRootGuard::start();
    let Some(host) = host_php() else {
        refused_without_a_host("entry in place", "entry-in-place");
        return;
    };
    let scratch = Scratch::new("entry-in-place", PHP_MANIFEST);
    let entry = scratch.write(ENTRY, NORMAL_PHP);
    let pinned = std::fs::metadata(&entry).expect("the entry's metadata");
    let libphp = host.libphp.to_string_lossy().into_owned();
    let shim_dir = host
        .shim
        .parent()
        .expect("a shim directory")
        .to_string_lossy()
        .into_owned();
    let boot_env = [
        ("BLASTIT_LIBPHP", libphp.as_str()),
        ("BLASTIT_PHP_SHIM_DIR", shim_dir.as_str()),
    ];
    let mut boot = Boot::start(&scratch.dir, &boot_env);
    assert!(
        boot.await_boot().is_none(),
        "this host has {} and {}, so the route must be served: {}",
        host.libphp.display(),
        host.shim.display(),
        boot.stderr_text()
    );
    let served = boot.get("/users/42");
    assert!(
        served.starts_with("HTTP/1.1 200") && served.contains(BODY),
        "an untouched tree must serve its own entry: {served}"
    );

    // The rewrite: the SAME inode, the same size, another script. `cp` and
    // `tar -x` write through the existing file; nothing is renamed.
    let original = std::fs::read(&entry).expect("the recorded page");
    let mut decoy = String::from("<?php echo \"IN-PLACE-DECOY-RAN\\n\"; //");
    while decoy.len() < original.len() {
        decoy.push('x');
    }
    assert_eq!(
        decoy.len(),
        original.len(),
        "the decoy must be the same length, or st_size alone would catch it"
    );
    // Cross the kernel's coarse timestamp tick before rewriting: `st_ctime`
    // advances one tick (~1–4 ms) at a time, and a same-size rewrite inside the
    // tick the boot stamped would be invisible to any metadata record
    // (`LESSONS.md`, `MB.3.56`). A live swap is always later than the boot.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&entry, &decoy).expect("rewriting the entry in place");
    let rewritten = std::fs::metadata(&entry).expect("the rewritten entry's metadata");
    assert_eq!(
        (rewritten.dev(), rewritten.ino()),
        (pinned.dev(), pinned.ino()),
        "a write in place must keep the inode, or this is the rename leg"
    );
    assert_eq!(
        rewritten.len(),
        pinned.len(),
        "and it must keep st_size, so only st_ctime can catch it"
    );
    // The premise the refusal rests on, stated here rather than inferred from the
    // `500`: a same-size rewrite moves `st_ctime`, and the sleep above is what
    // makes that true on a coarse tick. Without this assert a filesystem with a
    // tick longer than the sleep would redden this leg for the tick, not the pin.
    assert_ne!(
        (rewritten.ctime(), rewritten.ctime_nsec()),
        (pinned.ctime(), pinned.ctime_nsec()),
        "a write in place must move st_ctime, or this leg measures the clock tick"
    );

    let refused = boot.get("/users/42");
    assert!(
        refused.starts_with("HTTP/1.1 5"),
        "an entry rewritten in place must be refused, not served: {refused}"
    );
    assert!(
        !refused.contains("IN-PLACE-DECOY-RAN"),
        "the in-place decoy ran: the pin compared identity only, not size/ctime: {refused}"
    );
    assert!(
        !refused.contains(BODY),
        "the refusal answered with the app's own body: {refused}"
    );
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        let logged = refusal_text(&boot);
        if logged.contains("rewritten in place") && logged.contains("restart blastit") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the worker never named an in-place rewrite and the fix: {logged:?}\n{}",
            boot.stdout_text()
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // The fix the sentence names: put the page back (in place) and restart, so
    // the boot records what is there now. That is the deploy/restore path.
    std::fs::write(&entry, NORMAL_PHP).expect("restoring the page in place");
    boot.stop();
    drop(boot);
    let mut fresh = Boot::start(&scratch.dir, &boot_env);
    assert!(
        fresh.await_boot().is_none(),
        "a restart must serve the restored page: {}",
        fresh.stderr_text()
    );
    let again = fresh.get("/users/42");
    assert!(
        again.starts_with("HTTP/1.1 200") && again.contains(BODY),
        "a restart must serve the file as it is now: {again}"
    );
    fresh.stop();
    println!(
        "an in-place rewrite is refused; the restored page serves after a restart \
         ({} -> {} -> {})",
        served.lines().next().unwrap_or_default(),
        refused.lines().next().unwrap_or_default(),
        again.lines().next().unwrap_or_default()
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
        .expect("a host with no usable libphp cannot serve an entrypoint php file");
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

// ----------------------------------- production provenance (`R7`, `B2-R5`)

/// Distinguishes fixtures of one binary run.
static FIXTURES: AtomicUsize = AtomicUsize::new(0);

/// A directory a **deployment** boot would accept as a tree: `0755`, owned by
/// this process, with no directory above it that another uid can write.
///
/// **Not `temp_dir()`**, and that is the whole point of this helper: `/tmp` is
/// `1777`, `host::facts_of` walks every ancestor, so a `0755` fixture *under*
/// `/tmp` is still a tree a route uid could replace — which is exactly the
/// refusal these legs are about (`LESSONS.md`, `MB.3.40`). The candidates, in
/// order: the build's own `target/tmp` (`CARGO_TARGET_TMPDIR` under `cargo`,
/// and the workspace's `target/tmp` when the binaries are run *without* cargo —
/// `make rs-test-php-image` runs them in a container that bind-mounts the
/// checkout at the same absolute path as the runner's uid, while `$HOME` there
/// is the image's unwritable `/root`), `$HOME`, and the three system
/// directories a single uid usually owns.
///
/// Every rejected candidate is recorded with its reason — "not a directory",
/// `create_dir_all`'s errno, or `host::protected`'s own sentence — so the panic
/// names what was tried instead of printing `(tried )` (`LESSONS.md`,
/// `MB.3.43`). A host where all of them are unusable is a host a deployment boot
/// could not run on either, which is why this is a panic and never a skip.
fn protected_root(tag: &str) -> ProtectedRoot {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(dir) = std::env::current_dir() {
        candidates.push(dir.join("target/tmp"));
    }
    // The checkout the binary was *compiled* against: the same absolute path the
    // image bind-mounts, so it is owned by the test's uid with no world-writable
    // ancestor — the one candidate that survives a container with no cargo.
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"));
    if let Some(dir) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(dir));
    }
    for dir in ["/opt", "/usr/local", "/srv"] {
        candidates.push(PathBuf::from(dir));
    }
    let mut tried = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for parent in candidates {
        if !parent.is_dir() {
            tried.push(format!("{}: not a directory", parent.display()));
            continue;
        }
        let parent = std::fs::canonicalize(&parent).unwrap_or(parent);
        if seen.contains(&parent) {
            continue;
        }
        seen.push(parent.clone());
        let dir = parent.join(format!(
            "blastit-php-provenance-{}-{tag}-{}",
            std::process::id(),
            FIXTURES.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(err) = std::fs::create_dir_all(&dir) {
            tried.push(format!("{}: create_dir_all failed ({err})", dir.display()));
            continue;
        }
        set_mode(&dir, 0o755);
        let probe = dir.join("probe");
        let reason = match std::fs::write(&probe, b"#!/bin/sh\n") {
            Err(err) => Some(format!("a probe file cannot be written ({err})")),
            Ok(()) => {
                match std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)) {
                    Err(err) => Some(format!("the probe cannot be made 0755 ({err})")),
                    Ok(()) => match blastit::host::facts_of(&probe) {
                        Err(err) => Some(format!("the probe cannot be inspected ({err})")),
                        Ok(facts) => blastit::host::protected(&facts, effective_uid()).err(),
                    },
                }
            }
        };
        let _ = std::fs::remove_file(&probe);
        if let Some(reason) = reason {
            tried.push(format!("{}: {reason}", dir.display()));
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        return ProtectedRoot(dir);
    }
    panic!(
        "no directory on this host is one uid's alone (tried [{}]): a deployment boot could not \
         run on this host either, so the accepted twin has nothing to accept",
        tried.join("; ")
    );
}

/// The tree [`protected_root`] accepted, removed when the leg ends.
///
/// Every leg that builds a fixture under it creates `0777` directories on
/// purpose, so a fixture that outlived a panicking test would leave a writable
/// tree behind for the next run — and for the host, if it is the checkout's own
/// `target/tmp`. The guard removes it on the pass *and* on the panic path.
struct ProtectedRoot(PathBuf);

impl std::ops::Deref for ProtectedRoot {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for ProtectedRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `chmod path mode`.
fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|err| panic!("chmod {mode:o} {}: {err}", path.display()));
}

/// The **deployment** authorisation of a set of trees (`R7`, `B2-R5`), exactly as
/// `stage0::deployment_refusal` builds it: `host::Provenance` against init's uid.
fn trees_refusal(claims: Vec<Claim>) -> Result<(), String> {
    Provenance {
        init_uid: effective_uid(),
        objects: Vec::new(),
        trees: claims,
    }
    .check_trees()
}

/// The same for the objects a PHP worker `dlopen`s: the NTS `libphp` and its
/// per-minor shim, which are also asked for file capabilities and a search path.
fn objects_refusal(claims: Vec<Claim>) -> Result<(), String> {
    Provenance {
        init_uid: effective_uid(),
        objects: claims,
        trees: Vec::new(),
    }
    .check_objects()
}

/// The first non-empty HTTP answer from a served boot, retried a bounded number
/// of times.
///
/// A request that beats the `http` role's first `accept` can come back as an
/// empty stream; that is a harness race, not a refusal, and the leg that uses
/// this is about the **gate** (does a deployment rule refuse this tree?) rather
/// than about accept latency. Five seconds is far longer than the boot's own
/// `ready`, so a boot that died says so through the timeout's message.
fn first_answer(boot: &Boot) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = boot.get("/");
        if !response.is_empty() {
            return response;
        }
        assert!(
            Instant::now() < deadline,
            "the dev boot never answered over HTTP, so it never served:\n{}",
            boot.stderr_text()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// A fixture `Blastfile` directory, at `mode`, with a `0644` front controller
/// and the manifest the documented page uses.
fn manifest_tree(root: &Path, mode: u32) -> PathBuf {
    let app = root.join("app");
    std::fs::create_dir_all(app.join("public")).expect("a fixture directory");
    std::fs::write(app.join("Blastfile"), PHP_MANIFEST).expect("the manifest");
    std::fs::write(app.join(ENTRY), NORMAL_PHP).expect("the front controller");
    set_mode(&app.join(ENTRY), 0o644);
    set_mode(&app, mode);
    app
}

/// **`R7`/`B2-R5`, the tree half.** A deployment refuses a `Blastfile`
/// directory a route uid could write, because that directory is every PHP
/// worker's `cwd` and `include_path` (`ADR-0010` §2): a route uid that can write
/// a file there chooses what the worker `include`s.
///
/// **Unit-level, and it says so.** An unprivileged process cannot reach the
/// `Production` half of stage 0 at all — `caps::detect` refuses the boot before
/// `Config::from_env` (`LESSONS.md`, `MB.1.6`) — so this leg drives the gate's
/// own decision function, `blastit::host::Provenance::check_trees`, which is the
/// call `stage0::deployment_refusal` makes before `Provenance` is handed to
/// stage 1. The sentence and the order are pinned by
/// `stage0::php_boot_gate`'s `a_deployment_boot_refuses_a_tree_a_route_uid_could_write`,
/// and the **end-to-end** half is
/// [`a_dev_boot_of_a_world_writable_tree_still_serves`] below. Removing the
/// check fails this leg (see `RESULT.md`'s red run).
#[test]
fn a_world_writable_manifest_root_refuses_a_production_boot() {
    let root = protected_root("manifest");
    let app = manifest_tree(&root, 0o755);
    let what = "the directory holding the Blastfile";

    // The permission bits, not the path, are the decision: the same tree the
    // same fixture just created is the accepted shape at 0755.
    assert!(
        trees_refusal(vec![Claim::new(what, &app)]).is_ok(),
        "a 0755 tree owned by init's uid is what a deployment boots"
    );
    assert!(
        trees_refusal(vec![Claim::new("entrypoint php", app.join(ENTRY))]).is_ok(),
        "and its 0644 front controller is part of the accepted tree"
    );

    set_mode(&app, 0o777);
    let err = trees_refusal(vec![Claim::new(what, &app)])
        .expect_err("a world-writable manifest directory must refuse a deployment boot");
    assert!(
        err.contains(&app.display().to_string()),
        "the refusal names the offending path: {err}"
    );
    assert!(
        err.contains("could be replaced by another uid"),
        "and what is wrong with it: {err}"
    );
    assert!(
        err.contains("chown") && err.contains("chmod"),
        "and the fix: {err}"
    );
    assert!(
        !err.contains("install the PHP embed package"),
        "a tree is not fixed by installing a package: {err}"
    );

    // The front controller inside a protected tree is the same refusal: a
    // `0666` `index.php` is a file any route uid can rewrite.
    set_mode(&app, 0o755);
    set_mode(&app.join(ENTRY), 0o666);
    let err = trees_refusal(vec![Claim::new(
        "route \"app\": entrypoint php \"public/index.php\"",
        app.join(ENTRY),
    )])
    .expect_err("a world-writable front controller must refuse a deployment boot");
    assert!(
        err.contains(&app.join(ENTRY).display().to_string()),
        "{err}"
    );
    assert!(err.contains("entrypoint php"), "{err}");

    // `root` is a `ProtectedRoot`: it removes the fixture, panic path included.
    println!("refused (production provenance, tree half): {err}");
}

/// **`R7`, the pair half.** A shim (or `libphp`) whose directory a route uid can
/// write is a file that uid owns the contents of, and every `[route.php]` worker
/// `dlopen`s it — so a deployment refuses it, at the shim, not at the library
/// that is fine.
///
/// Unit-level for the same reason as the leg above: the decision is
/// `blastit::host::Provenance::check_objects`, the call `stage0` makes on the
/// resolved pair before the `(path, st_dev, st_ino)` record is written.
#[test]
fn a_world_writable_shim_directory_refuses_a_production_boot() {
    let _root = StubRootGuard::start();
    let stubs = stubs();
    let root = protected_root("shim");
    let libs = root.join("lib");
    let shims = root.join("shims");
    std::fs::create_dir_all(&libs).expect("a library directory");
    std::fs::create_dir_all(&shims).expect("a shim directory");
    set_mode(&libs, 0o755);
    set_mode(&shims, 0o777);
    let libphp = libs.join("libphp8.4.so");
    let shim = shims.join(shim_name(84));
    std::fs::copy(&stubs.nts_libphp, &libphp).expect("the stub library");
    std::fs::copy(&stubs.nts_shim, &shim).expect("the stub shim");
    set_mode(&libphp, 0o755);
    set_mode(&shim, 0o755);

    // The library on its own, in a directory only init's uid can write, is the
    // accepted shape — so what the assertion below catches is the shim.
    assert!(
        objects_refusal(vec![Claim::new("the NTS libphp", &libphp)]).is_ok(),
        "a 0755 pair in a protected directory is what a deployment boots"
    );

    let err = objects_refusal(vec![
        Claim::new("the NTS libphp", &libphp),
        Claim::new("the embedded shim", &shim),
    ])
    .expect_err("a shim in a world-writable directory must refuse a deployment boot");
    assert!(
        err.contains(&shim.display().to_string()),
        "the refusal names the shim: {err}"
    );
    assert!(err.contains("the embedded shim"), "{err}");
    assert!(err.contains("could be replaced by another uid"), "{err}");
    assert!(err.contains("chown"), "and the fix: {err}");

    // The bits are the decision: the same shim in a 0755 directory is accepted.
    set_mode(&shims, 0o755);
    assert!(
        objects_refusal(vec![
            Claim::new("the NTS libphp", &libphp),
            Claim::new("the embedded shim", &shim),
        ])
        .is_ok(),
        "the same pair at 0755 is what a deployment boots"
    );

    println!("refused (production provenance, pair half): {err}");
}

/// **`R7`, the dynamic half.** An object whose `DT_RPATH`/`DT_RUNPATH` names a
/// directory a route uid can write is one that uid chooses the `DT_NEEDED`
/// resolution of: it drops `libz.so.1` there and the worker loads it. The
/// fixture is a real ELF this suite links itself, and the search path is read
/// from the file (`host::dynamic_search_paths`), never by loading it.
///
/// Both spellings are covered, because the two linkers disagree:
/// `--disable-new-dtags` writes `DT_RPATH` and the modern default writes
/// `DT_RUNPATH`. The same source with no `-rpath` at all is accepted, so the
/// refusal is the tag and not the stub.
#[test]
fn a_runpath_into_a_writable_directory_refuses_a_production_boot() {
    let root = protected_root("runpath");
    let evil = root.join("evil");
    let libs = root.join("lib");
    std::fs::create_dir_all(&evil).expect("a writable directory");
    std::fs::create_dir_all(&libs).expect("a library directory");
    set_mode(&evil, 0o777);
    set_mode(&libs, 0o755);
    let source = root.join("libphp_stub.c");
    std::fs::write(&source, LIBPHP_STUB_C).expect("the stub source");
    let defines = [
        ("BT_CANARY", quote("/nonexistent/canary")),
        ("BT_TAG", quote("libphp")),
        ("BT_VERSION", quote("8.4.27")),
    ];
    let entry = evil.display().to_string();

    let legacy = libs.join("libphp8.4-rpath.so");
    compile_linked(
        &source,
        &legacy,
        &defines,
        &[format!("-Wl,--disable-new-dtags,-rpath,{entry}")],
    );
    let modern = libs.join("libphp8.4-runpath.so");
    compile_linked(&source, &modern, &defines, &[format!("-Wl,-rpath,{entry}")]);
    let clean = libs.join("libphp8.4.so");
    compile(&source, &clean, &defines);

    // The walk reads what `readelf -d` reads, and nothing else is in the string
    // table: this is the assertion that makes the refusals below about the tag.
    for object in [&legacy, &modern] {
        assert_eq!(
            blastit::host::dynamic_search_paths(object).expect("a search path this build reads"),
            vec![entry.clone()],
            "{}",
            object.display()
        );
        assert!(
            std::fs::read(object)
                .expect("the object")
                .windows(b"blastit-php-provenance".len())
                .any(|window| window == b"blastit-php-provenance"),
            "the fixture really is the directory this test made writable"
        );
    }
    assert_eq!(
        blastit::host::dynamic_search_paths(&clean).expect("no search path"),
        Vec::<String>::new(),
        "the accepted twin carries no DT_RPATH/DT_RUNPATH at all"
    );

    for object in [&legacy, &modern] {
        let err = objects_refusal(vec![Claim::new("the NTS libphp", object)])
            .expect_err("a writable search path must refuse a deployment boot");
        assert!(
            err.contains(&entry),
            "{}: the refusal names the entry: {err}",
            object.display()
        );
        assert!(
            err.contains("DT_RPATH/DT_RUNPATH"),
            "{}: and the tag: {err}",
            object.display()
        );
        assert!(
            err.contains("-rpath"),
            "{}: and the fix: {err}",
            object.display()
        );
    }
    assert!(
        objects_refusal(vec![Claim::new("the NTS libphp", &clean)]).is_ok(),
        "the same object with no search path is what a deployment boots"
    );

    println!(
        "refused (DT_RPATH and DT_RUNPATH into {entry}); the same object without -rpath is \
         accepted"
    );
}

/// **`R7`/`B2-R5`, the gate's *call site*.** The three legs above drive
/// `host::Provenance` — the decision `stage0::deployment_refusal` makes — and
/// cannot see whether the boot **makes** that call: deleting it from
/// `php_boot_in` leaves them green, which is `MB.3.40`'s reviewer's first
/// finding and the reason this leg exists.
///
/// It runs the same function a deployment boot runs — `stage0::php_boot_in` with
/// a `Deployment` — through the one seam that reaches it,
/// `blastit::host::deployment_boot_check` (`#[doc(hidden)]`: `stage0` is a
/// private module and an unprivileged test cannot reach a `Production` boot at
/// all, `LESSONS.md` `MB.1.6`). It requires **Refused** in front of
/// **Proceed**: the world-writable tree and the world-writable shim directory
/// are each refused with their own sentence, and the same fixture is accepted
/// before and after, so the refusals are the authorisation and not the fixture.
#[test]
fn the_deployment_gate_is_invoked_behind_its_sentence() {
    let _root = StubRootGuard::start();
    let stubs = stubs();
    let root = protected_root("gate");
    let app = manifest_tree(&root, 0o755);
    let libs = root.join("lib");
    std::fs::create_dir_all(&libs).expect("a library directory");
    set_mode(&libs, 0o755);
    let libphp = libs.join("libphp8.4.so");
    let shim = libs.join(shim_name(84));
    std::fs::copy(&stubs.nts_libphp, &libphp).expect("the stub library");
    std::fs::copy(&stubs.nts_shim, &shim).expect("the stub shim");
    set_mode(&libphp, 0o755);
    set_mode(&shim, 0o755);

    // `stage0`'s own unit tests parse these manifests with the same base; this
    // one names no uid, so the value only has to be a legal base.
    let check = |dir: &Path, shim_dir: &Path| {
        blastit::host::deployment_boot_check(PHP_MANIFEST, dir, &libphp, shim_dir, 20_000)
    };

    assert_eq!(
        check(&app, &libs),
        Ok(()),
        "a 0755 tree and pair owned by init's uid is what a deployment boots: {:?}",
        check(&app, &libs)
    );

    // 0. A file with no `entrypoint php` has nothing to authorise, and a
    // deployment boots it: the seam says `Ok`, so a caller cannot read a
    // proceeding manifest as a refusal (`MB.3.44`).
    assert_eq!(
        blastit::host::deployment_boot_check(STATIC_ONLY_MANIFEST, &app, &libphp, &libs, 20_000),
        Ok(()),
        "a manifest with no entrypoint php proceeds through the deployment gate: {:?}",
        blastit::host::deployment_boot_check(STATIC_ONLY_MANIFEST, &app, &libphp, &libs, 20_000)
    );

    // 1. The tree: the accepted manifest directory, world-writable.
    set_mode(&app, 0o777);
    let err = check(&app, &libs).expect_err("a deployment boot must run the tree authorisation");
    assert!(
        err.contains(&app.display().to_string()),
        "the offending tree is named: {err}"
    );
    assert!(
        err.contains("could be replaced by another uid"),
        "and what is wrong with it: {err}"
    );
    assert!(err.contains("chown"), "and the fix: {err}");
    set_mode(&app, 0o755);

    // 2. The pair: a 0755 tree and library, with the shim in a world-writable
    // directory — so the refusal is the shim's and not the library's.
    let shims = root.join("shims");
    std::fs::create_dir_all(&shims).expect("a shim directory");
    set_mode(&shims, 0o777);
    let shim_in_shims = shims.join(shim_name(84));
    std::fs::copy(&stubs.nts_shim, &shim_in_shims).expect("the stub shim");
    set_mode(&shim_in_shims, 0o755);
    let err = check(&app, &shims).expect_err("a deployment boot must run the pair authorisation");
    assert!(err.contains("the embedded shim"), "{err}");
    assert!(
        err.contains(&shim_in_shims.display().to_string()),
        "the offending shim is named: {err}"
    );
    assert!(
        !err.contains(&shim.display().to_string()),
        "the shim in the protected directory is not the one refused: {err}"
    );
    assert!(err.contains("could be replaced by another uid"), "{err}");
    set_mode(&shims, 0o755);

    // 3. The bits are the decision: the same fixture at 0755 proceeds.
    assert_eq!(
        check(&app, &libs),
        Ok(()),
        "the same tree and pair at 0755 must proceed: {:?}",
        check(&app, &libs)
    );
    println!(
        "the deployment gate proceeded on the manifest with no entrypoint php, refused the 0777 \
         tree and the 0777 shim directory, and proceeded once both were 0755"
    );
}

/// **The accepted twin** (`R7`, `B2-R5`): a whole deployment — the pair, the
/// manifest directory, the front controller and a `static` root — at the
/// modes an install has, owned by init's uid, is accepted. Then one directory is
/// made world-writable and the same check refuses, so the acceptance is the
/// permission bits and cannot be vacuous.
#[test]
fn the_same_tree_owned_by_init_is_accepted() {
    let _root = StubRootGuard::start();
    let stubs = stubs();
    let root = protected_root("accepted");
    let app = manifest_tree(&root, 0o755);
    let site = app.join("site");
    std::fs::create_dir_all(&site).expect("a static root");
    std::fs::write(site.join("index.html"), b"<!doctype html>\n").expect("a static page");
    set_mode(&site, 0o755);
    let libs = root.join("lib");
    std::fs::create_dir_all(&libs).expect("a library directory");
    set_mode(&libs, 0o755);
    let libphp = libs.join("libphp8.4.so");
    let shim = libs.join(shim_name(84));
    std::fs::copy(&stubs.nts_libphp, &libphp).expect("the stub library");
    std::fs::copy(&stubs.nts_shim, &shim).expect("the stub shim");
    set_mode(&libphp, 0o755);
    set_mode(&shim, 0o755);

    let objects = || {
        vec![
            Claim::new("the NTS libphp", &libphp),
            Claim::new("the embedded shim", &shim),
        ]
    };
    let trees = || {
        vec![
            Claim::new("the directory holding the Blastfile", &app),
            Claim::new(
                "route \"app\": entrypoint php \"public/index.php\"",
                app.join(ENTRY),
            ),
            Claim::new("static \"/\": root \"site\"", &site),
        ]
    };
    assert!(
        objects_refusal(objects()).is_ok() && trees_refusal(trees()).is_ok(),
        "an install at 0755/0644 owned by init's uid is what a deployment boots: {:?} {:?}",
        objects_refusal(objects()),
        trees_refusal(trees())
    );
    // Every path is owned by init's uid (this process), which is the ownership
    // rule's allowed branch rather than an accident of the fixture.
    for path in [&libphp, &shim, &app, &app.join(ENTRY), &site] {
        assert_eq!(
            std::fs::metadata(path).expect("the fixture").uid(),
            effective_uid(),
            "{} is owned by init's uid",
            path.display()
        );
    }

    // One directory at 0777 and the same check refuses: the bits, not the path.
    set_mode(&site, 0o777);
    let err = trees_refusal(trees()).expect_err("a 0777 static root must refuse");
    assert!(err.contains(&site.display().to_string()), "{err}");

    println!("accepted at 0755/0644 owned by init's uid; refused once one directory is 0777");
}

/// **The `Production`-scoped twin, end to end.** The same `0777` fixture the
/// legs above refuse — a `Blastfile` directory a route uid can write, with
/// the stub pair inside it — still boots and serves under `blastit dev`, because
/// `ADR-0010` §3's provenance rule is a deployment rule (`R7`). A developer's
/// shim lives in a checkout their own uid owns and writes, so asking "could
/// another uid replace it" there protects nothing and would refuse every
/// development boot.
///
/// This is the leg that is end to end: a real `blastit dev`, a real listener, a
/// real answering port. Removing the scope split (applying the rule in
/// `Development` too) makes it fail.
#[test]
fn a_dev_boot_of_a_world_writable_tree_still_serves() {
    let scratch = Scratch::new("dev-provenance", PHP_MANIFEST);
    scratch.write(ENTRY, NORMAL_PHP);
    // The stub pair is compiled *here*, with this fixture's own canaries: the
    // shared `stubs()` pair is the ZTS and inode-swap legs' evidence, and a boot
    // that loaded it would write into their files.
    let source = scratch.write("libphp_stub.c", LIBPHP_STUB_C);
    let libphp = scratch.dir.join("libphp8.4.so");
    compile(
        &source,
        &libphp,
        &[
            (
                "BT_CANARY",
                quote(&scratch.dir.join("libphp.canary").display().to_string()),
            ),
            ("BT_TAG", quote("libphp")),
            ("BT_VERSION", quote("8.4.27")),
        ],
    );
    let shim_source = scratch.write("shim_stub.c", SHIM_STUB_C);
    let shim = scratch.dir.join(shim_name(84));
    compile(
        &shim_source,
        &shim,
        &[
            (
                "BT_CANARY",
                quote(&scratch.dir.join("shim.canary").display().to_string()),
            ),
            ("BT_TAG", quote("shim84")),
            ("BT_MINOR", "804".to_string()),
        ],
    );
    // The fixture is exactly what the legs above refuse: the tree and the pair
    // live in a directory any uid can write.
    set_mode(&scratch.dir, 0o777);
    assert_eq!(
        std::fs::metadata(&scratch.dir).expect("the fixture").mode() & 0o777,
        0o777,
        "the fixture must be the world-writable tree the other legs refuse"
    );
    assert!(
        trees_refusal(vec![Claim::new(
            "the directory holding the Blastfile",
            &scratch.dir
        )])
        .is_err(),
        "and it really is refused by the deployment rule"
    );

    let mut boot = Boot::start(
        &scratch.dir,
        &[
            ("BLASTIT_LIBPHP", &libphp.to_string_lossy()),
            ("BLASTIT_PHP_SHIM_DIR", &scratch.dir.to_string_lossy()),
        ],
    );
    assert!(
        boot.await_boot().is_none(),
        "`blastit dev` must still serve a 0777 checkout (the rule is Production-only): {}",
        boot.stderr_text()
    );
    // The port accepts before stage 1's `ready`, so the event is *waited* for
    // rather than snapshotted: `await_boot` returns the moment anything accepts.
    assert!(
        boot.wait_event(BOOT_TIMEOUT, |event| event["event"] == "ready")
            .is_some(),
        "a dev boot that serves announces `ready`: {}",
        boot.stdout_text()
    );
    // It answers: a listener that binds and then refuses is not "serving". The
    // stub shim has no request entry, so the answer is a `5xx` — what matters is
    // that the boot accepted the request instead of refusing to start.
    let response = first_answer(&boot);
    assert!(
        response.starts_with("HTTP/1.1 "),
        "the dev boot answered over HTTP: {response}"
    );
    boot.stop();
    println!(
        "`blastit dev` served a 0777 tree the deployment rule refuses: {}",
        response.lines().next().unwrap_or_default()
    );
}
