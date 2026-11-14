//! Protected critical tests: the param builder, the allowlist wall and the
//! binary refusal matrix (design `docs/design/php-fastcgi.md` §4.6, §4.7;
//! rows owned by `MB.3.4.2`).
//!
//! Every row is binary: request bytes in, one typed [`LockdownError`] or the
//! exact param stream out. The status mapping (`Refusal`) is `MB.3.6.2.1`'s and
//! is deliberately absent here. Each test is named for its §4.7 owner; the
//! positive twin proves the documented `public/index.php` request serialises to
//! the exact stream, so a builder that dropped everything could not pass.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_fastcgi::lockdown::{
    FastCgiPaths, LockdownError, Params, RequestFacts, Scheme, build, resolve,
};

fn paths() -> FastCgiPaths {
    FastCgiPaths {
        script_filename: "/app/public/index.php".into(),
        document_root: "/app/public".into(),
        script_name: "/index.php".into(),
    }
}

fn facts<'a>(target: &'a [u8], headers: &'a [(&'a [u8], &'a [u8])]) -> RequestFacts<'a> {
    RequestFacts {
        method: b"GET",
        target,
        headers,
        server_name: "example.test",
        server_port: 8080,
        remote_addr: "203.0.113.9",
        remote_port: 51234,
        scheme: Scheme::Http,
        content_type: None,
        content_length: None,
    }
}

fn params_for<'a>(target: &'a [u8], headers: &'a [(&'a [u8], &'a [u8])]) -> Params {
    build(&paths(), &facts(target, headers)).expect("builds")
}

fn value<'a>(params: &'a Params, name: &[u8]) -> &'a [u8] {
    params
        .get(name)
        .unwrap_or_else(|| panic!("missing param {}", String::from_utf8_lossy(name)))
}

/// A name outside the closed allowlist is absent from the stream *and* refused
/// by the builder API (design §4.6.1, §4.7 row 18).
fn assert_never_emitted(params: &Params, names: &[&[u8]]) {
    for name in names {
        assert!(
            params.get(name).is_none(),
            "{}",
            String::from_utf8_lossy(name)
        );
        assert_eq!(
            resolve(name, &paths(), &facts(b"/", &[])),
            Err(LockdownError::UnknownParam),
            "{}",
            String::from_utf8_lossy(name)
        );
    }
}

/// No param name can be an argv-style `-d …` token.
fn assert_no_dash_names(params: &Params) {
    assert!(
        params
            .pairs()
            .iter()
            .all(|(name, _)| !name.starts_with(b"-"))
    );
}

/// Row 1 (`E1`): a path and a header naming `SCRIPT_FILENAME` cannot move it.
#[test]
fn script_filename_is_pinned_ignoring_path_and_header() {
    let headers: [(&[u8], &[u8]); 1] = [(b"SCRIPT_FILENAME", b"/app/elsewhere/evil.php")];
    let params = params_for(b"/app/elsewhere/evil", &headers);
    assert_eq!(value(&params, b"SCRIPT_FILENAME"), b"/app/public/index.php");
    assert_eq!(value(&params, b"PATH_INFO"), b"/app/elsewhere/evil");
    // The header is data only: it folds to HTTP_SCRIPT_FILENAME.
    assert_eq!(
        value(&params, b"HTTP_SCRIPT_FILENAME"),
        b"/app/elsewhere/evil.php"
    );
    assert_eq!(
        resolve(b"SCRIPT_FILENAME", &paths(), &facts(b"/", &[])),
        Ok(Some(b"/app/public/index.php".to_vec()))
    );
}

/// Row 2's lockdown half: a traversal path is data and never feeds the script.
/// The authoritative `400` oracle stays `conformance_static::malformed_path`.
#[test]
fn path_info_traversal_never_feeds_script_filename() {
    for target in [
        &b"/../index.php"[..],
        b"/%2e%2e/index.php",
        b"/../../etc/passwd",
        b"/index.php/../../evil.php",
    ] {
        let params = params_for(target, &[]);
        assert_eq!(value(&params, b"SCRIPT_FILENAME"), b"/app/public/index.php");
        assert_eq!(value(&params, b"SCRIPT_NAME"), b"/index.php");
        assert_eq!(value(&params, b"REQUEST_URI"), target);
    }
}

/// Row 3 (`E3`): the pinned value has no `?`; the query stays its own param.
#[test]
fn script_filename_never_absorbs_query_string() {
    let params = params_for(b"/index.php?x=/etc/passwd", &[]);
    assert_eq!(value(&params, b"SCRIPT_FILENAME"), b"/app/public/index.php");
    assert!(!value(&params, b"SCRIPT_FILENAME").contains(&b'?'));
    assert_eq!(value(&params, b"QUERY_STRING"), b"x=/etc/passwd");
    assert_eq!(value(&params, b"REQUEST_URI"), b"/index.php?x=/etc/passwd");
}

