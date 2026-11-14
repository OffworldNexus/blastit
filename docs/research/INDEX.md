# Research Index

Cited research notes produced by `researcher` agents (see skill `research`). Excluded from the public Zensical documentation site.

<!-- Append entries as: - [YYYY-MM-DD-slug](YYYY-MM-DD-slug.md): one-line summary -->
- [2026-11-13-connection-pooling](connection-pooling.md): DO managed pooler (`:25061`, transaction, pool-name-as-dbname) and Compose PgBouncer 1.26 (`max_prepared_statements`, auth, healthcheck, `options` passthrough) for the deploy presets.
- [2026-11-14-php-fastcgi-wire](php-fastcgi-wire.md): FastCGI 1.0 records, name-value encoding, requestId/no-multiplexing, size limits and the measured `STDOUT`/`END_REQUEST` quirks of php-fpm 8.4.27, plus the canonical non-root `php-fpm -F -y` pool.
- [2026-11-14-php2shell](php2shell.md): the php2shell attack class field by field (20 techniques: `SCRIPT_FILENAME`, `PATH_INFO`, `PHP_VALUE`/`PHP_ADMIN_VALUE`, `auto_prepend_file`, httpoxy, `DOCUMENT_ROOT`, CVE-2012-1823, CVE-2024-4577) with measured binary defences and how Symfony/Laravel/Slim consume the params.
