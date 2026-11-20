# MB.42.3.4 — signed-repo publishing: `packages.yml` + `nightly.yml`

## Status: done

Dry-run only, as the node requires: no real publish happened, no secret was
printed, and nothing was written to `docs/**`, `~/.local` or `/usr/bin/blastit`.

## What landed

| Path | What |
|---|---|
| `.github/workflows/packages.yml` | `v*` tag: plan → matrix → shims/py-shims → 4 packages → **12 container install legs** → Cloudsmith `main` + Forgejo mirror → release reseal. `workflow_dispatch` rehearses the whole graph and prints every publish command instead of uploading. |
| `.github/workflows/nightly.yml` | `schedule` + `workflow_dispatch` on `develop`: the same graph with the stamped versions, the `nightly` component, then retention (newest 7). |
| `scripts/publish_packages.py` | The only place the publish commands, the nightly stamps, the Forgejo upload and retention live. Every mode has `--dry-run`. |
| `scripts/install_matrix.sh` | One leg of the install matrix, addressed by the table's own name; delegates the container proof to `scripts/test_packages.sh`. |
| `scripts/platform_matrix.py` | `INSTALL_LEGS` (6 distributions) + `install_legs()` (× 2 architectures = 12), `--matrix/--count/--summary/--leg install`, `--name`, and a `--check` that refuses a WSGI leg on an image without CPython 3.13. |
| `scripts/check_workflows.py` | Rule 20: no distribution image and no literal `strategy.matrix` in `packages.yml`/`nightly.yml`; `packages`/`install` families wired into rules 7/19. Self-tested before it runs. |
| `scripts/test_packaging_gates.py` | `package-workflow` (7 cases) + `publish` (13 cases): stamps, both builders' acceptance, the exact `cloudsmith push` argv, key-unset fail-closed, the Forgejo `::notice::`/multipart/basic-auth against a loopback server, retention by (date, run). |
| `scripts/test_packages.sh` | `BLASTIT_PACKAGE_MODES` (the matrix decides which phases an image runs); an explicit `BLASTIT_DEB_IMAGE` means exactly one image. |

The four build scripts needed no change: `build-deb.sh` already accepts
`[0-9][0-9A-Za-z.+~]*` (so the literal `~` lands unmodified) and `build-rpm.sh`
already splits `Version: 0.2.0` from `Release: 0.nightly.<date>.r<run>.g<sha>`
(the `-0.` prefix). That is now *proved*, not assumed — see case 2 below.

## Evidence

### `uv run --no-project scripts/check_workflows.py`

```
workflow-check: rule 20 self-test ok — a distribution written in a package workflow is a second copy of the table
workflow-check: ok — 674 checks over 9 workflows and 1 actions, 0 problem(s)
```

### `uv run --no-project scripts/test_packaging_gates.py`

```
packaging-gates-test: ok — 98 cases passed (workflow=19, matrix=15, wheel-tag=4, exec-env=12, test-seam=7, portable-elf=2, max-glibc=4, repack=13, matrix-producer=2, package-workflow=7, publish=13)
```

### The nightly stamps, printed by the script the workflow calls

```
$ uv run --no-project scripts/publish_packages.py stamp --format both --run 7 --sha 3f9a1c2d4e5f6a7b
deb_version=0.2.0~nightly20261120.r7+g3f9a1c2
deb_release=
rpm_version=0.2.0
rpm_release=0.nightly.20261120.r7.g3f9a1c2
```

Both builders accept them (their version checks run before the bundle check, so
"no such bundle" is the accepted answer) — `test_packaging_gates.py` proves it by
running the real scripts; `0.2.0-rc1` is still refused by `build-deb.sh`.

### The token-unset path: exit 0 + one `::notice::`

```
$ FORGEJO_PACKAGES_TOKEN= FORGEJO_URL=https://forge.invalid \
    uv run --no-project scripts/publish_packages.py forgejo \
    --format deb --file …/blastit_0.2.0_amd64.deb --component main
::notice::forgejo mirror skipped: FORGEJO_PACKAGES_TOKEN is unset, so nothing is mirrored (the Cloudsmith repositories are the install path)
exit=0
```

### The push commands, exercised by `--dry-run`

```
cloudsmith: would run: cloudsmith push deb offworld-nexus/apt/any-distro/any-version …/blastit_0.2.0_amd64.deb --component main --republish
cloudsmith: would run: cloudsmith push rpm offworld-nexus/rpm/any-distro/any-version …/blastit-0.2.0-1.x86_64.rpm --republish
forgejo: would POST https://forge.offworld.nexus/api/packages/OffworldNexus/rpm/nightly/upload (basic auth Xowap:<token>, multipart package=@…/blastit-0.2.0-1.x86_64.rpm)
```

