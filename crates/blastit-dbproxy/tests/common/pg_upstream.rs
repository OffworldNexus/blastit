//! `PgUpstream`: a real PostgreSQL v3 peer on loopback, shared by the critical
//! suites.
//!
//! The dbproxy, its per-route sockets, the policy and the wire framing under
//! test are the real crates; this module is the *external* peer they dial, so
//! it is a real [`std::net::TcpListener`] bound to `127.0.0.1:0` speaking the
//! PostgreSQL v3 subset those suites need:
//!
//! * a startup packet (`user`, `database` and any other parameter) answered
//!   with `AuthenticationOk`, three `ParameterStatus` frames, `BackendKeyData`
//!   and `ReadyForQuery`;
//! * one `RowDescription`/`DataRow`/`CommandComplete` answer per statement,
//!   then `ReadyForQuery`, for the simple protocol and for the `Parse`/`Bind`/
//!   `Describe`/`Execute` sequence;
//! * the startup packets and the statement texts it received, recorded so a
//!   suite can prove a statement reached — or never reached — the database.
//!
//! No PostgreSQL binary, no Docker and no root are needed, and the port is
//! ephemeral. It is a plain `std` thread rather than a tokio task because one of
//! its two suites (`blastit`'s `dev`) drives a real child process and reads its
//! output synchronously.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// PostgreSQL protocol version 3.0.
const PROTOCOL_VERSION: u32 = 196_608;

/// An `SSLRequest` (`80877103`), which the upstream answers with `N`.
const SSL_REQUEST: u32 = 80_877_103;

/// PostgreSQL's `text` type oid: the type of the one column every canned
/// answer describes.
const TEXT_OID: i32 = 25;

/// A PostgreSQL peer on loopback that records what it was sent.
pub struct PgUpstream {
    port: u16,
    recorded: Arc<Recorded>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// What one startup packet asked for.
///
/// Each suite reads the parameters it asserts on: `blastit`'s `dev` suite checks
/// the user dbproxy authenticated with, `blastit-dbproxy`'s suite also checks
/// the database `set_db` steers the next session to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct StartupRequest {
    /// The `user` parameter.
    pub user: String,
    /// The `database` parameter.
    pub database: String,
}

/// The startup packets and statements this upstream received, shared with its
/// connection threads.
#[derive(Default)]
struct Recorded {
    startups: Mutex<Vec<StartupRequest>>,
    statements: Mutex<Vec<String>>,
}

impl Recorded {
    fn startup(&self, startup: StartupRequest) {
        self.startups.lock().expect("the startup log").push(startup);
    }

    fn statement(&self, sql: String) {
        self.statements.lock().expect("the statement log").push(sql);
    }

    fn startups(&self) -> Vec<StartupRequest> {
        self.startups.lock().expect("the startup log").clone()
    }

    fn statements(&self) -> Vec<String> {
        self.statements.lock().expect("the statement log").clone()
    }
}

impl PgUpstream {
    /// Binds `127.0.0.1:0` and serves in the background.
    ///
    /// # Panics
    ///
    /// When the loopback listener cannot be bound, which is a broken host
    /// rather than a test failure to recover from.
    pub fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a loopback listener");
        listener
            .set_nonblocking(true)
            .expect("a non-blocking listener");
        let port = listener.local_addr().expect("a bound address").port();
        let recorded = Arc::new(Recorded::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let handle = thread::spawn({
            let recorded = Arc::clone(&recorded);
            let shutdown = Arc::clone(&shutdown);
            move || {
                while !shutdown.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _peer)) => {
                            let recorded = Arc::clone(&recorded);
                            thread::spawn(move || {
                                let _ = serve(stream, &recorded);
                            });
                        }
                        Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => return,
                    }
                }
            }
        });
        Self {
            port,
            recorded,
            shutdown,
            handle: Some(handle),
        }
    }

    /// The loopback port the upstream listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Every startup packet received, in order.
    pub fn startups(&self) -> Vec<StartupRequest> {
        self.recorded.startups()
    }

    /// Every statement received, in order.
    pub fn statements(&self) -> Vec<String> {
        self.recorded.statements()
    }
}

