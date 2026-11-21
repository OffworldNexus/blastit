---
id: MB.64
title: Pin sury PHP 8.6 payloads per arch (sury index is durably inconsistent)
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/php-sapi/Dockerfile"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-21T15:55"
updated: "2026-11-21T16:12"
---

# Goal

`MB.63`'s signature-verified, variant-adaptive index acquisition is correct, and it is *proven* to work when sury is merely mid-publish (it recovered a live rotation in the lead's own run). But the arm64 index is now **durably** inconsistent: at `InRelease` 14:17:55Z, all three variants mismatch and the divergence is not a simple lag — arm64 served `Packages` 2258246 (signed 2257457), `.gz` 523272 (525237), `.xz` 294520 (296088). A 3×10s retry cannot span that, and CI run 44915554086 failed on exactly that after (correctly) exhausting its attempts.

The payloads, however, are immutable and every one is fetchable and hash-verifiable right now. So: **stop consulting sury's index at build time and install the payloads from verified pins.**

## Change (only the `EXTRA_REPO=sury` branch of `e2e/php-sapi/Dockerfile`)

Replace the `gpgv` + `InRelease` + variant-selection + index-derived mapping with a **pinned payload list per architecture**, downloaded and hash-verified:

```
case "$(dpkg --print-architecture)" in
  amd64) set -- <14 args: 7 × 'sha' 'pool/…deb'> ;;
  arm64) set -- <…> ;;
  *) echo "no pinned PHP 8.6 payloads for this architecture" >&2; exit 1 ;;
esac
while [ "$#" -gt 0 ]; do
    sha=$1; path=$2; shift 2
    file=$(basename "$path")
    curl -fsSL --retry 3 "https://packages.sury.org/php/$path" -o "/tmp/sury-debs/$file"
    printf '%s  /tmp/sury-debs/%s\n' "$sha" "$file" >> /tmp/sury-SUMS
done
sha256sum -c /tmp/sury-SUMS
apt-get install -y --no-install-recommends /tmp/sury-debs/*.deb \
    gcc make pkg-config libc6-dev binutils ca-certificates curl bash tar xz-utils \
    clang libclang-dev
```

The exact pins (each verified today against a signature-checked index; `php-common` is `all` and shared):

## amd64 (dpkg --print-architecture == amd64)
- `4017e18448b109f640161eacfbd9e2c14f5ccd43fee0711f4f752b1faa39a6b9` `pool/main/p/php8.6/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_amd64.deb` — libphp8.6-embed 8.6.0-3+debsuryorg12.1+1 (amd64)
- `6e729a360ac400a3be48e084389cc66c9e04af7cec30d9b1a7a69336ab029383` `pool/main/p/php-defaults/php-common_102+debsuryorg12.1+3_all.deb` — php-common 2:102+debsuryorg12.1+3 (all)
- `6bbe1c4d2af3a29fb818d38989d866a5805a987511b5f6da857c0fe6a141b1d6` `pool/main/p/php8.6/php8.6-cli_8.6.0-3+debsuryorg12.1+1_amd64.deb` — php8.6-cli 8.6.0-3+debsuryorg12.1+1 (amd64)
- `c1deb64ad9f064b5f06b1ebc8eefd9cb7c4b93407f35709b620b2b770059d941` `pool/main/p/php8.6/php8.6-common_8.6.0-3+debsuryorg12.1+1_amd64.deb` — php8.6-common 8.6.0-3+debsuryorg12.1+1 (amd64)
- `ff98afc81bcbbf4395ccfec15d1990ac52e9a4dcbddc4fa0315440840d8d4843` `pool/main/p/php8.6/php8.6-curl_8.6.0-3+debsuryorg12.1+1_amd64.deb` — php8.6-curl 8.6.0-3+debsuryorg12.1+1 (amd64)
- `dc84a0d58ada1a6eed84e6c02a711e17766f774d6425aa1db691198b1ad3675c` `pool/main/p/php8.6/php8.6-dev_8.6.0-3+debsuryorg12.1+1_amd64.deb` — php8.6-dev 8.6.0-3+debsuryorg12.1+1 (amd64)
- `076bd388dc546c64b44fa47da7e75c79855b4377dea42af02563ebca0e570ce9` `pool/main/p/php8.6/php8.6-readline_8.6.0-3+debsuryorg12.1+1_amd64.deb` — php8.6-readline 8.6.0-3+debsuryorg12.1+1 (amd64)