/// Row 5 (`E5`): `PHP_VALUE` is not in the allowlist on any channel.
#[test]
fn php_value_never_emitted_from_any_input() {
    let headers: [(&[u8], &[u8]); 1] = [(b"PHP_VALUE", b"allow_url_include=1")];
    let params = params_for(b"/PHP_VALUE?PHP_VALUE=allow_url_include%3d1", &headers);
    assert_never_emitted(&params, &[b"PHP_VALUE"]);
    assert_eq!(
        value(&params, b"QUERY_STRING"),
        b"PHP_VALUE=allow_url_include%3d1"
    );
    assert_eq!(value(&params, b"HTTP_PHP_VALUE"), b"allow_url_include=1");
}

/// Row 6 (`E6`): `PHP_ADMIN_VALUE` and every `PHP_ADMIN_*` name are refused.
#[test]
fn php_admin_value_never_emitted_from_any_input() {
    let headers: [(&[u8], &[u8]); 2] = [
        (b"PHP_ADMIN_VALUE", b"open_basedir=/"),
        (b"PHP-Admin-Extra", b"x"),
    ];
    let params = params_for(b"/?PHP_ADMIN_VALUE=open_basedir%3d%2f", &headers);
    assert_never_emitted(
        &params,
        &[b"PHP_ADMIN_VALUE", b"PHP_ADMIN_EXTRA", b"PHP_ADMIN_ANY"],
    );
    assert_eq!(value(&params, b"HTTP_PHP_ADMIN_VALUE"), b"open_basedir=/");
}

/// Row 7 (`E7`): the prepend/URL/`open_basedir`/`disable_functions` chain.
#[test]
fn auto_prepend_directives_never_emitted() {
    let headers: [(&[u8], &[u8]); 4] = [
        (b"Auto-Prepend-File", b"/tmp/x.php"),
        (b"Allow-Url-Include", b"1"),
        (b"Open-BASEDIR", b"/"),
        (b"Disable-Functions", b"exec"),
    ];
    let params = params_for(
        b"/?auto_prepend_file=/tmp/x.php&allow_url_include=1&open_basedir=/&disable_functions=exec",
        &headers,
    );
    assert_never_emitted(
        &params,
        &[
            b"auto_prepend_file",
            b"allow_url_include",
            b"open_basedir",
            b"disable_functions",
        ],
    );
    assert_eq!(
        value(&params, b"QUERY_STRING"),
        b"auto_prepend_file=/tmp/x.php&allow_url_include=1&open_basedir=/&disable_functions=exec"
    );
}

/// Row 8 (`E8`, CVE-2012-1823): the query is one verbatim value, never argv.
#[test]
fn query_string_dash_d_stays_verbatim_query_data() {
    let target = b"/?-d+allow_url_include=1+-d+auto_prepend_file=php://input";
    let params = params_for(target, &[]);
    assert_eq!(value(&params, b"QUERY_STRING"), &target[2..]);
    assert_no_dash_names(&params);
    assert!(params.get(b"allow_url_include").is_none());
    // Only the two verbatim carriers may hold the query bytes.
    let needle = b"-d+allow_url_include=1";
    let carriers: Vec<&[u8]> = params
        .pairs()
        .iter()
        .filter(|(_, value)| value.windows(needle.len()).any(|window| window == needle))
        .map(|(name, _)| name.as_slice())
        .collect();
    assert_eq!(carriers, [&b"REQUEST_URI"[..], b"QUERY_STRING"]);
}

/// Row 9 (`E9`, CVE-2024-4577): the soft-hyphen probe stays query data and the
/// path is decoded exactly once.
#[test]
fn soft_hyphen_probe_stays_verbatim_query_data() {
    let params = params_for(b"/%ADd?%ADd+allow_url_include%3d1", &[]);
    assert_eq!(value(&params, b"PATH_INFO"), b"/\xADd");
    assert_eq!(
        value(&params, b"QUERY_STRING"),
        b"%ADd+allow_url_include%3d1"
    );
    assert!(params.get(b"allow_url_include").is_none());
    assert_no_dash_names(&params);
}

