<?php
// The production fixture: one small front controller whose routes let the
// production suite observe what a deployment promises — the view's own OS uid,
// the client address and scheme blastit derived, the environment the worker was
// handed, the sandbox's refusal of a shell and of an undeclared outbound
// connection, and per-view SQL grants.
//
// It answers only what the driver asks for, so a row cannot pass on a fact this
// file invented: `remote_addr`/`https` are the CGI variables blastit sets,
// `uid` is the kernel's answer, and the SQL is the same catalogue/orders read
// the application suite uses (`e2e/apps/postgres/schema.sql`).

declare(strict_types=1);

/** One JSON answer, then the end of the request. */
function prod_json(array $document, int $status = 200): never
{
    http_response_code($status);
    header('Content-Type: application/json');
    echo json_encode($document, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), "\n";
    exit(0);
}

/**
 * The DSN blastit handed this view.
 *
 * A URL store renders `postgresql://blastit@/apps?host=/run/blastit/views/<uid>`
 * (no password, the socket directory in the libpq `host` parameter); a split
 * store renders `DB_HOST`/`DB_PORT`/`DB_DATABASE`/`DB_USERNAME` instead. PHP's
 * `parse_url` rejects the empty-authority shape, so the four pieces are read
 * with the pattern the application fixture also uses.
 */
function prod_pdo(): PDO
{
    $options = [PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION, PDO::ATTR_EMULATE_PREPARES => false];
    $url = getenv('DATABASE_URL');
    if (is_string($url) && $url !== '') {
        $pattern = '#^(?<scheme>[a-z][a-z0-9+.-]*)://'
            . '(?:(?<userinfo>[^@/]*)@)?'
            . '(?<authority>[^/?]*)'
            . '(?<path>/[^?]*)?'
            . '(?:\?(?<query>.*))?$#i';
        if (preg_match($pattern, $url, $match) !== 1) {
            throw new RuntimeException('DATABASE_URL is not a URL this driver can read');
        }
        $query = [];
        if (($match['query'] ?? '') !== '') {
            parse_str($match['query'], $query);
        }
        $user = ($match['userinfo'] ?? '') !== ''
            ? explode(':', $match['userinfo'], 2)[0]
            : '';

        return new PDO(
            sprintf(
                'pgsql:host=%s;port=%s;dbname=%s;user=%s',
                $query['host'] ?? ($match['authority'] ?? ''),
                $query['port'] ?? '5432',
                ltrim($match['path'] ?? '', '/'),
                $user,
            ),
            null,
            null,
            $options,
        );
    }

    $host = getenv('DB_HOST');
    if (is_string($host) && $host !== '') {
        return new PDO(
            sprintf(
                'pgsql:host=%s;port=%s;dbname=%s;user=%s',
                $host,
                getenv('DB_PORT') ?: '5432',
                getenv('DB_DATABASE') ?: '',
                getenv('DB_USERNAME') ?: '',
            ),
            null,
            null,
            $options,
        );
    }

    throw new RuntimeException('no DATABASE_URL or DB_HOST in this view');
}

/** The uid the kernel gave this view process (`null` when posix is absent). */
function prod_uid(): ?int
{
    return function_exists('posix_geteuid') ? posix_geteuid() : null;
}

/** One denied statement: SQLSTATE and the proxy's sentence, never a credential. */
function prod_denied(PDOException $error): never
{
    prod_json(
        [
            'denied' => true,
            'sqlstate' => (string) $error->getCode(),
            'error' => preg_replace('/[^\x20-\x7e]/', ' ', $error->getMessage()),
        ],
        403,
    );
}

$path = $_SERVER['PATH_INFO'] ?? '/';
$method = $_SERVER['REQUEST_METHOD'] ?? 'GET';

if ($method === 'GET' && $path === '/whoami') {
    $headers = [];
    foreach (['HTTP_X_FORWARDED_FOR', 'HTTP_X_FORWARDED_PROTO', 'HTTP_DO_CONNECTING_IP'] as $name) {
        if (isset($_SERVER[$name])) {
            $headers[$name] = $_SERVER[$name];
        }
    }
    prod_json([
        'pid' => getmypid(),
        'uid' => prod_uid(),
        'remote_addr' => $_SERVER['REMOTE_ADDR'] ?? null,
        'https' => $_SERVER['HTTPS'] ?? 'off',
        'headers' => $headers,
    ]);
}

if ($method === 'GET' && $path === '/fast') {
    prod_json(['fast' => true, 'pid' => getmypid(), 'uid' => prod_uid()]);
}

if ($method === 'GET' && $path === '/slow') {
    $milliseconds = (int) ($_GET['ms'] ?? 500);
    $milliseconds = max(1, min(5000, $milliseconds));
    usleep($milliseconds * 1000);
    prod_json(['slept_ms' => $milliseconds, 'pid' => getmypid()]);
}

