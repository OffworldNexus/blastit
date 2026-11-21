<?php
// The `SCRIPT_FILENAME` override's target (row 1): it lives *outside* the
// static root and prints its canary only if something executes it. A response
// carrying `EVIL-RAN` means a request named the script that ran.

declare(strict_types=1);

echo "EVIL-RAN\n";
