//! The dev-only loaded-file report: one bounded, versioned datagram codec.
//!
//! A view worker (and the `preload` phase, by inheritance) tells the dev
//! supervisor which files it actually loaded. The wire format is one
//! `AF_UNIX SOCK_DGRAM` datagram per batch, little-endian
//! (`docs/design/dev-fanotify-reload.md` §4.4, `ADR-0014`):
//!
//! ```text
//! batch := u8 version (=1)
//!          u8 flags         bit0 boot, bit1 resync, bits 2..7 zero
//!          u16 source       0xffff = the preload phase, else a route index
//!          u32 seq          monotonic per source
//!          u16 count        0..=64 paths in this datagram
//!          count * { u16 len, len * u8 }   # len excludes any NUL; no NUL is sent
//! ```
//!
//! # What each field means, and what it does not
//!
//! * `source` is the reporting worker's identity, for logging and for the
//!   per-source `seq` space. It is **never** a route to restart: the consumer
//!   unions every source's set and reloads every view together (§4.4). A view
//!   can only speak as itself — it holds only its own write end — so the field
//!   is a self-check, not an attribution mechanism.
//! * `boot` (always with `seq = 0`, and never with `seq != 0`) replaces that
//!   source's whole set: it is what a freshly forked view sends first, so the
//!   module closure imported before the fork — the `preload` phase — is
//!   reported by inheritance rather than by a second socket in the parent.
//!   A boot set larger than one datagram is the first datagram **only**; every
//!   later chunk of the same send is a plain delta whose `seq` continues the
//!   stream, so a consumer that applies the design's "`boot` replaces the set"
//!   rule keeps every path instead of the last chunk alone.
//! * `resync` means "treat every path this source ever reported as changed": a
//!   batch was dropped (`EAGAIN`, `EMSGSIZE`), the sender hit its path budget,
//!   or the process changed directory. It can only cause one conservative
//!   all-view reload.
//! * `seq` is monotonic per source. [`Sequence`] tells the consumer whether a
//!   batch continues the stream, starts one, or leaves a gap that has to
//!   rebuild the source's contribution.
//!
//! # Bounds
//!
//! A path is UTF-8 and absolute and at most [`MAX_PATH_BYTES`] long; a datagram
//! carries at most [`MAX_PATHS`] of them and at most [`MAX_BATCH_BYTES`]
//! encoded. [`chunks`] enforces both, so a batch never approaches the measured
//! `EMSGSIZE` (~256 KiB) — and [`Reporter`] sends with a non-blocking socket,
//! so a full peer buffer drops the batch and sets `resync` instead of ever
//! blocking a request on reporting.

use std::fmt;
use std::io;
use std::os::unix::net::UnixDatagram;

/// The codec version this build speaks. A batch with any other value is
/// refused whole: a half-understood report is worse than a resync.
pub const VERSION: u8 = 1;

/// `flags` bit 0: this batch replaces the source's set (a fresh worker).
pub const FLAG_BOOT: u8 = 0b0000_0001;

/// `flags` bit 1: treat everything this source ever reported as changed.
pub const FLAG_RESYNC: u8 = 0b0000_0010;

/// The flag bits this version defines; every other bit must be zero.
pub const FLAGS_KNOWN: u8 = FLAG_BOOT | FLAG_RESYNC;

/// The `source` of the `preload` phase, when a report comes from it directly.
///
/// Nothing sends as this source today — the preloaded closure is reported by
/// every worker's boot batch — and the value stays reserved so a later
/// parent-side reporter cannot collide with a route index.
pub const PRELOAD_SOURCE: u16 = 0xffff;

/// The most paths one datagram may carry (the research's measured safe batch).
pub const MAX_PATHS: usize = 64;

/// The most bytes one path may take on the wire. Longer paths are dropped by
/// the sender: a Linux path is `PATH_MAX` (4096) bytes including the NUL.
pub const MAX_PATH_BYTES: usize = 4096;

/// The most bytes one encoded datagram may take.
///
/// The measured in-flight ceiling of a `SOCK_DGRAM` socketpair is about 40 KiB
/// before `EAGAIN` (research §2.3) and a full 64 × 4096-byte batch would be
/// 256 KiB and `EMSGSIZE`. Design §4.4 therefore pins the encoder at **32 KiB**,
/// so a datagram always fits the measured in-flight window on an idle peer
/// instead of being `EAGAIN`-dropped on arrival.
pub const MAX_BATCH_BYTES: usize = 32 * 1024;

