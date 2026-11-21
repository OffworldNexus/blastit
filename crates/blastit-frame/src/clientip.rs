//! One client-IP resolver, for fair queuing and for `REMOTE_ADDR` (D5).
//!
//! Both the front end (which fills in `REQ_HEAD.remote_addr`, the value the app
//! sees as `REMOTE_ADDR`) and the scheduler (which keys the fair queue's flows
//! on the client address) need the same answer to "who is this?", and they must
//! agree: a request that is scheduled as one client and logged as another is a
//! bug either way. So the rule lives here, once.
//!
//! # The rule
//!
//! * The **raw socket peer** — [`crate::ReqHead::peer_addr`], never a header —
//!   decides whether anything in the request may be believed.
//! * A peer outside [`ClientIpConfig::trusted`] is the client. Its
//!   [`ClientIpConfig::header`] is not read at all, so a client that can reach
//!   the front end directly cannot forge an address.
//! * A trusted peer's header values are read as one chain, in the order they
//!   arrived, and walked from the right: the first entry that is itself
//!   untrusted is the client. The rightmost entry is the one the closest
//!   proxy appended, so the walk skips our own infrastructure and stops at the
//!   first address it did not add.
//! * If *every* entry is trusted, the leftmost is taken: nothing in the chain
//!   is more plausible than anything else, and that is what the reference
//!   implementation does.
//! * `*` in [`ClientIpConfig::trusted`] means every peer is trusted. There the
//!   **rightmost** entry is taken, and that is a deliberate difference from the
//!   reference implementation (see [`ClientIpConfig::trusted`]).
//! * One unparsable entry makes the whole header ignored: the peer is used
//!   instead. Nothing falls through to another header, because "read every
//!   plausible header until one parses" is a forgery amplifier, not a
//!   fallback. An *empty* entry (`,,`) is skipped the way a list's noise is.
//!   An entry is a bare address or a bracketed one — a chain entry is an
//!   address, so `1.2.3.4:5678` is unparsable, exactly as the reference
//!   implementation's `forwarded_chain` treats it, and the header is ignored.
//! * IPv4-mapped IPv6 addresses (`::ffff:203.0.113.7`) are normalised to IPv4,
//!   so a client that arrives as a mapped address is not a second client.
//!
//! # What the app sees
//!
//! The resolved address is `REMOTE_ADDR`, and CGI and PEP 3333 make that the
//! *address*: a bare IP, normalised, with no port, brackets or zone
//! ([`ClientIp::remote_addr`]). The socket address the request arrived on keeps
//! its `ip:port` shape in [`crate::ReqHead::peer_addr`], which is where the port
//! the app sees as `REMOTE_PORT` comes from.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A list of peers whose client-IP header may be believed.
///
/// Entries are IP addresses, CIDR networks (`10.0.0.0/8`) or the single entry
/// `*`, which trusts every peer. Anything else — including `127.0.0.1:8080`,
/// where a port and a network are confused — is an error, because a trusted
/// proxy list that quietly drops what it cannot read is a list that trusts
/// nothing when the operator meant it to trust something.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustedProxies {
    /// `*`: every peer is trusted.
    always: bool,
    /// Exact addresses.
    hosts: Vec<IpAddr>,
    /// Networks, as `(network, prefix length)`.
    nets: Vec<(IpAddr, u8)>,
}

impl TrustedProxies {
    /// An empty list: no peer's header is believed.
    pub fn none() -> Self {
        Self::default()
    }

    /// `*`: every peer's header is believed.
    pub fn any() -> Self {
        Self {
            always: true,
            hosts: Vec::new(),
            nets: Vec::new(),
        }
    }

