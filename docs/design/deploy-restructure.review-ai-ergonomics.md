# Review — deployments restructure, lens `ai-ergonomics`

> **Provenance.** This review was produced by a `critic` subagent with the `ai-ergonomics`
> lens, which hit its step limit after finishing the investigation and before writing this
> file. `doc-writer` transcribed its report verbatim below; the findings, grades, line
> numbers and verdict are the critic's, not the writer's. Line numbers refer to the pages as
> the critic read them (index 91 / DigitalOcean 206 / Compose 305 lines), i.e. before the
> fixes.

Reviewed at `release/0.2.0` (worktree `.worktrees/release-0.2.0`):

| Page | Lines |
|---|---|
| `docs/deploy/index.md` | 91 |
| `docs/deploy/digitalocean-paas.md` | 206 |
| `docs/deploy/compose-traefik.md` | 305 |
| `docs/deploy/anything-else.md` (reference) | 275 |
| `docs/deploy/fair-queuing.md` (reference) | 54 |

Cross-checked: `docs/reference/{settings,manifest,log-events}.md`, `docs/ci.md`,
`crates/blastit/src/config.rs` (`PRESETS`), `crates/blastit/src/stage0.rs`,
`crates/blastit-events/src/json.rs`, `e2e/rootless/compose.yml`, `e2e/docs_deploy.py`,
`python/tests/runnable_docs/{harness.py,inventory.toml}`.

## BLOCKER

### A1. `index.md:4` claims a WASM ingress gate the workspace does not have

No `wasmtime`/`wasmer` dependency exists anywhere in the workspace; ingress is native Rust
(`crates/blastit-ingress`, `serde_json`/form codecs). `docs/VISION.md` puts WASM ingress in
Milestone C (`0.3.x`). A page that says "validates input in a WASM gate" describes code that
does not exist. → "validates the body in safe Rust (JSON and form codecs today)".

### A2. No success oracle on either preset page

The restructured `index.md` deleted the pooler verification (`SHOW TimeZone` must print
`UTC`, direct port, as `app`), so the silent-failure trap in `anything-else.md:111-113` is
now unverified. `e2e/docs_deploy.py` proves it: **8/42 checks FAIL** (7 on `index.md`, 1 on
`digitalocean-paas.md`) and `e2e/run.py:111` runs it first, so `make e2e` is red.
→ restore a short "did it work" block on `index.md#pooler`, or retarget the harness.

## MAJOR

### A3. `$ blastit --preset <name>` is shown with no output, and is not what the deployment does

`digitalocean-paas.md:6-8`, `compose-traefik.md:6-8`. `--preset` starts production stage 0
and exits `1` without capabilities (`stage0.rs:234-259`; `harness.py:1579-1612` asserts
exactly exit 1 + `blastit dev` + `cap_`). Neither page's deploy file passes the flag — they
set `BLASTIT_PRESET` — so the snippet reads as a dangling instruction. → keep it, state what
it does, and say the deploy file sets `BLASTIT_PRESET`.

### A4. Placeholder soup on the DigitalOcean page

One page mixes `<id>`, `<password>`, `<host>`, `REPLACE_WITH_*`, `$GIT_SHA` and a bare
`yourregistry` (`digitalocean-paas.md:91,105-106,162,176,181-184`). An agent cannot tell
"substitute from where" from "literal". → one dialect plus one table or sentence mapping
token → source.

### A5. `digitalocean-paas.md:192-196` — "publish no other route to it" is Compose's instruction

App Platform has no port to publish; the line has no actionable content. → delete it, keep
the trust statement ("DO's edge is the only ingress, which is why `*` is safe").

### A6. Compose step order asks the reader to paste files that do not exist yet

`compose.yaml` (step 3) references `/usr/local/bin/python3.13-init` (explained in step 5) and
`./db/grants.sql` (defined in step 4). → move "The image" earlier and "Grants" before the
stack.

### A7. `.env` loading is contradictory

`compose-traefik.md:111-112` ("load the file into your shell") versus `:283` (`--env-file`)
versus `:288` ("none reaches your shell history"). Sourcing a file puts its secrets in every
command's environment, so the last claim is false; and `docker build …:$TAG` needs `$TAG`
regardless. → one mechanism.

### A8. Neither preset page sends the reader through `blastit check`

`ci.md:36-39,138-167` is the completeness gate; no deploy page mentions it. → one line plus
a link, on both.

### A9. `index.md:53-55` — "a view never opens a TCP connection" is false

`anything-else.md:131` and `frameworks/index.md:110-111`: `egress = true` opens any address
and port. → qualify the sentence.

### A10. `index.md:10-12` is self-contradictory

"your framework is not a process of its own: a view process per route namespace is forked
from it" — the sentence that makes a reader scroll to `ARCHITECTURE.md`. → one clause.

### A11. `digitalocean-paas.md:104` + `:110-111` — the `app` user is created twice

`doctl databases user create … app` followed by "Users & Databases → Add user" reads as two
steps; doing both errors out.

## MINOR

- **a12** `compose-traefik.md:285` boot line is a trimmed sample, not labelled trimmed (unlike
  `anything-else.md:72-73`), and has no "if the line is absent" branch.
- **a13** `index.md:24` "Sixteen workers … **fill** a 1 GB cluster's ~22 backends" — 16 ≠ 22.
- **a14** `compose-traefik.md:63` lists `pg_isready` under "health" as if it were the app's.
- **a15** `digitalocean-paas.md:192-199` and `compose-traefik.md:290-297` are the same three
  items, copied.
- **a16** `blastit --preset` space-form versus `--preset=` — noted, no action.
- **a17** "never reaches your shell history" is untrue once step 2 sources the file.

## Verified clean

`make docs-check` passes (no broken links/anchors); the preset tables match
`config.rs:84-105` exactly, including `assumes: Root`/`Capabilities`;
`/_blastit/health` is answered by the scheduler (`config.rs:1005`); `503` +
`Retry-After` (`scheduler.rs:3542`); `Upgrade` → `501` (`head.rs:309`); `140MiB` parses
(`config.rs:2647`); the `${db.<pool>.DATABASE_URL}` three-part form is real
(`docs/research/connection-pooling.md:19`); `ALTER ROLE … SET timezone` is in the Compose
`grants.sql`; `python -m blastit` is real (`python/blastit/__main__.py`).

## Verdict

**"Can a reader explain a blastit deployment to a colleague after 60 seconds on this page,
and does any paragraph make them scroll back?"** Yes to the first: one container, `blastit`
as PID 1 and the front door, per-route uid views, `dbproxy` holding the role, pooler versus
direct port, TLS at the edge. The paragraphs that force a scroll-back are `index.md:10-12`
and the three-table repeat on each preset page ("What the preset sets" → "What we thought
the config would be" → "The steps"), where the same values must be reconciled three times.

An agent can deploy from the index plus one preset page: the cross-contamination is gone for
the *values* (no DigitalOcean tokens on the Compose page, no PgBouncer on the DigitalOcean
page). The gap most likely to make it guess is the **preset command with no output**, then
the mixed placeholder dialects on the DigitalOcean page.
