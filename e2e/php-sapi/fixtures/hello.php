<?php

// The probe's first fixture: a head and a body, nothing else. `header()` only
// works because the shim registers a real `send_headers`, which is the whole
// reason `php_embed_init()` cannot be used (it sets `SG(headers_sent) = 1`).
header('X-Probe: 1');
header('Content-Type: text/plain');

// Deliberate, bounded CPU: `CLOCK_PROCESS_CPUTIME_ID` advances in scheduler
// ticks (~4 ms at HZ=250), so a request that costs less than one tick honestly
// measures 0 ns. Hashing ~4 MB of userland CPU crosses a tick, which makes the
// probe's `cpu_ns > 0` an assertion about the shim's clock, not about HZ.
for ($i = 0; $i < 40; $i++) {
    hash('sha256', str_repeat('x', 100000));
}

echo 'hello from php';
