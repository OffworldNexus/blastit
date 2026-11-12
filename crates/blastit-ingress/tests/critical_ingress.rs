//! Protected critical tests for `R14..R16` (design §4.2 A, `ADR-0006`).
//!
//! Every row of the `plan_body` / `transcode_buffered` matrix is exercised in
//! both directions: the body that must be accepted and re-encoded, and the
//! malicious or malformed twin that must be a typed `400`/`413`. A test that
//! only asserted the happy path would pass with validation deleted.

use blastit_ingress::{
    BodyCodec, BodyPlan, BodySample, IngressPolicy, IngressReject, MAX_CHECK_SAMPLE_BYTES,
    MAX_JSON_DEPTH, MAX_MIME_REPORT_BYTES, TranscodedBody, apply_transcoded_headers, parse_mime,
    plan_body, probe_sample, suggest_fix, transcode_buffered,
};

const JSON: &str = "application/json";
const FORM: &str = "application/x-www-form-urlencoded";
const JSON_CT: Option<&str> = Some(JSON);
const FORM_CT: Option<&str> = Some(FORM);

fn json_policy() -> Option<IngressPolicy> {
    Some(IngressPolicy {
        body: BodyCodec::Json,
        allow_raw: false,
    })
}

fn form_policy() -> Option<IngressPolicy> {
    Some(IngressPolicy {
        body: BodyCodec::Form,
        allow_raw: false,
    })
}

fn raw_policy() -> Option<IngressPolicy> {
    Some(IngressPolicy {
        body: BodyCodec::Raw,
        allow_raw: true,
    })
}

fn none_policy() -> Option<IngressPolicy> {
    Some(IngressPolicy {
        body: BodyCodec::None,
        allow_raw: false,
    })
}

/// The accepted outcome for `bytes`, so a test can compare a whole `Result`.
fn accepted(
    bytes: &[u8],
    content_type: Option<&'static str>,
) -> Result<TranscodedBody, IngressReject> {
    Ok(TranscodedBody {
        bytes: bytes.to_vec(),
        content_type,
    })
}

fn json_body(content_type: Option<&str>, raw: &[u8]) -> Result<TranscodedBody, IngressReject> {
    transcode_buffered(BodyCodec::Json, "POST", content_type, raw)
}

fn form_body(content_type: Option<&str>, raw: &[u8]) -> Result<TranscodedBody, IngressReject> {
    transcode_buffered(BodyCodec::Form, "POST", content_type, raw)
}

/// A document nested exactly `depth` arrays deep.
fn nested_json(depth: usize) -> Vec<u8> {
    let mut document = vec![b'['; depth];
    document.push(b'1');
    document.extend(vec![b']'; depth]);
    document
}

fn header_values<'a>(headers: &'a [(Vec<u8>, Vec<u8>)], name: &str) -> Vec<&'a [u8]> {
    headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name.as_bytes()))
        .map(|(_, value)| value.as_slice())
        .collect()
}

// ---------------------------------------------------------------------------
// JSON: canonical re-encoding (positive twins)
// ---------------------------------------------------------------------------

#[test]
fn json_vendor_media_type_is_canonicalized() {
    // §4.2 A row: `application/vnd.api+json; charset=UTF-8`.
    let raw = b" { \"b\" : 1e0 , \"a\" : \"\\u00e9\" } ";
    assert_eq!(
        json_body(Some("application/vnd.api+json; charset=UTF-8"), raw),
        accepted(b"{\"a\":\"\xc3\xa9\",\"b\":1.0}", Some(JSON))
    );
}

#[test]
fn json_sorts_keys_and_strips_whitespace() {
    assert_eq!(
        json_body(JSON_CT, b"{ \"z\" : 1 , \"a\" : [ 2 , 3 ] }"),
        accepted(b"{\"a\":[2,3],\"z\":1}", Some(JSON))
    );
}

