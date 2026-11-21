---
id: MB.42.3.2
title: "docs/install.sh: signed-repo first, /dev/tty interactivity, site/install.sh publication"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.3.1]
paths: [docs/install.sh, .github/workflows/docs.yml, Makefile, "scripts/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T01:33"
updated: "2026-11-20T02:56"
---

# Goal
`docs/install.sh` — the one command the hero advertises — published at `https://offworldnexus.github.io/blastit/install.sh`, and its repository-setup behaviour proven.

Authoritative detail: `plan/nodes/MB/MB.42/MB.42.3/NODE.md` §2 (install.sh contract). Read it.

# Exact Deliverables
1. `docs/install.sh`, POSIX `sh`, `set -eu`, no bashisms, `NO_COLOR`/`TERM=dumb` safe. Flags `--version`, `--nightly`, `--mirror cloudsmith|forgejo`, `--user`, `--prefix`, `--develop [ref]`, `-y/--yes`, `-h/--help`; env `BLASTIT_RELEASE_BASE_URL`, `BLASTIT_DEB`, `BLASTIT_RPM`. Usage errors and an unwritable `--prefix` -> usage on **stderr**, exit `2`.
2. **`/dev/tty` rule (non-negotiable):** `stdin` is **never** consulted for interactivity (`[ -t 0 ]` is forbidden) because `curl … | sh` carries the script on fd 0. Test interactivity with `[ -r /dev/tty ] && ( : </dev/tty ) 2>/dev/null`; read confirmations and `sudo` prompts from `</dev/tty`; write notes to **stderr**. So `curl -fsSL …/install.sh | sh` in a terminal still takes the signed-repo path.
3. Install paths: (a) `BLASTIT_DEB`/`BLASTIT_RPM` installs the local file; (b) **default** = signed repo: Debian/Ubuntu runs the Cloudsmith `bash.deb.sh` (or signed Forgejo source with `--mirror forgejo`) then `apt-get update && apt-get install -y --no-install-recommends blastit[=<ver>]` via root/`sudo`; Fedora/RHEL/Rocky/Alma runs `bash.rpm.sh` then `dnf install -y blastit[-<ver>]`; `--nightly` selects the `nightly` component; (c) direct `.tar.gz` **only** for `--user`/`--prefix`/`--develop`/`musl`/no-`sudo` fallback, verified against the **embedded** SHA-256 table, or requiring `BLASTIT_SHA256`/`BLASTIT_SHA256SUMS_FILE` when `BLASTIT_RELEASE_BASE_URL` overrides the origin — **failing closed** when absent.
4. Never `--allow-unauthenticated` / `--nogpgcheck`. Print `Add it to your PATH: export PATH="<dir>/bin:$PATH"` when needed, and the exact next command on any refusal.
5. **Publication:** `.github/workflows/docs.yml` copies `docs/install.sh` to the built `site/install.sh` (Zensical does not copy non-`.md` files), and CI proves `curl -fsSL <local site>/install.sh | sh -s -- --help` works (a `--help`-only pipe smoke; never a real install).

# Notes
- Test every path with `BLASTIT_RELEASE_BASE_URL` against a **local fixture server** — never against a real repository.
- `--help` must be runnable without root and without network.
- Never write `/home/remy/.local` or `/usr/bin/blastit`; container-only for anything that installs a package.
- Never touch `docs/**` beyond `docs/install.sh` and `.github/workflows/docs.yml`.

# Acceptance
- `sh -n docs/install.sh` + `shellcheck`-clean if available; `--help` exits 0
- `curl -fsSL <local site>/install.sh | sh -s -- --help` works from a real pipe
- `--user` against a fixture base URL installs into a scratch prefix and `blastit --version` runs
- `BLASTIT_DEB`/`BLASTIT_RPM` local-file path verified in a container
- the `docs.yml` publication step proven (built site contains `install.sh` with identical bytes)
- `make fast-check`
