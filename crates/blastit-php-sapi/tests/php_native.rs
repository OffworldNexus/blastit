//! The real load, against the host's NTS `libphp` and the shim `make php-sapi`
//! built. **Ignored by default**: a PHP-free host has neither, and `make check`
//! must stay green there.
//!
//! ```console
//! $ make php-sapi
//! $ BLASTIT_LIBPHP=/usr/lib/libphp8.4.so \
//!   BLASTIT_PHP_SHIM="$PWD/target/php-sapi/libblastit_php84.so" \
//!   cargo test -p blastit-php-sapi --test php_native -- --ignored --nocapture
//! ```
//!
//! It is a harness, not a skip: with `--ignored` and a missing variable it
//! fails with the usage above rather than returning success, so a green
//! "0 passed" can never be mistaken for a proven load.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::CString;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use blastit_php_sapi::abi::{OUTCOME_OK, RequestV1, ServerVarV1};
use blastit_php_sapi::{Egress, IniPolicy, Loaded, ServerVars, SharedObject, Sink};

/// Records what the shim's callbacks produced, in order.
#[derive(Default)]
struct Recorder {
    status: Option<i32>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    logs: Vec<String>,
}

impl Sink for Recorder {
    fn head(&mut self, status: i32, name: Option<&[u8]>, value: &[u8]) -> bool {
        match name {
            None => self.status = Some(status),
            Some(name) => self.headers.push((
                String::from_utf8_lossy(name).into_owned(),
                String::from_utf8_lossy(value).into_owned(),
            )),
        }
        true
    }

    fn body(&mut self, data: &[u8]) -> bool {
        self.body.extend_from_slice(data);
        true
    }

    fn log(&mut self, _level: i32, line: &str) {
        self.logs.push(line.to_owned());
    }
}

/// A required environment variable, or the usage line.
fn required(name: &str) -> PathBuf {
    std::env::var_os(name).map_or_else(
        || {
            panic!(
                "{name} is not set; run `make php-sapi` first, then \
                 BLASTIT_LIBPHP=/usr/lib/libphp8.4.so \
                 BLASTIT_PHP_SHIM=$PWD/target/php-sapi/libblastit_php84.so \
                 cargo test -p blastit-php-sapi --test php_native -- --ignored"
            )
        },
        PathBuf::from,
    )
}

fn c_string(text: &str) -> CString {
    CString::new(text).expect("no NUL")
}

#[test]
#[ignore = "needs the host's libphp and `make php-sapi`; see the module docs"]
fn an_fd_pinned_libphp_and_shim_load_through_proc() {
    // The production spelling: stage 0 opens and verifies both files, stage 1
    // passes the descriptors, and the worker loads them as
    // `/proc/self/fd/<n>` — never by a path that could be swapped in between.
    let libphp = required("BLASTIT_LIBPHP");
    let shim = required("BLASTIT_PHP_SHIM");
    let pin = |path: &Path| {
        let file = std::fs::File::open(path)
            .unwrap_or_else(|err| panic!("cannot open {}: {err}", path.display()));
        OwnedFd::from(file)
    };
    let libphp = pin(&libphp);
    let shim = pin(&shim);
    // The loader is handed the numbers; the `OwnedFd`s stay alive here so the
    // files cannot be closed under the `dlopen`.
    let libphp_fd = libphp.as_raw_fd();
    let shim_fd = shim.as_raw_fd();
    for (what, fd) in [("libphp", libphp_fd), ("shim", shim_fd)] {
        assert!(
            std::fs::read_link(format!("/proc/self/fd/{fd}")).is_ok(),
            "the pinned {what} descriptor is not open"
        );
    }

    let loaded = Loaded::load(SharedObject::Fd(libphp_fd), SharedObject::Fd(shim_fd))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(loaded.abi_version(), 1);
    assert_eq!(loaded.minor().dotted(), "8.4", "{}", loaded.php_version());
    assert!(
        loaded.php_version().starts_with("8.4."),
        "host version {}",
        loaded.php_version()
    );
}

