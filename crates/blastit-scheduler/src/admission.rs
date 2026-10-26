//! Fair admission queuing: which client waits in which queue, and who is
//! served next (ARCHITECTURE.md §2.4).
//!
//! §2.4's admission queue is one bounded queue. This module is what
//! `BLASTIT_FAIR_QUEUING` turns it into: one FIFO per client, served
//! round-robin, so a burst from one client cannot starve the others.
//!
//! Two rules come straight from the reference (granian's HTTP fair queuing):
//!
//! * **The client key comes from the peer, never from the request**, unless an
//!   operator names a header *and* the peer is a trusted proxy. A client that
//!   could choose its key could choose to be a thousand clients — or to be the
//!   one client everybody else queues behind.
//! * **Waiting requests are served round-robin, and a client that cannot be
//!   served right now is skipped, not dropped.** When the global worker budget
//!   or a per-view cap makes clients compete, each one gets a turn in the ring
//!   in order, rather than the queue being drained first-come-first-served.
//!
//! With fair queuing off every request shares [`ClientId::All`], the ring holds
//! exactly one client, and the queue is the first-come-first-served FIFO it
//! always was.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};

use blastit_frame::ReqHead;

use crate::config::{ClientKey, SchedulerConfig, TrustedProxies};

/// Which client a request belongs to.
///
/// Hashable and cheap to compare; it is only ever a queue key, and no part of
/// it (in particular a header value) is ever logged (§8.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ClientId {
    /// Fair queuing is off: every request shares one FIFO.
    All,
    /// A client address, without the connection's port.
    Ip(IpAddr),
    /// A peer that is not an address (a Unix socket, or a peer the front end
    /// reported in a form this does not know), verbatim.
    Other(String),
}

impl ClientId {
    /// The key a `remote_addr` stands for: its address when it has one, the
    /// string itself otherwise.
    fn from_remote_addr(remote_addr: &str) -> Self {
        let text = remote_addr.trim();
        match parse_ip(text) {
            Some(ip) => Self::Ip(normalize_ip(ip)),
            None => Self::Other(text.to_string()),
        }
    }
}

/// The client key of one request (§2.4).
pub(crate) fn client_for(head: &ReqHead, cfg: &SchedulerConfig) -> ClientId {
    if !cfg.fair_queuing {
        return ClientId::All;
    }
    let peer = ClientId::from_remote_addr(&head.remote_addr);
    let ClientKey::Header(name) = &cfg.client_key else {
        return peer;
    };
    // A header is only read from a peer the operator trusts. A peer that is not
    // an address is only trusted when `*` says so: a header is never believed
    // because it is there, only because an operator said whose it is.
    let trusted = match &peer {
        ClientId::Ip(ip) => cfg.trusted_proxies.contains(ip),
        ClientId::All | ClientId::Other(_) => cfg.trusted_proxies.trusts_all(),
    };
    if !trusted {
        return peer;
    }
    header_client(head, name, &cfg.trusted_proxies).unwrap_or(peer)
}

/// The client a trusted client-IP header names, if it names one.
///
/// The header may carry a chain of addresses; the address that is not itself a
/// trusted proxy — scanning from the right, which is where the proxy that
/// forwarded to us appends — is the client. A chain that is empty or does not
/// parse falls back to the peer: a header this cannot read is a header it will
/// not act on.
fn header_client(head: &ReqHead, name: &str, trusted: &TrustedProxies) -> Option<ClientId> {
    let wanted = name.as_bytes();
    let mut chain: Vec<IpAddr> = Vec::new();
    let mut seen = false;
    for (header, value) in &head.headers {
        if !header.eq_ignore_ascii_case(wanted) {
            continue;
        }
        seen = true;
        let text = std::str::from_utf8(value).ok()?;
        for item in text.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            chain.push(parse_ip(item)?);
        }
    }
    if !seen || chain.is_empty() {
        return None;
    }
    Some(ClientId::Ip(normalize_ip(select_client_ip(
        &chain, trusted,
    ))))
}

/// The address in a forwarded chain that belongs to the client.
///
/// With `*` trusted, the leftmost entry is the client (the chain is only
/// believed because the operator said every peer may be). Otherwise the
/// rightmost entry that is not a trusted proxy is the first address a trusted
/// proxy saw, which is the client; when every entry is trusted, the leftmost is
/// all there is.
fn select_client_ip(chain: &[IpAddr], trusted: &TrustedProxies) -> IpAddr {
    if trusted.trusts_all() {
        return chain[0];
    }
    for ip in chain.iter().rev() {
        if !trusted.contains(ip) {
            return *ip;
        }
    }
    chain[0]
}

