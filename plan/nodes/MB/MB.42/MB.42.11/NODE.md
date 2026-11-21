---
id: MB.42.11
title: "blastit check from a package/bundle: bundled_python() demands _bin, which no bundle ships (check exits 3)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.3.1]
paths: [crates/blastit/src/check.rs, "crates/blastit/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T01:51"
updated: "2026-11-20T01:59"
---

# Goal
`blastit check` must work from a **package or bundle** install. Today it cannot: `bundled_python()` (`crates/blastit/src/check.rs:1122-1137`) keeps a root only if it contains **both** `blastit/_check_pytest.py` **and** `blastit/_bin/blastit-dbproxy`, but `_bin` is a *retired wheel layout* — `scripts/check-bundle-layout.sh` actively **fails** a bundle that contains it, and `make install` never creates it. So `bundled_python()` returns `None` for every `.deb`, `.rpm`, `.tar.gz` and `make install` tree, `PYTHONPATH` is never set, and:

```
$ env -i PATH=<prefix>/bin:/usr/bin:/bin blastit check        # WSGI fixture
ERROR: Error importing plugin "blastit._check_pytest": No module named 'blastit'
exit 3
```

Confirmed twice, independently, by `review-arch` and `review-ux` on `MB.42.3.1`; dropping a dummy `_bin/blastit-dbproxy` into the same tree makes it work, which proves causality. `blastit dev` on the same tree serves WSGI 200 (so no shim-dir env var is needed) — **`check` is the only broken command**, and it is the CI story.

# Exact Deliverables
1. **Change the marker** to something a real bundle ships. `_check_pytest.py` alone is already sufficient to distinguish a bundle from a `cargo` tree (a `target/<profile>/../python` and `…/../../..` never contain a `blastit/` package), so use `_check_pytest.py` **and** `__main__.py` as the pair, and delete the `_bin` requirement.
2. **Rewrite the stale comment** above it (`check.rs:1119-1121`, which claims `_bin/blastit-dbproxy` is "what only a bundle ships") to state the real roots: a bundle's `<exe_dir>/../python` (`<prefix>/share/blastit/python`) and a wheel's `<exe_dir>/../../..`.
3. Add a test proving it: a temp tree shaped like a **bundle** (seven modules under `python/blastit/`, `bin/blastit` symlink -> `../share/blastit/lib/blastit`, **no** `_bin`) resolves a package root, and the same tree **with** `_bin` absent still resolves. The existing "a cargo tree is not a bundle" case must keep passing.
4. If a protected critical test pins the old marker or the old comment text, **stop and report** instead of editing it — that is a lead decision.

# Notes
- This is a functional bug with a one-place fix; do not refactor `check.rs`.
- `scripts/check-bundle-layout.sh` stays as it is: `_bin` must keep being refused. Do **not** "fix" this by shipping `_bin`.
- `<500` prod LOC, no globals; never touch `docs/**`; never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- The reproduction above (a package-shaped prefix, a WSGI fixture, `env -i`, no `PYTHONPATH`) now runs `blastit check` and reports the gate **clean** (exit 0), with the plugin importing from the bundle's `python/` tree
- `cargo test -p blastit --locked` (the new bundle-root test + the cargo-tree-not-a-bundle case)
- `cargo clippy -p blastit --all-targets --locked -- -D warnings` and `--no-default-features`
- `make test-critical` (the staged `_bin/blastit` layout must keep working)
- `make fast-check`
