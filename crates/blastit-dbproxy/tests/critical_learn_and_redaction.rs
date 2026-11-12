//! Critical: `dbproxy --learn`, dynamic-check and literal redaction
//! (R24, R26, R31; §7.2 row `R26 (db), R31`; ADR-0003, ADR-0004).
//!
//! Everything here runs unprivileged and offline against [`MockPgServer`] on
//! `127.0.0.1:0`: no root, no Docker, no `BLASTIT_TEST_PG_URL`.
//!
//! The positive twins prove one pass of a suite is enough:
//!
//! * fixture SQL under an empty target stack produces **zero** `obs_fd` lines,
//!   while `SET TIME ZONE 'UTC'` and `select pg_catalog.version()` are recorded
//!   and replayed exactly once on the first `push_routed`;
//! * `DynamicCheck` answers `Allow` to a statement no grant lists;
//! * `set_db` switches the database a proxy's next session requests.
//!
//! The negative twins prove the invariant behind them:
//!
//! * `Enforce` still answers `42501`, and `'s3cr3t'` appears in **no** byte of
//!   the `SqlDenied` event or the `obs_fd` stream;
//! * a `uid == 0` listener without `allow_root_listener` is rejected;
//! * a malformed or oversized control line cannot hang the run: it is answered
//!   and closed, and a stalled one is cut off by the 5 s deadline.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::os::fd::IntoRawFd;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use blastit_dbproxy::{
    DbproxyConfig, DbproxyError, Emit, ListenerMode, ListenerSpec, ObsTarget, ProxySpec,
    RoutePolicySpec, WireSqlObs, run_with_ready,
};
use blastit_events::{Event, EventKind, encode_json};
use blastit_sqlpolicy::TableGrants;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// A value no log line, event or observation may ever carry.
const SENTINEL: &str = "s3cr3t";

/// PostgreSQL protocol version 3.0.
const PROTOCOL_VERSION: u32 = 196_608;

//
// MockPgServer: a PostgreSQL good enough for pregresql's upstream client.
//

/// What one upstream connection's startup packet asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupRequest {
    /// `user` parameter.
    user: String,
    /// `database` parameter.
    database: String,
}

/// A PostgreSQL server on `127.0.0.1:0`.
///
/// It speaks exactly the wire pregresql's upstream client needs — trust
/// authentication, three `ParameterStatus` frames, a backend key, and then the
/// simple and extended protocol acknowledgements — and records what it was
/// asked, so a test can prove a statement reached (or never reached) the
/// database.
struct MockPgServer {
    port: u16,
    startups: Arc<Mutex<Vec<StartupRequest>>>,
    statements: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl MockPgServer {
    /// Binds `127.0.0.1:0` and serves in the background.
    async fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("the mock server binds a loopback port");
        let port = listener
            .local_addr()
            .expect("the mock server has an address")
            .port();
        let startups = Arc::new(Mutex::new(Vec::new()));
        let statements = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn({
            let startups = Arc::clone(&startups);
            let statements = Arc::clone(&statements);
            async move {
                loop {
                    let Ok((stream, _peer)) = listener.accept().await else {
                        return;
                    };
                    let startups = Arc::clone(&startups);
                    let statements = Arc::clone(&statements);
                    tokio::spawn(async move {
                        let _ = serve_upstream(stream, startups, statements).await;
                    });
                }
            }
        });
        Self {
            port,
            startups,
            statements,
            task,
        }
    }

    /// Every startup packet the server received, in order.
    fn startups(&self) -> Vec<StartupRequest> {
        self.startups.lock().expect("the startup log").clone()
    }

    /// Every statement the server was sent, in order.
    fn statements(&self) -> Vec<String> {
        self.statements.lock().expect("the statement log").clone()
    }
}

