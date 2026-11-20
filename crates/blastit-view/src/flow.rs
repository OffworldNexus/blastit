//! Flow control: the credit windows of §3.1's "Flow control" paragraph.
//!
//! A `view.chan` is shared by every stream, so no stream may make a reader
//! wait: the I/O thread always keeps reading, and every per-stream body is
//! bounded by a *credit window* instead of by a buffer limit. Two windows
//! matter to a view, one in each direction:
//!
//! * **Request body** (`scheduler → view`). The scheduler may send
//!   [`INITIAL_WINDOW`] bytes of a request body without asking; after that it
//!   waits for `WINDOW` frames. The view sends one as the application consumes
//!   bytes from `wsgi.input`, so a slow application slows down its own client,
//!   over TCP, and nobody else. [`Flow::body_received`] is the other half of
//!   that contract: bytes beyond the credit are a protocol violation and the
//!   process is recycled (§3.1).
//!
//! * **Response body** (`view → scheduler`). Every request starts with
//!   [`INITIAL_WINDOW`] bytes of response credit; the scheduler (on behalf of
//!   http, which spends it as it writes to the client socket) grants more with
//!   `WINDOW` frames. A request thread that runs out of credit blocks in its
//!   write — with the GIL released, and on a condvar rather than on the shared
//!   channel — exactly as it would on a slow client's socket. That is
//!   [`Credit::take`]. `CANCEL`, and the end of the channel, close the window
//!   so a blocked thread wakes up and finishes instead of hanging the process.
//!
//! The request-body counter and the response credit both live in [`Flow`], one
//! per request, shared by the I/O thread, the request thread and `wsgi.input`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use blastit_frame::INITIAL_WINDOW;

use crate::sync::lock;

/// How much consumed request body the view credits back at once.
///
/// Granting one `WINDOW` per byte would be a frame per byte; waiting for more
/// than this delays the next read of a streaming upload. 64 KiB is a few
/// frames' worth at the default frame size and small next to the window.
pub const WINDOW_BATCH: usize = 64 * 1024;

/// The flow-control state of one request.
#[derive(Debug)]
pub struct Flow {
    /// Request-body bytes the scheduler may still send us.
    ///
    /// It starts at [`INITIAL_WINDOW`] and grows as the application consumes
    /// body bytes and the view sends `WINDOW` for them.
    body_left: Mutex<i64>,
    /// Response-body bytes we may still send.
    response: Credit,
}

impl Flow {
    /// A fresh window in both directions.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            body_left: Mutex::new(i64::from(INITIAL_WINDOW)),
            response: Credit::new(INITIAL_WINDOW),
        })
    }

    /// Account for `bytes` of request body that just arrived.
    ///
    /// Returns `false` when the scheduler sent more than its credit, which
    /// §3.1 calls a broken protocol: the caller decides what to do with it.
    pub fn body_received(&self, bytes: usize) -> bool {
        let mut left = lock(&self.body_left);
        if i64::try_from(bytes).unwrap_or(i64::MAX) > *left {
            return false;
        }
        *left -= bytes as i64;
        true
    }

    /// Credit the scheduler `bytes` more request-body bytes.
    ///
    /// The caller must send the matching `WINDOW` frame: the local count is
    /// raised first so that frames already in flight from the scheduler are
    /// never refused, and a lost `WINDOW` frame only ever stalls the sender.
    pub fn body_granted(&self, bytes: usize) {
        let mut left = lock(&self.body_left);
        *left = left.saturating_add(i64::try_from(bytes).unwrap_or(i64::MAX));
    }

    /// Request-body bytes the scheduler may still send us.
    ///
    /// `pub` because a sibling view crate reads it from outside this crate,
    /// where a crate-private item would not reach.
    pub fn body_left(&self) -> i64 {
        *lock(&self.body_left)
    }

    /// The response-body credit of this request.
    pub fn response(&self) -> &Credit {
        &self.response
    }
}

/// A one-directional credit window: how much body the peer still accepts.
#[derive(Debug)]
pub struct Credit {
    available: Mutex<u64>,
    wake: Condvar,
    closed: AtomicBool,
}

impl Credit {
    /// A window holding `bytes` of credit.
    fn new(bytes: u32) -> Self {
        Self {
            available: Mutex::new(u64::from(bytes)),
            wake: Condvar::new(),
            closed: AtomicBool::new(false),
        }
    }

    /// Take up to `want` bytes of credit, blocking until some is available.
    ///
    /// Returns `0` only when the window has been closed and has no credit left:
    /// the stream is over (`CANCEL`, or the channel's far end), and the caller
    /// must stop writing rather than wait for a grant that cannot come.
    ///
    /// Blocks on a condvar, so the caller must have released the GIL: nothing
    /// here touches the interpreter, but a request thread that waited while
    /// holding it would stop every other request in the process.
    pub fn take(&self, want: usize) -> usize {
        let mut available = lock(&self.available);
        loop {
            if *available > 0 {
                let taken = (want as u64).min(*available);
                *available -= taken;
                return taken as usize;
            }
            if self.closed.load(Ordering::SeqCst) {
                return 0;
            }
            available = self
                .wake
                .wait(available)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Take up to `want` bytes of credit without blocking.
    ///
    /// `Some(0)` means the window is closed and empty (the stream is over);
    /// `None` means it is open but holds no credit yet, so a caller that can
    /// only make progress by reading more frames (a single-threaded worker
    /// transport, which is `blastit-view-php`'s loop) can go and read them
    /// instead of parking here.
    ///
    /// This crate has no features after `MB.36.5`: the PHP worker that needs it
    /// is a sibling crate, so the method is always compiled.
    pub fn try_take(&self, want: usize) -> Option<usize> {
        let mut available = lock(&self.available);
        if *available > 0 {
            let taken = (want as u64).min(*available);
            *available -= taken;
            return Some(taken as usize);
        }
        self.closed.load(Ordering::SeqCst).then_some(0)
    }

    /// Grant `bytes` more credit.
    pub fn grant(&self, bytes: u32) {
        {
            let mut available = lock(&self.available);
            *available = available.saturating_add(u64::from(bytes));
        }
        self.wake.notify_all();
    }

    /// Close the window: no more credit is coming.
    ///
    /// A taker with credit left still spends it — the frames may yet be
    /// deliverable on a drained channel — but one that would have to wait
    /// stops instead.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }
}