/// Row 10 (`E10`, CVE-2024-8926): best-fit/encoding variants never become params.
#[test]
fn best_fit_encoding_probes_never_become_params() {
    for probe in [
        &b"%EF%BC%8Dd+allow_url_include%3d1"[..],
        b"%C2%ADd+allow_url_include%3d1",
        b"%E2%80%90d+allow_url_include%3d1",
        b"%2Dd+allow_url_include%3d1",
        b"%252Dd+allow_url_include%3d1",
    ] {
        let target = [&b"/?"[..], probe].concat();
        let params = params_for(&target, &[]);
        assert_eq!(value(&params, b"QUERY_STRING"), probe);
        assert!(params.get(b"allow_url_include").is_none());
        assert_no_dash_names(&params);
    }
    // One decode, never two: `%252D` is `%2D`, not `-`.
    let params = params_for(b"/%252Dd?x=1", &[]);
    assert_eq!(value(&params, b"PATH_INFO"), b"/%2Dd");
}

/// Row 11 (`E11`, CVE-2024-8927): `REDIRECT_STATUS` is never emitted.
#[test]
fn redirect_status_is_never_emitted() {
    let headers: [(&[u8], &[u8]); 1] = [(b"Redirect-Status", b"200")];
    let params = params_for(b"/?REDIRECT_STATUS=200", &headers);
    assert_never_emitted(&params, &[b"REDIRECT_STATUS"]);
    assert_eq!(value(&params, b"HTTP_REDIRECT_STATUS"), b"200");
    assert_eq!(value(&params, b"QUERY_STRING"), b"REDIRECT_STATUS=200");
}

/// Row 12 (`E12`): `DOCUMENT_ROOT` is pinned; a header cannot select a tree.
#[test]
fn document_root_is_pinned_ignoring_headers() {
    let headers: [(&[u8], &[u8]); 1] = [(b"Document-Root", b"/etc")];
    let params = params_for(b"/?document_root=/etc", &headers);
    assert_eq!(value(&params, b"DOCUMENT_ROOT"), b"/app/public");
    assert_eq!(value(&params, b"HTTP_DOCUMENT_ROOT"), b"/etc");
}

/// Row 13 (`E13`, httpoxy): `Proxy:` never produces `HTTP_PROXY`.
#[test]
fn proxy_header_never_produces_http_proxy() {
    let headers: [(&[u8], &[u8]); 3] = [
        (b"Proxy", b"http://evil"),
        (b"Proxy-Connection", b"keep-alive"),
        (b"HTTP_PROXY", b"http://evil2"),
    ];
    let params = params_for(b"/", &headers);
    assert_never_emitted(&params, &[b"HTTP_PROXY", b"HTTP_PROXY_CONNECTION"]);
    // A header literally named HTTP_PROXY is data, not the param.
    assert_eq!(value(&params, b"HTTP_HTTP_PROXY"), b"http://evil2");
    assert_eq!(
        params.dropped(),
        [b"HTTP_PROXY".to_vec(), b"HTTP_PROXY_CONNECTION".to_vec()]
    );
}

/// Row 14 (`E14`): colliding names keep the first value and record the drops.
#[test]
fn colliding_http_headers_keep_first_and_record_drop() {
    let headers: [(&[u8], &[u8]); 3] = [(b"X-Foo", b"1"), (b"X_Foo", b"2"), (b"X-Foo", b"3")];
    let params = params_for(b"/", &headers);
    assert_eq!(value(&params, b"HTTP_X_FOO"), b"1");
    assert_eq!(
        params.dropped(),
        [b"HTTP_X_FOO".to_vec(), b"HTTP_X_FOO".to_vec()]
    );
    assert_eq!(
        params
            .pairs()
            .iter()
            .filter(|(name, _)| name == b"HTTP_X_FOO")
            .count(),
        1
    );
}

/// Row 15's lockdown half: a NUL/CR/LF value is refused, never truncated.
#[test]
fn nul_in_header_value_is_malformed() {
    for value in [&b"a\0b"[..], b"a\rb", b"a\nb"] {
        let headers: [(&[u8], &[u8]); 1] = [(b"X-Foo", value)];
        assert_eq!(
            build(&paths(), &facts(b"/", &headers)),
            Err(LockdownError::Malformed)
        );
    }
}

/// Row 16's lockdown half: an unrepresentable header name is dropped, not refused.
#[test]
fn unrepresentable_header_name_is_dropped() {
    let headers: [(&[u8], &[u8]); 5] = [
        (b"X.Foo", b"1"),
        (b"-x", b"2"),
        (b"\xC3\x9Cnicode", b"3"),
        // A digit-leading name is a legal RFC 7230 token from the front end and
        // still starts with a digit after folding (§4.6.2), so it is dropped.
        (b"1x", b"4"),
        // The empty name is reachable only through this builder API; it folds to
        // nothing, so the `HTTP_` prefix alone is unrepresentable too.
        (b"", b"4"),
    ];
    let params = params_for(b"/", &headers);
    assert_eq!(params.len(), 16, "only the fixed params");
    assert_eq!(
        params.dropped(),
        [
            b"HTTP_X.FOO".to_vec(),
            b"HTTP__X".to_vec(),
            b"HTTP_\xC3\x9CNICODE".to_vec(),
            b"HTTP_1X".to_vec(),
            b"HTTP_".to_vec(),
        ]
    );
    for name in [&b"HTTP_1X"[..], b"HTTP_"] {
        assert!(
            params.get(name).is_none(),
            "{}",
            String::from_utf8_lossy(name)
        );
    }
}

