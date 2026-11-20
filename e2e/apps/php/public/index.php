<?php
// The fixture's front controller: a Symfony/Laravel-shaped `public/index.php`.
//
// It is the only file `entrypoint php` names, so every request either reaches
// this file or is refused before it. `/__inspect` answers exactly the twelve
// `$_SERVER` facts the php2shell matrix turns on — no more, so a row cannot
// pass on a key the fixture invented.
//
// The shop routes below are the application this file also serves: a public
// catalogue read, a parameterised order insert, and two routes that try SQL no
// view was granted (a cross-table column read and a `DELETE`), so the driver
// can prove the proxy refuses them per view.

declare(strict_types=1);

require __DIR__ . '/../src/inspect.php';
require __DIR__ . '/../src/shop.php';

$path = $_SERVER['PATH_INFO'] ?? '/';
$method = $_SERVER['REQUEST_METHOD'] ?? 'GET';

if (str_ends_with($path, '/__inspect')) {
    blastit_emit_json(blastit_inspect());
    exit(0);
}

$shop = blastit_shop_route($path, $method);
if ($shop !== null) {
    blastit_emit_json($shop['document'], $shop['status']);
    exit(0);
}

if (preg_match('#^/users/(\d+)$#', $path, $match) === 1) {
    blastit_emit_json(['entry' => 'front-controller', 'user' => (int) $match[1]]);
    exit(0);
}

blastit_emit_json([
    'entry' => 'front-controller',
    'path' => $path,
    'method' => $method,
    'script_filename' => $_SERVER['SCRIPT_FILENAME'] ?? null,
]);
