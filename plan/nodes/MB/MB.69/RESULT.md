# MB.69 — generated RPM spec builds non-root (no `%install` chown, no live macros in comments)

**Status:** done — 3 files (`scripts/rpm/gen_spec.py`, regenerated `scripts/rpm/blastit.spec`, `scripts/build-rpm.sh`). `.github/workflows/packages.yml` unchanged: CI already builds RPMs non-root, which is why it caught this and a local container build did not.

## Review round (fixes 1-3)

| Review item | Change |
|---|---|
| **[MINOR] the ownership oracle never fires on this host's default path** | The cargo-cult `awk '$1 != "root:root"'` check now runs **inside** the container `INNER` script, before `cp "$pkg" /out/`, using the image's own `rpm` as uid `HOST_UID`; a payload entry that is not `root:root` `exit 1`s naming the package (and an unreadable `rpm -qp` fails too). It prints `container-payload-ownership: <rpm>: every payload entry is root:root` per package, so a build shows the check ran rather than a skip. |
| **[MINOR] `\|\| true` made the oracle pass vacuously** | Both the container and host paths now capture the listing in `if ! listing=$(rpm -qp …)`, so an unreadable payload is a hard failure (`cannot read the payload of …: rpm -qp failed`), and the non-root filter is `awk '!/^root:root /'` (exits 0 with no output), so no `\|\| true` remains. Both branches use the same sentence, `records payload entries that are not root:root`. The "no rpm on this host" skip branch is kept and now says the payload ownership was asserted by the builder / is left to `e2e/run_packaging.py --leg rpm`. |
| **[NIT] doubled prose sigils** | `_prose_ownership()` reads "the install step", "each files block", "a `defattr(-,root,root,-)` line" — no `%%` before a step name, so `^%install` stays greppable. `%%{…}` escapes remain only for the macro *names* in `_prose_one_floor`. |

## What changed

| File | Change |
|---|---|
| `scripts/rpm/gen_spec.py` | `_INSTALL` no longer emits `chown -R 0:0 %{buildroot}` (and drops the comment citing the deleted `scripts/test_packages.sh`). `_CORE_FILES` and `_files(row)` open with `%defattr(-,root,root,-)` — owner/group pinned in the header, mode left as `-` so the staged tree's `install -m` modes (0755 binaries, 0644 modules) survive. `_prose_one_floor`'s two live macros are escaped (`%%{blastit_glibc_core}`, `%%{blastit_glibc_<slug>}`). New `_prose_ownership()` header paragraph states the invariant (root:root comes from `%defattr` metadata, not a filesystem chown; `rpmbuild` runs as an ordinary user). |
| `scripts/rpm/blastit.spec` | Regenerated with `gen_spec.py --write` (never hand-edited). |
| `scripts/build-rpm.sh` | The docker fallback now installs `rpm-build`/stages inputs as root, then `exec setpriv --reuid "$HOST_UID" --regid "$HOST_GID" --clear-groups … rpmbuild` (mirroring `scripts/build-package-bundle.sh:183-214`); the output `chown` is gone. Payload-ownership assertion in **both** places: inside the container before each RPM is copied out, and after the build where `rpm` exists on the host. Header prose restated. |

## Acceptance

### Non-root build (the point of the node) — real `scripts/build-rpm.sh`, current non-root user (uid 1000)

Bundle: `scripts/build-package-bundle.sh` needs the CI shim artifacts, which this host does not have, so the staging tree was assembled as the brief allows: `target/release/blastit`, `target/release/blastit-lsp`, `target/release/libblastit_py_shim.so` **copied under each of the six shim names** `packaging/languages.tsv` declares (`libblastit_{py313,py314,php83…86}-x86_64-gnu.so`), `LICENSE`, `python/blastit/*.py`, and the two `bin/` symlinks. **The shim bytes are stand-ins** (the real Python shim, so every floor is a real ELF measurement; the PHP rows therefore report 2.34 instead of 2.17).

