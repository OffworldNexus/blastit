# RESULT — MB.42.3.2 (`docs/install.sh` + its publication)

## What landed

| File | What |
|---|---|
| `docs/install.sh` | **New.** The one command the hero advertises — POSIX `sh`, `set -eu`, 536 lines / **437 prod LOC** (comments and blanks excluded), `NO_COLOR`/`TERM=dumb` safe, exit `2` on usage errors. |
| `scripts/install.sh` | **New** 10-line wrapper for a checkout (`sh scripts/install.sh --help`), as the parent brief §2 asks; it `exec`s `sh docs/install.sh`, so there is one installer, never a copy that drifts. |
| `.github/workflows/docs.yml` | Publishes `docs/install.sh` at the site root and proves it (below). No new job, no permission change. |

### Install paths (contract §6.2)

* **Default — signed repository.** Debian/Ubuntu: `bash.deb.sh` from Cloudsmith
  (`offworld-nexus/apt`), then `apt-get update` and
  `apt-get install -y --no-install-recommends blastit[=<ver>]` through
  root / `sudo -n` / `sudo </dev/tty`. Fedora/RHEL/Rocky/Alma: `bash.rpm.sh`
  then `dnf install -y blastit[-<ver>]`. `--mirror forgejo` writes a
  `signed-by=` APT source (key fetched from
  `${BLASTIT_FORGEJO_URL:-https://forge.offworld.nexus}/api/packages/OffworldNexus/debian/public.key`)
  or a `gpgcheck=1` `.repo` file; when the mirror publishes no key the script
  refuses and names `--mirror cloudsmith` as the next command. Never
  `--allow-unauthenticated`, never `--nogpgcheck`.
* **`--nightly`.** APT: rewrites the `offworld-nexus` source line's component to
  `nightly` and *verifies* the rewrite (`grep -Eq "^deb .* nightly$"`), failing
  with the next command otherwise. RPM: the nightly repo
  (`${CLOUDSMITH_RPM_NIGHTLY_REPO:-rpm-nightly}`), since an RPM repo carries no
  component. `--nightly` is refused together with `--version`, `--user`,
  `--prefix` and `--develop` (exit 2), and refused on a host that can only take
  the archive, with an explicit "drop --nightly" instruction.
* **Archive `.tar.gz`** only for `--user`, `--prefix <dir>`, `--develop [ref]`,
  a musl host, no `apt-get`/`dnf`, or no usable privilege. It handles both the
  shipped flat `blastit-<target>/{blastit,LICENSE,libblastit_*.so}` layout and a
  `share/blastit/lib/blastit` layout, installing `bin/` symlinks; it refuses
  tar listings with absolute or `..` entries.
* **Digest before bytes.** `resolve_digest` runs *before* the download:
  `BLASTIT_SHA256` → `BLASTIT_SHA256SUMS_FILE` → embedded table → canonical
  origin's own `SHA256SUMS`. An overridden `BLASTIT_RELEASE_BASE_URL` or
  `--develop` never reaches the table: it needs the operator's digest and fails
  closed with `set BLASTIT_SHA256=<hex>` as the next command. A mismatch (or a
  non-hex / non-64-char digest) refuses to unpack.
* **`BLASTIT_DEB` / `BLASTIT_RPM`** install the local file
  (`dpkg -i` / `rpm -Uvh --replacepkgs`) with the exact `sudo …` command printed
  when there is no privilege.

### `/dev/tty` rule

`stdin` is never consulted (no `[ -t 0 ]` in the file — `grep` finds only the
argument-count loop). Interactivity is
`[ -r /dev/tty ] && [ -w /dev/tty ] && ( : </dev/tty ) 2>/dev/null`;
confirmation and sudo read `</dev/tty`; every note goes to stderr. Proof below:
with stdin a pipe **and** a controlling terminal, the confirmation still appears
and the **repository** path runs; with no terminal there is no prompt and the
same repository path runs.

### Publication

Zensical 0.0.73 happens to copy non-Markdown files, but nothing in the gate
proves that, so `docs.yml` copies the file explicitly, `cmp`s it, `cmp`s the
HTTP response against the source, and pipes the served URL into
`sh -s -- --help` (a help-only smoke: no repository touched). It also refuses to
publish when `BLASTIT_DEFAULT_VERSION` drifts from `Cargo.toml`.

## Acceptance (command → output tail)

