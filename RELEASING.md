# Releasing blastit

Whoever tags a release, this is the whole of it: what to set up once, how to
rehearse, and how to publish 0.1.0. The workflow is
[`.github/workflows/publish-pypi.yml`](.github/workflows/publish-pypi.yml), and
it publishes through **trusted publishing**: PyPI checks a GitHub OIDC token, so
there is no API token in this repository and none in anyone's password manager.

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

   * `pypi`: add **required reviewers** (yourself is enough). The upload job
     waits for an approval, which is the last gate before a version number is
     spent forever. Under *Deployment branches and tags*, leave only
     **Selected branches and tags** with the tag rule `v*`.
   * `testpypi`: no reviewers, and either no restriction or `Selected branches
     and tags` limited to the branch you rehearse from. It holds throwaway
     versions.

Renaming the workflow file, or the environment, means updating both publishers
on the two indices: PyPI matches the OIDC token's workflow and environment
against what is written there, and nothing else.

## Rehearse: TestPyPI

*Actions → Publish → Run workflow*, on `develop`, with **`dry-run` unchecked**.
The run does everything a release does — the tag checks are skipped, since there
is no tag — and uploads the same files to TestPyPI.

```console
$ uv venv /tmp/rehearsal && VIRTUAL_ENV=/tmp/rehearsal uv pip install \
      --index-url https://test.pypi.org/simple/ blastit
```

`--index-url` rather than `--extra-index-url`: TestPyPI is the only index that
has this version, and blastit has no dependencies to fetch. Re-running the
workflow is safe — `skip-existing` means the files already there are left alone.

With `dry-run` left checked, the same run builds and verifies everything and
stops before any upload: what a pull request gets.

## Release

The version is in `Cargo.toml` (`[workspace.package] version`), and the tag is
`v` plus it, so 0.1.0 is `v0.1.0`. Nothing stamps the version for a release: the
tag is the release.

1. **Describe the release**: in [`CHANGELOG.md`](CHANGELOG.md), turn
   `## 0.1.0 (unreleased)` into `## 0.1.0 (2026-10-26)` — today's date. The
   release notes are that entry, verbatim.
2. **Land it on `develop`** as a pull request with green CI, like anything else.
3. **Tag and push**:

   ```console
   $ git switch develop && git pull
   $ git tag -a v0.1.0 -m "blastit 0.1.0"
   $ git push origin v0.1.0
   ```

4. **Watch it**: `gh run watch`, or *Actions → Publish*. In order: the tag is
   checked against `Cargo.toml` and the changelog (the `guard` job), the wheels
   are built for CPython 3.13 and 3.14 on x86_64, the sdist is built and
   installed from, everything goes through `twine check --strict`, and then the
   `pypi` job waits for your approval in the environment before it uploads. The
   last job creates the GitHub release, with the artifacts attached and the
   changelog entry as its notes.
5. **Clean up, once**: the development wheels used to be published on a rolling
   `wheels` GitHub release, and it is not a release at all. Once 0.1.0 is on
   PyPI, delete it:

   ```console
   $ gh release delete wheels --cleanup-tag
   ```

   Do it once, and not before: until 0.1.0 is up, that release is the only place
   those older development wheels can be found.
6. **Check the result**:

   ```console
   $ uv venv /tmp/blastit-0.1.0 && VIRTUAL_ENV=/tmp/blastit-0.1.0 uv pip install blastit==0.1.0
   $ /tmp/blastit-0.1.0/bin/blastit --help
   ```

   and the [project page](https://pypi.org/project/blastit/), whose description
   is the README (with its links rewritten to absolute ones, because PyPI
   renders it without a repository around it).

## When it goes wrong

* **The tag doesn't match `Cargo.toml`**: the `guard` job says so and nothing is
  built. Delete the tag (`git push origin :v0.1.1`), decide which of the two is
  right, and tag again.
* **A build or `twine check` fails**: nothing has been uploaded. Fix it on
  `develop`, and either move the tag to the new commit
  (`git push origin :v0.1.0 && git tag -a v0.1.0 && git push origin v0.1.0`) or
  re-run the workflow from the tag.
* **The upload failed partway**: re-run the `pypi` job. PyPI never accepts the
  same file twice, and `skip-existing` leaves the files that made it where they
  are.
* **A wrong artifact is on PyPI**: a version number cannot be reused, and a file
  cannot be replaced. Yank the release on PyPI (*Manage → Releases → Yank*),
  which stops new installs of it from resolving, then release the next patch
  version. This is what the `pypi` environment's required reviewers are for.

## The development wheels are not this

Every commit to `develop` builds the same wheels (`.github/workflows/release.yml`)
and leaves them as artifacts of that run, with a `0.1.0.dev<run>` version so
they can't be mistaken for the release of the same commit. Nothing publishes
them, and nothing outside the run can install from them — they exist to catch a
broken wheel on the commit that broke it, and for whoever wants to try the
current tree. The two workflows build with the same action,
`.github/actions/build-wheel`.

## Only x86_64, for now

aarch64 does not compile: `blastit-sys` uses `libc::SYS_fchmodat2`, which libc
does not define for aarch64 (the log is in run 40388835113 of `Publish`). That
is a code fix in `crates/blastit-sys`, not a packaging one, so the release
matrix is CPython 3.13 and 3.14 on x86_64 until it lands. Adding it back is one
matrix dimension in `.github/workflows/publish-pypi.yml` on
`ubuntu-24.04-arm`.