#[test]
#[ignore = "needs the host's libphp and `make php-sapi`; see the module docs"]
fn a_native_libphp_loads_and_serves_one_request() {
    let libphp = required("BLASTIT_LIBPHP");
    let shim = required("BLASTIT_PHP_SHIM");
    assert!(Path::new(&shim).is_file(), "{} is missing", shim.display());

    let mut loaded = Loaded::load(
        SharedObject::Path(libphp.clone()),
        SharedObject::Path(shim.clone()),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(loaded.abi_version(), 1);
    assert_eq!(loaded.minor().dotted(), "8.4", "{}", loaded.php_version());
    assert!(
        loaded.php_version().starts_with("8.4."),
        "host version {}",
        loaded.php_version()
    );

    // A real policy, rendered by the crate under test, and a fixture that burns
    // a CPU tick so `cpu_ns` is not a tick-granular zero (MB.3.12's lesson).
    let app = std::env::temp_dir().join(format!("blastit-php-native-{}", std::process::id()));
    std::fs::create_dir_all(&app).expect("fixture dir");
    let entry = app.join("index.php");
    std::fs::write(
        &entry,
        b"<?php\nfor ($i = 0; $i < 20000; $i++) { $x = $i * $i; }\n\
          header('X-Native: 1');\necho 'hello from native php';\n",
    )
    .expect("fixture");
    let view_tmp = app.join("tmp");
    std::fs::create_dir_all(&view_tmp).expect("view tmp");
    let policy = IniPolicy::new(&app, &view_tmp, 30, Egress::Denied).expect("policy");
    loaded
        .worker_init(policy.as_str(), &view_tmp)
        .unwrap_or_else(|err| panic!("{err}"));

    let mut vars = ServerVars::new();
    vars.insert("REQUEST_METHOD", "GET").expect("method");
    vars.insert("SCRIPT_NAME", "/index.php").expect("script");
    vars.push_header("Host", "native.test");
    let entries: Vec<ServerVarV1> = vars
        .iter()
        .map(|var| ServerVarV1 {
            name: var.name().as_ptr().cast(),
            name_len: var.name().len(),
            value: var.value().as_ptr().cast(),
            value_len: var.value().len(),
        })
        .collect();

    let entry_c = c_string(&entry.to_string_lossy());
    let root_c = c_string(&app.to_string_lossy());
    let script_c = c_string("/index.php");
    let uri_c = c_string("/index.php");
    let method_c = c_string("GET");
    let request = RequestV1 {
        entry: entry_c.as_ptr(),
        document_root: root_c.as_ptr(),
        script_name: script_c.as_ptr(),
        request_uri: uri_c.as_ptr(),
        query_string: std::ptr::null(),
        request_method: method_c.as_ptr(),
        content_type: std::ptr::null(),
        content_length: 0,
        cookie: std::ptr::null(),
        server_vars: entries.as_ptr(),
        server_var_count: entries.len(),
        body: std::ptr::null(),
        body_len: 0,
    };
    let mut sink = Recorder::default();
    let result = loaded.request(&request, &mut sink).expect("one request");

    assert_eq!(result.outcome, OUTCOME_OK, "logs: {:?}", sink.logs);
    assert_eq!(result.status, 200);
    assert_eq!(sink.status, Some(200), "logs: {:?}", sink.logs);
    assert!(
        sink.headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("X-Native") && value == "1"),
        "headers: {:?}",
        sink.headers
    );
    assert!(
        String::from_utf8_lossy(&sink.body).contains("hello from native php"),
        "body: {:?}",
        String::from_utf8_lossy(&sink.body)
    );
    assert!(result.cpu_ns > 0, "cpu_ns was zero on a busy fixture");
    assert!(result.body_bytes >= sink.body.len() as u64);

    // A second `worker_init` is refused with a typed code, not a crash, and the
    // loaded object survives it: a failed startup is a status, not a panic.
    let second = loaded
        .worker_init(policy.as_str(), &view_tmp)
        .expect_err("a second startup is refused");
    assert!(
        matches!(second, blastit_php_sapi::LoadError::Init { .. }),
        "{second}"
    );

    loaded.worker_shutdown();
    let _ = std::fs::remove_dir_all(&app);
}
