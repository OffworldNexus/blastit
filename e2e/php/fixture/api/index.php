<?php
// The second `[route.php]` route's entry (`path = ['/api/.*']`), which has no
// `[route.static]` block. `/__inspect` answers the same twelve facts there, so
// the traversal row can assert what this worker pinned *and* that `..` reached
// the script as data.

declare(strict_types=1);

require __DIR__ . '/../src/inspect.php';

if (str_ends_with($_SERVER['PATH_INFO'] ?? '/', '/__inspect')) {
    blastit_emit_json(blastit_inspect());
    exit(0);
}

blastit_emit_json([
    'entry' => 'api',
    'path' => $_SERVER['PATH_INFO'] ?? '/',
    // `..` reached this worker as data, so the row can see it.
    'path_info' => $_SERVER['PATH_INFO'] ?? null,
    'method' => $_SERVER['REQUEST_METHOD'] ?? 'GET',
    'script_filename' => $_SERVER['SCRIPT_FILENAME'] ?? null,
]);
