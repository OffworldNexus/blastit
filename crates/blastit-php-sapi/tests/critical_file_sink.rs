//! Protected critical test: PHP's loaded-file sink (`MB.23.4.2`, `MB.23.4`).
//!
//! A dev PHP view reports the files a request actually loaded so the supervisor
//! can watch them (`ADR-0014`, design §4.4). The sink is **optional**: a shim or
//! a PHP build without `blastit_php_file_sink_install` must serve exactly as it
//! did before, and a reporting failure must never fail a request. Both halves of
//! every claim are asserted here:
//!
//! * **With the pair** (`make rs-test-php`): a script that includes one file and
//!   reads a `.env`-style data file reports the include — absolute, canonical,
//!   once, sorted — and *not* the data file: a file the application read is not a
//!   file PHP included, and templates are reached through the compiled→source
//!   map (`crates/blastit-view/src/php.rs`), never by inventing a path here. A
//!   request that `exit()`s still reports, because the shim snapshots
//!   `EG(included_files)` between `php_execute_script` and
//!   `php_request_shutdown`.
//! * **Without it** (every `make check`, on a PHP-free host): a shim built by
//!   `cc` that exports the five required symbols and **not** the sink is a
//!   silent no-op — `install_file_sink` answers `Err(NoSink::Absent)` and keeps
//!   nothing, and requests still run. A stub that *does* export it proves the
//!   Rust side of the contract (the address handed over, that the loader owns it
//!   across a request boundary and a move, the bytes read, `NULL`/zero-length
//!   never forming a slice).
//!
//! The suite never skips: with no pair, the stub legs and the ABI source pins
//! are the assertions, and they run on every host.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::Command;

use blastit_php_sapi::abi::{OUTCOME_BAILOUT, OUTCOME_OK, RequestV1, ResultV1, ServerVarV1};
use blastit_php_sapi::{
    Egress, FileSink, IniPolicy, LoadError, Loaded, NoSink, ServerVars, SharedObject, Sink,
};

const C_SHIM: &str = include_str!("../c/blastit_php_sapi.c");
const C_HEADER: &str = include_str!("../c/include/blastit_php_abi_v1.h");

// --------------------------------------------------------------- the ABI pins

/// The sink is additive: it is declared, and it did not move the ABI version.
///
/// This is the "no ABI bump" claim `MB.23.4.2` makes, asserted on the two files
/// that state it — a bump in either direction fails here rather than in a
/// comment.
#[test]
fn the_sink_is_an_optional_addition_and_the_abi_version_is_unchanged() {
    assert!(
        C_HEADER.contains("int32_t blastit_php_file_sink_install(void *ctx,"),
        "the header declares the install entry point"
    );
    assert!(
        C_HEADER.contains("int32_t (*included)(void *, const char *, size_t)"),
        "the header declares the callback's exact signature"
    );
    assert!(C_HEADER.contains("#define BLASTIT_PHP_ABI_VERSION 1u"));
    assert_eq!(blastit_php_sapi::ABI_VERSION, 1);
    assert_eq!(blastit_php_sapi::abi::ABI_VERSION, 1);

    // The frozen sink struct is untouched: the file report is not a fourth
    // callback on it, because that would be an ABI change.
    assert!(
        C_HEADER.contains("typedef struct blastit_php_sink_v1 {"),
        "the response sink stays the frozen struct"
    );

    // The snapshot reads the request's include table, and the plain-file
    // wrapper — whose replacement would weaken `open_basedir` — is not touched.
    assert!(
        C_SHIM.contains("ZEND_HASH_FOREACH_STR_KEY(&EG(included_files), key)"),
        "the shim iterates EG(included_files) by string key"
    );
    assert!(
        !C_SHIM.contains("php_plain_files_wrapper"),
        "the plain-file wrapper is never replaced"
    );
    assert!(
        !C_SHIM.contains("php_resolve_path"),
        "include-path resolution is left to PHP"
    );

    // And it is a *sink*, not a hook: it is called only when installed.
    assert!(
        C_SHIM.contains("if (bt_file_included == NULL) {"),
        "an uninstalled sink is never called"
    );
}

// ------------------------------------------------------------------ fixtures