#[test]
fn json_accepts_u64_max_exactly() {
    assert_eq!(
        json_body(JSON_CT, b"{\"id\":18446744073709551615}"),
        accepted(b"{\"id\":18446744073709551615}", Some(JSON))
    );
}

#[test]
fn json_accepts_i64_min_exactly() {
    assert_eq!(
        json_body(JSON_CT, b"{\"id\":-9223372036854775808}"),
        accepted(b"{\"id\":-9223372036854775808}", Some(JSON))
    );
}

#[test]
fn json_accepts_depth_64() {
    let document = nested_json(MAX_JSON_DEPTH);
    assert_eq!(
        json_body(JSON_CT, &document),
        accepted(&document, Some(JSON))
    );
}

#[test]
fn json_accepts_distinct_keys() {
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":1,\"b\":2}"),
        accepted(b"{\"a\":1,\"b\":2}", Some(JSON))
    );
}

#[test]
fn json_accepts_surrogate_pair() {
    assert_eq!(
        json_body(JSON_CT, b"\"\\ud83d\\ude00\""),
        accepted("\"\u{1f600}\"".as_bytes(), Some(JSON))
    );
}

#[test]
fn json_accepts_empty_object() {
    assert_eq!(json_body(JSON_CT, b"{}"), accepted(b"{}", Some(JSON)));
}

// ---------------------------------------------------------------------------
// JSON: parser-differential rejections (negative twins)
// ---------------------------------------------------------------------------

#[test]
fn json_rejects_u64_max_plus_one() {
    assert_eq!(
        json_body(JSON_CT, b"{\"id\":18446744073709551616}"),
        Err(IngressReject::JsonIntegerOutOfRange)
    );
}

#[test]
fn json_rejects_i64_min_minus_one() {
    assert_eq!(
        json_body(JSON_CT, b"{\"id\":-9223372036854775809}"),
        Err(IngressReject::JsonIntegerOutOfRange)
    );
}

#[test]
fn json_rejects_depth_65() {
    assert_eq!(
        json_body(JSON_CT, &nested_json(MAX_JSON_DEPTH + 1)),
        Err(IngressReject::JsonTooDeep)
    );
}

#[test]
fn json_rejects_duplicate_keys() {
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":1,\"a\":2}"),
        Err(IngressReject::JsonDuplicateKey)
    );
    // The duplicate is detected after escapes are decoded, so an escape cannot
    // smuggle a second key past the check.
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":1,\"\\u0061\":2}"),
        Err(IngressReject::JsonDuplicateKey)
    );
}

#[test]
fn json_rejects_bom() {
    assert_eq!(
        json_body(JSON_CT, b"\xef\xbb\xbf{\"a\":1}"),
        Err(IngressReject::JsonBom)
    );
}

#[test]
fn json_rejects_trailing_bytes() {
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":1} {}"),
        Err(IngressReject::JsonTrailingBytes)
    );
}

#[test]
fn json_rejects_lone_surrogate() {
    assert_eq!(
        json_body(JSON_CT, b"\"\\ud83d\""),
        Err(IngressReject::JsonLoneSurrogate)
    );
}

#[test]
fn json_rejects_invalid_utf8() {
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":\"\xff\"}"),
        Err(IngressReject::JsonNotUtf8)
    );
}

#[test]
fn json_rejects_empty_body() {
    assert_eq!(json_body(JSON_CT, b""), Err(IngressReject::EmptyJsonBody));
}

#[test]
fn json_rejects_non_finite_number() {
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":1e400}"),
        Err(IngressReject::JsonNumberOutOfRange)
    );
}

#[test]
fn json_rejects_content_type_mismatch() {
    assert_eq!(
        json_body(Some("multipart/form-data; boundary=x"), b"{\"a\":1}"),
        Err(IngressReject::ContentTypeMismatch {
            expected: JSON,
            observed: "multipart/form-data".to_string(),
        })
    );
    assert_eq!(
        json_body(Some("text/plain"), b"{\"a\":1}"),
        Err(IngressReject::ContentTypeMismatch {
            expected: JSON,
            observed: "text/plain".to_string(),
        })
    );
}