/// The IP in an address that may carry a port, in any form a front end writes
/// it: `1.2.3.4`, `1.2.3.4:80`, `::1`, `[::1]`, `[::1]:80`.
fn parse_ip(text: &str) -> Option<IpAddr> {
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

/// An IPv4-mapped IPv6 address folded to IPv4, so one client is one key.
fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// One client's waiting requests, in arrival order.
#[derive(Default)]
struct ClientQueue {
    waiting: VecDeque<u32>,
    /// Whether the client is currently in the round-robin ring. There is one
    /// ring entry per client, never one per request.
    ringed: bool,
}

/// The admission queue: one FIFO per client, served round-robin (§2.4).
pub(crate) struct AdmissionQueue {
    clients: HashMap<ClientId, ClientQueue>,
    /// The round-robin ring. The client at the front is the one to consider
    /// next; serving it moves it to the back.
    ring: VecDeque<ClientId>,
    /// Total waiting requests, kept alongside so `len` is O(1).
    total: usize,
    /// Whether requests are spread over per-client queues at all.
    fair: bool,
}

impl AdmissionQueue {
    /// An empty queue, fair or first-come-first-served.
    pub(crate) fn new(fair: bool) -> Self {
        Self {
            clients: HashMap::new(),
            ring: VecDeque::new(),
            total: 0,
            fair,
        }
    }

    /// The key actually used for `client`: with fair queuing off, everything
    /// shares one queue.
    fn bucket<'a>(&self, client: &'a ClientId) -> Cow<'a, ClientId> {
        if self.fair {
            Cow::Borrowed(client)
        } else {
            Cow::Owned(ClientId::All)
        }
    }

    /// Puts a request at the back of its client's queue.
    pub(crate) fn push(&mut self, client: &ClientId, stream_id: u32) {
        let key = self.bucket(client).into_owned();
        let entry = self.clients.entry(key.clone()).or_default();
        entry.waiting.push_back(stream_id);
        if !entry.ringed {
            entry.ringed = true;
            self.ring.push_back(key);
        }
        self.total += 1;
    }

    /// Takes a request out of its client's queue, wherever it is in it.
    pub(crate) fn remove(&mut self, client: &ClientId, stream_id: u32) {
        let key = self.bucket(client);
        let Some(queue) = self.clients.get_mut(key.as_ref()) else {
            return;
        };
        let before = queue.waiting.len();
        queue.waiting.retain(|id| *id != stream_id);
        if queue.waiting.len() == before {
            return;
        }
        self.total = self.total.saturating_sub(1);
        if queue.waiting.is_empty() {
            let key = self.bucket(client).into_owned();
            self.clients.remove(&key);
            self.ring.retain(|client| *client != key);
        }
    }

    /// How many requests `client` has waiting.
    pub(crate) fn client_len(&self, client: &ClientId) -> usize {
        self.clients
            .get(self.bucket(client).as_ref())
            .map(|queue| queue.waiting.len())
            .unwrap_or(0)
    }

    /// Every waiting request, in no particular order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.clients
            .values()
            .flat_map(|queue| queue.waiting.iter().copied())
    }

    /// The client whose turn it is, if any.
    pub(crate) fn front_client(&self) -> Option<ClientId> {
        self.ring.front().cloned()
    }

    /// The request at the front of `client`'s queue, if it has one.
    pub(crate) fn front_stream(&self, client: &ClientId) -> Option<u32> {
        self.clients
            .get(self.bucket(client).as_ref())
            .and_then(|queue| queue.waiting.front().copied())
    }

    /// A serving turn ended: drop the served request and give the client its
    /// next turn at the back of the ring, or take it out of the ring entirely
    /// when it has nothing left to serve.
    pub(crate) fn after_served(&mut self, client: &ClientId, stream_id: u32) {
        let key = self.bucket(client).into_owned();
        let mut emptied = false;
        if let Some(queue) = self.clients.get_mut(&key) {
            let before = queue.waiting.len();
            queue.waiting.retain(|id| *id != stream_id);
            if queue.waiting.len() != before {
                self.total = self.total.saturating_sub(1);
            }
            emptied = queue.waiting.is_empty();
            if emptied {
                queue.ringed = false;
            }
        }
        if emptied {
            self.clients.remove(&key);
            self.ring.retain(|entry| *entry != key);
            return;
        }
        // The front is this client's ring entry; move it to the back.
        if self.ring.front() == Some(&key) {
            self.ring.rotate_left(1);
        }
    }

    /// A blocked client's turn ended without a dispatch: move it to the back so
    /// the next client is considered first, and it comes round again later.
    pub(crate) fn rotate(&mut self, client: &ClientId) {
        let key = self.bucket(client);
        if self.ring.front() == Some(key.as_ref()) {
            self.ring.rotate_left(1);
        }
    }

    /// How many clients have requests waiting.
    pub(crate) fn client_count(&self) -> usize {
        self.ring.len()
    }

    /// How many requests are waiting.
    pub(crate) fn len(&self) -> usize {
        self.total
    }

    /// Whether anything is waiting.
    pub(crate) fn is_empty(&self) -> bool {
        self.total == 0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn head_with(remote_addr: &str, headers: &[(&str, &str)]) -> ReqHead {
        ReqHead {
            method: b"GET".to_vec(),
            target: b"/".to_vec(),
            headers: headers
                .iter()
                .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
                .collect(),
            remote_addr: remote_addr.to_string(),
            ..ReqHead::default()
        }
    }

    #[test]
    fn a_peer_key_drops_the_connection_port() {
        assert_eq!(
            ClientId::from_remote_addr("203.0.113.7:41234"),
            ClientId::Ip(ip("203.0.113.7"))
        );
        assert_eq!(
            ClientId::from_remote_addr("203.0.113.7"),
            ClientId::Ip(ip("203.0.113.7"))
        );
        assert_eq!(
            ClientId::from_remote_addr("[2001:db8::1]:443"),
            ClientId::Ip(ip("2001:db8::1"))
        );
        assert_eq!(
            ClientId::from_remote_addr("[::ffff:10.0.0.1]:99"),
            ClientId::Ip(ip("10.0.0.1")),
            "a mapped address is the same peer"
        );
        assert_eq!(
            ClientId::from_remote_addr("::ffff:10.0.0.1"),
            ClientId::Ip(ip("10.0.0.1"))
        );
        assert_eq!(
            ClientId::from_remote_addr("/run/blastit.sock"),
            ClientId::Other("/run/blastit.sock".to_string())
        );
        assert_eq!(
            ClientId::from_remote_addr(""),
            ClientId::Other(String::new())
        );
    }

    #[test]
    fn fair_queuing_off_makes_everything_one_client() {
        let cfg = SchedulerConfig::default();
        let head = head_with("203.0.113.7:1", &[("X-Forwarded-For", "10.0.0.1")]);
        assert_eq!(client_for(&head, &cfg), ClientId::All);
    }

    #[test]
    fn a_header_is_ignored_from_an_untrusted_peer() {
        let mut cfg = SchedulerConfig {
            fair_queuing: true,
            client_key: ClientKey::Header("x-forwarded-for".to_string()),
            ..SchedulerConfig::default()
        };
        let head = head_with("198.51.100.5:1", &[("X-Forwarded-For", "10.0.0.1")]);
        assert_eq!(
            client_for(&head, &cfg),
            ClientId::Ip(ip("198.51.100.5")),
            "the peer is not trusted, so the header is not read"
        );

        cfg.trusted_proxies = TrustedProxies::parse("203.0.113.0/24").unwrap();
        let untrusted = head_with("198.51.100.5:1", &[("X-Forwarded-For", "10.0.0.1")]);
        assert_eq!(
            client_for(&untrusted, &cfg),
            ClientId::Ip(ip("198.51.100.5"))
        );
        let trusted = head_with("203.0.113.7:1", &[("X-Forwarded-For", "10.0.0.1")]);
        assert_eq!(client_for(&trusted, &cfg), ClientId::Ip(ip("10.0.0.1")));
    }

    #[test]
    fn a_trusted_header_falls_back_to_the_peer_when_it_cannot_be_read() {
        let cfg = SchedulerConfig {
            fair_queuing: true,
            client_key: ClientKey::Header("x-forwarded-for".to_string()),
            trusted_proxies: TrustedProxies::parse("*").unwrap(),
            ..SchedulerConfig::default()
        };
        let peer = ClientId::Ip(ip("203.0.113.7"));
        for headers in [
            vec![],
            vec![("X-Forwarded-For", "")],
            vec![("X-Forwarded-For", "not-an-ip")],
            vec![("X-Forwarded-For", "10.0.0.1, nope")],
            vec![("X-Other", "10.0.0.1")],
        ] {
            let head = head_with("203.0.113.7:1", &headers);
            assert_eq!(client_for(&head, &cfg), peer, "{headers:?}");
        }
    }

    #[test]
    fn a_forwarded_chain_picks_the_first_untrusted_address() {
        let trusted = TrustedProxies::parse("10.0.0.0/8").unwrap();
        let chain = vec![ip("203.0.113.9"), ip("10.0.0.5"), ip("10.0.0.1")];
        assert_eq!(select_client_ip(&chain, &trusted), ip("203.0.113.9"));
        // Every entry trusted: the leftmost is all the chain says.
        let all_trusted = TrustedProxies::parse("0.0.0.0/0").unwrap();
        assert_eq!(select_client_ip(&chain, &all_trusted), ip("203.0.113.9"));
        // `*` believes the whole chain, so the leftmost entry wins.
        let star = TrustedProxies::parse("*").unwrap();
        assert_eq!(select_client_ip(&chain, &star), ip("203.0.113.9"));
        let middle = vec![ip("10.0.0.2"), ip("203.0.113.9"), ip("10.0.0.1")];
        assert_eq!(select_client_ip(&middle, &trusted), ip("203.0.113.9"));
    }

    #[test]
    fn the_ring_serves_clients_round_robin_and_each_client_fifo() {
        let mut queue = AdmissionQueue::new(true);
        let a = ClientId::Ip(ip("10.0.0.1"));
        let b = ClientId::Ip(ip("10.0.0.2"));
        for id in [1, 2, 3] {
            queue.push(&a, id);
        }
        queue.push(&b, 4);
        assert_eq!(queue.len(), 4);
        assert_eq!(queue.client_len(&a), 3);
        assert_eq!(queue.client_len(&b), 1);
        assert_eq!(queue.client_count(), 2);

        // First turn: A's head, and A goes to the back of the ring.
        assert_eq!(queue.front_client(), Some(a.clone()));
        assert_eq!(queue.front_stream(&a), Some(1));
        queue.after_served(&a, 1);
        // Second turn: B, which is now at the front.
        assert_eq!(queue.front_client(), Some(b.clone()));
        assert_eq!(queue.front_stream(&b), Some(4));
        queue.after_served(&b, 4);
        assert_eq!(queue.client_count(), 1, "B has nothing left");
        // Then A's remaining requests in order.
        assert_eq!(queue.front_stream(&a), Some(2));
        queue.after_served(&a, 2);
        assert_eq!(queue.front_stream(&a), Some(3));
        queue.after_served(&a, 3);
        assert!(queue.is_empty());
        assert_eq!(queue.front_client(), None);
    }

    #[test]
    fn a_blocked_client_does_not_hold_up_the_others() {
        let mut queue = AdmissionQueue::new(true);
        let a = ClientId::Ip(ip("10.0.0.1"));
        let b = ClientId::Ip(ip("10.0.0.2"));
        queue.push(&a, 1);
        queue.push(&b, 2);
        // A cannot be served: it rotates to the back and B is next.
        queue.rotate(&a);
        assert_eq!(queue.front_client(), Some(b.clone()));
        assert_eq!(queue.front_stream(&b), Some(2));
        queue.after_served(&b, 2);
        assert_eq!(queue.front_client(), Some(a.clone()));
    }

    #[test]
    fn with_fair_queuing_off_the_queue_is_one_fifo() {
        let mut queue = AdmissionQueue::new(false);
        let a = ClientId::Ip(ip("10.0.0.1"));
        let b = ClientId::Ip(ip("10.0.0.2"));
        for (client, id) in [(&a, 1), (&b, 2), (&a, 3)] {
            queue.push(client, id);
        }
        assert_eq!(queue.client_count(), 1);
        assert_eq!(queue.client_len(&a), 3, "one bucket holds them all");
        assert_eq!(queue.iter().collect::<Vec<_>>(), vec![1, 2, 3]);
        queue.after_served(&a, 1);
        assert_eq!(queue.front_stream(&b), Some(2));
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn removing_keeps_the_ring_and_the_count_honest() {
        let mut queue = AdmissionQueue::new(true);
        let a = ClientId::Ip(ip("10.0.0.1"));
        let b = ClientId::Ip(ip("10.0.0.2"));
        queue.push(&a, 1);
        queue.push(&b, 2);
        queue.remove(&b, 2);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.client_count(), 1);
        assert_eq!(queue.front_client(), Some(a.clone()));
        // Removing something that is not there changes nothing.
        queue.remove(&b, 99);
        queue.remove(&a, 99);
        assert_eq!(queue.len(), 1);
        queue.after_served(&a, 1);
        assert!(queue.is_empty());
    }
}