/// A temporary application tree, removed with the directory.
struct Fixture {
    /// The directory PHP sees as `DOCUMENT_ROOT` and `open_basedir`.
    app: PathBuf,
    /// The view's private scratch directory.
    tmp: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let app =
            std::env::temp_dir().join(format!("blastit-php-sink-{}-{name}", std::process::id()));
        let tmp = app.join("tmp");
        std::fs::create_dir_all(&tmp).expect("fixture dirs");
        Self { app, tmp }
    }

    /// Writes `rel` under the app root and returns its canonical path.
    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.app.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("fixture parent");
        }
        std::fs::write(&path, contents).expect("fixture file");
        path.canonicalize().expect("the fixture resolves")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.app);
    }
}

/// Records what the shim's response sink produced; the stub legs never call it.
#[derive(Default)]
struct Recorder {
    body: Vec<u8>,
    logs: Vec<String>,
}

impl Sink for Recorder {
    fn head(&mut self, _status: i32, _name: Option<&[u8]>, _value: &[u8]) -> bool {
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

/// One load with the pair installed, plus the body and outcome it produced.
struct Ran {
    result: ResultV1,
    body: String,
    logs: Vec<String>,
}

/// Runs `entry` (a path under the fixture) through `loaded`.
fn run(loaded: &Loaded, fx: &Fixture, entry: &Path) -> Ran {
    let mut vars = ServerVars::new();
    vars.insert("REQUEST_METHOD", "GET").expect("method");
    vars.insert("SCRIPT_NAME", "/index.php").expect("script");
    vars.push_header("Host", "sink.test");
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
    let root_c = c_string(&fx.app.to_string_lossy());
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
    let result = loaded
        .request(&request, &mut sink)
        .unwrap_or_else(|err| panic!("the shim must run the request: {err}"));
    Ran {
        result,
        body: String::from_utf8_lossy(&sink.body).into_owned(),
        logs: sink.logs,
    }
}

fn c_string(text: &str) -> CString {
    CString::new(text).expect("no NUL in a fixture path")
}

/// The sink this loader owns and handed its shim, or a failure naming the API
/// misuse: `None` here means the install was never accepted, which every caller
/// of this helper has already asserted.
fn sink_of(loaded: &Loaded) -> &FileSink {
    loaded
        .file_sink()
        .expect("the sink the shim was handed is owned by the loader")
}

// ------------------------------------------------------------- the host pair

/// This host's real `libphp` plus the shim `make php-sapi` built for it.
struct Pair {
    libphp: PathBuf,
    shim: PathBuf,
}

/// The pair a real-load leg can use, or `None` when this host has neither (a
/// PHP-free `make check`) or has a library with no shim built for it — which is
/// why the suite always has a stub leg to fall back to.
fn pair() -> Option<Pair> {
    let libphp = libphp_path()?;
    for shim in shim_paths() {
        if Loaded::load(
            SharedObject::Path(libphp.clone()),
            SharedObject::Path(shim.clone()),
        )
        .is_ok()
        {
            return Some(Pair { libphp, shim });
        }
    }
    None
}

/// The host's NTS `libphp`, found without leaning on the code under test: the
/// `BLASTIT_LIBPHP` override, then the per-distro locations (`ADR-0010` §4.1).
fn libphp_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BLASTIT_LIBPHP") {
        let path = PathBuf::from(path);
        return path.canonicalize().ok();
    }
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    let triplet = format!("{}-linux-{libc}", std::env::consts::ARCH);
    let mut dirs = vec![
        format!("/usr/lib/{triplet}"),
        "/usr/lib64".to_string(),
        "/usr/lib".to_string(),
        "/usr/local/lib".to_string(),
    ];
    for minor in ["85", "84", "83"] {
        dirs.push(format!("/usr/lib/php{minor}"));
        dirs.push(format!("/usr/local/lib/php{minor}"));
    }
    let names = [
        "libphp8.5.so",
        "libphp85.so",
        "libphp8.4.so",
        "libphp84.so",
        "libphp8.3.so",
        "libphp83.so",
        "libphp.so",
    ];
    names.iter().find_map(|name| {
        dirs.iter()
            .map(|dir| Path::new(dir).join(name))
            .find(|path| path.is_file())
            .and_then(|path| path.canonicalize().ok())
    })
}