#[test]
fn json_rejects_non_utf8_charset() {
    assert_eq!(
        json_body(Some("application/json; charset=ibm037"), b"{\"a\":1}"),
        Err(IngressReject::CharsetNotUtf8 {
            observed: "ibm037".to_string(),
        })
    );
}

// ---------------------------------------------------------------------------
// JSON: canonical string escaping and strict-string rejections
// ---------------------------------------------------------------------------

#[test]
fn json_re_escapes_backslash_and_quote() {
    // §4.2 A: the decoded `\` and `"` must be written back as escapes, or the
    // re-encoded body is not valid JSON on the wire.
    assert_eq!(
        json_body(JSON_CT, br#"{"a":"x\\y\"z"}"#),
        accepted(br#"{"a":"x\\y\"z"}"#, Some(JSON))
    );
}

#[test]
fn json_re_emits_short_control_escapes_and_lowercase_unicode() {
    // The five short escapes survive a round trip unchanged...
    assert_eq!(
        json_body(JSON_CT, b"{\"s\":\"a\\nb\\rc\\td\\be\\ff\\u0001g\"}"),
        accepted(b"{\"s\":\"a\\nb\\rc\\td\\be\\ff\\u0001g\"}", Some(JSON))
    );
    // ...and a `\uXXXX` control character is re-emitted in the short form
    // (or four lowercase hex digits when no short form exists), never raw.
    assert_eq!(
        json_body(
            JSON_CT,
            b"{\"s\":\"\\u000A\\u000D\\u0009\\u0008\\u000C\\u0001\"}"
        ),
        accepted(b"{\"s\":\"\\n\\r\\t\\b\\f\\u0001\"}", Some(JSON))
    );
}

#[test]
fn json_rejects_raw_control_byte_in_a_string() {
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":\"\x01\"}"),
        Err(IngressReject::JsonSyntax)
    );
}

#[test]
fn json_rejects_unknown_escape() {
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":\"\\q\"}"),
        Err(IngressReject::JsonSyntax)
    );
    // A `\u` escape is exactly four hex digits.
    assert_eq!(
        json_body(JSON_CT, b"{\"a\":\"\\u12\"}"),
        Err(IngressReject::JsonSyntax)
    );
}

#[test]
fn json_rejects_strict_string_violation_twins() {
    // A lone *low* surrogate is rejected like a lone high one (the existing
    // test covers the high half), never panicking on the conversion.
    assert_eq!(
        json_body(JSON_CT, b"\"\\udc00\""),
        Err(IngressReject::JsonLoneSurrogate)
    );
    let mut high_then_text = Vec::from(b"\"\\ud800".as_slice());
    high_then_text.extend_from_slice(b"x\"");
    assert_eq!(
        json_body(JSON_CT, &high_then_text),
        Err(IngressReject::JsonLoneSurrogate)
    );
    // Invalid UTF-8 inside a string stays a typed rejection.
    assert_eq!(
        json_body(JSON_CT, b"\"\xc3\x28\""),
        Err(IngressReject::JsonNotUtf8)
    );
}

// ---------------------------------------------------------------------------
// Form: canonical re-encoding and validation
// ---------------------------------------------------------------------------

#[test]
fn form_reencodes_semicolon() {
    // §4.2 A row: `;` is literal in a form body and must come back as `%3B`.
    assert_eq!(
        form_body(
            Some("application/x-www-form-urlencoded; charset=UTF-8"),
            b"b=hello+world&a=%41%3B1&b=2"
        ),
        accepted(b"b=hello+world&a=A%3B1&b=2", Some(FORM))
    );
}

#[test]
fn form_treats_a_raw_semicolon_as_data() {
    // `ADR-0006` / §4.2 A: the form codec splits on `&` only. A raw `;` is
    // data, so `a=1;b=2` is one pair whose value percent-encodes `;` and `=`.
    assert_eq!(
        form_body(FORM_CT, b"a=1;b=2"),
        accepted(b"a=1%3Bb%3D2", Some(FORM))
    );
    // The `&` separator still produces two pairs.
    assert_eq!(
        form_body(FORM_CT, b"a=1&b=2"),
        accepted(b"a=1&b=2", Some(FORM))
    );
}