    /// Read a list of IPs, CIDRs and `*`.
    ///
    /// # Errors
    ///
    /// [`ClientIpError::Proxy`] for an entry that is not an IP, a CIDR or `*`.
    pub fn parse<S: AsRef<str>>(values: &[S]) -> Result<Self, ClientIpError> {
        let mut trusted = Self::default();
        for value in values {
            let value = value.as_ref().trim();
            if value.is_empty() {
                // An empty element of a list is noise, not a network.
                continue;
            }
            if value == "*" {
                return Ok(Self::any());
            }
            match value.split_once('/') {
                Some((host, prefix)) => {
                    let host = host.trim();
                    let ip = host.parse::<IpAddr>().map_err(|_| ClientIpError::Proxy {
                        value: value.to_string(),
                        reason: "the network is not an IP address".to_string(),
                    })?;
                    let prefix = prefix
                        .trim()
                        .parse::<u8>()
                        .map_err(|_| ClientIpError::Proxy {
                            value: value.to_string(),
                            reason: "the prefix length is not a number".to_string(),
                        })?;
                    if usize::from(prefix) > bits_of(ip) {
                        return Err(ClientIpError::Proxy {
                            value: value.to_string(),
                            reason: format!("a /{prefix} is longer than an address"),
                        });
                    }
                    trusted.nets.push((normalize_ip(ip), prefix));
                }
                None => {
                    let ip = value.parse::<IpAddr>().map_err(|_| ClientIpError::Proxy {
                        value: value.to_string(),
                        reason: if value.contains(':') {
                            "expected an IP address or a CIDR, not an address and a port"
                                .to_string()
                        } else {
                            "not an IP address or a CIDR".to_string()
                        },
                    })?;
                    trusted.hosts.push(normalize_ip(ip));
                }
            }
        }
        Ok(trusted)
    }

    /// Whether every peer is trusted (`*`).
    pub fn is_any(&self) -> bool {
        self.always
    }

    /// Whether `ip` may say who the client is.
    pub fn contains(&self, ip: &IpAddr) -> bool {
        let ip = normalize_ip(*ip);
        if self.always {
            return true;
        }
        if self.hosts.contains(&ip) {
            return true;
        }
        self.nets
            .iter()
            .any(|(net, prefix)| in_network(&ip, net, *prefix))
    }
}

/// Which address a [`ClientIp`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClientIpSource {
    /// The socket peer: nothing else was believable.
    Peer,
    /// A trusted peer's client-IP header named it.
    Header,
}

/// Who made a request, and on what grounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp {
    ip: IpAddr,
    source: ClientIpSource,
}

impl ClientIp {
    /// The address, normalised: IPv4-mapped IPv6 is IPv4.
    pub fn ip(self) -> IpAddr {
        self.ip
    }

    /// Where the address came from.
    pub fn source(self) -> ClientIpSource {
        self.source
    }

    /// Whether a trusted client-IP header named it.
    pub fn is_forwarded(self) -> bool {
        self.source == ClientIpSource::Header
    }

    /// What the app sees in `REMOTE_ADDR`: the address, and nothing else.
    ///
    /// A bare IP — normalised, so an IPv4-mapped IPv6 address is the IPv4 one,
    /// and without a port, brackets or zone. CGI and PEP 3333 make
    /// `REMOTE_ADDR` the *address*; the port belongs in `REMOTE_PORT`, and the
    /// connection's is in [`crate::ReqHead::peer_addr`].
    pub fn remote_addr(self) -> String {
        self.ip.to_string()
    }
}

/// How to read the client address out of a request (D5).
///
/// The [`Default`] is `BLASTIT_CLIENT_IP_HEADER=x-forwarded-for` and
/// `BLASTIT_TRUSTED_PROXIES=127.0.0.1,::1`: the header is believed only when
/// the request comes from the same machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIpConfig {
    /// `BLASTIT_CLIENT_IP_HEADER`: the header to read the chain from, matched
    /// case-insensitively.
    pub header: String,
    /// `BLASTIT_TRUSTED_PROXIES`: the peers whose header may be believed.
    pub trusted: TrustedProxies,
}

