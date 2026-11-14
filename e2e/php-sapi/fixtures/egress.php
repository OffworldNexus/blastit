<?php

// The egress fixture. Two layers decide whether this route can reach the
// network, and this file reports evidence for both without deciding which one
// stopped it:
//
//   1. `allow_url_fopen` gates PHP's URL wrappers (`http://`, `ftp://`). It is
//      the INI half, pinned by `IniPolicy`, and it is **defence in depth, never
//      the wall**.
//   2. The worker's seccomp `View { egress: false }` filter refuses
//      `socket(AF_INET)`/`socket(AF_INET6)` outright. That is the wall, and it
//      is what stops `curl`, `sockets` and PDO, none of which consult
//      `allow_url_fopen`.
//
// The probe classifies the text/errno; the driver asserts which class each run
// must produce. Nothing here writes to the network when it succeeds: port 9 is
// closed in the image, and a successful connect is reported as `connected` so
// the probe fails loudly instead of silently passing.
header('Content-Type: application/json');

error_clear_last();
$report = [
    'allow_url_fopen' => ini_get('allow_url_fopen'),
    'disable_functions' => ini_get('disable_functions'),
    // The three directives attempt 1 rendered but never applied: read back the
    // *effective* values, not the string we asked for.
    'open_basedir' => ini_get('open_basedir'),
    'ffi_enable' => ini_get('ffi.enable'),
    'phar_require_hash' => ini_get('phar.require_hash'),
    'attempts' => [],
];

// The URL wrapper: the one path `allow_url_fopen=0` really does close.
$handle = @fopen('http://127.0.0.1:9/', 'r');
$error = error_get_last();
$report['attempts']['fopen_http'] = [
    'connected' => $handle !== false,
    'error' => $handle === false && $error !== null ? $error['message'] : '',
];
if ($handle !== false) {
    fclose($handle);
}

// A stream socket: opens its own `socket(AF_INET)`. `errno` 1 is EPERM (the
// filter at `socket()`), 111 is ECONNREFUSED (nothing listening, no filter).
// A failure at `socket()` does not fill `$errno` — PHP only sets it when
// `connect` fails — so the suppressed warning is reported too.
error_clear_last();
$errno = 0;
$errstr = '';
$stream = @stream_socket_client('tcp://127.0.0.1:9', $errno, $errstr, 1.0);
$stream_error = error_get_last();
$report['attempts']['stream_socket'] = [
    'connected' => $stream !== false,
    'errno' => $errno,
    'error' => $stream === false
        ? ($errstr !== '' ? $errstr : (string) ($stream_error['message'] ?? ''))
        : '',
];
if ($stream !== false) {
    fclose($stream);
}

// curl opens its own socket and never looks at `allow_url_fopen`.
// `CURLINFO_OS_ERRNO` is the errno curl saw, which is the one value that
// separates "the kernel said EPERM" from "nothing was listening".
$curl = curl_init('http://127.0.0.1:9/');
curl_setopt_array($curl, [
    CURLOPT_RETURNTRANSFER => true,
    CURLOPT_TIMEOUT => 1,
    CURLOPT_CONNECTTIMEOUT => 1,
]);
$body = curl_exec($curl);
$report['attempts']['curl'] = [
    'connected' => $body !== false,
    'errno' => curl_errno($curl),
    'os_errno' => curl_getinfo($curl, CURLINFO_OS_ERRNO),
    'error' => $body === false ? curl_error($curl) : '',
];
curl_close($curl);

echo json_encode($report);
