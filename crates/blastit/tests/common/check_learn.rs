//! The `blastit check --learn --dry-run` fixture: a shim "interpreter", the
//! suite it runs, and the one spawn.
//! `check` spawns `<python> -m pytest …` and reads two observation streams. On a
//! host with no pytest (and no network to install one) the suite is a shim: an
//! executable `python3` that answers the `-c "import …"` preflights and, for
//! `-m pytest`, runs [`SUITE`] on the host's real interpreter. The suite writes
//! the HTTP observations itself and runs three session-setup statements through
//! the route's own dbproxy socket — which is exactly what a real suite's gate
//! does — so the dry-run line is computed from real observations by the real
//! binary, and no product code is stubbed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The manifest the fixture audits: one store, one declared route, and two paths
/// nothing routes (so `--learn` proposes two views).
pub const MANIFEST: &str = "\
postgres $DATABASE_URL

view shop GET \"/shop/*\" {}
";

/// The shim suite. It writes two unrouted observations and runs three `SET`
/// statements (session setup) on the route's socket, which the proxy attributes
/// to the first route — three parameterless statements, no client value.
const SUITE: &str = r#"
import json
import os
import socket
import struct


def _read(sock, count):
    data = b""
    while len(data) < count:
        chunk = sock.recv(count - len(data))
        if not chunk:
            raise OSError(0, "the server closed the session")
        data += chunk
    return data


def _until_ready(sock):
    while True:
        tag = _read(sock, 1)
        size = struct.unpack("!I", _read(sock, 4))[0]
        body = _read(sock, size - 4)
        if tag == b"E":
            raise RuntimeError("pg: " + body.decode("utf-8", "replace"))
        if tag == b"Z":
            return


def session(statements):
    url = os.environ["DATABASE_URL"]
    query = url.split("?", 1)[1] if "?" in url else ""
    params = dict(part.split("=", 1) for part in query.split("&") if "=" in part)
    path = os.path.join(params.get("host", "/proc/self/cwd"),
                        ".s.PGSQL." + params.get("port", "5432"))
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(10)
    sock.connect(path)
    startup = struct.pack("!I", 196608) + b"user\0blastit\0database\0app\0\0"
    sock.sendall(struct.pack("!I", len(startup) + 4) + startup)
    _until_ready(sock)
    try:
        for statement in statements:
            body = statement.encode() + b"\0"
            sock.sendall(b"Q" + struct.pack("!I", len(body) + 4) + body)
            _until_ready(sock)
    finally:
        sock.close()


with open(os.environ["BLASTIT_CHECK_HTTP_OBS"], "a", encoding="utf-8") as sink:
    for path in ("/cart/7", "/orders/9"):
        sink.write(json.dumps({"method": "GET", "path": path}) + "\n")


def _control():
    channel = socket.socket(
        socket.AF_UNIX,
        socket.SOCK_STREAM,
        fileno=int(os.environ["BLASTIT_CHECK_CTL_FD"]),
    )
    channel.settimeout(5)
    return channel


def _send(channel, payload):
    channel.sendall(json.dumps(payload).encode() + b"\n")
    reply = b""
    while not reply.endswith(b"\n"):
        reply += channel.recv(4096)
    if json.loads(reply).get("ok") is not True:
        raise RuntimeError("the control channel refused a request")


channel = _control()
_send(channel, {"op": "push_unrouted", "method": "GET", "path": "/orders/9"})
session(["SELECT 1", "SELECT 2", "SELECT 3"])
_send(channel, {"op": "pop"})
"#;

/// Writes the shim, the suite and the manifest into `dir`, and returns the shim
/// the run is pointed at with `BLASTIT_PYTHON`.
pub fn write_fixture(dir: &Path, python: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(dir.join("Blastfile"), MANIFEST).expect("the fixture manifest");
    std::fs::write(dir.join("suite.py"), SUITE).expect("the fixture suite");
    let shim = dir.join("python3");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\n\
             case \"$*\" in *pytest_django*) exit 1 ;; *-c*) exit 0 ;; esac\n\
             exec {python} {suite} \"$@\"\n",
            python = shell_quote(python),
            suite = shell_quote(&dir.join("suite.py")),
        ),
    )
    .expect("the shim interpreter");
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
        .expect("an executable shim");
    shim
}

/// One `'…'`-quoted word, so a path with spaces still reaches `exec`.
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// Runs `<blastit> check --learn --dry-run` in `dir` with `url` as the store.
pub fn dry_run(dir: &Path, binary: &Path, shim: &Path, url: &str) -> Output {
    let mut command = Command::new(binary);
    command
        .args(["check", "--learn", "--dry-run"])
        .current_dir(dir)
        .env_clear()
        .env("BLASTIT_PYTHON", shim)
        .env("DATABASE_URL", url)
        .env(
            "LD_LIBRARY_PATH",
            std::env::var_os("LD_LIBRARY_PATH").unwrap_or_default(),
        )
        .env("TMPDIR", std::env::var_os("TMPDIR").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
        .output()
        .expect("run `blastit check --learn --dry-run`")
}
