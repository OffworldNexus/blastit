# Native packages (`packaging/`)

Blastit ships the interpreter-free core `blastit` plus **one shim package per row of
[`languages.tsv`](languages.tsv)** — the one file a version is added to. The package
names, their payload and their dependency are declared once in
[`package-set.sh`](package-set.sh), which derives every row from that table; the
`.deb`/`.rpm` builders, the generated RPM spec and the set check all read it.

| Package | Payload under `/usr/share/blastit/` | Hard dependency | Weak dependency |
| --- | --- | --- | --- |
| `blastit` | `lib/{blastit,blastit-lsp}`, `LICENSE`, `python/blastit/*.py`, `/usr/bin` symlinks | measured `libc6` floor (`+ libgcc-s1` when measured) | — |
| `blastit-php-8.4` — a worked example; every [`languages.tsv`](languages.tsv) row is the same shape, one package per interpreter minor | `lib/libblastit_php84-<arch>-gnu.so` | `blastit (= <version>)`, `libc6 (>= its floor)` | `Recommends: libphp8.4-embed` (deb) / `php-embedded` (rpm) |

`blastit` is the interpreter-free server: it `dlopen`s a shim, never links
`libpython`/`libphp`. A shim package is useless without `blastit` (hence the hard
`= <version>` dependency) and `blastit` serves static routes and `check` without
any shim, so the split costs a host nothing it does not use.

## Why the version is (only) in the dependency, not the name

The interpreter minor is part of the **package name**, not of a version suffix:
`blastit-php-8.4` upgrades like any other package and can be installed beside the shim
of another minor (the table decides which). A `blastit-8.4` versioned package would
instead look like a second Blastit and could never coexist. Debian file names follow
the name, so they read `blastit-php-8.4_0.2.1_amd64.deb` and
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

Debian's archive names one interpreter package per minor, so a `Depends:` on the row's
runtime is satisfiable and hard. RPM distributions do not: Rocky 9 ships an older CPython
and an older `php-embedded` than the declared rows, so a hard `Requires:` there would make
the shim package uninstallable on a distribution the release still supports. The RPM
subpackages therefore declare each row's runtime as `Recommends:` and require only
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
