---
id: MB.3.17
title: "Remove the external FastCGI path: delete blastit-fastcgi, drop [route.fastcgi], the ALLOW flag and every doc that sanctions it"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.12]
paths:
  - "crates/blastit-fastcgi/**"
  - "crates/blastit-manifest/**"
  - "crates/blastit-scheduler/**"
  - "crates/blastit/**"
  - "crates/blastit-view/**"
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - "scripts/**"
  - ".github/workflows/**"
  - docs/reference/settings.md
  - docs/knowledge/LESSONS.md
  - python/tests/runnable_docs/inventory.toml
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T15:14"
updated: "2026-11-14T16:20"
---

# Goal

Remove the external FastCGI path entirely: delete `crates/blastit-fastcgi` and its
protected suites, drop `[route.fastcgi]` as a key, and delete every flag, doc row and
design sentence that sanctions it. Owner decision, 2026-11-14: **"No fallback please"**.

# Context

- ADR-0010 makes embedded PHP the only PHP story. A second, weaker transport kept alive "for
  hosts that cannot install `libphp-embed`" buys a second threat model (the CGI/FastCGI
  param-injection class the param wall exists for), a second transport to keep green, and a
  security switch (`BLASTIT_ALLOW_EXTERNAL_FASTCGI`) whose whole purpose is to weaken the
  product. It is deleted, not documented.
- `[route.fastcgi]` therefore stops existing as a key. A manifest that writes it is an
  **unknown key**, refused with the key named — not a silently-ignored block and not a
  deprecation warning.
- The protected suites under `crates/blastit-fastcgi/tests/critical_*.rs` are guarded by
  `tools/critical_guard.py`: deleting them is allowed **only** because this node's `paths`
  cover them and the guard is invoked with this node as authorizer. Run it explicitly and
  paste the verdict; never `rm` silently.
- **What stays untouched:** `crates/blastit-php-sapi`, `PhpEmbedRuntime`, `view.chan`
  framing, the `Upstream`/`WorkerRuntime` contract and `MB.9.3`. This removes one backend
  implementation, not the abstraction.
- History is not rewritten: the epic-gate audit and the plan nodes keep their records and
  gain a **superseded** note that names this decision and this node.

# Acceptance

```console
$ ! cargo metadata --no-deps --format-version 1 | grep -q blastit-fastcgi && echo "crate gone from the workspace"
$ ! grep -rn 'FastCgi\|fastcgi\|FASTCGI' crates/ --include=*.rs | grep -v '^crates/blastit-php-sapi' && echo "no backend or flag left"
$ grep -rn 'route.fastcgi' docs/ | wc -l        # zero: no doc teaches or sanctions it
$ cargo test --workspace --locked
$ make check
$ uv run --no-project tools/critical_guard.py --base <HEAD> --node MB.3.17
```
- `crates/blastit-fastcgi/**` deleted; `Cargo.toml` members and `Cargo.lock` updated;
  `cargo tree` has no trace of it.
- `Executor` has no FastCgi arm; no `AnswerKind`/dispatch plumbing remains that exists only
  for it; `BLASTIT_ALLOW_EXTERNAL_FASTCGI` is gone from code, `docs/reference/settings.md`
  and the page.
- A manifest with `[route.fastcgi]` is refused as an unknown key naming the key (asserted,
  with the accepted twin: the same manifest with `[route.php]` parses).
- `docs/frameworks/php.md` has no fallback note; `docs/design/**` and `docs/decisions/**`
  gain a superseded/amendment line where they sanctioned the fallback instead of being
  silently edited.
- `docs/knowledge/LESSONS.md`: **one sentence** — an audited, green, zero-dependency crate
  can still be the wrong code; deleting it when the architecture says so is cheaper than
  carrying a second threat model (cite this node).
- `docs/testing/AUDIT-MB.3.4-gate.md` gets a superseded note, not a rewrite.

# Notes

- Depends on `MB.3.12` settling: both edit `Cargo.toml`/`Cargo.lock`, and that leaf is the
  one that must not be racing a member removal.
- Do not touch the `Upstream`/`WorkerRuntime` contract, `MB.9.3`, or any `[route.static]`
  behaviour. Static, WSGI and embedded PHP all keep working; only the fallback dies.

## Split: code now, docs after MB.12's lenses (owner, 2026-11-14)

`docs/frameworks/**`, `docs/design/**` and `docs/decisions/**` are `MB.12`'s paths while its
four lenses run, and a second writer at an architect-owned document already cost a 65 KB
file. So:

- **This execution:** the code half only — crate removed from the workspace, `Executor`
  arm and dispatch plumbing gone, `BLASTIT_ALLOW_EXTERNAL_FASTCGI` gone from code, the
  protected suites deleted **with `critical_guard.py --node MB.3.17` passing as
  authorizer**, the `docs/reference/settings.md` row removed (that file is ours) and the
  `LESSONS` one-liner appended.
- **Held follow-up (after the lenses):** `php.md`'s fallback note gone, `[route.fastcgi]`
  documented as an unknown key (if mentioned at all), the design/ADR supersession lines,
  and the `AUDIT-MB.3.4-gate.md` superseded note. Record it as a follow-up task, not a TODO
  in a commit message.

Acceptance for the code half:
1. `[route.fastcgi]` is an **unknown key** whose diagnostic names it, with the accepted twin
   (`[route.php]` parses) asserted in the same test.
2. `cargo build --locked`, `make check` exit 0, and the Python-free build still refuses
   WSGI **and** FastCGI at exit 1 with its sentence.
3. No orphan references: `grep -rn 'fastcgi\|FastCgi\|ALLOW_EXTERNAL' crates/ scripts/ Makefile .github/` empty (paste it).
4. `MB.3.11`'s `no_python_build` and `MB.3.12`'s probe still pass — the removal must not
   quietly break the capability gate.
5. RESULT states what was deleted and why, that the protected deletion was **authorized**
   (with the guard verdict), and that the crate was green, audited and still the wrong code
   for the architecture.
