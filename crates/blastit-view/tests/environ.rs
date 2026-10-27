//! The WSGI `environ` the view builds for a request (PEP 3333).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use blastit_frame::{Frame, Scheme};
use blastit_view::ViewConfig;
use support::{Harness, echo_app, load_app, load_module};

/// Start an echo app for the given keys and serve one request with `head`.
///
/// The module is named after the key set, which keeps `sys.modules` tidy
/// without a counter: two tests that use the same keys get the same source, so
/// a name they share cannot surprise either of them.
fn echo(head: blastit_frame::ReqHead, keys: &[&str], route: &str) -> support::Response {
    let source = echo_app(keys);
    let name = format!("echo_{}", keys.join("_"));
    let mut harness = Harness::for_route(route, load_app(&name, &source));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));
    harness.exchange(Frame::ReqHead(head), &[], 1)
}

#[test]
fn the_standard_environment_is_there() {
    let response = echo(
        support::head("POST", "/x?y=1", Vec::new()),
        &[
            "REQUEST_METHOD",
            "SCRIPT_NAME",
            "PATH_INFO",
            "QUERY_STRING",
            "SERVER_NAME",
            "SERVER_PORT",
            "SERVER_PROTOCOL",
            "REMOTE_ADDR",
            "REMOTE_PORT",
            "blastit.route",
        ],
        "web",
    );
    assert_eq!(response.status(), 200);
    assert_eq!(response.field("REQUEST_METHOD"), "POST");
    assert_eq!(response.field("SCRIPT_NAME"), "");
    assert_eq!(response.field("PATH_INFO"), "/x");
    assert_eq!(response.field("QUERY_STRING"), "y=1");
    assert_eq!(response.field("SERVER_NAME"), "view.test");
    assert_eq!(response.field("SERVER_PORT"), "80");
    assert_eq!(response.field("SERVER_PROTOCOL"), "HTTP/1.1");
    // CGI and PEP 3333: the address is one key and the port is another.
    assert_eq!(
        response.field("REMOTE_ADDR"),
        "203.0.113.7",
        "the peer's port is not part of `REMOTE_ADDR`"
    );
    assert_eq!(response.field("REMOTE_PORT"), "51000");
    assert_eq!(response.field("blastit.route"), "web");
}

#[test]
fn a_peer_that_is_not_an_address_keeps_its_name_and_has_no_port() {
    // A socket with no name — an unnamed Unix socket — is not an `ip:port`, so
    // there is no port to report: the key is absent rather than empty. The
    // address stays what the front end could read, because an empty value would
    // claim the connection has no client at all.
    let mut head = support::head("GET", "/", Vec::new());
    head.remote_addr = "unnamed-socket".to_string();
    head.peer_addr = "unnamed-socket".to_string();
    let response = echo(head, &["REMOTE_ADDR", "REMOTE_PORT"], "web");
    assert_eq!(response.field("REMOTE_ADDR"), "unnamed-socket");
    assert_eq!(
        response.field("REMOTE_PORT"),
        "None",
        "no port to report: {}",
        response.text()
    );
}

#[test]
fn path_info_is_percent_decoded_and_read_as_latin1() {
    // `%C3%A9` decodes to two bytes, which latin-1 reads as two characters:
    // the body carries exactly the bytes the client sent in the path.
    let response = echo(
        support::head("GET", "/caf%C3%A9/%2Fplus+a%zz", Vec::new()),
        &["PATH_INFO", "QUERY_STRING"],
        "web",
    );
    assert!(
        response.contains(b"PATH_INFO=/caf\xc3\xa9//plus+a%zz"),
        "latin-1 path: {}",
        response.text()
    );
}

#[test]
fn the_query_string_is_the_raw_bytes_after_the_first_question_mark() {
    let response = echo(
        support::head("GET", "/x?a=%20+1&b=%zz?c", Vec::new()),
        &["PATH_INFO", "QUERY_STRING"],
        "web",
    );
    assert_eq!(response.field("PATH_INFO"), "/x");
    assert_eq!(
        response.field("QUERY_STRING"),
        "a=%20+1&b=%zz?c",
        "the query string is not decoded"
    );
}

#[test]
fn a_request_without_a_query_string_has_an_empty_one() {
    let response = echo(
        support::head("GET", "/x", Vec::new()),
        &["QUERY_STRING"],
        "web",
    );
    assert_eq!(response.field("QUERY_STRING"), "");
}

#[test]
fn headers_become_http_keys_and_repeats_are_joined() {
    let response = echo(
        support::head(
            "GET",
            "/",
            vec![
                ("X-A", "1"),
                ("x-a", "2"),
                ("Cookie", "c=1"),
                ("Host", "view.test"),
            ],
        ),
        &["HTTP_X_A", "HTTP_COOKIE", "HTTP_HOST", "HTTP_ACCEPT"],
        "web",
    );
    assert_eq!(response.field("HTTP_X_A"), "1, 2", "repeats join in order");
    assert_eq!(response.field("HTTP_COOKIE"), "c=1");
    assert_eq!(response.field("HTTP_HOST"), "view.test");
    assert_eq!(response.field("HTTP_ACCEPT"), "None");
}

