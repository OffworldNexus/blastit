---
id: MB.21.4.2
title: "Prove runtime parity on Blastfile and land design §4.3's removals: static precedence, ingress modes, entrypoint php, traits, grants"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.1]
paths:
  - "crates/blastit/src/**"
  - "crates/blastit/tests/**"
  - crates/blastit/tests/critical_static_cli.rs
  - crates/blastit/tests/critical_php_backend.rs
  - crates/blastit/tests/no_python_build.rs
  - crates/blastit/tests/critical_raw_ingress.rs
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-scheduler/tests/critical_static_entry.rs"
  - "crates/blastit-scheduler/tests/critical_static_assets.rs"
  - "crates/blastit-scheduler/tests/critical_static_resolve.rs"
  - "crates/blastit-scheduler/tests/critical_rewrite.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/routing.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/uid.rs"
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/blastfile/lower.rs"
  - "crates/blastit-manifest/src/blastfile/write.rs"
  - "crates/blastit-manifest/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T08:04"
---

# Goal

Prove the running product is identical on `Blastfile`: every capability-map row gets a protected
end-to-end assertion, and any real gap the new syntax exposes is closed here rather than documented
away. This leaf is also where design §4.3's **removals** land — the four per-view accessors, the
`SpaLost` → `fallback_missing` rename, §9.5/§9.6 over mounts, §9.2's precedence and the per-mount
`immutable` caps — because they could not land while the retired reader was still the only caller
(`MB.21.3.7`'s attempt 1, H-010).

# Context

- Design §9.2–§9.5 and §12. The parser produces the same `Manifest`, so this leaf is mostly **proof**
  plus the few places where a new spelling lands differently:
  * static precedence: (1) a file on disk under the prefix, (2) the first matching view, (3) the
    declared deep-URL file, (4) `404`; and the refusals that must survive — `.phar`/`.php` sources
    under the root, dotfiles, `..`, NUL bytes and symlink escapes;
  * ingress `body json|form|none|raw` and the production refusal for an unacknowledged `raw`;
  * `entrypoint php` as the app-wide front controller (the per-view `workers` cap is removed, §12.3);
  * `prefork`, `egress any` (boot warning parity), a trait's grants (`use`), `postgres … unrestricted`
    (inline-only, warns), table/column grants and `query '…'`;
  * `HEAD` implied by `GET` through the scheduler's real matching (not only at parse time).
- Suites: extend/replace fixtures in
  `crates/blastit/tests/{critical_static_cli,critical_raw_ingress,critical_php_backend,critical_dev_and_fail_closed}.rs`
  and add `crates/blastit/tests/critical_blastfile_parity.rs` as the one place the capability map is
  enumerated. All are listed in `paths` deliberately; no assertion may be weakened, and `RESULT.md`
  lists each fixture rewrite.
- Keep the scheduler untouched unless a real gap appears: `crates/blastit-scheduler` is in `paths` so
  the precedence work may land there if `body/plan_body` or the static pump genuinely needs it.
- **The §4.3 removals, which are this leaf's work list** (`MB.21.3.7` deferred them; `paths` now covers
  every file below):
  * delete `Route::static_policy()`, `Route::with_static`, `Route::php()` and `Route::with_php`
    (`model.rs`): a `Blastfile` has no per-view static or php declaration, and keeping them would let
    the model express only what the retired file could. Compile errors are the work list, in this
    order: `model.rs`'s `is_all_static`/`can_boot_without_app`/`static_roots`/`check_static_roots`,
    `validate.rs`, `blastfile/{lower,write}.rs`, `edit.rs`, `parse.rs` and the two manifest suites.
  * the retired reader is still alive until `MB.21.4.5`, so `parse.rs` keeps compiling by lowering
    into `Manifest::with_mounts`/`with_php` and `Route::with_sandbox` through the parse-time bridge
    `MB.21.3.7` recorded — it gains no feature, and 4.5 deletes it as it stands;
  * `StaticOutcome::SpaLost` → `fallback_missing` (`crates/blastit-scheduler/src/static_asset.rs`),
    doc comment "the declared `fallback` file is missing or refused" (§9.2); the arm in `rewrite.rs`
    and the three suites that pin it follow: `critical_rewrite.rs`, `critical_static_resolve.rs`,
    `critical_static_assets.rs`;
  * §9.5 `is_all_static`/`can_boot_without_app` (no app, no php, no views, ≥ 1 mount) and §9.6
    `static_roots`/`check_static_roots` iterate **mounts**; a static-only file must boot and `check`
    must audit it without `BLASTIT_APP`. The sentences change subject (`route "site"` → the mount's
    prefix, `route "app"` → the entrypoint), so `crates/blastit/tests/{critical_static_cli,
    critical_php_backend,no_python_build}.rs` change with them — every changed assertion is listed in
    `RESULT.md` beside its new subject, and none is deleted;
  * §9.2's four steps: longest prefix wins on a segment boundary, `Missing` falls to step 2 (never
    `404`), `Hidden`/`Refused` are terminal on the full decoded path, the fallback answers `GET`/`HEAD`
    only, `405` after every view. `RouteInfo::static_forward` is deleted from
    `crates/blastit-scheduler/src/route.rs` and its readers (`rewrite.rs`, `static_response.rs`,
    `config.rs`/`stage1.rs`) read the mount instead;
  * §9.4 `immutable` keeps the client's full path (the `prefix + relative` join, never `//assets/...`)
    and its caps move from per route to per mount (≤ 32 patterns, each ≤ 256 bytes), with the sentences
    naming the mount; §9.1's dotfile definition is applied to `check_static_index`: a dotfile segment
    is refused in `index`/`fallback` while `root "."` stays legal;
  * the two retired-spelling sentences in `validate.rs` (`route … own sandbox`, "a root that names no
    directory beneath `blastit.toml`") are re-spelled; their assertions live in the `crates/blastit/tests/*`
    suites above, which is why this leaf owns them and 3.7 did not.

# Acceptance

```console
$ make test-critical
$ make rs-test
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.2
$ grep -rnE 'Route::with_static|fn static_policy|fn with_static|SpaLost|static_forward' crates/ --include='*.rs'
$ grep -rn 'route "site"\|route "app"' crates/blastit/tests/
$ make check
```

1. `critical_blastfile_parity.rs` enumerates one case per capability-map row and names the row in the
   test name, so a reviewer can diff the map against the test list.
2. Precedence is proven in the documented order on a fixture where all four steps are reachable — a
   nested `static "/assets/v2"` beats `static "/assets"` on the longest prefix, `Missing` at step 1
   reaches a view, `Hidden`/`Refused` never do — and each refusal (dotfile, `..`, NUL, symlink escape,
   `.php` under the root) is a deny-side assertion.
3. Each ingress mode is proven allowed and denied (a JSON route rejects a form body with `400`, a `none`
   route rejects any body with `413`, a `raw` route without acknowledgement refuses the production boot
   and warns under `dev`).
4. A static-only `Blastfile` (mounts, no views) boots and passes `check` with no `BLASTIT_APP`; a
   missing/unreadable `root` is refused naming the mount's prefix, not a route; the per-mount
   `immutable` cap and the `index`/`fallback` dotfile refusal each have a deny-side test.
5. The first `grep` prints nothing in `crates/*/src` or `crates/*/tests` (`Manifest::php()` stays; only
   the per-view accessors go), and the second prints nothing: the static/PHP sentences name a mount or
   the entrypoint, and `RESULT.md` lists each one with its old and new subject.
6. `make test-critical` exit 0 (zero skips) and `make check` exit 0.
7. Any behavioural change beyond parsing is named in `RESULT.md` with the test that pins it; there are none
   silently.

# Notes

- `HEAD`-from-`GET` is the one intentional semantic change: it must be visible in the parity suite and
  in `docs/reference/manifest.md`'s replacement (`MB.21.5.1`).
- If a case can only be proven in a container, say so and give the `e2e/` scenario name that covers it;
  `MB.21.4.4` owns that fixture.
- Protected suites in `paths` are there to be **rewritten, never weakened**: every removed assertion is
  named in `RESULT.md` with the contract that replaced it, and `critical_guard.py --node MB.21.4.2` is
  clean. If a sentence genuinely loses its subject (a mount has no route name), that is a design
  decision already taken in §9.5/§9.6 — record the new subject, do not keep a dead noun.
- `Manifest::php()` and `Manifest::with_php` stay (one entrypoint for the file). Only the per-view
  accessors go; a grep that also matches `Manifest::php` is the wrong grep.
