//! Embedded PHP: [`PhpEmbedRuntime`] over `view.chan` (design §4.1, §4.3,
//! `ADR-0010` §1, `ADR-0016` decision 2).
//!
//! The PHP worker is an ordinary view process. It runs [`PhpEmbedRuntime`] — and
//! it is the *only* place a `libphp` symbol is reachable: the crate stays
//! `#![forbid(unsafe_code)]`, and the `dlopen`/`dlsym` and the `extern "C"` sink
//! trampolines live in `blastit-php-sapi`'s `loader`.
//!
//! # One request at a time
//!
//! NTS `libphp` runs one request per OS process, so the worker serves one
//! request at a time: `serve` takes the process-wide handle behind a `Mutex`,
//! and two concurrent calls would corrupt PHP's globals. The scheduler forks
//! `1..=workers` such processes instead of threading one.
//!
//! # The entry pin is a required value, not an adapter
//!
//! `ADR-0016` decision 2 supersedes `MB.36.5`'s original note: the runtime seam
//! the pin used to wrap is gone, so [`PhpRuntimeConfig::new`] takes the
//! [`EntryPin`] the boot recorded as a **required** argument. There is no
//! `Option` and no `Default`: a runtime that could be built without a pin would
//! be a fail-open seam, and a `SCRIPT_FILENAME` swapped in place must be a `500`
//! (with the sentence on the worker's stderr) rather than a silent re-resolution
//! to whatever is at the path now. The record is checked once at load and again
//! before every request, beside the call into the shim.
//!
//! # The transport is single-threaded, and that is why `Credit` has `try_take`
//!
//! [`serve_forever`] reads `REQ_HEAD`, runs exactly one request, and writes
//! `RESP_END_COST` + `DONE` from the [`RequestEnd`] — the shape of the WSGI
//! loop, without its worker pool. The request body is read in full before PHP
//! starts (the C ABI takes one `body` slice), so during the request the only
//! frames left to arrive are `WINDOW` and `CANCEL`. A response body larger than
//! the initial credit therefore has to *read* while it waits, which is what
//! [`RequestIo::write_body`] does here: it asks the credit window for what it
//! has, and reads the next frame when it has nothing. No second thread, no
//! deadlock.
//!
//! # What a request costs
//!
//! The shim measures `CLOCK_PROCESS_CPUTIME_ID` around
//! `php_request_startup`..`php_request_shutdown` outside PHP userland, so the
//! number this runtime reports cannot be forged by a script (`ADR-0009`: the
//! `"php"` kind is `ProcessExact`). A zero reading is reported as `None` — no
//! claim at all — never as "free".

use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use blastit_frame::{
    Decoder, DoneOutcome, Frame, HEADER_LEN, Limits, ReqHead, RequestCost, RespHead,
};
use blastit_php_sapi::abi::{OUTCOME_BAILOUT, OUTCOME_OK, RequestV1, ServerVarV1};
use blastit_php_sapi::loader::bailed_out;
use blastit_php_sapi::{FileSink, LoadError, Loaded, ServerVars, SharedObject, Sink};

use blastit_view::ViewError;
use blastit_view::flow::Flow;
use blastit_view::loaded::{LoadedReport, Reporter};
use blastit_view::stream::{OUTPUT_LINE_CAP, OutputStream};
use blastit_view::sync::lock;
use blastit_view::wire::Writer;

use crate::pinning::{EntryPin, PhpBinding, verify_entry};

/// How much of the channel is read at once.
const READ_SIZE: usize = 64 * 1024;

/// How much request body is read per `read_body` call.
const BODY_READ: usize = 64 * 1024;

/// What a PHP view process is told about itself; fixed for its lifetime.
///
/// Built by [`PhpRuntimeConfig::new`] from one [`PhpBinding`], so the worker's
/// view of the slot cannot drift from the descriptors and the record the boot
/// verified.
#[derive(Debug, Clone)]
pub struct PhpRuntimeConfig {
    /// The route whose entry is pinned, for the refusal sentence.
    pub route: String,
    /// Absolute path of the script `SCRIPT_FILENAME` is pinned to — the path
    /// `entrypoint php` resolved to. Never request-derived (ADR-0010 §2).
    pub entry: PathBuf,
    /// The application root PHP sees as `DOCUMENT_ROOT` and `open_basedir`.
    pub document_root: PathBuf,
    /// The route's mount point, reported as `SCRIPT_NAME`; empty at the root.
    pub script_name: String,
    /// The `ZEND_INI_SYSTEM` blob [`blastit_php_sapi::IniPolicy`] rendered.
    pub ini: String,
    /// The view's private scratch directory (`open_basedir` allows it too;
    /// it is the process's `HOME`/`TMPDIR` and PHP's `sys_temp_dir`).
    pub view_tmp: PathBuf,
    /// Requests this process serves before it retires (`max_requests`).
    pub max_requests: u32,
    /// The largest request body this worker buffers, in bytes.
    ///
    /// The same number the route's INI pins as `post_max_size` / upload size.
    /// The C ABI takes one `body` slice, so the whole body is buffered before
    /// `php_request_startup`; without this cap a client could make one worker
    /// allocate without bound (the scheduler's own body limit is the first
    /// line, this is the worker's).
    pub max_body_bytes: usize,
    /// The record of the resolved `entry` the boot took (`ADR-0010` §3,
    /// `MB.3.56`): `(st_dev, st_ino)` plus the size/ctime content half, checked
    /// once at load and again before every request.
    ///
    /// Private and **required**: `ADR-0016` decision 2 deleted the adapter that
    /// used to re-check the pin outside the runtime, so a value that could be
    /// left unset would be a fail-open seam. Nothing reads it back out — the
    /// check is [`PhpEmbedRuntime`]'s.
    pin: EntryPin,
}

impl PhpRuntimeConfig {
    /// The worker's configuration for one PHP slot.
    ///
    /// Everything the worker is told about itself comes from the binding the
    /// boot verified — the two pinned descriptors, the resolved entry, the INI
    /// blob, the scratch directory and the two budgets — plus the mount point
    /// the composition root normalised and the entry's record.
    ///
    /// `pin` is a required argument rather than a field read back out of
    /// `binding`, because that is the value the check is about: a runtime built
    /// without one would run whatever is at `SCRIPT_FILENAME` now
    /// (`ADR-0016` decision 2).
    #[must_use]
    pub fn new(binding: &PhpBinding, script_name: String, pin: EntryPin) -> Self {
        Self {
            route: binding.route.clone(),
            entry: binding.entry.clone(),
            document_root: binding.document_root.clone(),
            script_name,
            ini: binding.ini.clone(),
            view_tmp: binding.view_tmp.clone(),
            max_requests: binding.max_requests,
            max_body_bytes: binding.max_body_bytes,
            pin,
        }
    }

    /// Refuses a configuration the worker could not serve, at boot.
    ///
    /// # Errors
    ///
    /// A message naming the field and the fix — never a secret. Everything here
    /// would otherwise become a first-request fault, and the rule is that a PHP
    /// worker fails at boot or not at all.
    pub fn validate(&self) -> Result<(), String> {
        if !self.entry.is_absolute() {
            return Err(format!(
                "entry {} must be an absolute path (the path `entrypoint php` resolved to)",
                self.entry.display()
            ));
        }
        if !self.entry.is_file() {
            return Err(format!(
                "entry {} is not a file: SCRIPT_FILENAME is pinned to it",
                self.entry.display()
            ));
        }
        if !self.document_root.is_absolute() || !self.document_root.is_dir() {
            return Err(format!(
                "the application root {} must be an existing absolute directory",
                self.document_root.display()
            ));
        }
        if !self.view_tmp.is_absolute() || !self.view_tmp.is_dir() {
            return Err(format!(
                "view_tmp {} must be an existing absolute directory: it is the worker's \
                 HOME, TMPDIR and PHP's sys_temp_dir",
                self.view_tmp.display()
            ));
        }
        if !self.script_name.is_empty()
            && (!self.script_name.starts_with('/') || self.script_name.ends_with('/'))
        {
            return Err(format!(
                "script_name {:?} must be empty or a mount point like \"/app\"",
                self.script_name
            ));
        }
        if self.ini.is_empty() {
            return Err(
                "the INI lockdown is empty; render it with IniPolicy::new() instead of \
                 inheriting the host's php.ini"
                    .to_string(),
            );
        }
        if self.max_requests == 0 {
            return Err("max_requests must be at least 1".to_string());
        }
        if self.max_body_bytes == 0 {
            return Err(
                "max_body_bytes must be at least 1: the worker buffers the whole body \
                 before PHP runs"
                    .to_string(),
            );
        }
        Ok(())
    }
}

/// Why a worker could not start. Every variant is a boot refusal, never a
/// degraded request.
#[derive(Debug, thiserror::Error)]
pub enum PhpError {
    /// The route's configuration cannot be served.
    #[error("invalid PHP route configuration: {0}")]
    Config(String),

    /// The `libphp`/shim load was refused (see [`LoadError`]).
    #[error(transparent)]
    Load(#[from] LoadError),
}

/// The state of the one PHP process this runtime owns.
struct PhpWorker {
    /// The loaded `libphp` + shim and its vtable.
    handle: Loaded,
    /// Requests served so far, for `max_requests`.
    served: u32,
    /// Set when a request bailed out: the interpreter is suspect, so the worker
    /// retires instead of serving another.
    bailed: bool,
}

// The dev-only loaded-file report a PHP view is handed (`MB.23.4.2`,
// `ADR-0014`) is the transport's: one `LoadedReport` type for every backend, and
// `MB.36.5` deleted this crate's duplicate of it (design §4.2.1). The socket is
// the write end of this view's own `AF_UNIX SOCK_DGRAM` socketpair — created
// before the fork, inherited in the child's keep list, never passed through the
// environment.

/// One PHP view's report (`MB.23.4.2`): the datagram sender and the
/// compiled→source memo, over the sink the *loader* owns.
///
/// The `FileSink` is not a field here, on purpose: `Loaded::install_file_sink`
/// takes it by value and keeps it in a `Box` for the process lifetime, which is
/// what makes the address the shim holds sound. Every method below is handed a
/// `&FileSink` borrowed from the loader (`Loaded::file_sink`), so there is
/// exactly one owner and no address this struct could invalidate. Everything
/// here is best effort — nothing returns an error, because a reporting failure
/// must never fail a request.
#[derive(Debug)]
struct PhpFileReport {
    /// The sender and the memo for the compiled→source map.
    state: Mutex<FileReportState>,
}

/// The report's mutable half, kept under one lock so a request either sends
/// its batch or does not, never half of it.
#[derive(Debug)]
struct FileReportState {
    /// The view's datagram sender.
    reporter: Reporter,
    /// Whether this source's one `boot` datagram is still owed. A view's first
    /// report replaces its set; an empty report sends nothing at all, so the
    /// boot stays owed until there is something to replace it with.
    boot_pending: bool,
    /// Compiled artefact → the template sources it names. A compiled Blade or
    /// Twig file never changes which template it was built from, so the read
    /// happens once per file rather than once per request.
    sources: HashMap<PathBuf, Vec<String>>,
}

impl PhpFileReport {
    /// Wraps the view's socket, or `None` when it cannot be made non-blocking.
    fn new(report: LoadedReport) -> Option<Self> {
        let reporter = Reporter::new(report.socket, report.source).ok()?;
        Some(Self {
            state: Mutex::new(FileReportState {
                reporter,
                boot_pending: true,
                sources: HashMap::new(),
            }),
        })
    }

    /// Forgets the previous request's snapshot.
    fn begin(&self, sink: &FileSink) {
        sink.clear();
    }