#[test]
fn a_header_name_with_an_underscore_is_dropped() {
    let response = echo(
        support::head("GET", "/", vec![("X_User", "evil"), ("X-User", "good")]),
        &["HTTP_X_USER"],
        "web",
    );
    assert_eq!(
        response.field("HTTP_X_USER"),
        "good",
        "only the dashed spelling may reach HTTP_*"
    );
    assert!(
        !response.contains(b"evil"),
        "the underscored header leaked into the environ"
    );
}

#[test]
fn an_empty_header_name_is_dropped() {
    let response = echo(
        support::head("GET", "/", vec![("", "evil")]),
        &["HTTP_"],
        "web",
    );
    assert_eq!(response.field("HTTP_"), "None");
    assert!(!response.contains(b"evil"));
}

#[test]
fn content_headers_are_not_http_keys() {
    let response = echo(
        support::head(
            "POST",
            "/",
            vec![
                ("Content-Type", "text/plain; charset=utf-8"),
                ("CONTENT-LENGTH", "12"),
            ],
        ),
        &["CONTENT_TYPE", "CONTENT_LENGTH", "HTTP_CONTENT_LENGTH"],
        "web",
    );
    assert_eq!(response.field("CONTENT_TYPE"), "text/plain; charset=utf-8");
    assert_eq!(response.field("CONTENT_LENGTH"), "12");
    assert_eq!(response.field("HTTP_CONTENT_LENGTH"), "None");
}

#[test]
fn the_script_name_is_stripped_from_the_path_only_on_a_boundary() {
    let source = echo_app(&["SCRIPT_NAME", "PATH_INFO"]);
    let app = load_app("script_name", &source);
    let cfg = ViewConfig {
        script_name: "/app".to_string(),
        ..ViewConfig::for_route("web")
    };
    let mut harness = Harness::start(app, cfg);
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.exchange(
        Frame::ReqHead(support::head("GET", "/app/list?page=2", Vec::new())),
        &[],
        1,
    );
    assert_eq!(response.field("SCRIPT_NAME"), "/app");
    assert_eq!(response.field("PATH_INFO"), "/list");

    let response = harness.exchange(
        Frame::ReqHead(support::head("GET", "/application", Vec::new())),
        &[],
        2,
    );
    assert_eq!(
        response.field("PATH_INFO"),
        "/application",
        "a sibling route's path is not mangled"
    );

    let response = harness.exchange(
        Frame::ReqHead(support::head("GET", "/app", Vec::new())),
        &[],
        3,
    );
    assert_eq!(response.field("PATH_INFO"), "");
    assert!(harness.finish().is_ok());
}

#[test]
fn the_wsgi_keys_are_present() {
    let module = load_module(
        "wsgi_keys",
        "def app(environ, start_response):\n\
         \x20   import sys\n\
         \x20   e = environ\n\
         \x20   lines = [\n\
         \x20       'version=' + repr(e['wsgi.version']),\n\
         \x20       'scheme=' + e['wsgi.url_scheme'],\n\
         \x20       'multithread=' + repr(e['wsgi.multithread']),\n\
         \x20       'multiprocess=' + repr(e['wsgi.multiprocess']),\n\
         \x20       'run_once=' + repr(e['wsgi.run_once']),\n\
         \x20       'input_terminated=' + repr(e['wsgi.input_terminated']),\n\
         \x20       'errors_is_stderr=' + repr(e['wsgi.errors'] is sys.stderr),\n\
         \x20       'input_is_a_stream=' + repr(all(hasattr(e['wsgi.input'], n) for n in ('read', 'readline', 'readlines'))),\n\
         \x20       'input_iterable=' + repr(iter(e['wsgi.input']) is not None),\n\
         \x20       'remote_addr=' + e['REMOTE_ADDR'],\n\
         \x20       'remote_port=' + e['REMOTE_PORT'],\n\
         \x20   ]\n\
         \x20   body = '\\n'.join(lines).encode('latin-1')\n\
         \x20   start_response('200 OK', [('Content-Type', 'text/plain')])\n\
         \x20   return [body]\n",
    );
    let mut harness = Harness::for_route("web", support::app_of(&module));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.field("version"), "(1, 0)");
    assert_eq!(response.field("scheme"), "http");
    assert_eq!(response.field("multithread"), "True");
    assert_eq!(response.field("multiprocess"), "True");
    assert_eq!(response.field("run_once"), "False");
    assert_eq!(response.field("input_terminated"), "True");
    assert_eq!(response.field("errors_is_stderr"), "True");
    assert_eq!(response.field("input_is_a_stream"), "True");
    assert_eq!(response.field("input_iterable"), "True");
    // The client's address, split the way CGI and PEP 3333 split it.
    assert_eq!(response.field("remote_addr"), "203.0.113.7");
    assert_eq!(response.field("remote_port"), "51000");

    // The scheme comes from the front end, not from the app.
    let mut https = support::head("GET", "/", Vec::new());
    https.scheme = Scheme::Https;
    let response = harness.exchange(Frame::ReqHead(https), &[], 1);
    assert_eq!(response.field("scheme"), "https");
    assert!(harness.finish().is_ok());
}