if ($method === 'GET' && $path === '/env') {
    $url = getenv('DATABASE_URL');
    $split = [];
    foreach (['DB_HOST', 'DB_PORT', 'DB_USERNAME', 'DB_DATABASE', 'DB_PASSWORD'] as $name) {
        $split[$name] = getenv($name) !== false;
    }
    prod_json([
        'pid' => getmypid(),
        'uid' => prod_uid(),
        'database_url' => is_string($url) ? $url : null,
        'url_has_password' => is_string($url)
            && preg_match('#^[a-z+]+://[^/@:]+:[^/@]*@#', $url) === 1,
        'db_password_in_env' => getenv('DB_PASSWORD') !== false,
        'split' => $split,
    ]);
}

if ($method === 'GET' && $path === '/shell') {
    // Every way PHP starts a process is unavailable in the embedded worker; when
    // one is compiled in anyway, the filter refuses the fork, so the positive
    // signal is "nothing ran" either way.
    $functions = [];
    foreach (['exec', 'shell_exec', 'system', 'passthru', 'proc_open', 'popen', 'pcntl_exec', 'pcntl_fork'] as $name) {
        if (function_exists($name)) {
            $functions[] = $name;
        }
    }
    $started = false;
    $output = '';
    if (in_array('proc_open', $functions, true)) {
        $pipes = [];
        $process = @proc_open('/usr/bin/id', [1 => ['pipe', 'w']], $pipes);
        $started = is_resource($process);
        if ($started) {
            $output = (string) stream_get_contents($pipes[1]);
        }
    }
    prod_json([
        'functions' => $functions,
        'started' => $started,
        'output' => $output,
        'blocked' => $functions === []
            || (!$started && !str_contains($output, 'uid=')),
    ]);
}

if ($method === 'GET' && $path === '/egress') {
    $canary = getenv('CANARY');
    [$host, $port] = is_string($canary) && str_contains($canary, ':')
        ? explode(':', $canary, 2)
        : ['127.0.0.1', '1'];
    // Two spellings of the same attempt: the stream layer (what an application
    // usually writes) and `socket_create`, which is the one whose errno the
    // refusal sentence can be read from. `connected` is captured before any
    // close, and a connection that *did* open sends the same request the
    // driver's control sends, so a fully open egress cannot pass on an empty
    // body.
    $stream_errno = 0;
    $stream_errstr = '';
    $stream = @fsockopen($host, (int) $port, $stream_errno, $stream_errstr, 2);
    $stream_connected = is_resource($stream);
    $stream_read = '';
    if ($stream_connected) {
        stream_set_timeout($stream, 2);
        fwrite($stream, "GET / HTTP/1.0\r\nHost: canary\r\n\r\n");
        $stream_read = (string) stream_get_contents($stream);
        fclose($stream);
    }

    $socket_connected = false;
    $socket_errno = 0;
    $socket_errstr = 'the sockets extension is not available';
    if (function_exists('socket_create') && defined('AF_INET') && defined('SOL_TCP')) {
        $socket = @socket_create(AF_INET, SOCK_STREAM, SOL_TCP);
        if ($socket === false) {
            $socket_errno = socket_last_error();
            $socket_errstr = socket_strerror($socket_errno);
        } else {
            $socket_connected = @socket_connect($socket, $host, (int) $port);
            if (!$socket_connected) {
                $socket_errno = socket_last_error($socket);
                $socket_errstr = socket_strerror($socket_errno);
            }
            socket_close($socket);
        }
    }

    prod_json([
        'stream_connected' => $stream_connected,
        'stream_errno' => $stream_errno,
        'stream_errstr' => $stream_errstr,
        'stream_read' => $stream_read,
        'socket_connected' => $socket_connected,
        'socket_errno' => $socket_errno,
        'socket_errstr' => $socket_errstr,
        'connected' => $stream_connected || $socket_connected,
    ]);
}

if ($method === 'GET' && preg_match('#^/(catalog|warehouse)/(products|orders)(?:/(\d+))?$#', $path, $match) === 1) {
    $table = $match[2];
    $id = $match[3] ?? null;
    $columns = $table === 'products'
        ? 'id, name, price'
        : 'customer_id, total';
    $order = $table === 'products' ? 'id' : 'customer_id';
    $sql = $id === null
        ? sprintf('SELECT %s FROM %s ORDER BY %s', $columns, $table, $order)
        : sprintf('SELECT %s FROM %s WHERE id = ?', $columns, $table);

    try {
        $pdo = prod_pdo();
        $statement = $pdo->prepare($sql);
        $statement->execute($id === null ? [] : [(int) $id]);
        prod_json(['table' => $table, 'rows' => $statement->fetchAll(PDO::FETCH_ASSOC)]);
    } catch (PDOException $error) {
        prod_denied($error);
    }
}

prod_json(['error' => 'no route', 'path' => $path, 'method' => $method], 404);