impl Drop for MockPgServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Serves one upstream connection.
async fn serve_upstream(
    mut stream: tokio::net::TcpStream,
    startups: Arc<Mutex<Vec<StartupRequest>>>,
    statements: Arc<Mutex<Vec<String>>>,
) -> std::io::Result<()> {
    // A reachability probe connects and closes without a word: nothing to do.
    let mut length = [0u8; 4];
    let mut read = 0usize;
    while read < length.len() {
        match stream.read(&mut length[read..]).await? {
            0 if read == 0 => return Ok(()),
            0 => return Ok(()),
            n => read += n,
        }
    }
    let length = u32::from_be_bytes(length) as usize;
    if !(4..=16 * 1024).contains(&length) {
        return Ok(());
    }
    let mut payload = vec![0u8; length - 4];
    stream.read_exact(&mut payload).await?;
    let protocol = u32::from_be_bytes(
        payload[..4]
            .try_into()
            .expect("the startup packet has a protocol version"),
    );
    if protocol != PROTOCOL_VERSION {
        return Ok(());
    }
    let mut parameters: HashMap<String, String> = HashMap::new();
    let mut fields = payload[4..].split(|byte| *byte == 0);
    while let Some(key) = fields.next() {
        if key.is_empty() {
            break;
        }
        let Some(value) = fields.next() else {
            break;
        };
        parameters.insert(
            String::from_utf8_lossy(key).into_owned(),
            String::from_utf8_lossy(value).into_owned(),
        );
    }
    startups
        .lock()
        .expect("the startup log")
        .push(StartupRequest {
            user: parameters.get("user").cloned().unwrap_or_default(),
            database: parameters.get("database").cloned().unwrap_or_default(),
        });

    // Trust authentication: a view proves its identity with its socket, and the
    // upstream is dbproxy's own session.
    stream.write_all(&frame(b'R', &0i32.to_be_bytes())).await?;
    for (name, value) in [
        ("client_encoding", "UTF8"),
        ("standard_conforming_strings", "on"),
        ("server_version", "16.15"),
    ] {
        let mut body = Vec::new();
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        body.push(0);
        stream.write_all(&frame(b'S', &body)).await?;
    }
    let mut key = Vec::new();
    key.extend_from_slice(&1i32.to_be_bytes());
    key.extend_from_slice(&2i32.to_be_bytes());
    stream.write_all(&frame(b'K', &key)).await?;
    stream.write_all(&frame(b'Z', b"I")).await?;

    loop {
        let mut header = [0u8; 5];
        if stream.read_exact(&mut header).await.is_err() {
            return Ok(());
        }
        let tag = header[0];
        let length = u32::from_be_bytes(
            header[1..]
                .try_into()
                .expect("a message header carries four length bytes"),
        ) as usize;
        if !(4..=1_000_000).contains(&length) {
            return Ok(());
        }
        let mut body = vec![0u8; length - 4];
        stream.read_exact(&mut body).await?;
        match tag {
            b'Q' => {
                let sql = String::from_utf8_lossy(trim_nul(&body)).into_owned();
                statements.lock().expect("the statement log").push(sql);
                stream.write_all(&frame(b'C', b"SELECT 0\0")).await?;
                stream.write_all(&frame(b'Z', b"I")).await?;
            }
            // Parse carries the statement text; Bind, Describe and Execute are
            // acknowledgements of a statement the Parse already reported.
            b'P' => {
                let sql = parse_text(&body);
                statements.lock().expect("the statement log").push(sql);
                stream.write_all(&frame(b'1', &[])).await?;
            }
            b'B' => stream.write_all(&frame(b'2', &[])).await?,
            b'D' => stream.write_all(&frame(b'n', &[])).await?,
            b'E' => stream.write_all(&frame(b'C', b"SELECT 0\0")).await?,
            b'C' => stream.write_all(&frame(b'3', &[])).await?,
            b'S' => stream.write_all(&frame(b'Z', b"I")).await?,
            b'H' => {}
            b'X' => return Ok(()),
            _ => {}
        }
    }
}

/// The text of a `Parse` message: `name\0sql\0types`.
fn parse_text(body: &[u8]) -> String {
    let mut parts = body.split(|byte| *byte == 0);
    let _name = parts.next();
    let sql = parts.next().unwrap_or_default();
    String::from_utf8_lossy(sql).into_owned()
}

/// `body` without its trailing NUL.
fn trim_nul(body: &[u8]) -> &[u8] {
    match body.last() {
        Some(0) => &body[..body.len() - 1],
        _ => body,
    }
}