#[test]
fn form_normalizes_a_lowercase_hex_escape() {
    // `%3b` decodes to `;` and is re-encoded with uppercase hex digits.
    assert_eq!(form_body(FORM_CT, b"a=%3b"), accepted(b"a=%3B", Some(FORM)));
}

#[test]
fn form_accepts_empty_body() {
    assert_eq!(form_body(FORM_CT, b""), accepted(b"", Some(FORM)));
}

#[test]
fn form_rejects_bad_escape() {
    assert_eq!(
        form_body(FORM_CT, b"a=%ZZ"),
        Err(IngressReject::FormBadEscape)
    );
    assert_eq!(
        form_body(FORM_CT, b"a=%4"),
        Err(IngressReject::FormBadEscape)
    );
}

#[test]
fn form_rejects_nul() {
    assert_eq!(form_body(FORM_CT, b"a=1%00"), Err(IngressReject::FormNul));
    assert_eq!(form_body(FORM_CT, b"a=1\0"), Err(IngressReject::FormNul));
}

#[test]
fn form_rejects_non_utf8() {
    assert_eq!(
        form_body(FORM_CT, b"a=%FF"),
        Err(IngressReject::FormNotUtf8)
    );
}

#[test]
fn form_rejects_non_utf8_charset() {
    assert_eq!(
        form_body(
            Some("application/x-www-form-urlencoded; charset=ibm037"),
            b"a=1"
        ),
        Err(IngressReject::CharsetNotUtf8 {
            observed: "ibm037".to_string(),
        })
    );
}

#[test]
fn form_rejects_content_type_mismatch() {
    assert_eq!(
        form_body(JSON_CT, b"a=1"),
        Err(IngressReject::ContentTypeMismatch {
            expected: FORM,
            observed: JSON.to_string(),
        })
    );
}

// ---------------------------------------------------------------------------
// Head-level planning: `plan_body`
// ---------------------------------------------------------------------------

#[test]
fn raw_streams_with_zero_buffering() {
    // §4.2 A row: `Raw` is `StreamPassthrough` for any method, length or
    // encoding — the scheduler never buffers it (`R16`).
    assert_eq!(
        plan_body(
            "POST",
            raw_policy(),
            Some("application/octet-stream"),
            Some("gzip"),
            Some(100 * 1024 * 1024)
        ),
        Ok(BodyPlan::StreamPassthrough)
    );
    assert_eq!(
        plan_body("PUT", raw_policy(), None, None, None),
        Ok(BodyPlan::StreamPassthrough)
    );
}

#[test]
fn raw_transcode_is_identity() {
    // The scheduler must not call this for `Raw`; if it does, no validation
    // may silently rewrite the bytes.
    assert_eq!(
        transcode_buffered(BodyCodec::Raw, "POST", None, b"\x00\xffraw"),
        accepted(b"\x00\xffraw", None)
    );
    assert_eq!(
        transcode_buffered(BodyCodec::Raw, "GET", None, b"streamed"),
        accepted(b"streamed", None)
    );
}

#[test]
fn structured_body_with_content_encoding_is_rejected() {
    assert_eq!(
        plan_body("POST", json_policy(), JSON_CT, Some("gzip"), Some(10)),
        Err(IngressReject::ContentEncodingNotSupported {
            observed: "gzip".to_string(),
        })
    );
    assert_eq!(
        plan_body("POST", form_policy(), FORM_CT, Some("br"), None),
        Err(IngressReject::ContentEncodingNotSupported {
            observed: "br".to_string(),
        })
    );
}

