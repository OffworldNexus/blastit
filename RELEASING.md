# Releasing blastit

Releases are **pull-request driven**: autonomous work happens on a `release/<version>`
branch, the conductor opens a pull request into `develop` when a release gate
passes, and **a human merging that PR into `develop` validates everything on
`develop` and triggers the entire release pipeline automatically** — PyPI upload,
annotated `v<version>` git tag, GitHub Release, and GitHub Pages docs deployment.

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
release corresponds to git tag `v<version>` (for example, `0.1.1` → `v0.1.1`).

1. **Prepared automatically on `release/<version>`**:
   * `Cargo.toml` (`[workspace.package] version`) is set to `<version>`.
   * [`CHANGELOG.md`](CHANGELOG.md) has a dated `## <version> (YYYY-MM-DD)`
     heading — the release notes are that entry verbatim, and `scripts/release-notes.sh --require-dated`
     refuses `(unreleased)` or empty entries.
   * The autonomous conductor pushes `release/<version>` and opens a pull request
     into `develop`.
2. **Pre-merge rehearsal on the PR**:
   * `CI` (`.github/workflows/ci.yml`), `Docs` (`.github/workflows/docs.yml`),
     and `Publish` (`.github/workflows/publish-pypi.yml`) all run on the PR.
   * `Publish`'s `guard` job checks that the branch matches `release/<version>`,
     the changelog entry is dated, and tag `v<version>` does not exist yet; then
     `wheels` (CPython 3.13 & 3.14), `sdist`, and `verify` (`scripts/check-long-description.py`)
     rehearse the full distribution build.
3. **Human validation & merge**:
   * Review the PR on GitHub and click **Squash and merge** into `develop`.
   * Merging into `develop` automatically runs `CI`, `Docs` (deploying GitHub
     Pages), `Live (DigitalOcean)` (App Platform cloud e2e), and `Publish`.
   * On `develop`, `Publish` detects that `v<version>` is not yet tagged, builds
     and verifies the wheels and sdist with README links pinned to `v<version>`,
     waits in `validate` until `CI`, `Docs`, and `Live (DigitalOcean)` on that
     exact `develop` commit are all green, uploads to **PyPI**, creates and
     pushes the annotated `v<version>` git tag, and creates the **GitHub Release**
     with artifacts and `CHANGELOG.md` notes attached.
   * Ordinary non-release PRs merged into `develop` (where `v<version>` already
     exists) skip the release jobs in a few seconds.
4. **Check the result**:

   ```console
   $ uv venv /tmp/blastit-check && VIRTUAL_ENV=/tmp/blastit-check uv pip install blastit==0.1.1
   $ /tmp/blastit-check/bin/blastit --help
   ```

   and the [project page](https://pypi.org/project/blastit/), whose description
   is the README (with its links rewritten to absolute ones pinned to `v<version>`,
   because PyPI renders it without a repository around it).

*(Manual fallback: pushing an annotated `v<version>` tag directly still triggers
the same `Publish` workflow idempotently if ever needed.)*

## When it goes wrong

* **Branch/version mismatch or undated changelog**: caught on the `release/<version>`
  PR by `guard` before merge; push a fix to `release/<version>`.
* **`CI`, `Docs`, or `Live (DigitalOcean)` fails after merge onto `develop`**:
  `validate` stops `Publish` before PyPI or the `v<version>` tag is touched. If
  the failure was transient, re-run the failed workflow and then re-run `Publish`;
  if it needs a code fix, merge a follow-up PR into `develop` (since `v<version>`
  was never created, the fixed `develop` commit will automatically release `<version>`).
* **The upload failed partway**: re-run the `pypi` job. PyPI never accepts the
  same file twice, and `skip-existing` leaves the files that made it where they
  are, then `release` tags and creates the GitHub release.
* **A wrong artifact is on PyPI**: a version number cannot be reused, and a file
  cannot be replaced. Yank the release on PyPI (*Manage → Releases → Yank*),
  which stops new installs of it from resolving, then ship the next patch
  version via a new `release/<next>` PR.

## The development wheels are not this

Every commit to `develop` builds the same wheels (`.github/workflows/wheels.yml`)
and leaves them as artifacts of that run, with a `0.1.0.dev<run>` version so
they can't be mistaken for the release of the same commit. Nothing publishes
them, and nothing outside the run can install from them — they exist to catch a
broken wheel on the commit that broke it, and for whoever wants to try the
current tree. The two workflows build with the same action,
`.github/actions/build-wheel`.

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

## Only x86_64, for now

The release matrix is CPython 3.13 and 3.14 on x86_64. arm64 is not a matrix
dimension waiting to be switched on, it is a step of its own:

* **it does not compile.** `blastit-sys` uses `libc::SYS_fchmodat2`, which libc
  does not define for aarch64 (the log is run 40388835113 of `Publish`), so
  there is no arm64 wheel *and* no arm64 source build.
* **and that is not the whole of it.** §7.3's seccomp filters, and the arch
  check they are built under, are written for x86_64 and have never run on arm64
  at all.

So arm64 means fixing the syscall numbers, reviewing the filters against the
arm64 ABI, and adding a job on `ubuntu-24.04-arm` that builds *and* runs the
tests there — a step with its own test run, not a one-line change.

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
