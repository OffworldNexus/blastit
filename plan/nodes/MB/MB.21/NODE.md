---
id: MB.21
title: "Replace the v1 manifest with the purpose-built Blastfile DSL (docs first, then parser, fmt and cutover)"
kind: epic
status: ready
priority: 1
depends_on: []
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T01:49"
---

# Goal

Replace the v1 manifest **completely** with `Blastfile`, a purpose-built DSL that is Blastit's whole
routing table and permission model. `./Blastfile` is discovered automatically, `BLASTIT_MANIFEST`
still names another path, and there is no third answer: a missing file says `missing ./Blastfile`, and
a body that is not a Blastfile is an ordinary `Blastfile:<line>:<col>` syntax error.

The v1 format is **buried**, under the same total-erasure law the retired PHP transport got in
`MB.3.17`/`MB.3.30`: no detector, no sniffer, no suffix test, no migration sentence, no migration
page, no key map, no apology, no comparison. The retired name survives only in records (`LOG.md`,
`RESULT.md`, finished briefs, ADRs, the changelog), and a `make check` guard keeps it out of every
page, crate, script and live brief a reader or a runtime can reach.

Blastfile removes the friction a developer pays for at the keyboard: no `version` header, no `=`
anywhere, no four-level `[route.db.tables]` nesting, the protocol is the keyword
(`postgres $DATABASE_URL`), raw `'...'` strings for SQL and regex, and a dedicated `$IDENT` token so
an environment-variable name can never be pasted as a literal by accident.

This epic is **DX-First, in the mandatory order**: the lean user documentation (a basic guide plus a
complete language reference) is written and critiqued by fresh-context `dx` + `ai-ergonomics`
goldfish critics **before** any design; the technical design and ADR pass `security`,
`implementability`, `testability` and `consistency` critics **before** one line of lexer.

## Settled language spec (owner, 2026-11-16 — do not re-litigate)

```text
1. Universal block grammar: `keyword [args...] [{ directives... }]`, newline or `;` statement
   terminators, `#` comments, comma-separated items, **zero `=` anywhere**, **no `version` header**.
2. Tokens: `"..."` escaped strings, `'...'` raw strings (zero backslash escaping for SQL/regex),
   `$IDENT` dedicated env-var token (`[A-Z_][A-Z0-9_]*` — passing a string literal where `$VAR` is
   required is a syntax error at `line:col`).
3. Zero cloud/infra specifics in `Blastfile`: **no `pooler` / `digitalocean` / vendor knobs in
   `Blastfile`** (relocate any pooler transaction-mode knob that existed in the v1 manifest to a host
   deployment env var, never in `Blastfile`). Outgoing HTTP templates (`http ...`) and
   `mysql`/`redis`/`mongodb` are Milestone D — do NOT document or parse placeholder blocks for
   unbuilt proxies in `0.2.0`; only `postgres` is the store keyword in `0.2.0`, and `egress any` is
   the explicit raw-network escape hatch.
