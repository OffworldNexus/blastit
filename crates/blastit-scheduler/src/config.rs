//! Scheduler configuration (ARCHITECTURE.md §2.4, §8.2).
//!
//! Every field mirrors one environment variable, but the scheduler never reads
//! the environment: init parses `BLASTIT_*` in stage 1 and hands the values
//! over (ARCHITECTURE.md §4, §8.2). The defaults here are the documented
//! defaults of those variables.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use blastit_frame::Limits;

/// How long the scheduler waits before asking init for a view it just lost
/// (§2.4: "don't spin: back off before respawning the same route").
pub const SPAWN_BACKOFF: Duration = Duration::from_secs(1);

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

/// How the scheduler identifies the client behind a request (§2.4).
///
/// The key decides which per-client sub-queue a waiting request lands in when
/// `BLASTIT_FAIR_QUEUING` is on. It is derived from the request head, and a
/// header is only ever read when the peer is a trusted proxy: a client cannot
/// choose its own key unless the operator says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientKey {
    /// The peer address the front end saw (`ReqHead::remote_addr`, without its
    /// port). The default, and the only safe choice with no proxy in front of
    /// blastit.
    Peer,
    /// A request header naming the client, honoured only from a peer in
    /// [`TrustedProxies`]. This is what a load balancer that terminates the
    /// connection (App Platform's edge, §10) needs: there the peer is the
    /// balancer, and every request would otherwise share one key.
    Header(String),
}

impl ClientKey {
    /// Parses the `BLASTIT_CLIENT_KEY` spelling, `peer` or `header:<name>`.
    ///
    /// # Errors
    ///
    /// The reason, without the value, when the setting is neither.
    pub fn parse(value: &str) -> Result<Self, String> {
        let text = value.trim();
        if text.eq_ignore_ascii_case("peer") {
            return Ok(Self::Peer);
        }
        if let Some(name) = text.strip_prefix("header:") {
            let name = name.trim();
            if name.is_empty() {
                return Err("expected a header name after `header:`".to_string());
            }
            if !name.bytes().all(is_header_name_byte) {
                return Err("is not a valid header name".to_string());
            }
            return Ok(Self::Header(name.to_ascii_lowercase()));
        }
        Err("expected `peer` or `header:<name>`".to_string())
    }

    /// The `BLASTIT_CLIENT_KEY` spelling of this key.
    pub fn as_str(&self) -> String {
        match self {
            Self::Peer => "peer".to_string(),
            Self::Header(name) => format!("header:{name}"),
        }
    }

    /// The header this key reads, if any.
    pub fn header(&self) -> Option<&str> {
        match self {
            Self::Peer => None,
            Self::Header(name) => Some(name),
        }
    }
}

/// Whether `byte` may appear in an HTTP header name (RFC 9110's `token`).
fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// The peers whose client-IP header may be believed (`BLASTIT_TRUSTED_PROXIES`,
/// §2.4).
///
/// A header is only read when the peer that opened the connection is in this
/// set, so a client cannot forge its key. `*` trusts every peer, which is what
/// a platform whose edge address is not fixed (App Platform, §10) needs, at the
/// cost of believing the header from anyone who can reach the socket.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustedProxies {
    /// The text this was parsed from, kept verbatim for the stage 1 document.
    source: String,
    /// `*`: every peer is trusted.
    always: bool,
    /// Single addresses.
    hosts: Vec<std::net::IpAddr>,
    /// CIDR networks and their prefix length.
    nets: Vec<(std::net::IpAddr, u8)>,
}

impl TrustedProxies {
    /// The empty list: no peer is trusted, so a header is never read.
    pub fn none() -> Self {
        Self::default()
    }