## arm64 (dpkg --print-architecture == arm64)
- `05c7df15aee65495629856b6a357bddc241cc858df20fe0fd1ad6b0a8e901150` `pool/main/p/php8.6/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_arm64.deb` — libphp8.6-embed 8.6.0-3+debsuryorg12.1+1 (arm64)
- `6e729a360ac400a3be48e084389cc66c9e04af7cec30d9b1a7a69336ab029383` `pool/main/p/php-defaults/php-common_102+debsuryorg12.1+3_all.deb` — php-common 2:102+debsuryorg12.1+3 (all)
- `114b0a361209e8cc2649fa096580e3ff645f8500dff373117b6d9cb0d99984af` `pool/main/p/php8.6/php8.6-cli_8.6.0-3+debsuryorg12.1+1_arm64.deb` — php8.6-cli 8.6.0-3+debsuryorg12.1+1 (arm64)
- `ab4d4939ec7609ade3d653f5cb2a79fb0876e9b5f927c91f551ee09b4d10e141` `pool/main/p/php8.6/php8.6-common_8.6.0-3+debsuryorg12.1+1_arm64.deb` — php8.6-common 8.6.0-3+debsuryorg12.1+1 (arm64)
- `5e65f362112e9a17c1413df38fa0b871f07c77b911aeb4bcb04cd74f9ba76330` `pool/main/p/php8.6/php8.6-curl_8.6.0-3+debsuryorg12.1+1_arm64.deb` — php8.6-curl 8.6.0-3+debsuryorg12.1+1 (arm64)
- `5b26ce094929289b721533075ccd46a4035d425aef5f82213bd54f214210f917` `pool/main/p/php8.6/php8.6-dev_8.6.0-3+debsuryorg12.1+1_arm64.deb` — php8.6-dev 8.6.0-3+debsuryorg12.1+1 (arm64)
- `a1a18d293f22f03a7c8f76417cb272c05f2b0e7250ac5f64877230f59b5318db` `pool/main/p/php8.6/php8.6-readline_8.6.0-3+debsuryorg12.1+1_arm64.deb` — php8.6-readline 8.6.0-3+debsuryorg12.1+1 (arm64)

Hard constraints:

- Verification must stay fatal under `set -eux`: `sha256sum -c` is the gate, `curl --retry 3` is transport only. No `--allow-unauthenticated`, no `|| true`.
- Drop `gpgv`/`xz-utils` from the sury branch (no index is fetched any more). Keep the Debian `apt-get update` + its `MB.62` retry escalation for the **non-sury** legs, untouched — sury is no longer an apt source at all.
- Comment the pin's provenance precisely: sury `by-hash/` is 404 and its index objects (all three variants, per edge) can disagree with its signed `InRelease` for >1h, so the payloads are pinned against the signature-checked index of 2026-11-21T14:17:55Z; bump these pins when PHP 8.6 moves.
- Nothing version-agnostic may regress: the pins are the only new literals; keep them grouped and sorted by package name.
- Alpine branch, `MB.62` escalation, non-sury install line: byte-identical.

# Acceptance

- `make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1` green on this host — this exercises the pinned install on amd64 end to end (7 payloads verified, dpkg install, shim build, boot legs). Paste the `sha256sum -c` output.
- Fail-closed proof in `RESULT.md`: flip one pinned hash and show `sha256sum -c` aborting the build; restore it.
- `uvx --offline ruff format --check e2e/`, `guard.py`, `plan.py validate` clean.