/// One protocol frame.
fn frame(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 5);
    out.push(tag);
    out.extend_from_slice(
        &u32::try_from(body.len() + 4)
            .expect("a frame body fits in u32")
            .to_be_bytes(),
    );
    out.extend_from_slice(body);
    out
}

//
// A raw client, so the test sees protocol frames rather than a driver's
// interpretation of them.
//

/// One frame the proxy sent the client.
#[derive(Debug, Clone)]
struct Frame {
    tag: u8,
    body: Vec<u8>,
}

impl Frame {
    /// The `C` (SQLSTATE) field of an `ErrorResponse`.
    fn sqlstate(&self) -> Option<String> {
        self.error_field(b'C')
    }

    /// The `M` (message) field of an `ErrorResponse`.
    fn message(&self) -> Option<String> {
        self.error_field(b'M')
    }

    /// One field of an `ErrorResponse` body.
    fn error_field(&self, wanted: u8) -> Option<String> {
        if self.tag != b'E' {
            return None;
        }
        let mut rest = self.body.as_slice();
        while let Some((&code, tail)) = rest.split_first() {
            if code == 0 {
                return None;
            }
            let end = tail.iter().position(|byte| *byte == 0)?;
            if code == wanted {
                return Some(String::from_utf8_lossy(&tail[..end]).into_owned());
            }
            rest = &tail[end + 1..];
        }
        None
    }
}

/// A client that speaks the protocol by hand.
struct RawClient {
    stream: tokio::net::UnixStream,
    buffer: Vec<u8>,
}

impl RawClient {
    /// Connects to `<directory>/.s.PGSQL.<port>` and completes the startup.
    async fn connect(directory: &Path, port: u16) -> Self {
        let path = directory.join(format!(".s.PGSQL.{port}"));
        let std_stream = StdUnixStream::connect(&path).expect("the client connects");
        std_stream
            .set_nonblocking(true)
            .expect("the client is non-blocking");
        let mut stream =
            tokio::net::UnixStream::from_std(std_stream).expect("the client stream converts");
        let mut startup = Vec::new();
        startup.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
        for (name, value) in [
            ("user", "blastit"),
            ("database", "app"),
            ("application_name", "critical"),
        ] {
            startup.extend_from_slice(name.as_bytes());
            startup.push(0);
            startup.extend_from_slice(value.as_bytes());
            startup.push(0);
        }
        startup.push(0);
        let mut packet = Vec::new();
        packet.extend_from_slice(
            &u32::try_from(startup.len() + 4)
                .expect("the startup packet fits in u32")
                .to_be_bytes(),
        );
        packet.extend_from_slice(&startup);
        stream
            .write_all(&packet)
            .await
            .expect("the startup packet is written");

        let mut client = Self {
            stream,
            buffer: Vec::new(),
        };
        let frames = client.read_until_ready().await;
        assert!(
            frames
                .iter()
                .any(|frame| frame.tag == b'R' && frame.body == 0i32.to_be_bytes()),
            "the session is trusted: {frames:?}"
        );
        client
    }

    /// Sends one simple query and returns the frames up to `ReadyForQuery`.
    async fn query(&mut self, sql: &str) -> Vec<Frame> {
        let mut body = Vec::with_capacity(sql.len() + 1);
        body.extend_from_slice(sql.as_bytes());
        body.push(0);
        self.stream
            .write_all(&frame(b'Q', &body))
            .await
            .expect("the query is written");
        self.read_until_ready().await
    }

    /// Reads frames until a `ReadyForQuery` (`Z`), or end of stream.
    async fn read_until_ready(&mut self) -> Vec<Frame> {
        let mut frames = Vec::new();
        loop {
            let Some(frame) = self.read_frame().await else {
                return frames;
            };
            let ready = frame.tag == b'Z';
            frames.push(frame);
            if ready {
                return frames;
            }
        }
    }

