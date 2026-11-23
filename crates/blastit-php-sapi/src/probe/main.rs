#![deny(unsafe_code)]
#![deny(missing_docs)]
// The probe is a test harness: it is allowed the crate's one `unsafe` module
// (`ffi.rs`) and nothing else. Assertions return `Err`, never `panic!`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The real load: `dlopen(libphp)`, `dlopen(shim)`, five requests, one JSON line.
//!
//! Run by `make php-sapi-e2e` from inside `e2e/php-sapi/` (debian:trixie-slim +
//! `php8.4-dev` + `libphp8.4-embed`), never by `make check`: a PHP-free host can
//! compile this binary, and it would fail at the first `dlopen` there.
//!
//! What it asserts, in order (the numbers are the node's "Container + probe"):
//!
//! 1. `dlopen(shim)` **before** `libphp` is refused with `undefined symbol`.
//! 2. `dlopen(libphp, RTLD_NOW|RTLD_GLOBAL)` then `dlopen(shim,
//!    RTLD_NOW|RTLD_LOCAL)` succeeds; the ABI version is 1 and the shim's minor
//!    is one this build supports.
//! 3. `hello.php` returns status 200, a `X-Probe: 1` head, the body
//!    `hello from php`, and `cpu_ns > 0 && cpu_ns <= wall_ns`.
//! 4. `expose_php=0` held: no `X-Powered-By` head is emitted.
//! 5. `vars.php` reports that `PHP_VALUE`, `PHP_ADMIN_VALUE`, `REDIRECT_STATUS`
//!    and `HTTP_PROXY` — force-injected into the request's server-variable
//!    array, bypassing the Rust type — never reach `$_SERVER`; and that
//!    `SCRIPT_FILENAME`, `QUERY_STRING` and `$_COOKIE` arrived.
//! 6. `exit.php` (`echo` then `exit(3)`) returns a status with the body already
//!    sent, contained; `fatal.php` (undefined function) returns status 500,
//!    contained.
//! 7. A **second** `hello.php`, after both, still returns 200 and the body: the
//!    `zend_first_try` guard left the worker able to serve.

#[allow(unsafe_code)]
mod ffi;

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use blastit_php_sapi::abi::{
    ConfigV1, OUTCOME_BAILOUT, OUTCOME_OK, RequestV1, ResultV1, ServerVarV1,
};
use blastit_php_sapi::{
    ABI_VERSION, Egress, IniPolicy, SUPPORTED_MINORS, ServerVars, supported_minors_sentence,
};

/// One fixture's request and everything it produced.
struct Call {
    status: i32,
    outcome: i32,
    contained: i32,
    cpu_ns: u64,
    wall_ns: u64,
    head: Vec<(String, String)>,
    body: String,
    logs: Vec<String>,
}

