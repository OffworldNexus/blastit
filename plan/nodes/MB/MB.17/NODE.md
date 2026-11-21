---
id: MB.17
title: "Default build must be the portable standalone binary (no libpython, no cache RUNPATH)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - Cargo.toml
  - Cargo.lock
  - crates/blastit/Cargo.toml
  - crates/blastit/build.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/main.rs
  - "crates/blastit-py/Cargo.toml"
  - ".github/actions/build-wheel/action.yml"
  - Makefile
  - "scripts/**"
  - ".github/workflows/**"
  - "docs/reference/**"
  - "docs/getting-started.md"
  - docs/knowledge/LESSONS.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T16:38"
updated: "2026-11-14T17:43"
---

# Goal

Make the binary a plain `cargo build` produces **runnable and shippable on any Linux** — no `libpython` linkage, no `RUNPATH` into a developer's uv cache. Owners reported it directly: `target/debug/blastit --port 4242` dies with `libpython3.10.so.1.0: cannot open shared object file` on 0.2.0.

# Context

Measured on `release/0.2.0`/`0.2.1`:
- `readelf -d target/debug/blastit` → `NEEDED libpython3.14.so.1.0` **and** `RUNPATH /home/remy/.local/share/uv/python/cpython-3.14.10-…/lib`. That path is specific to one machine's uv cache; the artifact is not portable even to another user on the same box.
- `cargo build -p blastit --no-default-features --features php` → `NEEDED` is libc/libm/libgcc/ld only, and the binary runs. So the capability gate works (`MB.3.11`); the **defaults** are what is wrong.
- `crates/blastit/Cargo.toml` has `default = ["python", "php"]`, so every plain `cargo build` produces the Python-linked binary.
- `--port` is a **`blastit dev`** flag only; production `blastit` takes `--preset` and settings (`BLASTIT_*`). The reported command was wrong too, but the loader error masked it.

# Acceptance

1. **Default build is the standalone one.** `crates/blastit/Cargo.toml` default features no longer include `python` (keep `php`, which needs nothing at link time). A fresh `cargo build -p blastit --locked` then `./target/debug/blastit dev --port 8080` must work on a machine with **no libpython and no Python at all** — prove it with `readelf -d` showing no `libpython` and no `RUNPATH` to a cache path, and by running the binary.
2. **WSGI opts in explicitly.** `crates/blastit-py` (the wheel) enables `blastit/python`; `make` targets that need the interpreter (`check`, `test-critical`, e2e) pass `--features python`, so nothing silently loses WSGI coverage. `make check` stays green.
3. **No `RUNPATH` to a build host.** Whatever still links Python (the dev/test build and the wheel path) must not embed `$HOME`/uv-cache runpaths: use `-C link-arg=-Wl,--disable-new-dtags`? no — the requirement is simply that the shipped artifact (`--no-default-features`) has **no** Python `NEEDED`/`RUNPATH`, and that the wheel's extension module does not either (verify the maturin-built `.so` with `readelf`).
4. **CI guard.** The standalone artifact is checked mechanically: `readelf -d | grep -q libpython` must fail the build, and a job runs the standalone binary on an image with **no Python installed** serving a static fixture (200/404/no-cache), so "no libpython" is proven by execution, not by inspection alone.
5. **Docs.** `docs/reference/cli.md` (or the getting-started page) states: `blastit dev --port N` for local; production `blastit --preset …` with `BLASTIT_*` settings and no `--port`; and the standalone build command (`cargo build -p blastit --locked`, plain) is the one that produces the portable binary. One or two lines, not a tutorial.
6. `make docs-serve` (MB.14) builds the **standalone** variant, so the dogfood target cannot reintroduce the dependency.

# Notes

- This is the difference between "a Rust tool you can copy onto a server" and "a build artifact that only works in the checkout that produced it". It must land **before** the 0.2.0 release PR is presented.
- Do not gate the static/PHP paths behind `python`; they never needed it. The rule is: `python` is a capability, `php` is a capability, and the default build carries the one that needs no interpreter.
- Keep `cargo test --workspace` (with python features) as the dev gate; the standalone build is a separate, smaller target that CI also tests.
