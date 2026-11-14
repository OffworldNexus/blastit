---
id: MB.11
title: Rewrite the DigitalOcean roles/pooler section for clarity (backport to 0.2.0)
kind: task
status: ready
owner: "doc-writer"
priority: 1
depends_on: []
paths: ["docs/deploy/digitalocean-paas.md", python/tests/runnable_docs/inventory.toml]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T12:56"
updated: "2026-11-14T12:56"
---

# Goal

Rewrite the **"Cluster, roles and the pooler"** section of `docs/deploy/digitalocean-paas.md` so a developer understands the situation and the platform's answer, instead of being handed a wall of inline commands.

# Context

Owner review of the 0.2.0 PR, verbatim:

> "I'm reading 'Cluster, roles and the pooler' section of the documentation for the 0.2.0 PR and I must say it's not super clear. Big blobs of paragraphs, lots of code inline, visually looks like shit and there is no structure in what is said. You're supposed to help the user, not hammer them with obscure command lines and god knows what. Make them understand the situation, how it's address in DO, etc..."

The current section (lines ~51-97) is four dense paragraphs with `doctl` calls, an `awk` one-liner and a 15-line heredoc inline, then a claim about `dead_grant` and `prepare_threshold=None` buried mid-paragraph. It also mixes two different stories — *who connects as what* and *why there is a pooler* — instead of teaching them in order.

# Acceptance

1. **Structure first.** The section becomes short, ordered subsections with scannable headings, e.g.:
   - **What you end up with** — a 3-4 sentence explanation and one table: role → connects to → port → URL → used by. Make the two-URL rule (app → pool, migrator → cluster) the first thing the reader sees.
   - **1. Create the cluster** — one short command block, console alternative in prose (one line).
   - **2. Create the app role** — one short command.
   - **3. Owner, migrator, grants** — the SQL in a single fenced file-style block (not a heredoc chained to `export`s), with a one-line comment per group; *why* the roles exist in two sentences before it.
   - **4. Create the pool** — console path + `doctl`, and a short **"Why a pooler"** / **"What transaction mode does not do"** pair (prepared statements, advisory locks, `LISTEN/NOTIFY`) with the session-mode escape hatch.
   - **5. Wire the two URLs** — where each secret goes, pointing at the spec below instead of repeating it.
2. **No inline command soup.** Commands live in fenced blocks with their own short line; prose carries the *why*. Any `export`/`awk`/flag chain gets its own block or is dropped in favour of the console path.
3. **Facts stay true and are stated once.** Keep: role default `UTC` makes Django send no `TimeZone`, so the `connection` group's `set_config` grant is a `dead_grant` on this platform (CI still needs it); transaction mode cannot change after creation; pool is 25061 and cluster 25060; the app prepares nothing / `prepare_threshold=None` claim — verify it against the code before restating it, and drop or correct it if it is wrong.
4. **Lean.** The whole page stays <= 150 lines (it is exactly 150 now, so the rewrite must remove as much as it adds). Short sentences, active voice, no kernel trivia, links instead of explanations where a doc page already exists.
5. **Gates.** Every fenced block is registered in `python/tests/runnable_docs/inventory.toml` at its **new index** (the file indexes fences by position — renumbering after restructuring is mandatory, and a wrong index silently checks the wrong block); `make docs-unit show no unreferenced/missing fence`, `make docs-check` (zensical `--strict`) and `make docs-test` green. `docs/knowledge/LESSONS.md` untouched unless you learn something new.
6. **Critics.** `dx` and `ai-ergonomics` goldfish critics run **foreground** and pass on the revised page. Their brief: "could a developer who has never used DigitalOcean follow this and end up with the right two URLs, and does any sentence make them re-read it twice?"