impl Call {
    /// A header value, case-insensitive, if the response carried it.
    fn header(&self, name: &str) -> Option<&str> {
        self.head
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("php-sapi-probe: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `php-sapi-probe <libphp.so> <shim.so> [fixtures-dir]`.
fn run() -> Result<String, String> {
    let mut args = std::env::args_os().skip(1);
    let libphp = PathBuf::from(
        args.next()
            .ok_or("usage: php-sapi-probe <libphp.so> <shim.so> [fixtures-dir]")?,
    );
    let shim_path = PathBuf::from(
        args.next()
            .ok_or("usage: php-sapi-probe <libphp.so> <shim.so> [fixtures-dir]")?,
    );
    let fixtures = args
        .next()
        .map_or_else(|| PathBuf::from("/fixtures"), PathBuf::from);
    // `--egress denied|allowed`: which INI half this worker is built with. It is
    // an explicit argument (never a default) because it *is* the policy under
    // test: the driver runs the same probe three times to show that the INI
    // layer and the seccomp layer each do their own job.
    let mut egress = Egress::Denied;
    while let Some(flag) = args.next() {
        let Some(flag) = flag.to_str() else {
            return Err("a non-UTF-8 argument".to_owned());
        };
        match flag {
            "--egress" => {
                let value = args.next().ok_or("--egress needs `denied` or `allowed`")?;
                egress = match value.to_str() {
                    Some("denied") => Egress::Denied,
                    Some("allowed") => Egress::Allowed,
                    _ => return Err(format!("unknown --egress value {value:?}")),
                };
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }

    // 1. The linkage rule: the shim alone must be refused, not crash.
    let alone = ffi::probe_shim_alone(&shim_path);
    if !alone.starts_with("refused") {
        return Err(format!(
            "{} loaded before libphp; the shim must carry no DT_NEEDED and resolve \
             Zend/SAPI symbols only from a RTLD_GLOBAL libphp ({alone})",
            shim_path.display()
        ));
    }

    // 2. The real load.
    let shim = ffi::Shim::load(&libphp, &shim_path)?;
    let abi = shim.abi_version();
    if abi != ABI_VERSION {
        return Err(format!(
            "the shim reports blastit_php_abi version {abi}, this probe speaks {ABI_VERSION}"
        ));
    }
    let minor_code = shim.minor();
    let minor = SUPPORTED_MINORS
        .iter()
        .copied()
        .find(|minor| minor.accepts(minor_code))
        .ok_or_else(|| {
            format!(
                "the shim reports PHP minor {minor_code}; this build supports {}",
                supported_minors_sentence()
            )
        })?;

    // 3. The real lockdown, rendered by the Rust side under test.
    let view_tmp = PathBuf::from("/tmp/blastit-php-probe-view");
    let view_tmp_text = view_tmp.to_string_lossy().into_owned();
    std::fs::create_dir_all(&view_tmp)
        .map_err(|err| format!("cannot create {}: {err}", view_tmp.display()))?;
    let policy = IniPolicy::new(&fixtures, &view_tmp, 30, egress).map_err(|err| err.to_string())?;
    let ini = CString::new(policy.as_str()).map_err(|_| "the INI blob holds a NUL".to_owned())?;
    let view_tmp_c = CString::new(view_tmp.to_string_lossy().as_bytes())
        .map_err(|_| "the view temp path holds a NUL".to_owned())?;
    let config = ConfigV1 {
        ini: ini.as_ptr(),
        view_tmp: view_tmp_c.as_ptr(),
    };
    // Is the egress wall installed in *this* process? Asked before PHP starts,
    // and again after the requests: `1` is the seccomp refusal, `0` means no
    // filter. This is the layer-independent evidence; PHP's own error text for
    // `curl` is identical whether the socket was refused or nothing listened.
    let inet_socket_before = ffi::inet_socket_errno();

    // The ABI's only lockdown channel must be refused when absent, not treated
    // as "use the host's php.ini". `worker_init` returns before `sapi_startup`,
    // so the real init below is unaffected.
    let null_ini = ConfigV1 {
        ini: std::ptr::null(),
        view_tmp: view_tmp_c.as_ptr(),
    };
    let refused = shim.worker_init(&null_ini);
    if refused != -1 {
        return Err(format!(
            "worker_init with a NULL ini returned {refused}, expected -1 (the lockdown is the \
             ABI's only ini channel)"
        ));
    }

    let init = shim.worker_init(&config);
    if init != 0 {
        return Err(format!("blastit_php_worker_init returned {init}"));
    }

    let sink = ffi::Sink::new()?;
    let cwd_before =
        std::env::current_dir().map_err(|err| format!("cannot read the probe's cwd: {err}"))?;

    // 4. A plain request.
    let hello = call(
        &shim,
        &sink,
        &fixtures,
        "hello.php",
        &base_vars(&fixtures, "hello.php", None),
        None,
        None,
        &[],
    )?;
    if hello.status != 200 {
        return Err(format!("hello.php returned status {}", hello.status));
    }
    if hello.header("X-Probe") != Some("1") {
        return Err(format!(
            "hello.php sent no `X-Probe: 1` head (heads: {:?})",
            hello.head
        ));
    }
    if !hello.body.contains("hello from php") {
        return Err(format!("hello.php body was {:?}", hello.body));
    }
    if hello.cpu_ns == 0 || hello.cpu_ns > hello.wall_ns {
        return Err(format!(
            "cpu_ns {} is not in 1..=wall_ns {}",
            hello.cpu_ns, hello.wall_ns
        ));
    }
    if hello.header("X-Powered-By").is_some() {
        return Err("expose_php=0 did not take effect: X-Powered-By was sent".to_owned());
    }
    if hello.outcome != OUTCOME_OK || hello.contained != 0 {
        return Err(format!(
            "a clean script reported outcome {} contained {}",
            hello.outcome, hello.contained
        ));
    }

    // 5. The reserved names, force-injected past the Rust types.
    let smuggled = [
        ("PHP_VALUE", "allow_url_include=1"),
        ("PHP_ADMIN_VALUE", "auto_prepend_file=php://input"),
        ("REDIRECT_STATUS", "200"),
        ("HTTP_PROXY", "http://attacker.example:8080"),
    ];
    let vars = call(
        &shim,
        &sink,
        &fixtures,
        "vars.php",
        &base_vars(&fixtures, "vars.php", Some("a=1&b=2")),
        Some("probe=1234"),
        Some("a=1&b=2"),
        &smuggled,
    )?;
    if vars.status != 200 {
        return Err(format!("vars.php returned status {}", vars.status));
    }
    for key in [
        "php_value",
        "php_admin_value",
        "redirect_status",
        "http_proxy",
    ] {
        if !vars.body.contains(&format!("\"{key}\":\"absent\"")) {
            return Err(format!(
                "{key} reached $_SERVER (vars.php said {:?})",
                vars.body
            ));
        }
    }
    if !vars.body.contains("\"cookie\":\"1234\"") {
        return Err(format!(
            "read_cookies did not reach $_COOKIE (vars.php said {:?})",
            vars.body
        ));
    }
    if !vars.body.contains("\"query\":\"a=1&b=2\"") {
        return Err(format!(
            "QUERY_STRING did not arrive (vars.php said {:?})",
            vars.body
        ));
    }
    if !vars.body.contains("vars.php") {
        return Err(format!(
            "SCRIPT_FILENAME was not the pinned entry (vars.php said {:?})",
            vars.body
        ));
    }

    // 6. `exit()` and a fatal: a status, no unwinding, no dead worker.
    let exited = call(
        &shim,
        &sink,
        &fixtures,
        "exit.php",
        &base_vars(&fixtures, "exit.php", None),
        None,
        None,
        &[],
    )?;
    if exited.outcome != OUTCOME_BAILOUT || exited.contained != 1 {
        return Err(format!(
            "exit() was not trapped (outcome {} contained {})",
            exited.outcome, exited.contained
        ));
    }
    if exited.status != 200 || !exited.body.contains("bye from exit") {
        return Err(format!(
            "exit() lost the response (status {}, body {:?})",
            exited.status, exited.body
        ));
    }

    // The egress fixture: the URL wrapper the INI layer gates, and the two
    // clients that ignore it and open their own `socket(AF_INET)`. Nothing may
    // connect. Which *layer* refused each attempt is asserted per run by the
    // driver, which knows whether the real `SeccompRole::View { egress: false }`
    // filter was installed around this process before it started.
    let outbound = call(
        &shim,
        &sink,
        &fixtures,
        "egress.php",
        &base_vars(&fixtures, "egress.php", None),
        None,
        None,
        &[],
    )?;
    if outbound.status != 200 {
        return Err(format!("egress.php returned status {}", outbound.status));
    }
    // The effective value, from `ini_get`, not the string we rendered.
    let pinned = format!("\"allow_url_fopen\":\"{}\"", egress.allow_url_fopen());
    if !outbound.body.contains(&pinned) {
        return Err(format!(
            "allow_url_fopen was not pinned to {} (egress.php said {:?})",
            egress.allow_url_fopen(),
            outbound.body
        ));
    }
    // ...and the effective `disable_functions`, including the three names whose
    // absence let a script signal its process group (ADR-0010 §2).
    for function in [
        "pcntl_signal",
        "posix_kill",
        "posix_getpgid",
        "posix_setsid",
        "posix_setpgid",
    ] {
        if !outbound.body.contains(function) {
            return Err(format!(
                "disable_functions does not disable {function} (egress.php said {:?})",
                outbound.body
            ));
        }
    }
    if outbound.body.contains("\"connected\":true") {
        return Err(
            "an outbound attempt succeeded: a route must never reach the network here".to_owned(),
        );
    }
    let failed = outbound.body.matches("\"connected\":false").count();
    if failed != 3 {
        return Err(format!(
            "only {failed} of the 3 outbound attempts reported a failure (egress.php said {:?})",
            outbound.body
        ));
    }

    let fatal = call(
        &shim,
        &sink,
        &fixtures,
        "fatal.php",
        &base_vars(&fixtures, "fatal.php", None),
        None,
        None,
        &[],
    )?;
    if fatal.outcome != OUTCOME_BAILOUT || fatal.contained != 1 {
        return Err(format!(
            "an undefined function was not trapped (outcome {} contained {})",
            fatal.outcome, fatal.contained
        ));
    }
    if fatal.status != 500 {
        return Err(format!(
            "a contained fatal reported status {} instead of 500",
            fatal.status
        ));
    }
    if fatal.logs.is_empty() {
        return Err("a contained fatal produced no log frame".to_owned());
    }

    // 7. Re-entrancy: the next request still serves.
    let again = call(
        &shim,
        &sink,
        &fixtures,
        "hello.php",
        &base_vars(&fixtures, "hello.php", None),
        None,
        None,
        &[],
    )?;
    if again.status != 200 || !again.body.contains("hello from php") {
        return Err(format!(
            "the worker did not serve the request after exit()+fatal (status {}, body {:?})",
            again.status, again.body
        ));
    }
    if again.cpu_ns == 0 {
        return Err("cpu_ns was zero on the request after a bailout".to_owned());
    }

    // The worker's cwd is its private directory holding its granted proxy
    // sockets; `php_execute_script` must not chdir into the application tree.
    let cwd_after =
        std::env::current_dir().map_err(|err| format!("cannot read the probe's cwd: {err}"))?;
    if cwd_before != cwd_after {
        return Err(format!(
            "a request moved the worker's cwd from {} to {}",
            cwd_before.display(),
            cwd_after.display()
        ));
    }

    shim.worker_shutdown();

    let inet_socket_after = ffi::inet_socket_errno();

    Ok(json(&Report {
        abi,
        minor_code,
        minor_dotted: minor.dotted(),
        alone: &alone,
        hello_status: hello.status,
        hello_head: &hello.head,
        hello_body: &hello.body,
        hello_cpu_ns: hello.cpu_ns,
        hello_wall_ns: hello.wall_ns,
        vars_body: &vars.body,
        exit_status: exited.status,
        exit_contained: exited.contained,
        fatal_status: fatal.status,
        fatal_contained: fatal.contained,
        again_status: again.status,
        again_body: &again.body,
        cwd_preserved: cwd_before == cwd_after,
        egress_mode: match egress {
            Egress::Denied => "denied",
            Egress::Allowed => "allowed",
        },
        egress_body: &outbound.body,
        view_tmp: &view_tmp_text,
        inet_socket_before,
        inet_socket_after,
    }))
}

/// The `$_SERVER` entries every fixture request carries.
///
/// `QUERY_STRING` is here because PHP only sets it from this list — Zend calls
/// `register_server_variables` and never derives it from
/// `SG(request_info).query_string` (`php_variables.c` `php_register_server_variables`).
fn base_vars(fixtures: &Path, fixture: &str, query: Option<&str>) -> ServerVars {
    let mut vars = ServerVars::new();
    let entry = fixtures.join(fixture);
    let script = format!("/{fixture}");
    // The worker pins these; nothing here comes from a request header.
    let pinned = [
        ("SCRIPT_FILENAME", entry.to_string_lossy().into_owned()),
        ("DOCUMENT_ROOT", fixtures.to_string_lossy().into_owned()),
        ("SCRIPT_NAME", script.clone()),
        ("REQUEST_URI", script),
        ("REQUEST_METHOD", "GET".to_owned()),
        ("SERVER_PROTOCOL", "HTTP/1.1".to_owned()),
        ("SERVER_SOFTWARE", "blastit".to_owned()),
        ("REMOTE_ADDR", "127.0.0.1".to_owned()),
        ("REMOTE_PORT", "54321".to_owned()),
        ("SERVER_PORT", "80".to_owned()),
        ("SERVER_NAME", "probe.blastit.invalid".to_owned()),
        ("QUERY_STRING", query.unwrap_or("").to_owned()),
    ];
    for (name, value) in pinned {
        // A literal name and a path under the fixtures directory: neither can
        // be reserved, and a failure here means the fixture directory is unusable.
        if let Err(err) = vars.insert(name, &value) {
            eprintln!("php-sapi-probe: dropping {name}: {err}");
        }
    }
    vars
}

/// One request through the shim, with frames read from the socketpair.
#[allow(clippy::too_many_arguments)]
fn call(
    shim: &ffi::Shim,
    sink: &ffi::Sink,
    fixtures: &Path,
    fixture: &str,
    vars: &ServerVars,
    cookie: Option<&str>,
    query: Option<&str>,
    smuggled: &[(&str, &str)],
) -> Result<Call, String> {
    let entry = fixtures.join(fixture);
    let entry_c = cstring(&entry.to_string_lossy(), "the entry path")?;
    let document_root_c = cstring(&fixtures.to_string_lossy(), "the fixtures directory")?;
    let script_c = cstring(&format!("/{fixture}"), "the script name")?;
    let request_uri_c = match query {
        Some(query) => cstring(&format!("/{fixture}?{query}"), "the request URI")?,
        None => script_c.clone(),
    };
    let method_c = CString::new("GET").map_err(|_| "the method holds a NUL".to_owned())?;
    let query_c = query.map(|q| cstring(q, "the query string")).transpose()?;
    let cookie_c = cookie.map(|c| cstring(c, "the cookie")).transpose()?;

    // The validated entries, then the ones the Rust API refuses: they are
    // appended by hand, exactly as a compromised worker would, so the shim's own
    // filter is what is under test.
    let mut owned: Vec<(CString, CString)> = Vec::with_capacity(vars.len() + smuggled.len());
    for var in vars {
        owned.push((
            CString::new(var.name()).map_err(|_| "a name holds a NUL".to_owned())?,
            CString::new(var.value()).map_err(|_| "a value holds a NUL".to_owned())?,
        ));
    }
    for (name, value) in smuggled {
        owned.push((
            CString::new(*name).map_err(|_| "a name holds a NUL".to_owned())?,
            CString::new(*value).map_err(|_| "a value holds a NUL".to_owned())?,
        ));
    }
    let raw_vars: Vec<ServerVarV1> = owned
        .iter()
        .map(|(name, value)| ServerVarV1 {
            name: name.as_ptr(),
            name_len: name.as_bytes().len(),
            value: value.as_ptr(),
            value_len: value.as_bytes().len(),
        })
        .collect();

    let request = RequestV1 {
        entry: entry_c.as_ptr(),
        document_root: document_root_c.as_ptr(),
        script_name: script_c.as_ptr(),
        request_uri: request_uri_c.as_ptr(),
        query_string: query_c
            .as_ref()
            .map_or(std::ptr::null(), |query| query.as_ptr()),
        request_method: method_c.as_ptr(),
        content_type: std::ptr::null(),
        content_length: 0,
        cookie: cookie_c
            .as_ref()
            .map_or(std::ptr::null(), |cookie| cookie.as_ptr()),
        server_vars: raw_vars.as_ptr(),
        server_var_count: raw_vars.len(),
        body: std::ptr::null(),
        body_len: 0,
    };
    let mut result = ResultV1 {
        outcome: -1,
        status: -1,
        contained: -1,
        reserved: 0,
        cpu_ns: 0,
        body_bytes: 0,
    };
    let vtable = sink.vtable();

    let started = Instant::now();
    let rc = shim.request(&request, &vtable, &mut result);
    let wall_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    if rc != 0 {
        return Err(format!("blastit_php_request({fixture}) returned {rc}"));
    }
    drop(owned);

    let mut status = None;
    let mut head = Vec::new();
    let mut body = Vec::new();
    let mut logs = Vec::new();
    for frame in sink.drain()? {
        match frame {
            ffi::Frame::Head {
                status: frame_status,
                name,
                value,
            } => {
                if name.is_empty() {
                    status = Some(frame_status);
                } else {
                    head.push((name, value));
                }
            }
            ffi::Frame::Body(chunk) => body.extend_from_slice(&chunk),
            ffi::Frame::Log { level, line } => {
                // Bounded log lines are what the worker forwards to app.output.
                if line.len() > 2048 {
                    return Err(format!("{fixture}: a log line was {} bytes", line.len()));
                }
                logs.push(format!("{level}: {line}"));
            }
        }
    }
    let status = status.ok_or_else(|| format!("{fixture} sent no end-of-head frame"))?;
    if result.status != status {
        return Err(format!(
            "{fixture}: the result status {} disagrees with the head frame {status}",
            result.status
        ));
    }

    Ok(Call {
        status,
        outcome: result.outcome,
        contained: result.contained,
        cpu_ns: result.cpu_ns,
        wall_ns,
        head,
        body: String::from_utf8_lossy(&body).into_owned(),
        logs,
    })
}

/// A `CString` from a path/string, or an error naming what failed.
fn cstring(value: &str, what: &str) -> Result<CString, String> {
    CString::new(value.as_bytes()).map_err(|_| format!("{what} holds a NUL"))
}

/// Everything the driver asserts on, flattened into the one JSON line.
struct Report<'a> {
    abi: u32,
    minor_code: u32,
    minor_dotted: &'a str,
    alone: &'a str,
    hello_status: i32,
    hello_head: &'a [(String, String)],
    hello_body: &'a str,
    hello_cpu_ns: u64,
    hello_wall_ns: u64,
    vars_body: &'a str,
    exit_status: i32,
    exit_contained: i32,
    fatal_status: i32,
    fatal_contained: i32,
    again_status: i32,
    again_body: &'a str,
    cwd_preserved: bool,
    egress_mode: &'a str,
    egress_body: &'a str,
    view_tmp: &'a str,
    inet_socket_before: i32,
    inet_socket_after: i32,
}

/// The single stdout line the driver parses.
fn json(report: &Report<'_>) -> String {
    let head = report
        .hello_head
        .iter()
        .map(|(name, value)| format!("\"{}\"", escape(&format!("{name}: {value}"))))
        .collect::<Vec<_>>()
        .join(",");
    let vars = report.vars_body;
    format!(
        "{{\"abi\":{},\"php_minor\":{},\"php_version\":\"{}\",\"shim_before_libphp\":\"{}\",\"status\":{},\
         \"head\":[{}],\"body\":\"{}\",\"cpu_ns\":{},\"wall_ns\":{},\"contained\":{},\
         \"cwd_preserved\":{},\"egress_mode\":\"{}\",\"egress\":\"{}\",\
         \"view_tmp\":\"{}\",\
         \"inet_socket_errno\":{{\"before\":{},\"after\":{}}},\
         \"requests\":[\
         {{\"fixture\":\"hello.php\",\"status\":{}}},\
         {{\"fixture\":\"vars.php\",\"body\":\"{}\"}},\
         {{\"fixture\":\"egress.php\",\"body\":\"{}\"}},\
         {{\"fixture\":\"exit.php\",\"status\":{},\"contained\":{}}},\
         {{\"fixture\":\"fatal.php\",\"status\":{},\"contained\":{}}},\
         {{\"fixture\":\"hello.php\",\"status\":{},\"body\":\"{}\"}}]}}",
        report.abi,
        report.minor_code,
        report.minor_dotted,
        escape(report.alone),
        report.hello_status,
        head,
        escape(report.hello_body),
        report.hello_cpu_ns,
        report.hello_wall_ns,
        0,
        report.cwd_preserved,
        report.egress_mode,
        escape(report.egress_body),
        escape(report.view_tmp),
        report.inet_socket_before,
        report.inet_socket_after,
        report.hello_status,
        escape(vars),
        escape(report.egress_body),
        report.exit_status,
        report.exit_contained,
        report.fatal_status,
        report.fatal_contained,
        report.again_status,
        escape(report.again_body),
    )
}

/// JSON string escaping for the bytes a fixture can print.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}
