---
id: MB.21.4.2
title: "Prove runtime parity on Blastfile: static precedence, ingress modes, entrypoint php, traits, grants"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.1]
paths: ["crates/blastit/src/**", "crates/blastit/tests/**", "crates/blastit-scheduler/src/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Prove the running product is identical on `Blastfile`: every capability-map row gets a protected
end-to-end assertion, and any real gap the new syntax exposes is closed here rather than documented
away.

# Context

- Design §4–§8. The parser produces the same `Manifest`, so this leaf is mostly **proof** plus the few
  places where a new spelling lands differently:
  * static precedence: (1) a file on disk under the prefix, (2) the first matching view, (3) the
    declared deep-URL file, (4) `404`; and the refusals that must survive — `.phar`/`.php` sources
    under the root, dotfiles, `..`, NUL bytes and symlink escapes;
  * ingress `body json|form|none|raw` and the production refusal for an unacknowledged `raw`;
  * `entrypoint php` as the app-wide front controller (with the design's `workers` answer, if any);
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

# Acceptance

```console
$ make test-critical
$ make rs-test
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.2
$ make check
```

1. `critical_blastfile_parity.rs` enumerates one case per capability-map row and names the row in the
   test name, so a reviewer can diff the map against the test list.
2. Precedence is proven in the documented order on a fixture where all four steps are reachable, and
   each refusal (dotfile, `..`, NUL, symlink escape, `.php` under the root) is a deny-side assertion.
3. Each ingress mode is proven allowed and denied (a JSON route rejects a form body with `400`, a `none`
   route rejects any body with `413`, a `raw` route without acknowledgement refuses the production boot
   and warns under `dev`).
4. `make test-critical` exit 0 (zero skips) and `make check` exit 0.
5. Any behavioural change beyond parsing is named in `RESULT.md` with the test that pins it; there are none
   silently.

# Notes

- `HEAD`-from-`GET` is the one intentional semantic change: it must be visible in the parity suite and
  in `docs/reference/manifest.md`'s replacement (`MB.21.5.1`).
- If a case can only be proven in a container, say so and give the `e2e/` scenario name that covers it;
  `MB.21.4.4` owns that fixture.
