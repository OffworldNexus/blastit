//! The two-view live-reload project: the manifest every view is replaced from,
//! and the app whose preloaded `VERSION` a reload changes.
/// A live-reloading project whose views read a preloaded module, so breaking that
/// module reaches the interpreter's compile preflight rather than a worker.
pub const RELOAD_MANIFEST: &str = "\
app \"app\"
entrypoint python \"app:app\" {
    preload \"models\"
}

view app regex '.*' {}

view other GET \"/other/*\" {}
";

/// The reloading app: the preloaded `VERSION`, and a sleep long enough that an
/// in-flight request is still running when the file is saved under it.
pub const RELOAD_APP: &str = r#"
import models
import time


def app(environ, start_response):
    if environ.get("PATH_INFO", "/").endswith("/slow"):
        time.sleep(0.5)
    body = ("reload:%s\n" % models.VERSION).encode()
    start_response("200 OK", [("Content-Type", "text/plain"),
                              ("Content-Length", str(len(body)))])
    return [body]
"#;
