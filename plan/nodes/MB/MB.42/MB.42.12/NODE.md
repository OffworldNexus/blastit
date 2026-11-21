---
id: MB.42.12
title: "blastit-lsp argv: --version/--help answer silence and exit 1 (now reachable on PATH)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.3.3]
paths: ["crates/blastit-lsp/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T03:39"
updated: "2026-11-20T04:57"
---

# Goal
`blastit-lsp` is now on `PATH` for the first time (`MB.42.3.3` restored the console script), so its argv behaviour is newly reachable — and it answers silence:

```
$ blastit-lsp --help     -> exit 1, stdout 0 bytes, stderr 0 bytes
$ blastit-lsp --version  -> exit 1, stdout 0 bytes, stderr 0 bytes
$ blastit lsp --help     -> usage: blastit lsp … ; -h, --help ; exit 0
```

`crates/blastit-lsp/src/lib.rs:212-219` ignores argv entirely (`blastit_lsp::run()` → stdin EOF → "client disappeared" → `1`). An editor extension or a package manager that probes `--version` reads exit-1-silence as "broken binary", and a user who types the binary name gets a hang-like silence.

# Exact Deliverables
1. At the top of `run()` (before touching stdin): if argv has any argument, then
   - `--version` / `-V` -> print `blastit-lsp <CARGO_PKG_VERSION>` to **stdout** and return `0`;
   - anything else -> print one sentence to **stderr** — this is a language server, it takes no arguments and speaks LSP on stdin/stdout, point your editor at it — and return `2`.
2. Keep the no-argument behaviour byte-identical (a real LSP session on stdin/stdout); do not print anything on startup in that path (an LSP client's stream must stay clean).
3. `--help` must not exit 1 or print nothing: either the same `2`-with-sentence path (acceptable: it explains itself) or a usage block on stderr. State the choice in `RESULT.md`.
4. Tests: `blastit-lsp --version` prints exactly one line and exits 0; `blastit-lsp --help` exits non-zero **with** a sentence on stderr; no-argument run still exits cleanly at EOF with empty stdout.

# Notes
- `docs/reference/cli.md` documents `blastit lsp`; the binary's own argv is a packaging surface, so this is in scope for `0.2.0` even though it was previously unreachable.
- Do not add a `--help` that lists LSP options — there are none.
- Never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- `cargo test -p blastit-lsp --locked`
- the three command tails above (before/after)
- `cargo clippy -p blastit-lsp --all-targets --locked -- -D warnings`
- `make zed-check` if it exists, and `make fast-check`