/// Every shim this build could have produced, in the directories the boot looks
/// in: `BLASTIT_PHP_SHIM_DIR`, the workspace's `target/php-sapi`, then beside
/// this test binary.
fn shim_paths() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("BLASTIT_PHP_SHIM_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    dirs.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/php-sapi")
            .to_path_buf(),
    );
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.to_path_buf());
    }
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    let suffix = format!("-{}-{libc}.so", std::env::consts::ARCH);
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut shims: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("libblastit_php8") && name.ends_with(&suffix)
                    })
            })
            .collect();
        // The shim's own name states its minor; newest first is the boot's
        // order, and a mismatch is simply another candidate here.
        shims.sort();
        shims.reverse();
        found.extend(shims);
    }
    found
}

/// The pair through `PhpEmbedRuntime`'s policy shape: a real INI lockdown, a
/// real view temp directory, a real request.
fn start(pair: &Pair, fx: &Fixture) -> Loaded {
    let mut loaded = Loaded::load(
        SharedObject::Path(pair.libphp.clone()),
        SharedObject::Path(pair.shim.clone()),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    let policy = IniPolicy::new(&fx.app, &fx.tmp, 30, Egress::Denied).expect("policy");
    loaded
        .worker_init(policy.as_str(), &fx.tmp)
        .unwrap_or_else(|err| panic!("{err}"));
    loaded
}

// ------------------------------------------------------- the real-load legs

/// A request's includes are reported: absolute, canonical, once, sorted — and a
/// file the script merely *read* is not.
///
/// **One test for the whole real-load leg**, because `libphp` is process-wide:
/// `php_module_startup` may run once per process, so a second `#[test]` that
/// started it would race this one (`LESSONS.md`: the interpreter is
/// process-wide). The bailout leg is therefore a second *request* on the same
/// worker, which is also what `MB.3.41` promises — a contained `exit()` leaves a
/// worker that can serve the next request.
#[test]
fn a_request_reports_what_it_included_and_never_a_file_it_read() {
    let Some(pair) = pair() else {
        // The honest degradation, asserted rather than skipped: with no pair,
        // the load is a typed refusal that names the file — never a panic.
        let missing = SharedObject::Path(PathBuf::from("/nonexistent/libphp.so"));
        let err = Loaded::load(
            missing,
            SharedObject::Path(PathBuf::from("/nonexistent/shim.so")),
        )
        .err()
        .expect("a missing library cannot load");
        assert!(matches!(err, LoadError::NotALibrary { .. }), "{err}");
        assert!(err.to_string().contains("libphp"), "{err}");
        println!("refused (no host pair; the stub legs below are the contract): {err}");
        return;
    };

    let fx = Fixture::new("included");
    let helper = fx.write(
        "lib/helper.php",
        "<?php\nfunction helper(): string { return 'ok'; }\n",
    );
    // The data file: read, never included. A `.env` is what a real app reads
    // with `file_get_contents`/`parse_ini_file`, and reporting it would be
    // guessing at a file PHP never loaded.
    let data = fx.write(".env", "APP_ENV=dev\n");
    let entry = fx.write(
        "index.php",
        "<?php\n\
         require __DIR__ . '/lib/helper.php';\n\
         $data = file_get_contents(__DIR__ . '/.env');\n\
         echo strlen($data) . ':' . helper();\n",
    );
    let bail = fx.write(
        "bail.php",
        "<?php\n\
         require __DIR__ . '/lib/helper.php';\n\
         echo helper();\n\
         exit(0);\n",
    );

    let mut loaded = start(&pair, &fx);
    loaded
        .install_file_sink(FileSink::new())
        .unwrap_or_else(|err| {
            panic!("the shim this build produced must export blastit_php_file_sink_install: {err}")
        });

    let ran = run(&loaded, &fx, &entry);
    assert_eq!(ran.result.outcome, OUTCOME_OK, "logs: {:?}", ran.logs);
    assert!(
        ran.body.contains(":ok"),
        "the script ran and read its data file: {:?}",
        ran.body
    );

    let paths = sink_of(&loaded).take();
    assert!(
        paths.iter().all(|path| path.starts_with('/')),
        "every reported path is absolute: {paths:?}"
    );
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "the report is sorted: {paths:?}");

    let entry_s = entry.display().to_string();
    let helper_s = helper.display().to_string();
    assert_eq!(
        paths.iter().filter(|path| **path == entry_s).count(),
        1,
        "the entry script is reported exactly once: {paths:?}"
    );
    assert_eq!(
        paths.iter().filter(|path| **path == helper_s).count(),
        1,
        "the included file is reported exactly once, however it was resolved: {paths:?}"
    );
    let data_s = data.display().to_string();
    assert!(
        !paths.contains(&data_s),
        "a file the script read is not a file it included: {paths:?}"
    );

    // The next request reports its own snapshot: what the first one loaded is
    // gone, so a delta names only what the new request touched.
    sink_of(&loaded).clear();
    assert!(
        sink_of(&loaded).take().is_empty(),
        "an emptied sink reports nothing"
    );

    // The contained bailout: `exit()` after including a file. The snapshot runs
    // between `php_execute_script` and `php_request_shutdown`, so the files are
    // still there to report even though the script never returned.
    let ran = run(&loaded, &fx, &bail);
    assert_eq!(
        ran.result.outcome, OUTCOME_BAILOUT,
        "an exit() is the shim's contained bailout: {:?}",
        ran.logs
    );
    assert_eq!(ran.result.contained, 1, "the bailout was contained");
    let paths = sink_of(&loaded).take();
    assert!(
        paths.contains(&helper_s),
        "a bailed request still reports what it included: {paths:?}"
    );
    assert!(
        paths.contains(&bail.display().to_string()),
        "and the entry script it bailed out of: {paths:?}"
    );
    assert!(
        paths.iter().all(|path| !path.ends_with(".env")),
        "the bailed request still does not report its data file: {paths:?}"
    );
    println!(
        "served: {} file(s) after the clean request, {} after the bailout: {paths:?}",
        sorted.len(),
        paths.len()
    );
    loaded.worker_shutdown();
}