impl Default for ClientIpConfig {
    fn default() -> Self {
        // §8.2's defaults, spelled out rather than parsed: a default that can
        // fail to build is a default that can panic on startup.
        Self {
            header: "x-forwarded-for".to_string(),
            trusted: TrustedProxies {
                always: false,
                hosts: vec![
                    IpAddr::V4(Ipv4Addr::LOCALHOST),
                    IpAddr::V6(Ipv6Addr::LOCALHOST),
                ],
                nets: Vec::new(),
            },
        }
    }
}

impl ClientIpConfig {
    /// The documented configuration, checked.
    ///
    /// # Errors
    ///
    /// [`ClientIpError::Header`] when `header` is not a header name, and
    /// whatever [`TrustedProxies::parse`] reports.
    pub fn new<S: AsRef<str>>(header: &str, trusted: &[S]) -> Result<Self, ClientIpError> {
        let header = header.trim();
        if !is_header_name(header) {
            return Err(ClientIpError::Header {
                value: header.to_string(),
            });
        }
        Ok(Self {
            header: header.to_ascii_lowercase(),
            trusted: TrustedProxies::parse(trusted)?,
        })
    }

    /// Resolve the client address of one request.
    ///
    /// `peer` is the raw socket peer ([`crate::ReqHead::peer_addr`]) and
    /// `headers` the request's headers as they arrived. `None` means the peer
    /// could not be read as an address at all, which is the only case with
    /// nothing to key a flow on; callers fall back to the peer string for
    /// logging.
    pub fn resolve(&self, peer: &str, headers: &[(Vec<u8>, Vec<u8>)]) -> Option<ClientIp> {
        self.resolve_iter(
            peer,
            headers
                .iter()
                .map(|(name, value)| (name.as_slice(), value.as_slice())),
        )
    }

    /// The same resolution over headers that have not been decoded.
    ///
    /// The scheduler reads a request head straight out of its `REQ_HEAD`
    /// payload instead of decoding it into owned `Vec`s, so the headers arrive
    /// as an iterator of borrowed slices rather than a slice of pairs. Both this
    /// and [`ClientIpConfig::resolve`] funnel into `chain_iter`, so one request
    /// cannot resolve to two
    /// different addresses depending on which path read it (D5).
    pub fn resolve_iter<'h, I>(&self, peer: &str, headers: I) -> Option<ClientIp>
    where
        I: Iterator<Item = (&'h [u8], &'h [u8])>,
    {
        let peer_ip = parse_addr(peer)?;
        if !self.trusted.contains(&peer_ip) {
            return Some(ClientIp {
                ip: peer_ip,
                source: ClientIpSource::Peer,
            });
        }
        // One unparsable entry: believe nothing in this request's header.
        let chain = self.chain_iter(headers).unwrap_or_default();
        match select_client_ip(&chain, &self.trusted) {
            Some(ip) => Some(ClientIp {
                ip,
                source: ClientIpSource::Header,
            }),
            // No header, an empty one, or one that named nothing: the peer.
            None => Some(ClientIp {
                ip: peer_ip,
                source: ClientIpSource::Peer,
            }),
        }
    }

    /// The candidate addresses in this request's client-IP header, in the order
    /// they arrived.
    ///
    /// `None` — "do not believe any of this" — when any entry is not a chain
    /// entry (see [`parse_chain_entry`]).
    fn chain_iter<'h, I>(&self, headers: I) -> Option<Vec<IpAddr>>
    where
        I: Iterator<Item = (&'h [u8], &'h [u8])>,
    {
        let mut chain = Vec::new();
        for value in values_of(headers, &self.header) {
            let Ok(value) = std::str::from_utf8(value) else {
                return None;
            };
            for item in value.split(',') {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                chain.push(parse_chain_entry(item)?);
            }
        }
        Some(chain)
    }
}