```
$ sh -n docs/install.sh ; dash -n docs/install.sh
syntax ok (sh + dash)

$ sh docs/install.sh --help ; echo $?
usage: install.sh [options]
…
  --version <ver>   install exactly this version       (default: 0.2.0)
…
help exit=0 stderr_bytes=0

$ cat docs/install.sh | sh -s -- --help ; echo $?
pipe help exit=0

$ BLASTIT_RELEASE_BASE_URL=http://127.0.0.1:8731 BLASTIT_SHA256=<fixture> \
    sh docs/install.sh --user --prefix /tmp/opencode/inst-test/prefix1 -y
blastit: verified blastit-x86_64-unknown-linux-gnu.tar.gz sha256 f6ed444fe806…
blastit: installed: blastit 0.2.0 in /tmp/opencode/inst-test/prefix1
blastit: Add it to your PATH: export PATH="/tmp/opencode/inst-test/prefix1/bin:$PATH"
$ /tmp/opencode/inst-test/prefix1/bin/blastit --version
blastit 0.2.0
$ stat -c %U:%G …/prefix1/bin/blastit ; readlink …/prefix1/bin/blastit
remy:remy
../share/blastit/lib/blastit

$ … BLASTIT_SHA256SUMS_FILE=/tmp/opencode/inst-test/site/SHA256SUMS \
    sh docs/install.sh --prefix …/prefix2 -y
blastit: installed: blastit 0.2.0 in /tmp/opencode/inst-test/prefix2
$ …/prefix2/bin/blastit --version
blastit 0.2.0

$ BLASTIT_RELEASE_BASE_URL=http://127.0.0.1:8731 sh docs/install.sh --prefix … -y
blastit: error: an overridden release origin is unsigned, so this installer will not guess:
set BLASTIT_SHA256=<hex> (or BLASTIT_SHA256SUMS_FILE=<local file>) to the digest you checked
(http log 22 -> 22 lines: nothing was downloaded before the refusal)

$ … BLASTIT_SHA256=<64 zeros> sh docs/install.sh --prefix … -y
blastit: error: sha256 mismatch for blastit-x86_64-unknown-linux-gnu.tar.gz: expected 000…0,
got f6ed444f… — refusing to install it; download it again from the official origin

$ sh docs/install.sh --nope                → exit 2
$ sh docs/install.sh --prefix /usr/bin     → exit 2 ("not writable by remy: choose a directory you own")
$ sh docs/install.sh --prefix rel          → exit 2 ("--prefix must be an absolute path")
$ sh docs/install.sh --nightly --user      → exit 2
$ sh docs/install.sh --mirror github       → exit 2
```

`BLASTIT_DEB` in `debian:trixie-slim` (`--network none`, real 0.2.0 package from
`MB.42.3.1`):

```
blastit: installing /pkg/blastit_0.2.0_amd64.deb with dpkg -i
Setting up blastit (0.2.0) ...
blastit: installed: blastit 0.2.0
blastit: upgrade later with: sudo apt-get update && sudo apt-get install --only-upgrade blastit
dpkg -W: blastit	0.2.0
blastit 0.2.0
```

`BLASTIT_RPM` in `rockylinux:9` (`--network none`):

```
blastit: installing /pkg/blastit-0.2.0-1.x86_64.rpm with rpm -Uvh --replacepkgs
blastit-0.2.0-1                       ########################################
blastit: installed: blastit 0.2.0
blastit: upgrade later with: sudo dnf upgrade blastit
rpm: 0.2.0
blastit 0.2.0
```

`/dev/tty` (stdin piped, controlling terminal present → prompt + repository path):

```
$ printf 'piped\n' | BLASTIT_FORGEJO_URL=http://127.0.0.1:8731 \
    script -qec 'sh docs/install.sh --mirror forgejo' /dev/null
Install Blastit 0.2.0 from the signed forgejo APT repository (main component)? [Y/n]
blastit: adding the signed Forgejo deb mirror (http://127.0.0.1:8731/api/packages/OffworldNexus/deb)
blastit: error: no public key at …/public.key: nothing to trust, so nothing is installed —
run: sh install.sh --mirror cloudsmith

$ printf '' | … setsid sh docs/install.sh --mirror forgejo     # no tty at all
blastit: adding the signed Forgejo deb mirror (http://127.0.0.1:8731/api/packages/OffworldNexus/deb)
blastit: error: no public key at …            # same refusal, no prompt
```

Publication (the `docs.yml` step, run verbatim against the real build):

```
$ make docs-check
No issues found
$ cmp docs/install.sh site/install.sh ; echo $?
cmp docs/install.sh site/install.sh: identical
$ curl -fsSL http://127.0.0.1:8734/install.sh | cmp - docs/install.sh
served bytes: identical
$ curl -fsSL http://127.0.0.1:8734/install.sh | sh -s -- --help
curl|sh -s -- --help: exit 0
$ sha256sum docs/install.sh site/install.sh
ae29c87c266713fbb7183f96afdbaf70965b3be38b3e4a654195a85834f92700  docs/install.sh
ae29c87c266713fbb7183f96afdbaf70965b3be38b3e4a654195a85834f92700  site/install.sh
```

