<?php
// The twelve `$_SERVER` facts the probe matrix turns on, as the interpreter
// itself sees them (ADR-0010 §2: `SCRIPT_FILENAME` and `DOCUMENT_ROOT` are
// pinned in C, `HTTP_PROXY` is dropped, `PHP_VALUE` has no channel at all).
//
// `server_keys` is `array_keys($_SERVER)` — names only, never a value — so a
// row can assert that `REDIRECT_STATUS` or `HTTP_PROXY` is *absent* without any
// response ever carrying what the interpreter does hold.

declare(strict_types=1);

function blastit_inspect(): array
{
    $length = $_SERVER['CONTENT_LENGTH'] ?? null;
    $proxy = getenv('HTTP_PROXY');

    return [
        'script_filename' => $_SERVER['SCRIPT_FILENAME'] ?? null,
        'script_name' => $_SERVER['SCRIPT_NAME'] ?? null,
        'path_info' => $_SERVER['PATH_INFO'] ?? null,
        'document_root' => $_SERVER['DOCUMENT_ROOT'] ?? null,
        'request_uri' => $_SERVER['REQUEST_URI'] ?? null,
        'query_string' => $_SERVER['QUERY_STRING'] ?? null,
        'request_method' => $_SERVER['REQUEST_METHOD'] ?? null,
        'content_length' => $length === null ? null : (int) $length,
        'allow_url_include' => ini_get('allow_url_include'),
        'http_proxy' => $proxy === false ? false : $proxy,
        'user_ini' => ini_get('user_ini.filename'),
        'server_keys' => array_keys($_SERVER),
    ];
}

function blastit_emit_json(array $document, int $status = 200): void
{
    http_response_code($status);
    header('Content-Type: application/json; charset=utf-8');
    header('Cache-Control: no-store');
    echo json_encode($document, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), "\n";
}