    /// Parses a comma- or space-separated list of addresses and CIDR networks,
    /// or `*`.
    ///
    /// Addresses are compared after IPv4-mapped IPv6 addresses are folded to
    /// IPv4, so `::ffff:10.0.0.1` and `10.0.0.1` are the same peer.
    ///
    /// # Errors
    ///
    /// The reason, without the value, for an entry that is neither an address,
    /// a network, nor `*`.
    pub fn parse(value: &str) -> Result<Self, String> {
        let mut parsed = Self {
            source: value.to_string(),
            ..Self::default()
        };
        for entry in value.split([',', ' ', '\t', '\n']) {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if entry == "*" {
                parsed.always = true;
                continue;
            }
            match entry.split_once('/') {
                Some((host, prefix)) => {
                    let ip = parse_ip(host)
                        .ok_or_else(|| "an entry is not an IP address or network".to_string())?;
                    let prefix: u8 = prefix
                        .trim()
                        .parse()
                        .map_err(|_| "an entry's prefix length is not a number".to_string())?;
                    let limit = if ip.is_ipv4() { 32 } else { 128 };
                    if prefix > limit {
                        return Err(format!("an entry's prefix length is above {limit}"));
                    }
                    parsed.nets.push((ip, prefix));
                }
                None => {
                    let ip = parse_ip(entry)
                        .ok_or_else(|| "an entry is not an IP address or network".to_string())?;
                    parsed.hosts.push(ip);
                }
            }
        }
        Ok(parsed)
    }

    /// The text this was parsed from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether every peer is trusted, as `*` says.
    pub fn trusts_all(&self) -> bool {
        self.always
    }

    /// Whether `ip` is a trusted peer.
    pub fn contains(&self, ip: &std::net::IpAddr) -> bool {
        if self.always {
            return true;
        }
        let ip = normalize_ip(*ip);
        if self.hosts.iter().any(|host| normalize_ip(*host) == ip) {
            return true;
        }
        self.nets
            .iter()
            .any(|(net, prefix)| same_network(normalize_ip(*net), ip, *prefix))
    }
}

/// The IP in an address that may carry a port, in any form a front end writes
/// it: `1.2.3.4`, `1.2.3.4:80`, `::1`, `[::1]`, `[::1]:80`.
fn parse_ip(text: &str) -> Option<std::net::IpAddr> {
    use std::net::{IpAddr, SocketAddr};
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(ip) = text.parse::<IpAddr>() {
        return Some(ip);
    }
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Some(addr.ip());
    }
    let bracketed = text.strip_prefix('[')?.strip_suffix(']')?;
    bracketed.parse::<IpAddr>().ok()
}

/// An IPv4-mapped IPv6 address folded to IPv4, which is the same peer.
fn normalize_ip(ip: std::net::IpAddr) -> std::net::IpAddr {
    match ip {
        std::net::IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map_or(std::net::IpAddr::V6(v6), std::net::IpAddr::V4),
        std::net::IpAddr::V4(_) => ip,
    }
}