#[test]
fn identity_encoding_is_accepted() {
    assert_eq!(
        plan_body("POST", json_policy(), JSON_CT, Some("identity"), Some(2)),
        Ok(BodyPlan::Buffer {
            codec: BodyCodec::Json
        })
    );
}

#[test]
fn undeclared_and_none_routes_allow_only_empty_bodies() {
    assert_eq!(
        plan_body("POST", None, None, None, Some(0)),
        Ok(BodyPlan::AllowEmptyOnly)
    );
    assert_eq!(
        plan_body("POST", None, None, None, None),
        Ok(BodyPlan::AllowEmptyOnly)
    );
    assert_eq!(
        plan_body("POST", none_policy(), JSON_CT, None, Some(0)),
        Ok(BodyPlan::AllowEmptyOnly)
    );
    assert_eq!(
        plan_body("POST", None, None, None, Some(1)),
        Err(IngressReject::BodyNotAllowed)
    );
    assert_eq!(
        plan_body("PATCH", none_policy(), None, None, Some(5)),
        Err(IngressReject::BodyNotAllowed)
    );
}

#[test]
fn bodyless_safe_methods_reject_bodies() {
    for method in ["GET", "HEAD", "OPTIONS"] {
        assert_eq!(
            plan_body(method, json_policy(), JSON_CT, None, Some(2)),
            Err(IngressReject::BodyNotAllowed)
        );
        assert_eq!(
            plan_body(method, json_policy(), JSON_CT, None, Some(0)),
            Ok(BodyPlan::AllowEmptyOnly)
        );
    }
}

#[test]
fn buffered_bodyless_methods_reject_any_bytes() {
    // §4.2 A row 1: the buffered API enforces the same `413` as `plan_body`,
    // independently of it, and a zero-byte body is still accepted.
    for method in ["GET", "HEAD", "OPTIONS"] {
        assert_eq!(
            transcode_buffered(BodyCodec::Json, method, JSON_CT, b"{\"a\":1}"),
            Err(IngressReject::BodyNotAllowed)
        );
        assert_eq!(
            transcode_buffered(BodyCodec::Json, method, JSON_CT, b""),
            accepted(b"", None)
        );
    }
}

#[test]
fn delete_with_a_body_follows_post_rules() {
    assert_eq!(
        plan_body("DELETE", json_policy(), JSON_CT, None, Some(7)),
        Ok(BodyPlan::Buffer {
            codec: BodyCodec::Json
        })
    );
    // A malformed `DELETE` body is a `400`, not a pass-through.
    assert_eq!(
        transcode_buffered(BodyCodec::Json, "DELETE", JSON_CT, b"{\"a\":}"),
        Err(IngressReject::JsonSyntax)
    );
    // A `DELETE` with no body is fine on any codec.
    assert_eq!(
        transcode_buffered(BodyCodec::Json, "DELETE", JSON_CT, b""),
        accepted(b"", None)
    );
    // A `DELETE` body on a `none` route is the `413`.
    assert_eq!(
        transcode_buffered(BodyCodec::None, "DELETE", None, b"x"),
        Err(IngressReject::BodyNotAllowed)
    );
}

#[test]
fn delete_without_a_body_needs_no_content_type() {
    assert_eq!(
        plan_body("DELETE", json_policy(), None, None, Some(0)),
        Ok(BodyPlan::AllowEmptyOnly)
    );
    assert_eq!(
        plan_body("DELETE", json_policy(), Some("text/plain"), None, Some(0)),
        Ok(BodyPlan::AllowEmptyOnly)
    );
    // A `DELETE` that declares bytes still needs the codec's type.
    assert_eq!(
        plan_body("DELETE", json_policy(), None, None, Some(2)),
        Err(IngressReject::ContentTypeMismatch {
            expected: JSON,
            observed: "none".to_string(),
        })
    );
}