    /// Sends what this request loaded, once it has run.
    ///
    /// Called after the response is on the wire and before `DONE`, so the batch
    /// names exactly the files this request depended on. A batch with nothing in
    /// it is not sent: an empty report is not news, and the codec's `send`
    /// already treats it that way.
    fn finish(&self, sink: &FileSink) {
        let paths = sink.take();
        if paths.is_empty() {
            return;
        }
        let mut state = lock(&self.state);
        let mut all: BTreeSet<String> = paths.iter().cloned().collect();
        all.extend(template_sources(&paths, &mut state.sources));
        let paths: Vec<String> = all.into_iter().collect();
        let boot = state.boot_pending;
        let sent = state.reporter.send(boot, &paths);
        if sent.datagrams > 0 {
            // The `boot` flag went out with the first datagram of this send.
            state.boot_pending = false;
        }
    }
}

/// `entrypoint php`'s worker: NTS `libphp` behind the shim's vtable.
pub struct PhpEmbedRuntime {
    /// The process-wide handle. `Mutex` because `serve` takes `&self` (the
    /// scheduler's request path never owns the runtime) and `libphp` is not
    /// re-entrant.
    worker: Mutex<PhpWorker>,
    /// What every request is built from.
    cfg: PhpRuntimeConfig,
    /// The dev-only loaded-file report (`MB.23.4.2`). `None` in production,
    /// with reload off, and for a shim whose optional file sink is absent — in
    /// which case every request path below is the one it always was.
    report: Option<PhpFileReport>,
}

impl PhpEmbedRuntime {
    /// Loads `libphp` and the shim, runs `php_module_startup`, and returns the
    /// ready runtime — or a boot refusal.
    ///
    /// This is the production boot: no loaded-file report, no datagram socket,
    /// no reporting work on any request.
    ///
    /// # Errors
    ///
    /// [`PhpError::Config`] for a configuration [`PhpRuntimeConfig::validate`]
    /// refuses, and [`PhpError::Load`] for every linkage/ABI refusal. Nothing
    /// here touches the network, and a failure leaves no module started.
    pub fn load(
        libphp: SharedObject,
        shim: SharedObject,
        cfg: PhpRuntimeConfig,
    ) -> Result<Self, PhpError> {
        Self::load_reporting(libphp, shim, cfg, None)
    }

    /// [`PhpEmbedRuntime::load`], plus the dev-only loaded-file report
    /// (`MB.23.4.2`, `ADR-0014`).
    ///
    /// `report` is the write end of this view's report socketpair — created
    /// before the fork by whoever forks the view, `MB.23.5`'s job — and `None`
    /// is production and `--no-reload`: no socket, no reader, no report, and not
    /// one byte of extra work per request.
    ///
    /// A shim without the optional `blastit_php_file_sink_install` symbol, or
    /// one that refuses it, keeps that `None` behaviour exactly: this is an
    /// **addition** to the ABI, never a requirement of it, so a PHP build or
    /// shim from before it must serve as it always did.
    ///
    /// # Errors
    ///
    /// Exactly what [`PhpEmbedRuntime::load`] returns.
    pub fn load_reporting(
        libphp: SharedObject,
        shim: SharedObject,
        cfg: PhpRuntimeConfig,
        report: Option<LoadedReport>,
    ) -> Result<Self, PhpError> {
        cfg.validate().map_err(PhpError::Config)?;
        let mut handle = Loaded::load(libphp, shim)?;
        handle.worker_init(&cfg.ini, &cfg.view_tmp)?;
        let report = report.and_then(|report| {
            let report = PhpFileReport::new(report)?;
            // The sink is handed to the loader, which owns it (a `Box` it keeps
            // for the process) and is the only thing that can name the address
            // the shim holds. Both `NoSink` cases are silent by design — a shim
            // without the optional symbol, or one that refuses it — and neither
            // is a load failure: the worker then runs exactly as a production
            // one does.
            handle
                .install_file_sink(FileSink::new())
                .ok()
                .map(|_installed| report)
        });
        Ok(Self {
            worker: Mutex::new(PhpWorker {
                handle,
                served: 0,
                bailed: false,
            }),
            cfg,
            report,
        })
    }

    /// The host `libphp`'s version string (`"8.4.27"`), for the boot log.
    #[must_use]
    pub fn php_version(&self) -> String {
        lock(&self.worker).handle.php_version().to_owned()
    }
}

impl PhpEmbedRuntime {
    /// Runs one dispatched request to completion.
    ///
    /// Blocking, on the transport's request thread, with `&self`: the
    /// process-wide handle is taken behind a lock, because `libphp` is not
    /// re-entrant and an NTS worker runs one request per OS process. The
    /// response is written through `io` before this returns; the loop that owns
    /// the channel writes `RESP_END_COST` and `DONE` from the [`RequestEnd`].
    ///
    /// The entry pin is re-checked **here**, before the body is read and before
    /// `libphp` opens `SCRIPT_FILENAME` (`MB.3.56`, `ADR-0010` §3): a mismatch
    /// answers an empty `500` with the sentence on the worker's stderr, never in
    /// the response.
    pub fn serve(&self, io: &mut dyn RequestIo) -> RequestEnd {
        if let Err(sentence) = verify_entry(&self.cfg.route, &self.cfg.entry, self.cfg.pin) {
            return refuse(io, &sentence);
        }
        let req = io.head().clone();
        let mut worker = lock(&self.worker);
        worker.served = worker.served.saturating_add(1);
        let prepared = match Prepared::new(&self.cfg, &req) {
            Ok(prepared) => prepared,
            Err(message) => {
                io.output(OutputStream::Stderr, &message);
                return RequestEnd::new(DoneOutcome::Errored, None);
            }
        };

        // The C ABI takes one body slice, so the whole body is read first. The
        // request-body window (plus the `WINDOW` frames `read_body` sends as it
        // consumes) paces the sender; `max_body_bytes` is the worker's own
        // bound, checked before a byte is appended.
        let body = match read_body(io, self.cfg.max_body_bytes) {
            Ok(body) => body,
            Err(BodyError::TooLarge) => {
                io.output(
                    OutputStream::Stderr,
                    &format!(
                        "php: request body is larger than {} bytes",
                        self.cfg.max_body_bytes
                    ),
                );
                worker.bailed = true;
                return RequestEnd::new(DoneOutcome::Errored, None);
            }
            Err(BodyError::Io(err)) => {
                io.output(
                    OutputStream::Stderr,
                    &format!("php: reading the body: {err}"),
                );
                return RequestEnd::new(DoneOutcome::Aborted, None);
            }
        };

        let entries = prepared.entries();
        // The shim's snapshot is per request; this drops the last one. The cell
        // is the loader's, so this is the same address the shim was handed —
        // never a second owner of it.
        if let Some(report) = &self.report
            && let Some(sink) = worker.handle.file_sink()
        {
            report.begin(sink);
        }
        let mut sink = PhpSink {
            io,
            headers: Vec::new(),
            header_bytes: 0,
            head_sent: false,
            abort: false,
        };
        let result = worker
            .handle
            .request(&prepared.view(&entries, &body), &mut sink);
        let head_sent = sink.head_sent;
        drop(sink);
        // The contract says `flush_response` runs once, at the end, before the
        // caller closes the request; a flush that fails means the response did
        // not reach the wire.
        let flushed = io.flush_response();

        let (outcome, retire, cpu_ns) = match &result {
            Ok(result) => {
                let (outcome, retire) = ended(result, head_sent);
                (outcome, retire, cpu(result))
            }
            Err(err) => {
                io.output(OutputStream::Stderr, &format!("php: {err}"));
                (DoneOutcome::Errored, true, None)
            }
        };
        let outcome = if flushed.is_err() {
            DoneOutcome::Aborted
        } else {
            outcome
        };
        // `MB.23.4.2`: what this request loaded, reported after the response is
        // on the wire and before `DONE` — so it names exactly the files the
        // response depended on, and a reporting failure can only ever lose a
        // report, never change this request's outcome.
        if let Some(report) = &self.report
            && let Some(sink) = worker.handle.file_sink()
        {
            report.finish(sink);
        }
        worker.bailed |= retire;
        RequestEnd::new(outcome, cpu_ns)
    }

    /// Whether the worker must exit now instead of taking another request.
    ///
    /// A fatal bailout left the interpreter in a state it cannot reuse, and
    /// `max_requests` is the boot's recycling rule; the supervisor respawns
    /// lazily, and a `max_requests` exit is a clean `0`.
    #[must_use]
    pub fn recycle(&self) -> bool {
        let worker = lock(&self.worker);
        should_recycle(worker.bailed, worker.served, self.cfg.max_requests)
    }
}

/// The answer a per-request refusal gets: an empty `500` and the sentence on
/// the worker's stderr.
///
/// The reason belongs in the log, not in an answer a client can steer. The
/// outcome is `Completed` when the empty head reached the wire: the request was
/// answered, just not with the application's output.
fn refuse(io: &mut dyn RequestIo, sentence: &str) -> RequestEnd {
    io.output(OutputStream::Stderr, sentence);
    let head = RespHead {
        status: 500,
        reason: Vec::new(),
        headers: Vec::new(),
    };
    let outcome = match io.write_head(head).and_then(|()| io.flush_response()) {
        Ok(()) => DoneOutcome::Completed,
        Err(_) => DoneOutcome::Aborted,
    };
    RequestEnd::new(outcome, None)
}

/// What `serve` reports for one shim result: the `DONE` outcome, and whether
/// the worker must retire because the handler bailed out of a request.
///
/// `MB.3.41`: the return value of `php_execute_script` (and the shim's
/// `contained` flag) is the only thing that separates a `500` an app answered
/// from a `500` a dying handler left behind — both travel as a real
/// `RESP_HEAD`. The handler that bailed never returned, so its request is
/// [`DoneOutcome::Bailed`]; one that answered and returned is a completed
/// request whatever its status.
fn ended(result: &blastit_php_sapi::abi::ResultV1, head_sent: bool) -> (DoneOutcome, bool) {
    if bailed_out(result) {
        // A contained `exit()`/`die()`/fatal: the interpreter is left in a
        // state the worker cannot reuse, so it retires after this request.
        (DoneOutcome::Bailed, true)
    } else if result.outcome == OUTCOME_BAILOUT {
        // A shim reporting a bailout it says it did not contain is a shim this
        // side does not trust: report it as a failure, never as `Bailed`.
        (DoneOutcome::Errored, true)
    } else if result.outcome == OUTCOME_OK && head_sent {
        (DoneOutcome::Completed, false)
    } else {
        // No head reached the client: the request was never answered.
        (DoneOutcome::Errored, false)
    }
}

/// The worker's retirement rule: a fatal bailout, or `max_requests` served.
///
/// Split out so the rule is testable without a loaded `libphp`: the two
/// reasons the process must exit are the interpreter's state and its request
/// count, and nothing else.
fn should_recycle(bailed: bool, served: u32, max_requests: u32) -> bool {
    bailed || served >= max_requests
}

/// How much of one reported file the compiled→source map scans.
///
/// Blade's `/**PATH … ENDPATH**/` header is the first line of the compiled
/// file and Twig's `getSourceContext()` is within the first kilobyte; 4 KiB is
/// far past both and keeps the read bounded for a huge bundled artefact.
const MAP_READ_BYTES: u64 = 4096;

/// How many reported files one request scans for a source marker. A memo hit
/// costs no read and does not count, so a steady-state view scans nothing.
const MAX_MAP_FILES: usize = 128;

/// How many compiled→source entries the memo keeps at most.
///
/// The bound is on the **memo**, not on the results: once it holds this many
/// entries nothing new is remembered, so a long-lived view's map cannot grow
/// without limit even if every request names a fresh artefact. A miss past the
/// bound still reports its sources — it is simply rescanned next time, and one
/// request's rescan budget is [`MAX_MAP_FILES`].
const MAX_MAP_ENTRIES: usize = 4096;

/// The template sources named by the compiled artefacts among `paths`.
///
/// This is the compiled→source half of the report (`docs/research/
/// 2026-11-16-dev-fanotify-reload.md` §2.3): a framework compiles a template to
/// PHP and the artefact carries its source's absolute path, so the `.twig` /
/// `.blade.php` a route actually used is reported too — without touching PHP's
/// plain-file stream wrapper or `open_basedir`, both of which stay exactly as
/// they are.
///
/// Every failure is "no source": an unreadable file, a non-UTF-8 name and a
/// budget that is spent all contribute nothing, and nothing here can fail a
/// request.
fn template_sources(paths: &[String], memo: &mut HashMap<PathBuf, Vec<String>>) -> Vec<String> {
    let mut out = BTreeSet::new();
    let mut scanned = 0usize;
    for path in paths {
        let key = PathBuf::from(path);
        if let Some(sources) = memo.get(&key) {
            out.extend(sources.iter().cloned());
            continue;
        }
        if scanned >= MAX_MAP_FILES {
            continue;
        }
        scanned += 1;
        let sources = sources_in_file(&key);
        // The memo is bounded, empties included: `!sources.is_empty() ||
        // memo.len() < MAX_MAP_ENTRIES` kept inserting compiled artefacts past
        // the cap (an empty entry is still an entry), so a view that met a new
        // artefact on every request grew this map forever.
        if memo.len() < MAX_MAP_ENTRIES {
            memo.insert(key, sources.clone());
        }
        out.extend(sources);
    }
    out.into_iter().collect()
}

/// The template sources `path` names, or nothing when it is not a compiled
/// artefact (the ordinary case: most included files are hand-written PHP).
fn sources_in_file(path: &Path) -> Vec<String> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut prefix = Vec::new();
    // `take` is the bound: a huge file is read to `MAP_READ_BYTES` and no more.
    if file.take(MAP_READ_BYTES).read_to_end(&mut prefix).is_err() {
        return Vec::new();
    }
    // The prefix may end in the middle of a codepoint; a lossy view is what a
    // text scan wants, and it cannot panic.
    let text = String::from_utf8_lossy(&prefix);
    let mut out = BTreeSet::new();
    for source in compiled_sources(&text) {
        if let Some(source) = absolute(&source) {
            out.insert(source);
        }
    }
    out.into_iter().collect()
}

