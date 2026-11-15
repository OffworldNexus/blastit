<?php
// The fixture's front controller: a Symfony/Laravel-shaped `public/index.php`.
//
// It is the only file `[route.php] entry` names, so every probe request either
// reaches this file or is refused before it. `/__inspect` answers exactly the
// twelve `$_SERVER` facts the matrix turns on — no more, so a row cannot pass
// on a key the fixture invented.

declare(strict_types=1);

require __DIR__ . '/../src/inspect.php';

$path = $_SERVER['PATH_INFO'] ?? '/';

if (str_ends_with($path, '/__inspect')) {
    blastit_emit_json(blastit_inspect());
    exit(0);
}

if (preg_match('#^/users/(\d+)$#', $path, $match) === 1) {
    blastit_emit_json(['entry' => 'front-controller', 'user' => (int) $match[1]]);
    exit(0);
}

blastit_emit_json([
    'entry' => 'front-controller',
    'path' => $path,
    'method' => $_SERVER['REQUEST_METHOD'] ?? 'GET',
    'script_filename' => $_SERVER['SCRIPT_FILENAME'] ?? null,
]);