```
$ make workflow-check
workflow-check: ok — 451 checks over 6 workflows and 1 actions, 0 problem(s)
```

`make fast-check` (final run): exit `0` — 163 `test result: ok`, no
`test result: FAILED`, no compiler/manifest/clippy warning.

## Follow-ups (out of scope here)

* **Stamp the embedded digest table.** `embedded_sha256` answers `-` for the
  four 0.2.0 targets, so a canonical-origin archive install verifies against the
  release's own `SHA256SUMS` today. The packaging/nightly workflows
  (`MB.42.3.4`) should write the four real digests into `docs/install.sh` when a
  tag is cut; the Docs job republishes the file either way.
* **Forgejo RPM signing.** The mirror path requires
  `/api/packages/OffworldNexus/rpm/public.key` and refuses without it (no
  `gpgcheck=0`). If Forgejo does not publish one, `--mirror forgejo` stays
  APT-only and Cloudsmith is the RPM path; that is a publishing decision, not a
  script one.
* **In-container repository legs** (`install.sh` default / `--user` /
  `--develop` inside `debian:trixie-slim` and `rockylinux:9`) belong to the
  packaging smoke harness (`scripts/test_packages.sh`): they need a signed local
  repository fixture, which is that slice's scope, not this one's.
* `docs.yml` publishes from `develop` and tags only, so a PR proves the copy and
  the pipe but not a deploy — unchanged from the existing workflow shape.

## Progress (run 2)

Applied the consolidated `review-arch` + `review-ux` findings to
`docs/install.sh`, `scripts/test_install.sh` (new), `.github/workflows/docs.yml`
and `Makefile`. `docs/install.sh` is 475 prod LOC (comments/blanks excluded);
`scripts/test_install.sh` is 141.

### BLOCKER 1 — a child can no longer eat the piped script

`as_root` now closes fd 0 on its two non-interactive branches
(`root) "$@" </dev/null`, `sudo) sudo -n "$@" </dev/null`; `sudo-tty` keeps
`</dev/tty`), and both `--version` captures are `… --version 2>/dev/null
</dev/null`. `fetch`, `sudo -n true` and every other non-interactive child get
`</dev/null` too. Proved by case (g) of the suite, and by running the same case
against the previous script (`/tmp/opencode/install.sh.bak`), where the child
read the *marker line* out of the piped script and the run failed:

```
# pre-fix script
exit=1
install-probe: child read stdin: echo install-probe: reached-the-end-of-the-script
blastit: error: dpkg refused …/fake.deb …
# fixed script (scripts/test_install.sh, case g)
install-test: ok (g) a piped 'curl | bash' run whose child reads stdin reaches the last line
```

### BLOCKER 2 — every recovery command is runnable as delivered

One constant, `SELF="curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh -s --"`,
is used by all fifteen "do it again" hints (no `sh install.sh` string remains).
The digest hint puts the variable on `sh`, where it is actually read:
`curl -fsSL … | BLASTIT_SHA256=<hex> sh -s -- --develop my-branch`.

### MAJOR 3 — a hostile archive cannot copy host files into the prefix

`unpack_archive` refuses any `l`/`h` member from `tar -tzvf` before extraction
(`… contains a link member (/etc/hostname): refusing to unpack it`),
`install_tree` skips `-h` entries in both loops, and the prefix-layout copy is
`cp -RP` (no dereference). Case (d) proves `/etc/hostname` never reaches the
prefix.

### MAJOR 4 — the digest table is a real, gated table

* Keyed by `BLASTIT_DEFAULT_VERSION` (one version to bump) instead of a second
  hardcoded `0.2.0`, with all four targets present so the release job can stamp
  them (`packages.yml`/`nightly.yml`, `MB.42.3.4`).
* `docs.yml` refuses a table not keyed by that constant, and **refuses a `v*`
  tag** whose rows are still `-`, so an unstamped release cannot ship.
* The `-` fallback is documented in the script header as the *same trust root as
  the download*: "proves the bytes arrived intact and proves nothing about who
  published them" — no independent assurance is claimed.

### MAJOR 5 — the root setup script is now named trust

Header paragraph states the anchor (package signatures; the setup script is TLS
only, compromise = root) and `note`s it before running:
`running Cloudsmith's repository setup script (HTTPS, not signature-verified; the packages it enables are GPG-verified)`.
`BLASTIT_SETUP_SCRIPT_SHA256` is honoured (verified with `verify_archive`), and
`fetch`'s `wget` fallback is `--https-only` for `https://` URLs.