With a **fake** CLI on `PATH` the gate records the exact argv and asserts
`CLOUDSMITH_API_KEY` reached only the child's environment; with the key unset the
push exits 1 (the repositories are the install path, so a release may not be
silently dropped).

### Hand-removed leg: the auditor goes red, and a revert is green again

```
# baseline (unmodified copy)              workflow-check: ok — 674 checks … 0 problem(s)
# one producer line deleted from nightly.yml:
FAIL nightly.yml: job install takes its legs from needs.matrix.outputs.install, but no step of matrix builds them with `scripts/platform_matrix.py --matrix install`: a hand-written leg list ships legs the matrix does not declare, and no other rule can see the difference
workflow-check: FAIL — 673 checks …, 1 problem(s)                          exit=1
# reverted                                 workflow-check: ok — 674 checks … 0 problem(s)
```

And a leg the matrix does not declare, written into `packages.yml` by hand:

```
FAIL packages.yml: job legacy_install (line 3 of the job) names the distribution 'debian:stretch': that is a second copy of the table, … Declare it in `INSTALL_LEGS` (scripts/platform_matrix.py) and take the leg from `--matrix install`
```

### `make fast-check`

```
fast-check exit=0
163 x `test result: ok`
68 files already formatted
plan OK
ok   …/target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

## Decisions worth a reviewer's minute

1. **Two workflows, not a reusable one.** A `workflow_call` indirection would
   halve the YAML but put the artifact uploads in a different file from the
   downloads that consume them, which rule 9 cannot see across files — that is
   exactly the "it looks wired" failure the auditor exists for. The duplicated
   graph is safe because every leg comes from `--matrix install`/`--matrix
   packages` and rule 20 refuses a rewritten one.
2. **12 install legs, not 6.** The node lists six distributions; each is expanded
   over the two architectures the release ships, because the four packages are two
   `.deb` and two `.rpm` and an `aarch64` package that nothing installs is a
   promise. The `aarch64` legs run on the native `ubuntu-24.04-arm` runner.
3. **Cloudsmith fails closed, Forgejo never does.** The repositories are the
   install path, so a missing `CLOUDSMITH_API_KEY` is a red release (and the
   publish job is gated on `github.repository`, so a fork's tag is never asked for
   one). The mirror is best effort by design: unset token → `::notice::`, HTTP
   error → `::warning::`, both exit 0.
4. **Retention sorts by (date, run), not by version string.** `r12` sorts below
   `r9` as text, so a string sort would delete the newest nightly. Only
   `~nightly`/`.nightly.` versions are candidates: the `main` component shares the
   repository and is never touched.
5. **The release assets are the archives'.** `Publish` builds the four `.tar.gz`,
   the five editor assets and the shims from the same tag push; `packages.yml`
   waits (≤30 min) for that release, adds its `.deb`/`.rpm`, reseals **one**
   `SHA256SUMS` over everything and re-uploads with `--clobber` (idempotent on a
   rerun). Building the archives a second time here would be a second copy of the
   matrix that rule 15 exists to police.
6. **The package graph builds its own shims** (`php-sapi-e2e` per gnu leg,
   `make py-sapi` per minor) instead of reading `php-sapi.yml`/`py-sapi.yml`
   artifacts: those workflows run on `develop`/PR pushes, and a tag can arrive
   without one — a release that silently shipped a two-day-old shim would be
   worse than a slow tag.

## Follow-ups

- **`MB.42.4` (docs).** `docs/install.md` must name the two Cloudsmith
  coordinates in this release's exact spelling (`offworld-nexus/apt`,
  `offworld-nexus/rpm`, components `main`/`nightly`), state that the `.deb`/`.rpm`
  support floor is the *measured* `shim_floor` (`2_41` today, from
  `scripts/platform_matrix.py --shim-floor`), and that a nightly never shadows a
  release (`~` sorts below it). Nothing under `docs/**` was touched here.
- **Operator step (not code):** the `nightly` component must exist on both
  Cloudsmith repositories before the first scheduled run, and
  `FORGEJO_PACKAGES_TOKEN` + `vars.FORGEJO_URL` are optional. The Cloudsmith
  repositories' own GPG signing/entitlements are Cloudsmith-side
  (`universal-distribution.md` §6.1), so no key material lives here.
- **`rpmbuild` is absent on this host**, so `build-rpm.sh` was exercised up to its
  version checks only; the container path (`rockylinux:9`) is untouched by this
  node and already covered by `MB.42.3.1`.
- The `release` job assumes, by design, that `Publish` has created the GitHub
  release; if it never does, the job fails after 30 minutes with the exact
  instruction ("run this workflow again once Publish has finished").

---

# Progress (run 2) — the `review-arch` findings

Status: **done**. Every MAJOR is closed, every MINOR is either closed or decided
and named below. No real publish happened; no secret is printed anywhere.

## MAJOR 1 — a `workflow_dispatch` on a tag cannot publish

`packages.yml`'s plan step now requires `event_name == push` **or** the explicit
`publish: true` input, and the input is documented in the header and in
`--help`-shaped prose:

```yaml
      publish:
        description: >-
          Publish (or replace) this ref's packages in the signed repositories.
          A rehearsal prints every command and uploads nothing.
        type: boolean
        default: false
```

It is **proved by running the shipped step**, not by describing it: the gate
extracts the `id: plan` `run:` block from the real YAML and executes it four
ways, with `GITHUB_OUTPUT` captured:

| event | ref_type | `publish` input | `should_publish` |
|---|---|---|---|
| `push` | `tag` | — | `true` |
| `workflow_dispatch` | `tag` | — | **`false`** |
| `workflow_dispatch` | `tag` | `true` | `true` |
| `workflow_dispatch` | `branch` | — | `false` |

## MAJOR 2 — retention counts runs, not packages

`nightly_groups()` groups candidates by the `(date, run)` the stamp encodes and
`cmd_prune` deletes whole groups beyond the newest `--keep`; `--keep 7` is now
seven **runs**. Case, 5 runs × 2 architectures, `--keep 3`:

```
prune: deb: 5 nightly run(s), keeping the newest 3 (6 package(s)), deleting 2 run(s) (4 package(s))
prune: deleted run r1 of 20261101: deb-amd64-r1-day01
prune: deleted run r1 of 20261101: deb-arm64-r1-day01
...
```

The case asserts the exact four slugs (runs 1 and 2, both arches) — never a split
run — plus the same-date pair `r12`/`r9` where `--keep 1` keeps **both** packages
of r12 (a string sort would keep r9). `--dry-run` prints the same grouping and
deletes nothing.

## MAJOR 3 — a literal leg name is refused

Rule 20 now also scans `run:` bodies: `install_matrix.sh --leg <literal>` and
`install_matrix.sh --matrix` are refused in both package workflows.

```
FAIL packages.yml: job install step 4 runs install_matrix.sh with the literal leg 'debian-13-x86_64': that runs one of the twelve install legs and every other distribution goes uninstalled, while `publish` still reads `needs.install.result == 'success'`. Use `--leg "${{ matrix.name }}"` and the matrix from `--matrix install`
```

Both shapes are in `self_test_rule20` (so the recognizer cannot go quiet), and the
original negative — a hand-deleted `--matrix install` producer — still fails:

```
FAIL nightly.yml: job install takes its legs from needs.matrix.outputs.install, but no step of matrix builds them with `scripts/platform_matrix.py --matrix install`: …
```

## MAJOR 4 — the reseal is ordered, not raced

* `packages.yml` and `publish-pypi.yml` now share `concurrency: group: release-${{ github.ref }}`
  (the group is the serialisation; the comment in both files says why).
* The `release` job waits for the **`Publish` run on this commit and this tag** to
  reach `completed`, via `gh api …/actions/runs?head_sha=…` filtered on
  `path == .github/workflows/publish-pypi.yml` and `head_branch == <tag>`, and
  **fails** if it finished non-success (naming `html_url`). `gh release view`
  succeeding — a release that exists halfway through its own upload — is no
  longer a signal.
* Before resealing, every archive `--matrix binaries` declares must be present in
  `dist/`; a missing one is `::error::` and no upload happens, so a `SHA256SUMS`
  can never cover three of four archives.

## MAJOR 5 — no third-party binary holds the key (decision: drop it)

I dropped the CLI. `push` is now one `urllib` request —
`POST https://upload.cloudsmith.io/<owner>/<repo>/{deb,rpm}/` with multipart
`package_file` + `distribution`/`release`/`component` + `republish` — the same
endpoint `cloudsmith push` calls, authenticated with `Authorization: Bearer` and
`X-Api-Key`. Nothing is installed on a tag or a nightly, and the key never leaves
this process. `CLOUDSMITH_CLI` remains an **opt-in** escape hatch (pinned by the
caller, e.g. `uvx --from cloudsmith-cli==<x.y.z> cloudsmith`) and runs with the
allowlisted environment below.

The wire is proved against a loopback server (`CLOUDSMITH_UPLOAD_URL`), not
asserted:

```
POST /offworld-nexus/apt/deb/   Authorization: Bearer <key>   X-Api-Key: <key>
package_file=@blastit_0.2.0_amd64.deb  distribution=any-distro  release=any-version  component=main  republish=true
POST /offworld-nexus/rpm/rpm/   (no `component` field: an RPM repository is one component)
```

**Risk, stated:** if Cloudsmith rejects a field name, the first tag fails with the
HTTP status and the file's name — loud, and fixable without touching the
workflow. `scripts/publish_packages.py` prints the equivalent
`cloudsmith push …` command on every `--dry-run`, so a reviewer can compare.

## MINORs

| # | Disposition |
|---|---|
| 6 | Done. `cli_environment()` hands a child only `PATH`, `HOME`, `LANG` and the three Cloudsmith variables; the case asserts `FORGEJO_PACKAGES_TOKEN` is *absent* from the child's `env`, and the default path has no child at all. |
| 7 | Done, split decision: `workflow-check` is now in `fast-check` (674 checks, offline, ~2 s). `packaging-gates-test` stays out of `fast-check` (it spawns ~20 auditors and ELF fixtures, ~1 min) and is added to the node's `# Acceptance` instead, with the rationale written there. |
| 8 | Done. `scripts/test_publish_gates.py` (740 lines) owns the publish/retention suite; `test_packaging_gates.py` went 2 758 → 2 270 lines and remains the entry point (`import test_publish_gates`). |
| 9 | Done as the assertion: **rule 21** compares the five shared jobs (`matrix`, `shims`, `py_shims`, `packages`, `install`) after folding `plan`/`stamp` and dropping comment-only lines. A reusable workflow would move the producers into a file rule 9 cannot match downloads against, which is the failure that rule exists for. |
| 10 | Done. `dpkg --compare-versions … lt …` proves the nightly sorts below the release (and one night before the next); `rpmdev-vercmp` runs where `rpm` exists (skipped here, and it prints that it skipped). |
| 11 | Done. `list_packages` pages until a short/empty page and **refuses** at ten full pages; the case serves 100 rows forever and asserts exit 1 with zero deletes. |
| 12 | Done. `release-stamp` prints all four fields, so `packages.yml` no longer writes `deb_release=`/`rpm_release=1` by hand. |

## Evidence (run 2)

```
$ make fast-check workflow-check packaging-gates-test        # gate exit=0
workflow-check: rule 20 self-test ok — a distribution written in a package workflow is a second copy of the table
workflow-check: rule 21 self-test ok — the two package graphs must not drift
workflow-check: ok — 674 checks over 9 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 108 cases passed (workflow=19, matrix=15, wheel-tag=4, exec-env=12, test-seam=7, portable-elf=2, max-glibc=4, repack=13, matrix-producer=2, package-workflow=11, publish=19)
163 × `test result: ok` (Rust), plan OK, install-test all cases passed
```

```
$ uv run --no-project scripts/test_publish_gates.py
publish-gates-test: ok — 19 cases passed (stamps, upload, mirror, retention)
```

```
$ uv run --no-project scripts/publish_packages.py stamp --format both --run 7 --sha 3f9a1c2d4e5f6a7b
deb_version=0.2.0~nightly20261120.r7+g3f9a1c2
deb_release=
rpm_version=0.2.0
rpm_release=0.nightly.20261120.r7.g3f9a1c2

$ uv run --no-project scripts/publish_packages.py release-stamp
deb_version=0.2.0
deb_release=
rpm_version=0.2.0
rpm_release=1

$ uv run --no-project scripts/publish_packages.py push --format deb --dry-run --file …/blastit_0.2.0_amd64.deb --component main
cloudsmith: would POST https://upload.cloudsmith.io/offworld-nexus/apt/deb/ (Authorization: Bearer <token>) multipart package_file=@…/blastit_0.2.0_amd64.deb distribution=any-distro release=any-version component=main republish=true
cloudsmith: equivalent CLI: cloudsmith push deb offworld-nexus/apt/any-distro/any-version …/blastit_0.2.0_amd64.deb --component main --republish

$ FORGEJO_PACKAGES_TOKEN= uv run --no-project scripts/publish_packages.py forgejo --format deb --file … --component main
::notice::forgejo mirror skipped: FORGEJO_PACKAGES_TOKEN is unset, so nothing is mirrored (the Cloudsmith repositories are the install path)
exit=0
```

Negative, on a throwaway copy (reverted by re-copying the file):

```
# literal leg:                 FAIL … the literal leg 'debian-13-x86_64' …        exit=1
# graph drift (one-sided edit): FAIL … disagree on the shared job 'packages' …   exit=1
# reverted:                     workflow-check: ok — 674 checks … 0 problem(s)    exit=0
```

## Still open (not this node)

- The `nightly` component must exist on both Cloudsmith repositories before the
  first scheduled run (operator step), and `docs/install.md` still owes the
  repository/floor prose (`MB.42.4`).
- `rpmdev-vercmp` was not run here (no RPM on this host); the release-order
  assertion therefore rests on `dpkg` plus the design's `-0.` prefix, and the case
  prints its own skip so nobody reads a silent pass.