/// Whether `ip` is inside `net/prefix`, in the same family.
fn same_network(net: std::net::IpAddr, ip: std::net::IpAddr, prefix: u8) -> bool {
    match (net, ip) {
        (std::net::IpAddr::V4(net), std::net::IpAddr::V4(ip)) => {
            let prefix = prefix.min(32);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            u32::from(net) & mask == u32::from(ip) & mask
        }
        (std::net::IpAddr::V6(net), std::net::IpAddr::V6(ip)) => {
            let prefix = prefix.min(128);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            u128::from(net) & mask == u128::from(ip) & mask
        }
        _ => false,
    }
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
    /// `BLASTIT_FAIR_QUEUING`: serve the admission queue round-robin across
    /// clients instead of first-come-first-served (§2.4). Off by default, in
    /// which case every waiting request shares one FIFO and a single client
    /// sees exactly the queue it always did.
    pub fair_queuing: bool,
    /// `BLASTIT_CLIENT_KEY`: what identifies a client for fair queuing.
    pub client_key: ClientKey,
    /// `BLASTIT_TRUSTED_PROXIES`: the peers whose client-IP header may be
    /// believed. Empty means none, and an empty list can never be forged into
    /// one that is not.
    pub trusted_proxies: TrustedProxies,
    /// `BLASTIT_MAX_QUEUED_PER_CLIENT`: requests one client may have waiting in
    /// the admission queue before the next one is refused with a `503`
    /// `client_queue_full`. Zero disables the cap. Only ever consulted when
    /// `fair_queuing` is on.
    pub max_queued_per_client: usize,
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
            client_key: ClientKey::Peer,
            trusted_proxies: TrustedProxies::none(),
            max_queued_per_client: 0,
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

    /// The `Retry-After` value, in seconds, the scheduler puts on its own `503`
    /// answers (§8: "queue full, or queue wait exceeds `BLASTIT_QUEUE_TIMEOUT`
    /// | `503` with `Retry-After`").
    ///
    /// It is the admission queue's own timeout, rounded up and never below one
    /// second: a client that came back sooner than that would only be refused
    /// again.
    pub fn retry_after_secs(&self) -> u64 {
        let seconds = self.queue_timeout.as_secs_f64();
        if !seconds.is_finite() || seconds <= 0.0 {
            return 1;
        }
        (seconds.ceil() as u64).max(1)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::field_reassign_with_default
)]
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
    fn the_client_key_takes_peer_or_a_header() {
        assert_eq!(ClientKey::parse("peer").unwrap(), ClientKey::Peer);
        assert_eq!(ClientKey::parse(" PEER ").unwrap(), ClientKey::Peer);
        assert_eq!(
            ClientKey::parse("header:X-Forwarded-For").unwrap(),
            ClientKey::Header("x-forwarded-for".to_string())
        );
        assert_eq!(
            ClientKey::parse("header:do-connecting-ip")
                .unwrap()
                .as_str(),
            "header:do-connecting-ip"
        );
        assert_eq!(ClientKey::Peer.as_str(), "peer");
        assert_eq!(ClientKey::Peer.header(), None);
        assert_eq!(
            ClientKey::Header("x-real-ip".to_string()).header(),
            Some("x-real-ip")
        );
        for bad in ["", "ip", "forwarded", "header:", "header:  ", "header:a b"] {
            assert!(ClientKey::parse(bad).is_err(), "`{bad}` must be refused");
        }
    }

    #[test]
    fn trusted_proxies_match_hosts_and_networks() {
        use std::net::IpAddr;
        let none = TrustedProxies::none();
        assert!(!none.trusts_all());
        assert!(!none.contains(&"10.0.0.1".parse::<IpAddr>().unwrap()));

        let list = TrustedProxies::parse("127.0.0.1, ::1, 10.0.0.0/8, 2001:db8::/32").unwrap();
        assert_eq!(list.source(), "127.0.0.1, ::1, 10.0.0.0/8, 2001:db8::/32");
        for inside in ["127.0.0.1", "::1", "10.1.2.3", "2001:db8::1"] {
            assert!(list.contains(&inside.parse().unwrap()), "{inside}");
        }
        for outside in ["127.0.0.2", "10.1.2.3.4", "11.0.0.1", "2001:db9::1"] {
            if let Ok(ip) = outside.parse::<IpAddr>() {
                assert!(!list.contains(&ip), "{outside}");
            }
        }
        // A mapped address is the same peer as its IPv4 form.
        assert!(list.contains(&"::ffff:10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn trusted_proxies_accept_the_star_and_refuse_nonsense() {
        let all = TrustedProxies::parse("*").unwrap();
        assert!(all.trusts_all());
        assert!(all.contains(&"203.0.113.9".parse().unwrap()));

        assert!(TrustedProxies::parse("10.0.0.0/33").is_err());
        assert!(TrustedProxies::parse("2001:db8::/129").is_err());
        assert!(TrustedProxies::parse("proxy.example.com").is_err());
        assert!(TrustedProxies::parse("10.0.0.0/x").is_err());
        assert!(TrustedProxies::parse("").unwrap().source().is_empty());
        // The empty list trusts nobody.
        assert!(!TrustedProxies::parse("").unwrap().trusts_all());
    }

    #[test]
    fn the_retry_after_is_the_queue_timeout_rounded_up() {
        let mut cfg = SchedulerConfig::default();
        cfg.queue_timeout = Duration::from_secs(10);
        assert_eq!(cfg.retry_after_secs(), 10);
        cfg.queue_timeout = Duration::from_millis(1500);
        assert_eq!(cfg.retry_after_secs(), 2);
        // Never below a second, even with a timeout of zero.
        cfg.queue_timeout = Duration::ZERO;
        assert_eq!(cfg.retry_after_secs(), 1);
    }
}