/// The template source paths a compiled artefact's text names.
///
/// Two markers, because two frameworks:
///
/// * Blade writes `<?php /**PATH /app/resources/views/welcome.blade.php
///   ENDPATH**/ ?>` as the artefact's first line;
/// * Twig's `getSourceContext()` returns `new Source("", "<name>", "<absolute
///   path>")`, so the absolute literals of that method are the template.
fn compiled_sources(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let open = "/**PATH ";
    let close = " ENDPATH**/";
    let mut at = 0usize;
    while let Some(offset) = text.get(at..).and_then(|rest| rest.find(open)) {
        let start = at + offset + open.len();
        let Some(end) = text.get(start..).and_then(|rest| rest.find(close)) else {
            break;
        };
        out.push(text[start..start + end].to_owned());
        at = start + end;
    }
    if let Some(method) = text.find("getSourceContext") {
        // One method's worth of text, not the whole file: a compiled Twig
        // template is a few kilobytes and the `Source` is at the top.
        let window: String = text[method..].chars().take(1024).collect();
        for literal in quoted(&window) {
            if literal.starts_with('/') {
                out.push(literal.to_owned());
            }
        }
    }
    out
}

/// The single- or double-quoted runs of `text`, in order.
///
/// A backslash escapes the next byte, so an escaped quote does not end a run;
/// the scan works on bytes and each run is converted to `str` on its own, which
/// keeps a non-UTF-8 byte from losing the rest of the text.
fn quoted(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let quote = bytes[index];
        if quote != b'"' && quote != b'\'' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        while end < bytes.len() && bytes[end] != quote {
            if bytes[end] == b'\\' {
                end += 1;
            }
            end += 1;
        }
        if end >= bytes.len() {
            break;
        }
        if let Ok(run) = std::str::from_utf8(&bytes[start..end]) {
            out.push(run);
        }
        index = end + 1;
    }
    out
}

/// `path` as the filesystem resolves it, when it is an absolute path that
/// resolves to a UTF-8 one — the same normalisation the sink applies, so a
/// template and a file included through a symlink are one entry in the report.
fn absolute(path: &str) -> Option<String> {
    if path.is_empty() || path.contains('\0') || !Path::new(path).is_absolute() {
        return None;
    }
    match std::fs::canonicalize(path) {
        Ok(resolved) => Some(
            resolved
                .to_str()
                .map_or_else(|| path.to_owned(), str::to_owned),
        ),
        Err(_) => Some(path.to_owned()),
    }
}

/// Why [`read_body`] stopped early.
#[derive(Debug)]
enum BodyError {
    /// The body is larger than the worker's budget.
    TooLarge,
    /// The channel failed while reading the body.
    Io(io::Error),
}

/// Reads the whole request body into one buffer, refusing more than `max`.
///
/// The C ABI takes a single `body` slice, so buffering is not optional; this is
/// the one place that bounds it.
fn read_body(io: &mut dyn RequestIo, max: usize) -> Result<Vec<u8>, BodyError> {
    let mut body = Vec::new();
    let mut buf = vec![0u8; BODY_READ];
    loop {
        match io.read_body(&mut buf) {
            Ok(0) => return Ok(body),
            Ok(n) => {
                if body.len().saturating_add(n) > max {
                    return Err(BodyError::TooLarge);
                }
                body.extend_from_slice(&buf[..n]);
            }
            Err(err) => return Err(BodyError::Io(err)),
        }
    }
}

/// The CPU time a request claims, or `None` when the shim could not read the
/// clock (a zero is "no report", never "free").
fn cpu(result: &blastit_php_sapi::abi::ResultV1) -> Option<u64> {
    (result.cpu_ns > 0).then_some(result.cpu_ns)
}

/// Serve one PHP view process on `chan` until the scheduler's end closes.
///
/// Sends `READY` before reading anything, serves one request at a time, and
/// returns when the channel ends, when `runtime.recycle()` says the process
/// must retire (`max_requests`, a fatal bailout), or on a channel/protocol
/// error. The caller owns the process's exit code: a `max_requests` exit is a
/// clean `0`, which is what the supervisor's respawn expects.
///
/// # Errors
///
/// [`ViewError`]: the channel could not be read or written, the bytes are not a
/// frame stream, or the scheduler sent a frame a request may not receive.
pub fn serve_forever(
    chan: UnixStream,
    runtime: PhpEmbedRuntime,
    limits: Limits,
) -> Result<(), ViewError> {
    serve_loop(chan, limits, |io| runtime.serve(io), || runtime.recycle())
}

/// [`serve_forever`], with the two things the loop needs from the worker as
/// parameters: run one request, and say whether to retire.
///
/// It is private because the public seam is the concrete [`PhpEmbedRuntime`]
/// (`ADR-0016` decision 2): a worker process owns its loop, and there is no
/// cross-crate trait object left to implement. The unit cases below drive this
/// function with a stand-in that answers requests without a loaded `libphp`;
/// nothing outside `#[cfg(test)]` can name it.
fn serve_loop(
    mut chan: UnixStream,
    limits: Limits,
    mut serve: impl FnMut(&mut dyn RequestIo) -> RequestEnd,
    mut recycle: impl FnMut() -> bool,
) -> Result<(), ViewError> {
    let writer = Arc::new(Writer::new(chan.try_clone()?, limits));
    writer.write_frame(&Frame::Ready, 0)?;
    let mut decoder = Decoder::with_capacity(limits, limits.max_frame() + HEADER_LEN);
    let mut buf = vec![0u8; READ_SIZE];

    loop {
        let (id, head) = match read_frame(&mut chan, &mut decoder, &mut buf)? {
            None => return Ok(()),
            Some((id, Frame::ReqHead(head))) if id != 0 => (id, head),
            // A grant for a request that already ended: the scheduler may still
            // be spending its response credit when the last frame went out.
            // There is no window to add it to, so it is dropped.
            Some((_, Frame::Window { .. })) => continue,
            // A cancel for a request that already ended is stale scheduling
            // bookkeeping, not a channel failure: `DONE` already went out, so
            // there is nothing left to abort.
            Some((_, Frame::Cancel)) => continue,
            Some((_, other)) => {
                return Err(ViewError::Protocol(format!(
                    "a PHP view may not receive {} before a REQ_HEAD",
                    blastit_view::frame_kind(&other)
                )));
            }
        };
        let flow = Flow::new();
        let mut io = ChanIo {
            head,
            id,
            writer: &writer,
            flow,
            chan: &mut chan,
            decoder: &mut decoder,
            buf: &mut buf,
            pending: Vec::new(),
            ended: false,
            head_sent: false,
            cancelled: false,
            error: None,
        };
        let started = Instant::now();
        let mut end = serve(&mut io);
        let wall_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        // A runtime that did not read the whole body (a PHP script killed
        // before it looked at it) must not leave `REQ_BODY`/`REQ_END` queued:
        // the next frame on the channel has to be the next `REQ_HEAD`.
        io.drain_body();
        if let Some(err) = io.error.take() {
            return Err(err);
        }
        // A client disconnect is not the view's failure. The scheduler has
        // already abandoned this stream and is waiting for `DONE` to release
        // the slot; the response is reported as aborted and the process goes on
        // to the next request. Only a broken channel ends the loop (above).
        if io.cancelled {
            end.outcome = DoneOutcome::Aborted;
        }
        writer.write_frame(
            &Frame::RespEndCost {
                cost: RequestCost::new(wall_ns, end.cpu_ns),
            },
            id,
        )?;
        writer.write_frame(
            &Frame::Done {
                outcome: end.outcome,
            },
            id,
        )?;
        if recycle() {
            return Ok(());
        }
    }
}

/// How one dispatched request ended.
///
/// The same two facts `RESP_END`/`RESP_END_COST` and `DONE` carry: how the
/// request ended — completed, abandoned, errored, or bailed out of a handler
/// that never returned (`DoneOutcome::Bailed`, `MB.3.41`) — and what the request
/// cost in CPU time. The wall time on the wire is the transport's business, and
/// the scheduler ignores it (`R18`), so it is not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestEnd {
    /// What to report in `DONE`, which is what frees the scheduler's slot.
    pub outcome: DoneOutcome,
    /// The request's CPU time, or `None` when the runtime could not measure it.
    ///
    /// Absent is not zero: a measurement of zero is a claim about the request,
    /// and a runtime with no readable clock (`CLOCK_PROCESS_CPUTIME_ID` here)
    /// makes no claim at all. The scheduler charges a `None` as its kernel
    /// floor, never as zero (`ADR-0009`).
    pub cpu_ns: Option<u64>,
}

impl RequestEnd {
    /// A request that ended with `outcome` and cost `cpu_ns`.
    #[must_use]
    pub const fn new(outcome: DoneOutcome, cpu_ns: Option<u64>) -> Self {
        Self { outcome, cpu_ns }
    }
}

/// The transport, as one request sees it: the `view.chan` framing and nothing
/// underneath it.
///
/// Implemented here by the channel loop and by the unit cases'
/// stand-ins. Every method is a frame or a flow-control operation on
/// `view.chan` — `REQ_BODY`/`WINDOW` for the body, `RESP_HEAD`, `RESP_BODY` and
/// the window for the response — so the runtime never holds a socket, a file
/// descriptor or a scheduler handle, and cannot write to another request's
/// stream.
///
/// The methods are `&mut self` because a request owns its stream for its
/// lifetime; the implementation is free to share the process-wide writer behind
/// it, as `view.chan` requires one writer per frame.
pub trait RequestIo {
    /// The head the scheduler dispatched.
    fn head(&self) -> &ReqHead;

    /// Read request-body bytes.
    ///
    /// Returns the number of bytes read, or `Ok(0)` when the body has ended.
    /// Paced by the request-body credit window (§3.1): a runtime that reads
    /// slowly is granted more credit slowly, so a slow application slows its
    /// own client and no one else. Consuming bytes is what returns the credit.
    ///
    /// # Errors
    ///
    /// The channel could not be read, the stream was cancelled, or the body
    /// exceeded its window.
    fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Send the response head.
    ///
    /// At most once per request: the head must precede the body, and a second
    /// head is a protocol error the transport refuses.
    ///
    /// # Errors
    ///
    /// The frame could not be encoded or written, or the stream was cancelled.
    fn write_head(&mut self, head: RespHead) -> io::Result<()>;

    /// Write response-body bytes.
    ///
    /// Paced by the response credit window (§3.1): a runtime that has spent its
    /// credit blocks here until the scheduler grants more, as it would on a
    /// slow client's socket, rather than buffering without bound. A cancelled
    /// stream wakes the write and reports it as an error.
    ///
    /// # Errors
    ///
    /// The channel could not be written, or the stream was cancelled.
    fn write_body(&mut self, data: &[u8]) -> io::Result<()>;