### MAJOR 6 — automated coverage

New `scripts/test_install.sh` (real components only: a fixture directory served
by `uv run --no-project python -m http.server … --bind 127.0.0.1`, scratch
prefixes, no root, no package install), wired as `make install-test` and a
`fast-check` prerequisite; `make install-sh-syntax` (`sh -n` + `dash -n`) is a
`docs-unit` prerequisite.

```
$ sh scripts/test_install.sh
install-test: ok (a) --user installs into a scratch prefix; blastit --version runs (blastit 0.2.0 (fixture))
install-test: ok (b) a wrong digest exits 1 and installs nothing
install-test: ok (c) an overridden origin without a digest fails closed with zero requests
install-test: ok (d) an archive with a symlink member is refused before unpacking
install-test: ok (e) --help exits 0 with env -i (no root, no network)
install-test: ok (f) usage errors exit 2 (bad mirror, --nightly --user, path-shaped --version)
install-test: ok (g) a piped 'curl | bash' run whose child reads stdin reaches the last line
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

### MAJOR 7 / MINOR 8–19

* The happy path prints the next command:
  `note "next: blastit dev — https://offworldnexus.github.io/blastit/getting-started/"`,
  before the upgrade hint, on both the repository and archive paths.
* Prompts now read `Install Blastit 0.2.0 system-wide from the signed Blastit
  APT repository? (needs sudo; later updates are plain apt-get upgrade)` (RPM
  twin uses `$rpm_tool upgrade`).
* `--help` says the default needs root/sudo and names the `~/.local` fallback;
  `--nightly` says it switches the component and that re-running without it
  returns to releases.
* Forgejo refusal: `re-run without --mirror to use the default signed
  repository: $SELF`.
* Archive installs say `archive installs do not upgrade themselves: re-run this
  installer to update`; the PATH line is lowercase and mentions the startup
  file; the "no pinned digest" noise is now `checking <asset> against the
  release's SHA256SUMS`.
* Step lines describe the action, not the command
  (`adding the signed Blastit APT repository (hosted by Cloudsmith)`,
  `updating the package list, then installing blastit`, `installing blastit
  with dnf`).
* `--version` / `--develop` are validated against `[0-9A-Za-z._-]` with `..`
  rejected, so `--version '../../evil'` exits **2** instead of shaping a URL.
* `--nightly`'s component switch snapshots `/etc/apt/sources.list.d` first and
  only rewrites the file this run created, validating the line before writing.
* `sha256_of` output is checked for 64 hex characters with its own message
  (`no usable sha256 digest for <asset>: install coreutils …`).
* The prefix write probe is `mktemp`, no predictable `$$` path.
* `docs.yml`'s proof steps run `set -euo pipefail` and serve with `uv run
  --no-project python -m http.server 8731 --bind 127.0.0.1 --directory site`.

### Evidence (run 2)

```
$ sh -n docs/install.sh ; dash -n docs/install.sh
syntax ok

$ sh docs/install.sh --help     → exit 0, 0 bytes on stderr
$ cat docs/install.sh | sh -s -- --help → exit 0

$ make workflow-check
workflow-check: ok — 451 checks over 6 workflows and 1 actions, 0 problem(s)

# the docs.yml "Publish and prove install.sh" step, extracted and run verbatim
$ bash -euo pipefail publish-step.sh
install.sh: published for v0.2.0, served byte-identical, and 'curl … | sh -s -- --help' exits 0

# tag gate: an unstamped table on a v* tag is refused, a stamped one passes
GITHUB_REF_TYPE=tag … → "this is the refs/tags/v0.2.0 tag but the embedded digest table still holds '-' rows" (exit 1)
stamped throwaway copy → passes

$ make fast-check
exit 0 — install-test + 163 test results ok, 0 failed, no warnings
```

### Follow-ups (unchanged or newly precise)

* Stamp the four digests from `packages.yml`/`nightly.yml` (`MB.42.3.4`); the
  docs tag gate now fails until they are.
* The RPM nightly repo name belongs to that workflow: either it publishes
  `rpm-nightly` (the script's `CLOUDSMITH_RPM_NIGHTLY_REPO` default) or the
  workflow should set that variable where it can be asserted
  (`scripts/check_workflows.py`), which is outside this node's paths.
* `select_apt_component` matches `*.list`; a future Cloudsmith script writing a
  deb822 `*.sources` file would refuse with an actionable message instead of
  editing it. Worth handling when/if the vendor switches format.
