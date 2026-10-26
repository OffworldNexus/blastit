//! Scheduler configuration (ARCHITECTURE.md §2.4, §8.2).
//!
//! Every field mirrors one environment variable, but the scheduler never reads
//! the environment: init parses `BLASTIT_*` in stage 1 and hands the values
//! over (ARCHITECTURE.md §4, §8.2). The defaults here are the documented
//! defaults of those variables.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use blastit_frame::{ClientIpConfig, Limits};

/// How long the scheduler waits before asking init for a view it just lost
/// (§2.4: "don't spin: back off before respawning the same route").
pub const SPAWN_BACKOFF: Duration = Duration::from_secs(1);

/// How often the fair queue's idle flows are collected (D5).
///
/// A flow's own request end collects it; this is the sweep for one whose last
/// request ended while the queue stayed busy, so the tables cannot grow with
/// the number of clients that have ever connected.
pub const FAIR_GC_INTERVAL: Duration = Duration::from_secs(1);

/// §2.4's default for `BLASTIT_FQ_BURST`: what a client may spend before it
/// starts queueing behind the others.
pub const FQ_BURST_DEFAULT: Duration = Duration::from_millis(20);

/// §2.4's default for `BLASTIT_FQ_MAX_DEBT`: how far ahead of the queue's
/// virtual time one flow may borrow.
pub const FQ_MAX_DEBT_DEFAULT: Duration = Duration::from_secs(5);

/// §2.4's default for `BLASTIT_FQ_DEBT_TTL`: how long an idle debtor is kept.
pub const FQ_DEBT_TTL_DEFAULT: Duration = Duration::from_secs(30);

/// §2.4's default for `BLASTIT_FQ_OUTER_MAX`.
pub const FQ_OUTER_MAX_DEFAULT: usize = 65_536;

/// §2.4's default for `BLASTIT_FQ_INNER_MAX_PER_OUTER`.
pub const FQ_INNER_MAX_PER_OUTER_DEFAULT: usize = 1_024;

/// §2.4's default for `BLASTIT_FQ_INNER_MAX`.
pub const FQ_INNER_MAX_DEFAULT: usize = 262_144;

/// §2.4's defaults for `BLASTIT_FQ_OUTER_PREFIX_V4` and `_V6`.
pub const FQ_OUTER_PREFIX_V4_DEFAULT: u8 = 24;
/// See [`FQ_OUTER_PREFIX_V4_DEFAULT`].
pub const FQ_OUTER_PREFIX_V6_DEFAULT: u8 = 56;

/// The headers `BLASTIT_FQ_INNER_HEADERS` hashes by default.
pub const FQ_INNER_HEADERS_DEFAULT: &[&str] = &[
    "user-agent",
    "accept-language",
    "sec-ch-ua",
    "sec-ch-ua-platform",
    "sec-ch-ua-mobile",
    "authorization",
];

/// The cookies `BLASTIT_FQ_INNER_COOKIES` hashes by default.
pub const FQ_INNER_COOKIES_DEFAULT: &[&str] = &["sessionid", "session"];

/// §2.4's fair-queuing knobs, mirroring the reference implementation's
/// `--fq-*` options (D5).
///
/// Every field has its own environment variable; the defaults here are §2.4's,
/// which are the reference implementation's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FairConfig {
    /// `BLASTIT_FQ_OUTER_PREFIX_V4`: how much of an IPv4 address an outer flow
    /// keeps.
    pub outer_prefix_v4: u8,
    /// `BLASTIT_FQ_OUTER_PREFIX_V6`: the same for IPv6.
    pub outer_prefix_v6: u8,
    /// `BLASTIT_FQ_OUTER_MAX`: outer flows per worker before the overflow flow
    /// takes over.
    pub outer_max: usize,
    /// `BLASTIT_FQ_INNER_MAX_PER_OUTER`: inner flows one outer flow may hold.
    pub inner_max_per_outer: usize,
    /// `BLASTIT_FQ_INNER_MAX`: inner flows per worker.
    pub inner_max: usize,
    /// `BLASTIT_FQ_BURST`: what a flow may spend before it starts queueing.
    pub burst: Duration,
    /// `BLASTIT_FQ_MAX_DEBT`: how far ahead of the clock one flow may borrow.
    pub max_debt: Duration,
    /// `BLASTIT_FQ_DEBT_TTL`: how long an idle flow with debt is kept.
    pub debt_ttl: Duration,
    /// `BLASTIT_FQ_INNER_HEADERS`: headers hashed into the inner flow key.
    pub inner_headers: Vec<String>,
    /// `BLASTIT_FQ_INNER_COOKIES`: cookies hashed into the inner flow key.
    pub inner_cookies: Vec<String>,
    /// `BLASTIT_FQ_INNER_INCLUDE_IP`: whether the full client address is part
    /// of the inner flow key.
    pub inner_include_ip: bool,
    /// `BLASTIT_CLIENT_IP_HEADER` / `BLASTIT_TRUSTED_PROXIES`: how the client
    /// address is resolved, exactly as the front end resolves `REMOTE_ADDR`
    /// (D5).
    pub client_ip: ClientIpConfig,
}