    /// Record one line of the runtime's own output.
    ///
    /// Not the response: this is the runtime's `STDOUT`/`STDERR` —
    /// PHP's output outside the response — where a Zend notice goes.
    /// Implementations keep at most [`OUTPUT_LINE_CAP`] bytes of each line, so
    /// one huge `echo` cannot bury the log it lands in, and they never fail the
    /// request for an output error: losing a diagnostic line is better than
    /// failing a response that was otherwise fine.
    fn output(&mut self, stream: OutputStream, line: &str);

    /// Push everything written so far onto the channel.
    ///
    /// Called once, at the end of a request, so the response a runtime reports
    /// is on the wire before the caller closes the request with
    /// `RESP_END`/`DONE`.
    ///
    /// # Errors
    ///
    /// The channel could not be written, or the stream was cancelled.
    fn flush_response(&mut self) -> io::Result<()>;
}

/// Read one frame from `chan`, or `None` at a clean end of channel.
fn read_frame(
    chan: &mut UnixStream,
    decoder: &mut Decoder,
    buf: &mut [u8],
) -> Result<Option<(u32, Frame)>, ViewError> {
    loop {
        if let Some(frame) = decoder.next_frame()? {
            return Ok(Some(frame));
        }
        match chan.read(buf) {
            Ok(0) => return Ok(None),
            Ok(n) => decoder.push(&buf[..n]),
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
                ) =>
            {
                return Ok(None);
            }
            Err(err) => return Err(ViewError::Io(err)),
        }
    }
}

/// One request's view of `view.chan`.
///
/// The response is written through the process's one [`Writer`], one frame at
/// a time. Reads and writes are sequential — one request is in flight — so a
/// write that runs out of credit reads the next `WINDOW` itself.
struct ChanIo<'a> {
    head: ReqHead,
    id: u32,
    writer: &'a Writer,
    flow: Arc<Flow>,
    chan: &'a mut UnixStream,
    decoder: &'a mut Decoder,
    buf: &'a mut [u8],
    /// Request body bytes received but not yet handed to the runtime.
    pending: Vec<u8>,
    /// `REQ_END` arrived.
    ended: bool,
    /// `RESP_HEAD` went out.
    head_sent: bool,
    /// The scheduler sent `CANCEL`: the client is gone. The rest of this
    /// request's response is dropped, but the process keeps serving — a
    /// disconnect must not cost a fork and a `php_module_startup`.
    cancelled: bool,
    /// The first channel/protocol error, reported once `serve` returns: the
    /// runtime has no way to return it, and a broken channel ends the view.
    error: Option<ViewError>,
}

impl ChanIo<'_> {
    /// The next frame, remembering a broken channel as the request's error.
    fn next(&mut self) -> io::Result<(u32, Frame)> {
        loop {
            match self.decoder.next_frame() {
                Ok(Some(frame)) => return Ok(frame),
                Ok(None) => {}
                Err(err) => return Err(self.fail(ViewError::Frame(err))),
            }
            match self.chan.read(self.buf) {
                Ok(0) => {
                    return Err(self.fail(ViewError::Io(io::Error::from(ErrorKind::UnexpectedEof))));
                }
                Ok(n) => self.decoder.push(&self.buf[..n]),
                Err(err)
                    if matches!(
                        err.kind(),
                        ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
                    ) => {}
                Err(err) => return Err(self.fail(ViewError::Io(err))),
            }
        }
    }

    /// Records `err` as the request's outcome and returns the `io::Error` the
    /// runtime sees. The first error wins.
    fn fail(&mut self, err: ViewError) -> io::Error {
        let message = err.to_string();
        self.error.get_or_insert(err);
        io::Error::other(message)
    }

    /// Credits `bytes` of already-received request body back to the scheduler,
    /// refusing more than the window allows.
    fn credit_back(&mut self, bytes: usize) -> io::Result<()> {
        if !self.flow.body_received(bytes) {
            let message = format!("request {} sent more body than its credit window", self.id);
            return Err(self.fail(ViewError::Protocol(message)));
        }
        self.flow.body_granted(bytes);
        let granted = u32::try_from(bytes).unwrap_or(u32::MAX);
        self.writer
            .write_frame(&Frame::Window { bytes: granted }, self.id)
            .map_err(|err| self.fail(err.into()))
    }

    /// Consume whatever request body the runtime left unread, so the next frame
    /// on the channel is the next `REQ_HEAD`.
    ///
    /// A PHP script that dies before reading its input leaves `REQ_BODY` and
    /// `REQ_END` queued; without this the next request's dispatch would read
    /// them as its own and the whole view would fail. The bytes are credited
    /// back as they are dropped, exactly as a reader would, so the scheduler's
    /// sender is never left waiting.
    fn drain_body(&mut self) {
        while !self.ended && self.error.is_none() {
            let (id, frame) = match self.next() {
                Ok(frame) => frame,
                Err(_) => return,
            };
            if id != self.id {
                // A cancel for a request that already ended is stale scheduling
                // bookkeeping; every other foreign frame is a protocol violation.
                if matches!(frame, Frame::Cancel) {
                    continue;
                }
                let message = format!(
                    "a frame for request {id} arrived while request {} was running",
                    self.id
                );
                self.fail(ViewError::Protocol(message));
                return;
            }
            match frame {
                Frame::ReqBody(bytes) => {
                    if self.credit_back(bytes.len()).is_err() {
                        return;
                    }
                }
                Frame::ReqEnd => self.ended = true,
                Frame::Cancel => {
                    // The stream is abandoned; nothing more of its body will be
                    // read, and the next frame is the next request's.
                    let _ = self.cancel();
                    return;
                }
                Frame::Window { bytes } => self.flow.response().grant(bytes),
                other => {
                    let message = format!(
                        "unexpected {} while draining a request body",
                        blastit_view::frame_kind(&other)
                    );
                    self.fail(ViewError::Protocol(message));
                    return;
                }
            }
        }
    }

    /// Records the scheduler's `CANCEL`: the client is gone, so the response is
    /// abandoned but the view stays alive.
    ///
    /// This is deliberately **not** [`ChanIo::fail`]: a cancel is an ordinary
    /// event in a request's life, while `error` means the channel itself is
    /// broken and the process must retire.
    fn cancel(&mut self) -> io::Error {
        self.flow.response().close();
        self.cancelled = true;
        self.ended = true;
        io::Error::other("the scheduler cancelled the request")
    }

    /// Reads one inbound frame and applies it while waiting for response credit.
    fn pump(&mut self) -> io::Result<()> {
        let (id, frame) = self.next()?;
        if id != self.id {
            if matches!(frame, Frame::Cancel) {
                return Ok(());
            }
            return Err(self.fail(ViewError::Protocol(format!(
                "a frame for request {id} arrived while request {} was running",
                self.id
            ))));
        }
        match frame {
            Frame::Window { bytes } => {
                self.flow.response().grant(bytes);
                Ok(())
            }
            Frame::ReqBody(bytes) => self.credit_back(bytes.len()),
            Frame::ReqEnd => {
                self.ended = true;
                Ok(())
            }
            Frame::Cancel => Err(self.cancel()),
            other => Err(self.fail(ViewError::Protocol(format!(
                "unexpected {} during a PHP request",
                blastit_view::frame_kind(&other)
            )))),
        }
    }
}

impl RequestIo for ChanIo<'_> {
    fn head(&self) -> &ReqHead {
        &self.head
    }

    fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pending.is_empty() && !self.ended {
            let (id, frame) = self.next()?;
            if id != self.id {
                if matches!(frame, Frame::Cancel) {
                    continue;
                }
                return Err(self.fail(ViewError::Protocol(format!(
                    "a frame for request {id} arrived while request {} was running",
                    self.id
                ))));
            }
            match frame {
                Frame::ReqBody(bytes) => {
                    if !self.flow.body_received(bytes.len()) {
                        return Err(self.fail(ViewError::Protocol(format!(
                            "request {} sent more body than its credit window",
                            self.id
                        ))));
                    }
                    self.pending.extend_from_slice(&bytes);
                }
                Frame::ReqEnd => self.ended = true,
                Frame::Window { bytes } => self.flow.response().grant(bytes),
                Frame::Cancel => return Err(self.cancel()),
                other => {
                    return Err(self.fail(ViewError::Protocol(format!(
                        "unexpected {} while reading a request body",
                        blastit_view::frame_kind(&other)
                    ))));
                }
            }
        }
        let n = buf.len().min(self.pending.len());
        if n == 0 {
            return Ok(0);
        }
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        // Consuming bytes is what returns the credit (§3.1).
        self.flow.body_granted(n);
        let granted = u32::try_from(n).unwrap_or(u32::MAX);
        self.writer
            .write_frame(&Frame::Window { bytes: granted }, self.id)
            .map_err(|err| self.fail(err.into()))?;
        Ok(n)
    }

    fn write_head(&mut self, head: RespHead) -> io::Result<()> {
        // A cancelled client has nowhere to receive a head; the shim is told
        // the write succeeded so it finishes tidily instead of panicking the
        // request path.
        if self.cancelled {
            return Ok(());
        }
        if self.head_sent {
            return Err(self.fail(ViewError::Protocol(
                "RESP_HEAD was written twice for one request".to_string(),
            )));
        }
        self.writer
            .write_frame(&Frame::RespHead(head), self.id)
            .map_err(|err| self.fail(err.into()))?;
        self.head_sent = true;
        Ok(())
    }

    fn write_body(&mut self, data: &[u8]) -> io::Result<()> {
        // Nothing of an abandoned response reaches the channel; the runtime
        // still gets `Ok` so PHP can unwind normally.
        if self.cancelled {
            return Ok(());
        }
        // The head must precede the body (the same rule `RESP_HEAD` documents
        // on the wire); a runtime that writes a body first is a bug, and the
        // scheduler would refuse the frame anyway.
        if !self.head_sent {
            return Err(self.fail(ViewError::Protocol(
                "RESP_BODY before RESP_HEAD".to_string(),
            )));
        }
        let mut sent = 0;
        while sent < data.len() {
            if self.cancelled {
                return Ok(());
            }
            let want = (data.len() - sent).min(self.writer.max_payload());
            match self.flow.response().try_take(want) {
                Some(0) => {
                    if self.cancelled {
                        return Ok(());
                    }
                    return Err(self.fail(ViewError::Protocol(
                        "the response window closed before the body was written".to_string(),
                    )));
                }
                Some(taken) => {
                    let chunk = data[sent..sent + taken].to_vec();
                    self.writer
                        .write_frame(&Frame::RespBody(chunk), self.id)
                        .map_err(|err| self.fail(err.into()))?;
                    sent += taken;
                }
                // No credit yet: read the next WINDOW (or CANCEL).
                None => {
                    if let Err(err) = self.pump() {
                        // A cancel is not a failed write: the client is gone,
                        // and the shim must finish the request tidily rather
                        // than see an aborted output (which would look like a
                        // fatal bailout and retire the worker).
                        if self.cancelled {
                            return Ok(());
                        }
                        return Err(err);
                    }
                }
            }
        }
        Ok(())
    }

    fn output(&mut self, stream: OutputStream, line: &str) {
        // The shim already bounds a log line; the contract is the cap, and one
        // huge `echo` outside the response must not bury the log either.
        let capped = capped_line(line);
        let _ = match stream {
            OutputStream::Stdout => writeln!(io::stdout(), "{capped}"),
            OutputStream::Stderr => writeln!(io::stderr(), "{capped}"),
        };
    }

    fn flush_response(&mut self) -> io::Result<()> {
        // Every frame is written synchronously, so there is nothing buffered:
        // the response is on the wire before `serve` returns.
        Ok(())
    }
}

/// A `ResponseSink` for the shim: headers accumulate until the end-of-head
/// marker, the body streams straight through `RequestIo`, and logs become the
/// worker's stderr.
struct PhpSink<'a> {
    io: &'a mut dyn RequestIo,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// Bytes the accumulated header lines hold, for the cap.
    header_bytes: usize,
    head_sent: bool,
    abort: bool,
}

/// The most response headers one request may send.
///
/// A script in a loop can emit headers without bound; each one is a `Vec` pair
/// held until the end-of-head marker. 256 is far above any real framework and
/// small enough that the buffer is bounded by construction.
const MAX_HEADERS: usize = 256;

