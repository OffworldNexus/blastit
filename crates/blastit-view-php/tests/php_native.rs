//! The embedded PHP worker end to end, over a real `view.chan` socketpair.
//! **Ignored by default**: it needs the host's NTS `libphp` and the shim
//! `make php-sapi` built, and `make check` must stay green on a PHP-free host.
//!
//! ```console
//! $ make php-sapi
//! $ BLASTIT_LIBPHP=/usr/lib/libphp8.4.so \
//!   BLASTIT_PHP_SHIM="$PWD/target/php-sapi/libblastit_php84-<arch>-<libc>.so" \
//!   cargo test -p blastit-view-php --test php_native -- --ignored --nocapture
//! ```
//!
//! `<arch>-<libc>` is this build's tuple (`x86_64-gnu` on this host);
//! `blastit_php_sapi::Minor::Php84.shim_file()` prints the exact name.
//!
//! It is a harness, not a skip: run with `--ignored` and a missing variable, it
//! fails with that usage instead of reporting success.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{ErrorKind, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use blastit_frame::{Decoder, DoneOutcome, Frame, HEADER_LEN, Limits, ReqHead, Scheme};
use blastit_php_sapi::{Egress, IniPolicy, SharedObject};
use blastit_sys::child::{Filter, Privileges};
use blastit_view_php::{EntryPin, PhpBinding, PhpEmbedRuntime, PhpRuntimeConfig, serve_forever};

/// A required environment variable, or the usage line.
fn required(name: &str) -> PathBuf {
    std::env::var_os(name).map_or_else(
        || panic!("{name} is not set; see the module documentation for the command"),
        PathBuf::from,
    )
}

