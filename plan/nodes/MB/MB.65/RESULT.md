# MB.65 — Offworld Nexus packaging pipeline + one package per interpreter minor

STATUS: done

## What changed

- `Cargo.toml` `[workspace.package] version = "0.2.1"`, every intra-workspace
  `version = "0.2.0"` requirement in `crates/*/Cargo.toml`, and `Cargo.lock`
  regenerated (`cargo metadata --format-version 1`). `scripts/dev-version.sh
  --check` passes.
- `CHANGELOG.md`: dated `## 0.2.1 (2026-11-22)` entry (packaging migration to the
  Offworld Nexus zero-secret pipeline, the seven-package split, the 0.2.0
  publication gap, the explicit `workflow_dispatch`).
- New `packaging/`:
  - `package-set.sh` — the table of the seven names, each shim glob and its
    interpreter dependency. `scripts/build-deb.sh`, `scripts/build-rpm.sh` and
    `scripts/check-bundle-layout.sh` **read** it; `scripts/rpm/blastit.spec` is
    **asserted against** it by `--check` (stanzas and `%files` globs), and
    `scripts/platform_matrix.py --check` diffs the matrix's `set` against
    `--names`/`--shims` (`e2e/run_packaging.py` runs `--shims` at test time and
    keeps no copy).
  - `package-stamp.sh` — `deb_version`/`deb_release`/`rpm_version`/`rpm_release`
    for `$GITHUB_OUTPUT`, `--format deb|rpm|both`; `--run`/`--sha` required only
    on the nightly channel.
  - `stage-dist.py` — byte-for-byte copy of the skill's `scripts/stage_dist.py`
    (`cmp` clean), `chmod +x`.
  - `README.md` — names/payload/dependency table, channel and stamp mapping, and
    the Rocky 9 `Recommends:`/`Suggests:` rationale.
- `scripts/build-deb.sh` — the seven `.deb`s per run, each with its own
  `md5sums`, `--root-owner-group`, `SOURCE_DATE_EPOCH` and **its own measured**
  floor; refuses an unowned or doubly-owned shim; `--out` resolved to an absolute
  path; `--help` exits 0.
- `scripts/rpm/blastit.spec` + `scripts/build-rpm.sh` — seven subpackages, one
  `glibc`/`libgcc` pair **measured per package** (`blastit_glibc_<tag>` /
  `blastit_libgcc_<tag>`), `AutoReq: no`/`AutoProv: no` per subpackage, weak
  interpreter dependencies, every produced `.rpm` copied into `--out`, stale
  same-version outputs removed first, `--out` resolved (docker `-v`), `--help`
  exits 0.
- `scripts/check-bundle-layout.sh` — PREFIX mode unchanged; `--deb-set`/`--rpm-set`
  unpack a whole set into one root and run the same §6.1 assertions on an
  absolute-resolved directory, with a dedicated sentence for a missing **core**
  and a count message that says *missing*, not "stray"; the scratch root is
  `mkdir -p`'d; `--help` exits 0.
- `scripts/build-package-bundle.sh` — comments only (feeds seven of each).

## Evidence

### Version, changelog, provenance

```
$ scripts/dev-version.sh --check
every intra-workspace requirement names `0.2.1`

$ scripts/release-notes.sh --require-dated v0.2.1 >/dev/null && echo notes ok
notes ok

$ cmp /home/remy/offworld-nexus/.opencode/skills/offworld-packaging/scripts/stage_dist.py \
      packaging/stage-dist.py && echo "cmp ok"
cmp ok
```

### Stamps (format-prefixed, straight into `$GITHUB_OUTPUT`)

```
$ packaging/package-stamp.sh --base 0.2.1 --channel stable --format deb
deb_version=0.2.1
deb_release=
$ packaging/package-stamp.sh --base 0.2.1 --channel stable --format rpm
rpm_version=0.2.1
rpm_release=1
$ packaging/package-stamp.sh --base 0.2.1 --channel nightly --run 42 --sha 90cb2db1234 --format both
deb_version=0.2.1~nightly20261122.r42+g90cb2db
deb_release=
rpm_version=0.2.1
rpm_release=0.nightly.20261122.r42.g90cb2db
```

