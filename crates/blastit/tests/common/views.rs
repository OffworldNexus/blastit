//! One loopback HTTP client, one request burst and the calibration both
//! concurrency suites read, kept once.
//!
//! `crates/blastit/tests/e2e_dev.rs` (the threaded WSGI leg) and
//! `crates/blastit/tests/critical_parallel_views.rs` (the PHP legs) measure the
//! same promise — four 200 ms requests at once against a serialized control
//! measured in the same function — so the numbers and the client live here
//! instead of in two copies that drift. No mock: every request is a real
//! `TcpStream` to a real boot, and a response only counts when its status line
//! parses.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

/// How many requests a wall-clock leg sends at once: `ADR-0015`'s promise is
/// about four.
pub const WIDTH: usize = 4;

/// How long one request may take once the port accepts connections.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// The serialized floor: four 200 ms requests, one at a time.
pub const CONTROL_FLOOR: Duration = Duration::from_millis(800);

/// How much faster than the serialized control the parallel leg must be, so a
/// fast host cannot pass on its own speed (`ADR-0015`'s measured promise is a
/// comparison, not a constant).
pub const CONTROL_MARGIN: Duration = Duration::from_millis(400);

/// One parsed response: the status and everything after the blank line.
#[derive(Debug)]
pub struct Response {
    /// The status code from the first line.
    pub status: u16,
    /// The body exactly as it arrived (these fixtures declare a length).
    pub body: String,
}

/// One `GET`/`POST`, or `None` while the boot does not answer.
///
/// A response is only an answer when its status line parses: a connection the
/// listener accepted before the boot reached `ready`, and a read that timed out
/// mid-body, both stay `None` so a wait retries instead of grading a status the
/// server never sent.
pub fn request(port: u16, method: &str, path: &str) -> Option<Response> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(REQUEST_TIMEOUT)).ok()?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    parse(&String::from_utf8_lossy(&raw))
}

/// The status and body of one raw response.
fn parse(raw: &str) -> Option<Response> {
    let (head, body) = raw.split_once("\r\n\r\n")?;
    let status = head
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(Response {
        status,
        body: body.to_owned(),
    })
}

/// The client half of a boot: only the port, so request threads need no lock on
/// the process the main thread owns.
#[derive(Clone, Copy)]
pub struct Client {
    /// The port `blastit dev` bound.
    pub port: u16,
}

impl Client {
    /// One request that must be answered.
    pub fn request(&self, method: &str, path: &str) -> Response {
        request(self.port, method, path)
            .unwrap_or_else(|| panic!("no answer to {method} {path} on 127.0.0.1:{}", self.port))
    }

    /// One `GET` that must be answered.
    pub fn get(&self, path: &str) -> Response {
        self.request("GET", path)
    }
}

/// Asserts a `200`, failing with what the server actually said.
pub fn assert_200(what: &str, response: &Response) {
    assert_eq!(
        response.status, 200,
        "{what}: expected a `200`, got {}:\n{}",
        response.status, response.body
    );
}

/// One `GET` per path **released together**: a barrier of `paths.len() + 1`, so
/// the clock starts before the first request can leave.
pub fn burst(client: Client, paths: &[String]) -> (Duration, Vec<Response>) {
    let barrier = Arc::new(Barrier::new(paths.len() + 1));
    let mut handles = Vec::with_capacity(paths.len());
    for path in paths {
        let barrier = Arc::clone(&barrier);
        let path = path.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            client.get(&path)
        }));
    }
    let started = Instant::now();
    barrier.wait();
    let bodies = handles
        .into_iter()
        .map(|handle| handle.join().expect("a request thread"))
        .collect();
    (started.elapsed(), bodies)
}
