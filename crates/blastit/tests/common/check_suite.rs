//! A scriptable `pytest` suite for `blastit check`, and the interpreter shim it
//! runs on.
//!
//! `check` spawns `<python> -m pytest` and reads two observation streams. A host
//! without pytest in the audited interpreter runs the shared [`SUITE`] script
//! instead: the shim answers the two `-c "import …"` preflights (pytest, and
//! `pytest_django` when a case wants the Django handshake), and `-m pytest`
//! executes [`SUITE`] on the host's real interpreter. The script writes the HTTP
//! observations, pushes control targets and runs statements through the route's
//! own dbproxy socket — so `check`, dbproxy, the PG peer and the audit are all
//! real; only the interpreter's `pytest` is a shim.
//!
//! Every fixture says what the run observes in `spec.json`:
//!
//! ```json
//! {"http": [{"method":"GET","path":"/"}], "push": [{"op":"push_routed","route":"home"}],
//!  "sql": ["SELECT 1"], "exit": 0, "record": "/abs/record.json", "django": true}
//! ```
//!
//! `record` makes the script dump what the child inherited (`PYTHONPATH`, the
//! one-shot Django handshake and its mode), which is how the gate-root and
//! handshake promises are observed without reading the product's internals.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

/// The suite `-m pytest` runs: observations first, then the control channel and
/// the session, then the recorded facts, then the exit shape.
const SUITE: &str = r#"
import json
import os
import signal
import socket
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(HERE, "spec.json"), encoding="utf-8") as handle:
    spec = json.load(handle)


def read(sock, count):
    data = b""
    while len(data) < count:
        chunk = sock.recv(count - len(data))
        if not chunk:
            raise OSError(0, "the server closed the session")
        data += chunk
    return data


def until_ready(sock):
    while True:
        tag = read(sock, 1)
        size = struct.unpack("!I", read(sock, 4))[0]
        body = read(sock, size - 4)
        if tag == b"E":
            raise RuntimeError("pg: " + body.decode("utf-8", "replace"))
        if tag == b"Z":
            return


def session(variable, statements):
    url = os.environ[variable]
    query = url.split("?", 1)[1] if "?" in url else ""
    params = dict(part.split("=", 1) for part in query.split("&") if "=" in part)
    path = os.path.join(
        params.get("host", "/proc/self/cwd"), ".s.PGSQL." + params.get("port", "5432")
    )
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(10)
    sock.connect(path)
    startup = struct.pack("!I", 196608) + b"user\0blastit\0database\0app\0\0"
    sock.sendall(struct.pack("!I", len(startup) + 4) + startup)
    until_ready(sock)
    try:
        for statement in statements:
            body = statement.encode() + b"\0"
            sock.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
            until_ready(sock)
    finally:
        sock.close()


def control():
    channel = socket.socket(
        socket.AF_UNIX,
        socket.SOCK_STREAM,
        fileno=int(os.environ["BLASTIT_CHECK_CTL_FD"]),
    )
    channel.settimeout(5)
    return channel


def send(channel, payload):
    channel.sendall(json.dumps(payload).encode() + b"\n")
    reply = b""
    while not reply.endswith(b"\n"):
        reply += channel.recv(4096)
    if json.loads(reply).get("ok") is not True:
        raise RuntimeError("the control channel refused a request")


http = spec.get("http", [])
raw_http = spec.get("raw_http")
if http or raw_http is not None:
    with open(os.environ["BLASTIT_CHECK_HTTP_OBS"], "a", encoding="utf-8") as sink:
        for line in http:
            sink.write(json.dumps(line) + "\n")
        for line in raw_http or []:
            sink.write(line + "\n")

pushes = spec.get("push", [])
statements = spec.get("sql", [])
analytics = spec.get("analytics_sql", [])
channel = control() if pushes or statements or analytics else None
for push in pushes:
    send(channel, push)
if statements:
    session("DATABASE_URL", statements)
if analytics:
    session("ANALYTICS_URL", analytics)
for _ in pushes:
    send(channel, {"op": "pop"})