/// Design §4.6.2: a header value above 8 KiB is dropped, the request proceeds.
#[test]
fn over_long_header_value_is_dropped_not_refused() {
    let exact = vec![b'v'; 8_192];
    let over = vec![b'v'; 8_193];
    let headers: [(&[u8], &[u8]); 2] = [(b"Cookie", &exact), (b"X-Big", &over)];
    let params = params_for(b"/", &headers);
    assert_eq!(value(&params, b"HTTP_COOKIE").len(), 8_192);
    assert!(params.get(b"HTTP_X_BIG").is_none());
    assert_eq!(params.dropped(), [b"HTTP_X_BIG".to_vec()]);
}

/// Design §4.6.2: the ≤ 8 192-byte value rule also covers `CONTENT_TYPE`, which
/// comes from the head rather than an `HTTP_*` header. An over-long value is
/// dropped and recorded, never refused; `CONTENT_LENGTH` is a decimal `u64`
/// (at most 20 bytes), so the rule cannot bite there.
#[test]
fn over_long_content_type_is_dropped_not_refused() {
    let exact = vec![b'v'; 8_192];
    let over = vec![b'v'; 8_193];

    let mut at_limit = facts(b"/", &[]);
    at_limit.content_type = Some(exact.as_slice());
    at_limit.content_length = Some(u64::MAX);
    let params = build(&paths(), &at_limit).expect("builds");
    assert_eq!(value(&params, b"CONTENT_TYPE").len(), 8_192);
    assert_eq!(value(&params, b"CONTENT_LENGTH"), b"18446744073709551615");
    assert!(params.dropped().is_empty());

    let mut over_limit = facts(b"/", &[]);
    over_limit.content_type = Some(over.as_slice());
    let params = build(&paths(), &over_limit).expect("builds");
    assert!(params.get(b"CONTENT_TYPE").is_none());
    assert_eq!(params.dropped(), [b"CONTENT_TYPE".to_vec()]);
    // The rest of the stream is unaffected.
    assert_eq!(value(&params, b"REQUEST_METHOD"), b"GET");
    assert_eq!(value(&params, b"SCRIPT_FILENAME"), b"/app/public/index.php");
}

/// Row 17 (`E17`): the count and byte budgets, including their exact boundaries.
#[test]
fn over_budget_param_count_or_bytes_is_params_too_large() {
    let names: Vec<String> = (0..113).map(|index| format!("X-{index}")).collect();
    let headers: Vec<(&[u8], &[u8])> = names
        .iter()
        .map(|name| (name.as_bytes(), &b"1"[..]))
        .collect();
    let mut with_body = facts(b"/", &headers[..110]);
    with_body.content_type = Some(b"text/plain");
    with_body.content_length = Some(0);
    // 18 fixed params + 110 headers = 128: the boundary is allowed.
    assert_eq!(build(&paths(), &with_body).expect("128 params").len(), 128);
    let mut over = facts(b"/", &headers[..111]);
    over.content_type = Some(b"text/plain");
    over.content_length = Some(0);
    assert_eq!(
        build(&paths(), &over).expect_err("129 params"),
        LockdownError::ParamsTooLarge
    );
    // Without a body two fixed params vanish: 112 headers fit, 113 do not.
    assert_eq!(
        build(&paths(), &facts(b"/", &headers[..112]))
            .expect("128")
            .len(),
        128
    );
    assert_eq!(
        build(&paths(), &facts(b"/", &headers)).expect_err("129"),
        LockdownError::ParamsTooLarge
    );

    // The single-pair, total and count boundaries through the insertion point
    // are crate-private (`Params::insert` is `pub(crate)`), so they live in
    // `lockdown::tests::insert_enforces_the_pair_total_and_count_budgets`.

    // A representable but huge header name blows the pair budget.
    let huge = vec![b'X'; 60_000];
    let headers: [(&[u8], &[u8]); 1] = [(&huge, &vec![b'v'; 8_192])];
    assert_eq!(
        build(&paths(), &facts(b"/", &headers)),
        Err(LockdownError::ParamsTooLarge)
    );
}