impl Drop for PgUpstream {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Wake the accept loop, which is sleeping between non-blocking polls.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Serves one upstream connection.
fn serve(mut stream: TcpStream, recorded: &Recorded) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    loop {
        let Some((protocol, payload)) = read_startup(&mut stream)? else {
            // A reachability probe connects and closes without a word.
            return Ok(());
        };
        if protocol == SSL_REQUEST {
            // Loopback needs no TLS: the client proceeds in cleartext.
            stream.write_all(b"N")?;
            continue;
        }
        if protocol != PROTOCOL_VERSION {
            return Ok(());
        }
        let parameters = startup_parameters(&payload);
        recorded.startup(StartupRequest {
            user: parameters.get("user").cloned().unwrap_or_default(),
            database: parameters.get("database").cloned().unwrap_or_default(),
        });
        // Trust authentication: a view proves its identity with its socket, and
        // the upstream session is the proxy's own.
        stream.write_all(&frame(b'R', &0i32.to_be_bytes()))?;
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
            stream.write_all(&frame(b'S', &body))?;
        }
        let mut key = Vec::new();
        key.extend_from_slice(&1i32.to_be_bytes());
        key.extend_from_slice(&2i32.to_be_bytes());
        stream.write_all(&frame(b'K', &key))?;
        stream.write_all(&frame(b'Z', b"I"))?;
        serve_messages(&mut stream, recorded)?;
        return Ok(());
    }
}

/// One startup packet: its protocol word and the remainder.
fn read_startup(stream: &mut TcpStream) -> io::Result<Option<(u32, Vec<u8>)>> {
    let mut length = [0u8; 4];
    match stream.read_exact(&mut length) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }
    let length = u32::from_be_bytes(length) as usize;
    if !(4..=16 * 1024).contains(&length) {
        return Ok(None);
    }
    let mut payload = vec![0u8; length - 4];
    stream.read_exact(&mut payload)?;
    let protocol = u32::from_be_bytes(
        payload[..4]
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "a short startup packet"))?,
    );
    Ok(Some((protocol, payload[4..].to_vec())))
}

/// The startup packet's parameter pairs.
fn startup_parameters(payload: &[u8]) -> HashMap<String, String> {
    let mut parameters = HashMap::new();
    let mut fields = payload.split(|byte| *byte == 0);
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
    parameters
}

/// The message phase: each statement answered, then the session is ready.
fn serve_messages(stream: &mut TcpStream, recorded: &Recorded) -> io::Result<()> {
    loop {
        let mut header = [0u8; 5];
        if stream.read_exact(&mut header).is_err() {
            return Ok(());
        }
        let tag = header[0];
        let length = u32::from_be_bytes(
            header[1..]
                .try_into()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "a short header"))?,
        ) as usize;
        if !(4..=1_000_000).contains(&length) {
            return Ok(());
        }
        let mut body = vec![0u8; length - 4];
        stream.read_exact(&mut body)?;
        match tag {
            // Simple query: the statement's answer, then the session is ready.
            b'Q' => {
                recorded.statement(String::from_utf8_lossy(trim_nul(&body)).into_owned());
                rows(stream)?;
                stream.write_all(&frame(b'Z', b"I"))?;
            }
            // `Parse` carries the statement text; `Bind`, `Describe` and
            // `Execute` answer the statement the `Parse` already reported.
            b'P' => {
                recorded.statement(parse_text(&body));
                stream.write_all(&frame(b'1', &[]))?;
            }
            b'B' => stream.write_all(&frame(b'2', &[]))?,
            b'D' => stream.write_all(&frame(b'T', &row_description()))?,
            b'E' => rows(stream)?,
            b'C' => stream.write_all(&frame(b'3', &[]))?,
            b'S' => stream.write_all(&frame(b'Z', b"I"))?,
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

/// One statement's rows: `RowDescription`, `DataRow`, `CommandComplete`.
fn rows(stream: &mut TcpStream) -> io::Result<()> {
    stream.write_all(&frame(b'T', &row_description()))?;
    stream.write_all(&frame(b'D', &data_row()))?;
    stream.write_all(&frame(b'C', b"SELECT 1\0"))
}

/// One `RowDescription` with the single `text` column every answer describes.
fn row_description() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1i16.to_be_bytes());
    body.extend_from_slice(b"?column?");
    body.push(0);
    body.extend_from_slice(&0i32.to_be_bytes()); // table oid: none
    body.extend_from_slice(&0i16.to_be_bytes()); // column number: none
    body.extend_from_slice(&TEXT_OID.to_be_bytes());
    body.extend_from_slice(&(-1i16).to_be_bytes()); // variable width
    body.extend_from_slice(&(-1i32).to_be_bytes()); // no type modifier
    body.extend_from_slice(&0i16.to_be_bytes()); // text format
    body
}

/// One `DataRow` carrying the single value `0` every canned `SELECT` returns.
fn data_row() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1i16.to_be_bytes());
    body.extend_from_slice(&1i32.to_be_bytes());
    body.push(b'0');
    body
}

/// `body` without its trailing NUL.
fn trim_nul(body: &[u8]) -> &[u8] {
    match body.last() {
        Some(0) => &body[..body.len() - 1],
        _ => body,
    }
}

/// One protocol frame: the upstream and the raw client both speak it.
#[allow(dead_code)] // the `blastit` suite only reads the traffic it recorded
pub fn frame(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 5);
    out.push(tag);
    let length = u32::try_from(body.len() + 4).unwrap_or(u32::MAX);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(body);
    out
}