/// `version`, `flags`, `source`, `seq`, `count`.
pub const HEADER_BYTES: usize = 1 + 1 + 2 + 4 + 2;

/// One datagram's worth of a loaded-file report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    /// The reporting worker's identity (see the module documentation).
    pub source: u16,
    /// Monotonic per `source`; `0` exactly for a boot batch.
    pub seq: u32,
    /// This batch replaces the source's set.
    pub boot: bool,
    /// Treat every path this source ever reported as changed.
    pub resync: bool,
    /// The paths, UTF-8 and absolute, in the sender's order.
    pub paths: Vec<String>,
}

impl Batch {
    /// A batch, refusing one the wire format could not carry.
    ///
    /// # Errors
    ///
    /// [`EncodeError`] naming the first path that breaks a bound, or
    /// [`EncodeError::BootNotFirst`] for a `boot` batch whose `seq` is not `0`
    /// (a chunked boot must be sent as `boot` + continuing deltas).
    pub fn new(
        source: u16,
        seq: u32,
        boot: bool,
        resync: bool,
        paths: Vec<String>,
    ) -> Result<Self, EncodeError> {
        if boot && seq != 0 {
            return Err(EncodeError::BootNotFirst { seq });
        }
        check_paths(&paths)?;
        Ok(Self {
            source,
            seq,
            boot,
            resync,
            paths,
        })
    }

    /// The encoded datagram, little-endian.
    ///
    /// The batch was validated by [`Batch::new`] (or [`decode`]), so this
    /// cannot fail; it is deliberately infallible so a reporting path has no
    /// error branch to get wrong.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        debug_assert!(
            !self.boot || self.seq == 0,
            "a boot batch must start the stream (design §4.4)"
        );
        let mut out = Vec::with_capacity(
            HEADER_BYTES + self.paths.iter().map(|p| 2 + p.len()).sum::<usize>(),
        );
        let mut flags = 0u8;
        if self.boot {
            flags |= FLAG_BOOT;
        }
        if self.resync {
            flags |= FLAG_RESYNC;
        }
        out.push(VERSION);
        out.push(flags);
        out.extend_from_slice(&self.source.to_le_bytes());
        out.extend_from_slice(&self.seq.to_le_bytes());
        // `check_paths` capped the count at `MAX_PATHS`.
        let count = u16::try_from(self.paths.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&count.to_le_bytes());
        for path in &self.paths {
            let len = u16::try_from(path.len()).unwrap_or(u16::MAX);
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(path.as_bytes());
        }
        out
    }
}