### The seven PHP/CPython shims: built and probed

All four PHP minors and both CPython minors exist for `x86_64-gnu`; the three
that were missing were built **and probed** here (three egress cases each, a real
request answered, `exit()`/fatal contained):

| Minor | image | tail |
| --- | --- | --- |
| 8.3 | `blastit-php-sapi:8.3` (matrix `ubuntu:24.04` + `php8.3-dev`) | `ok php 8.3 abi 1: status 200, cpu_ns 14431174 <= wall_ns 16627198, exit()+fatal contained, next request served` |
| 8.5 | `blastit-php-sapi:8.5` (`ubuntu:26.04` + `php8.5-dev`) | `ok php 8.5 abi 1: status 200, cpu_ns 11680958 <= wall_ns 13183710, …` |
| 8.6 | `blastit-php-sapi:8.6` (`bookworm-slim` + Ondřej Surý) | `ok php 8.6 abi 1: status 200, cpu_ns 11836824 <= wall_ns 12668182, …` |

Collected with `scripts/collect-shim.sh … --minor 8.X --libc gnu --arch x86_64`
(`NEEDED libc.so.6; no RPATH/RUNPATH`); 8.4 and the CPython shims are what
`make install` builds/probes on the host. Bundle: `blastit 0.2.1`, 2 Python + 4
PHP shims (the musl CPython shim `make install` also copies is not part of a
`gnu` package set — `build-package-bundle.sh` never copies it).

### Seven `.deb`s and seven `.rpm`s, built into `target/packages`

```
build-deb: blastit -> Depends: libc6 (>= 2.34), libgcc-s1
build-deb: blastit-php-8.3 -> Depends: libphp8.3-embed, blastit (= 0.2.1), libc6 (>= 2.17)
build-deb: blastit-php-8.4 -> Depends: libphp8.4-embed, blastit (= 0.2.1), libc6 (>= 2.17)
build-deb: blastit-php-8.5 -> Depends: libphp8.5-embed, blastit (= 0.2.1), libc6 (>= 2.17)
build-deb: blastit-php-8.6 -> Depends: libphp8.6-embed, blastit (= 0.2.1), libc6 (>= 2.17)
build-deb: blastit-python-3.13 -> Depends: python3.13, blastit (= 0.2.1), libc6 (>= 2.34), libgcc-s1
build-deb: blastit-python-3.14 -> Depends: python3.14, blastit (= 0.2.1), libc6 (>= 2.34), libgcc-s1
build-deb: 7 package(s) in .../target/packages (4 python module(s))

build-rpm: blastit: measured glibc floor 2.34, libgcc 1
build-rpm: blastit-python-3.13: measured glibc floor 2.34, libgcc 1
build-rpm: blastit-python-3.14: measured glibc floor 2.34, libgcc 1
build-rpm: blastit-php-8.3: measured glibc floor 2.17, libgcc 0
build-rpm: blastit-php-8.4: measured glibc floor 2.17, libgcc 0
build-rpm: blastit-php-8.5: measured glibc floor 2.17, libgcc 0
build-rpm: blastit-php-8.6: measured glibc floor 2.17, libgcc 0
build-rpm: 7 package(s) in .../target/packages (6 shim(s), 4 python module(s))
```

Per-package `Requires:` from the RPM headers (`rockylinux:9`,
`rpm -qp --requires`, filtered to the two runtime names):

```
blastit-0.2.1-1                    glibc >= 2.34 libgcc
blastit-php-8.3-0.2.1-1            glibc >= 2.17
blastit-php-8.4-0.2.1-1            glibc >= 2.17
blastit-php-8.5-0.2.1-1            glibc >= 2.17
blastit-php-8.6-0.2.1-1            glibc >= 2.17
blastit-python-3.13-0.2.1-1        glibc >= 2.34 libgcc
blastit-python-3.14-0.2.1-1        glibc >= 2.34 libgcc
```