    /// Reads one frame, or `None` at end of stream.
    async fn read_frame(&mut self) -> Option<Frame> {
        while self.buffer.len() < 5 {
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return None,
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
            }
        }
        let tag = self.buffer[0];
        let length = u32::from_be_bytes(self.buffer[1..5].try_into().expect("four bytes")) as usize;
        while self.buffer.len() < 1 + length {
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return None,
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
            }
        }
        let body = self.buffer[5..1 + length].to_vec();
        self.buffer.drain(..1 + length);
        Some(Frame { tag, body })
    }
}

//
// The proxy and its two internal channels.
//

/// The uid this test runs as: what the kernel stamps on `SCM_CREDENTIALS`.
fn current_uid() -> u32 {
    std::fs::metadata("/proc/self")
        .expect("/proc/self is readable")
        .uid()
}

/// A unique, short scratch directory (Unix socket paths are limited).
fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("blastit-learn-{}-{name}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

/// Binds `.s.PGSQL.<port>` with `SO_PASSCRED` before any client can connect.
fn bind(directory: &Path, port: u16) -> StdUnixListener {
    let listener = StdUnixListener::bind(directory.join(format!(".s.PGSQL.{port}")))
        .expect("the listener binds");
    pregresql::proxy::stream::set_passcred(&listener).expect("SO_PASSCRED");
    listener
}

/// A running dbproxy, its events, and the client ends of its two channels.
struct Running {
    directory: PathBuf,
    port: u16,
    events: Arc<Mutex<Vec<Event>>>,
    obs: BufReader<tokio::net::UnixStream>,
    obs_bytes: Vec<u8>,
    ctl: tokio::io::BufReader<tokio::net::UnixStream>,
    task: tokio::task::JoinHandle<Result<(), DbproxyError>>,
}

impl Running {
    /// The next observation line, or `None` when none arrived in `timeout`.
    ///
    /// The bytes are kept so a test can assert that no literal appears in the
    /// stream at all, not merely in the parsed fields.
    async fn observe(&mut self, timeout: Duration) -> Option<WireSqlObs> {
        let mut line = String::new();
        match tokio::time::timeout(timeout, self.obs.read_line(&mut line)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => None,
            Ok(Ok(_)) => {
                self.obs_bytes.extend_from_slice(line.as_bytes());
                Some(serde_json::from_str(&line).expect("the observation is a WireSqlObs"))
            }
        }
    }

    /// The next observation line, which must arrive.
    async fn observation(&mut self) -> WireSqlObs {
        self.observe(Duration::from_secs(2))
            .await
            .expect("an observation line")
    }

    /// Every event so far.
    fn events(&self) -> Vec<Event> {
        self.events.lock().expect("the event log").clone()
    }

    /// Sends one control command and returns the parsed reply.
    async fn control(&mut self, command: &str) -> serde_json::Value {
        self.ctl
            .write_all(format!("{command}\n").as_bytes())
            .await
            .expect("the command is written");
        self.control_reply().await
    }

    /// Reads one control reply line.
    async fn control_reply(&mut self) -> serde_json::Value {
        let mut line = String::new();
        let read =
            tokio::time::timeout(Duration::from_secs(2), self.ctl.read_line(&mut line)).await;
        match read {
            Ok(Ok(0)) => panic!("the control channel closed without a reply"),
            Ok(Ok(_)) => serde_json::from_str(line.trim()).expect("the reply is JSON"),
            Ok(Err(error)) => panic!("the control reply failed: {error}"),
            Err(_) => panic!("no control reply within 2 s"),
        }
    }

    /// Waits for the control channel to close, returning how long it took.
    async fn control_eof(&mut self, timeout: Duration) -> Option<Duration> {
        let started = Instant::now();
        let mut chunk = [0u8; 256];
        let read = tokio::time::timeout(timeout, async {
            loop {
                match self.ctl.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
            }
        })
        .await;
        match read {
            Ok(()) => Some(started.elapsed()),
            Err(_) => None,
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// A proxy specification pointing at the mock server.
fn proxy_spec(server: &MockPgServer, database: &str) -> ProxySpec {
    ProxySpec {
        name: "db".to_string(),
        host: "127.0.0.1".to_string(),
        port: server.port,
        user: "real-user".to_string(),
        password: "real-password".to_string().into(),
        database: database.to_string(),
        sslmode: Some("disable".to_string()),
        ca_cert: None,
        options: Vec::new(),
        max_connections: 4,
    }
}

/// A listener spec for `route`, with `fd` filled in by [`start`].
fn listener_spec(route: &str, uid: u32) -> ListenerSpec {
    ListenerSpec {
        fd: -1,
        route: route.to_string(),
        proxy: "db".to_string(),
        uid,
        queries: Vec::new(),
        transaction_control: false,
        unrestricted: false,
        tables: TableGrants::default(),
    }
}

/// Starts dbproxy over one listener, with the two internal channels.
///
/// `config` carries everything but the listener and the channel descriptors;
/// the tests fill in the rest, exactly as the orchestrator does.
async fn start(name: &str, mut config: DbproxyConfig, route: &str) -> Running {
    let directory = scratch(name);
    let port = 5432;
    let listener = bind(&directory, port);
    let fd = listener.into_raw_fd();

    let (obs_ours, obs_theirs) = StdUnixStream::pair().expect("an obs channel");
    let (ctl_ours, ctl_theirs) = StdUnixStream::pair().expect("a control channel");
    obs_ours
        .set_nonblocking(true)
        .expect("the obs channel is non-blocking");
    ctl_ours
        .set_nonblocking(true)
        .expect("the control channel is non-blocking");
    let obs_fd = obs_theirs.into_raw_fd();
    let ctl_fd = ctl_theirs.into_raw_fd();

    config.obs_fd = Some(obs_fd);
    config.check_ctl_fd = Some(ctl_fd);
    config.listeners = vec![ListenerSpec {
        fd,
        ..listener_spec(route, current_uid())
    }];

    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emit: Emit = Arc::new(move |event| {
        sink.lock().expect("the event log").push(event);
    });
    let (ready_tx, ready_rx) = oneshot::channel::<()>();
    let served = config.clone();
    let task = tokio::spawn(async move {
        let ready = move || {
            let _ = ready_tx.send(());
        };
        run_with_ready(served, emit, ready).await
    });
    tokio::time::timeout(Duration::from_secs(5), ready_rx)
        .await
        .expect("dbproxy becomes ready")
        .expect("the ready signal");

    Running {
        directory,
        port,
        events,
        obs: BufReader::new(
            tokio::net::UnixStream::from_std(obs_ours).expect("the obs channel is a stream"),
        ),
        obs_bytes: Vec::new(),
        ctl: tokio::io::BufReader::new(
            tokio::net::UnixStream::from_std(ctl_ours).expect("the control channel is a stream"),
        ),
        task,
    }
}

/// The dynamic-check configuration: one wildcard listener per proxy, and the
/// route grants `Manifest::effective_grants` would have produced.
fn dynamic_check(server: &MockPgServer, policies: Vec<RoutePolicySpec>) -> DbproxyConfig {
    DbproxyConfig {
        proxies: vec![proxy_spec(server, "app")],
        mode: ListenerMode::DynamicCheck,
        route_policies: policies,
        ..DbproxyConfig::default()
    }
}

/// The policy of a route that grants one statement.
fn policy(route: &str, query: &str) -> RoutePolicySpec {
    RoutePolicySpec {
        route: route.to_string(),
        proxy: "db".to_string(),
        queries: vec![query.to_string()],
        ..RoutePolicySpec::default()
    }
}

//
// Positive twins.
//

#[tokio::test]
async fn learning_ignores_fixture_sql_and_replays_setup_once_on_the_first_route() {
    let server = MockPgServer::start().await;
    let mut proxy = start(
        "learn",
        dynamic_check(&server, vec![policy("product-detail", "SELECT 1")]),
        "*",
    )
    .await;
    let mut client = RawClient::connect(&proxy.directory, proxy.port).await;

    // Fixture SQL: a request is not in flight, and an `INSERT` is not session
    // setup, so it is never observed.
    let frames = client
        .query("INSERT INTO fixture_rows (id) VALUES (1)")
        .await;
    assert!(
        !frames.iter().any(|frame| frame.tag == b'E'),
        "DynamicCheck must not refuse fixture SQL: {frames:?}"
    );
    // Driver setup: recorded, but not yet attributed.
    for setup in ["SET TIME ZONE 'UTC'", "select pg_catalog.version()"] {
        let frames = client.query(setup).await;
        assert!(
            !frames.iter().any(|frame| frame.tag == b'E'),
            "DynamicCheck must not refuse {setup:?}: {frames:?}"
        );
    }
    assert!(
        proxy.observe(Duration::from_millis(200)).await.is_none(),
        "nothing under an empty target stack is observed"
    );
    assert_eq!(
        server.statements().len(),
        3,
        "every statement ran: the mock saw them"
    );

    // The first routed request: the held setup is replayed against it, before
    // the statement that caused the replay.
    let reply = proxy
        .control(r#"{"op":"push_routed","route":"product-detail"}"#)
        .await;
    assert_eq!(reply["ok"], serde_json::json!(true), "{reply}");

    // A statement no grant lists, with a literal a log must never carry.
    let frames = client
        .query("SELECT 's3cr3t' FROM orders WHERE id = 1")
        .await;
    assert!(
        !frames.iter().any(|frame| frame.tag == b'E'),
        "DynamicCheck returns Allow for an ungranted statement: {frames:?}"
    );

    let mut observed = Vec::new();
    for _ in 0..3 {
        observed.push(proxy.observation().await);
    }
    assert_eq!(
        observed
            .iter()
            .map(|obs| obs.sql.as_str())
            .collect::<Vec<_>>(),
        [
            "SET TIME ZONE 'UTC'",
            "select pg_catalog.version()",
            "SELECT … FROM orders WHERE id = 1",
        ],
        "the two setup statements are replayed, then the statement itself"
    );
    for obs in &observed {
        assert_eq!(
            obs.target,
            ObsTarget::Routed {
                route: "product-detail".to_string()
            },
            "every replayed observation belongs to the route that caused it"
        );
        assert_eq!(obs.proxy, "db");
        assert!(!obs.allowed, "the route grants none of these statements");
        assert!(
            matches!(obs.kind, blastit_events::SqlKind::Query),
            "{obs:?}"
        );
    }
    assert_eq!(
        observed[0].verdict.normalized(),
        Some("SET TIME ZONE 'UTC'"),
        "a protocol constant is grantable as it stands"
    );
    assert!(
        !observed[2].verdict.is_query(),
        "an inline literal cannot be granted: {:?}",
        observed[2].verdict
    );
    assert!(
        observed[2]
            .verdict
            .reason()
            .is_some_and(|reason| reason.contains("literal")),
        "{:?}",
        observed[2].verdict
    );
    assert_eq!(
        observed[1].verdict.normalized(),
        Some("select pg_catalog.version()")
    );

    // The replay is one-time: a second routed request does not repeat the setup.
    proxy.control(r#"{"op":"pop"}"#).await;
    proxy
        .control(r#"{"op":"push_routed","route":"product-detail"}"#)
        .await;
    let frames = client.query("SELECT 2").await;
    assert!(!frames.iter().any(|frame| frame.tag == b'E'), "{frames:?}");
    let second = proxy.observation().await;
    assert_eq!(second.sql, "SELECT 2");
    assert!(
        proxy.observe(Duration::from_millis(200)).await.is_none(),
        "the setup statements are not replayed again"
    );

    // No byte of the observation stream carries the literal.
    assert!(
        !String::from_utf8_lossy(&proxy.obs_bytes).contains(SENTINEL),
        "the obs_fd stream leaked a literal"
    );
    assert!(
        proxy
            .events()
            .iter()
            .all(|event| !matches!(event.kind(), EventKind::SqlDenied(_))),
        "a learning listener never emits sql.denied: {:#?}",
        proxy.events()
    );
}

#[tokio::test]
async fn an_unrouted_request_carries_its_own_target() {
    let server = MockPgServer::start().await;
    let mut proxy = start("unrouted", dynamic_check(&server, Vec::new()), "*").await;
    let mut client = RawClient::connect(&proxy.directory, proxy.port).await;

    let reply = proxy
        .control(r#"{"op":"push_unrouted","method":"GET","path":"/orders/42/"}"#)
        .await;
    assert_eq!(reply["ok"], serde_json::json!(true), "{reply}");
    client.query("SELECT 's3cr3t' FROM orders").await;

    let observed = proxy.observation().await;
    assert_eq!(
        observed.target,
        ObsTarget::Unrouted {
            method: "GET".to_string(),
            path: "/orders/42/".to_string()
        }
    );
    assert_eq!(observed.sql, "SELECT … FROM orders");
    assert!(observed.verdict.redacted().is_some());
    assert!(!String::from_utf8_lossy(&proxy.obs_bytes).contains(SENTINEL));
}

#[tokio::test]
async fn set_db_switches_the_database_the_next_session_requests() {
    let server = MockPgServer::start().await;
    let mut proxy = start("set-db", dynamic_check(&server, Vec::new()), "*").await;

    let mut first = RawClient::connect(&proxy.directory, proxy.port).await;
    first.query("SELECT 1").await;
    assert_eq!(
        server.startups(),
        [StartupRequest {
            user: "real-user".to_string(),
            database: "app".to_string()
        }],
        "a session starts on the proxy's configured database"
    );

    let reply = proxy
        .control(r#"{"op":"set_db","proxy":"db","database":"test_shop"}"#)
        .await;
    assert_eq!(reply["ok"], serde_json::json!(true), "{reply}");

    // The database is chosen when a session is set up, so a new connection sees
    // it and the open one keeps what it opened with.
    drop(first);
    let mut second = RawClient::connect(&proxy.directory, proxy.port).await;
    second.query("SELECT 2").await;
    assert_eq!(
        server.startups().last(),
        Some(&StartupRequest {
            user: "real-user".to_string(),
            database: "test_shop".to_string()
        }),
        "set_db steers the next session"
    );
    assert_eq!(server.startups().len(), 2);

    // An unknown proxy is refused, and the reply names no credential.
    let reply = proxy
        .control(r#"{"op":"set_db","proxy":"nope","database":"test_shop"}"#)
        .await;
    assert_eq!(reply["ok"], serde_json::json!(false), "{reply}");
    assert_eq!(
        reply["error"],
        serde_json::json!("unknown proxy"),
        "{reply}"
    );
    assert!(!reply.to_string().contains("real-password"));
}

//
// Negative twins.
//

#[tokio::test]
async fn enforce_still_answers_42501_and_redacts_every_literal() {
    let server = MockPgServer::start().await;
    let directory = scratch("enforce");
    let port = 5432;
    let listener = bind(&directory, port);
    let fd = listener.into_raw_fd();
    let config = DbproxyConfig {
        proxies: vec![proxy_spec(&server, "app")],
        listeners: vec![ListenerSpec {
            fd,
            ..listener_spec("product-detail", current_uid())
        }],
        ..DbproxyConfig::default()
    };
    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emit: Emit = Arc::new(move |event| {
        sink.lock().expect("the event log").push(event);
    });
    let (ready_tx, ready_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let ready = move || {
            let _ = ready_tx.send(());
        };
        run_with_ready(config, emit, ready).await
    });
    tokio::time::timeout(Duration::from_secs(5), ready_rx)
        .await
        .expect("dbproxy becomes ready")
        .expect("the ready signal");

    let mut client = RawClient::connect(&directory, port).await;
    let frames = client
        .query("SELECT 's3cr3t' FROM orders WHERE id = 1")
        .await;
    let error = frames
        .iter()
        .find(|frame| frame.tag == b'E')
        .expect("the statement is refused");
    assert_eq!(
        error.sqlstate().as_deref(),
        Some("42501"),
        "Enforce still answers insufficient_privilege: {error:?}"
    );
    assert!(
        error
            .message()
            .is_some_and(|message| message.contains("product-detail")),
        "the refusal names the route: {error:?}"
    );
    assert_eq!(
        server.statements(),
        Vec::<String>::new(),
        "the refused statement never reached the database"
    );

    let events = events.lock().expect("the event log").clone();
    let denied = events
        .iter()
        .find_map(|event| match event.kind() {
            EventKind::SqlDenied(denied) => Some(denied),
            _ => None,
        })
        .expect("a sql.denied event");
    assert_eq!(denied.sql(), "SELECT … FROM orders WHERE id = 1");
    assert_eq!(denied.sqlstate(), "42501");
    assert_eq!(denied.kind(), &blastit_events::SqlKind::Query);
    for event in &events {
        let encoded = encode_json(event);
        assert!(
            !encoded.contains(SENTINEL),
            "a sql.denied event leaked the literal: {encoded}"
        );
        assert!(
            !format!("{event:?}").contains(SENTINEL),
            "the debug form leaked the literal"
        );
    }

    task.abort();
    let _ = std::fs::remove_dir_all(&directory);
}

#[tokio::test]
async fn a_root_listener_without_the_opt_in_is_rejected() {
    let server = MockPgServer::start().await;
    let directory = scratch("root-listener");
    let listener = bind(&directory, 5432);
    let fd = listener.into_raw_fd();
    let config = DbproxyConfig {
        proxies: vec![proxy_spec(&server, "app")],
        listeners: vec![ListenerSpec {
            fd,
            ..listener_spec("product-detail", 0)
        }],
        ..DbproxyConfig::default()
    };
    let emit: Emit = Arc::new(|_: Event| {});
    let outcome = tokio::time::timeout(Duration::from_secs(5), run_with_ready(config, emit, || {}))
        .await
        .expect("dbproxy refuses instead of hanging");
    let error = match outcome {
        Ok(_) => panic!("a root listener must be refused"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("uid 0"), "{error}");
    assert!(
        !error.to_string().contains("real-password"),
        "the refusal names no credential: {error}"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[tokio::test]
async fn a_malformed_control_line_is_answered_and_closed() {
    let server = MockPgServer::start().await;
    let mut proxy = start("ctl-malformed", dynamic_check(&server, Vec::new()), "*").await;
    proxy
        .ctl
        .write_all(b"not json at all\n")
        .await
        .expect("the malformed line is written");
    let reply = proxy.control_reply().await;
    assert_eq!(reply["ok"], serde_json::json!(false), "{reply}");
    assert_eq!(
        reply["error"],
        serde_json::json!("malformed control command"),
        "{reply}"
    );
    assert!(
        proxy.control_eof(Duration::from_secs(2)).await.is_some(),
        "a peer that cannot speak the protocol is disconnected"
    );
}

#[tokio::test]
async fn an_oversized_control_line_is_refused_without_buffering_it() {
    let server = MockPgServer::start().await;
    let mut proxy = start("ctl-oversized", dynamic_check(&server, Vec::new()), "*").await;
    let oversized = vec![b'x'; 8 * 1024];
    proxy
        .ctl
        .write_all(&oversized)
        .await
        .expect("the oversized line is written");
    let reply = proxy.control_reply().await;
    assert_eq!(reply["ok"], serde_json::json!(false), "{reply}");
    assert!(
        reply["error"]
            .as_str()
            .is_some_and(|error| error.contains("4096")),
        "{reply}"
    );
    assert!(
        proxy.control_eof(Duration::from_secs(2)).await.is_some(),
        "an oversized line closes the channel"
    );
}

#[tokio::test]
async fn a_stalled_control_line_times_out_instead_of_hanging() {
    let server = MockPgServer::start().await;
    let mut proxy = start("ctl-stalled", dynamic_check(&server, Vec::new()), "*").await;
    proxy
        .ctl
        .write_all(b"{\"op\":")
        .await
        .expect("a partial line is written");

    // The 5 s deadline cuts the peer off; the test allows a little more for the
    // scheduler to notice, and refuses to wait longer than that.
    let elapsed = proxy
        .control_eof(Duration::from_secs(9))
        .await
        .expect("the stalled control channel is cut off");
    assert!(
        elapsed >= Duration::from_secs(4),
        "the deadline is the 5 s one, not an immediate close: {elapsed:?}"
    );
    assert!(
        elapsed <= Duration::from_secs(8),
        "a stalled line must not hold the channel for ever: {elapsed:?}"
    );
}
