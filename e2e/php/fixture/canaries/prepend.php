<?php
// The `auto_prepend_file` canary (rows 9 and 10). It is installed at a path the
// probe names in a `PHP_VALUE`, and prints only if something prepends it.

declare(strict_types=1);

echo "PREPEND-RAN\n";