#[test]
#[ignore = "needs the host's libphp and `make php-sapi`; see the module docs"]
fn a_php_route_answers_two_requests_through_view_chan() {
    let libphp = required("BLASTIT_LIBPHP");
    let shim = required("BLASTIT_PHP_SHIM");
    let app = std::env::temp_dir().join(format!("blastit-php-view-{}", std::process::id()));
    std::fs::create_dir_all(&app).expect("app dir");
    let view_tmp = app.join("tmp");
    std::fs::create_dir_all(&view_tmp).expect("view tmp");
    let entry = app.join("index.php");
    std::fs::write(
        &entry,
        b"<?php\n// Enough work to cross a clock tick: `CLOCK_PROCESS_CPUTIME_ID`\n\
          // is tick-granular and a trivial script can measure 0 ns (MB.3.12).\n\
          for ($i = 0; $i < 2000000; $i++) { $x = $i * $i; }\n\
          header('X-Embedded: 1');\n\
          echo 'method=', $_SERVER['REQUEST_METHOD'], ';', $_SERVER['SCRIPT_FILENAME'];\n",
    )
    .expect("entry");

    let ini = IniPolicy::new(&app, &view_tmp, 30, Egress::Denied).expect("policy");
    // The production spelling: both files are pinned as descriptors and the
    // loader names them `/proc/self/fd/<n>`. One `php_module_startup` per
    // process, so this harness runs the Fd form end to end (the `Path` form is
    // `blastit-php-sapi`'s harness) and must not start a second module.
    let pin = |path: &std::path::Path| {
        blastit_sys::pin::SharedObject::open(path, None)
            .unwrap_or_else(|err| panic!("cannot open {}: {err}", path.display()))
    };
    let libphp = pin(&libphp);
    let shim = pin(&shim);
    // The binding the boot would have built for this slot (`MB.36.5`): the
    // worker's configuration is made from it, and its entry record is the value
    // every request re-checks — so this harness exercises the pin's accepted
    // path as well as the load.
    let entry_pin = EntryPin::of(&std::fs::metadata(&entry).expect("the entry's metadata"));
    let binding = PhpBinding {
        libphp,
        shim,
        entry: entry.clone(),
        declared_entry: "index.php".to_string(),
        entry_pin,
        entry_fd: OwnedFd::from(std::fs::File::open(&entry).expect("the entry")),
        document_root: app.clone(),
        ini: ini.as_str().to_owned(),
        view_tmp,
        // Two requests, then the worker retires: the second request proves
        // reuse, and the clean exit proves `max_requests`.
        max_requests: 2,
        max_body_bytes: 1024 * 1024,
        route: "app".to_string(),
        // The worker never reads the uid: the probe child and the fork do, and
        // this harness owns neither.
        uid: 0,
        script_name: String::new(),
        privileges: Privileges::Shared,
        filter: Filter::Off,
    };
    let config = PhpRuntimeConfig::new(&binding, binding.script_name.clone(), entry_pin);
    let runtime = PhpEmbedRuntime::load(
        SharedObject::Fd(binding.libphp.as_raw_fd()),
        SharedObject::Fd(binding.shim.as_raw_fd()),
        config,
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        runtime.php_version().split('.').take(2).collect::<Vec<_>>(),
        ["8", "4"]
    );

    let limits = Limits::default();
    let (worker, scheduler) = UnixStream::pair().expect("socketpair");
    scheduler.set_nonblocking(true).expect("non-blocking");
    // `libphp` records the thread stack it starts on and checks requests
    // against it (`zend.max_allowed_stack_size`), so load and serve run on this
    // thread — exactly as the forked view child does (ADR-0010 §3). The
    // scheduler's end lives on its own thread.
    let client = std::thread::spawn(move || {
        let mut peer = Peer {
            stream: scheduler,
            decoder: Decoder::with_capacity(limits, limits.max_frame() + HEADER_LEN),
        };
        let ready = peer.wait_for_ready();
        assert!(matches!(ready[0].1, Frame::Ready), "{ready:?}");
        let mut runs = Vec::new();
        for id in [1u32, 2] {
            let head = ReqHead {
                method: b"GET".to_vec(),
                target: b"/index.php?x=1".to_vec(),
                headers: vec![
                    (b"Host".to_vec(), b"native.test".to_vec()),
                    (b"Cookie".to_vec(), b"probe=1".to_vec()),
                ],
                remote_addr: "203.0.113.7".to_string(),
                scheme: Scheme::Http,
                server_name: "native.test".to_string(),
                server_port: 80,
                peer_addr: "203.0.113.7:4242".to_string(),
            };
            peer.send(id, Frame::ReqHead(head));
            peer.send(id, Frame::ReqEnd);
            runs.push(peer.wait_for_done(id));
        }
        runs
    });

    // `max_requests = 2`: the worker exits cleanly after the second request,
    // which is what the supervisor respawns on.
    let joined = serve_forever(worker, runtime, limits);
    assert!(joined.is_ok(), "the worker exited badly: {joined:?}");
    let runs = client.join().expect("client thread");

    for (index, frames) in runs.iter().enumerate() {
        let id = index as u32 + 1;
        let head = frames
            .iter()
            .find_map(|(_, frame)| match frame {
                Frame::RespHead(head) => Some(head.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no RESP_HEAD in {frames:?}"));
        assert_eq!(head.status, 200);
        assert!(
            head.headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case(b"X-Embedded") && value == b"1"),
            "headers: {:?}",
            head.headers
        );
        let body: Vec<u8> = frames
            .iter()
            .flat_map(|(_, frame)| match frame {
                Frame::RespBody(chunk) => chunk.clone(),
                _ => Vec::new(),
            })
            .collect();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("method=GET"), "body: {body}");
        assert!(
            body.contains(entry.file_name().unwrap().to_string_lossy().as_ref()),
            "SCRIPT_FILENAME was not the pinned entry: {body}"
        );
        let cost = frames.iter().find_map(|(_, frame)| match frame {
            Frame::RespEndCost { cost } => Some(*cost),
            _ => None,
        });
        let cost = cost.unwrap_or_else(|| panic!("no RESP_END_COST in {frames:?}"));
        assert!(cost.cpu_ns.is_some_and(|cpu| cpu > 0), "{cost:?}");
        assert!(cost.wall_ns > 0, "{cost:?}");
        assert!(
            frames.iter().any(|(_, frame)| matches!(
                frame,
                Frame::Done {
                    outcome: DoneOutcome::Completed
                }
            )),
            "request {id} did not finish with DONE(Completed): {frames:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&app);
}

/// The scheduler's end of the channel.
struct Peer {
    stream: UnixStream,
    decoder: Decoder,
}

impl Peer {
    fn send(&mut self, id: u32, frame: Frame) {
        let mut bytes = Vec::new();
        self.decoder
            .limits()
            .encode(&frame, id, &mut bytes)
            .expect("encode");
        self.stream.write_all(&bytes).expect("write");
    }

    /// Read whatever has arrived, waiting up to a second for more.
    fn pump(&mut self, frames: &mut Vec<(u32, Frame)>) {
        let mut buf = [0u8; 8192];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.decoder.push(&buf[..n]),
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err) => panic!("read: {err}"),
            }
        }
        while let Some(frame) = self.decoder.next_frame().expect("decode") {
            frames.push(frame);
        }
    }

    /// Wait for the worker's `READY`.
    fn wait_for_ready(&mut self) -> Vec<(u32, Frame)> {
        let mut frames = Vec::new();
        for _ in 0..200 {
            self.pump(&mut frames);
            if !frames.is_empty() {
                return frames;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the worker never sent READY");
    }

    /// Wait until `id`'s `DONE` arrives, returning everything in between.
    fn wait_for_done(&mut self, id: u32) -> Vec<(u32, Frame)> {
        let mut frames = Vec::new();
        for _ in 0..400 {
            self.pump(&mut frames);
            let done = frames
                .iter()
                .any(|(frame_id, frame)| *frame_id == id && matches!(frame, Frame::Done { .. }));
            if done {
                return frames;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("request {id} never finished: {frames:?}");
    }
}
