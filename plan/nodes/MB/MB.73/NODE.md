---
id: MB.73
title: "Install legs must install every produced package, not just runtime-matched shims"
kind: task
status: active
owner: implementer
priority: 1
depends_on: []
paths: [e2e/run_packaging.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T11:04"
updated: "2026-11-23T11:06"
---

# Goal

Owner's observation, confirmed in the code: we build **28** artifacts (7 names × amd64/arm64 × deb/rpm) but the install matrix only installs the shims whose interpreter happens to exist in each image. `e2e/run_packaging.py`'s `shim_probe()` mirrors `docs/install.sh`:

```sh
present=
for minor in 3.13 3.14; do command -v "python$minor" && present="$present blastit-python-$minor"; done
for minor in 8.3 8.4 8.5 8.6; do [ -e /usr/lib*/libphp$minor.so ] && present="$present blastit-php-$minor"; done
```

So `blastit-php-8.3`, `-8.5`, `-8.6` (and the Python shims on images without 3.13/3.14) are **built but never installed** by any leg, and nothing proves they even install. That is the gap.

# Change (e2e/run_packaging.py only — do not touch .github/)

In every `deb` and `rpm` install leg:

1. **Install the whole set for that format and arch, always**: `blastit` plus all seven shim packages from `packaging/languages.tsv`, by explicit file path (not by package name, and not filtered by what the image has). This is only possible because `MB.66` made the runtime a `Recommends:` — say so in the leg's comment, since it is what makes "install a shim whose runtime is absent" a supported state rather than a broken one.
2. **Keep the runtime-driven exercises** as they are: serve the WSGI fixture through the shipped shim where CPython 3.13/3.14 exists, and the PHP fixture where `libphp8.x.so` exists. Those remain the "prove it actually loads" half.
3. **Assert the full set installed**: exactly `1 + <shim rows>` package files present in the image's package database (`dpkg -l` / `rpm -q` on the seven names), naming any missing package. A leg that installs six of seven must fail by name — the same discipline `MB.71` added for the RPM selection.
4. State in one line per leg which shims were installed-but-not-runtime-exercised for that image (e.g. "php 8.3/8.5/8.6 installed, runtime absent, not exercised"), so the coverage is visible in the log instead of implied.

# Acceptance

- On a real run of `--leg deb`/`--leg rpm` for at least two images (one Debian-like, one RPM), the log shows **all seven** packages installed and the exact count assertion, plus the runtime-exercised subset.
- Negative proof: drop one shim from the directory and show the leg fails naming it.
- `--print-legs` still lists the same rows (it reads `languages.tsv`).
- `make fast-check` green.