/// The values of every instance of `name`, matched case-insensitively, in
/// arrival order.
///
/// The header pairs are taken as an iterator of borrowed slices so that a
/// decoded head (`&[(Vec<u8>, Vec<u8>)]`, mapped down to slices) and a head
/// still in its `REQ_HEAD` payload (`ReqHeadView::headers`) share this one
/// reader: the client-IP chain and the fair queue's inner key must see the same
/// sequence either way, or the scheduler would schedule one request as two
/// different clients depending on whether the head was decoded (D5).
pub fn values_of<'h, 'n, I>(headers: I, name: &'n str) -> impl Iterator<Item = &'h [u8]>
where
    I: Iterator<Item = (&'h [u8], &'h [u8])>,
{
    let name = name.as_bytes();
    headers.filter_map(move |(field, value)| {
        (field.len() == name.len() && field.eq_ignore_ascii_case(name)).then_some(value)
    })
}

/// What a `BLASTIT_TRUSTED_PROXIES` or `BLASTIT_CLIENT_IP_HEADER` value could
/// not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientIpError {
    /// An entry of the trusted proxy list.
    #[error("`{value}` is not a trusted proxy: {reason}")]
    Proxy {
        /// The entry as it was written.
        value: String,
        /// What is wrong with it.
        reason: String,
    },
    /// A client-IP header name.
    #[error("`{value}` is not a header name")]
    Header {
        /// The value as it was written.
        value: String,
    },
}

/// `*` is trusted; a non-`*` peer is the client. Otherwise walk the chain from
/// the right, past the infrastructure, and take the first entry that is not
/// trusted.
///
/// The reference implementation takes `chain[0]` when every peer is trusted
/// (`*`). That is forgeable wherever the edge *appends* to a header the client
/// already sent — the leftmost entry is then the client's own invention — so
/// this takes the rightmost instead: whatever the closest hop added is the one
/// thing that hop cannot be lying about.
fn select_client_ip(chain: &[IpAddr], trusted: &TrustedProxies) -> Option<IpAddr> {
    if trusted.is_any() {
        return chain.last().copied();
    }
    for ip in chain.iter().rev() {
        if !trusted.contains(ip) {
            return Some(*ip);
        }
    }
    chain.first().copied()
}

/// An IPv6 literal in a chain may be bracketed the way it is in a URL.
fn strip_brackets(item: &str) -> &str {
    item.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(item)
}

/// One entry of a client-IP chain: a bare address, or an IPv6 literal in
/// brackets, and nothing else.
///
/// An entry that is anything else — `1.2.3.4:5678`, `example.com`, `unknown`, a
/// `Forwarded`-style `for=1.2.3.4` — is not a chain entry, and the whole header
/// is then ignored (see the module documentation): reading half of a value we
/// cannot read is how a forgery becomes an address. The reference
/// implementation's `forwarded_chain` parses its entries as addresses for the
/// same reason.
///
/// The *peer*, by contrast, arrives as an `ip:port` socket address and is read
/// with [`parse_addr`]: the two are different things and are spelled
/// differently on purpose.
fn parse_chain_entry(item: &str) -> Option<IpAddr> {
    strip_brackets(item)
        .parse::<IpAddr>()
        .ok()
        .map(normalize_ip)
}

/// An `ip`, an `ip:port`, or a bracketed IPv6 with or without a port.
///
/// A port that is not a port (`1.2.3.4:70000`, `[::1]:x`) makes the whole thing
/// unreadable: reading the address out of something we cannot read is how a
/// value nobody can vouch for becomes a trusted one, and it is the same reason
/// an unparsable chain entry ignores the whole header.
///
/// An IPv6 scope (`fe80::1%eth0`) is dropped, with the brackets: a zone names an
/// interface *of the machine that has it*, not part of the address, and
/// `REMOTE_ADDR` is the address alone.
pub fn parse_addr(text: &str) -> Option<IpAddr> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (host, port) = split_port(text);
    if let Some(port) = port
        && port.parse::<u16>().is_err()
    {
        return None;
    }
    strip_zone(host).parse::<IpAddr>().ok().map(normalize_ip)
}

