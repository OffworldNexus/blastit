# Result MB.2.8 — release prep 0.2.0

## Summary

Workspace is release-ready at **version 0.2.0, dated 2026-11-14**. Every one of
the **28** intra-workspace `version = "...", path = "../…"` requirements now
names `0.2.0` (28 before, 28 after; none left behind). `Cargo.lock` was edited
line by line — only the 13 workspace `[[package]]` blocks changed, one line each,
no dependency or checksum moved, and no `cargo update` ran.

- `CHANGELOG.md`: a dated `## 0.2.0 (2026-11-14)` entry in the 0.1.1 voice and
  length (lede + six bullets): static serving and its total order, SPA fallback
  and front controller, the `400`/`404` denial classes, immutable caching keyed
  on the answering file, the all-static `dev`/`check` experience (`0`/`2`, no
  pytest), and the two pooled deployment recipes.
- `ARCHITECTURE.md` §2.4 gains `#### The static responder` (25 lines): answered
  by the scheduler itself (same process, uid and inherited listener — no new
  role, uid or socket), the first-match order, and the path-safety boundary
  (`openat2` with `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS`,
  the `O_NOFOLLOW` walk fallback, `O_PATH` probe + read-open with a matching
  `(st_dev, st_ino)`, `400` before any syscall vs `404`), plus the answering-file
  cache key.
- **Design vs code:** no material divergence found. The section was written from
  the shipped `crates/blastit-scheduler/src/{static_asset,rewrite,static_response}.rs`
  and `crates/blastit/src/stage0.rs::app_rule`; every claim re-read there. One
  shipped detail the design's summary omits is documented because the code has
  it: the per-segment `O_NOFOLLOW` `Backend::Walk` used where the kernel has no
  `openat2` (`static_asset.rs:15-16`, `:538-560`).

No tag, push, commit, `git add` or plan-status mutation. `dist/` now holds the
single `0.2.0` wheel, as `make e2e` requires.

## Evidence

All commands run from `/home/remy/offworld-nexus/blastit` on `release/0.2.0`
(base `ebd0ee5`), after the final edits.

### 1. Workspace version

```console
$ grep -q '^version = "0.2.0"$' Cargo.toml && echo "workspace version bumped"
workspace version bumped
```

### 2. Every intra-workspace requirement (count before/after)

Before (`git show HEAD`): 28 lines matching `path = "../` **and**
`version = "0.1.2"`. After: 28 matching `version = "0.2.0"`.

```console
$ before=$(grep -rn 'path = "\.\./' crates/*/Cargo.toml | grep -c 'version = "0.1.2"'); echo "before: $before"
before: 28
$ grep -c 'path = "\.\./' crates/*/Cargo.toml | awk -F: '{s+=$2} END {print "total:", s}'
total: 28

$ ! grep -rn 'path = "\.\./' crates/*/Cargo.toml | grep -v 'version = "0.2.0"' | grep 'version = ' && echo "every intra-workspace requirement names 0.2.0"
every intra-workspace requirement names 0.2.0
```

### 3. Lockfile still agrees (`--locked`)

```console
$ CARGO_NET_OFFLINE=true cargo metadata --locked --no-deps --format-version 1 > /dev/null && echo "lockfile agrees"
lockfile agrees
```

`git diff --stat Cargo.lock` → `13 insertions(+), 13 deletions(-)`: the 13
workspace `[[package]]` versions. The three other `version = "0.2.0"` lines in
the file (`fallible-iterator`, `matchers`, `tracing-log`) already said so at
`ebd0ee5`; the only `0.1.2` left is the external `peeking_take_while`.

### 4. Release tag gate

```console
$ scripts/check-release-tag.sh v0.2.0
tag v0.2.0 names the workspace version 0.2.0
```

### 5. Dated release notes

```console
$ scripts/release-notes.sh --require-dated v0.2.0 > /dev/null && echo "dated changelog entry"
dated changelog entry
```

`scripts/release-notes.sh v0.2.0 | head -3` starts:

```text
Your built front end now runs under blastit, and an all-static site needs no app
at all.

- **Serve a built front end from the manifest.** A `[route.static]` with a
```

### 6. `make check`

```console
$ make check
...
47 fences: 28 executed, 19 accounted, 0 failed · 23.1s
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

(`docs-test` built and installed `blastit-0.2.0-…whl`, so the version is
consistent through the wheel build too.)

### 7. `make e2e`

First run, with the stale development wheel still in `dist/`:

```console
$ make e2e
...
a prebuilt e2e image installs exactly one wheel from /home/remy/offworld-nexus/blastit/dist, found 2:
  blastit-0.1.2-cp314-cp314-manylinux_2_39_x86_64.whl
  blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl
...
docs_deploy: 41/41 documented pooler promises hold
make: *** [Makefile:161: e2e] Error 1
```

Deleted **only** the stale wheel (`rm -f dist/blastit-0.1.2-*.whl`) — no other
file in `dist/` was touched — and re-ran:

```console
$ ls dist/
blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl
$ make e2e
...
  ok   and nothing claims the manifest is missing ([])
exit=0
```

No "exactly one wheel" complaint in the final run; the whole harness (Docker,
rootless capabilities, docs pooler promises) passed.

## Files

- `Cargo.toml` — `[workspace.package] version = "0.2.0"`.
- `crates/blastit/Cargo.toml` (11), `crates/blastit-py/Cargo.toml` (4),
  `crates/blastit-scheduler/Cargo.toml` (4), `crates/blastit-dbproxy/Cargo.toml` (3),
  `crates/blastit-http/Cargo.toml` (2), `crates/blastit-manifest/Cargo.toml` (2),
  `crates/blastit-env/Cargo.toml` (1), `crates/blastit-view/Cargo.toml` (1) —
  28 requirements to `0.2.0` (counts per file sum to 28).
- `Cargo.lock` — 13 workspace package versions, nothing else.
- `CHANGELOG.md` — `## 0.2.0 (2026-11-14)` above `## 0.1.2`.
- `ARCHITECTURE.md` — `#### The static responder` in §2.4.

## Follow-ups

1. **`ARCHITECTURE.md` §7.1 trust-boundary table.** The scheduler row still says
   only "it can send any request to any view"; the shipped responder also reads
   beneath each operator-declared static `root` (stated in the new §2.4
   subsection, and in the design's §5.1 trust paragraph). Left alone to keep this
   node to one section; a one-clause edit would make the security table match.
2. **`dist/` housekeeping is manual after a version bump.** `scripts/build-wheel.sh`
   leaves the previous release's wheel next to the new one, and `make e2e`'s
   prebuilt-wheel guard then refuses the tree. Either the guard could ignore
   wheels whose version is not `Cargo.toml`'s, or `build-wheel.sh` could remove
   stale `blastit-*.whl` before building. Both are outside this node's paths.
3. **Pre-existing dirty `README.md`.** It was modified before this session
   (` M README.md` at the base checkout, absolute links from a `build-wheel.sh`
   rewrite). Not touched here; `scripts/build-wheel.sh` reports
   `README.md: already absolute`, so the tree stays as it was.
4. **`docs/frameworks/static-and-spa.md:141`** still says "`0.2.0` adds no
   deployment story" while §132-142 shows `--preset compose-traefik`; that is
   about the static feature (a preset already served the site), and MB.2.11
   changed the recipes, not this page. Changelog bullet 6 states the pooler
   change; the page could say so in 0.2.1.