#[test]
fn plan_body_checks_the_content_type_before_buffering() {
    assert_eq!(
        plan_body("POST", json_policy(), Some("text/plain"), None, Some(4)),
        Err(IngressReject::ContentTypeMismatch {
            expected: JSON,
            observed: "text/plain".to_string(),
        })
    );
    assert_eq!(
        plan_body("POST", form_policy(), JSON_CT, None, Some(4)),
        Err(IngressReject::ContentTypeMismatch {
            expected: FORM,
            observed: JSON.to_string(),
        })
    );
    assert_eq!(
        plan_body(
            "POST",
            form_policy(),
            Some("application/x-www-form-urlencoded; charset=ibm037"),
            None,
            Some(4)
        ),
        Err(IngressReject::CharsetNotUtf8 {
            observed: "ibm037".to_string(),
        })
    );
}

#[test]
fn status_maps_body_not_allowed_to_413() {
    assert_eq!(IngressReject::BodyNotAllowed.status(), 413);
    assert_eq!(IngressReject::JsonSyntax.status(), 400);
    assert_eq!(IngressReject::JsonTooDeep.status(), 400);
    assert_eq!(IngressReject::FormBadEscape.status(), 400);
    assert_eq!(
        IngressReject::ContentEncodingNotSupported {
            observed: "gzip".to_string()
        }
        .status(),
        400
    );
}

// ---------------------------------------------------------------------------
// Header scrub: `apply_transcoded_headers`
// ---------------------------------------------------------------------------

#[test]
fn transcoded_headers_keep_one_canonical_length() {
    let mut headers = vec![
        (b"Content-Length".to_vec(), b"3".to_vec()),
        (b"content-type".to_vec(), b"text/plain".to_vec()),
        (b"X-Keep".to_vec(), b"1".to_vec()),
        (b"Content-Length".to_vec(), b"9".to_vec()),
    ];
    apply_transcoded_headers(&mut headers, Some(JSON), 2);

    assert_eq!(header_values(&headers, "content-length"), vec![b"2"]);
    assert_eq!(
        header_values(&headers, "content-type"),
        vec![JSON.as_bytes()]
    );
    assert_eq!(header_values(&headers, "x-keep"), vec![b"1"]);
}

#[test]
fn transcoded_headers_strip_transfer_encoding() {
    let mut headers = vec![
        (b"Transfer-Encoding".to_vec(), b"chunked".to_vec()),
        (b"Content-Encoding".to_vec(), b"gzip".to_vec()),
        (b"Expect".to_vec(), b"100-continue".to_vec()),
        (b"content-type".to_vec(), b"application/json".to_vec()),
    ];
    apply_transcoded_headers(&mut headers, Some(FORM), 0);

    assert!(header_values(&headers, "transfer-encoding").is_empty());
    assert!(header_values(&headers, "content-encoding").is_empty());
    assert!(header_values(&headers, "expect").is_empty());
    assert_eq!(
        header_values(&headers, "content-type"),
        vec![FORM.as_bytes()]
    );
    assert_eq!(header_values(&headers, "content-length"), vec![b"0"]);
}

#[test]
fn transcoded_headers_without_a_content_type_emit_only_a_length() {
    let mut headers = vec![(b"Content-Type".to_vec(), b"application/json".to_vec())];
    apply_transcoded_headers(&mut headers, None, 0);
    assert_eq!(headers.len(), 1);
    assert_eq!(header_values(&headers, "content-type"), Vec::<&[u8]>::new());
    assert_eq!(header_values(&headers, "content-length"), vec![b"0"]);
}

// ---------------------------------------------------------------------------
// MIME parsing
// ---------------------------------------------------------------------------

#[test]
fn parse_mime_returns_essence_and_charset() {
    assert_eq!(
        parse_mime(Some("Application/JSON; Charset=\"UTF-8\"; boundary=x")),
        ("application/json".to_string(), Some("utf-8".to_string()))
    );
    assert_eq!(
        parse_mime(Some("application/vnd.api+json")),
        ("application/vnd.api+json".to_string(), None)
    );
    assert_eq!(parse_mime(None), ("none".to_string(), None));
    assert_eq!(parse_mime(Some("")), ("none".to_string(), None));
}