/// Row 18: an unknown bare name is the internal builder guard, never a status.
#[test]
fn unknown_bare_param_is_refused_500() {
    for name in [
        &b"FOO_BAR"[..],
        b"PHP_VALUE",
        b"PHP_ADMIN_VALUE",
        b"PHP_ADMIN_EXTRA",
        b"auto_prepend_file",
        b"allow_url_include",
        b"open_basedir",
        b"disable_functions",
        b"REDIRECT_STATUS",
        b"HTTP_PROXY",
        b"FCGI_ROLE",
        b"PATH_TRANSLATED",
        b"SERVER_ADDR",
        b"SCRIPT_FILENAME\0",
    ] {
        assert_eq!(
            resolve(name, &paths(), &facts(b"/", &[])),
            Err(LockdownError::UnknownParam),
            "{}",
            String::from_utf8_lossy(name)
        );
    }
    assert_eq!(
        resolve(b"GATEWAY_INTERFACE", &paths(), &facts(b"/", &[])),
        Ok(Some(b"CGI/1.1".to_vec()))
    );
}

/// Row 23 (`E23`): `..` without `[route.static]` is data; the script stays pinned.
#[test]
fn dotdot_without_static_keeps_script_filename_pinned() {
    let params = params_for(b"/../../etc/passwd", &[]);
    assert_eq!(value(&params, b"SCRIPT_FILENAME"), b"/app/public/index.php");
    assert_eq!(value(&params, b"PATH_INFO"), b"/../../etc/passwd");
    assert_eq!(value(&params, b"REQUEST_URI"), b"/../../etc/passwd");
}

/// The positive twin: the documented `public/index.php` request, exact stream.
#[test]
fn documented_index_request_serialises_to_the_exact_param_stream() {
    let headers: [(&[u8], &[u8]); 4] = [
        (b"Host", b"example.test"),
        (b"Accept", b"*/*"),
        (b"X-Forwarded-For", b"203.0.113.9"),
        (b"Cookie", b"a=b"),
    ];
    let params = params_for(b"/users/42?tab=1", &headers);
    let expected: Vec<(&[u8], &[u8])> = vec![
        (b"SCRIPT_FILENAME", b"/app/public/index.php"),
        (b"SCRIPT_NAME", b"/index.php"),
        (b"PATH_INFO", b"/users/42"),
        (b"DOCUMENT_ROOT", b"/app/public"),
        (b"REQUEST_URI", b"/users/42?tab=1"),
        (b"QUERY_STRING", b"tab=1"),
        (b"REQUEST_METHOD", b"GET"),
        (b"SERVER_PROTOCOL", b"HTTP/1.1"),
        (b"GATEWAY_INTERFACE", b"CGI/1.1"),
        (
            b"SERVER_SOFTWARE",
            concat!("blastit/", env!("CARGO_PKG_VERSION")).as_bytes(),
        ),
        (b"SERVER_NAME", b"example.test"),
        (b"SERVER_PORT", b"8080"),
        (b"REMOTE_ADDR", b"203.0.113.9"),
        (b"REMOTE_PORT", b"51234"),
        (b"REQUEST_SCHEME", b"http"),
        (b"HTTPS", b"off"),
        (b"HTTP_HOST", b"example.test"),
        (b"HTTP_ACCEPT", b"*/*"),
        (b"HTTP_X_FORWARDED_FOR", b"203.0.113.9"),
        (b"HTTP_COOKIE", b"a=b"),
    ];
    let got: Vec<(&[u8], &[u8])> = params
        .pairs()
        .iter()
        .map(|(name, value)| (name.as_slice(), value.as_slice()))
        .collect();
    assert_eq!(got, expected);
    assert!(params.dropped().is_empty());

    // The full expected `PARAMS` content, pair by pair as explicit byte chunks:
    // every name and value literal, so a reordering or an extra param fails.
    // `SERVER_SOFTWARE` is the one value that embeds the crate version.
    let software = concat!("blastit/", env!("CARGO_PKG_VERSION"));
    let mut want: Vec<u8> = Vec::new();
    want.extend_from_slice(b"\x0f\x15SCRIPT_FILENAME/app/public/index.php");
    want.extend_from_slice(b"\x0b\x0aSCRIPT_NAME/index.php");
    want.extend_from_slice(b"\x09\x09PATH_INFO/users/42");
    want.extend_from_slice(b"\x0d\x0bDOCUMENT_ROOT/app/public");
    want.extend_from_slice(b"\x0b\x0fREQUEST_URI/users/42?tab=1");
    want.extend_from_slice(b"\x0c\x05QUERY_STRINGtab=1");
    want.extend_from_slice(b"\x0e\x03REQUEST_METHODGET");
    want.extend_from_slice(b"\x0f\x08SERVER_PROTOCOLHTTP/1.1");
    want.extend_from_slice(b"\x11\x07GATEWAY_INTERFACECGI/1.1");
    want.push(0x0f);
    want.push(u8::try_from(software.len()).expect("one prefix byte"));
    want.extend_from_slice(b"SERVER_SOFTWARE");
    want.extend_from_slice(software.as_bytes());
    want.extend_from_slice(b"\x0b\x0cSERVER_NAMEexample.test");
    want.extend_from_slice(b"\x0b\x04SERVER_PORT8080");
    want.extend_from_slice(b"\x0b\x0bREMOTE_ADDR203.0.113.9");
    want.extend_from_slice(b"\x0b\x05REMOTE_PORT51234");
    want.extend_from_slice(b"\x0e\x04REQUEST_SCHEMEhttp");
    want.extend_from_slice(b"\x05\x03HTTPSoff");
    want.extend_from_slice(b"\x09\x0cHTTP_HOSTexample.test");
    want.extend_from_slice(b"\x0b\x03HTTP_ACCEPT*/*");
    want.extend_from_slice(b"\x14\x0bHTTP_X_FORWARDED_FOR203.0.113.9");
    want.extend_from_slice(b"\x0b\x03HTTP_COOKIEa=b");

    let encoded = params.encode().expect("encodes");
    assert_eq!(encoded, want);
    assert_eq!(params.encoded_len(), encoded.len());
    assert!(!encoded.contains(&0), "no NUL in the stream");
}