4. Four sections:
   - `app "<name>"`
   - `entrypoint python "<module:attr>"` (optional `{ preload "<mod>", ... }`) OR
     `entrypoint php "<relative/path.php>"` (declared ONCE at top level, replacing per-route
     `[route.php] entry`; omitted for static-only apps).
   - Store connections (protocol IS the keyword): `postgres $DATABASE_URL`, named second DB
     `postgres <name> $ENV_VAR`, or block form `postgres [name] { host $DB_HOST; port $DB_PORT;
     user $DB_USERNAME; password $DB_PASSWORD; database $DB_DATABASE; ... }` (check existing
     `blastit-manifest` proxy input/output templates so all real `0.2.0` Postgres capabilities map
     cleanly).
   - Optional reusable grants: `trait <name> { postgres [name] { ... } }`, attached explicitly inside
     a view via `use <name>[, ...]` (`unrestricted` and `egress any` forbidden inside `trait`).
   - Views:
     * Single-signature header: `view <name> [METHOD[, ...]] <pattern[, ...]> { ... }`
       (`HEAD` automatically implied by `GET`; omitted method = any method).
     * Multi-endpoint resource form: `view <name> { GET "/cart"; POST "/cart/items" { body json };
       DELETE "/cart/items/{id:int}"; ... }` so body rules attach per method in one view sandbox.
     * Route matching: quoted string `"/products/{id:int}"` = literal path pattern (`.` is literal;
       `{name}` = `[^/]+`, `{name:int}` = `[0-9]+`, `{name:uuid}` = UUID, trailing `*` = prefix tail)
       OR `regex '<pattern>'` (full anchored regex).
     * View directives: `body <mode>` (`json|form|none|raw[ unvalidated]`), `use <trait>`,
       `egress any` (warns at boot), `prefork`, `postgres [name] unrestricted`, and
       `postgres [name] { transactions; functions fn1, fn2;
       select|insert|update table[(col1, col2)], ...; delete table, ...;
       query 'SELECT ... WHERE x = $?'; }`.
   - Static mounts:
     `static "<prefix>" { root "<dir>"; immutable ["<glob>"]; fallback "<file>"; index "<file>" }`
     (NOTE: `python/tests/test_docs_assets.py` bans sentences mentioning both `PHP` and `fallback` —
     a denial such as "never pair `PHP` and `fallback` in one sentence" passes, an offer does not;
     read its docstring before writing prose.)
   - Request precedence (one rule): (1) `static` prefix match where a regular non-`.php`/`.phar` file
     exists on disk → (2) first matching `view` → (3) `static` `fallback` → (4) `404`.
   - Tooling: `blastit fmt [--check] [PATH]` canonical formatter + lossless comment-preserving CST
     surgical editor for `--learn`.