/// The port of an `ip:port` string, when it has one.
///
/// The same spelling [`parse_addr`] reads — a bare `ip:port`, or a bracketed
/// IPv6 literal with one (`[::1]:8080`) — because the two are always read
/// together: this is the port CGI puts in `REMOTE_PORT` while the address goes
/// in `REMOTE_ADDR`. `None` when the text carries no port, when the port is `0`
/// — a socket that arrived from port zero is not a connection anyone made, and
/// `REMOTE_PORT=0` would be a number an app would take for a port — and when the
/// text is no address at all: a port is the port *of* an address, and
/// `localhost:80` has neither.
pub fn parse_port(text: &str) -> Option<u16> {
    let text = text.trim();
    parse_addr(text)?;
    split_port(text).1?.parse().ok().filter(|port| *port != 0)
}

/// A socket peer as `REMOTE_ADDR` spells one: its address, and nothing else.
///
/// `ip:port` becomes the bare IP, and a peer that is not an address at all (a
/// socket with no name) is returned exactly as it arrived: a front end that
/// cannot read its peer has no address to report, and an empty value would be a
/// lie about a connection that is there.
pub fn peer_address(peer: &str) -> String {
    parse_addr(peer).map_or_else(|| peer.to_string(), |ip| ip.to_string())
}

/// Splits an address string into its host and its port, if it has one.
///
/// An IPv6 literal is bracketed when a port follows it, exactly as a URL writes
/// one, so the brackets and what is inside them are the host; anything after
/// `]:` is the port. A bare address may hold several colons (`2001:db8::1`) and
/// no port, so the text is only read as `host:port` when what follows the colon
/// is a number.
fn split_port(text: &str) -> (&str, Option<&str>) {
    if let Some(rest) = text.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':')),
            // An unterminated bracket is not a host.
            None => ("", None),
        };
    }
    match text.split_once(':') {
        Some((host, port)) if port.parse::<u16>().is_ok() => (host, Some(port)),
        _ => (text, None),
    }
}

/// An address without its IPv6 zone (`fe80::1%eth0` → `fe80::1`).
fn strip_zone(text: &str) -> &str {
    match text.split_once('%') {
        Some((host, _zone)) => host,
        None => text,
    }
}

/// IPv4-mapped IPv6 becomes IPv4, so one client is one client.
pub fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// How many bits an address of this family has.
const fn bits_of(ip: IpAddr) -> usize {
    match ip {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

/// Whether `ip` is inside `net/prefix`. Addresses of different families are
/// never in one network.
fn in_network(ip: &IpAddr, net: &IpAddr, prefix: u8) -> bool {
    match (ip, net) {
        (IpAddr::V4(ip), IpAddr::V4(net)) => masked_v4(*ip, prefix) == masked_v4(*net, prefix),
        (IpAddr::V6(ip), IpAddr::V6(net)) => masked_v6(*ip, prefix) == masked_v6(*net, prefix),
        _ => false,
    }
}

fn masked_v4(ip: Ipv4Addr, prefix: u8) -> u32 {
    let prefix = prefix.min(32);
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    u32::from(ip) & mask
}

fn masked_v6(ip: Ipv6Addr, prefix: u8) -> u128 {
    let prefix = prefix.min(128);
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    u128::from(ip) & mask
}

/// Whether `name` can be a header field name on the wire (§3.1: an HTTP token).
///
/// The same check serves a cookie name, which is a token too (RFC 6265): the
/// fair queue's `BLASTIT_FQ_INNER_HEADERS` and `BLASTIT_FQ_INNER_COOKIES` are
/// lists of these, so init can reject a name that could never match a request
/// instead of hashing a name nothing will ever send.
pub fn is_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
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
        })
}