/// Design §4.6.1/§4.6.2: `HTTP_*` folding, and the two content params are not
/// header-derived.
#[test]
fn http_star_derivation_folds_names_and_skips_content_headers() {
    let headers: [(&[u8], &[u8]); 6] = [
        (b"Content-Type", b"text/plain"),
        (b"Content-Length", b"999"),
        (b"X-Foo", b"a"),
        (b"x-lower", b"b"),
        (b"X.Foo", b"c"),
        (b"Connection", b"close"),
    ];
    let mut facts = facts(b"/", &headers);
    facts.content_type = Some(b"application/json");
    facts.content_length = Some(3);
    let params = build(&paths(), &facts).expect("builds");
    assert_eq!(value(&params, b"CONTENT_TYPE"), b"application/json");
    assert_eq!(value(&params, b"CONTENT_LENGTH"), b"3");
    assert!(params.get(b"HTTP_CONTENT_TYPE").is_none());
    assert!(params.get(b"HTTP_CONTENT_LENGTH").is_none());
    assert_eq!(value(&params, b"HTTP_X_FOO"), b"a");
    assert_eq!(value(&params, b"HTTP_X_LOWER"), b"b");
    assert!(params.get(b"HTTP_CONNECTION").is_none());
    assert_eq!(
        params.dropped(),
        [b"HTTP_X.FOO".to_vec(), b"HTTP_CONNECTION".to_vec()]
    );
}

/// Design §4.6.2 "one decode": `PATH_INFO` is decoded once, other bytes verbatim.
#[test]
fn path_info_is_decoded_exactly_once() {
    let params = params_for(b"/%2525/%2e%2e/x?a=%2525", &[]);
    assert_eq!(value(&params, b"PATH_INFO"), b"/%25/../x");
    assert_eq!(value(&params, b"QUERY_STRING"), b"a=%2525");
    // An invalid escape is kept literally, and a decoded NUL is refused.
    assert_eq!(value(&params_for(b"/%zz", &[]), b"PATH_INFO"), b"/%zz");
    assert_eq!(
        build(&paths(), &facts(b"/%00", &[])),
        Err(LockdownError::Malformed)
    );
    // `/` is the empty path info, and the scheme drives REQUEST_SCHEME/HTTPS.
    let params = params_for(b"/", &[]);
    assert_eq!(value(&params, b"PATH_INFO"), b"");
    let mut tls = facts(b"/", &[]);
    tls.scheme = Scheme::Https;
    let tls = build(&paths(), &tls).expect("builds");
    assert_eq!(value(&tls, b"REQUEST_SCHEME"), b"https");
    assert_eq!(value(&tls, b"HTTPS"), b"on");
}

/// A malformed request fact is refused, never coerced into a param byte.
#[test]
fn malformed_request_facts_are_refused() {
    let mut bad_method = facts(b"/", &[]);
    bad_method.method = b"GE T";
    assert_eq!(build(&paths(), &bad_method), Err(LockdownError::Malformed));
    let mut empty_name = facts(b"/", &[]);
    empty_name.server_name = "";
    assert_eq!(build(&paths(), &empty_name), Err(LockdownError::Malformed));
    let mut nul_target = facts(b"/\0", &[]);
    nul_target.method = b"GET";
    assert_eq!(build(&paths(), &nul_target), Err(LockdownError::Malformed));
    let mut nul_type = facts(b"/", &[]);
    nul_type.content_type = Some(b"text/plain\0");
    assert_eq!(build(&paths(), &nul_type), Err(LockdownError::Malformed));
    let mut crlf_addr = facts(b"/", &[]);
    crlf_addr.remote_addr = "1.2.3.4\r\nX: y";
    assert_eq!(build(&paths(), &crlf_addr), Err(LockdownError::Malformed));
}