impl Default for FairConfig {
    fn default() -> Self {
        Self {
            outer_prefix_v4: FQ_OUTER_PREFIX_V4_DEFAULT,
            outer_prefix_v6: FQ_OUTER_PREFIX_V6_DEFAULT,
            outer_max: FQ_OUTER_MAX_DEFAULT,
            inner_max_per_outer: FQ_INNER_MAX_PER_OUTER_DEFAULT,
            inner_max: FQ_INNER_MAX_DEFAULT,
            burst: FQ_BURST_DEFAULT,
            max_debt: FQ_MAX_DEBT_DEFAULT,
            debt_ttl: FQ_DEBT_TTL_DEFAULT,
            inner_headers: FQ_INNER_HEADERS_DEFAULT
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            inner_cookies: FQ_INNER_COOKIES_DEFAULT
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            inner_include_ip: true,
            client_ip: ClientIpConfig::default(),
        }
    }
}

/// How often the memory high watermark is checked (§8 Sizing).
pub const MEMORY_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// §8 Sizing's memory reserve: what is kept free for the kernel, the platform's
/// agents and the next view process, `max(64 MiB, 10 % of total)`.
pub const MIN_MEM_RESERVE: u64 = 64 * 1024 * 1024;

/// Reads the machine's current memory usage, in bytes (§8 Sizing).
///
/// §9.2 #8: only aggregates are reliable — `VmRSS` does not move under gVisor
/// — so init wires this to the cgroup's own accounting (or, without cgroups,
/// `/proc/meminfo`). [`MemoryProbe::read`] returning `None` means "could not be
/// read", and every caller then does nothing rather than acting on a number it
/// does not have.
#[derive(Clone)]
pub struct MemoryProbe {
    read: Arc<dyn Fn() -> Option<u64> + Send + Sync>,
}

impl MemoryProbe {
    /// A probe around `read`.
    pub fn new(read: impl Fn() -> Option<u64> + Send + Sync + 'static) -> Self {
        Self {
            read: Arc::new(read),
        }
    }

    /// The current usage in bytes, or `None` when it cannot be read.
    pub fn read(&self) -> Option<u64> {
        (self.read)()
    }
}

impl fmt::Debug for MemoryProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The closure is not printable; `Clone` and `read` are its whole API.
        f.write_str("MemoryProbe")
    }
}

/// §8 Sizing's reserve for a machine of `total` bytes.
pub fn mem_reserve(total: u64) -> u64 {
    (total / 10).max(MIN_MEM_RESERVE)
}

/// §8 Sizing's derived default for `BLASTIT_MAX_VIEW_PROCESSES`.
///
/// `max(1, (total − used − reserve) / estimate)`: one view process always, so
/// a tiny instance is still usable, and `used` is clamped so a reading that
/// exceeds `total` cannot underflow. Stage 1 reports the same arithmetic in the
/// `boot` event, which is why it is a public function.
pub fn derived_view_cap(total: u64, used: u64, estimate: u64) -> u32 {
    let free = total
        .saturating_sub(used)
        .saturating_sub(mem_reserve(total));
    let width = free / estimate.max(1);
    u32::try_from(width).unwrap_or(u32::MAX).max(1)
}

impl FairConfig {
    /// `BLASTIT_FQ_BURST`, in nanoseconds.
    ///
    /// The virtual clock is nanoseconds, so the knobs are read once per call
    /// rather than once per request.
    pub(crate) fn burst_ns(&self) -> u64 {
        nanos(self.burst)
    }

    /// `BLASTIT_FQ_MAX_DEBT`, in nanoseconds.
    pub(crate) fn max_debt_ns(&self) -> u64 {
        nanos(self.max_debt)
    }
}

