//! The in-process world the scheduler tests run in.
//!
//! Nothing here forks: a fake init answers `SPAWN` with a real `socketpair`
//! end, and a fake view task on the other end plays a script. The fake client
//! speaks frames on `http.chan` directly. That is the whole of blastit as far
//! as the scheduler can tell, which is exactly what these tests need.

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use blastit_events::Event;
use blastit_frame::{
    Ctl, Decoder, DoneOutcome, ExitStatus, Frame, INITIAL_WINDOW, Limits, ReqHead, RequestCost,
    RespHead, Scheme,
};
use blastit_manifest::{Manifest, RoutingTable, UidLayout};
use blastit_scheduler::{
    CtlChannel, Emit, MemoryProbe, SchedulerConfig, SchedulerError, routes_from_manifest, run,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{Notify, Semaphore, mpsc};
use tokio::task::JoinHandle;

/// How many `yield_now` round trips a spin-wait may take before it gives up.
///
/// A spin-wait never parks, so it never lets tokio's paused clock advance:
/// tests that must not trip a timeout spin instead of awaiting.
const SPIN_BUDGET: usize = 200_000;

/// A view's canned behaviour.
#[derive(Clone)]
pub enum Script {
    /// `READY`, then a `200` with `body` and `DONE { Completed }`.
    Answer { body: Vec<u8> },
    /// The answer, without the `DONE`: the response is complete but the worker
    /// slot is not released.
    NoDone { body: Vec<u8> },
    /// The answer, with `DONE` only after the gate is notified.
    Gated { body: Vec<u8>, gate: Arc<Notify> },
    /// `READY`, then a `RESP_HEAD` and nothing else.
    HeadOnly,
    /// `READY`, then silence.
    Silent,
    /// `READY`, then `DONE` only when a `CANCEL` arrives.
    WaitCancel,
    /// No `READY` at all, then the socket closes after `after`.
    NeverReady { after: Duration },
    /// `READY`, then the socket closes.
    ReadyThenExit,
    /// `READY`, then the socket closes as soon as a request arrives.
    ExitOnHead,
    /// `READY`, then answers, but first writes a frame under a foreign id.
    ForeignId { body: Vec<u8> },
    /// `READY`, then holds every request until it is granted a permit.
    Hold { body: Vec<u8>, gate: Arc<Semaphore> },
    /// `READY`, then a `RESP_HEAD`, a `RESP_ABORT` and `DONE` for each request.
    AbortAfterHead,
    /// `READY`, then a `RESP_ABORT` and `DONE`, with no head at all.
    AbortWithoutHead,
    /// `READY`, then: every `REQ_BODY` is consumed and granted back with a
    /// `WINDOW`, and the request is answered when `REQ_END` arrives.
    Consume { body: Vec<u8> },
    /// `READY`, then the answer, and `DONE` twice.
    DoubleDone { body: Vec<u8> },
    /// `READY`, then `DONE` after a delay, with no response at all.
    DelayedDone { after: Duration },
    /// `READY`, then a head and `count` body frames of `chunk` bytes, ignoring
    /// any credit: it breaks flow control on purpose (§3.1).
    OverCredit { chunk: usize, count: usize },
    /// `READY`, then it grants `window` bytes of request-body credit per
    /// request and stops reading altogether: the scheduler's queue towards it
    /// is what has to stay bounded.
    Deaf { window: u32 },
    /// `READY`, then a `200` with `body`, a `RESP_END_COST` reporting `cost`
    /// and a `DONE` (D5): what the fair-queuing tests need to say how much a
    /// request cost the view.
    Cost { body: Vec<u8>, cost: RequestCost },
    /// Like [`Script::Cost`], but each request waits for a permit before it is
    /// answered, so the test decides when it ends — and therefore how long the
    /// scheduler observed it.
    HeldCost {
        body: Vec<u8>,
        cost: RequestCost,
        gate: Arc<Semaphore>,
    },
}

/// What a script wants the view task to do after handling a frame.
enum Step {
    /// Keep reading.
    Continue,
    /// Close `view.chan`, which is what a dying process looks like.
    Close,
    /// Hold this request until the gate grants a permit.
    Hold(u32),
}

/// A fake memory model for the §8 Sizing tests.
///
/// Aggregate usage starts at `base` and grows by `per_view` every time the
/// fake init forks a view process, which is what §9.2 #2 says a fork does under
/// gVisor: the child copies the 2 MiB chunks of the preloaded heap it writes
/// to. Tests read or set the usage directly, and the scheduler sees it through
/// [`Memory::probe`].
pub struct Memory {
    usage: AtomicU64,
    per_view: u64,
}

impl Memory {
    /// A machine holding `base` bytes in which each view process costs
    /// `per_view` more, in bytes.
    pub fn new(base: u64, per_view: u64) -> Arc<Self> {
        Arc::new(Self {
            usage: AtomicU64::new(base),
            per_view,
        })
    }

    /// The probe the scheduler reads through: the usage right now.
    pub fn probe(self: &Arc<Self>) -> MemoryProbe {
        let model = self.clone();
        MemoryProbe::new(move || Some(model.usage()))
    }

    /// The usage right now, in bytes.
    pub fn usage(&self) -> u64 {
        self.usage.load(Ordering::Relaxed)
    }

    /// Sets the usage, as a machine going under or over a watermark would.
    pub fn set(&self, bytes: u64) {
        self.usage.store(bytes, Ordering::Relaxed);
    }

    /// Records one more forked view process.
    fn fork(&self) {
        self.usage.fetch_add(self.per_view, Ordering::Relaxed);
    }
}

/// Everything the fake init and the fake views saw.
#[derive(Default)]
pub struct Log {
    /// Frames a view received, as `(route, request id, frame)`.
    pub received: Vec<(usize, u32, Frame)>,
    /// Routes whose view channel was closed.
    pub closed: Vec<usize>,
    /// Routes init was asked to spawn, in order.
    pub spawns: Vec<usize>,
    /// `RETIRE` messages init received, as `(route, reason)`.
    pub retires: Vec<(usize, String)>,
    /// Every process init forked, as `(route, pid)`.
    pub pids: Vec<(usize, u32)>,
}

impl Log {
    /// The frames view `route` received.
    pub fn received_by(&self, route: usize) -> Vec<(u32, Frame)> {
        self.received
            .iter()
            .filter(|(seen, _, _)| *seen == route)
            .map(|(_, id, frame)| (*id, frame.clone()))
            .collect()
    }

    /// The request ids view `route` saw `REQ_HEAD` for.
    pub fn request_ids(&self, route: usize) -> Vec<u32> {
        self.received
            .iter()
            .filter(|(seen, _, frame)| *seen == route && matches!(frame, Frame::ReqHead(_)))
            .map(|(_, id, _)| *id)
            .collect()
    }

    /// Whether view `route` received a frame matching `pred`.
    pub fn saw(&self, route: usize, pred: impl Fn(&Frame) -> bool) -> bool {
        self.received
            .iter()
            .any(|(seen, _, frame)| *seen == route && pred(frame))
    }
}

/// The fake client's end of `http.chan`.
///
/// It tracks flow control the way a real front end does (§3.1): it never sends
/// more request-body bytes than the credit it has, and it grants response-body
/// credit as it "writes" the body out.
///
/// Writing is *queued*, not done here (D5): a real front end keeps reading
/// `http.chan` while its requests wait in the scheduler's admission queue, and a
/// test that had to wait for the scheduler to read before it could send the next
/// request would wedge on exactly the state these tests set up — a full queue.
/// [`Http::send`] hands the bytes to [`writer_task`], which never blocks the
/// test; the read half stays here, because every look at the world goes through
/// it.
pub struct Http {
    stream: OwnedReadHalf,
    /// Frames on their way out, in order, on the writer task.
    to_send: mpsc::UnboundedSender<Outgoing>,
    decoder: Decoder,
    inbox: VecDeque<(u32, Frame)>,
    closed: bool,
    /// Request-body bytes this client may still send per stream.
    upload: HashMap<u32, u32>,
    /// Response-body bytes this client has granted per stream.
    download: HashMap<u32, u32>,
    /// Total request-body credit granted back per stream, for tests.
    granted: HashMap<u32, u64>,
}

/// What the test client queues for [`writer_task`].
enum Outgoing {
    /// Bytes to write, already encoded.
    Bytes(Vec<u8>),
    /// Shut the write half down, which is what finishing EOF looks like.
    Close,
}

/// Writes what the test sends, in order, on a task of its own.
///
/// This is the piece that keeps a queued request from wedging the test: when the
/// scheduler stops reading — its own inbound queue is full, which is what a
/// full admission queue looks like from here — this task blocks in `write_all`
/// while the test keeps sending, asserting and looking at the world.
async fn writer_task(mut write: OwnedWriteHalf, mut outgoing: mpsc::UnboundedReceiver<Outgoing>) {
    while let Some(message) = outgoing.recv().await {
        match message {
            Outgoing::Bytes(bytes) => {
                if write.write_all(&bytes).await.is_err() {
                    return;
                }
            }
            Outgoing::Close => {
                let _ = write.shutdown().await;
                return;
            }
        }
    }
}

impl Http {
    fn new(stream: UnixStream) -> Self {
        let (stream, write) = stream.into_split();
        let (to_send, outgoing) = mpsc::unbounded_channel();
        tokio::spawn(writer_task(write, outgoing));
        Self {
            stream,
            to_send,
            decoder: Decoder::new(Limits::default()),
            inbox: VecDeque::new(),
            closed: false,
            upload: HashMap::new(),
            download: HashMap::new(),
            granted: HashMap::new(),
        }
    }

    /// Decodes whatever arrived, keeping the client's credit up to date.
    ///
    /// A `WINDOW` is pure flow control: it is accounted for and never queued,
    /// so a test that looks at a stream's response sees only the response.
    fn ingest(&mut self, bytes: &[u8]) {
        self.decoder.push(bytes);
        while let Ok(Some((id, frame))) = self.decoder.next_frame() {
            if let Frame::Window { bytes } = frame {
                let credit = self.upload.entry(id).or_insert(INITIAL_WINDOW);
                *credit = credit.saturating_add(bytes);
                let granted = self.granted.entry(id).or_insert(0);
                *granted = granted.saturating_add(u64::from(bytes));
                continue;
            }
            self.inbox.push_back((id, frame));
        }
    }

    /// Encodes one frame and queues it for the writer task.
    ///
    /// It yields once, so the writer usually has it on the wire before the
    /// caller looks at anything — but it never waits for the scheduler to take
    /// it, which is the difference between a test that can fill the scheduler's
    /// queue and one that cannot.
    pub async fn send(&mut self, id: u32, frame: &Frame) {
        let mut wire = Vec::new();
        Limits::default().encode(frame, id, &mut wire).unwrap();
        self.queue(wire).await;
    }

    /// Queues already-encoded bytes, yielding so the writer task can run.
    async fn queue(&mut self, bytes: Vec<u8>) {
        if self.to_send.send(Outgoing::Bytes(bytes)).is_err() {
            // The writer task is gone only when the scheduler's end is, which
            // every caller finds out through `closed()`.
            self.closed = true;
        }
        tokio::task::yield_now().await;
    }

    /// Sends a request head, which is what starts a stream's credit.
    pub async fn send_head(&mut self, id: u32, head: ReqHead) {
        self.upload.entry(id).or_insert(INITIAL_WINDOW);
        self.download.entry(id).or_insert(INITIAL_WINDOW);
        self.send(id, &Frame::ReqHead(head)).await;
    }

    /// Writes a body, chunked, under the credit the scheduler granted.
    pub async fn send_body(&mut self, id: u32, body: &[u8]) {
        self.send_body_inner(id, body, false).await;
    }

    /// Writes a body *ignoring* credit, which breaks the protocol on purpose.
    pub async fn send_body_forced(&mut self, id: u32, body: &[u8]) {
        self.send_body_inner(id, body, true).await;
    }

    async fn send_body_inner(&mut self, id: u32, body: &[u8], ignore_credit: bool) {
        let mut offset = 0;
        let mut reads = 0;
        while offset < body.len() {
            self.poll();
            if !ignore_credit {
                let credit = *self.upload.entry(id).or_insert(INITIAL_WINDOW);
                if credit == 0 {
                    // Wait for the view to consume some of what it has, which
                    // is what a real front end does.
                    assert!(
                        !self.stream_over(id),
                        "stream {id} ended while waiting for body credit"
                    );
                    assert!(reads < 64, "no credit for stream {id} after {reads} reads");
                    reads += 1;
                    let mut buf = [0u8; 8192];
                    let read = self.stream.read(&mut buf).await.unwrap();
                    assert!(read > 0, "http.chan closed while uploading");
                    self.ingest(&buf[..read]);
                    continue;
                }
                let take = (credit as usize).min(body.len() - offset).min(16 * 1024);
                self.send(id, &Frame::ReqBody(body[offset..offset + take].to_vec()))
                    .await;
                *self.upload.get_mut(&id).unwrap() -= take as u32;
                offset += take;
            } else {
                let take = (body.len() - offset).min(64 * 1024);
                self.send(id, &Frame::ReqBody(body[offset..offset + take].to_vec()))
                    .await;
                offset += take;
            }
        }
    }

    /// Sends raw bytes on `http.chan`, which a real http would never do.
    pub async fn send_raw(&mut self, bytes: &[u8]) {
        self.queue(bytes.to_vec()).await;
    }

    /// Sends a whole request: head, body and end.
    pub async fn send_request(&mut self, id: u32, head: ReqHead, body: &[u8]) {
        self.send_head(id, head).await;
        self.send_body(id, body).await;
        self.send(id, &Frame::ReqEnd).await;
    }

    /// Whether this stream already ended: a response terminator or a `CANCEL`.
    fn stream_over(&mut self, id: u32) -> bool {
        self.inbox.iter().any(|(stream, frame)| {
            *stream == id
                && matches!(
                    frame,
                    Frame::RespEnd
                        | Frame::RespEndCost { .. }
                        | Frame::RespAbort { .. }
                        | Frame::Cancel
                )
        })
    }

    /// Grants the view more response-body credit (§3.1).
    pub async fn grant(&mut self, id: u32, bytes: u32) {
        let credit = self.download.entry(id).or_insert(INITIAL_WINDOW);
        *credit = credit.saturating_add(bytes);
        self.send(id, &Frame::Window { bytes }).await;
    }

    /// Total request-body credit the view has granted back on a stream.
    pub fn granted_bytes(&mut self, id: u32) -> u64 {
        self.poll();
        self.granted.get(&id).copied().unwrap_or(0)
    }

    /// How much request-body credit this client has left on a stream.
    pub fn upload_credit(&mut self, id: u32) -> u32 {
        self.poll();
        *self.upload.entry(id).or_insert(INITIAL_WINDOW)
    }

    /// Writes whatever the scheduler sent, without waiting.
    pub fn poll(&mut self) {
        let mut buf = [0u8; 8192];
        loop {
            match self.stream.try_read(&mut buf) {
                Ok(0) => {
                    self.closed = true;
                    return;
                }
                Ok(read) => self.ingest(&buf[..read]),
                Err(_) => return,
            }
        }
    }

    /// The frames of a finished response for `id`, if they have all arrived.
    ///
    /// Nothing is consumed until the terminating frame is buffered: a response
    /// read in pieces must not lose its head to an earlier look.
    fn take_response(&mut self, id: u32) -> Option<Vec<Frame>> {
        let end = self.inbox.iter().position(|(stream, frame)| {
            *stream == id
                && matches!(
                    frame,
                    Frame::RespEnd | Frame::RespEndCost { .. } | Frame::RespAbort { .. }
                )
        })?;
        let mut frames = Vec::new();
        let mut rest = VecDeque::new();
        for (index, (stream, frame)) in self.inbox.drain(..).enumerate() {
            if stream == id && index <= end {
                frames.push(frame);
            } else {
                rest.push_back((stream, frame));
            }
        }
        self.inbox = rest;
        Some(frames)
    }

    /// Waits for a complete response for `id`, spinning so time stands still.
    pub async fn response(&mut self, id: u32) -> Vec<Frame> {
        for _ in 0..SPIN_BUDGET {
            self.poll();
            if let Some(frames) = self.take_response(id) {
                return frames;
            }
            tokio::task::yield_now().await;
        }
        panic!("no response for stream {id}; inbox holds {:?}", self.inbox);
    }

    /// Waits for a complete response for `id`, allowing timers to fire.
    ///
    /// This one parks when there is nothing to do, which is what makes tokio's
    /// paused clock advance to the next deadline: it is the way to observe a
    /// `504` or a `503` that only a timeout can produce.
    pub async fn awaited_response(&mut self, id: u32) -> Vec<Frame> {
        loop {
            self.poll();
            if let Some(frames) = self.take_response(id) {
                return frames;
            }
            let mut buf = [0u8; 8192];
            let read = self
                .stream
                .read(&mut buf)
                .await
                .expect("http.chan stayed open");
            assert!(read > 0, "http.chan closed while waiting for stream {id}");
            self.ingest(&buf[..read]);
        }
    }

    /// Whether the scheduler closed its end of `http.chan`.
    pub fn closed(&mut self) -> bool {
        self.poll();
        self.closed
    }

    /// Closes the client's end of `http.chan`, which ends the scheduler's run.
    pub async fn close(&mut self) {
        let _ = self.to_send.send(Outgoing::Close);
        tokio::task::yield_now().await;
    }

    /// Every frame buffered so far, leaving the inbox empty.
    pub fn take_all(&mut self) -> Vec<(u32, Frame)> {
        self.poll();
        self.inbox.drain(..).collect()
    }

    /// Frames still buffered that no test has taken yet.
    pub fn leftover(&mut self) -> Vec<(u32, Frame)> {
        self.poll();
        self.inbox.iter().cloned().collect()
    }
}

/// The scheduler, its fake peers, and everything they recorded.
pub struct World {
    pub cfg: SchedulerConfig,
    pub http: Http,
    pub events: Arc<Mutex<Vec<Event>>>,
    pub log: Arc<Mutex<Log>>,
    /// The scripts init gives new views; a test may replace one between
    /// spawns.
    pub scripts: Arc<Mutex<HashMap<usize, Script>>>,
    /// A raw `ctl.chan` sender towards the scheduler, for messages only init
    /// would send.
    pub ctl: mpsc::UnboundedSender<(Ctl, Option<OwnedFd>)>,
    /// The scheduler's future.
    run: Option<JoinHandle<Result<(), SchedulerError>>>,
}

/// The knobs a test turns before [`World::start`].
pub struct Options {
    pub cfg: SchedulerConfig,
    pub manifest: String,
    pub scripts: Arc<Mutex<HashMap<usize, Script>>>,
    pub exit_on_retire: bool,
    /// A fake memory model whose usage the fake init grows on every fork.
    pub memory_model: Option<Arc<Memory>>,
}

impl Options {
    /// The default configuration and a one-route manifest that matches
    /// everything.
    pub fn new() -> Self {
        Self {
            cfg: SchedulerConfig {
                max_workers: 4,
                queue_size: 8,
                queue_timeout: Duration::from_secs(60),
                request_timeout: Duration::from_secs(30),
                ..SchedulerConfig::default()
            },
            manifest: "version = 1\n[[route]]\nname = \"app\"\npath = ['.*']\n".to_string(),
            scripts: Arc::new(Mutex::new(HashMap::new())),
            exit_on_retire: true,
            memory_model: None,
        }
    }

    /// Replaces the manifest.
    pub fn manifest(mut self, source: &str) -> Self {
        self.manifest = source.to_string();
        self
    }

    /// Gives route `route` a script.
    pub fn script(self, route: usize, script: Script) -> Self {
        self.scripts
            .lock()
            .expect("scripts lock")
            .insert(route, script);
        self
    }

    /// Makes `model` the machine's memory: the scheduler reads it through a
    /// probe, and the fake init grows it by the model's per-view cost on every
    /// `SPAWN`, the way §9.2 #2 says a fork does.
    pub fn memory(mut self, model: Arc<Memory>) -> Self {
        self.cfg.memory_usage = Some(model.probe());
        self.memory_model = Some(model);
        self
    }
}

impl Default for Options {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// Starts the scheduler with a fake init and a fake client.
    pub fn start(options: Options) -> Self {
        let manifest = Manifest::from_toml_str(&options.manifest).expect("valid manifest");
        let table = RoutingTable::new(&manifest).expect("valid routing table");
        let routes = routes_from_manifest(&manifest, &UidLayout::default());
        let (scheduler_end, client_end) = UnixStream::pair().expect("http socketpair");
        let (to_init, mut from_scheduler) = mpsc::unbounded_channel::<Ctl>();
        let (to_scheduler, from_init) = mpsc::unbounded_channel::<(Ctl, Option<OwnedFd>)>();
        let ctl_tx = to_scheduler.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let collected = events.clone();
        let emit: Emit = Arc::new(move |event: Event| {
            collected.lock().expect("events lock").push(event);
        });
        let log = Arc::new(Mutex::new(Log::default()));
        let view_log = log.clone();

        let fake = FakeCtl { to_init, from_init };
        let run = tokio::spawn(run(
            options.cfg.clone(),
            routes,
            table,
            scheduler_end,
            fake,
            emit,
        ));

        let scripts = options.scripts.clone();
        let exit_on_retire = options.exit_on_retire;
        let memory_model = options.memory_model.clone();
        let init_log = log.clone();
        tokio::spawn(async move {
            let mut pid = 1000u32;
            while let Some(message) = from_scheduler.recv().await {
                match message {
                    Ctl::Spawn { route } => {
                        pid += 1;
                        let route = route as usize;
                        // §9.2 #2: by the time init reports the fork, the copy
                        // it costs is part of the aggregate reading.
                        if let Some(model) = &memory_model {
                            model.fork();
                        }
                        let (for_scheduler, for_view) =
                            UnixStream::pair().expect("view socketpair");
                        let script = scripts
                            .lock()
                            .expect("scripts lock")
                            .get(&route)
                            .cloned()
                            .unwrap_or(Script::Answer {
                                body: b"hello".to_vec(),
                            });
                        init_log.lock().expect("log lock").spawns.push(route);
                        init_log.lock().expect("log lock").pids.push((route, pid));
                        tokio::spawn(view_task(route, for_view, script, init_log.clone()));
                        let std_stream: StdUnixStream =
                            for_scheduler.into_std().expect("std UnixStream");
                        let fd: OwnedFd = std_stream.into();
                        let _ = to_scheduler.send((
                            Ctl::Spawned {
                                route: route as u32,
                                pid,
                            },
                            Some(fd),
                        ));
                    }
                    Ctl::Retire {
                        route,
                        pid: view_pid,
                        reason,
                    } => {
                        init_log
                            .lock()
                            .expect("log lock")
                            .retires
                            .push((route as usize, reason));
                        if exit_on_retire {
                            let _ = to_scheduler.send((
                                Ctl::Exited {
                                    route,
                                    pid: view_pid,
                                    status: ExitStatus::Signal(9),
                                },
                                None,
                            ));
                        }
                    }
                    Ctl::Drain | Ctl::Spawned { .. } | Ctl::Exited { .. } => {}
                }
            }
        });

        Self {
            cfg: options.cfg,
            http: Http::new(client_end),
            events,
            log: view_log,
            scripts: options.scripts.clone(),
            ctl: ctl_tx,
            run: Some(run),
        }
    }

    /// Replaces a route's script for the next view spawned for it.
    pub fn set_script(&self, route: usize, script: Script) {
        self.scripts
            .lock()
            .expect("scripts lock")
            .insert(route, script);
    }

    /// Sends a `ctl.chan` message to the scheduler, as init would.
    pub fn ctl_send(&self, message: Ctl) {
        let _ = self.ctl.send((message, None));
    }

    /// Sends a `ctl.chan` message with a descriptor, as init does for
    /// `SPAWNED`.
    pub fn ctl_send_fd(&self, message: Ctl, fd: OwnedFd) {
        let _ = self.ctl.send((message, Some(fd)));
    }

    /// Tells init the view for `route` is gone, the way a crashed process
    /// would be reaped.
    pub fn ctl_exit(&self, route: usize, pid: u32) {
        self.ctl_send(Ctl::Exited {
            route: route as u32,
            pid,
            status: ExitStatus::Signal(9),
        });
    }

    /// Moves the paused clock forward, then lets the scheduler settle.
    ///
    /// This parks, so tokio's auto-advance can move the clock to whichever
    /// deadline comes first (this one, or a scheduler timer). The yields after
    /// it let the scheduler act before the test looks at anything.
    pub async fn advance(&self, by: Duration) {
        tokio::time::advance(by).await;
        for _ in 0..64 {
            tokio::task::yield_now().await;
        }
    }

    /// The `request` events emitted so far.
    pub fn requests(&self) -> Vec<blastit_events::RequestInfo> {
        self.events
            .lock()
            .expect("events lock")
            .iter()
            .filter_map(|event| match event.kind() {
                blastit_events::EventKind::Request(info) => Some(info.clone()),
                _ => None,
            })
            .collect()
    }

    /// The `request` event of the request that had this `path`, if emitted.
    pub fn event_by_path(&self, path: &str) -> Option<blastit_events::RequestInfo> {
        self.requests()
            .into_iter()
            .find(|event| event.path() == path)
    }

    /// The `process.retired` events emitted so far.
    pub fn retirements(&self) -> Vec<blastit_events::ProcessRetired> {
        self.events
            .lock()
            .expect("events lock")
            .iter()
            .filter_map(|event| match event.kind() {
                blastit_events::EventKind::ProcessRetired(retired) => Some(retired.clone()),
                _ => None,
            })
            .collect()
    }

    /// The `process.cap` events emitted so far (§8 Sizing).
    pub fn caps(&self) -> Vec<blastit_events::ProcessCap> {
        self.events
            .lock()
            .expect("events lock")
            .iter()
            .filter_map(|event| match event.kind() {
                blastit_events::EventKind::ProcessCap(cap) => Some(*cap),
                _ => None,
            })
            .collect()
    }

    /// The `route` field of every `request` event so far, in order.
    pub fn request_routes(&self) -> Vec<Option<String>> {
        self.events
            .lock()
            .expect("events lock")
            .iter()
            .filter_map(|event| match event.kind() {
                blastit_events::EventKind::Request(_) => Some(event.route().map(str::to_owned)),
                _ => None,
            })
            .collect()
    }

    /// The whole event stream, as encoded JSON lines.
    pub fn event_lines(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("events lock")
            .iter()
            .map(blastit_events::encode_json)
            .collect()
    }

    /// Spins until `pred` holds, without letting the clock move.
    pub async fn until(&self, predicate: impl Fn(&World) -> bool) {
        for _ in 0..SPIN_BUDGET {
            if predicate(self) {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("condition never became true");
    }

    /// Spins until `pred` holds, without letting the clock move.
    pub async fn spin_until(predicate: impl Fn() -> bool) {
        for _ in 0..SPIN_BUDGET {
            if predicate() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("condition never became true");
    }

    /// Waits for the scheduler to return and gives back its result.
    pub async fn finish(&mut self) -> Result<(), SchedulerError> {
        let handle = self.run.take().expect("the scheduler ran once");
        handle.await.expect("the scheduler task did not panic")
    }
}

/// The scheduler's end of `ctl.chan`, in memory.
struct FakeCtl {
    to_init: mpsc::UnboundedSender<Ctl>,
    from_init: mpsc::UnboundedReceiver<(Ctl, Option<OwnedFd>)>,
}

impl CtlChannel for FakeCtl {
    async fn send(&mut self, msg: Ctl) -> Result<(), SchedulerError> {
        self.to_init.send(msg).map_err(|_| SchedulerError::closed())
    }

    async fn recv(&mut self) -> Result<(Ctl, Option<OwnedFd>), SchedulerError> {
        self.from_init
            .recv()
            .await
            .ok_or_else(SchedulerError::closed)
    }
}

/// A fake view process: reads frames, logs them, and plays its script.
///
/// A `Hold` script keeps reading while it holds its requests: the view task
/// races its `view.chan` against a gate permit, so a held request does not
/// stop the process from receiving the rest of the request — or a `CANCEL`.
async fn view_task(route: usize, stream: UnixStream, mut script: Script, log: Arc<Mutex<Log>>) {
    let (mut read, mut write) = stream.into_split();
    let mut decoder = Decoder::new(Limits::default());
    let mut buf = vec![0u8; 8192];
    let mut pending: VecDeque<u32> = VecDeque::new();
    if !matches!(script, Script::NeverReady { .. }) {
        send_frame(&mut write, 0, &Frame::Ready).await;
    }
    if let Script::NeverReady { after } = script {
        tokio::time::sleep(after).await;
        log.lock().expect("log lock").closed.push(route);
        return;
    }
    if matches!(script, Script::ReadyThenExit) {
        log.lock().expect("log lock").closed.push(route);
        return;
    }
    loop {
        tokio::select! {
            biased;
            result = read.read(&mut buf) => {
                let read_bytes = match result {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                decoder.push(&buf[..read_bytes]);
                loop {
                    match decoder.next_frame() {
                        Ok(Some((id, frame))) => {
                            log.lock()
                                .expect("log lock")
                                .received
                                .push((route, id, frame.clone()));
                            match script.step(id, &frame, &mut write).await {
                                Step::Continue => {}
                                Step::Close => {
                                    log.lock().expect("log lock").closed.push(route);
                                    return;
                                }
                                Step::Hold(id) => pending.push_back(id),
                            }
                        }
                        Ok(None) => break,
                        Err(_) => {
                            log.lock().expect("log lock").closed.push(route);
                            return;
                        }
                    }
                }
            }
            permit = script.permit(), if !pending.is_empty() => {
                let Some(permit) = permit else { break };
                if let Some(id) = pending.pop_front() {
                    permit.forget();
                    let body = script.hold_body();
                    match script.hold_cost() {
                        Some(cost) => answer_with_cost(&mut write, id, &body, cost, true).await,
                        None => answer(&mut write, id, &body, true).await,
                    }
                }
            }
        }
    }
    log.lock().expect("log lock").closed.push(route);
}

impl Script {
    /// Handles one frame, writing whatever the script answers.
    async fn step(
        &mut self,
        id: u32,
        frame: &Frame,
        write: &mut tokio::net::unix::OwnedWriteHalf,
    ) -> Step {
        let head = matches!(frame, Frame::ReqHead(_));
        match self {
            Script::Answer { body } => {
                if head {
                    answer(write, id, body, true).await;
                }
            }
            Script::NoDone { body } => {
                if head {
                    answer(write, id, body, false).await;
                }
            }
            Script::Gated { body, gate } => {
                if head {
                    answer(write, id, body, false).await;
                    gate.notified().await;
                    send_frame(
                        write,
                        id,
                        &Frame::Done {
                            outcome: DoneOutcome::Completed,
                        },
                    )
                    .await;
                }
            }
            Script::HeadOnly => {
                if head {
                    send_frame(write, id, &resp_head(200, 0)).await;
                }
            }
            Script::Silent => {}
            Script::Hold { body, gate } => {
                if head {
                    // A permit already granted answers at once; otherwise the
                    // request waits, and the process keeps reading.
                    match gate.clone().try_acquire_owned() {
                        Ok(permit) => {
                            permit.forget();
                            let body = body.clone();
                            answer(write, id, &body, true).await;
                        }
                        Err(_) => return Step::Hold(id),
                    }
                }
            }
            Script::WaitCancel => {
                if matches!(frame, Frame::Cancel) {
                    send_frame(
                        write,
                        id,
                        &Frame::Done {
                            outcome: DoneOutcome::Aborted,
                        },
                    )
                    .await;
                }
            }
            Script::NeverReady { .. } | Script::ReadyThenExit => {}
            Script::ExitOnHead => {
                if head {
                    return Step::Close;
                }
            }
            Script::ForeignId { body } => {
                if head {
                    // A request id this view was never given: the scheduler must
                    // drop it instead of touching another view's stream.
                    send_frame(write, id.wrapping_add(1000), &resp_head(200, 0)).await;
                    answer(write, id, body, true).await;
                }
            }
            Script::AbortAfterHead => {
                if head {
                    send_frame(write, id, &resp_head(200, 0)).await;
                    send_frame(
                        write,
                        id,
                        &Frame::RespAbort {
                            reason: "app raised".to_string(),
                        },
                    )
                    .await;
                    send_frame(
                        write,
                        id,
                        &Frame::Done {
                            outcome: DoneOutcome::Errored,
                        },
                    )
                    .await;
                }
            }
            Script::AbortWithoutHead => {
                if head {
                    send_frame(
                        write,
                        id,
                        &Frame::RespAbort {
                            reason: "app raised".to_string(),
                        },
                    )
                    .await;
                    send_frame(
                        write,
                        id,
                        &Frame::Done {
                            outcome: DoneOutcome::Errored,
                        },
                    )
                    .await;
                }
            }
            Script::Consume { body } => match frame {
                // The app read this much of `wsgi.input`, so the client may
                // send that much more (§3.1).
                Frame::ReqBody(chunk) => {
                    let bytes = u32::try_from(chunk.len()).unwrap_or(u32::MAX);
                    send_frame(write, id, &Frame::Window { bytes }).await;
                }
                Frame::ReqEnd => {
                    let body = body.clone();
                    answer(write, id, &body, true).await;
                }
                _ => {}
            },
            Script::DoubleDone { body } => {
                if head {
                    answer(write, id, body, true).await;
                    // A second `DONE` for the same request id: the slot was
                    // already released, so nothing may happen.
                    send_frame(
                        write,
                        id,
                        &Frame::Done {
                            outcome: DoneOutcome::Completed,
                        },
                    )
                    .await;
                }
            }
            Script::DelayedDone { after } => {
                if head {
                    let after = *after;
                    tokio::time::sleep(after).await;
                    send_frame(
                        write,
                        id,
                        &Frame::Done {
                            outcome: DoneOutcome::Completed,
                        },
                    )
                    .await;
                }
            }
            Script::OverCredit { chunk, count } => {
                if head {
                    let (chunk, count) = (*chunk, *count);
                    send_frame(write, id, &resp_head(200, chunk * count)).await;
                    let payload = vec![b'x'; chunk];
                    for _ in 0..count {
                        send_frame(write, id, &Frame::RespBody(payload.clone())).await;
                    }
                    send_frame(write, id, &Frame::RespEnd).await;
                }
            }
            Script::Cost { body, cost } => {
                if head {
                    answer_with_cost(write, id, body, *cost, true).await;
                }
            }
            Script::HeldCost { body, cost, gate } => {
                if head {
                    match gate.clone().try_acquire_owned() {
                        Ok(permit) => {
                            permit.forget();
                            answer_with_cost(write, id, body, *cost, true).await;
                        }
                        Err(_) => return Step::Hold(id),
                    }
                }
            }
            Script::Deaf { window } => {
                if head {
                    // Hand out credit and then never read again: the process is
                    // alive, it is simply not keeping up.
                    let window = *window;
                    send_frame(write, id, &Frame::Window { bytes: window }).await;
                    std::future::pending::<()>().await;
                }
            }
        }
        Step::Continue
    }

    /// Waits for a permit, for the scripts that hold requests.
    async fn permit(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        match self {
            Script::Hold { gate, .. } | Script::HeldCost { gate, .. } => {
                gate.clone().acquire_owned().await.ok()
            }
            _ => std::future::pending().await,
        }
    }

    /// The body a held request is answered with.
    fn hold_body(&self) -> Vec<u8> {
        match self {
            Script::Hold { body, .. } | Script::HeldCost { body, .. } => body.clone(),
            _ => Vec::new(),
        }
    }

    /// The cost a held request is answered with, when the script reports one.
    fn hold_cost(&self) -> Option<RequestCost> {
        match self {
            Script::HeldCost { cost, .. } => Some(*cost),
            _ => None,
        }
    }
}

/// Writes a head, body and a cost-reporting end, plus `DONE` when asked (D5).
async fn answer_with_cost(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    id: u32,
    body: &[u8],
    cost: RequestCost,
    done: bool,
) {
    send_frame(write, id, &resp_head(200, body.len())).await;
    if !body.is_empty() {
        send_frame(write, id, &Frame::RespBody(body.to_vec())).await;
    }
    send_frame(write, id, &Frame::RespEndCost { cost }).await;
    if done {
        send_frame(
            write,
            id,
            &Frame::Done {
                outcome: DoneOutcome::Completed,
            },
        )
        .await;
    }
}

/// Writes a head, body and end, plus `DONE` when asked.
async fn answer(write: &mut tokio::net::unix::OwnedWriteHalf, id: u32, body: &[u8], done: bool) {
    send_frame(write, id, &resp_head(200, body.len())).await;
    if !body.is_empty() {
        send_frame(write, id, &Frame::RespBody(body.to_vec())).await;
    }
    send_frame(write, id, &Frame::RespEnd).await;
    if done {
        send_frame(
            write,
            id,
            &Frame::Done {
                outcome: DoneOutcome::Completed,
            },
        )
        .await;
    }
}

/// A `200` response head with a `Content-Length`.
pub fn resp_head(status: u16, length: usize) -> Frame {
    Frame::RespHead(RespHead {
        status,
        reason: Vec::new(),
        headers: vec![(b"Content-Length".to_vec(), length.to_string().into_bytes())],
    })
}

/// Writes one frame, ignoring a peer that has gone away.
async fn send_frame(write: &mut tokio::net::unix::OwnedWriteHalf, id: u32, frame: &Frame) {
    let mut wire = Vec::new();
    if Limits::default().encode(frame, id, &mut wire).is_err() {
        return;
    }
    let _ = write.write_all(&wire).await;
}

/// A request head for `method` and `target`.
pub fn head(method: &str, target: &str) -> ReqHead {
    ReqHead {
        method: method.as_bytes().to_vec(),
        target: target.as_bytes().to_vec(),
        headers: vec![(b"Host".to_vec(), b"example.com".to_vec())],
        remote_addr: "203.0.113.7:41234".to_string(),
        scheme: Scheme::Http,
        server_name: "example.com".to_string(),
        server_port: 80,
        peer_addr: "203.0.113.7:41234".to_string(),
    }
}

/// A request head whose socket peer is `peer`.
///
/// The peer is what the client-IP trust decision and the fair queue's outer
/// flow are made on (D5), so a test that wants a header believed says where
/// the request came from.
pub fn head_from(method: &str, target: &str, peer: &str, forwarded: Option<&str>) -> ReqHead {
    let mut head = head(method, target);
    head.peer_addr = peer.to_string();
    head.remote_addr = peer.to_string();
    if let Some(value) = forwarded {
        head.headers
            .push((b"X-Forwarded-For".to_vec(), value.as_bytes().to_vec()));
    }
    head
}

/// The status of a response's first head frame.
pub fn status_of(frames: &[Frame]) -> u16 {
    match frames.first() {
        Some(Frame::RespHead(head)) => head.status,
        Some(Frame::RespAbort { .. }) => 0,
        other => panic!("no response head: {other:?}"),
    }
}

/// The body bytes of a response.
pub fn body_of(frames: &[Frame]) -> Vec<u8> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            Frame::RespBody(body) => Some(body.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Whether the frames end with a `RESP_END`, with or without a cost report.
pub fn ended(frames: &[Frame]) -> bool {
    matches!(
        frames.last(),
        Some(Frame::RespEnd | Frame::RespEndCost { .. })
    )
}

/// Whether the frames end with a `RESP_ABORT`.
pub fn aborted(frames: &[Frame]) -> bool {
    matches!(frames.last(), Some(Frame::RespAbort { .. }))
}

/// A manifest route source line.
pub fn route(name: &str, path: &str) -> String {
    format!("[[route]]\nname = \"{name}\"\npath = ['{path}']\n")
}

/// Waits until a view holds a request, and gives back that request's id.
pub fn view_request_id(log: &Log, route: usize) -> Option<u32> {
    log.request_ids(route).first().copied()
}
