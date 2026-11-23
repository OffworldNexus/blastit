# Releasing blastit

Releases are **pull-request driven**: autonomous work happens on a `release/<version>`
branch, the conductor opens a pull request into `develop` when a release gate
passes, and **a human merging that PR into `develop` validates everything on
`develop` and triggers the entire release pipeline automatically** — PyPI upload,
annotated `v<version>` git tag, GitHub Release, GitHub Pages docs deployment, and
the signed `apt`/`rpm` packages.

The workflow is [`.github/workflows/publish-pypi.yml`](.github/workflows/publish-pypi.yml),
and it publishes through **trusted publishing**: PyPI checks a GitHub OIDC token,
so there is no API token in this repository and none in anyone's password manager.

## Once, per index

Both registrations are "pending publishers" until the first upload creates the
project; after that they are the project's own publishing settings.

1. **PyPI** — [pypi.org/manage/account/publishing](https://pypi.org/manage/account/publishing/)
   → *Add a pending publisher* → GitHub:

   | Field | Value |
   |---|---|
   | PyPI project name | `blastit` |
   | Owner | `OffworldNexus` |
   | Repository name | `blastit` |
   | Workflow name | `publish-pypi.yml` |
   | Environment name | `pypi` |

2. **TestPyPI** — [test.pypi.org/manage/account/publishing](https://test.pypi.org/manage/account/publishing/)
   → the same, with environment `testpypi`. TestPyPI is a separate index with
   its own accounts; registering there does not create the project on PyPI.

3. **GitHub environments** — *Settings → Environments* in this repository:

   * `pypi`: under *Deployment branches and tags*, allow **Selected branches and
     tags** with the branch rule `develop` and the tag rule `v*` (optional
     required reviewers may also be configured if you want a second click after
     merging the PR).
   * `testpypi`: no reviewers, and either no restriction or `Selected branches
     and tags` limited to the branch you rehearse from. It holds throwaway
     versions.

Renaming the workflow file, or the environment, means updating both publishers
on the two indices: PyPI matches the OIDC token's workflow and environment
against what is written there, and nothing else.

## Rehearse: TestPyPI

*Actions → Publish → Run workflow*, on `develop` (or a `release/*` branch), with
**`dry-run` unchecked**. The run builds and verifies everything and uploads the
same files to TestPyPI.

```console
$ uv venv /tmp/rehearsal && VIRTUAL_ENV=/tmp/rehearsal uv pip install \
      --index-url https://test.pypi.org/simple/ blastit
```

`--index-url` rather than `--extra-index-url`: TestPyPI is the only index that
has this version, and blastit has no dependencies to fetch. Re-running the
workflow is safe — `skip-existing` means the files already there are left alone.

With `dry-run` left checked, the same run builds and verifies everything and
stops before any upload: what a pull request gets.

A dispatch cannot reach the real PyPI: `pypi` and `release` require
`github.event_name == 'push'`, so the only thing a manual run can upload to is
TestPyPI — and only with `dry-run` unchecked.

## Release (PR merge → automatic publish)

The version lives in `Cargo.toml` (`[workspace.package] version`), and each
release corresponds to git tag `v<version>` (for example, `0.2.0` → `v0.2.0`).

1. **Prepared automatically on `release/<version>`**:
   * `Cargo.toml` (`[workspace.package] version`) is set to `<version>`.
   * [`CHANGELOG.md`](CHANGELOG.md) has a dated `## <version> (YYYY-MM-DD)`
     heading — the release notes are that entry verbatim, and `scripts/release-notes.sh --require-dated`
     refuses `(unreleased)` or empty entries.
   * The autonomous conductor pushes `release/<version>` and opens a pull request
     into `develop`.
2. **Pre-merge rehearsal on the PR**:
   * `CI` (`.github/workflows/ci.yml`), `Docs` (`.github/workflows/docs.yml`),
     `PHP SAPI` (`.github/workflows/php-sapi.yml`), `Python SAPI`
     (`.github/workflows/py-sapi.yml`), and `Publish`
     (`.github/workflows/publish-pypi.yml`) run on the PR.
   * `Publish`'s `guard` and `matrix` jobs check that the branch matches
     `release/<version>`, the changelog entry is dated, tag `v<version>` does
     not exist yet, and every leg comes from `scripts/platform_matrix.py`; then
     `wheels_gnu` + `wheels_musl` (`4` universal
     `py3-none-{manylinux_2_28,musllinux_1_2}_{x86_64,aarch64}` wheels,
     `manylinux` wheels smoke-tested on the matrix's CPython smoke images
     (`--matrix wheels`), `musllinux` on its own, each carrying the PHP shims of its own
     `<arch>`/`<libc>` beside the CPython ones — a short shim leg fails its own
     leg and skips the wheels rather than publishing a wheel without PHP — built
     by `scripts/build-wheel.sh`
     (`maturin build --bindings bin` + `scripts/repack_universal_wheel.py` — no
     extension module compiled any more), `sdist`, `shims`
     (every `--matrix shims` leg: `packaging/languages.tsv`'s PHP minors × arch ×
     libc), `binaries_gnu` + `binaries_musl`
     (`4` standalone Linux archives), `lsp` (`5` `blastit-lsp` archives across
     Linux, macOS, and Windows), and `verify` (`scripts/check-long-description.py`)
     rehearse the full distribution build.
3. **Human validation & merge**:
   * Review the PR on GitHub and click **Squash and merge** into `develop`.
   * Merging into `develop` automatically runs `CI`, `Docs` (deploying GitHub
     Pages), and `Publish`.
   * On `develop`, `Publish` detects that `v<version>` is not yet tagged, builds
     and verifies the wheels, `sdist`, standalone archives, PHP shims, and
     `blastit-lsp` archives with README links pinned to `v<version>`, waits in
     `validate` until `CI` and `Docs` on that exact `develop` commit are green,
     uploads the wheels and `sdist` to **PyPI**, creates and pushes the annotated
     `v<version>` git tag, and creates the **GitHub Release** with all archives,
     shims, `SHA256SUMS`, and `CHANGELOG.md` notes attached.
   * `Publish` then **dispatches Packages explicitly** — on the `develop` merge
     path only. GitHub documents that
     "events created with the repository's `GITHUB_TOKEN` do not start new
     workflow runs, with the exception of `workflow_dispatch` and
     `repository_dispatch`" — so the tag `Publish` pushes is invisible to
     `on: push: tags`, and the workflow has to ask for the run:

     ```yaml
     permissions:
       contents: read
       actions: write
     # ...
     - run: gh workflow run packages.yml --ref "$RELEASE_TAG"
       env:
         GH_TOKEN: ${{ github.token }}
     ```

     That documented exception is why `0.2.0`'s packages never built: the tag was
     pushed by `GITHUB_TOKEN`, so no `push` event started `Packages`.
   * **Packages** (`.github/workflows/packages.yml`) builds **28 package files** —
     seven names × two architectures in each of two formats (`7` × `{amd64, arm64}`
     `.deb`, `7` × `{x86_64, aarch64}` `.rpm`) — install-verifies each set in its
     own distribution container, runs `packaging/stage-dist.py`, and uploads exactly
     one artifact named `offworld-dist`.
   * Nothing needs a shared concurrency group: `Publish` runs on
     `refs/heads/develop` (group `release-refs/heads/develop`), while the dispatched
     `Packages` run has `github.ref = refs/tags/v<version>` and therefore the tag's
     own group. What serialises them is ordering, not a lock — `Publish`'s `release`
     job issues the dispatch as its **last** act and then asserts that a
     `workflow_dispatch` run exists for that tag, and `Packages` keeps
     `cancel-in-progress: false`.
   * `rehearsal: true` is the only safe way to exercise this path:
     `gh workflow run packages.yml --ref v<version> -f rehearsal=true` runs the full
     build and verify matrix plus `stage-dist.py --dry-run` and uploads no
     `offworld-dist`, so the App never sees it.
   * The **`offworld-nexus-packages` GitHub App** signs that artifact and publishes
     it to `apt.offworld-nexus.app` and `rpm.offworld-nexus.app`, enforcing
     `release_set` retention — every stable release kept forever, the newest seven
     nightlies — and posts an `offworld-pkg / publish (stable)` check run. No
     token, key or secret lives in Actions: the App holds the signing key, and the
     workflow only hands it the bytes.
   * **Nightly** (`.github/workflows/nightly.yml`, cron `41 3 * * *`) stamps
     `0.2.1~nightly<YYYYMMDD>.r<run>+g<sha7>` (`.deb`) and
     `0.2.1-0.nightly.<YYYYMMDD>.r<run>.g<sha7>` (`.rpm`) into the `nightly`
     component and keeps nothing locally; the server retains the newest seven.
   * Ordinary non-release PRs merged into `develop` (where `v<version>` already
     exists) skip the release jobs in a few seconds.
4. **Check the result**:

   ```console
   $ uv venv /tmp/blastit-check && VIRTUAL_ENV=/tmp/blastit-check uv pip install blastit==0.2.1
   $ /tmp/blastit-check/bin/blastit --help
   ```

   and the [project page](https://pypi.org/project/blastit/), whose description
   is the README (with its links rewritten to absolute ones pinned to `v<version>`,
   because PyPI renders it without a repository around it).

*(Manual fallback: pushing an annotated `v<version>` tag by hand still releases
idempotently — the human-pushed tag is not a `GITHUB_TOKEN` event, so `Packages`
starts from its own `on: push: tags` and `Publish`'s dispatch step (gated to
`develop`) stays out of the way, rather than building the tag twice.)*

## The package names

Every architecture gets the same names, so a host installs `blastit` plus the shim(s) it
needs. [`packaging/languages.tsv`](packaging/languages.tsv) is the one place a name is
added; [`packaging/package-set.sh`](packaging/package-set.sh) derives the full set from it:

| Package | Payload | Hard dependency |
|---|---|---|
| `blastit` | `blastit`, `blastit-lsp`, `LICENSE`, the pure-Python modules, `/usr/bin` links | measured `libc6` floor |
| `blastit-php-8.4` — worked example; one such package per `languages.tsv` row | that `libphp`'s PHP shim | `blastit (= <version>)`, measured `libc6`; recommends `libphp8.4-embed` |

The version sits in the name so `apt-get install blastit-php-8.4` reads as "Blastit's
PHP 8.4 shim", and every shim pins the exact core version it was built against. A minor
the distribution does not ship comes from a signed third-party repository (Ondřej Surý's
on Debian and Ubuntu).

## When it goes wrong

* **Branch/version mismatch or undated changelog**: caught on the `release/<version>`
  PR by `guard` before merge; push a fix to `release/<version>`.
* **`CI` or `Docs` fails after merge onto `develop`**:
  `validate` stops `Publish` before PyPI or the `v<version>` tag is touched. If
  the failure was transient, re-run the failed workflow and then re-run `Publish`;
  if it needs a code fix, merge a follow-up PR into `develop` (since `v<version>`
  was never created, the fixed `develop` commit will automatically release `<version>`).
* **The tag exists but `Packages` never ran**: the dispatch step in `Publish` is
  what starts it. Re-run it by hand and watch the App's
  `offworld-pkg / publish (stable)` check run:
  `gh workflow run packages.yml --ref v<version>`.
* **The upload failed partway**: re-run the `pypi` job. PyPI never accepts the
  same file twice, and `skip-existing` leaves the files that made it where they
  are, then `release` tags and creates the GitHub release.
* **A wrong artifact is on PyPI**: a version number cannot be reused, and a file
  cannot be replaced. Yank the release on PyPI (*Manage → Releases → Yank*),
  which stops new installs of it from resolving, then ship the next patch
  version via a new `release/<next>` PR.

## The development wheels are not this

Every commit to `develop` builds the same wheels (`.github/workflows/wheels.yml`)
and leaves them as artifacts of that run, with a `<version>-dev<run>` version
(for example, `0.2.0-dev<run>`) so they can't be mistaken for the release of the
same commit. Nothing publishes
them, and nothing outside the run can install from them — they exist to catch a
broken wheel on the commit that broke it, and for whoever wants to try the
current tree. The two workflows build with the same action,
`.github/actions/build-wheel`. A `develop` wheel carries the two CPython shims
only (`require-shims: py`) — that workflow has no shim legs — so PHP is a
release-wheel capability: only `publish-pypi.yml`'s wheels carry the four PHP
shims, and its `verify` job proves it on every libc before anything reaches PyPI.

## The description PyPI shows

`verify` runs `scripts/check-long-description.py` on the wheels and the sdist:
it checks the metadata a release needs, and it renders the description with the
renderer **PyPI itself uses** — `readme_renderer` with `comrak` (GFM) and `nh3`
for the tag allowlist — then fails if anything in the result is relative, or if
the README banner does not come from `raw.githubusercontent.com`.

It is not `twine check`, deliberately: twine does not render a Markdown
description at all (`twine.commands.check._RENDERERS["text/markdown"]` is
`None`, "rendering cannot fail"), so it cannot see a description that renders to
nothing or an image PyPI cannot fetch. To run it by hand:

```console
$ uv run --no-project scripts/check-long-description.py dist/*
```

## Multi-platform matrix and CI-only legs

`scripts/platform_matrix.py` is the single source of truth for the release
artifacts (`--matrix`/`--count`/`--leg` are what the workflows consume, and
`--check` validates the matrix itself):

* **Linux serving artifacts** (the `4` universal `blastit` wheels, `4` standalone
  archives, one PHP C SAPI shim per `packaging/languages.tsv` row, and the CPython
  shims the same file declares) target `{x86_64, aarch64} × {gnu
  (manylinux_2_28), musl (musllinux_1_2 / Alpine)}`. Every `x86_64` wheel,
  standalone archive, and shim is built and smoked locally on Linux `x86_64`;
  the `aarch64` legs build and run natively in GitHub Actions on
  `ubuntu-24.04-arm`.
* **Portable `blastit-lsp` editor archives** target five native triples:
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
  `aarch64-apple-darwin` (`macos-latest`), `x86_64-apple-darwin`
  (`macos-15-intel`), and `x86_64-pc-windows-msvc` (`windows-latest`). Only
  `x86_64-unknown-linux-gnu` is built and session-smoked on a Linux `x86_64`
  workstation; the `aarch64` Linux, macOS, and Windows legs run on native GitHub
  Actions runners (`ci.yml` and `publish-pypi.yml`) when the PR opens and
  merges.

## What floats

Every action is pinned to a major version (`actions/checkout@v4`,
`PyO3/maturin-action@v1`, …), which follows upstream's fixes without a diff per
release. Three of them move more than a version number does:

* `dtolnay/rust-toolchain@stable` and `pypa/gh-action-pypi-publish@release/v1`
  are *branches*: the first installs whatever stable Rust is that day (the
  workspace's MSRV is checked separately, in `ci.yml`), the second is what the
  publishing action's own documentation tells you to use;
* `PyO3/maturin-action@v1` is pinned, but the maturin it installs is the latest
  release by default, so the *build tool* floats inside it;
* `astral-sh/setup-uv` is pinned to an exact release (`v10.3.2`) rather than a
  major, because it stopped publishing major tags after `v7`.

Nothing is pinned by commit SHA. That would be stricter — a tag can be moved —
and is the right next step if this repository ever gets a bot that moves the
pins.