```
$ uv run --no-project scripts/build-rpm.sh --version 0.2.1 --release 1 \
      --bundle /tmp/mb69-bundle/bundle --out /tmp/mb69-out
build-rpm: blastit: measured glibc floor 2.34, libgcc 1, needed: ld-linux-x86-64.so.2 libc.so.6 libgcc_s.so.1 libm.so.6
build-rpm: blastit-python-3.13: measured glibc floor 2.34, libgcc 1, …
build-rpm: blastit-php-8.6: measured glibc floor 2.34, libgcc 1, …
build-rpm: /tmp/mb69-out/blastit-0.2.1-1.x86_64.rpm (5696050 bytes)
build-rpm: /tmp/mb69-out/blastit-python-3.13-0.2.1-1.x86_64.rpm (424353 bytes)
build-rpm: /tmp/mb69-out/blastit-python-3.14-0.2.1-1.x86_64.rpm (424507 bytes)
build-rpm: /tmp/mb69-out/blastit-php-8.3-0.2.1-1.x86_64.rpm (424137 bytes)
build-rpm: /tmp/mb69-out/blastit-php-8.4-0.2.1-1.x86_64.rpm (424129 bytes)
build-rpm: /tmp/mb69-out/blastit-php-8.5-0.2.1-1.x86_64.rpm (424129 bytes)
build-rpm: /tmp/mb69-out/blastit-php-8.6-0.2.1-1.x86_64.rpm (424129 bytes)
build-rpm: 7 package(s) in /tmp/mb69-out (6 shim(s), 4 python module(s))
build-rpm: no rpm command on this host, so the payload ownership of each RPM was asserted by the builder or is left to e2e/run_packaging.py --leg rpm (the install image re-checks every payload entry is root:root)
exit=0
```

Seven `.rpm` files; the build succeeded **because** `%install` no longer chowns (as uid 1000 a `chown -R 0:0` fails, which is exactly the CI failure). Note the in-container ownership oracle firing per package (review fix 1), not a skip:

```
container-payload-ownership: blastit-0.2.1-1.x86_64.rpm: every payload entry is root:root
container-payload-ownership: blastit-php-8.3-0.2.1-1.x86_64.rpm: every payload entry is root:root
container-payload-ownership: blastit-php-8.4-0.2.1-1.x86_64.rpm: every payload entry is root:root
container-payload-ownership: blastit-php-8.5-0.2.1-1.x86_64.rpm: every payload entry is root:root
container-payload-ownership: blastit-php-8.6-0.2.1-1.x86_64.rpm: every payload entry is root:root
container-payload-ownership: blastit-python-3.13-0.2.1-1.x86_64.rpm: every payload entry is root:root
container-payload-ownership: blastit-python-3.14-0.2.1-1.x86_64.rpm: every payload entry is root:root
```

### Ownership evidence — only `root:root`, despite the non-root build

```
$ docker run --rm -v /tmp/mb69-out:/pkg:ro rockylinux:9 bash -c 'for f in /pkg/*.rpm; do printf "%s " "$(basename "$f")"; rpm -qp --qf "[%{FILEUSERNAME}:%{FILEGROUPNAME} %{FILENAMES}\n]" "$f" | awk "{print \$1}" | sort -u | tr "\n" " "; echo; done'
blastit-0.2.1-1.x86_64.rpm root:root
blastit-php-8.3-0.2.1-1.x86_64.rpm root:root
blastit-php-8.4-0.2.1-1.x86_64.rpm root:root
blastit-php-8.5-0.2.1-1.x86_64.rpm root:root
blastit-php-8.6-0.2.1-1.x86_64.rpm root:root
blastit-python-3.13-0.2.1-1.x86_64.rpm root:root
blastit-python-3.14-0.2.1-1.x86_64.rpm root:root
```

Measured floors / `Recommends:` unchanged (header metadata, not the `%files` owner):

```
== core requires:            glibc >= 2.34 / libgcc
== core recommends:          (none)
== python-3.13 requires:     blastit = 0.2.1-1 / glibc >= 2.34 / libgcc
== python-3.13 recommends:   python3.13
== php-8.4 requires:         blastit = 0.2.1-1 / glibc >= 2.34 / libgcc
== php-8.4 recommends:       php-embedded
```

Modes still come from the staged tree (`-` in `%defattr`):

```
root:root lrwxrwxrwx /usr/bin/blastit
root:root -rw-r--r-- /usr/share/blastit/LICENSE
root:root -rwxr-xr-x /usr/share/blastit/lib/blastit
root:root -rwxr-xr-x /usr/share/blastit/lib/blastit-lsp
root:root drwxr-xr-x /usr/share/blastit/python
```