/// Nanoseconds in a duration, saturating.
pub(crate) fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// Everything the scheduler needs to know about its budgets and timers.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// `BLASTIT_MAX_WORKERS`: live request threads across every view process.
    pub max_workers: u32,
    /// `BLASTIT_VIEW_MAX_WORKERS`: per-view thread cap. `None` means
    /// `BLASTIT_MAX_WORKERS`.
    pub view_max_workers: Option<u32>,
    /// `BLASTIT_QUEUE_SIZE`: requests that may wait for a worker slot.
    pub queue_size: usize,
    /// `BLASTIT_QUEUE_TIMEOUT`: how long one of them may wait.
    pub queue_timeout: Duration,
    /// `BLASTIT_FAIR_QUEUING=on`: order the admission queue by the two-level
    /// fair scheduler instead of first-in-first-out (D5).
    ///
    /// `off` (the default) is exactly the §2.4 FIFO.
    pub fair_queuing: bool,
    /// The `BLASTIT_FQ_*` knobs of that scheduler (D5).
    pub fair: FairConfig,
    /// `BLASTIT_REQUEST_TIMEOUT`: time until the client gets a `504`.
    pub request_timeout: Duration,
    /// `BLASTIT_STUCK_THREADS`: overdue threads before a view is recycled.
    pub stuck_threads: u32,
    /// `BLASTIT_VIEW_SPAWN=eager`: spawn every view at startup.
    pub eager_spawn: bool,
    /// `BLASTIT_VIEW_IDLE_TIMEOUT`: retire an idle view after this long. Zero
    /// disables idle retirement.
    pub idle_timeout: Duration,
    /// `BLASTIT_VIEW_MAX_REQUESTS`: retire a view after this many dispatches.
    /// Zero disables the cap.
    pub max_requests: u64,
    /// `BLASTIT_MAX_VIEW_PROCESSES`: live view process cap, enforced by
    /// retiring the least-recently-used idle view. `None` means the §8 default
    /// derived from memory (see [`derived_view_cap`]), which the scheduler
    /// takes before its startup burst and again once init reports a preloaded
    /// view process (§4).
    pub max_view_processes: Option<u32>,
    /// `BLASTIT_HEALTH_PATH`: answered by the scheduler itself.
    pub health_path: String,
    /// `BLASTIT_SCRIPT_NAME`: mount prefix stripped before routing.
    pub script_name: Option<String>,
    /// `BLASTIT_FRAME_MAX`: the frame limit of every channel the scheduler
    /// speaks on.
    pub frame_limits: Limits,
    /// How long to wait before respawning a route whose last spawn failed.
    pub spawn_backoff: Duration,
    /// Reads the machine's current memory usage (§8 Sizing).
    pub memory_usage: Option<MemoryProbe>,
    /// `BLASTIT_MEM_HIGH_WATERMARK`: retire one idle view above this usage, in
    /// bytes. `None` disables the check.
    pub mem_high_watermark_bytes: Option<u64>,
    /// How often the watermark is checked ([`MEMORY_CHECK_INTERVAL`]).
    pub memory_check_interval: Duration,
    /// The memory the derived cap is measured against, in bytes: `min(MemTotal,
    /// cgroup limit)` (§8 Sizing). `None` leaves the cap unset.
    pub mem_total_bytes: Option<u64>,
    /// `BLASTIT_VIEW_MEM_ESTIMATE`, in bytes: what one more view process is
    /// expected to cost.
    pub view_mem_estimate: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_workers: 16,
            view_max_workers: None,
            queue_size: 256,
            queue_timeout: Duration::from_secs(10),
            fair_queuing: false,
            fair: FairConfig::default(),
            request_timeout: Duration::from_secs(30),
            stuck_threads: 4,
            eager_spawn: false,
            idle_timeout: Duration::from_secs(600),
            max_requests: 0,
            max_view_processes: None,
            health_path: "/_blastit/health".to_string(),
            script_name: None,
            frame_limits: Limits::default(),
            spawn_backoff: SPAWN_BACKOFF,
            memory_usage: None,
            mem_high_watermark_bytes: None,
            memory_check_interval: MEMORY_CHECK_INTERVAL,
            mem_total_bytes: None,
            view_mem_estimate: 32 * 1024 * 1024,
        }
    }
}

impl SchedulerConfig {
    /// The per-view thread cap: `BLASTIT_VIEW_MAX_WORKERS`, or the global
    /// budget when that is unset (§8.2).
    pub fn view_worker_cap(&self) -> u32 {
        self.view_max_workers.unwrap_or(self.max_workers)
    }