record = spec.get("record")
if record:
    facts = {
        "pythonpath": os.environ.get("PYTHONPATH"),
        "executable": sys.executable,
        "argv": sys.argv,
    }
    setup = os.environ.get("BLASTIT_CHECK_DJANGO_SETUP")
    if setup and spec.get("django"):
        with open(setup, encoding="utf-8") as handle:
            facts["django"] = {
                "path": setup,
                "mode": os.stat(setup).st_mode & 0o777,
                "content": json.load(handle),
            }
    with open(record, "w", encoding="utf-8") as sink:
        json.dump(facts, sink)

code = spec.get("exit", 0)
if code == "signal":
    os.kill(os.getpid(), signal.SIGKILL)
sys.exit(code)
"#;

/// The interpreter the script runs on: the test host's own CPython.
///
/// The suite is stdlib-only, so any Python 3 will do; a project pin is preferred
/// so the fixture matches the environment `check` documents.
pub fn host_python() -> PathBuf {
    if let Some(path) = std::env::var_os("BLASTIT_TEST_PYTHON_314") {
        return PathBuf::from(path);
    }
    if let Ok(output) = Command::new("uv")
        .args(["python", "find", "--no-project", "3.14"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        && output.status.success()
    {
        let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_owned());
        if path.is_file() {
            return path;
        }
    }
    for name in ["python3.14", "python3"] {
        if let Ok(output) = Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .stderr(Stdio::null())
            .output()
            && output.status.success()
        {
            let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_owned());
            if path.is_file() {
                return path;
            }
        }
    }
    panic!("no CPython to run the check suite: install one with `uv python install 3.14`");
}

/// Writes `Blastfile`, `spec.json`, the suite and a `python3` shim into `dir`;
/// returns the shim `BLASTIT_PYTHON` names.
pub fn fixture(dir: &Path, manifest: &str, spec: &Value) -> PathBuf {
    fixture_with(dir, manifest, spec, true, false, None)
}

/// [`fixture`] for a project the Django handshake must recognise.
pub fn django_fixture(dir: &Path, manifest: &str, spec: &Value) -> PathBuf {
    fixture_with(dir, manifest, spec, true, true, None)
}

/// The one writer: the import answers and the optional interpreter marker.
pub fn fixture_with(
    dir: &Path,
    manifest: &str,
    spec: &Value,
    pytest: bool,
    django: bool,
    marker: Option<(&Path, &str)>,
) -> PathBuf {
    std::fs::write(dir.join("Blastfile"), manifest).expect("the fixture manifest");
    std::fs::write(dir.join("spec.json"), spec.to_string()).expect("the fixture spec");
    std::fs::write(dir.join("suite.py"), SUITE).expect("the fixture suite");
    let shim = dir.join("python3");
    write_shim(
        &shim,
        &host_python(),
        &dir.join("suite.py"),
        marker,
        pytest,
        django,
    );
    shim
}

/// Writes one executable interpreter shim (`#!/bin/sh`).
///
/// `marker` appends `label` to that file, so a case can prove which candidate
/// `python_program` chose without reading the product.
pub fn write_shim(
    path: &Path,
    python: &Path,
    suite: &Path,
    marker: Option<(&Path, &str)>,
    pytest: bool,
    django: bool,
) {
    use std::os::unix::fs::PermissionsExt;

    let mut body = String::from("#!/bin/sh\n");
    if let Some((marker, label)) = marker {
        body.push_str(&format!("printf '{label}\\n' >> {}\n", quote(marker)));
    }
    body.push_str(&format!(
        "case \"$*\" in\n  *pytest_django*) exit {} ;;\n  *-c*) exit {} ;;\nesac\n",
        u8::from(!django),
        u8::from(!pytest)
    ));
    body.push_str(&format!("exec {} {} \"$@\"\n", quote(python), quote(suite)));
    std::fs::write(path, body).expect("the interpreter shim");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("an executable shim");
}

/// One `'…'`-quoted word, so a path with spaces still reaches `exec`.
fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}
