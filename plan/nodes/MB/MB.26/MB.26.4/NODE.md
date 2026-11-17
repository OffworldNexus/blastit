---
id: MB.26.4
title: "Remove the uid-0 silent skip from critical_blastfile_fmt's write-failure case"
kind: task
status: proposed
owner: implementer
priority: 5
depends_on: []
paths: [crates/blastit/tests/critical_blastfile_fmt.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T06:14"
updated: "2026-11-17T06:14"
---

Moved from `MB.21.5.10` (`MB.21`) to `MB.26.4` at the 0.2.0 release gate; scope, `paths`, owner and acceptance are unchanged. Ids quoted inside the text below keep their pre-move numbering — use this node's id in commands.

# Goal

Remove the silent skip in `crates/blastit/tests/critical_blastfile_fmt.rs:201`: under uid 0 the
write-failure case prints `skipped: this process writes into a read-only directory (uid 0)` and
returns, so on a root host it proves nothing — a `pytest.mark.skip` in Rust clothing, contrary to the
`testing` skill's "never skip a critical test".

# Context

- The case proves that `blastit fmt` refuses to write a read-only file (exit 2) and leaves it
  untouched; a mode-`0444` file is writable by root, so the premise does not hold there.
- `MB.21.5.3`'s audit (§4) and the node brief both call this a weak oracle rather than a bug.
- The fix must assert something on **both** hosts, not skip on one.

# Acceptance

```console
$ cargo test -p blastit --test critical_blastfile_fmt
$ make test-critical
```

1. Replace the early `return` with a branch that asserts the host-appropriate outcome: when the write
   target is genuinely unwritable (a `0555` directory owned by another uid, or a file made unwritable
   by a mount/attribute), `fmt --check`/`fmt` refuses and the bytes are unchanged; when running as uid 0
   (any write succeeds), assert instead that the command's *refusal path* is exercised through a
   deterministic injection — e.g. a target path whose parent does not exist, or a directory in place of
   the file — so the same "no partial write, no temp file" property is still proven.
2. No `#[ignore]`, no `eprintln!("skipped…") return`, no conditional that leaves zero assertions on
   either branch.
3. `tools/critical_guard.py --base <sha> --node <this id>` green (the file is protected; this edit
   strengthens, and `paths` names it).

# Notes

- Optional post-0.2.0 if the injection is awkward on this host: the unprivileged runner (`make check`
  in CI) does prove the property today. Say in `RESULT.md` which branch you chose and why.
