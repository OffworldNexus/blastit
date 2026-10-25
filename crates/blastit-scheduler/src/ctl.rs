//! `ctl.chan`: how the scheduler asks init for view processes
//! (ARCHITECTURE.md §3, §4).
//!
//! The channel carries four messages the scheduler can see ([`Ctl`]) and the
//! file descriptor of a brand-new `view.chan`, which travels *next to*
//! `SPAWNED` as `SCM_RIGHTS` data — never inside the payload (§3, `ctl.chan`
//! row of the channel table).
//!
//! [`CtlChannel`] is that channel as a trait, so tests can drive the scheduler
//! with an in-memory init and never fork anything. [`UnixCtl`] is the real
//! thing, over a `socketpair(AF_UNIX, SOCK_STREAM)`.

use std::io::IoSlice;
use std::io::IoSliceMut;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use blastit_frame::{Ctl, Frame, HEADER_LEN, Limits, RawFrame};
use rustix::cmsg_space;
use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};
use tokio::io::Interest;
use tokio::net::UnixStream;

use crate::error::SchedulerError;

/// Room for one `ctl.chan` message: the largest one is `RETIRE`, well under
/// this. A message that declares more than this is a protocol error.
const READ_CHUNK: usize = 64;

/// The `ctl.chan` end the scheduler holds.
///
/// Implementations must be *cancel safe* in [`CtlChannel::recv`]: the scheduler
/// polls it inside a `select!` and drops the future whenever another channel is
/// ready, then polls it again. [`UnixCtl`] keeps every byte it has read in its
/// own buffer, so a cancelled read loses nothing.
///
/// The trait uses `async fn` and is therefore not object safe; the scheduler
/// takes it as a generic parameter instead (§2.4's "pure Rust" end of the
/// process tree has no need for `dyn`).
#[allow(async_fn_in_trait)]
pub trait CtlChannel {
    /// Sends one `ctl.chan` message, with no file descriptor.
    ///
    /// The scheduler only ever sends `SPAWN`, `RETIRE` and `DRAIN`; the
    /// descriptor side of the protocol belongs to init, which answers
    /// `SPAWNED`.
    async fn send(&mut self, msg: Ctl) -> Result<(), SchedulerError>;

    /// Receives the next `ctl.chan` message and the descriptor that travelled
    /// with it, if any.
    ///
    /// Only `SPAWNED` carries one, and it carries exactly one: a message with
    /// the wrong number of descriptors is a protocol error, and the extras are
    /// closed (§3).
    async fn recv(&mut self) -> Result<(Ctl, Option<OwnedFd>), SchedulerError>;
}

/// A [`CtlChannel`] over a raw Unix socket, moving file descriptors with
/// `SCM_RIGHTS`.
///
/// Reads are exact: the reader takes only as many bytes as the frame it is
/// assembling needs, so one `recvmsg` can never span two messages. That is what
/// makes the association between a descriptor and the `SPAWNED` it belongs to
/// unambiguous on a stream socket.
#[derive(Debug)]
pub struct UnixCtl {
    stream: UnixStream,
    limits: Limits,
    read: Vec<u8>,
    descriptors: Vec<OwnedFd>,
    write: Vec<u8>,
    written: usize,
}

impl UnixCtl {
    /// A control channel with the default frame limit.
    pub fn new(stream: UnixStream) -> Self {
        Self::with_limits(stream, Limits::default())
    }

    /// A control channel for a connection with this frame limit.
    pub fn with_limits(stream: UnixStream, limits: Limits) -> Self {
        Self {
            stream,
            limits,
            read: Vec::new(),
            descriptors: Vec::new(),
            write: Vec::new(),
            written: 0,
        }
    }

    /// Sends `msg`, optionally handing a file descriptor to the peer.
    ///
    /// init uses this for `SPAWNED`: the descriptor is the scheduler's end of
    /// the new `view.chan` (§4). The scheduler itself only needs
    /// [`CtlChannel::send`], which is this with no descriptor.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Frame`] when `msg` cannot be encoded,
    /// [`SchedulerError::Io`] when the socket fails, and
    /// [`SchedulerError::Closed`] when the peer went away.
    pub async fn send_with_fd(
        &mut self,
        msg: &Ctl,
        fd: Option<OwnedFd>,
    ) -> Result<(), SchedulerError> {
        self.write.clear();
        self.written = 0;
        self.limits
            .encode(&Frame::Ctl(msg.clone()), 0, &mut self.write)?;

        while self.written < self.write.len() {
            let borrowed: Vec<BorrowedFd<'_>> = fd.iter().map(AsFd::as_fd).collect();
            let mut space = [MaybeUninit::<u8>::uninit(); cmsg_space!(ScmRights(1))];
            let mut control = SendAncillaryBuffer::new(&mut space);
            if self.written == 0 && !borrowed.is_empty() {
                control.push(SendAncillaryMessage::ScmRights(&borrowed));
            }
            let from = self.written;
            let written = self
                .stream
                .async_io(Interest::WRITABLE, || {
                    let sent = sendmsg(
                        &self.stream,
                        &[IoSlice::new(&self.write[from..])],
                        &mut control,
                        SendFlags::empty(),
                    )?;
                    Ok(sent)
                })
                .await
                .map_err(SchedulerError::io)?;
            if written == 0 {
                return Err(SchedulerError::closed());
            }
            self.written += written;
        }
        Ok(())
    }