// ------------------------------------------------------------ the stub legs

/// A shim compiled from a source string into `out`, with the given defines.
fn compile(source: &Path, out: &Path, defines: &[(&str, String)]) {
    let include = Path::new(env!("CARGO_MANIFEST_DIR")).join("c/include");
    let mut command = Command::new("cc");
    command
        .args(["-shared", "-fPIC", "-O0", "-Wall", "-Werror", "-o"])
        .arg(out)
        .arg(source)
        .arg("-lc")
        .arg(format!("-I{}", include.display()));
    for (name, value) in defines {
        command.arg(format!("-D{name}={value}"));
    }
    let output = command.output().unwrap_or_else(|err| {
        panic!("this suite compiles its stub shims with `cc`, which could not be run: {err}")
    });
    assert!(
        output.status.success(),
        "cc failed for {}: {}",
        out.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A `-D` string literal for the compiler.
fn quote(value: &str) -> String {
    format!("\"{value}\"")
}

/// A stub root with the three pairs this suite needs, removed on drop.
struct Stubs {
    dir: PathBuf,
    libphp: PathBuf,
    /// The five required symbols only: no `blastit_php_file_sink_install`.
    shim_without_sink: PathBuf,
    /// The same five, plus a sink that reports one path per request.
    shim_with_sink: PathBuf,
    /// A sink that is handed `NULL` and a zero length, which must never become a
    /// slice.
    shim_with_bad_calls: PathBuf,
    /// The path the sink stubs report.
    reported: PathBuf,
}

impl Drop for Stubs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const LIBPHP_STUB_C: &str = r#"
/* Only what the loader asks of the host library: `php_version`, the string the
 * shim's minor is checked against.
 *
 * Deliberately **no** `executor_globals` (or any other Zend symbol): these stubs
 * are opened `RTLD_GLOBAL`, and a Zend symbol in the global namespace would let
 * a real libphp's shim bind to the stub instead. The real-load leg may run in
 * this same process, so the stub adds nothing the real one could mistake for its
 * own. */
const char *php_version(void) { return "8.4.27"; }
"#;

const SHIM_STUB_C: &str = r#"
/* The five symbols `Loaded::load` requires — and, when `BT_SINK` is defined,
 * the optional file sink `MB.23.4.2` adds. Compiled against the real header, so
 * a signature drift on either side is a compile error here. */
#include "blastit_php_abi_v1.h"

uint32_t blastit_php_abi_version(void) { return BLASTIT_PHP_ABI_VERSION; }
uint32_t blastit_php_minor(void) { return BT_MINOR; }

int32_t blastit_php_worker_init(const blastit_php_config_v1 *cfg)
{
    (void)cfg;
    return 0;
}

void blastit_php_worker_shutdown(void) {}

#if defined(BT_SINK)
static void *bt_ctx;
static int32_t (*bt_included)(void *, const char *, size_t);

int32_t blastit_php_file_sink_install(void *ctx,
                                      int32_t (*included)(void *, const char *, size_t))
{
    bt_ctx = included != NULL ? ctx : NULL;
    bt_included = included;
    return 0;
}
#endif

int32_t blastit_php_request(const blastit_php_request_v1 *req,
                            const blastit_php_sink_v1 *sink,
                            blastit_php_result_v1 *out)
{
    (void)req;
    (void)sink;
    if (out != NULL) {
        out->outcome = BLASTIT_PHP_OUTCOME_OK;
        out->status = 200;
    }
#if defined(BT_SINK)
    if (bt_included == NULL || bt_ctx == NULL) {
        return 0;
    }
#if defined(BT_BAD_CALLS)
    /* A `NULL` path and a zero length: neither may form a slice. */
    (void)bt_included(bt_ctx, NULL, 7);
    (void)bt_included(bt_ctx, BT_PATH, 0);
#else
    (void)bt_included(bt_ctx, BT_PATH, sizeof BT_PATH - 1);
#endif
#endif
    return 0;
}
"#;
impl Stubs {
    fn build(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "blastit-php-sink-stubs-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("stub dir");
        let libphp = dir.join("libphp8.4.so");
        let reported = dir.join("included.php");
        std::fs::write(&reported, "<?php\n").expect("stub fixture");
        let reported = reported.canonicalize().expect("resolves");

        let libphp_c = dir.join("libphp_stub.c");
        std::fs::write(&libphp_c, LIBPHP_STUB_C).expect("stub source");
        compile(&libphp_c, &libphp, &[]);

        let shim_c = dir.join("shim_stub.c");
        std::fs::write(&shim_c, SHIM_STUB_C).expect("stub source");
        let minor = ("BT_MINOR", "804".to_string());
        let shim_without_sink = dir.join("libblastit_php84-x86_64-gnu.so");
        compile(&shim_c, &shim_without_sink, std::slice::from_ref(&minor));

        let shim_with_sink = dir.join("with_sink.so");
        compile(
            &shim_c,
            &shim_with_sink,
            &[
                minor.clone(),
                ("BT_SINK", "1".to_string()),
                ("BT_PATH", quote(&reported.display().to_string())),
            ],
        );
        let shim_with_bad_calls = dir.join("bad_calls.so");
        compile(
            &shim_c,
            &shim_with_bad_calls,
            &[
                minor,
                ("BT_SINK", "1".to_string()),
                ("BT_BAD_CALLS", "1".to_string()),
                ("BT_PATH", quote(&reported.display().to_string())),
            ],
        );

        Self {
            dir,
            libphp,
            shim_without_sink,
            shim_with_sink,
            shim_with_bad_calls,
            reported,
        }
    }

    fn load(&self, shim: &Path) -> Loaded {
        let mut loaded = Loaded::load(
            SharedObject::Path(self.libphp.clone()),
            SharedObject::Path(shim.to_path_buf()),
        )
        .unwrap_or_else(|err| panic!("the stub pair must load: {err}"));
        let policy = IniPolicy::new(&self.dir, &self.dir, 30, Egress::Denied).expect("policy");
        loaded
            .worker_init(policy.as_str(), &self.dir)
            .unwrap_or_else(|err| panic!("{err}"));
        loaded
    }
}

/// A shim without the symbol is a silent no-op: no install, no failure, and
/// requests still run — which is the whole "optional forever" promise.
#[test]
fn a_shim_without_the_sink_is_a_no_op_and_never_a_failure() {
    let stubs = Stubs::build("without");
    let mut loaded = stubs.load(&stubs.shim_without_sink);

    assert_eq!(
        loaded.install_file_sink(FileSink::new()),
        Err(NoSink::Absent),
        "a shim with no file sink says `no symbol` instead of failing"
    );
    // The load-bearing half of "no sink": the loader kept nothing, so the shim
    // was never handed an address at all. (An "and it reported nothing" check on
    // a sink here would be vacuous — the sink was refused, so there is no cell
    // for the request to fill.)
    assert!(
        loaded.file_sink().is_none(),
        "a refused install leaves no sink for the shim to name"
    );

    // The worker still runs.
    let fx = Fixture::new("no-sink");
    let entry = fx.write("index.php", "<?php echo 1;\n");
    let ran = run(&loaded, &fx, &entry);
    assert_eq!(
        ran.result.outcome, OUTCOME_OK,
        "a request still runs: {:?}",
        ran.logs
    );
    loaded.worker_shutdown();
}

/// With the symbol, the `ctx` the shim holds reaches the Rust sink and the bytes
/// arrive intact — the half a real `libphp` cannot prove on a PHP-free host.
#[test]
fn an_installed_sink_receives_the_paths_the_shim_reports() {
    let stubs = Stubs::build("with");
    let mut loaded = stubs.load(&stubs.shim_with_sink);
    loaded
        .install_file_sink(FileSink::new())
        .unwrap_or_else(|err| panic!("the stub shim exports the sink: {err}"));

    let fx = Fixture::new("with-sink");
    let entry = fx.write("index.php", "<?php echo 1;\n");
    let ran = run(&loaded, &fx, &entry);
    assert_eq!(ran.result.outcome, OUTCOME_OK, "logs: {:?}", ran.logs);

    let reported = stubs.reported.display().to_string();
    assert_eq!(
        sink_of(&loaded).take(),
        vec![reported],
        "the path crossed the boundary once, absolute and canonical"
    );
    loaded.worker_shutdown();
}

/// The sink outlives the request boundary **and** a loader that moves after the
/// hand-off.
///
/// The shim keeps `ctx` — the sink's address — in a static for the rest of the
/// process, so whose address it is decides whether this is sound. `Loaded` owns
/// the sink in a `Box` and takes it *by value*: the static names a heap
/// allocation, not the `Loaded` struct, so moving the loader (as
/// `PhpEmbedRuntime::load_reporting` does, into its `Mutex`, and then into an
/// `Arc`) cannot change where a later request reports. A caller-owned
/// `&FileSink` — the shape this replaced — made the next request a
/// use-after-free the moment the caller's cell went away.
#[test]
fn the_installed_sink_outlives_a_request_and_a_moved_loader() {
    let stubs = Stubs::build("moved");
    let mut loaded = stubs.load(&stubs.shim_with_sink);
    loaded
        .install_file_sink(FileSink::new())
        .unwrap_or_else(|err| panic!("the stub shim exports the sink: {err}"));
    let fx = Fixture::new("moved");
    let entry = fx.write("index.php", "<?php echo 1;\n");
    let reported = stubs.reported.display().to_string();

    // One request: the address the shim holds is the one it was handed.
    let ran = run(&loaded, &fx, &entry);
    assert_eq!(ran.result.outcome, OUTCOME_OK, "logs: {:?}", ran.logs);
    assert_eq!(sink_of(&loaded).take(), vec![reported.clone()]);

    // Now the loader moves, after the shim took the address.
    let mut moved = Box::new(loaded);
    let ran = run(&moved, &fx, &entry);
    assert_eq!(
        ran.result.outcome, OUTCOME_OK,
        "the moved loader still serves: {:?}",
        ran.logs
    );
    assert_eq!(
        sink_of(&moved).take(),
        vec![reported],
        "a request after the move reports into the same allocation"
    );

    // And the per-request state is still the shim's own cell, borrowed — not a
    // copy the loader keeps on the side.
    sink_of(&moved).clear();
    assert!(sink_of(&moved).take().is_empty(), "`take` drains the cell");

    // Shutdown clears the shim's static and drops the sink, in that order: after
    // it, the loader has nothing the shim could still name.
    moved.worker_shutdown();
    assert!(
        moved.file_sink().is_none(),
        "a shut-down worker no longer owns a sink"
    );
}

/// "Too early" and "no symbol" are two answers, not one `false`.
///
/// `install_file_sink` before `worker_init` is transient ([`NoSink::NotInitialised`])
/// and the same handle accepts a sink afterwards; a shim with no entry point is
/// permanent ([`NoSink::Absent`]) however late it is asked. A caller acts on the
/// difference — retry, or serve on with no report — where a `bool` made both
/// look like "this PHP build has no sink", which is what left the second case
/// silently unreported for a whole process.
#[test]
fn installing_before_the_worker_starts_is_not_the_same_answer_as_a_missing_symbol() {
    let stubs = Stubs::build("early");
    let mut loaded = Loaded::load(
        SharedObject::Path(stubs.libphp.clone()),
        SharedObject::Path(stubs.shim_with_sink.clone()),
    )
    .unwrap_or_else(|err| panic!("the stub pair must load: {err}"));

    assert_eq!(
        loaded.install_file_sink(FileSink::new()),
        Err(NoSink::NotInitialised),
        "nothing is running yet: that is a transient answer, not `no symbol`"
    );
    assert!(
        loaded.file_sink().is_none(),
        "a refused install keeps no sink"
    );

    let policy = IniPolicy::new(&stubs.dir, &stubs.dir, 30, Egress::Denied).expect("policy");
    loaded
        .worker_init(policy.as_str(), &stubs.dir)
        .unwrap_or_else(|err| panic!("{err}"));
    assert!(
        loaded.install_file_sink(FileSink::new()).is_ok(),
        "the same shim accepts the sink once the worker has started"
    );

    // The permanent answer stays permanent, from a different shim.
    let absent = Stubs::build("early-absent");
    let mut other = absent.load(&absent.shim_without_sink);
    assert_eq!(
        other.install_file_sink(FileSink::new()),
        Err(NoSink::Absent),
        "a shim with no entry point says so, however late it is asked"
    );
    other.worker_shutdown();
    loaded.worker_shutdown();
}

/// `NULL` and zero-length calls are not files: they never become a slice, which
/// is `from_raw_parts(NULL, 0)`'s undefined behaviour.
#[test]
fn a_null_or_empty_path_is_never_a_slice() {
    let stubs = Stubs::build("bad-calls");
    let mut loaded = stubs.load(&stubs.shim_with_bad_calls);
    loaded
        .install_file_sink(FileSink::new())
        .unwrap_or_else(|err| panic!("the stub shim exports the sink: {err}"));

    let fx = Fixture::new("bad-calls");
    let entry = fx.write("index.php", "<?php echo 1;\n");
    let ran = run(&loaded, &fx, &entry);
    assert_eq!(ran.result.outcome, OUTCOME_OK, "the request survived");
    assert!(
        sink_of(&loaded).take().is_empty(),
        "neither call reported a path"
    );
    loaded.worker_shutdown();
}

/// The sink's own bounds: a relative path, a non-UTF-8 path and an over-long one
/// are dropped, never truncated onto the wire.
#[test]
fn the_sink_reports_only_paths_the_wire_can_carry() {
    let sink = FileSink::new();
    sink.record(b"relative/thing.php");
    sink.record(b"/not/utf8/\xff.php");
    sink.record(&[b'/'; blastit_php_sapi::loader::MAX_RECORDED_PATH_BYTES + 1]);
    sink.record(b"");
    assert!(sink.take().is_empty(), "nothing qualifies");

    // The accepted twin: an absolute path that exists is reported canonically.
    let fixture = Fixture::new("bounds");
    let file = fixture.write("app.php", "<?php\n");
    sink.record(file.as_os_str().as_encoded_bytes());
    assert_eq!(sink.take(), vec![file.display().to_string()]);
    assert!(sink.take().is_empty(), "`take` drains");
}
