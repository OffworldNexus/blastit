# Native packages (`packaging/`)

Blastit ships **seven** binary packages per architecture instead of one archive
that carries every interpreter shim, because the shim a host can load is the
interpreter that host has. The names, their payload and their dependency live in
one file — [`package-set.sh`](package-set.sh) — and the `.deb`/`.rpm` builders,
the spec's payload lists and the set check all read it.

| Package | Payload under `/usr/share/blastit/` | Debian `Depends:` | RPM weak dep |
| --- | --- | --- | --- |
| `blastit` | `lib/{blastit,blastit-lsp}`, `LICENSE`, `python/blastit/*.py`, `/usr/bin` symlinks | `libc6 (>= measured)` (`+ libgcc-s1` when measured) | — |
| `blastit-python-3.13` | `lib/libblastit_py313-<arch>-gnu.so` | `python3.13`, `blastit (= <version>)`, `libc6 (>= its floor)` | `Recommends: python3.13` |
| `blastit-python-3.14` | `lib/libblastit_py314-<arch>-gnu.so` | `python3.14`, `blastit (= <version>)`, `libc6 (>= its floor)` | `Recommends: python3.14` |
| `blastit-php-8.3` | `lib/libblastit_php83-<arch>-gnu.so` | `libphp8.3-embed`, `blastit (= <version>)`, `libc6 (>= its floor)` | `Suggests: php-embedded` |
| `blastit-php-8.4` | `lib/libblastit_php84-<arch>-gnu.so` | `libphp8.4-embed`, `blastit (= <version>)`, `libc6 (>= its floor)` | `Suggests: php-embedded` |
| `blastit-php-8.5` | `lib/libblastit_php85-<arch>-gnu.so` | `libphp8.5-embed`, `blastit (= <version>)`, `libc6 (>= its floor)` | `Suggests: php-embedded` |
| `blastit-php-8.6` | `lib/libblastit_php86-<arch>-gnu.so` | `libphp8.6-embed`, `blastit (= <version>)`, `libc6 (>= its floor)` | `Suggests: php-embedded` |

`blastit` is the interpreter-free server: it `dlopen`s a shim, never links
`libpython`/`libphp`. A shim package is useless without `blastit` (hence the hard
`= <version>` dependency) and `blastit` serves static routes and `check` without
any shim, so the split costs a host nothing it does not use.

## Why the version is (only) in the dependency, not the name

The interpreter minor is part of the **package name**, not of a version suffix:
`blastit-php-8.4` upgrades like any other package and can be installed beside
`blastit-php-8.3` (two PHP minors on one host is normal — Debian's `trixie` is
8.4, Ubuntu `noble` is 8.3). A `blastit-8.4` versioned package would instead
look like a second Blastit and could never coexist. Debian file names follow the
name, so they read `blastit-php-8.4_0.2.1_amd64.deb` and
`blastit-php-8.4-0.2.1-1.x86_64.rpm`.

## Repositories and channels

Both formats are served by the organization's signed repositories and nothing
else (no package registry, no publishing secret in CI):

- Debian/Ubuntu — `https://apt.offworld-nexus.app`, suite `any-distro`,
  component `main` for stable, `nightly` for nightly builds.
- Fedora/RHEL/Rocky/Alma — `https://rpm.offworld-nexus.app/stable/$basearch/`,
  or `/nightly/$basearch/`.

Nightly stamps sort strictly below the release they lead up to, so a stable
`apt-get upgrade` never picks a nightly and an RPM upgrade nightly→stable is an
upgrade in both directions:

| Format | Stable | Nightly |
| --- | --- | --- |
| `.deb` | `Version: 0.2.1` | `Version: 0.2.1~nightly20261122.r42+g90cb2db` |
| `.rpm` | `Version: 0.2.1`, `Release: 1` | `Version: 0.2.1`, `Release: 0.nightly.20261122.r42.g90cb2db` |

`package-stamp.sh` prints both fields for either format; the workflows call it
instead of spelling the grammar twice, and the key names go straight into
`$GITHUB_OUTPUT`:

```sh
packaging/package-stamp.sh --base 0.2.1 --channel stable --format deb
# deb_version=0.2.1
# deb_release=
packaging/package-stamp.sh --base 0.2.1 --channel nightly \
    --run 42 --sha 90cb2db1234 --format both
# deb_version=0.2.1~nightly20261122.r42+g90cb2db
# deb_release=
# rpm_version=0.2.1
# rpm_release=0.nightly.20261122.r42.g90cb2db
```

`--run`/`--sha` are required only for `nightly`; a stable stamp is the workspace
version and needs only `--base`.

## The RPM interpreter dependency is deliberately weak

Debian's archive names one interpreter package per minor, so `Depends:
python3.13` / `libphp8.4-embed` is satisfiable and hard. RPM distributions do
not: **Rocky 9 has no `python3.13`** (it ships 3.9) and its `php-embedded` is
**8.0**. A hard `Requires:` there would make the shim package uninstallable on a
distribution the release still supports, so the RPM subpackages declare
`Recommends: python3.13`/`3.14` and `Suggests: php-embedded` and require only
`blastit = <version>`. Install the matching interpreter first — that is what
`docs/install.md` and `docs/install.sh` do.

## `stage-dist.py`

`stage-dist.py` is a **verbatim copy** of the `offworld-packaging` skill's
`scripts/stage_dist.py` (organization-wide, single owner); it validates the
built artifacts, writes `manifest.json` and lays out the `offworld-dist` tree the
release pipeline signs. Never hand-roll the manifest: re-copy the skill's file
when it changes and prove the copy with
`cmp <skill>/scripts/stage_dist.py packaging/stage-dist.py`.

## Local build

```sh
make install PREFIX=/tmp/opencode/bundle
make packages                       # or:
scripts/build-deb.sh --version 0.2.1 --bundle /tmp/opencode/bundle --out dist
scripts/build-rpm.sh --version 0.2.1 --bundle /tmp/opencode/bundle --out dist
scripts/check-bundle-layout.sh --deb-set dist --arch amd64
scripts/check-bundle-layout.sh --rpm-set dist --arch x86_64
packaging/stage-dist.py --dry-run --source blastit --channel stable \
    --version 0.2.1 --ref refs/tags/v0.2.1 \
    --file 'deb:amd64:dist/*_amd64.deb' --file 'rpm:x86_64:dist/*.x86_64.rpm'
```