    /// Whether the memory high watermark can be watched at all: §8 Sizing needs
    /// both a usage to read and a watermark to compare it against.
    pub fn watches_memory(&self) -> bool {
        self.memory_usage.is_some() && self.mem_high_watermark_bytes.is_some()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// One mebibyte, for arithmetic that reads the way §8 writes it.
    const MIB: u64 = 1024 * 1024;

    #[test]
    fn the_reserve_is_a_tenth_or_sixty_four_mib() {
        assert_eq!(mem_reserve(1024 * MIB), 1024 * MIB / 10, "10 % of 1 GiB");
        assert_eq!(mem_reserve(512 * MIB), MIN_MEM_RESERVE, "64 MiB wins");
        assert_eq!(
            mem_reserve(640 * MIB),
            MIN_MEM_RESERVE,
            "exactly at the floor"
        );
        assert_eq!(mem_reserve(8 * 1024 * MIB), 8 * 1024 * MIB / 10);
    }

    #[test]
    fn the_derived_cap_is_the_free_memory_over_the_estimate() {
        // A 1 GiB instance with a 200 MiB heap: reserve is 102.4 MiB, 32 MiB
        // per view, so (1024 − 200 − 102.4) / 32 = 22 views.
        let cap = derived_view_cap(1024 * MIB, 200 * MIB, 32 * MIB);
        assert_eq!(cap, 22);
    }

    #[test]
    fn the_derived_cap_is_never_below_one() {
        // Almost all of the memory is gone: there is still one view's worth of
        // hope, because a cap of zero would make blastit refuse every request.
        assert_eq!(derived_view_cap(1024 * MIB, 1024 * MIB, 32 * MIB), 1);
        assert_eq!(derived_view_cap(0, 0, 32 * MIB), 1);
        // A reading above `total` (a cgroup that is being reclaimed) cannot
        // underflow.
        assert_eq!(derived_view_cap(64 * MIB, u64::MAX, 32 * MIB), 1);
        // An estimate of zero would divide by zero; it is treated as one byte,
        // and a width that does not fit a `u32` is clamped.
        assert!(derived_view_cap(1024 * MIB, 0, 0) > 1);
        assert_eq!(derived_view_cap(u64::MAX / 2, 0, 1), u32::MAX);
    }

    #[test]
    fn a_probe_reports_what_it_reads() {
        let probe = MemoryProbe::new(|| Some(42));
        assert_eq!(probe.read(), Some(42));
        assert_eq!(format!("{probe:?}"), "MemoryProbe");
        let unreadable = MemoryProbe::new(|| None);
        assert_eq!(unreadable.read(), None);
        let clone = probe.clone();
        assert_eq!(clone.read(), Some(42));
    }

    #[test]
    fn the_watermark_needs_both_a_probe_and_a_watermark() {
        let mut cfg = SchedulerConfig::default();
        assert!(!cfg.watches_memory());
        cfg.mem_high_watermark_bytes = Some(1);
        assert!(!cfg.watches_memory(), "no probe");
        cfg.memory_usage = Some(MemoryProbe::new(|| Some(0)));
        assert!(cfg.watches_memory());
        cfg.mem_high_watermark_bytes = None;
        assert!(!cfg.watches_memory(), "no watermark");
    }

    #[test]
    fn fair_queuing_is_off_by_default() {
        let cfg = SchedulerConfig::default();
        assert!(
            !cfg.fair_queuing,
            "§2.4 defaults BLASTIT_FAIR_QUEUING to off"
        );
    }

    #[test]
    fn the_fair_defaults_are_the_documented_ones() {
        let fair = FairConfig::default();
        assert_eq!(fair.outer_prefix_v4, 24);
        assert_eq!(fair.outer_prefix_v6, 56);
        assert_eq!(fair.outer_max, 65_536);
        assert_eq!(fair.inner_max_per_outer, 1_024);
        assert_eq!(fair.inner_max, 262_144);
        assert_eq!(fair.burst, Duration::from_millis(20));
        assert_eq!(fair.max_debt, Duration::from_secs(5));
        assert_eq!(fair.debt_ttl, Duration::from_secs(30));
        assert!(fair.inner_include_ip);
        assert_eq!(
            fair.inner_headers,
            [
                "user-agent",
                "accept-language",
                "sec-ch-ua",
                "sec-ch-ua-platform",
                "sec-ch-ua-mobile",
                "authorization"
            ]
        );
        assert_eq!(fair.inner_cookies, ["sessionid", "session"]);
        assert_eq!(
            fair.client_ip,
            ClientIpConfig::default(),
            "the scheduler resolves the client exactly as the front end does"
        );
    }
}