/// An application that reproduces the whole `environ`, one line per key.
///
/// Each line is `key|type|value`, in sorted key order, so a single comparison
/// pins the entire dictionary: which keys are present, what type each value is
/// (a `str` for a CGI value, a `bool` for a `wsgi` flag, a tuple for
/// `wsgi.version`) and what each value holds. The two objects that have no
/// useful `repr` are named — `wsgi.input` is `<input>` and `wsgi.errors` is
/// `stderr` when it is `sys.stderr`.
const DUMP_APP: &str = r#"
def app(environ, start_response):
    import sys
    lines = []
    for key in sorted(environ):
        value = environ[key]
        if key == 'wsgi.input':
            kind, shown = 'object', '<input>'
        elif key == 'wsgi.errors':
            kind, shown = 'object', ('stderr' if value is sys.stderr else 'other')
        else:
            kind, shown = type(value).__name__, repr(value)
        lines.append(key + '|' + kind + '|' + shown)
    body = '\n'.join(lines).encode('latin-1')
    start_response('200 OK', [('Content-Type', 'text/plain')])
    return [body]
"#;

#[test]
fn the_environ_is_exactly_this() {
    // The refactor that lets a request reuse constant objects could change the
    // dictionary in ways no per-key test notices — a key renamed, a value
    // turned from `str` to `bytes`, a flag turned from `bool` to `int`. This
    // pins the whole thing at once.
    let mut harness = Harness::for_route("web", load_app("environ_dump", DUMP_APP));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    // A plain GET: the `HTTP_*` keys the client asked for, a peer with a port,
    // no content headers.
    let response = harness.exchange(
        Frame::ReqHead(support::head(
            "GET",
            "/x?y=1",
            vec![("Host", "view.test"), ("Accept", "text/html")],
        )),
        &[],
        1,
    );
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.text(),
        "\
HTTP_ACCEPT|str|'text/html'\n\
HTTP_HOST|str|'view.test'\n\
PATH_INFO|str|'/x'\n\
QUERY_STRING|str|'y=1'\n\
REMOTE_ADDR|str|'203.0.113.7'\n\
REMOTE_PORT|str|'51000'\n\
REQUEST_METHOD|str|'GET'\n\
SCRIPT_NAME|str|''\n\
SERVER_NAME|str|'view.test'\n\
SERVER_PORT|str|'80'\n\
SERVER_PROTOCOL|str|'HTTP/1.1'\n\
blastit.route|str|'web'\n\
wsgi.errors|object|stderr\n\
wsgi.input|object|<input>\n\
wsgi.input_terminated|bool|True\n\
wsgi.multiprocess|bool|True\n\
wsgi.multithread|bool|True\n\
wsgi.run_once|bool|False\n\
wsgi.url_scheme|str|'http'\n\
wsgi.version|tuple|(1, 0)"
    );

    // A POST over https, from a peer that is not an address: `CONTENT_TYPE` and
    // `CONTENT_LENGTH` replace their `HTTP_*` spelling, and there is no
    // `REMOTE_PORT` to report.
    let mut head = support::head(
        "POST",
        "/submit",
        vec![("Content-Type", "text/plain"), ("Content-Length", "5")],
    );
    head.scheme = Scheme::Https;
    head.server_port = 443;
    head.remote_addr = "unnamed-socket".to_string();
    head.peer_addr = "unnamed-socket".to_string();
    let response = harness.exchange(Frame::ReqHead(head), &[], 2);
    assert_eq!(
        response.text(),
        "\
CONTENT_LENGTH|str|'5'\n\
CONTENT_TYPE|str|'text/plain'\n\
PATH_INFO|str|'/submit'\n\
QUERY_STRING|str|''\n\
REMOTE_ADDR|str|'unnamed-socket'\n\
REQUEST_METHOD|str|'POST'\n\
SCRIPT_NAME|str|''\n\
SERVER_NAME|str|'view.test'\n\
SERVER_PORT|str|'443'\n\
SERVER_PROTOCOL|str|'HTTP/1.1'\n\
blastit.route|str|'web'\n\
wsgi.errors|object|stderr\n\
wsgi.input|object|<input>\n\
wsgi.input_terminated|bool|True\n\
wsgi.multiprocess|bool|True\n\
wsgi.multithread|bool|True\n\
wsgi.run_once|bool|False\n\
wsgi.url_scheme|str|'https'\n\
wsgi.version|tuple|(1, 0)"
    );

    assert!(harness.finish().is_ok());
}
