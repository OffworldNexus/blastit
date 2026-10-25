//! The `ctl.chan` messages between init and the scheduler (ARCHITECTURE.md §3,
//! §4).
//!
//! These share the frame header with `http.chan` and `view.chan` but use their
//! own type-code range (`0x40`–`0x44`) and always carry `id == 0`. Payload
//! layout:
//!
//! | Type | Code | Payload |
//! |---|---|---|
//! | `SPAWN` | `0x40` | `route u32` |
//! | `SPAWNED` | `0x41` | `route u32`, `pid u32` |
//! | `RETIRE` | `0x42` | `route u32`, `pid u32`, `reason string32` |
//! | `EXITED` | `0x43` | `route u32`, `pid u32`, `status u8 tag + i32` |
//! | `DRAIN` | `0x44` | empty |
//!
//! The scheduler's end of a new `view.chan` is **not** part of the `SPAWNED`
//! payload: it travels alongside the message as a file descriptor with
//! `SCM_RIGHTS` in the same `sendmsg`, so the payload only names the process.

use crate::error::FrameError;
use crate::frame::TypeCode;
use crate::wire::{Reader, Sink};

/// How a view process ended, carried by [`Ctl::Exited`].
///
/// Wire value: `u8` tag `0` for a normal exit with a code, `1` for a signal,
/// each followed by the `i32` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// `WIFEXITED`: the process returned this status.
    Code(i32),
    /// `WIFSIGNALED`: the process was killed by this signal.
    Signal(i32),
}

impl ExitStatus {
    /// The wire value of this status.
    pub const fn code(self) -> i32 {
        match self {
            Self::Code(value) | Self::Signal(value) => value,
        }
    }

    /// The tag byte that tells exit code from signal.
    pub const fn tag(self) -> u8 {
        match self {
            Self::Code(_) => 0,
            Self::Signal(_) => 1,
        }
    }

    /// Rebuild a status from its tag and value.
    ///
    /// # Errors
    ///
    /// [`FrameError::BadEnum`] for any tag other than `0` or `1`.
    pub const fn from_parts(tag: u8, value: i32) -> Result<Self, FrameError> {
        match tag {
            0 => Ok(Self::Code(value)),
            1 => Ok(Self::Signal(value)),
            other => Err(FrameError::BadEnum {
                field: "exit status tag",
                value: other as u64,
            }),
        }
    }

    pub(crate) fn write(&self, sink: &mut impl Sink) -> Result<(), FrameError> {
        sink.put_u8(self.tag());
        sink.put_raw(&self.code().to_le_bytes());
        Ok(())
    }

    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, FrameError> {
        let tag = reader.u8("exit status tag")?;
        let value = reader.i32("exit status")?;
        Self::from_parts(tag, value)
    }
}

/// A message on `ctl.chan`.
///
/// `route` is the manifest route's index in the routing table, which is how
/// both ends name a view without depending on a uid or a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ctl {
    /// Scheduler → init: fork a view process for `route`.
    Spawn {
        /// Route index whose view should be started.
        route: u32,
    },
    /// Init → scheduler: the view exists, with this pid.
    ///
    /// The scheduler's end of the new `view.chan` arrives as an `SCM_RIGHTS`
    /// file descriptor next to this message.
    Spawned {
        /// Route index the view serves.
        route: u32,
        /// Pid of the new process.
        pid: u32,
    },
    /// Scheduler → init: retire the view (idle, over the LRU cap, or damaged).
    Retire {
        /// Route index the view serves.
        route: u32,
        /// Pid of the process to retire.
        pid: u32,
        /// Why, for logs.
        reason: String,
    },
    /// Init → scheduler: the view is gone and has been reaped.
    Exited {
        /// Route index the view served.
        route: u32,
        /// Pid of the process.
        pid: u32,
        /// How it ended.
        status: ExitStatus,
    },
    /// Scheduler → init: finish in-flight work and stop spawning.
    Drain,
}

impl Ctl {
    /// The wire type code of this message.
    pub const fn type_code(&self) -> TypeCode {
        match self {
            Self::Spawn { .. } => TypeCode::Spawn,
            Self::Spawned { .. } => TypeCode::Spawned,
            Self::Retire { .. } => TypeCode::Retire,
            Self::Exited { .. } => TypeCode::Exited,
            Self::Drain => TypeCode::Drain,
        }
    }

    /// The wire type code as a byte.
    pub const fn code(&self) -> u8 {
        self.type_code().code()
    }

    /// Reject a control frame that is not addressed to id 0.
    pub(crate) fn check_id(&self, id: u32) -> Result<(), FrameError> {
        if id == 0 {
            Ok(())
        } else {
            Err(FrameError::IdNotZero { id })
        }
    }

    pub(crate) fn write_payload(&self, sink: &mut impl Sink) -> Result<(), FrameError> {
        match self {
            Self::Spawn { route } => {
                sink.put_u32(*route);
                Ok(())
            }
            Self::Spawned { route, pid } => {
                sink.put_u32(*route);
                sink.put_u32(*pid);
                Ok(())
            }
            Self::Retire { route, pid, reason } => {
                sink.put_u32(*route);
                sink.put_u32(*pid);
                sink.put_str32(reason, "reason")
            }
            Self::Exited { route, pid, status } => {
                sink.put_u32(*route);
                sink.put_u32(*pid);
                status.write(sink)
            }
            Self::Drain => Ok(()),
        }
    }

    pub(crate) fn decode(code: TypeCode, reader: &mut Reader<'_>) -> Result<Self, FrameError> {
        // The caller only routes the ctl.chan code range here (`Frame::decode`
        // matches on the ctl codes), but stay total anyway.
        debug_assert!(code.is_ctl(), "Ctl::decode called with {code:?}");
        match code {
            TypeCode::Spawn => Ok(Self::Spawn {
                route: reader.u32("route")?,
            }),
            TypeCode::Spawned => Ok(Self::Spawned {
                route: reader.u32("route")?,
                pid: reader.u32("pid")?,
            }),
            TypeCode::Retire => Ok(Self::Retire {
                route: reader.u32("route")?,
                pid: reader.u32("pid")?,
                reason: reader.string32("reason")?.to_owned(),
            }),
            TypeCode::Exited => Ok(Self::Exited {
                route: reader.u32("route")?,
                pid: reader.u32("pid")?,
                status: ExitStatus::read(reader)?,
            }),
            TypeCode::Drain => Ok(Self::Drain),
            _ => Err(FrameError::OutOfRange {
                field: "ctl type code",
                detail: "not a ctl.chan message",
            }),
        }
    }
}