/// Why a batch could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// More than [`MAX_PATHS`] paths in one batch.
    CountAboveCap(usize),
    /// One path longer than [`MAX_PATH_BYTES`].
    PathAboveCap {
        /// The path's index in the batch.
        index: usize,
        /// Its length in bytes.
        len: usize,
    },
    /// An empty path is not a file.
    EmptyPath {
        /// The path's index in the batch.
        index: usize,
    },
    /// A path with a NUL byte could never be a filesystem path.
    Nul {
        /// The path's index in the batch.
        index: usize,
    },
    /// The encoded batch would exceed [`MAX_BATCH_BYTES`].
    BatchAboveCap(usize),
    /// A `boot` batch that does not start its source's stream.
    ///
    /// Design §4.4: `boot` is the **one** datagram that replaces the source's
    /// set, so it carries `seq = 0`. Without this rule a chunked boot is
    /// indistinguishable from a genuine re-boot.
    BootNotFirst {
        /// The `seq` the caller asked for.
        seq: u32,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountAboveCap(count) => {
                write!(f, "{count} paths in one batch, at most {MAX_PATHS} fit")
            }
            Self::PathAboveCap { index, len } => write!(
                f,
                "path {index} is {len} bytes, at most {MAX_PATH_BYTES} fit"
            ),
            Self::EmptyPath { index } => write!(f, "path {index} is empty"),
            Self::Nul { index } => write!(f, "path {index} contains a NUL byte"),
            Self::BatchAboveCap(bytes) => {
                write!(f, "batch is {bytes} bytes, at most {MAX_BATCH_BYTES} fit")
            }
            Self::BootNotFirst { seq } => write!(
                f,
                "a boot batch must start the stream, but seq is {seq} (expected 0)"
            ),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Why a datagram could not be read as a batch.
///
/// Every variant means "drop this datagram", never "serve something from half
/// of it": the consumer treats a refusal as a gap and asks the source to
/// rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Fewer bytes than a header, or a record cut short.
    Truncated {
        /// Bytes needed at that point.
        need: usize,
        /// Bytes available.
        have: usize,
    },
    /// A version this build does not speak.
    Version(u8),
    /// A flag bit this version does not define.
    Flags(u8),
    /// A `count` above [`MAX_PATHS`].
    CountAboveCap(usize),
    /// A path longer than [`MAX_PATH_BYTES`].
    PathAboveCap {
        /// The path's index in the batch.
        index: usize,
        /// Its declared length.
        len: usize,
    },
    /// A zero-length path.
    EmptyPath {
        /// The path's index in the batch.
        index: usize,
    },
    /// A path that is not UTF-8.
    NotUtf8 {
        /// The path's index in the batch.
        index: usize,
    },
    /// A path containing a NUL byte.
    Nul {
        /// The path's index in the batch.
        index: usize,
    },
    /// The datagram is larger than [`MAX_BATCH_BYTES`].
    BatchAboveCap(usize),
    /// Bytes after the last record: a sender this build does not understand.
    Trailing(usize),
    /// A `boot` batch whose `seq` is not `0`: the wire cannot tell a chunked
    /// boot from a genuine re-boot, so it is refused whole.
    BootNotFirst {
        /// The `seq` the datagram carried.
        seq: u32,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { need, have } => {
                write!(f, "truncated batch: {have} bytes, {need} needed")
            }
            Self::Version(found) => write!(f, "codec version {found}, this build speaks {VERSION}"),
            Self::Flags(found) => write!(f, "flag bits {found:#04x} are not defined"),
            Self::CountAboveCap(count) => write!(
                f,
                "count {count}, at most {MAX_PATHS} paths fit in one batch"
            ),
            Self::PathAboveCap { index, len } => write!(
                f,
                "path {index} is {len} bytes, at most {MAX_PATH_BYTES} fit"
            ),
            Self::EmptyPath { index } => write!(f, "path {index} is empty"),
            Self::NotUtf8 { index } => write!(f, "path {index} is not UTF-8"),
            Self::Nul { index } => write!(f, "path {index} contains a NUL byte"),
            Self::BatchAboveCap(bytes) => write!(
                f,
                "datagram is {bytes} bytes, at most {MAX_BATCH_BYTES} fit"
            ),
            Self::Trailing(bytes) => write!(f, "{bytes} byte(s) after the last record"),
            Self::BootNotFirst { seq } => write!(
                f,
                "a boot batch must start the stream, but seq is {seq} (expected 0)"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Reads one datagram as a batch, refusing anything outside the format.
///
/// # Errors
///
/// [`DecodeError`] naming the first field that breaks the format.
pub fn decode(bytes: &[u8]) -> Result<Batch, DecodeError> {
    if bytes.len() > MAX_BATCH_BYTES {
        return Err(DecodeError::BatchAboveCap(bytes.len()));
    }
    let have = bytes.len();
    if have < HEADER_BYTES {
        return Err(DecodeError::Truncated {
            need: HEADER_BYTES,
            have,
        });
    }
    let version = bytes[0];
    if version != VERSION {
        return Err(DecodeError::Version(version));
    }
    let flags = bytes[1];
    if flags & !FLAGS_KNOWN != 0 {
        return Err(DecodeError::Flags(flags));
    }
    let source = u16::from_le_bytes([bytes[2], bytes[3]]);
    let seq = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if flags & FLAG_BOOT != 0 && seq != 0 {
        return Err(DecodeError::BootNotFirst { seq });
    }
    let count = usize::from(u16::from_le_bytes([bytes[8], bytes[9]]));
    if count > MAX_PATHS {
        return Err(DecodeError::CountAboveCap(count));
    }
    let mut paths = Vec::with_capacity(count);
    let mut at = HEADER_BYTES;
    for index in 0..count {
        let header_end = at + 2;
        if have < header_end {
            return Err(DecodeError::Truncated {
                need: header_end,
                have,
            });
        }
        let len = usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]));
        at = header_end;
        if len == 0 {
            return Err(DecodeError::EmptyPath { index });
        }
        if len > MAX_PATH_BYTES {
            return Err(DecodeError::PathAboveCap { index, len });
        }
        let end = at + len;
        if have < end {
            return Err(DecodeError::Truncated { need: end, have });
        }
        let raw = &bytes[at..end];
        at = end;
        let path = std::str::from_utf8(raw).map_err(|_| DecodeError::NotUtf8 { index })?;
        if path.contains('\0') {
            return Err(DecodeError::Nul { index });
        }
        paths.push(path.to_owned());
    }
    if at != have {
        return Err(DecodeError::Trailing(have - at));
    }
    Ok(Batch {
        source,
        seq,
        boot: flags & FLAG_BOOT != 0,
        resync: flags & FLAG_RESYNC != 0,
        paths,
    })
}

