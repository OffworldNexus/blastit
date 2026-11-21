<?php
// The shop behind `public/index.php`: a public catalogue read, a parameterised
// order insert, and the two deliberately ungranted statements the end-to-end
// driver expects the database proxy to refuse (a cross-table column read and a
// `DELETE`). Nothing here reads a password: the URL blastit renders carries no
// credential, and the driver asserts the value never reaches a response.

declare(strict_types=1);

/**
 * The DSN of the URL blastit handed this view.
 *
 * blastit renders the socket directory as the libpq `host` query parameter
 * (`postgresql://app@/shop?host=/proc/self/cwd&port=5432`), exactly as the
 * Django and SQLAlchemy clients read it. A raw `DB_*` fallback is kept because
 * that is the other spelling the manifest documents.
 */
function blastit_database_dsn(): string
{
    $url = getenv('DATABASE_URL');
    if (is_string($url) && $url !== '') {
        // PHP's `parse_url` rejects the rendered shape `postgresql://blastit@/shop`
        // (an empty authority host before the path), so read the four pieces
        // this driver needs directly.
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
        $user = '';
        if (($match['userinfo'] ?? '') !== '') {
            $user = explode(':', $match['userinfo'], 2)[0];
        }
        $host = $query['host'] ?? ($match['authority'] ?? '');
        $port = $query['port'] ?? '5432';
        $database = ltrim($match['path'] ?? '', '/');

        return sprintf(
            'pgsql:host=%s;port=%s;dbname=%s;user=%s',
            $host,
            $port,
            $database,
            $user,
        );
    }

    $host = getenv('DB_HOST');
    if (is_string($host) && $host !== '') {
        return sprintf(
            'pgsql:host=%s;port=%s;dbname=%s;user=%s',
            $host,
            getenv('DB_PORT') ?: '5432',
            getenv('DB_DATABASE') ?: '',
            getenv('DB_USERNAME') ?: '',
        );
    }

    throw new RuntimeException('no DATABASE_URL or DB_HOST in this view');
}

function blastit_database(): PDO
{
    return new PDO(
        blastit_database_dsn(),
        null,
        null,
        [PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION, PDO::ATTR_EMULATE_PREPARES => false],
    );
}

/** The request body, whatever codec `body form` left in it. */
function blastit_form(): array
{
    $form = $_POST;
    if ($form === []) {
        parse_str((string) file_get_contents('php://input'), $form);
    }

    return $form;
}

/** One refusal: the SQLSTATE the proxy answered with, never a credential. */
function blastit_denied(PDOException $error): array
{
    return [
        'status' => 403,
        'document' => [
            'denied' => true,
            'sqlstate' => (string) $error->getCode(),
            'error' => preg_replace('/[^\x20-\x7e]/', ' ', $error->getMessage()),
        ],
    ];
}

function blastit_shop_route(string $path, string $method): ?array
{
    if ($method === 'GET' && $path === '/products') {
        $statement = blastit_database()->query(
            'SELECT id, name, price FROM products ORDER BY id',
        );

        return [
            'status' => 200,
            'document' => ['products' => $statement->fetchAll(PDO::FETCH_ASSOC)],
        ];
    }

    if ($method === 'GET' && preg_match('#^/products/(\d+)$#', $path, $match) === 1) {
        $statement = blastit_database()->prepare(
            'SELECT id, name, price FROM products WHERE id = ?',
        );
        $statement->execute([(int) $match[1]]);

        return [
            'status' => 200,
            'document' => ['product' => $statement->fetch(PDO::FETCH_ASSOC) ?: null],
        ];
    }

    if ($method === 'GET' && $path === '/env') {
        $url = getenv('DATABASE_URL');

        return [
            'status' => 200,
            'document' => [
                'database_url' => is_string($url) ? $url : null,
                'url_has_password' => is_string($url)
                    && preg_match('#^[a-z+]+://[^/@:]+:[^/@]*@#', $url) === 1,
                'db_password_in_env' => getenv('DB_PASSWORD') !== false,
                'pid' => getmypid(),
            ],
        ];
    }

    if ($method === 'POST' && $path === '/orders') {
        $form = blastit_form();
        $customer = (int) ($form['customer_id'] ?? 0);
        $total = (string) ($form['total'] ?? '0');
        if ($customer <= 0 || !is_numeric($total)) {
            return ['status' => 400, 'document' => ['error' => 'customer_id and total are required']];
        }

        $pdo = blastit_database();
        $insert = $pdo->prepare(
            'INSERT INTO orders (customer_id, total) VALUES (?, ?)',
        );
        $insert->execute([$customer, $total]);
        $read = $pdo->prepare(
            'SELECT customer_id, total FROM orders WHERE customer_id = ?',
        );
        $read->execute([$customer]);

        return [
            'status' => 201,
            'document' => [
                'inserted' => true,
                'orders' => array_map(
                    fn (array $row): array => [
                        'customer_id' => (int) $row['customer_id'],
                        'total' => (string) $row['total'],
                    ],
                    $read->fetchAll(PDO::FETCH_ASSOC),
                ),
            ],
        ];
    }

    if ($method === 'POST' && $path === '/orders/purge') {
        try {
            blastit_database()->exec('DELETE FROM orders');
        } catch (PDOException $error) {
            return blastit_denied($error);
        }

        return ['status' => 200, 'document' => ['purged' => true]];
    }

    if ($method === 'GET' && $path === '/customers/peek') {
        try {
            $rows = blastit_database()
                ->query('SELECT email, password_hash FROM customers ORDER BY id')
                ->fetchAll(PDO::FETCH_ASSOC);
        } catch (PDOException $error) {
            return blastit_denied($error);
        }

        return ['status' => 200, 'document' => ['customers' => $rows]];
    }

    if ($method === 'GET' && $path === '/customers/insert') {
        try {
            $insert = blastit_database()->prepare(
                'INSERT INTO customers (email, password_hash) VALUES (?, ?)',
            );
            $insert->execute(['mallory@example.test', 'not-a-real-hash']);
        } catch (PDOException $error) {
            return blastit_denied($error);
        }

        return ['status' => 201, 'document' => ['inserted' => true]];
    }

    if ($method === 'GET' && $path === '/orders/note-insert') {
        try {
            $insert = blastit_database()->prepare(
                'INSERT INTO orders (customer_id, total, note) VALUES (?, ?, ?)',
            );
            $insert->execute([1, '1.00', 'note']);
        } catch (PDOException $error) {
            return blastit_denied($error);
        }

        return ['status' => 201, 'document' => ['inserted' => true]];
    }

    if ($method === 'POST' && $path === '/orders/note') {
        $form = blastit_form();
        try {
            $insert = blastit_database()->prepare(
                'INSERT INTO orders (customer_id, total, note) VALUES (?, ?, ?)',
            );
            $insert->execute([
                (int) ($form['customer_id'] ?? 1),
                (string) ($form['total'] ?? '1.00'),
                'note',
            ]);
        } catch (PDOException $error) {
            return blastit_denied($error);
        }

        return ['status' => 201, 'document' => ['inserted' => true]];
    }

    if ($method === 'GET' && $path === '/crash/exit') {
        echo "bye\n";
        exit(0);
    }

    if ($method === 'GET' && $path === '/crash/fatal') {
        // A real fatal: an uncaught Error ends this request, not the route.
        throw new Error('the fixture asks for a fatal error');
    }

    return null;
}
