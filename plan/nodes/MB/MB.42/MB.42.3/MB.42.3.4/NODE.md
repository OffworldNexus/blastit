---
id: MB.42.3.4
title: "Signed-repo publishing: packages.yml (main) + nightly.yml (stamps, prune 7) + Forgejo mirror"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.42.3.3]
paths: [".github/workflows/**", "scripts/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T01:33"
updated: "2026-11-20T01:33"
---

# Goal
Publish the packages to the **signed APT/RPM repositories**: Cloudsmith primary + Forgejo mirror, `main` for a tagged release and `nightly` for scheduled builds, with nightly retention.

Authoritative detail: `plan/nodes/MB/MB.42/MB.42.3/NODE.md` §A–§C and §5 (workflows). Read them.

# Exact Deliverables
1. **`.github/workflows/packages.yml`** — on `v*` tag: build the 4 native packages (+ archives + `SHA256SUMS`), run the container install matrix (Debian 12/13, Ubuntu 22.04/24.04, Rocky 9, Fedora latest) **before** publishing, then:
   - Cloudsmith (`.deb`): `cloudsmith push deb offworld-nexus/${CLOUDSMITH_APT_REPO:-apt}/any-distro/any-version <file> --component main --republish` with `secrets.CLOUDSMITH_API_KEY`;
   - Cloudsmith (`.rpm`): `cloudsmith push rpm offworld-nexus/${CLOUDSMITH_RPM_REPO:-rpm}/any-distro/any-version <file> --republish`;
   - Forgejo mirror (best-effort, never fatal): `POST ${vars.FORGEJO_URL:-https://forge.offworld.nexus}/api/packages/OffworldNexus/debian/pool/any-distro/main/upload` and `…/rpm/main/upload`, basic auth `Xowap:${secrets.FORGEJO_PACKAGES_TOKEN}`, multipart `package=@<file>`; **skip cleanly (exit 0 + `::notice::`)** when the token is unset so forks and PRs stay green;
   - attach `.deb`/`.rpm`/`.tar.gz`/`SHA256SUMS` to the GitHub Release.
2. **`.github/workflows/nightly.yml`** — `schedule:` + `workflow_dispatch` on `develop`: stamp `.deb` `0.2.0~nightly<YYYYMMDD>.r<run>+g<sha>` and `.rpm` `0.2.0-0.nightly.<YYYYMMDD>.r<run>.g<sha>`, publish to the `nightly` component, run the same container install matrix, then **prune to the newest 7** (Cloudsmith API list + delete, newest-first by version).
3. `scripts/check_workflows.py` and `scripts/test_packaging_gates.py` must **know** these workflows and leg lists (declare once in `scripts/platform_matrix.py`, never a second copy), and refuse a workflow that hard-codes a leg the matrix does not declare.
4. **Dry-run proof only** at leaf level: token-unset paths exit 0 with a notice, and the push commands are validated by a `--dry-run`/`echo` mode. Do **not** perform a real publish.

# Notes
- Never print a token; never interpolate a secret into a message or a log line.
- Never write `/home/remy/.local` or `/usr/bin/blastit`.
- No `--allow-unauthenticated`/`--nogpgcheck` anywhere; repository installs rely on apt/dnf GPG.
- Never touch `docs/**`.

# Acceptance
- `uv run --no-project scripts/check_workflows.py` green (and it fails if you delete a matrix leg from the workflow)
- `uv run --no-project scripts/test_packaging_gates.py` green
- the token-unset path proven locally (paste the `::notice::` line and exit 0)
- nightly version stamps printed by the same script the workflow calls (paste both formats)
- `make fast-check`
