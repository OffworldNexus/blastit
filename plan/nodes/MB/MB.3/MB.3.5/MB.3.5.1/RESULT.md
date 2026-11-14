# RESULT MB.3.5.1 (attempt 1)

STATUS: done

History: this node was briefly `blocked` while `docs/frameworks/php.md` still
carried `listen = "run/fpm.sock"`; the blocked window closed when that page
landed the entry-only manifest (`MB.3.9.1`). **No source or test file changed
since** — only this RESULT was rewritten with the green runs below.

## What landed (all in the six scoped files)

- `FastCgiPolicy { entry: String, workers: Option<u32> }` — no socket key; `None`
  means "blastit derives the pool size from its concurrency budget" (the
  derivation itself belongs to `MB.3.9.2`/the scheduler and is not implemented
  here).
- `Executor { View, Pool(FastCgiPolicy) }`, `kind()` (`"wsgi"`/`"fastcgi"`),
  `ALL_KINDS = ["wsgi", "fastcgi"]`, crate-root re-export + `lib.rs` doctest.
- `[route.fastcgi]` parsing: `entry` required; `workers` optional integer;
  `listen` refused with `FastCgiListenDerived`, whose message names the key and
  says blastit owns the socket; any other fourth key is `UnknownKey`.
- Validation (pure, no filesystem): entry relative / no `..` / no `\` / no
  control byte / `.php`; explicit `workers` in `1..=64`.
- Both dead-declaration arms: the static arm is gated on `fastcgi.is_none()` and
  keeps its 0.2.0 `why` strings; `spa = true` + fastcgi, `CONNECT` and `TRACE`
  are `FastCgiDeadDeclaration`.
- `Manifest::can_boot_without_app()`, `is_all_static()` (+ `fastcgi().is_none()`),
  `route_lines()` (boot line scan stored at parse time, routing order).
- `docs_fences.rs`: `FILES` 7→8 with `docs/frameworks/php.md`, `total` 7→8,
  module doc "seven"→"eight".
- Removed `FastCgiListenInvalid` / `FastCgiListenParentInvalid` (dead with the
  key); added `FastCgiListenDerived`.

## Acceptance evidence (green, taken after the blocked window closed)

```console
$ cargo test -p blastit-manifest --locked
test result: ok. 50 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (lib)
test result: ok. 17 passed; 0 failed; ...
test result: ok. 23 passed; 0 failed; ...
test result: ok. 15 passed; 0 failed; ...
test result: ok. 5 passed; 0 failed; ...   (docs_fences)
test result: ok. 19 passed; 0 failed; ...
test result: ok. 13 passed; 0 failed; ...
test result: ok. 20 passed; 0 failed; ...
test result: ok. 33 passed; 0 failed; ...
test result: ok. 11 passed; 0 failed; ...
test result: ok. 6 passed; 0 failed; ...
test result: ok. 29 passed; 0 failed; ...
test result: ok. 5 passed; 0 failed; ...   (doctests)
```

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
test every_documented_manifest_parses_and_validates ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

```console
$ make check
56 fences: 29 executed, 27 accounted, 0 failed · 22.1s
uv run --no-project tools/plan.py validate
plan OK
# EXIT=0
```

```console
$ cargo test -p blastit-manifest --test critical_static_manifest --locked
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 finished
```

Corrected crate for the CLI suite (reviewer finding): `critical_static_cli.rs`
lives in `crates/blastit/tests/`, so its command is
`cargo test -p blastit --test critical_static_cli --locked` — **not**
`-p blastit-manifest`. Both protected suites stay green unmodified.

```console
$ ! grep -qE 'rustix|libc' crates/blastit-manifest/Cargo.toml && echo "manifest stays syscall-free"
manifest stays syscall-free
```

## Budget: declared waiver

`git diff --numstat` over the six files: **873 insertions, 15 deletions = 888
changed lines**, against the design's ≤550-line guide. This is a declared
waiver, not an unreported overrun: the leaf owns the SQL-free grammar
(`entry` + optional `workers`, plus the `listen`-removed diagnostic), **both**
dead-declaration arms (static 0.2.0 arm preserved + FastCGI arm), the five
public error variants with their pinned messages, two public types, the three
boot predicates/line scan, and the 8-file docs-fence accounting. No rule, no
denied/accepted twin, and no protected test was dropped to fit the guide.

## Follow-ups

1. **Design table** — accepted `docs/design/php-fastcgi.md` §4.1 still shows the
   old three-key table; `MB.3.9.2` is amending it to the owned-socket shape the
   code now implements.
2. **`ROUTE_KEY_EXPECTED`** (parse.rs) still lists the route keys without
   `fastcgi`, because `crates/blastit-manifest/tests/parse.rs` (outside this
   node's paths) pins that exact string; update the constant and that test
   together in a later leaf.
3. **Consumers** — `FastCgiPolicy.workers: Option<u32>`: `MB.3.5.2`/`MB.3.6.*`
   must treat `None` as "derive from the worker budget" and never substitute a
   literal `4`.
4. **Harness note** — the `static-and-spa.md` install fence can fail when
   another session rebuilds `dist/` during a shared-checkout `make check`
   (installed wheel vs. hashed wheel differ); the green run above was taken with
   no concurrent `make check` running.