/// A non-absolute or `..`-bearing pinned path is a builder error, not a param.
#[test]
fn malformed_pinned_paths_are_refused() {
    for (script_filename, document_root, script_name) in [
        ("relative/index.php", "/app/public", "/index.php"),
        (
            "/app/public/../private/index.php",
            "/app/public",
            "/index.php",
        ),
        ("/app/public/index.php", "/app/public", "index.php"),
        ("/app/public/index.php", "/app/public", "/../index.php"),
        ("/app/public/index.php?x", "/app/public", "/index.php"),
    ] {
        let paths = FastCgiPaths {
            script_filename: script_filename.into(),
            document_root: document_root.into(),
            script_name: script_name.into(),
        };
        assert_eq!(
            build(&paths, &facts(b"/", &[])),
            Err(LockdownError::Malformed),
            "{script_filename} {document_root} {script_name}"
        );
    }
}

/// Design §4.6.2: `SCRIPT_NAME` is `?`-free; a query in the pinned value is a
/// builder error, never a param byte.
#[test]
fn script_name_with_a_query_is_malformed() {
    let mut queried = paths();
    queried.script_name = "/index.php?x".into();
    assert_eq!(
        build(&queried, &facts(b"/", &[])),
        Err(LockdownError::Malformed)
    );

    // The accepted twin still builds, and only the verbatim query carriers hold
    // a `?` — never the pinned script name.
    let params = build(&paths(), &facts(b"/", &[])).expect("builds");
    assert_eq!(value(&params, b"SCRIPT_NAME"), b"/index.php");
    assert!(!value(&params, b"SCRIPT_NAME").contains(&b'?'));
    assert_eq!(value(&params, b"QUERY_STRING"), b"");
}

/// `R19` (design §4.6.2): the encoded ≤ 65 535 **total** cap fires through the
/// public `build`, while one pair stays below the 65 531 pair cap and every
/// 8 192-byte value is legal (only a value above 8 192 is dropped).
#[test]
fn the_total_param_byte_budget_is_enforced_through_build() {
    // Eight headers `X-A` .. `X-H` of exactly 8 192 value bytes. Each folds to
    // the 8-byte param `HTTP_X_A`, so its encoded pair is
    // 1 (name length) + 4 (value length) + 8 + 8 192 = 8 205 bytes — well under
    // the 65 531 pair cap, so the pair branch can never fire. The 16 fixed
    // params of this request encode to 340 bytes, so seven pairs are
    // 340 + 57 435 = 57 775 (fits) and eight are 65 980 (over 65 535): only the
    // total branch can refuse, and only on the eighth pair.
    const NAME_PREFIX: &[u8] = b"HTTP_X_";
    let names = ["X-A", "X-B", "X-C", "X-D", "X-E", "X-F", "X-G", "X-H"];
    let value = [b'v'; 8_192];

    let mut fixed = None;
    let mut pair = None;
    let mut seven = None;
    for count in 0..=names.len() {
        let headers: Vec<(&[u8], &[u8])> = names[..count]
            .iter()
            .map(|name| (name.as_bytes(), &value[..]))
            .collect();
        let built = build(&paths(), &facts(b"/", &headers));
        if count < names.len() {
            let params = built.expect("seven 8 192-byte values fit");
            let carried: Vec<&Vec<u8>> = params
                .pairs()
                .iter()
                .filter(|(name, _)| name.starts_with(NAME_PREFIX))
                .map(|(_, carried)| carried)
                .collect();
            assert_eq!(carried.len(), count);
            assert!(
                carried.iter().all(|carried| carried.len() == 8_192),
                "a legal 8 192-byte value is carried whole, never truncated"
            );
            assert!(
                params.dropped().is_empty(),
                "8 192 bytes is legal, never dropped"
            );
            if count == 0 {
                fixed = Some(params.encoded_len());
            }
            if count == 1 {
                // Measured, not assumed: the pair width must sit under the cap.
                pair = Some(params.encoded_len() - fixed.expect("the zero-header case built"));
            }
            seven = Some(params.encoded_len());
        } else {
            assert_eq!(built, Err(LockdownError::ParamsTooLarge));
        }
    }

    // The boundary is the total, not the pair: each measured pair is under the
    // pair cap, and the eighth one still pushes the encoded set past the total.
    let fixed = fixed.expect("the zero-header case built");
    let pair = pair.expect("the one-header case built");
    let seven = seven.expect("the seven-header case built");
    assert_eq!(pair, 1 + 4 + 8 + 8_192);
    assert!(pair < blastit_fastcgi::MAX_PARAM_PAIR_BYTES);
    assert_eq!(seven, fixed + 7 * pair);
    assert!(seven < blastit_fastcgi::MAX_PARAMS_BYTES);
    assert!(seven + pair > blastit_fastcgi::MAX_PARAMS_BYTES);
}

