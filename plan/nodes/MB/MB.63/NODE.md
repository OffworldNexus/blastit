---
id: MB.63
title: "Install sury PHP 8.6 from signature-verified hash-pinned payloads (no sury index)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/php-sapi/Dockerfile"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-21T15:20"
updated: "2026-11-21T15:44"
---

# Goal

The only 2 red checks on PR #64 are the `("8.6","gnu")` leg (`§2 PHP 8.6 (aarch64-gnu)` and `shim (libblastit_php86-aarch64-gnu.so)`). Root cause is upstream and durable: Ondřej Surý's repo serves **index objects that do not match its own signed `InRelease`**, per-edge and per-variant, while the `.deb` payloads are fine. Measured 2026-11-21:

- `InRelease` 13:32:01Z signs `main/binary-arm64/Packages` = 2252475 (served correctly on every check), `.gz` = 522854 and `.xz` = 294336 (edges served 520904/523272 and 292780/294520 over the hour);
- amd64 is the mirror image: its **compressed** variant is the consistent one, its uncompressed is not;
- `by-hash/` returns 404, so apt has no content-addressed escape, and `Acquire::CompressionTypes::Order::=` does **not** avoid the compressed fetch;
- 12/12 `apt-get update` attempts fail from this host; the same leg passed at 11:39Z. So retries cannot fix it (that is why `MB.62` stays as-is: it fails closed with a correct diagnosis).

**The user asked for an alternative.** Verified in this session: every needed payload downloads and hash-verifies for both arches (amd64 + arm64, 7 packages each, ~6.8 MB per arch), `gpgv` verifies sury's `InRelease` inside `debian:bookworm-slim` with `--keyring /etc/apt/keyrings/sury.gpg` (`Good signature from "DEB.SURY.ORG Automatic Signing Key"`), and the signed index hash is extractable with plain `awk`.

## Change: install the sury leg's PHP from signature-verified, hash-pinned payloads (no sury index at all)

Only the `EXTRA_REPO=sury` branch of `e2e/php-sapi/Dockerfile` changes. Leave the Debian-only first `apt-get update`, the `MB.62` retry escalation for **non-sury** legs, and the whole Alpine branch exactly as they are. For `sury`:

1. Install `ca-certificates curl gpgv` first (from Debian, which works).
2. `curl` the signing key + `dists/bookworm/InRelease`; **`gpgv --keyring /etc/apt/keyrings/sury.gpg InRelease`** must verify, else fail the build.
3. Pick the arch's index: for each of `Packages`, `Packages.gz`, `Packages.xz` on `main/binary-$(dpkg --print-architecture)/`, download it, and accept the first whose raw `sha256sum` equals the value the *signed* `InRelease` lists for that exact filename (decompress `.gz`/`.xz` for use). If none matches, fail with a message naming the mismatch — that is upstream.
4. From that verified index, extract `SHA256  Filename` for the closure with one `awk` pass over blank-line-separated stanzas (`Package:`/`Filename:`/`SHA256:`), for these names only:
   `php8.6-dev libphp8.6-embed php8.6-curl php8.6-cli php8.6-common php8.6-readline php-common`
5. `curl` each payload to `/tmp/sury-debs/`, then `sha256sum -c` every one against the index value — a mismatch fails the build.
6. `apt-get install -y --no-install-recommends /tmp/sury-debs/*.deb` plus the existing tool list (`gcc make pkg-config libc6-dev binutils bash tar xz-utils clang libclang-dev`); apt resolves the Debian-side dependencies (`autoconf`, `automake`, `libtool`, `shtool`, `libpcre2-dev`, `libssl-dev`, …) from bookworm.

Hard constraints:

- **Do not** add the sury suite to `sources.list.d` in this branch (that is what breaks), and do not weaken any verification: signature first, then index hash, then payload hashes.
- No hardcoded per-version SHA256 list: the version must follow the repo (the index is the pin).
- `set -eux` semantics must make any verification failure fatal — no `|| true`, no `--allow-unauthenticated`.
- Keep it one RUN layer, keep the Alpine/`alpine-edge-testing` branch untouched, and keep the `MB.62` comment+escalation intact for the non-sury apt legs.
- No new file; everything stays in the Dockerfile.

# Acceptance

- `make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1` green on this host — this is the real exercise of the new path on amd64 (signature verify → index variant pick → 7 payload hashes → dpkg install → shim build → boot legs). Paste the log lines showing **which** index variant matched and that all 7 payloads verified.
- Fail-closed proofs, pasted into `RESULT.md`: (a) a deliberately corrupted expected hash makes `sha256sum -c` abort the build; (b) a tampered/absent signature makes `gpgv` abort it. (Use a scratch copy or a `docker run` snippet; do not leave the tree broken.)
- `uvx --offline ruff format --check e2e/` unchanged-green, `uv run --no-project tools/guard.py` clean, `uv run --no-project tools/plan.py validate` OK.
- Report the final RUN block verbatim so the conductor can confirm no verification was weakened.