    /// The next frame, or `None` when the buffered bytes are not a whole one
    /// yet.
    fn take_frame(&mut self) -> Result<Option<Frame>, SchedulerError> {
        if self.read.len() < HEADER_LEN {
            return Ok(None);
        }
        let mut length = [0u8; 4];
        length.copy_from_slice(&self.read[..4]);
        let total = self
            .limits
            .check_declared_len(u32::from_le_bytes(length))
            .map_err(SchedulerError::frame)?;
        if self.read.len() < total {
            return Ok(None);
        }
        let ty = self.read[4];
        let mut id = [0u8; 4];
        id.copy_from_slice(&self.read[5..HEADER_LEN]);
        if u32::from_le_bytes(id) != 0 {
            return Err(SchedulerError::protocol(
                "ctl.chan frame with a non-zero id",
            ));
        }
        let payload = self.read[HEADER_LEN..total].to_vec();
        self.read.drain(..total);
        RawFrame::new(ty, 0, payload)
            .into_frame()
            .map(Some)
            .map_err(SchedulerError::frame)
    }

    /// Turns a decoded frame into a message plus its descriptor, checking the
    /// descriptor count the protocol allows.
    fn finish_frame(&mut self, frame: Frame) -> Result<(Ctl, Option<OwnedFd>), SchedulerError> {
        let msg = match frame {
            Frame::Ctl(msg) => msg,
            other => {
                return Err(SchedulerError::protocol(format!(
                    "non-ctl frame on ctl.chan: {other:?}"
                )));
            }
        };
        match msg {
            Ctl::Spawned { .. } => {
                if self.descriptors.len() != 1 {
                    return Err(SchedulerError::protocol(format!(
                        "SPAWNED carried {} file descriptors, expected exactly one",
                        self.descriptors.len()
                    )));
                }
                let fd = self.descriptors.remove(0);
                Ok((msg, Some(fd)))
            }
            other => {
                if !self.descriptors.is_empty() {
                    return Err(SchedulerError::protocol(format!(
                        "{} carried {} file descriptors, expected none",
                        other.type_code().name(),
                        self.descriptors.len()
                    )));
                }
                Ok((other, None))
            }
        }
    }

    /// Reads exactly `want` more bytes, plus whatever descriptors come with
    /// them.
    async fn read_exact_more(&mut self, want: usize) -> Result<(), SchedulerError> {
        let mut offset = 0;
        while offset < want {
            let mut buf = [0u8; READ_CHUNK];
            let take = (want - offset).min(READ_CHUNK);
            let mut space = [MaybeUninit::<u8>::uninit(); cmsg_space!(ScmRights(1))];
            let read = self
                .stream
                .async_io(Interest::READABLE, || {
                    let mut control = RecvAncillaryBuffer::new(&mut space);
                    let msg = recvmsg(
                        &self.stream,
                        &mut [IoSliceMut::new(&mut buf[..take])],
                        &mut control,
                        RecvFlags::CMSG_CLOEXEC,
                    )?;
                    let mut descriptors: Vec<OwnedFd> = Vec::new();
                    for message in control.drain() {
                        if let RecvAncillaryMessage::ScmRights(fds) = message {
                            descriptors.extend(fds);
                        }
                    }
                    Ok((msg.bytes, descriptors))
                })
                .await
                .map_err(SchedulerError::io)?;
            let (read, mut fds) = read;
            if read == 0 {
                return Err(SchedulerError::closed());
            }
            self.read.extend_from_slice(&buf[..read]);
            self.descriptors.append(&mut fds);
            offset += read;
        }
        Ok(())
    }

    /// How many bytes the frame being assembled still needs.
    ///
    /// Four bytes at first, to learn the length; then exactly the rest of that
    /// frame. Reading only what the current frame needs is what keeps a
    /// descriptor from being attributed to the wrong message.
    fn needed(&mut self) -> Result<usize, SchedulerError> {
        if self.read.len() < HEADER_LEN {
            return Ok(HEADER_LEN - self.read.len());
        }
        let mut length = [0u8; 4];
        length.copy_from_slice(&self.read[..4]);
        let total = self
            .limits
            .check_declared_len(u32::from_le_bytes(length))
            .map_err(SchedulerError::frame)?;
        Ok(total - self.read.len())
    }
}

impl CtlChannel for UnixCtl {
    async fn send(&mut self, msg: Ctl) -> Result<(), SchedulerError> {
        self.send_with_fd(&msg, None).await
    }

    async fn recv(&mut self) -> Result<(Ctl, Option<OwnedFd>), SchedulerError> {
        loop {
            if let Some(frame) = self.take_frame()? {
                return self.finish_frame(frame);
            }
            let want = self.needed()?;
            self.read_exact_more(want).await?;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn ctl_messages_encode_with_id_zero() {
        let limits = Limits::default();
        let mut wire = Vec::new();
        limits
            .encode(&Frame::Ctl(Ctl::Spawn { route: 7 }), 0, &mut wire)
            .unwrap();
        assert_eq!(wire.len(), HEADER_LEN + 4);
        assert_eq!(wire[4], Ctl::Spawn { route: 7 }.code());
        assert_eq!(&wire[5..9], &[0, 0, 0, 0]);
        assert_eq!(&wire[9..], &7u32.to_le_bytes());

        // A message that does not fit the channel's limit is refused, not
        // truncated.
        let tight = Limits::new(16).unwrap();
        assert!(
            tight
                .encode(
                    &Frame::Ctl(Ctl::Retire {
                        route: 1,
                        pid: 2,
                        reason: "x".repeat(64),
                    }),
                    0,
                    &mut Vec::new(),
                )
                .is_err()
        );
    }
}
