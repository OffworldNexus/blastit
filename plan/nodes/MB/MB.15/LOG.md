# Log MB.15

## 2026-11-14T13:58 [conductor]

created (task) blastit dev must reap stage 1 on SIGTERM (no orphan, no port left open)

## 2026-11-14T13:59 [lead]

Premise check: could not reproduce against a freshly built target/debug/blastit. SIGTERM to the dev init pid drains in ~1s (shutdown drain -> process.exited signal=15 -> stopped), port refuses, no --dev-root descendant left. The MB.14 'orphan' was an artifact: blastit dev execve's in place into --stage=1, so pgrep -f 'blastit dev --port N' never matches after boot, and the probe's $! was a wrapper. Holding this node until MB.14 lands; then reproduce formally before any fix.

## 2026-11-14T14:00 [lead]

edited fields