```

5. **`body raw` / `body raw unvalidated`** (settled, recorded in `ADR-0012`): `body raw` lowers to
   `(BodyCodec::Raw, allow_raw = false)` — `blastit dev` serves it with a warning, `blastit check`
   reports `raw_ingress`, and a production boot refuses the file. `body raw unvalidated` lowers to
   `(Raw, true)`: production serves the same bytes with the boot warning. `unvalidated` after any
   other codec is a syntax error, and `--learn` never writes either form.
6. **No `workers` keyword** (settled, recorded in `ADR-0012` §12.3): the per-view PHP concurrency cap
   is removed — the scheduler owns parallelism, `PhpPolicy.workers` stays `Option<u32>` and is always
   `None`, and writing `workers <n>` is a `Blastfile:<line>:<col>` refusal that names the removal and
   the ADR.

`MB.21.2` records units 5 and 6 in `ADR-0012`; a design that re-opens either is out of scope.

## Internal provenance: v1 spelling → `Blastfile` (never published)

| v1 spelling | Blastfile | Landed by |
|---|---|---|
| `version = 1` | gone — no header | MB.21.2 |
| `app = "mod:attr"` | `entrypoint python "mod:attr"` | MB.21.2 |
| `preload = ["mod"]` | `entrypoint python "mod:attr" { preload "mod" }` | MB.21.2 |
| `[proxy.<name>]` `protocol`/`input`/`output` | `postgres $DATABASE_URL` (in-place outputs) or `postgres <name> { host $DB_HOST; … }` | MB.21.2 |
| `[group.<name>]` | `trait <name> { … }`, attached by `use <name>` inside a view | MB.21.2 |
| top-level `groups` (attached to every route) | gone — explicit `use` per view only; an invisible global attach has no fail-closed story | MB.21.2 |
| `[[route]]` `name`/`path`/`methods` | `view <name> [METHOD] "<pattern>"` or the multi-endpoint form | MB.21.2 |
| `[route.ingress]` `body` + `allow_raw` | `body json\|form\|none\|raw[ unvalidated]` (settled unit 5) | MB.21.2 |
| the static block `root`/`index`/`immutable`/`spa` | `static "<prefix>" { root; index; immutable; fallback }` | MB.21.2 |
| `[route.php] entry` | `entrypoint php "<relative/path.php>"` | MB.21.2 |
| `[route.php] workers` (optional cap, 1..64) | **removed** — no keyword; the scheduler owns parallelism (settled unit 6, `ADR-0012` §12.3) | MB.21.2 |
| `prefork = true` | `prefork` | MB.21.2 |
| `egress = true` | `egress any` (warns at boot) | MB.21.2 |
| `[route.<proxy>]` `queries`/`functions`/`transaction_control`/`tables`/`unrestricted` | `postgres [name] { transactions; functions …; select\|insert\|update …; delete …; query '…' }` and `postgres [name] unrestricted` | MB.21.2 |

This table is an implementation checklist for `MB.21.2`–`MB.21.4`. It is **not** a published key map:
no page, README, nav entry or reference table carries it, or any sentence that offers one.

Out of scope by owner decision: `pooler`, any vendor knob, and every Milestone C/D proxy keyword
(`http`, `mysql`, `redis`, `mongodb`) — the reference documents only what `0.2.0` runs.

## Children

| Node | Owner | What |
|---|---|---|
| `MB.21.1` | doc-writer | *(done)* Basic guide + complete language reference, registered in the site and the fence inventory, through `dx` + `ai-ergonomics` critics |
| `MB.21.1.1` | doc-writer | Purge the last traces of the retired format from those two pages: no "coming from" line, no key map, no nav row |
| `MB.21.2` | designer | Technical design + `ADR-0012` + `security`/`implementability`/`testability`/`consistency` critics |
| `MB.21.3` | epic | Toolchain: bounded lexer, LL(1) parser, path compiler, `fmt` + lossless CST, surgical `--learn` editor, `docs_fences` validator |
| `MB.21.4` | epic | Cutover: discovery and missing-file sentence, runtime parity, `check`/`--learn`/`--json`, Python package, Makefile, `e2e/` |
| `MB.21.5` | epic | Docs sweep, live fences + permanent retired-name guard, `test-curator` audit, visual check |

# Context

- Owner mandate (2026-11-16): *"Replace the v1 manifest completely with the purpose-built `Blastfile`
  DSL. Start strictly DX-First by writing the language documentation — a basic guide plus a complete
  language reference — critiqued by `dx` and `ai-ergonomics` goldfish critics, BEFORE technical design
  and BEFORE lexer/parser/formatter/runtime implementation."*
- Owner directive (2026-11-16): the v1 format is buried like the retired PHP transport. Delete the
  migration prose, the key map, the detector and the parser; a foreign body is a syntax error; a
  permanent `make check` guard refuses the name. `MB.21.1.1`, `MB.21.4.1`, `MB.21.4.4`, `MB.21.4.5`,
  `MB.21.5.1` and `MB.21.5.2` carry that directive.
- Read first: `docs/VISION.md` (DX law), `docs/knowledge/LESSONS.md`, `crates/blastit-manifest/src/`
  (`model.rs`, `parse.rs`, `validate.rs`, `routing.rs`, `edit.rs`), `docs/design/blastfile-dsl.md` and
  `docs/decisions/ADR-0012-blastfile-dsl.md` (written by `MB.21.2`), `plan/nodes/MB/MB.20/NODE.md`.
- Measured surface the design must preserve: `Manifest` carries `app`, `preload`, global `groups`,
  `proxies` (protocol + raw `input`/`output` maps), `groups`, `routes` (name, path regexes, methods,
  groups, `egress`, `prefork`, grants) and per-route `ingress` (`BodyCodec` `Json|Form|None|Raw` +
  `allow_raw`), `static` (`root`/`index`/`spa`/`immutable`) and `php` (`entry`/`workers`).
  `Protocol::ALL` is `[Postgresql]` only; `PgGrant` is queries + `transaction_control` +
  `unrestricted` + `TableGrants` (`select`/`insert`/`update` columns, `delete`).
- Today's discovery: `BLASTIT_MANIFEST` else `./Blastfile`, no upward search (`ADR-0002`), and
  `crates/blastit/src/cli.rs` currently refuses `--manifest` with a sentence naming the
  working-directory file. `Config::from_env`'s messages, `check.rs`'s `read_manifest`, `stage0.rs`'s
  missing-file sentence and `config.rs`'s default move with it; find every site with
  `grep -rnE 'blastit[.]toml' crates/`.
- `crates/blastit-manifest/tests/docs_fences.rs` counts complete manifest fences per documented file
  and lives in the crate the parser will grow in; every page the release rewrites must keep it green.
- Coordinate with `MB.20` (frameworks regroup, held until v0.2.0 is merged): `MB.21.5.1` rewrites the
  manifest fences and the reference links inside `docs/frameworks/**`, and `MB.20` restructures those
  pages afterwards. Its `depends_on` is extended with `MB.21.5.1` so no file is rewritten twice.

# Acceptance

```console
$ uv run --no-project tools/plan.py tree --under MB.21
$ uv run --no-project tools/plan.py validate
$ make check
$ make e2e
$ uvx --offline zensical build --strict
$ ls docs/blastfile.md docs/reference/blastfile.md docs/design/blastfile-dsl.md docs/decisions/ADR-0012-blastfile-dsl.md
$ test ! -e docs/reference/manifest.md
$ find e2e python/tests -name 'Blastfile' | wc -l
$ find e2e python/tests -name '*.toml' ! -name 'Cargo.toml' ! -name 'inventory.toml' | wc -l
$ grep -rnE 'blastit[.]toml' README.md docs ARCHITECTURE.md crates python scripts e2e Makefile \
    --exclude-dir=__pycache__ --exclude='*.so' --exclude-dir=target \
    | grep -vE 'docs/changelog\.md|docs/(decisions|design|research|knowledge|testing|archive)/'
$ for f in $(grep -rlE 'blastit[.]toml' plan/nodes --include=NODE.md); do \
    printf '%s %s\n' "$(grep -m1 -oE 'status: [a-z]+' "$f" | cut -d' ' -f2)" "$f"; done \
    | grep -vE '^(done|superseded) '
```

1. Every child of `MB.21` is `done` with a `RESULT.md` carrying command + output evidence — `MB.21.1`,
   `MB.21.1.1`, `MB.21.2` and every child of `MB.21.{3,4,5}`; `plan.py tree --under MB.21` shows no
   `ready`/`active` node and no `blocked`/`failed` one.
2. The repository runs on `Blastfile`: the fixtures under `e2e/` and `python/tests/` are `Blastfile`
   (the two counts above are `≥ 15` and `0`), the Makefile's `docs-serve` stage writes one, and every
   Docker/rootless scenario loads one.
3. `make check` exit 0 (including `docs-unit`, `docs-check`, `docs-test` with the new pages executed
   live, `test-critical`, `rs-no-python`, `plan-validate`) and `make e2e` exit 0.
4. **No migration path exists.** A body that is not a Blastfile — a v1 body included — exits non-zero
   at boot, `dev`, `check` and `fmt` with a single `Blastfile:<line>:<col>` syntax error and no second
   sentence; a missing file says `missing ./Blastfile` (or names `BLASTIT_MANIFEST`) and no other file
   name. `test_the_real_tree_carries_no_retired_toml_manifest` in `python/tests/test_docs_assets.py`
   (run by `docs-unit`, and so by `make check`) is green, and it fails on a crafted page, a live
   brief, or a crate/script/fixture that names the retired file.
5. The retired name appears nowhere outside records: the last two greps above print nothing, and both
   pages are public, lean and true — `docs/blastfile.md` ≤ 150 lines, the reference complete enough to
   write any `0.2.0` manifest, zero Milestone C/D or vendor claims, zero mention of another format;
   `dx` and `ai-ergonomics` critics accepted on the docs, `security`/`implementability`/`testability`/
   `consistency` accepted on the design.
6. `test-curator` audit of `blastit-manifest` (lexer, parser, CST, editor) with a `cargo-mutants`
   ratio ≥ 75 % on the parsing/policy modules, and `visual-checker` proof that the two new pages
   render (nav, tables, code blocks) in both colour schemes.

# Notes

- **Burial law (owner, 2026-11-16).** The v1 manifest is erased, not migrated: no detector, no
  sniffer, no migration sentence, no migration page, no key map. Precedent `MB.3.30` (retired PHP
  transport vocabulary): purge the live text, then land a mechanical guard with negative self-tests so
  the name cannot return. For this epic that guard is
  `test_the_real_tree_carries_no_retired_toml_manifest` in `python/tests/test_docs_assets.py`
  (`MB.21.5.2`), and it is in `make check` by construction because `docs-unit` already runs that
  module. Records stay: `LOG.md`, `RESULT.md`, finished briefs, `docs/{decisions,design,research,
  knowledge,testing,archive}/**` and the changelog.
- **ADR number:** `ADR-0011` is already accepted (`raw-ingress boot refusal`, MB.3.53), so the
  Blastfile ADR is **`ADR-0012-blastfile-dsl.md`**. Any brief that says `ADR-0011` for this work is
  stale; fix the reference rather than the number.
- **Crate home:** unless `ADR-0012` decides otherwise, the lexer/parser/CST live inside
  `crates/blastit-manifest/` (a `blastfile/` module tree) so `docs_fences.rs`, `edit.rs` and the
  protected manifest suites keep their home. If the design introduces a new workspace crate, `lead`
  widens the child `paths` with `plan.py edit` **before** dispatch, and one strictly serialized child
  owns `Cargo.toml` + `Cargo.lock`.
- **Zero new dependencies:** hand-written lexer and LL(1) parser (no `logos`/`pest`/`chumsky`/`nom`).
  Every iteration must be bounded by input length, and nesting depth by the grammar (two levels), so
  a hostile `Blastfile` cannot blow the stack. The retired reader and its dependency are removed by
  `MB.21.4.5`, the only child that owns `Cargo.toml`/`Cargo.lock`.
- **Protected tests:** the cutover necessarily rewrites fixtures inside
  `crates/*/tests/critical_*.rs` and `python/tests/critical/`. Each such child lists the exact files
  in `paths` and says why in `# Goal`, and runs
  `uv run --no-project tools/critical_guard.py --base <sha> --node <id>` before reporting.