/// The most bytes the accumulated response headers may hold.
const MAX_HEADER_BYTES: usize = 64 * 1024;

impl Sink for PhpSink<'_> {
    fn head(&mut self, status: i32, name: Option<&[u8]>, value: &[u8]) -> bool {
        let Some(name) = name else {
            // The end-of-head marker: always sent, always carries the status.
            if self.head_sent {
                return true;
            }
            let status = u16::try_from(status).unwrap_or(500);
            let status = if (100..=999).contains(&status) {
                status
            } else {
                500
            };
            let head = RespHead {
                status,
                reason: Vec::new(),
                headers: std::mem::take(&mut self.headers),
            };
            if self.io.write_head(head).is_err() {
                self.abort = true;
            }
            self.head_sent = true;
            return !self.abort;
        };
        if self.head_sent {
            return true;
        }
        // Cap the buffer: past either limit the response is abandoned rather
        // than grown, and the C side stops sending headers.
        if self.headers.len() >= MAX_HEADERS
            || self
                .header_bytes
                .saturating_add(name.len())
                .saturating_add(value.len())
                > MAX_HEADER_BYTES
        {
            self.abort = true;
            return false;
        }
        self.header_bytes += name.len() + value.len();
        self.headers.push((name.to_vec(), value.to_vec()));
        true
    }

    fn body(&mut self, data: &[u8]) -> bool {
        if !data.is_empty() && self.io.write_body(data).is_err() {
            self.abort = true;
        }
        !self.abort
    }

    fn log(&mut self, _level: i32, line: &str) {
        self.io.output(OutputStream::Stderr, line);
    }
}

/// One request's `RequestV1` and the C strings it borrows.
struct Prepared {
    entry: CString,
    document_root: CString,
    script_name: CString,
    request_uri: CString,
    query_string: Option<CString>,
    method: CString,
    content_type: Option<CString>,
    cookie: Option<CString>,
    vars: ServerVars,
}

impl Prepared {
    /// Builds the request environment from `head` and the route's config.
    ///
    /// The CGI contract, pinned here so the three variables cannot disagree:
    /// `REQUEST_URI` is the target exactly as routed (query string included),
    /// `SCRIPT_NAME` is the route's mount point (`/` at the root), and
    /// `PATH_INFO` is the path with that mount point stripped at a segment
    /// boundary — `/app` + `/app/list` gives `SCRIPT_NAME=/app`,
    /// `PATH_INFO=/list`, and `/application` is left whole because it is not
    /// inside the mount.
    fn new(cfg: &PhpRuntimeConfig, head: &ReqHead) -> Result<Self, String> {
        let target = String::from_utf8_lossy(&head.target);
        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (&target[..], None),
        };
        let script = script_name(cfg);
        let path_info = strip_mount(script, path);
        let mut vars = ServerVars::new();
        let insert = |vars: &mut ServerVars, name: &str, value: &str| {
            if let Err(err) = vars.insert(name, value) {
                return Err(format!("{name}: {err}"));
            }
            Ok(())
        };
        insert(
            &mut vars,
            "REQUEST_METHOD",
            &String::from_utf8_lossy(&head.method),
        )?;
        insert(&mut vars, "REQUEST_URI", &target)?;
        // `SCRIPT_FILENAME` is the route's entry, never a request-derived
        // path: this is the `php2shell` invariant in its positive form
        // (ADR-0010 §2).
        insert(&mut vars, "SCRIPT_FILENAME", &cfg.entry.to_string_lossy())?;
        insert(
            &mut vars,
            "DOCUMENT_ROOT",
            &cfg.document_root.to_string_lossy(),
        )?;
        insert(&mut vars, "SCRIPT_NAME", script)?;
        insert(&mut vars, "PATH_INFO", path_info)?;
        insert(&mut vars, "QUERY_STRING", query.unwrap_or(""))?;
        insert(&mut vars, "SERVER_PROTOCOL", "HTTP/1.1")?;
        insert(&mut vars, "GATEWAY_INTERFACE", "CGI/1.1")?;
        insert(&mut vars, "SERVER_NAME", &head.server_name)?;
        insert(&mut vars, "SERVER_PORT", &head.server_port.to_string())?;
        insert(&mut vars, "REMOTE_ADDR", &head.remote_addr)?;
        insert(
            &mut vars,
            "HTTPS",
            if matches!(head.scheme, blastit_frame::Scheme::Https) {
                "on"
            } else {
                "off"
            },
        )?;
        for (name, value) in &head.headers {
            // `push_header` drops `Proxy:` (httpoxy) and anything that would
            // derive a reserved name; it can never make the boot fail.
            let _ = vars.push_header(
                &String::from_utf8_lossy(name),
                &String::from_utf8_lossy(value),
            );
        }
        Ok(Self {
            entry: c_string(&cfg.entry.to_string_lossy(), "entry")?,
            document_root: c_string(&cfg.document_root.to_string_lossy(), "document_root")?,
            script_name: c_string(script_name(cfg), "script_name")?,
            request_uri: c_string(&target, "request_uri")?,
            query_string: query
                .map(|query| c_string(query, "query_string"))
                .transpose()?,
            method: c_string(&String::from_utf8_lossy(&head.method), "method")?,
            content_type: header(head, b"content-type")
                .map(|value| c_string(&String::from_utf8_lossy(value), "content-type"))
                .transpose()?,
            cookie: header(head, b"cookie")
                .map(|value| c_string(&String::from_utf8_lossy(value), "cookie"))
                .transpose()?,
            vars,
        })
    }

    /// The `blastit_php_server_var_v1` array, borrowing `vars`'s strings.
    fn entries(&self) -> Vec<ServerVarV1> {
        self.vars
            .iter()
            .map(|var| ServerVarV1 {
                name: var.name().as_ptr().cast(),
                name_len: var.name().len(),
                value: var.value().as_ptr().cast(),
                value_len: var.value().len(),
            })
            .collect()
    }

    /// The request the shim runs. Every pointer borrows `self`, `entries` or
    /// `body` for the call.
    fn view<'a>(&'a self, entries: &'a [ServerVarV1], body: &'a [u8]) -> RequestV1 {
        RequestV1 {
            entry: self.entry.as_ptr(),
            document_root: self.document_root.as_ptr(),
            script_name: self.script_name.as_ptr(),
            request_uri: self.request_uri.as_ptr(),
            query_string: self
                .query_string
                .as_ref()
                .map_or(std::ptr::null(), |q| q.as_ptr()),
            request_method: self.method.as_ptr(),
            content_type: self
                .content_type
                .as_ref()
                .map_or(std::ptr::null(), |c| c.as_ptr()),
            content_length: i64::try_from(body.len()).unwrap_or(i64::MAX),
            cookie: self
                .cookie
                .as_ref()
                .map_or(std::ptr::null(), |c| c.as_ptr()),
            server_vars: entries.as_ptr(),
            server_var_count: entries.len(),
            body: if body.is_empty() {
                std::ptr::null()
            } else {
                body.as_ptr()
            },
            body_len: body.len(),
        }
    }
}

/// The first [`OUTPUT_LINE_CAP`] bytes of a log line, cut on a character
/// boundary so a multi-byte character is never split.
fn capped_line(line: &str) -> &str {
    let mut end = 0;
    for ch in line.chars() {
        let next = end + ch.len_utf8();
        if next > OUTPUT_LINE_CAP {
            break;
        }
        end = next;
    }
    &line[..end]
}

/// The mount point PHP sees as `SCRIPT_NAME`; `/` when mounted at the root.
fn script_name(cfg: &PhpRuntimeConfig) -> &str {
    if cfg.script_name.is_empty() {
        "/"
    } else {
        cfg.script_name.as_str()
    }
}

/// `path` with the mount point `script` stripped at a segment boundary.
///
/// `/app` matches `/app` (→ `/`) and `/app/list` (→ `/list`) but not
/// `/application`, which is not inside the mount and is returned unchanged. A
/// route mounted at `/` strips nothing. The same rule the WSGI view and the
/// scheduler's router apply, so `PATH_INFO` cannot mean two things.
fn strip_mount<'a>(script: &str, path: &'a str) -> &'a str {
    if script.is_empty() || script == "/" {
        return path;
    }
    let Some(rest) = path.strip_prefix(script) else {
        return path;
    };
    if rest.is_empty() {
        return "/";
    }
    if rest.starts_with('/') {
        return rest;
    }
    path
}

/// A header value, case-insensitively.
fn header<'a>(head: &'a ReqHead, name: &[u8]) -> Option<&'a [u8]> {
    head.headers
        .iter()
        .find(|(held, _)| held.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_slice())
}

/// A C string, naming the field in the refusal.
fn c_string(text: &str, what: &str) -> Result<CString, String> {
    CString::new(text).map_err(|_| format!("{what} holds a NUL byte"))
}

