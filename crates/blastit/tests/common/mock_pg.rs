//! `MockPgServer`: a PostgreSQL good enough for `blastit`'s upstream client.
//!
//! It binds `127.0.0.1:0` (the caller reads [`MockPgServer::port`]), accepts
//! trust authentication, answers the simple and extended protocol with a
//! canned `SELECT 0`, and records every statement it was sent. The critical
//! `blastit dev` suite uses it so one pass — dbproxy, the per-route socket, a
//! view's real Python client — needs no PostgreSQL binary, no Docker and no
//! root.
//!
//! It is a plain `std` thread rather than a tokio task because the suite drives
//! a real `blastit` child process and reads its output synchronously.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// PostgreSQL protocol version 3.0.
const PROTOCOL_VERSION: u32 = 196_608;

/// An SSLRequest (`80877103`), which the server answers with `N`.
const SSL_REQUEST: u32 = 80_877_103;

/// A PostgreSQL server on loopback that records what it was sent.
pub struct MockPgServer {
    port: u16,
    startups: Arc<Mutex<Vec<StartupRequest>>>,
    statements: Arc<Mutex<Vec<String>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// What one startup packet asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupRequest {
    /// The `user` parameter.
    pub user: String,
    /// The `database` parameter.
    pub database: String,
}

impl MockPgServer {
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
        let startups = Arc::new(Mutex::new(Vec::new()));
        let statements = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let handle = thread::spawn({
            let startups = Arc::clone(&startups);
            let statements = Arc::clone(&statements);
            let shutdown = Arc::clone(&shutdown);
            move || {
                while !shutdown.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _peer)) => {
                            let startups = Arc::clone(&startups);
                            let statements = Arc::clone(&statements);
                            thread::spawn(move || {
                                let _ = serve(stream, startups, statements);
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
            startups,
            statements,
            shutdown,
            handle: Some(handle),
        }
    }

    /// The loopback port the server listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Every startup packet received, in order.
    pub fn startups(&self) -> Vec<StartupRequest> {
        self.startups.lock().expect("the startup log").clone()
    }

    /// Every statement received, in order.
    pub fn statements(&self) -> Vec<String> {
        self.statements.lock().expect("the statement log").clone()
    }
}

impl Drop for MockPgServer {
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
fn serve(
    mut stream: TcpStream,
    startups: Arc<Mutex<Vec<StartupRequest>>>,
    statements: Arc<Mutex<Vec<String>>>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    loop {
        let Some((protocol, payload)) = read_startup(&mut stream)? else {
            return Ok(());
        };
        if protocol == SSL_REQUEST {
            stream.write_all(b"N")?;
            continue;
        }
        if protocol != PROTOCOL_VERSION {
            return Ok(());
        }
        let parameters = startup_parameters(&payload);
        startups
            .lock()
            .expect("the startup log")
            .push(StartupRequest {
                user: parameters.get("user").cloned().unwrap_or_default(),
                database: parameters.get("database").cloned().unwrap_or_default(),
            });
        // Trust authentication: the view proves its identity with the socket,
        // and the upstream session is dbproxy's own.
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
        serve_messages(&mut stream, &statements)?;
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

/// The message phase: the simple query and the extended protocol, acknowledged.
fn serve_messages(stream: &mut TcpStream, statements: &Arc<Mutex<Vec<String>>>) -> io::Result<()> {
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
            b'Q' => {
                statements
                    .lock()
                    .expect("the statement log")
                    .push(String::from_utf8_lossy(trim_nul(&body)).into_owned());
                stream.write_all(&frame(b'C', b"SELECT 0\0"))?;
                stream.write_all(&frame(b'Z', b"I"))?;
            }
            // `Parse` carries the statement text; `Bind`, `Describe` and
            // `Execute` acknowledge what the `Parse` already reported.
            b'P' => {
                let mut parts = body.split(|byte| *byte == 0);
                let _name = parts.next();
                let sql = parts.next().unwrap_or_default();
                statements
                    .lock()
                    .expect("the statement log")
                    .push(String::from_utf8_lossy(sql).into_owned());
                stream.write_all(&frame(b'1', &[]))?;
            }
            b'B' => stream.write_all(&frame(b'2', &[]))?,
            b'D' => stream.write_all(&frame(b'n', &[]))?,
            b'E' => stream.write_all(&frame(b'C', b"SELECT 0\0"))?,
            b'C' => stream.write_all(&frame(b'3', &[]))?,
            b'S' => stream.write_all(&frame(b'Z', b"I"))?,
            b'H' => {}
            b'X' => return Ok(()),
            _ => {}
        }
    }
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