/// What one batch means for the source it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// This batch replaces the source's set.
    pub boot: bool,
    /// Treat every path the source ever reported as changed.
    pub resync: bool,
    /// The stream skipped at least one batch, so the set must be rebuilt: the
    /// paths this datagram does not repeat are not paths the source stopped
    /// loading.
    pub gap: bool,
}

impl Step {
    /// Whether the consumer has to rebuild the source's contribution before
    /// applying this batch.
    #[must_use]
    pub fn rebuilds(&self) -> bool {
        self.boot || self.resync || self.gap
    }
}

/// The per-source `seq` check, kept by the consumer.
///
/// A gap is reported, never swallowed: a lost datagram is exactly the case
/// where a view's set would otherwise silently lose files (the node's "seq
/// gaps and `resync` are surfaced to the caller, not dropped").
#[derive(Debug, Default, Clone, Copy)]
pub struct Sequence {
    last: Option<u32>,
}

impl Sequence {
    /// A tracker that has not seen a batch yet.
    #[must_use]
    pub fn new() -> Self {
        Self { last: None }
    }

    /// The last `seq` accepted from this source, if any.
    #[must_use]
    pub fn last(&self) -> Option<u32> {
        self.last
    }

    /// Accept one batch and report what it means.
    ///
    /// A boot batch replaces the set and restarts the stream at its own `seq`;
    /// the wire guarantees that `seq` is `0` ([`Batch::new`] and [`decode`] both
    /// refuse a `boot` with any other `seq`), so a chunked boot is always one
    /// `boot` followed by continuing deltas. Any other batch must be the
    /// previous one plus one; a first batch from a source the consumer has not
    /// seen since a reboot is a gap too, because the set it belongs to is
    /// unknown.
    pub fn accept(&mut self, batch: &Batch) -> Step {
        debug_assert!(
            !batch.boot || batch.seq == 0,
            "a boot batch must start the stream (design §4.4)"
        );
        let gap = if batch.boot {
            false
        } else {
            match self.last {
                Some(last) => batch.seq != last.wrapping_add(1),
                None => true,
            }
        };
        self.last = Some(batch.seq);
        Step {
            boot: batch.boot,
            resync: batch.resync,
            gap,
        }
    }
}

/// Splits `paths` into datagram-sized runs: at most [`MAX_PATHS`] paths and at
/// most [`MAX_BATCH_BYTES`] encoded bytes each, in order.
///
/// A path that is alone larger than the byte budget is the caller's bug —
/// [`Batch::new`] refuses one longer than [`MAX_PATH_BYTES`] — so a chunk is
/// always non-empty and always fits.
#[must_use]
pub fn chunks(paths: &[String]) -> Vec<&[String]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut bytes = HEADER_BYTES;
    for (index, path) in paths.iter().enumerate() {
        let size = 2 + path.len();
        let full = index - start >= MAX_PATHS || bytes + size > MAX_BATCH_BYTES;
        if index > start && full {
            out.push(&paths[start..index]);
            start = index;
            bytes = HEADER_BYTES;
        }
        bytes += size;
    }
    if start < paths.len() {
        out.push(&paths[start..]);
    }
    out
}

/// Refuses a path list the wire format could not carry.
fn check_paths(paths: &[String]) -> Result<(), EncodeError> {
    if paths.len() > MAX_PATHS {
        return Err(EncodeError::CountAboveCap(paths.len()));
    }
    let mut bytes = HEADER_BYTES;
    for (index, path) in paths.iter().enumerate() {
        if path.is_empty() {
            return Err(EncodeError::EmptyPath { index });
        }
        if path.len() > MAX_PATH_BYTES {
            return Err(EncodeError::PathAboveCap {
                index,
                len: path.len(),
            });
        }
        if path.contains('\0') {
            return Err(EncodeError::Nul { index });
        }
        bytes += 2 + path.len();
    }
    if bytes > MAX_BATCH_BYTES {
        return Err(EncodeError::BatchAboveCap(bytes));
    }
    Ok(())
}