### No macro warning — `rpmbuild -bb` inside `rockylinux:9` as uid 1000

```
$ docker run --rm -e HOST_UID=1000 -e HOST_GID=1000 -v $PWD/scripts/rpm:/spec:ro \
    -v /tmp/mb69-bundle/bundle:/src:ro rockylinux:9 bash -euc '<dnf install rpm-build; stage; setpriv … rpmbuild -bb /work/spec/blastit.spec --define _topdir … --define blastit_bundle … --target x86_64>'
--- builder uid: 1000
--- rpms built: 7
--- Macro expanded in comment:
(none)
--- chown / Operation not permitted / Bad exit:
(none)
--- ownership of one payload:
root:root lrwxrwxrwx /usr/bin/blastit
root:root -rw-r--r-- /usr/share/blastit/LICENSE
root:root -rwxr-xr-x /usr/share/blastit/lib/blastit
```

Committed spec, after regeneration:

```
$ grep -cP '^\s*#.*(?<!%)%\{' scripts/rpm/blastit.spec
0
grep-exit=1
$ grep -nE '%%(install|files|defattr)' scripts/rpm/blastit.spec
(none)
$ grep -n '%%{' scripts/rpm/blastit.spec
17:# `Requires: glibc >= %%{blastit_glibc_core}` (2.34 on today's artifacts) and
18:# `blastit-php-8.6` `Requires: glibc >= %%{blastit_glibc_php86}` (2.17), so a
$ grep -c '^%defattr(-,root,root,-)$' scripts/rpm/blastit.spec
7
$ grep -n 'chown' scripts/rpm/blastit.spec
53:# step cannot `chown` the buildroot, and never tries: ownership is package
```

### Gates

```
$ uv run --no-project scripts/rpm/gen_spec.py --check
exit=0
$ packaging/package-set.sh --check
package-set: ok (7 packages; packaging/languages.tsv is consistent)
exit=0
$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared; …
exit=0
$ make fast-check
install-test: ok (i) the rpm half passes --nosignature only with BLASTIT_PKG_ACCEPT_UNSIGNED=1
install-test: ok (j) gen_language_blocks.py --check: the committed blocks are the table's and Cargo.toml's version
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
exit=0
```

(`packaging/package-set.sh --check` is a bash script, so it was run directly rather than through `uv run`.)

## Does CI already build RPMs as non-root?

**Yes.** `packages.yml`'s `§3 … from that bundle` step runs `scripts/build-rpm.sh` on the runner; on `ubuntu-*` runners `rpmbuild` is present, so `build-rpm.sh:366-381` takes the **host** branch, executed as the non-root `runner` user. That branch is the only way the old `chown -R 0:0 %{buildroot}` could have failed with `Operation not permitted` (job 137570493074) — the docker branch installed and ran `rpmbuild` as container **root**, where that chown succeeds — so the CI evidence itself proves the non-root path, and is why CI found this and our local container build did not. The local fallback now matches it.

## Other packaging paths and root

- **deb:** `scripts/build-deb.sh:350` uses `dpkg-deb --root-owner-group` — ownership is metadata, never a chown. No change needed.
- **bundle assembly:** `scripts/build-package-bundle.sh` already drops to `HOST_UID` with `setpriv`; `cp -a`/`install -m` staging needs no root.
- **`check-bundle-layout.sh`:** the RPM unpack path (`rpm2cpio | cpio` in `rockylinux:9`) runs as container root and then `chown -R "$HOST_UID:$HOST_GID" /root` **inside** the container so the host user can read/remove the scratch tree — container-root by design, not a host-root requirement. The deb path (`dpkg-deb -x`) unpacks as the host user.
- **`e2e/run_packaging.py`:** installs the packages as root *inside* the install image (that is what installing a package requires) and asserts `root:root` payload ownership there (`:1177-1199`) — the counterpart of the new builder-side assertion. Nothing to fix.

No follow-ups: the host-side ownership pass still runs only where `rpm` exists (this host has none), but the container build now asserts the same thing per package as uid 1000 before copying each RPM out, and the install leg re-checks it in the image.