/// `R18` (design §4.6.2): the log-safe `Debug` view prints the param names and
/// never a value — a value routinely carries a cookie.
#[test]
fn params_debug_prints_names_never_values() {
    let headers: [(&[u8], &[u8]); 2] = [(b"Cookie", b"session=secret"), (b"X-Foo", b"1")];
    let params = params_for(b"/", &headers);
    assert_eq!(value(&params, b"HTTP_COOKIE"), b"session=secret");

    let debug = format!("{params:?}");
    assert!(debug.contains("names"), "{debug}");
    assert!(debug.contains("HTTP_COOKIE"), "{debug}");
    assert!(debug.contains("HTTP_X_FOO"), "{debug}");
    assert!(!debug.contains("session"), "{debug}");
    assert!(!debug.contains("secret"), "{debug}");
}

/// Design §4.6.1/§4.6.2: every hop-by-hop name is dropped before `HTTP_*`
/// derivation, ASCII case-insensitively, and recorded as its folded name.
#[test]
fn every_hop_by_hop_name_is_dropped() {
    let headers: [(&[u8], &[u8]); 9] = [
        (b"Connection", b"close"),
        (b"kEeP-aLiVe", b"timeout=5"),
        (b"PROXY-Authenticate", b"Basic"),
        (b"proxy-AUTHORIZATION", b"Basic x"),
        (b"Proxy-Connection", b"keep-alive"),
        (b"Te", b"trailers"),
        (b"TRAILER", b"x"),
        (b"transfer-ENCODING", b"chunked"),
        (b"UpGrade", b"websocket"),
    ];
    let params = params_for(b"/", &headers);
    assert_eq!(
        params.dropped(),
        [
            b"HTTP_CONNECTION".to_vec(),
            b"HTTP_KEEP_ALIVE".to_vec(),
            b"HTTP_PROXY_AUTHENTICATE".to_vec(),
            b"HTTP_PROXY_AUTHORIZATION".to_vec(),
            b"HTTP_PROXY_CONNECTION".to_vec(),
            b"HTTP_TE".to_vec(),
            b"HTTP_TRAILER".to_vec(),
            b"HTTP_TRANSFER_ENCODING".to_vec(),
            b"HTTP_UPGRADE".to_vec(),
        ]
    );
    for name in [
        &b"HTTP_CONNECTION"[..],
        b"HTTP_KEEP_ALIVE",
        b"HTTP_PROXY_AUTHENTICATE",
        b"HTTP_PROXY_AUTHORIZATION",
        b"HTTP_PROXY_CONNECTION",
        b"HTTP_TE",
        b"HTTP_TRAILER",
        b"HTTP_TRANSFER_ENCODING",
        b"HTTP_UPGRADE",
    ] {
        assert!(
            params.get(name).is_none(),
            "{}",
            String::from_utf8_lossy(name)
        );
    }
    // The nine headers add no param at all: only the 16 fixed params remain.
    assert_eq!(params.len(), 16);
}

/// `R18`/§4.5.1: the crate-wide error surface formats its inner error and
/// exposes it through `source`, and the four `LockdownError` phrases are
/// non-empty and pairwise distinct.
#[test]
fn error_display_and_source_are_not_empty() {
    let record = blastit_fastcgi::Error::from(blastit_fastcgi::record::RecordError::Truncated);
    assert_eq!(
        record.to_string(),
        blastit_fastcgi::record::RecordError::Truncated.to_string()
    );
    assert!(std::error::Error::source(&record).is_some());

    let cgi = blastit_fastcgi::Error::from(blastit_fastcgi::cgi::CgiError::BadHead);
    assert_eq!(
        cgi.to_string(),
        blastit_fastcgi::cgi::CgiError::BadHead.to_string()
    );
    assert!(std::error::Error::source(&cgi).is_some());

    let phrases = [
        LockdownError::Malformed.to_string(),
        LockdownError::UnknownParam.to_string(),
        LockdownError::ParamsTooLarge.to_string(),
        LockdownError::DuplicateParam.to_string(),
    ];
    assert!(phrases.iter().all(|phrase| !phrase.is_empty()));
    for (index, phrase) in phrases.iter().enumerate() {
        assert!(
            !phrases[..index].contains(phrase),
            "the phrases must be pairwise distinct, duplicate: {phrase}"
        );
    }
}