/// What one [`Reporter::send`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sent {
    /// How many datagrams left the process.
    pub datagrams: usize,
    /// How many paths they carried.
    pub paths: usize,
    /// A datagram was dropped; the next batch carries `resync`.
    pub dropped: bool,
}

/// The sender's half of the channel: one non-blocking datagram socket per view.
///
/// The socket is `SOCK_DGRAM` (a datagram is atomic) and non-blocking, which is
/// the safe, `forbid(unsafe_code)` spelling of `MSG_DONTWAIT`: a full peer
/// buffer returns `EAGAIN` instead of blocking the request that is reporting.
/// Any send error — `EAGAIN`, `EMSGSIZE`, a peer that has gone — drops the
/// batch and sets `resync`, which is exactly what the design asks for and is
/// always conservative.
#[derive(Debug)]
pub struct Reporter {
    socket: UnixDatagram,
    source: u16,
    seq: u32,
    resync: bool,
}

impl Reporter {
    /// A reporter for `source` writing to `socket`.
    ///
    /// # Errors
    ///
    /// The socket could not be made non-blocking.
    pub fn new(socket: UnixDatagram, source: u16) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            source,
            seq: 0,
            resync: false,
        })
    }

    /// The source every batch this reporter sends names.
    #[must_use]
    pub fn source(&self) -> u16 {
        self.source
    }

    /// Whether the next batch will carry `resync`.
    #[must_use]
    pub fn resync_pending(&self) -> bool {
        self.resync
    }

    /// Set `resync` for the next batch (a `chdir`, a lost send).
    pub fn arm_resync(&mut self) {
        self.resync = true;
    }

    /// Sends `paths` as one boot batch or as a delta, split into datagrams.
    ///
    /// A boot set larger than one datagram is sent as **one** `boot` (the first
    /// datagram, `seq = 0`) followed by plain delta datagrams whose `seq`
    /// continues the stream: design §4.4 defines `boot` as the single chunk
    /// that replaces the source's set, so flagging every chunk would make a
    /// consumer that follows the design drop all but the last one — and the
    /// wire would no longer distinguish a chunked boot from a genuine re-boot.
    ///
    /// Nothing is sent for an empty delta, and nothing is sent after the first
    /// failure: the batch is dropped and `resync` is armed, because a request
    /// must never wait for reporting.
    pub fn send(&mut self, boot: bool, paths: &[String]) -> Sent {
        if paths.is_empty() {
            return Sent::default();
        }
        let mut sent = Sent::default();
        let resync = self.resync;
        for chunk in chunks(paths) {
            // `boot` belongs to the one datagram that replaces the set.
            let is_boot = boot && sent.datagrams == 0;
            let Ok(batch) = Batch::new(self.source, self.seq, is_boot, resync, chunk.to_vec())
            else {
                // `chunks` already enforced every bound; treat the impossible
                // case as a dropped batch so the consumer resyncs.
                self.resync = true;
                sent.dropped = true;
                break;
            };
            match self.socket.send(&batch.encode()) {
                Ok(_) => {
                    sent.datagrams += 1;
                    sent.paths += chunk.len();
                    self.seq = self.seq.wrapping_add(1);
                }
                Err(_) => {
                    self.resync = true;
                    sent.dropped = true;
                    break;
                }
            }
        }
        if !sent.dropped {
            // The flag went out with the first datagram of this send.
            self.resync = false;
        }
        sent
    }
}

/// What a view is handed so it can report: its own datagram socket and the
/// source id its batches name.
///
/// The socket is the write end of the per-view socketpair; the supervisor keeps
/// the read end and attributes a datagram by the descriptor it arrived on. The
/// source is a diagnostic (`seq` space), never a route to restart (§4.4).
///
/// The reporter that consumes it is the view kind's — a WSGI view's is
/// `blastit-view-python`'s `LoadedReporter`, because it also owns the inherited
/// interpreter state — while this pair is plain transport and lives here.
#[derive(Debug)]
pub struct LoadedReport {
    /// The write end of the view's `SOCK_DGRAM` socketpair.
    pub socket: UnixDatagram,
    /// The id this view's batches name: its route index, or
    /// [`PRELOAD_SOURCE`] for the preload phase.
    pub source: u16,
}
