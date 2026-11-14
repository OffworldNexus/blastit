<?php

// Reports what actually reached PHP's superglobals. The probe force-injects
// PHP_VALUE / PHP_ADMIN_VALUE / REDIRECT_STATUS / HTTP_PROXY into the request's
// server-variable array *past* the Rust types, so every "absent" below is the
// C shim refusing them, not the caller forgetting to send them.
header('Content-Type: application/json');

$report = [
    'php_value' => isset($_SERVER['PHP_VALUE']) ? 'present' : 'absent',
    'php_admin_value' => isset($_SERVER['PHP_ADMIN_VALUE']) ? 'present' : 'absent',
    'redirect_status' => isset($_SERVER['REDIRECT_STATUS']) ? 'present' : 'absent',
    'http_proxy' => isset($_SERVER['HTTP_PROXY']) ? 'present' : 'absent',
    'script_filename' => isset($_SERVER['SCRIPT_FILENAME']) ? $_SERVER['SCRIPT_FILENAME'] : '',
    'request_method' => isset($_SERVER['REQUEST_METHOD']) ? $_SERVER['REQUEST_METHOD'] : '',
    'query' => isset($_SERVER['QUERY_STRING']) ? $_SERVER['QUERY_STRING'] : '',
    'cookie' => isset($_COOKIE['probe']) ? $_COOKIE['probe'] : 'absent',
];

echo json_encode($report);