/// Whether `path` is an existing absolute file (used by the spawn path's
/// boot-time check).
#[must_use]
pub fn is_serveable_entry(path: &Path) -> bool {
    path.is_absolute() && path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::INITIAL_WINDOW;
    use blastit_php_sapi::abi::ResultV1;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixDatagram, UnixStream};

    /// The two things [`serve_loop`] needs from a worker, for the stand-ins
    /// below.
    ///
    /// It is the test module's own and cannot leave `#[cfg(test)]`: the public
    /// seam is the concrete [`PhpEmbedRuntime`], and a loaded `libphp` is what
    /// that needs. There is no `kind`, no concurrency claim and no trait object
    /// on any wire — only the call the loop makes and the retirement rule.
    trait StandIn: Send + Sync + 'static {
        /// Run one request against a recording transport.
        fn serve(&self, io: &mut dyn RequestIo) -> RequestEnd;
        /// Whether the loop must return after this request.
        fn recycle(&self) -> bool;
    }

    /// A runtime that answers every request with the body it read.
    struct Echo {
        requests: Mutex<u32>,
    }

    impl StandIn for Echo {
        fn serve(&self, io: &mut dyn RequestIo) -> RequestEnd {
            *lock(&self.requests) += 1;
            let req = io.head().clone();
            let mut buf = [0u8; 16];
            let mut got = Vec::new();
            while let Ok(n) = io.read_body(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            let head = RespHead {
                status: 200,
                reason: Vec::new(),
                headers: vec![(b"X-Method".to_vec(), req.method)],
            };
            if io.write_head(head).is_err() || io.write_body(&got).is_err() {
                return RequestEnd::new(DoneOutcome::Errored, None);
            }
            io.output(OutputStream::Stderr, "echoed");
            RequestEnd::new(DoneOutcome::Completed, Some(7))
        }
        fn recycle(&self) -> bool {
            false
        }
    }

    /// The scheduler's end of a `view.chan`, decoding what the worker writes.
    struct Peer {
        stream: UnixStream,
        decoder: Decoder,
        limits: Limits,
    }

    impl Peer {
        fn send(&mut self, id: u32, frame: Frame) {
            let mut bytes = Vec::new();
            self.limits.encode(&frame, id, &mut bytes).expect("encode");
            self.stream.write_all(&bytes).expect("write");
        }

        /// [`Peer::send`], with a **gone peer** returning its error instead of
        /// panicking.
        ///
        /// Only for a call site where the view ending is what is being
        /// asserted. Every other call site keeps [`Peer::send`]: a closed peer
        /// there would be a real failure, not a tolerated outcome.
        fn send_to_a_gone_peer(&mut self, id: u32, frame: Frame) -> io::Result<()> {
            let mut bytes = Vec::new();
            self.limits.encode(&frame, id, &mut bytes).expect("encode");
            match self.stream.write_all(&bytes) {
                Ok(()) => Ok(()),
                Err(err) if peer_is_gone(&err) => Err(err),
                Err(err) => panic!("write: {err}"),
            }
        }

        fn drain(&mut self) -> Vec<(u32, Frame)> {
            let mut buf = [0u8; 4096];
            loop {
                match self.stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => self.decoder.push(&buf[..n]),
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => panic!("read: {err}"),
                }
            }
            let mut frames = Vec::new();
            while let Some(frame) = self.decoder.next_frame().expect("decode") {
                frames.push(frame);
            }
            frames
        }
    }

    /// Whether a write failed because the view's end of `view.chan` is gone.
    ///
    /// Three names for one situation: an `AF_UNIX` peer that exits with data
    /// still queued reports `ECONNRESET` (or `UnexpectedEof`) as readily as
    /// `EPIPE` (`LESSONS.md`), so a test helper that only named `BrokenPipe`
    /// would be the same flake in a smaller box.
    fn peer_is_gone(err: &io::Error) -> bool {
        matches!(
            err.kind(),
            ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof
        )
    }

    fn pair(limits: Limits) -> (UnixStream, Peer) {
        let (worker, scheduler) = UnixStream::pair().expect("socketpair");
        scheduler.set_nonblocking(true).expect("non-blocking");
        (
            worker,
            Peer {
                stream: scheduler,
                decoder: Decoder::new(limits),
                limits,
            },
        )
    }

    /// Runs [`serve_loop`] on its own thread and returns the join handle.
    ///
    /// [`serve_forever`] takes a concrete [`PhpEmbedRuntime`] — which needs a
    /// loaded `libphp` — so the loop's unit cases drive the same function
    /// through this seam with a stand-in.
    fn serve_in_background(
        worker: UnixStream,
        runtime: Arc<dyn StandIn>,
        limits: Limits,
    ) -> std::thread::JoinHandle<Result<(), ViewError>> {
        let serving = Arc::clone(&runtime);
        std::thread::spawn(move || {
            serve_loop(
                worker,
                limits,
                move |io| serving.serve(io),
                move || runtime.recycle(),
            )
        })
    }

    fn wait_for(peer: &mut Peer, wanted: usize) -> Vec<(u32, Frame)> {
        for _ in 0..200 {
            let frames = peer.drain();
            if frames.len() >= wanted {
                return frames;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the worker never wrote {wanted} frames");
    }

    #[test]
    fn a_socketpair_request_is_answered_and_closed_in_order() {
        // The whole transport: READY, REQ_HEAD/REQ_BODY/REQ_END in, RESP_HEAD,
        // RESP_BODY, RESP_END_COST and DONE out — with the CPU number the
        // runtime returned riding on the frame that ends the response.
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);

        let ready = wait_for(&mut peer, 1);
        assert!(matches!(ready.as_slice(), [(0, Frame::Ready)]), "{ready:?}");

        peer.send(7, Frame::ReqHead(ReqHead::default()));
        peer.send(7, Frame::ReqBody(b"hello".to_vec()));
        peer.send(7, Frame::ReqEnd);
        let frames = wait_for(&mut peer, 5);

        let kinds: Vec<&str> = frames
            .iter()
            .map(|(_, frame)| match frame {
                Frame::Window { .. } => "window",
                Frame::RespHead(_) => "head",
                Frame::RespBody(_) => "body",
                Frame::RespEndCost { .. } => "end",
                Frame::Done { .. } => "done",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            ["window", "head", "body", "end", "done"],
            "the request-body credit goes back before the response: {frames:?}"
        );
        assert!(matches!(frames[0].1, Frame::Window { bytes: 5 }));
        assert!(matches!(
            frames[2].1,
            Frame::RespBody(ref body) if body == b"hello"
        ));
        assert!(matches!(
            frames[3].1,
            Frame::RespEndCost { cost } if cost.cpu_ns == Some(7) && cost.wall_ns > 0
        ));
        assert!(matches!(
            frames[4].1,
            Frame::Done {
                outcome: DoneOutcome::Completed
            }
        ));
        assert_eq!(
            frames[4].0, 7,
            "every response frame carries the request id"
        );

        // The channel ends: the worker returns cleanly and does not hang.
        drop(peer.stream);
        handle.join().expect("worker thread").expect("clean end");
    }

    #[test]
    fn a_large_response_waits_for_window_frames_instead_of_deadlocking() {
        // The transport is single-threaded, so a body past the initial credit
        // must read its own WINDOW: one byte more than the window is the
        // smallest honest version of that path.
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let body = vec![0x5au8; INITIAL_WINDOW as usize + 4096];
        struct Big(Vec<u8>);
        impl StandIn for Big {
            fn serve(&self, io: &mut dyn RequestIo) -> RequestEnd {
                let _ = io.write_head(RespHead::default());
                if io.write_body(&self.0).is_err() {
                    return RequestEnd::new(DoneOutcome::Aborted, None);
                }
                RequestEnd::new(DoneOutcome::Completed, None)
            }
            fn recycle(&self) -> bool {
                false
            }
        }
        let runtime = Arc::new(Big(body.clone()));
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);

        peer.send(1, Frame::ReqHead(ReqHead::default()));
        peer.send(1, Frame::ReqEnd);
        // Wait for the head, then spend the window down to nothing and grant.
        let mut rebuilt = Vec::new();
        for _ in 0..200 {
            for (_, frame) in peer.drain() {
                match frame {
                    Frame::RespBody(chunk) => rebuilt.extend_from_slice(&chunk),
                    Frame::Done { .. } => {}
                    _ => {}
                }
            }
            if rebuilt.len() == body.len() {
                break;
            }
            peer.send(1, Frame::Window { bytes: 65536 });
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(rebuilt.len(), body.len(), "the body never completed");
        drop(peer.stream);
        handle.join().expect("worker thread").expect("clean end");
    }

    #[test]
    fn a_cancelled_request_aborts_but_the_worker_serves_the_next_one() {
        // A client disconnect (CANCEL) is an ordinary event: the response is
        // reported aborted, `RESP_END_COST` + `DONE` still go out so the
        // scheduler frees the slot, a stale `CANCEL` arriving after `DONE`
        // (no request in flight) is dropped as bookkeeping, and the *same*
        // worker serves the next request. Retiring on either path would cost a
        // fork and a `php_module_startup` per disconnect.
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, Arc::clone(&runtime) as Arc<dyn StandIn>, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(9, Frame::ReqHead(ReqHead::default()));
        peer.send(9, Frame::Cancel);

        let mut frames = Vec::new();
        for _ in 0..200 {
            frames.extend(peer.drain());
            let dones = frames
                .iter()
                .filter(|(_, frame)| matches!(frame, Frame::Done { .. }))
                .count();
            if dones >= 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            !frames
                .iter()
                .any(|(id, frame)| *id == 9 && matches!(frame, Frame::RespHead(_))),
            "an abandoned response must not reach the channel: {frames:?}"
        );
        assert!(
            matches!(
                frames.as_slice(),
                [
                    (9, Frame::RespEndCost { .. }),
                    (
                        9,
                        Frame::Done {
                            outcome: DoneOutcome::Aborted
                        }
                    ),
                ]
            ),
            "RESP_END_COST must precede DONE(Aborted) for the cancelled request: {frames:?}"
        );

        // Now no request is in flight: a stale `CANCEL` for the already-DONE
        // request (the scheduler's `complete` view lagged) is bookkeeping, not
        // a protocol error.
        peer.send(9, Frame::Cancel);

        // The same worker (the thread never exited) answers a second request
        // with a full `RESP_HEAD`..`DONE(Completed)` sequence — even if a
        // stale `CANCEL(9)` arrives *after* `REQ_HEAD(10)` while request 10 is
        // reading its body.
        peer.send(10, Frame::ReqHead(ReqHead::default()));
        peer.send(9, Frame::Cancel);
        peer.send(10, Frame::ReqBody(b"still alive".to_vec()));
        peer.send(10, Frame::ReqEnd);
        let mut second = Vec::new();
        for _ in 0..200 {
            second.extend(peer.drain());
            if second
                .iter()
                .any(|(id, frame)| *id == 10 && matches!(frame, Frame::Done { .. }))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            matches!(
                second.as_slice(),
                [
                    (10, Frame::Window { bytes: 11 }),
                    (10, Frame::RespHead(head)),
                    (10, Frame::RespBody(body)),
                    (10, Frame::RespEndCost { cost }),
                    (
                        10,
                        Frame::Done {
                            outcome: DoneOutcome::Completed
                        }
                    ),
                ] if head.status == 200 && body == b"still alive" && cost.cpu_ns == Some(7)
            ),
            "the next request was not served in full after a stale CANCEL: {second:?}"
        );
        assert_eq!(*lock(&runtime.requests), 2);

        drop(peer.stream);
        handle.join().expect("worker thread").expect("clean end");
    }

    #[test]
    fn a_request_without_a_stream_is_a_protocol_error() {
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(0, Frame::ReqHead(ReqHead::default()));
        drop(peer.stream);
        assert!(handle.join().expect("worker thread").is_err());
    }

    #[test]
    fn a_stray_body_frame_names_only_its_kind_and_never_leaks_bytes() {
        // Security hygiene: protocol-error strings reach the view's stderr, so
        // a stray frame carrying payload bytes must name `frame_kind` only and
        // never interpolate `Debug` bytes (passwords/PII). Non-`Cancel` foreign
        // frames also remain strict protocol errors.
        let secret = b"s3cr3t-client-password".to_vec();
        let debug_bytes = format!("{secret:?}");
        let limits = Limits::default();

        // 1. Stray `REQ_BODY` while idle (before `REQ_HEAD`).
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(1, Frame::ReqBody(secret.clone()));
        drop(peer.stream);
        let idle_err = handle
            .join()
            .expect("worker thread")
            .expect_err("stray REQ_BODY before REQ_HEAD must fail")
            .to_string();
        assert!(idle_err.contains("REQ_BODY"), "{idle_err}");
        assert!(!idle_err.contains("s3cr3t"), "leaked text: {idle_err}");
        assert!(!idle_err.contains(&debug_bytes), "leaked bytes: {idle_err}");
        assert!(!idle_err.contains("115"), "leaked byte value: {idle_err}");

        // 2. Stray `RESP_BODY` while reading a request body.
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(2, Frame::ReqHead(ReqHead::default()));
        peer.send(2, Frame::RespBody(secret.clone()));
        drop(peer.stream);
        let body_err = handle
            .join()
            .expect("worker thread")
            .expect_err("stray RESP_BODY during request must fail")
            .to_string();
        assert!(body_err.contains("RESP_BODY"), "{body_err}");
        assert!(!body_err.contains("s3cr3t"), "leaked text: {body_err}");
        assert!(!body_err.contains(&debug_bytes), "leaked bytes: {body_err}");
        assert!(!body_err.contains("115"), "leaked byte value: {body_err}");

        // 3. Denied twin for stale-`CANCEL` exemption: a foreign `REQ_BODY`
        // (`id != self.id`) is still a strict protocol error (and leaks no bytes).
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Echo {
            requests: Mutex::new(0),
        });
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(2, Frame::ReqHead(ReqHead::default()));
        peer.send(99, Frame::ReqBody(secret));
        drop(peer.stream);
        let foreign_err = handle
            .join()
            .expect("worker thread")
            .expect_err("foreign REQ_BODY must remain a protocol error")
            .to_string();
        assert!(
            foreign_err.contains("request 99") && foreign_err.contains("request 2"),
            "{foreign_err}"
        );
        assert!(!foreign_err.contains("s3cr3t"), "{foreign_err}");
        assert!(!foreign_err.contains(&debug_bytes), "{foreign_err}");
    }

    /// A worker configuration for a fixture entry, pinned the way a boot pins
    /// one: `(st_dev, st_ino)`, `st_size` and `st_ctime` of the file the
    /// configuration names.
    fn config(
        entry: PathBuf,
        document_root: PathBuf,
        script_name: &str,
        ini: &str,
        view_tmp: PathBuf,
    ) -> PhpRuntimeConfig {
        let pin = EntryPin::of(&std::fs::metadata(&entry).unwrap_or_else(|err| {
            panic!("the fixture entry {} must exist: {err}", entry.display())
        }));
        PhpRuntimeConfig {
            route: "app".to_string(),
            entry,
            document_root,
            script_name: script_name.to_string(),
            ini: ini.to_string(),
            view_tmp,
            max_requests: 100,
            max_body_bytes: 1024 * 1024,
            pin,
        }
    }

    #[test]
    fn a_configuration_that_could_not_serve_is_refused_both_ways() {
        let dir = std::env::temp_dir().join(format!("blastit-php-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let entry = dir.join("index.php");
        std::fs::write(&entry, b"<?php echo 'x';").expect("entry");
        let good = config(
            entry.clone(),
            dir.clone(),
            "",
            "memory_limit=256M\n",
            dir.clone(),
        );
        assert!(good.validate().is_ok(), "the accepted twin must pass");

        let missing = PhpRuntimeConfig {
            entry: dir.join("nope.php"),
            ..good.clone()
        };
        assert!(missing.validate().is_err(), "a missing entry is refused");
        let relative = PhpRuntimeConfig {
            entry: PathBuf::from("public/index.php"),
            ..good.clone()
        };
        // The refusal names the directive the path came from, as a `Blastfile`
        // writes it, and never the retired per-route table.
        let refusal = relative
            .validate()
            .expect_err("a relative entry is refused");
        assert_eq!(
            refusal,
            "entry public/index.php must be an absolute path (the path `entrypoint php` \
             resolved to)"
        );
        assert!(!refusal.contains("[route."), "{refusal}");
        let no_ini = PhpRuntimeConfig {
            ini: String::new(),
            ..good.clone()
        };
        assert!(no_ini.validate().is_err(), "an empty INI is refused");
        let zero = PhpRuntimeConfig {
            max_requests: 0,
            ..good.clone()
        };
        assert!(zero.validate().is_err(), "max_requests = 0 is refused");
        let no_body_budget = PhpRuntimeConfig {
            max_body_bytes: 0,
            ..good.clone()
        };
        assert!(
            no_body_budget.validate().is_err(),
            "a zero body budget is refused"
        );
        let no_tmp = PhpRuntimeConfig {
            view_tmp: dir.join("missing-tmp"),
            ..good
        };
        assert!(
            no_tmp.validate().is_err(),
            "a missing view_tmp directory is refused"
        );
        let relative_tmp = PhpRuntimeConfig {
            view_tmp: PathBuf::from("tmp"),
            ..no_tmp
        };
        assert!(
            relative_tmp.validate().is_err(),
            "a relative view_tmp is refused"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_non_library_is_a_boot_refusal_and_never_a_request_fault() {
        let dir = std::env::temp_dir().join(format!("blastit-php-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let entry = dir.join("index.php");
        std::fs::write(&entry, b"<?php echo 'x';").expect("entry");
        let fake = dir.join("libphp.so");
        std::fs::write(&fake, b"not a library").expect("fake");
        let err = PhpEmbedRuntime::load(
            SharedObject::Path(fake),
            SharedObject::Path(dir.join("shim.so")),
            {
                let mut cfg = config(entry, dir.clone(), "", "memory_limit=256M\n", dir.clone());
                cfg.max_requests = 1;
                cfg.max_body_bytes = 1024;
                cfg
            },
        )
        .err()
        .expect("a text file cannot load");
        assert!(
            matches!(err, PhpError::Load(LoadError::NotALibrary { .. })),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_output_line_is_capped_on_a_character_boundary() {
        // A `echo str_repeat('é', 5000)` outside the response must not split a
        // character (a panic) or bury the log (the contract's 2048 bytes).
        let ascii = "x".repeat(OUTPUT_LINE_CAP + 10);
        assert_eq!(capped_line(&ascii).len(), OUTPUT_LINE_CAP);
        let wide = "é".repeat(OUTPUT_LINE_CAP);
        let capped = capped_line(&wide);
        assert!(capped.len() <= OUTPUT_LINE_CAP);
        assert_eq!(capped.len() % 2, 0, "a two-byte character was split");
        assert_eq!(capped_line("short"), "short");
    }

    #[test]
    fn the_php_environment_never_carries_http_proxy() {
        // httpoxy, at the seam that builds `$_SERVER`: a `Proxy:` header is
        // dropped before it could ever become `HTTP_PROXY`.
        let head = ReqHead {
            method: b"GET".to_vec(),
            target: b"/app/x?y=1".to_vec(),
            headers: vec![
                (b"Proxy".to_vec(), b"http://attacker.example".to_vec()),
                (b"Host".to_vec(), b"app.test".to_vec()),
            ],
            ..ReqHead::default()
        };
        // `Prepared` reads only the values a request is built from, so the
        // record here is present but never compared: this case is about the
        // environment, not about the pin (which has its own cases).
        let cfg = PhpRuntimeConfig {
            route: "app".to_string(),
            entry: PathBuf::from("/srv/app/index.php"),
            document_root: PathBuf::from("/srv/app"),
            script_name: "/app".to_string(),
            ini: "memory_limit=256M\n".to_string(),
            view_tmp: PathBuf::from("/tmp"),
            max_requests: 1,
            max_body_bytes: 1024,
            pin: EntryPin {
                dev: 0,
                ino: 0,
                size: 0,
                ctime_s: 0,
                ctime_ns: 0,
            },
        };
        let prepared = Prepared::new(&cfg, &head).expect("prepared");
        let names: Vec<&str> = prepared.vars.iter().map(|var| var.name()).collect();
        assert!(names.contains(&"REQUEST_METHOD"), "{names:?}");
        assert!(names.contains(&"QUERY_STRING"), "{names:?}");
        assert!(names.contains(&"HTTP_HOST"), "{names:?}");
        assert!(!names.contains(&"HTTP_PROXY"), "httpoxy: {names:?}");
        assert_eq!(
            prepared.query_string.as_ref().map(|q| q.to_str().unwrap()),
            Some("y=1")
        );
        // The CGI triple, from the same request: the URI stays whole, the
        // mount point is `SCRIPT_NAME`, and `PATH_INFO` is the remainder.
        let value = |name: &str| {
            prepared
                .vars
                .iter()
                .find(|var| var.name() == name)
                .map(|var| var.value().to_owned())
                .unwrap_or_else(|| panic!("{name} is missing"))
        };
        assert_eq!(value("REQUEST_URI"), "/app/x?y=1");
        assert_eq!(value("SCRIPT_NAME"), "/app");
        assert_eq!(value("PATH_INFO"), "/x");
        assert_eq!(value("QUERY_STRING"), "y=1");
    }

    #[test]
    fn the_mount_point_is_stripped_only_at_a_segment_boundary() {
        // The WSGI view and the router apply this rule; PHP's `PATH_INFO` must
        // not disagree with them.
        assert_eq!(strip_mount("/app", "/app"), "/");
        assert_eq!(strip_mount("/app", "/app/list"), "/list");
        assert_eq!(strip_mount("/app", "/application"), "/application");
        assert_eq!(strip_mount("/", "/anything"), "/anything");
        assert_eq!(strip_mount("", "/anything"), "/anything");
    }

    /// A `RequestIo` with a scripted body, recording what the response wrote.
    struct FakeIo {
        chunks: std::collections::VecDeque<Vec<u8>>,
        head: ReqHead,
        head_written: Option<RespHead>,
        body: Vec<u8>,
        lines: Vec<String>,
    }

    impl FakeIo {
        fn with_body(chunks: Vec<Vec<u8>>) -> Self {
            Self {
                chunks: chunks.into(),
                head: ReqHead::default(),
                head_written: None,
                body: Vec::new(),
                lines: Vec::new(),
            }
        }
    }

    impl RequestIo for FakeIo {
        fn head(&self) -> &ReqHead {
            &self.head
        }
        fn read_body(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let Some(chunk) = self.chunks.front_mut() else {
                return Ok(0);
            };
            let n = buf.len().min(chunk.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            chunk.drain(..n);
            if chunk.is_empty() {
                self.chunks.pop_front();
            }
            Ok(n)
        }
        fn write_head(&mut self, head: RespHead) -> io::Result<()> {
            self.head_written = Some(head);
            Ok(())
        }
        fn write_body(&mut self, data: &[u8]) -> io::Result<()> {
            self.body.extend_from_slice(data);
            Ok(())
        }
        fn output(&mut self, _stream: OutputStream, line: &str) {
            self.lines.push(line.to_owned());
        }
        fn flush_response(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_contained_bailout_is_its_own_done_outcome() {
        // `MB.3.41`: the shim is the only place that knows a `zend_bailout`
        // was trapped (`contained`), and `DONE` is the only frame that carries
        // how the handler ended. A script's own `500` is `Completed` (it
        // returned) — the two must never collapse into one outcome.
        let ok = |status: i32| ResultV1 {
            outcome: OUTCOME_OK,
            status,
            contained: 0,
            reserved: 0,
            cpu_ns: 7,
            body_bytes: 0,
        };
        let bailed = |contained: i32| ResultV1 {
            outcome: OUTCOME_BAILOUT,
            status: 500,
            contained,
            reserved: 0,
            cpu_ns: 7,
            body_bytes: 0,
        };

        // The app answered `500` and returned: an ordinary completion.
        assert_eq!(ended(&ok(500), true), (DoneOutcome::Completed, false));
        // A contained `exit()`/`die()`/fatal: the handler never returned.
        assert_eq!(ended(&bailed(1), true), (DoneOutcome::Bailed, true));
        // A shim that reports a bailout it did not contain is a shim this side
        // does not trust: it stays `Errored` (and retires), never `Bailed`.
        assert_eq!(ended(&bailed(0), true), (DoneOutcome::Errored, true));
        // A script that answered nothing at all is a failure, not a bailout.
        assert_eq!(ended(&ok(0), false), (DoneOutcome::Errored, false));
    }

    #[test]
    fn a_request_body_past_the_worker_budget_is_refused() {
        // The accepted twin first: the same body fits a larger budget.
        let mut io = FakeIo::with_body(vec![vec![0x41; 64], vec![0x42; 64]]);
        let body = read_body(&mut io, 1024).expect("128 bytes fit");
        assert_eq!(body.len(), 128);

        let mut io = FakeIo::with_body(vec![vec![0x41; 64], vec![0x42; 64]]);
        let err = read_body(&mut io, 127).expect_err("128 > 127");
        assert!(matches!(err, BodyError::TooLarge));
    }

    #[test]
    fn a_bailed_worker_retires_and_max_requests_still_applies() {
        // The rule `recycle()` implements, without a loaded libphp.
        assert!(should_recycle(true, 0, 100), "a bailout retires at once");
        assert!(should_recycle(false, 3, 3), "max_requests reached");
        assert!(!should_recycle(false, 2, 3), "one more is allowed");
    }

    #[test]
    fn a_runtime_that_retires_ends_the_loop_after_its_done() {
        // `max_requests`/bailout exit: `serve_forever` returns `Ok` right after
        // the request's `DONE`, without waiting for the channel to close.
        struct Once(Mutex<u32>);
        impl StandIn for Once {
            fn serve(&self, io: &mut dyn RequestIo) -> RequestEnd {
                *lock(&self.0) += 1;
                let _ = io.write_head(RespHead::default());
                RequestEnd::new(DoneOutcome::Completed, None)
            }
            fn recycle(&self) -> bool {
                *lock(&self.0) >= 1
            }
        }
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(Once(Mutex::new(0)));
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(3, Frame::ReqHead(ReqHead::default()));
        peer.send(3, Frame::ReqEnd);
        // The response is head, cost and done; there is no body.
        let frames = wait_for(&mut peer, 3);
        assert!(
            frames
                .iter()
                .any(|(_, frame)| matches!(frame, Frame::Done { .. })),
            "{frames:?}"
        );
        // The scheduler's end is still open, but the worker has retired.
        assert!(handle.join().expect("worker thread").is_ok());
    }

    #[test]
    fn a_response_body_before_the_head_is_a_protocol_error() {
        struct BodyFirst;
        impl StandIn for BodyFirst {
            fn serve(&self, io: &mut dyn RequestIo) -> RequestEnd {
                let _ = io.write_body(b"without a head");
                RequestEnd::new(DoneOutcome::Errored, None)
            }
            fn recycle(&self) -> bool {
                false
            }
        }
        let limits = Limits::default();
        let (worker, mut peer) = pair(limits);
        let runtime = Arc::new(BodyFirst);
        let handle = serve_in_background(worker, runtime, limits);
        let _ = wait_for(&mut peer, 1);
        peer.send(4, Frame::ReqHead(ReqHead::default()));
        // Order, do not tolerate: the head-less body is refused the moment the
        // head arrives, and the transport then ends the whole view, so joining
        // it here is what makes the next write deterministic instead of a race
        // between this thread and the worker's retirement. The refusal is
        // asserted by name, and the write after it is *expected* to find the
        // peer gone — that closed channel is the evidence the view ended, not a
        // flake (`MB.3.41`).
        //
        // This path reports no `DoneOutcome`: `serve_forever` takes the
        // recorded protocol error and returns it before it can write
        // `RESP_END_COST`/`DONE`, which is why the half that is asserted is the
        // returned [`ViewError::Protocol`] and the closed peer.
        let ended = handle.join().expect("worker thread");
        // By type **and** message (`MB.3.45`): a rewording of some other error,
        // or any other `ViewError` variant, cannot satisfy this.
        assert!(
            matches!(&ended, Err(ViewError::Protocol(msg)) if msg == "RESP_BODY before RESP_HEAD"),
            "a body before the head is the transport's protocol error: {ended:?}"
        );
        let gone = peer.send_to_a_gone_peer(4, Frame::ReqEnd);
        assert!(
            gone.is_err(),
            "the view ended, so its end of the channel is closed: {gone:?}"
        );
    }

    #[test]
    fn a_request_body_past_the_flow_window_is_a_protocol_error() {
        // The scheduler may not send more body than the window allows, and the
        // view must refuse the frame instead of buffering it.
        let limits = Limits::default();
        let (mut ours, mut peer) = UnixStream::pair().expect("socketpair");
        peer.set_nonblocking(true).expect("non-blocking");
        let writer = Writer::new(ours.try_clone().expect("clone"), limits);
        let flow = Flow::new();
        assert!(
            flow.body_received(INITIAL_WINDOW as usize),
            "spend the whole window"
        );
        let mut decoder = Decoder::new(limits);
        let mut buf = [0u8; 1024];
        let mut io = ChanIo {
            head: ReqHead::default(),
            id: 5,
            writer: &writer,
            flow,
            chan: &mut ours,
            decoder: &mut decoder,
            buf: &mut buf,
            pending: Vec::new(),
            ended: false,
            head_sent: false,
            cancelled: false,
            error: None,
        };
        let mut frames = Vec::new();
        limits
            .encode(&Frame::ReqBody(b"x".to_vec()), 5, &mut frames)
            .expect("encode");
        peer.write_all(&frames).expect("write");
        assert!(io.read_body(&mut [0u8; 8]).is_err());
        assert!(io.error.is_some(), "the refusal is remembered");
    }

    #[test]
    fn a_header_burst_past_the_cap_aborts_the_response() {
        let mut io = FakeIo::with_body(Vec::new());
        let mut sink = PhpSink {
            io: &mut io,
            headers: Vec::new(),
            header_bytes: 0,
            head_sent: false,
            abort: false,
        };
        // The accepted twin: a normal header block.
        for _ in 0..MAX_HEADERS {
            assert!(sink.head(200, Some(b"X-Ok"), b"1"));
        }
        // The (MAX_HEADERS + 1)th is refused, and so is a single oversized one.
        assert!(!sink.head(200, Some(b"X-Too-Many"), b"1"));
        assert!(sink.abort);
        let mut sink = PhpSink {
            io: &mut io,
            headers: Vec::new(),
            header_bytes: 0,
            head_sent: false,
            abort: false,
        };
        let big = vec![b'v'; MAX_HEADER_BYTES + 1];
        assert!(!sink.head(200, Some(b"X-Big"), &big));
        assert!(sink.abort);
        assert!(sink.headers.is_empty());
    }

    // ------------------------------------------------- the loaded-file report

    /// A temp directory of fixture files, removed with the directory.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("blastit-php-report-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn write(&self, name: &str, contents: &str) -> String {
            let path = self.0.join(name);
            std::fs::write(&path, contents).expect("fixture");
            canonical(&path.display().to_string())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The canonical spelling of a fixture path, which is what the report must
    /// carry (the sink and the map both canonicalise).
    fn canonical(path: &str) -> String {
        std::fs::canonicalize(path)
            .expect("the fixture exists")
            .display()
            .to_string()
    }

    fn report_datagrams(peer: &UnixDatagram) -> Vec<blastit_view::loaded::Batch> {
        peer.set_nonblocking(true).expect("non-blocking peer");
        let mut out = Vec::new();
        let mut buf = [0u8; blastit_view::loaded::MAX_BATCH_BYTES];
        loop {
            match peer.recv(&mut buf) {
                Ok(n) => {
                    out.push(blastit_view::loaded::decode(&buf[..n]).expect("the codec's own wire"))
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => return out,
                Err(err) => panic!("recv: {err}"),
            }
        }
    }

    #[test]
    fn the_report_is_a_boot_then_one_delta_of_sorted_absolute_paths() {
        let scratch = Scratch::new("report");
        let included = scratch.write("included.php", "<?php\n");
        let helper = scratch.write("helper.php", "<?php\n");
        let (ours, peer) = UnixDatagram::pair().expect("socketpair");
        let report = PhpFileReport::new(LoadedReport {
            socket: ours,
            source: 7,
        })
        .expect("a non-blocking reporter");
        // The sink the shim would report into. In a worker the loader owns it
        // (`Loaded::install_file_sink` takes it by value); here it is simply the
        // cell these requests fill.
        let sink = FileSink::new();

        // What the shim's snapshot looks like: one path per included file, in
        // PHP's own order. The relative and non-UTF-8 entries are the refused
        // half — `EG(included_files)` is resolved, but a future shim bug must
        // not put a relative path on the wire, and the codec could not carry a
        // path that is not UTF-8 at all.
        report.begin(&sink);
        sink.record(helper.as_bytes());
        sink.record(b"relative/thing.php");
        sink.record(b"/not/utf8/\xff.php");
        sink.record(included.as_bytes());
        sink.record(included.as_bytes());
        report.finish(&sink);

        let frames = report_datagrams(&peer);
        assert_eq!(frames.len(), 1, "one boot datagram");
        let batch = &frames[0];
        assert!(batch.boot, "a view's first report replaces its set");
        assert_eq!(batch.seq, 0);
        assert_eq!(batch.source, 7);
        assert!(batch.paths.iter().all(|path| path.starts_with('/')));
        let mut sorted = batch.paths.clone();
        sorted.sort();
        assert_eq!(batch.paths, sorted, "the report is sorted");
        let mut wanted = vec![included, helper.clone()];
        wanted.sort();
        assert_eq!(batch.paths, wanted, "each file once, deduplicated");

        // The second request is a delta, and an empty one sends nothing.
        report.begin(&sink);
        report.finish(&sink);
        assert!(
            report_datagrams(&peer).is_empty(),
            "nothing loaded is not a report"
        );
        report.begin(&sink);
        sink.record(helper.as_bytes());
        report.finish(&sink);
        let frames = report_datagrams(&peer);
        assert_eq!(frames.len(), 1);
        assert!(!frames[0].boot, "only the first report is a boot");
        assert_eq!(frames[0].seq, 1, "the stream continues");
        assert_eq!(frames[0].paths, vec![helper]);
    }

    #[test]
    fn a_compiled_artefact_reports_the_template_it_was_built_from() {
        let scratch = Scratch::new("compiled");
        let template = scratch.write("index.twig", "{{ x }}");
        // A Twig-compiled artefact: the source path is a literal of
        // `getSourceContext()`, which is the marker the map reads.
        let compiled = scratch.write(
            "compiled.php",
            &format!(
                "<?php\nclass __TwigTemplate_1 {{\n  public function getSourceContext(): Source\n  {{\n    return new Source(\"\", \"index.twig\", \"{template}\");\n  }}\n}}\n"
            ),
        );
        let blade_template = scratch.write("welcome.blade.php", "<h1>hi</h1>");
        let blade = scratch.write(
            "blade.php",
            &format!("<?php /**PATH {blade_template} ENDPATH**/ ?>\n<h1>hi</h1>\n"),
        );
        let plain = scratch.write("plain.php", "<?php echo 1;\n");

        let (ours, peer) = UnixDatagram::pair().expect("socketpair");
        let report = PhpFileReport::new(LoadedReport {
            socket: ours,
            source: 3,
        })
        .expect("a non-blocking reporter");
        let sink = FileSink::new();
        report.begin(&sink);
        for path in [&compiled, &blade, &plain] {
            sink.record(path.as_bytes());
        }
        report.finish(&sink);

        let frames = report_datagrams(&peer);
        assert_eq!(frames.len(), 1);
        let mut wanted = vec![compiled, blade, plain, template, blade_template];
        wanted.sort();
        assert_eq!(
            frames[0].paths, wanted,
            "a compiled artefact brings its template source, and plain PHP brings nothing"
        );
    }

    #[test]
    fn the_source_markers_are_read_from_the_artefacts_text() {
        // Blade: the header of a compiled view.
        assert_eq!(
            compiled_sources("<?php /**PATH /srv/app/views/a.blade.php ENDPATH**/ ?>"),
            vec!["/srv/app/views/a.blade.php".to_owned()]
        );
        // Twig: only the absolute literal of `getSourceContext()` counts; the
        // template *name* is relative, and relative is never a file to watch.
        let twig = "class T { public function getSourceContext(): Source { \
                    return new Source(\"\", \"index.twig\", \"/srv/app/templates/index.twig\"); } }";
        assert_eq!(
            compiled_sources(twig),
            vec!["/srv/app/templates/index.twig".to_owned()]
        );
        // Ordinary PHP names no template at all, which is the common case.
        assert!(compiled_sources("<?php\nforeach ($rows as $row) { echo $row; }\n").is_empty());
        // And an unterminated marker is not a path: the scan stops rather than
        // taking the rest of the file.
        assert!(compiled_sources("<?php /**PATH /srv/app/a.blade.php").is_empty());
    }

    /// The map reads each artefact once: a memo hit is still a report after the
    /// file is gone, which is the only way a test can see the difference
    /// without timing the read.
    #[test]
    fn the_compiled_to_source_map_is_memoised_per_artefact() {
        let scratch = Scratch::new("memo");
        let template = scratch.write("page.twig", "{{ y }}");
        let compiled = scratch.write(
            "compiled.php",
            &format!(
                "<?php // getSourceContext\nreturn new Source('', 'page.twig', '{template}');\n"
            ),
        );
        let mut memo = HashMap::new();
        let first = template_sources(std::slice::from_ref(&compiled), &mut memo);
        assert_eq!(first, vec![template.clone()]);
        assert_eq!(memo.len(), 1, "the artefact is remembered");

        std::fs::remove_file(&compiled).expect("remove the artefact");
        assert_eq!(
            template_sources(&[compiled], &mut memo),
            vec![template],
            "a memo hit is still reported after the file is unreadable"
        );
    }

    /// The bound is on the memo itself: a full map remembers nothing new, while
    /// the artefact in hand is still reported.
    ///
    /// `!sources.is_empty() || memo.len() < MAX_MAP_ENTRIES` inserted compiled
    /// artefacts past the cap (an empty entry is still an entry), so a
    /// long-lived view grew this map for every artefact it ever met.
    #[test]
    fn the_compiled_to_source_map_memo_stops_at_its_bound() {
        let scratch = Scratch::new("memo-bound");
        let template = scratch.write("bounded.twig", "{{ z }}");
        let compiled = scratch.write(
            "compiled.php",
            &format!("<?php /**PATH {template} ENDPATH**/ ?>\n<p>hi</p>\n"),
        );

        // A memo already at its bound: the miss is still reported, and the memo
        // does not grow — with a compiled artefact in hand, which is exactly the
        // case the old condition let through.
        let mut memo: HashMap<PathBuf, Vec<String>> = HashMap::new();
        for index in 0..MAX_MAP_ENTRIES {
            memo.insert(PathBuf::from(format!("/nonexistent/{index}")), Vec::new());
        }
        assert_eq!(memo.len(), MAX_MAP_ENTRIES);
        assert_eq!(
            template_sources(std::slice::from_ref(&compiled), &mut memo),
            vec![template.clone()],
            "a miss past the bound is still reported"
        );
        assert_eq!(
            memo.len(),
            MAX_MAP_ENTRIES,
            "the memo never grows past its bound"
        );
        assert!(
            !memo.contains_key(Path::new(&compiled)),
            "nothing is remembered once the memo is full"
        );

        // Below the bound it still remembers, which is what keeps a steady-state
        // view from rescanning.
        let mut memo = HashMap::new();
        template_sources(std::slice::from_ref(&compiled), &mut memo);
        assert_eq!(memo.len(), 1, "there is room, so the artefact is memoised");
    }
}