#[test]
fn parse_mime_caps_the_essence() {
    let long = "a".repeat(MAX_MIME_REPORT_BYTES * 4);
    let (essence, _) = parse_mime(Some(&long));
    assert_eq!(essence.len(), MAX_MIME_REPORT_BYTES);
}

// ---------------------------------------------------------------------------
// `blastit check`: sampling and suggestions
// ---------------------------------------------------------------------------

fn sample(mime: &str, truncated: bool, valid_json: bool, valid_form: bool) -> BodySample {
    BodySample {
        mime: mime.to_string(),
        truncated,
        valid_json,
        valid_form,
    }
}

#[test]
fn probe_sample_validates_against_the_declared_type() {
    let json = probe_sample(JSON_CT, b"{\"a\":1}", 7);
    assert_eq!(json, sample("application/json", false, true, false));

    let form = probe_sample(FORM_CT, b"a=1&b=2", 7);
    assert_eq!(
        form,
        sample("application/x-www-form-urlencoded", false, false, true)
    );

    // A JSON body sent as a form is not "valid JSON": declaring `json` would
    // reject the observed request, so the sample must not suggest it.
    let mismatched = probe_sample(FORM_CT, b"{\"a\":1}", 7);
    assert_eq!(
        mismatched,
        sample("application/x-www-form-urlencoded", false, false, true)
    );

    // Multipart is neither codec, and its essence is reported bare.
    let multipart = probe_sample(Some("multipart/form-data; boundary=x"), b"--x", 3);
    assert_eq!(
        multipart,
        sample("multipart/form-data", false, false, false)
    );
}

#[test]
fn probe_sample_accepts_exactly_the_cap() {
    // `truncated` is true only when `total_len > MAX_CHECK_SAMPLE_BYTES`, so a
    // body exactly at the cap is still probed and still suggests `json`.
    assert_eq!(
        probe_sample(JSON_CT, b"{\"a\":1}", MAX_CHECK_SAMPLE_BYTES as u64),
        sample("application/json", false, true, false)
    );
}

#[test]
fn probe_sample_truncates_past_the_cap() {
    let sample = probe_sample(JSON_CT, b"{\"a\":1}", MAX_CHECK_SAMPLE_BYTES as u64 + 1);
    assert!(sample.truncated);
    assert!(!sample.valid_json);
    assert!(!sample.valid_form);
}

#[test]
fn probe_sample_rejects_malformed_bodies() {
    let bad_json = probe_sample(JSON_CT, b"{\"a\":1,\"a\":2}", 13);
    assert!(!bad_json.valid_json);
    assert!(!bad_json.valid_form);

    let bad_form = probe_sample(FORM_CT, b"a=%ZZ", 5);
    assert!(!bad_form.valid_json);
    assert!(!bad_form.valid_form);

    let bad_charset = probe_sample(Some("application/json; charset=ibm037"), b"{\"a\":1}", 7);
    assert!(!bad_charset.valid_json);
}

#[test]
fn suggest_fix_maps_samples_to_codecs() {
    assert_eq!(
        suggest_fix(&[sample("application/json", false, true, false)]),
        (vec![BodyCodec::Json], false)
    );
    assert_eq!(
        suggest_fix(&[sample(
            "application/x-www-form-urlencoded",
            false,
            false,
            true
        )]),
        (vec![BodyCodec::Form], false)
    );
    assert_eq!(
        suggest_fix(&[sample("multipart/form-data", false, false, false)]),
        (vec![BodyCodec::Raw], true)
    );
    assert_eq!(
        suggest_fix(&[sample("application/json", true, false, false)]),
        (vec![BodyCodec::Raw], true)
    );
    assert_eq!(
        suggest_fix(&[
            sample("application/json", false, true, false),
            sample("application/x-www-form-urlencoded", false, false, true),
        ]),
        (vec![BodyCodec::Json, BodyCodec::Form], false)
    );
    assert_eq!(suggest_fix(&[]), (Vec::new(), false));
}