Each shim also carries `Requires: blastit = 0.2.1-1`, `Recommends: python3.13/3.14`
or `Suggests: php-embedded`, and zero generated `GLIBC_*` requirements (the core
included): `AutoReq: no`/`AutoProv: no` is restated in every subpackage.

### Set checks (relative directories) and the negative proofs

```
$ scripts/check-bundle-layout.sh --deb-set target/packages --arch amd64
ok   .../root/usr/share/blastit: bin/{blastit,blastit-lsp} -> lib/, 2 Python shims, 4 PHP shims, python/blastit/*.py
ok   .../root/usr/bin/blastit --version: blastit 0.2.1        deb rc=0

$ scripts/check-bundle-layout.sh --rpm-set target/packages --arch x86_64
check-bundle-layout: unpacking 7 RPM(s) with rpm2cpio in rockylinux:9
ok   .../root/usr/share/blastit: bin/{blastit,blastit-lsp} -> lib/, 2 Python shims, 4 PHP shims, python/blastit/*.py
ok   .../root/usr/bin/blastit --version: blastit 0.2.1        rpm rc=0

# one shim .deb and one shim .rpm removed from target/packages-neg
error: the deb set has no blastit-php-8.5 package (...): the release ships 7 and a set missing a shim would install a tree with no blastit-php-8.5 shim
error: .../target/packages-neg holds 6 deb package(s) for amd64; the release ships 7 (1 core + 2 Python + 4 PHP) and 1 is missing — see the package named above
neg deb rc=1
error: the rpm set has no blastit-python-3.14 package (...): the release ships 7 and a set missing a shim would install a tree with no blastit-python-3.14 shim
error: .../target/packages-neg holds 6 rpm package(s) for x86_64; the release ships 7 (1 core + 2 Python + 4 PHP) and 1 is missing — see the package named above
neg rpm rc=1

# only the core removed from target/packages-core-neg
error: the core package is missing from the deb set (.../blastit_[0-9]*_amd64.deb): run `make packages` for amd64, or point --deb-set at the build directory
core-neg rc=1
```

### The single source of truth is asserted at the gate

```
$ packaging/package-set.sh --check
package-set: ok (7 packages; scripts/rpm/blastit.spec's %package stanzas and %files globs match)

# with `%package -n blastit-php-8.5` renamed to blastit-php-9.9 in the spec
package-set: .../scripts/rpm/blastit.spec has no `%package -n blastit-php-8.5` stanza
package-set: .../scripts/rpm/blastit.spec declares `blastit-php-9.9`, which packaging/package-set.sh does not name
drift rc=1

$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared; …

# with `PACKAGE_FORMATS["deb"]["set"]` missing blastit-php-8.5 in memory
PACKAGE_FORMATS deb: names 6 packages, a set is the core + 2 CPython + 4 PHP shims
PACKAGE_FORMATS deb: no blastit-php-8.5 shim package
PACKAGE_FORMATS deb: `set` has drifted from packaging/package-set.sh --names (missing blastit-php-8.5)
```

`platform_matrix.py --check` now reads `packaging/package-set.sh --names/--shims`
and diffs `PACKAGE_FORMATS[deb|rpm]["set"]` against it, naming the package that
drifted; it also checks each shim package's table glob matches the shim filename
the matrix expects. `e2e/run_packaging.py` keeps no copy of the names (it runs
`--shims` at test time), so there is nothing there to diff.

### `--help` is success, bad arguments are usage errors

```
$ scripts/build-deb.sh --help >/dev/null; echo $?          # 0
$ scripts/build-rpm.sh --help >/dev/null; echo $?          # 0
$ scripts/check-bundle-layout.sh --help >/dev/null; echo $? # 0
$ scripts/build-deb.sh --bogus >/dev/null 2>&1; echo $?    # 2
$ scripts/build-deb.sh --help | head -2
usage: scripts/build-deb.sh --version <ver> --bundle <dir>
                            [--release <rel>] [--arch amd64|arm64]
```

