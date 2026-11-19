//! The scheduling loop: `http.chan`, `ctl.chan` and every `view.chan`, driven
//! as one future on init's runtime.

use super::*;

/// Sleeps until `deadline`, or forever when there is none.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Owns `ctl.chan` for the whole run: reads init's messages into the
/// scheduler, and writes the scheduler's messages out.
///
/// The two directions share one task because the channel is one object. Only
/// the *scheduler's* loop matters for the "never wait for a peer" rule: it is
/// this task that would wait for init, and by then every message is already in
/// the unbounded queue.
async fn ctl_task<C: CtlChannel>(
    mut ctl: C,
    inbound: mpsc::Sender<CtlIn>,
    mut outbound: mpsc::UnboundedReceiver<OutFrame>,
    pending: Arc<AtomicUsize>,
    limits: Limits,
) {
    loop {
        tokio::select! {
            message = ctl.recv() => match message {
                Ok((message, fd)) => {
                    if inbound.send(CtlIn::Message(message, fd)).await.is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = inbound.send(CtlIn::Failed(error)).await;
                    return;
                }
            },
            // Only `Ctl` frames are ever pushed here, and dropping the queue
            // means the run is over.
            outgoing = outbound.recv() => {
                let Some(OutFrame::Frame { frame: Frame::Ctl(message), .. }) = outgoing else {
                    return;
                };
                let size = ctl_frame_size(&message, limits);
                if ctl.send(message).await.is_err() {
                    let _ = inbound.send(CtlIn::Failed(SchedulerError::closed())).await;
                    return;
                }
                release(&pending, size);
            }
        }
    }
}

/// Serves `http.chan` from `ctl.chan` and one `view.chan` per route.
///
/// Returns when init sends `DRAIN` (once the in-flight requests are done, or
/// `BLASTIT_REQUEST_TIMEOUT` after the message) or when `http.chan` closes. It
/// fails on a broken `ctl.chan`, on a `ctl.chan` message the protocol does not
/// allow, and on a `request`/`response` frame from http that does not decode.
///
/// # Contract with http
///
/// http never reuses a stream id while that stream is still in flight: the
/// scheduler tracks a request by its stream id until the view reports `DONE`
/// for it, and a second `REQ_HEAD` for a live id is a protocol error that ends
/// the run rather than letting one request inherit another's worker slot.
///
/// # Errors
///
/// [`SchedulerError`] for a broken `ctl.chan`, a message on it that the
/// protocol does not allow, a frame from http that does not decode, or one of
/// the byte-capped peer queues overflowing.
pub async fn run<C: CtlChannel + Send + 'static>(
    cfg: SchedulerConfig,
    routes: Vec<RouteInfo>,
    table: RoutingTable,
    http: UnixStream,
    ctl: C,
    emit: Emit,
) -> Result<(), SchedulerError> {
    // A manifest that declares a static mount needs the responder this binary
    // was built without: refuse at boot, before a single channel is served
    // (`MB.36.2`, ADR-0016 decision 6).
    #[cfg(not(feature = "static"))]
    if !cfg.mounts.is_empty() {
        return Err(SchedulerError::StaticNotCompiled);
    }
    let limits = cfg.frame_limits;
    let (http_read, http_write) = http.into_split();
    let (http_in_tx, mut http_in_rx) = mpsc::channel(HTTP_IN_CAPACITY);
    let (http_out, http_out_rx, http_pending) = Outbound::new(limits, HTTP_OUTBOUND_CAP);
    let (view_in_tx, mut view_in_rx) = mpsc::channel(VIEW_IN_CAPACITY);
    let (ctl_out, ctl_out_rx, ctl_pending) = Outbound::new(limits, CTL_OUTBOUND_CAP);

    // http's frames are read without decoding: it is blastit's own Rust code,
    // so its bytes are well-formed by construction, and the request head is
    // forwarded to the view verbatim instead of being copied header by header.
    // The view channel is read the strict way (see `view_spawned`); do not
    // switch it to `read_raw_frames`.
    tokio::spawn(read_raw_frames(
        http_read,
        limits,
        http_in_tx.clone(),
        |id, raw| HttpIn::Frame { id, raw },
        |failure| match failure {
            Some(error) => HttpIn::Failed(error),
            None => HttpIn::Closed,
        },
    ));
    tokio::spawn(write_peer(http_write, limits, http_out_rx, http_pending));

    let (ctl_in_tx, mut ctl_in_rx) = mpsc::channel(CTL_IN_CAPACITY);
    tokio::spawn(ctl_task(ctl, ctl_in_tx, ctl_out_rx, ctl_pending, limits));

    // Held for the whole run, so `http_in_rx` only ever ends with an explicit
    // `HttpIn::Closed`.
    let _keepalive = http_in_tx;
    // The static responder is compiled in: one root per mount is opened here as
    // the scheduler's uid and before a single request is served. A root that
    // cannot be opened is a broken declaration, not a degraded answer, so it
    // exits the role (§9.2).
    #[cfg(feature = "static")]
    let (mounts, mount_roots, static_in, mut static_in_rx) = {
        let mounts = MountTable::from_mounts(&cfg.mounts);
        let mut mount_roots: Vec<Arc<StaticRoot>> = Vec::with_capacity(cfg.mounts.len());
        for mount in &cfg.mounts {
            let root = StaticRoot::open(&cfg.manifest_dir, &mount.policy)
                .map_err(|error| SchedulerError::protocol(error.to_string()))?
                .with_prefix(&mount.prefix)
                // The one source of the per-file ceiling: no environment
                // variable and no manifest key (design §4.6).
                .with_max_bytes(cfg.static_max_bytes);
            mount_roots.push(Arc::new(root));
        }
        let (static_in, static_in_rx) = mpsc::channel::<StaticReadMsg>(STATIC_IN_CAPACITY);
        (mounts, mount_roots, static_in, static_in_rx)
    };
    // Nothing ever sends on it (see `StaticReadMsg`), and the sender is held
    // for the whole run, so its `select!` arm never completes.
    #[cfg(not(feature = "static"))]
    let (_static_in, mut static_in_rx) = mpsc::channel::<StaticReadMsg>(1);
    #[cfg(feature = "static")]
    let mut scheduler = Scheduler::new(
        cfg,
        routes,
        table,
        emit,
        http_out,
        view_in_tx,
        ctl_out,
        mounts,
        mount_roots,
        static_in,
    );
    #[cfg(not(feature = "static"))]
    let mut scheduler = Scheduler::new(cfg, routes, table, emit, http_out, view_in_tx, ctl_out);
    scheduler.start()?;

    loop {
        let deadline = scheduler.next_deadline();
        tokio::select! {
            message = ctl_in_rx.recv() => match message {
                Some(CtlIn::Message(message, fd)) => scheduler.on_ctl(message, fd)?,
                Some(CtlIn::Failed(error)) => return Err(error),
                None => return Err(SchedulerError::closed()),
            },
            Some(event) = http_in_rx.recv() => scheduler.on_http(event)?,
            Some(event) = view_in_rx.recv() => scheduler.on_view(event)?,
            Some(read) = static_in_rx.recv() => scheduler.on_static_read(read)?,
            () = sleep_until(deadline) => {}
        }
        scheduler.pump()?;
        if scheduler.done() {
            return Ok(());
        }
    }
}