- **Prose rule:** `python/tests/test_docs_assets.py` refuses a sentence that names the runtime word
  together with the last-resort static file, and it treats `route` as that runtime word. Read its
  docstring; keep language examples in fences.
- Priority 1: this is release-blocking for v0.2.0. `MB.21.1.1` is independent (doc-only, zero code
  dependencies) and can run immediately; `MB.21.5.1` must not run concurrently with it, because both
  own `docs/blastfile.md`'s neighbourhood and `zensical.toml`.
- **Human requests `H-010`–`H-013`** (`plan/HUMAN.md`; `H-010`–`H-012` are already marked closed
  there, `H-013` is not): each was answered by a lead decision in the node logs — `MB.21.3.7`'s log for
  `H-010` (the four per-view accessors, `SpaLost`, §9.5/§9.6 and the sentences move to `MB.21.4.2`),
  `MB.21.3.1`'s for `H-011`, `MB.21.3.2`'s for `H-012`, and `MB.21.3.3`'s for `H-013` (`error.rs`
  widened into 3.3 and 3.8) — so the conductor closes `H-013` with `plan.py human close`; none of them
  is an open design question and no leaf should re-ask one.
- **Owner directive (2026-11-16): "let's allow something comfortable" — the budgets were too small for
  a generated file.** `MB.21.4.6` raises six numbers and lands between `MB.21.4.1` and `MB.21.4.2`:
  `MAX_SOURCE_BYTES` `1_048_576` → `16_777_216` (16 MiB), `MAX_TOKENS` `262_144` → `4_194_304`,
  `MAX_TRIVIA` `262_144` → `4_194_304`, `MAX_PATTERN_BYTES` `256` → `4_096` (with `REGEX_SIZE_LIMIT`
  still bounding the compiled program), `MAX_LIST_ITEMS` `256` → `4_096`, and `app "<name>"`
  `64` → `256` `char`s; it also replaces the "split the file" refusal, which advised splitting a format
  that has no `include`, with the real remedies (table grants, or a shared `trait`).
- **Out of scope for this epic:** the `blastit-view` flake in `crates/blastit-view/src/php.rs::wait_for`
  (200 × 5 ms = 1 s budget for three frames, which failed 2 of 10 `make check` runs under load) needs its
  own leaf in the milestone that owns that crate; no child of `MB.21` touches `blastit-view`, and this
  epic must not absorb it.