`bash -n` passes on `packaging/package-set.sh`, `packaging/package-stamp.sh`,
`scripts/build-deb.sh`, `scripts/build-rpm.sh`,
`scripts/check-bundle-layout.sh` and `scripts/build-package-bundle.sh`, and the
retired name order appears nowhere:

```
$ grep -rnE 'php[0-9]\.[0-9]-blastit|python3\.[0-9]+-blastit' \
    packaging/ scripts/build-deb.sh scripts/build-rpm.sh \
    scripts/check-bundle-layout.sh scripts/rpm/blastit.spec CHANGELOG.md
(no output)
```

### `make fast-check` — everything green except one environment-dependent test

```
$ cargo fmt --all -- --check        → fmt clean
$ make -k fast-check               → rc=2
guard: OK
cargo clippy --workspace --all-targets --locked --features python -- -D warnings   → clean
uvx --offline ruff check python tools            → All checks passed!
uvx --offline ruff format --check python tools   → 12 files already formatted
uv run --no-project tools/plan.py validate       → plan OK
scripts/check-portable-elf.sh target/debug/blastit → ok
install-test: all cases passed (a)…(i)
rs-test: FAILED. 23 passed; 1 failed
```

The single failure is
`crates/blastit/tests/e2e_check_fmt.rs::learn_writes_one_grant_without_widening_a_view`
(not in this node's `paths`, unchanged):

```
assertion `left == right` failed: blastit check:
/tmp/opencode/blastit-e2e-dev-…/tmp/blastit-check-…/pg/.s.PGSQL.5432:
path must be shorter than SUN_LEN
```

`crates/blastit/tests/common/project.rs::scratch_root()` prefers `/tmp/opencode`
when it can be created, and this host's harness always creates it; that 14-byte
prefix pushes the fixture's Unix-socket path past the 108-byte `sun_path` limit.
The test passes with the same binary when the scratch root is `/tmp` (CI, where
`/tmp/opencode` does not exist):

```
$ unshare -rm bash -c 'mount -t tmpfs tmpfs /tmp && touch /tmp/opencode && \
    TMPDIR=/tmp cargo test -p blastit --test e2e_check_fmt \
    learn_writes_one_grant_without_widening_a_view'
test learn_writes_one_grant_without_widening_a_view ... ok
```

(A whole `make fast-check` in that namespace is not usable: another suite's
`a_binary_owned_by_another_uid_is_refused` needs a real `chown` to a foreign uid,
which a user namespace refuses. Neither failure touches this node.)

## Follow-ups (out of this slice's scope)

- `e2e/run_packaging.py`'s install legs still need to install the sibling shim
  packages of the image's interpreter from the new names; it now derives the
  names from `packaging/package-set.sh --shims`, and the install steps are the
  e2e slice's.
- The local proof bundle was built by `make install` (host `cargo build --release
  -p blastit`), not `scripts/build-package-bundle.sh`, because the manylinux
  bundle build is CI's job; it measures glibc 2.34, inside the matrix's
  `shim_floor` of 2.34.

## Lead verification — the final tree (amd64 set in `target/packages`)

Every command below was run by the lead on the finished tree, after the last fix
round; the outputs are quoted, not summarised.

### The fourteen files, and the seven names

```
$ ls target/packages | sort
blastit-0.2.1-1.x86_64.rpm
blastit_0.2.1_amd64.deb
blastit-php-8.3-0.2.1-1.x86_64.rpm      blastit-php-8.3_0.2.1_amd64.deb
blastit-php-8.4-0.2.1-1.x86_64.rpm      blastit-php-8.4_0.2.1_amd64.deb
blastit-php-8.5-0.2.1-1.x86_64.rpm      blastit-php-8.5_0.2.1_amd64.deb
blastit-php-8.6-0.2.1-1.x86_64.rpm      blastit-php-8.6_0.2.1_amd64.deb
blastit-python-3.13-0.2.1-1.x86_64.rpm  blastit-python-3.13_0.2.1_amd64.deb
blastit-python-3.14-0.2.1-1.x86_64.rpm  blastit-python-3.14_0.2.1_amd64.deb
```

```
$ for f in target/packages/*.deb; do dpkg-deb -f "$f" Package Depends; done
blastit                          libc6 (>= 2.34), libgcc-s1
blastit-php-8.3                  libphp8.3-embed, blastit (= 0.2.1), libc6 (>= 2.17)
blastit-php-8.4                  libphp8.4-embed, blastit (= 0.2.1), libc6 (>= 2.17)
blastit-php-8.5                  libphp8.5-embed, blastit (= 0.2.1), libc6 (>= 2.17)
blastit-php-8.6                  libphp8.6-embed, blastit (= 0.2.1), libc6 (>= 2.17)
blastit-python-3.13              python3.13, blastit (= 0.2.1), libc6 (>= 2.34), libgcc-s1
blastit-python-3.14              python3.14, blastit (= 0.2.1), libc6 (>= 2.34), libgcc-s1
```

The core depends on **no** shim; each shim pins the exact core. In `rockylinux:9`,
`rpm -qp --requires` reads `glibc >= 2.34, libgcc` for the core, the same for the
two CPython shims, and `glibc >= 2.17` (no `libgcc`) for the four PHP shims — each
package carries its own measured floor, and the shims add only
`Requires: blastit = 0.2.1-1` plus `Recommends: python3.13|3.14` /
`Suggests: php-embedded`, so nothing on Rocky 9 is unsatisfiable.

### `stage-dist.py --dry-run` accepts the whole set (the skill's copy, verbatim)

```
$ cmp packaging/stage-dist.py ~/offworld-nexus/.opencode/skills/offworld-packaging/scripts/stage_dist.py
(identical; sha256 3bc069f9…a974b both)

$ uv run --no-project python packaging/stage-dist.py --dry-run --source blastit \
    --channel stable --version 0.2.1 --ref refs/tags/v0.2.1 \
    --file "deb:amd64:target/packages/*_amd64.deb" \
    --file "rpm:x86_64:target/packages/*.x86_64.rpm"
artifacts 14 | channel stable | version 0.2.1 | ref refs/tags/v0.2.1
deb: 7 rpm: 7
names: ['blastit', 'blastit-php-8.3', 'blastit-php-8.4', 'blastit-php-8.5',
        'blastit-php-8.6', 'blastit-python-3.13', 'blastit-python-3.14']
```

### The set installs the §6.1 tree — and a set missing one shim fails by name

```
$ scripts/check-bundle-layout.sh --deb-set target/packages --arch amd64
ok   …/root/usr/share/blastit: bin/{blastit,blastit-lsp} -> lib/, 2 Python shims, 4 PHP shims, python/blastit/*.py
ok   …/root/usr/bin/blastit --version: blastit 0.2.1        rc=0

$ scripts/check-bundle-layout.sh --rpm-set target/packages --arch x86_64
check-bundle-layout: unpacking 7 RPM(s) with rpm2cpio in rockylinux:9
ok   …/usr/share/blastit: … 2 Python shims, 4 PHP shims …  rc=0

$ cp target/packages/*.deb /tmp/mb65-negative/ && rm /tmp/mb65-negative/blastit-php-8.5_0.2.1_amd64.deb
$ scripts/check-bundle-layout.sh --deb-set /tmp/mb65-negative --arch amd64
error: the deb set has no blastit-php-8.5 package (…): the release ships 7 and a set missing a shim would install a tree with no blastit-php-8.5 shim
error: … holds 6 deb package(s) for amd64; the release ships 7 (1 core + 2 Python + 4 PHP) and 1 is missing — see the package named above
rc=1

$ scripts/check-bundle-layout.sh --deb-set target/packages --arch arm64
error: the deb set has no blastit-php-8.6 package (…_arm64.deb) … rc=1   # the arm64 set is CI's; it is not silently accepted
```

### Every advertised distribution installs it and serves a view

`scripts/install_matrix.sh --leg <name> --dir target/packages`, one leg per
(distribution, architecture) the repositories advertise; the `debian-13` leg is
`install static wsgi php` (it installs `libphp8.4-embed` first, so the Debian
shim's hard `Depends:` is exercised — and asserts `blastit-php-8.6` is *not*
chosen, trixie having no `libphp8.6-embed`):

```
PASS  deb   deb (debian:bookworm-slim)    1 passed, 0 failed, 0 skipped
PASS  deb   deb (debian:trixie)           1 passed, 0 failed, 0 skipped
PASS  deb   deb (ubuntu:22.04)            1 passed, 0 failed, 0 skipped
PASS  deb   deb (ubuntu:24.04)            1 passed, 0 failed, 0 skipped
PASS  rpm   rpm (rockylinux:9)            1 passed, 0 failed, 0 skipped
PASS  rpm   rpm (fedora:latest)           1 passed, 0 failed, 0 skipped
```

On `debian:trixie` the installer reports `installed: blastit 0.2.1` and
`language shims installed: blastit-python-3.13 blastit-php-8.4`, and only those
two `.so` files are on disk. `fedora:latest` needed
`BLASTIT_PKG_ACCEPT_UNSIGNED=1` for the *local* RPMs only (Fedora's
`%_pkgverify_level all` refuses an unsigned file; the repository path is never
relaxed and the App signs what it publishes).

### Gates

```
$ uv run --no-project scripts/platform_matrix.py --check     → ok, 0 leg(s) dropped
$ packaging/package-set.sh --check                           → ok (7 packages; spec stanzas/globs match)
$ sh scripts/test_install.sh                                 → all cases passed
$ make docs-check                                            → No issues found
$ uv run --no-project tools/guard.py                         → guard: OK
$ uv run --no-project tools/plan.py validate                 → plan OK
$ scripts/dev-version.sh --check                             → every intra-workspace requirement names `0.2.1`
$ scripts/release-notes.sh --require-dated v0.2.1            → the dated entry
$ make fast-check                                            → rc=2, one test only (below)
```

`make fast-check` fails exactly one test,
`e2e_check_fmt::learn_writes_one_grant_without_widening_a_view`, with
`path must be shorter than SUN_LEN`: `crates/blastit/tests/common/project.rs`
prefers `/tmp/opencode` when it exists (this harness always creates it), pushing
the fixture's Unix socket past `sun_path`. The test file is unchanged from HEAD
and the same binary passes with `/tmp/opencode` absent, which is CI:

```
$ unshare -rm bash -c 'mount -t tmpfs tmpfs /tmp && touch /tmp/opencode && \
    TMPDIR=/tmp/mb65 cargo test -p blastit --test e2e_check_fmt \
    learn_writes_one_grant_without_widening_a_view'
test learn_writes_one_grant_without_widening_a_view ... ok
test result: ok. 1 passed; 0 failed; …
```

### Zero retired-registry and zero internal-hostname references

```
$ git grep -n -i -e CLOUDSMITH_ -e FORGEJO_ -e 'cloudsmith\.io' \
      -e 'forge\.offworld' -e chulak -- .github scripts e2e packaging \
      docs/install.md docs/install.sh docs/index.md docs/getting-started.md \
      README.md RELEASING.md
rc=1   # no match

$ git grep -n -i -e CLOUDSMITH_ -e FORGEJO_ -e 'cloudsmith\.io' \
      -e 'forge\.offworld' -e chulak -- ':!plan' ':!docs/design' ':!docs/decisions'
rc=1   # no match
```

The only surviving mentions of the retired registries by *name* are changelog
history (`## 0.2.0`'s entry and this release's "…are gone" sentence) and the
archived `docs/design/`+`docs/decisions/` records, which are historical documents
and are deliberately not rewritten. No workflow, script, test, installer or public
page names them, and no public text names an internal host.
