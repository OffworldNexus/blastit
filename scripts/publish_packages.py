#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Publish the native packages to the signed repositories (`MB.42.3.4`).

`.github/workflows/packages.yml` (a `v*` tag, `main`) and
`.github/workflows/nightly.yml` (`schedule`/`workflow_dispatch` on `develop`,
`nightly`) both call this file for everything that is not a build:

    scripts/publish_packages.py base-version
    scripts/publish_packages.py release-stamp
    scripts/publish_packages.py stamp --format deb --run 12 --sha <sha>
    scripts/publish_packages.py stamp --format rpm --run 12 --sha <sha>
    scripts/publish_packages.py push    --format deb --file <pkg> --component main
    scripts/publish_packages.py forgejo --format rpm --file <pkg> --component nightly
    scripts/publish_packages.py prune   --format deb --keep 7

Why a script instead of four shell lines in the YAML: the same code has to be
**provable without publishing anything**, and a step that only exists inside a
workflow can only be proved by publishing. Every mode has a `--dry-run` that
prints the exact command or request it would issue and touches no network, so
"the token is unset" and "the seventh-newest nightly is deleted" are reviewable
before a tag runs them.

# The two repositories (design §6.1)

* **Cloudsmith** (`offworld-nexus/${CLOUDSMITH_APT_REPO:-apt}` and
  `offworld-nexus/${CLOUDSMITH_RPM_REPO:-rpm}`) is the install path: the
  `.deb` goes to `any-distro/any-version` with `--component main|nightly`, the
  `.rpm` to the same coordinate with no `--component` (an RPM repository is one
  component). `CLOUDSMITH_API_KEY` is required for a real push and **fails
  closed** when it is missing: a release that cannot reach the repository is a
  red run, never a green one that shipped nothing.

  The upload is one HTTPS request (`POST
  https://upload.cloudsmith.io/<owner>/<repo>/{deb,rpm}/`, multipart
  `package_file` + the format's `distribution`/`release`/`component`), so the
  credential never leaves this process: nothing third-party runs beside the
  secret, and nothing has to be installed on a tag or a nightly. The endpoint is
  the one `cloudsmith push` itself calls; `CLOUDSMITH_UPLOAD_URL` overrides it for
  a rehearsal against a local endpoint. A shape the service rejects is a loud HTTP
  status naming the file — never a silent skip.
  `CLOUDSMITH_CLI` (e.g. `uvx --from cloudsmith-cli==1.12.0 cloudsmith`) is an
  opt-in fallback that runs the pinned CLI instead, with a **minimal** environment
  (only `PATH`, `HOME` and the three Cloudsmith variables: the Forgejo token is
  never handed to a child process).

* **Forgejo** (`FORGEJO_URL`, default `https://forge.offworld.nexus`) is a
  best-effort mirror: `POST .../api/packages/OffworldNexus/debian/pool/any-distro/<component>/upload`
  and `.../rpm/<component>/upload`, basic auth `Xowap:<token>`, multipart
  `package=@<file>`. When `FORGEJO_PACKAGES_TOKEN` is unset it prints one
  `::notice::` and exits **0**, so a fork's tag and a `workflow_dispatch`
  rehearsal stay green; a mirror that answers 500 is a `::warning::` and also
  exits 0, because Cloudsmith is the install path and the mirror never is.

No token is ever printed: the key travels in an `Authorization`/`X-Api-Key`
header (or the CLI branch's allowed environment) for Cloudsmith and in an
`Authorization` header for Forgejo, and every failure message names the status,
the file and the fix — never the credential.

# Retention counts *runs*, not packages

A nightly run builds **two** packages per format (`--matrix packages` → two
`.deb` and two `.rpm`, one per architecture) and both carry the same version
string. Retention therefore groups the candidates by the `(date, run)` the stamp
encodes and deletes whole groups beyond the newest `--keep` **groups**: deleting
"the 4 oldest packages" would otherwise split a run and leave an install that
works on one architecture only. `CLOUDSMITH_API_URL` overrides the API root for
the loopback proof.

# The nightly stamps

A nightly must sort *below* the release it was cut from, and each package format
spells that differently (design §6.1):

    .deb   0.2.0~nightly20261120.r12+gabc1234      (`~` sorts below 0.2.0)
    .rpm   0.2.0-0.nightly.20261120.r12.gabc1234   (`-0.` release prefix)

`stamp` prints the values as `key=value` lines, because the workflow appends
them straight to `$GITHUB_OUTPUT`; `--format both` prints both, which is what
the node's acceptance pastes. Every stamp is validated against the grammar
`scripts/build-deb.sh` and `scripts/build-rpm.sh` enforce — a stamp the builders
would refuse is refused here, where the message can say why.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import shlex
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request
import uuid
from datetime import UTC, datetime
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

CLOUDSMITH_OWNER = "offworld-nexus"
DEFAULT_APT_REPO = "apt"
DEFAULT_RPM_REPO = "rpm"
#: Where the API lives. Overridable so retention can be rehearsed against a local
#: endpoint instead of the real one.
DEFAULT_API_URL = "https://api.cloudsmith.io/v1"

#: Where a package is uploaded: the endpoint `cloudsmith push` calls. Overridable
#: for the same reason as the API root — the gate drives the real request (method,
#: path, auth header, multipart parts) against loopback, rather than asserting a
#: URL it also wrote.
DEFAULT_UPLOAD_URL = "https://upload.cloudsmith.io"

#: Retention reads the repository's whole package list. Ten pages of 100 is more
#: nightlies than a year of runs, so a full tenth page means this module's
#: assumptions are stale: it refuses rather than deleting what it cannot see.
MAX_PAGES = 10
PAGE_SIZE = 100

#: The RPM `Release:` a *tagged release* ships (`make packages`'s default, and the
#: value `build-rpm.sh` is handed). It lives here so both workflows read all four
#: version fields from `release-stamp`/`stamp` instead of writing `rpm_release=1`
#: into a YAML step.
RELEASE_RELEASE = "1"

FORGEJO_DEFAULT_URL = "https://forge.offworld.nexus"
FORGEJO_OWNER_DEFAULT = "OffworldNexus"
FORGEJO_USER_DEFAULT = "Xowap"

#: The version grammar `scripts/build-deb.sh` and `scripts/build-rpm.sh` enforce
#: (a Debian version, a Debian revision, an RPM version and an RPM release). A
#: stamp outside it would be refused by the builder after the whole bundle was
#: assembled, so it is refused here instead — and the packaging-gates suite proves
#: the two agree by handing the printed stamp to both builders.
DEB_VERSION = re.compile(r"^[0-9][0-9A-Za-z.+~]*$")
RPM_VERSION = re.compile(r"^[0-9][0-9A-Za-z.+~]*$")
RPM_RELEASE = re.compile(r"^[0-9][0-9A-Za-z.+~_]*$")
SHA = re.compile(r"^[0-9a-f]{7,40}$")
DAY = re.compile(r"^[0-9]{8}$")
RUN = re.compile(r"^[0-9]+$")

#: Nightlies kept per repository (design §6.1: "keeping newest `7`").
DEFAULT_KEEP = 7


def notice(message: str) -> None:
    """A GitHub annotation: visible in the run summary, never fatal."""
    print(f"::notice::{message}", flush=True)


def warning(message: str) -> None:
    print(f"::warning::{message}", flush=True)


def fail(message: str, code: int = 1) -> int:
    """Refuse, naming the file and the fix. A credential is never part of it."""
    print(f"error: {message}", file=sys.stderr)
    return code


def base_version(repo: Path = REPO) -> str:
    """`[workspace.package] version` from `Cargo.toml`.

    One reader for the number both workflows need: `packages.yml` publishes it,
    `nightly.yml` stamps around it, and a second `awk` in a YAML step is the copy
    that drifts from `publish-pypi.yml`'s.
    """
    with (repo / "Cargo.toml").open("rb") as handle:
        document = tomllib.load(handle)
    version = str(
        ((document.get("workspace") or {}).get("package") or {}).get("version") or ""
    )
    if not version:
        raise SystemExit("error: Cargo.toml carries no [workspace.package] version")
    return version


# --- the nightly stamps -------------------------------------------------------


def stamp(fmt: str, base: str, day: str, run: str, sha: str) -> dict[str, str]:
    """The nightly version/release pair for one package format (design §6.1).

    The Debian version keeps the whole stamp (`~` sorts below the release, so a
    nightly never shadows `0.2.0`); the RPM keeps `Version: 0.2.0` and puts
    `0.nightly.<date>.r<run>.g<sha>` in `Release:`, because a `-` is the field
    separator there. `+` is legal in an RPM version but the design asks for `.`
    so no builder has to escape it.
    """
    short = sha[:7]
    if fmt == "deb":
        values = {
            "deb_version": f"{base}~nightly{day}.r{run}+g{short}",
            "deb_release": "",
        }
    elif fmt == "rpm":
        values = {
            "rpm_version": base,
            "rpm_release": f"0.nightly.{day}.r{run}.g{short}",
        }
    else:  # pragma: no cover - argparse restricts the value
        raise SystemExit(f"error: unknown package format {fmt!r}")

    problems: list[str] = []
    if not RPM_VERSION.fullmatch(base):
        problems.append(
            f"the base version {base!r} is not a version either builder accepts "
            "(letters, digits, '.', '+', '~', starting with a digit)"
        )
    if fmt == "deb":
        value = values["deb_version"]
        if not DEB_VERSION.fullmatch(value):
            problems.append(f"{value!r} is not a Debian version (§ build-deb.sh)")
        if "~" not in value:
            problems.append(
                f"{value!r} has no `~`: a nightly would sort above its release"
            )
    else:
        if not RPM_VERSION.fullmatch(values["rpm_version"]):
            problems.append(f"{values['rpm_version']!r} is not an RPM Version")
        release = values["rpm_release"]
        if not RPM_RELEASE.fullmatch(release):
            problems.append(f"{release!r} is not an RPM Release (§ build-rpm.sh)")
        if not release.startswith("0."):
            problems.append(
                f"{release!r} does not start with `0.`: a nightly would upgrade the release"
            )
    if problems:
        raise SystemExit("error: " + "; ".join(problems))
    return values


def release_stamp(base: str) -> dict[str, str]:
    """The four version keys a **tagged release** carries, from one place.

    The workflows read `deb_version`/`deb_release`/`rpm_version`/`rpm_release` and
    nothing else: a `rpm_release=1` typed into a YAML step would be a fifth copy
    of the value `build-rpm.sh` is handed, and the nightly stamp and the release
    stamp would then drift from the same fields.
    """
    if not RPM_VERSION.fullmatch(base):
        raise SystemExit(
            f"error: the base version {base!r} is not a version either builder "
            "accepts (letters, digits, '.', '+', '~', starting with a digit)"
        )
    return {
        "deb_version": base,
        "deb_release": "",
        "rpm_version": base,
        "rpm_release": RELEASE_RELEASE,
    }


# --- Cloudsmith ---------------------------------------------------------------


def cloudsmith_repo(fmt: str, environ: dict[str, str]) -> str:
    """`offworld-nexus/<repo>` — the repository this format publishes to."""
    name = (
        (environ.get("CLOUDSMITH_APT_REPO") or DEFAULT_APT_REPO)
        if fmt == "deb"
        else (environ.get("CLOUDSMITH_RPM_REPO") or DEFAULT_RPM_REPO)
    )
    return f"{CLOUDSMITH_OWNER}/{name}"


def cloudsmith_push(
    fmt: str, path: Path, component: str, environ: dict[str, str]
) -> list[str]:
    """The `cloudsmith push` command the design names (design §6.1).

    Printed by every `--dry-run` (so the design's command stays visible and the
    gate can assert it), and **executed** only when `CLOUDSMITH_CLI` opts into the
    CLI: the default is the upload API below, where no third-party binary runs
    beside the credential. The `.deb` names a component (`main` or `nightly`); the
    `.rpm` does not, because an RPM repository is a single component.
    """
    cli = shlex.split(environ.get("CLOUDSMITH_CLI") or "cloudsmith")
    # One coordinate, one argument: `cloudsmith push <format> <owner>/<repo>/<distro>/<version>`.
    coordinate = f"{cloudsmith_repo(fmt, environ)}/any-distro/any-version"
    if fmt == "deb":
        return [
            *cli,
            "push",
            "deb",
            coordinate,
            str(path),
            "--component",
            component,
            "--republish",
        ]
    return [*cli, "push", "rpm", coordinate, str(path), "--republish"]


def upload_url(fmt: str, environ: dict[str, str]) -> str:
    """`POST https://upload.cloudsmith.io/<owner>/<repo>/{deb,rpm}/`.

    The endpoint `cloudsmith push` itself calls; `CLOUDSMITH_UPLOAD_URL` overrides
    it so the gate drives the real request against loopback.
    """
    base = (environ.get("CLOUDSMITH_UPLOAD_URL") or DEFAULT_UPLOAD_URL).rstrip("/")
    return f"{base}/{cloudsmith_repo(fmt, environ)}/{fmt}/"


def upload_fields(fmt: str, component: str) -> list[tuple[str, str]]:
    """The text fields the upload endpoint takes beside `package_file`.

    `any-distro`/`any-version` are the coordinate the design names; `component` is
    the Debian channel (`main` or `nightly`) and is absent for RPM, whose
    repository is one component; `republish` is what folds a re-run of a
    half-finished release into the file the repository already has.
    """
    fields = [("distribution", "any-distro"), ("release", "any-version")]
    if fmt == "deb":
        fields.append(("component", component))
    fields.append(("republish", "true"))
    return fields


def cli_environment(environ: dict[str, str]) -> dict[str, str]:
    """The **only** variables a child `cloudsmith` inherits.

    Review item 6: `{**environ}` handed `FORGEJO_PACKAGES_TOKEN` — and every other
    secret in the job — to a third-party binary. A child gets what it needs to run
    and nothing else, which is one more reason the default path is the API call.
    """
    allowed = (
        "PATH",
        "HOME",
        "LANG",
        "CLOUDSMITH_API_KEY",
        "CLOUDSMITH_APT_REPO",
        "CLOUDSMITH_RPM_REPO",
    )
    return {key: environ[key] for key in allowed if key in environ}


def cmd_push(args: argparse.Namespace, environ: dict[str, str]) -> int:
    path = Path(args.file)
    if not path.is_file():
        return fail(f"{path} does not exist: publish the package this run built")
    url = upload_url(args.format, environ)
    fields = upload_fields(args.format, args.component)
    if args.dry_run:
        rendered = " ".join(f"{name}={value}" for name, value in fields)
        print(
            f"cloudsmith: would POST {url} (Authorization: Bearer <token>) "
            f"multipart package_file=@{path} {rendered}",
            flush=True,
        )
        print(
            "cloudsmith: equivalent CLI: "
            + shlex.join(cloudsmith_push(args.format, path, args.component, environ)),
            flush=True,
        )
        return 0
    key = environ.get("CLOUDSMITH_API_KEY") or ""
    if not key:
        return fail(
            "CLOUDSMITH_API_KEY is not set: the signed repositories are the install "
            f"path, so {path.name} may not be silently dropped — set the repository "
            "secret (nothing was uploaded)"
        )
    if environ.get("CLOUDSMITH_CLI"):
        return push_with_cli(args, path, key, environ)
    boundary = f"----blastit{uuid.uuid4().hex}"
    parts: list[tuple[str, str | None, bytes]] = [
        ("package_file", path.name, path.read_bytes())
    ]
    parts += [(name, None, value.encode()) for name, value in fields]
    request = urllib.request.Request(
        url, data=multipart_body(parts, boundary), method="POST"
    )
    request.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")
    # The same credential reaches the upload service as a bearer token and the v1
    # API as `X-Api-Key`; sending both means either shape is accepted, and neither
    # is ever printed.
    request.add_header("Authorization", f"Bearer {key}")
    request.add_header("X-Api-Key", key)
    try:
        with urllib.request.urlopen(request, timeout=args.timeout) as response:
            status = response.status
    except urllib.error.HTTPError as error:
        hint = ""
        if error.code in (400, 409):
            hint = (
                " — the repository refused the file it already has (bump the version, "
                "or delete that package in Cloudsmith first)"
            )
        return fail(
            f"uploading {path.name} to {url} failed (HTTP {error.code}){hint}: nothing "
            "was published for it"
        )
    except (urllib.error.URLError, OSError) as error:
        return fail(
            f"uploading {path.name} to {url} failed ({type(error).__name__}): nothing "
            "was published for it"
        )
    print(
        f"cloudsmith: published {path.name} to {cloudsmith_repo(args.format, environ)} "
        f"(component {args.component}, HTTP {status})",
        flush=True,
    )
    return 0


def push_with_cli(
    args: argparse.Namespace, path: Path, key: str, environ: dict[str, str]
) -> int:
    """The opt-in `CLOUDSMITH_CLI` path: the pinned CLI, with an allowlisted env."""
    command = cloudsmith_push(args.format, path, args.component, environ)
    try:
        completed = subprocess.run(
            command,
            env=cli_environment({**environ, "CLOUDSMITH_API_KEY": key}),
            check=False,
        )
    except FileNotFoundError:
        return fail(
            f"the CLI {command[0]!r} is not on PATH: install it in the job (pinned), "
            "or drop CLOUDSMITH_CLI so this script uploads over the API instead"
        )
    if completed.returncode != 0:
        return fail(
            f"`{command[0]} push {args.format}` exited {completed.returncode} for "
            f"{path.name}: nothing was published for it"
        )
    print(
        f"cloudsmith: published {path.name} to {cloudsmith_repo(args.format, environ)} "
        f"(component {args.component}, via {command[0]})",
        flush=True,
    )
    return 0


# --- Forgejo mirror -----------------------------------------------------------


def forgejo_url(fmt: str, component: str, environ: dict[str, str]) -> str:
    """The mirror's upload endpoint for this format and component (design §6.1)."""
    base = (environ.get("FORGEJO_URL") or FORGEJO_DEFAULT_URL).rstrip("/")
    owner = environ.get("FORGEJO_OWNER") or FORGEJO_OWNER_DEFAULT
    if fmt == "deb":
        return f"{base}/api/packages/{owner}/debian/pool/any-distro/{component}/upload"
    return f"{base}/api/packages/{owner}/rpm/{component}/upload"


def multipart_body(parts: list[tuple[str, str | None, bytes]], boundary: str) -> bytes:
    """A `multipart/form-data` body from `(name, filename, data)` parts.

    A `None` filename is a plain text field, which is what Cloudsmith's upload API
    wants for `distribution`/`release`/`component`; Forgejo wants exactly one
    file part (`package`) and nothing else.
    """
    crlf = b"\r\n"
    body = bytearray()
    for name, filename, data in parts:
        disposition = f'Content-Disposition: form-data; name="{name}"'
        if filename is not None:
            disposition += f'; filename="{filename}"'
        body += b"--" + boundary.encode() + crlf
        body += disposition.encode() + crlf
        if filename is not None:
            body += b"Content-Type: application/octet-stream" + crlf
        body += crlf
        body += data + crlf
    body += b"--" + boundary.encode() + b"--" + crlf
    return bytes(body)


def cmd_forgejo(args: argparse.Namespace, environ: dict[str, str]) -> int:
    path = Path(args.file)
    if not path.is_file():
        return fail(f"{path} does not exist: mirror the package this run built")
    url = forgejo_url(args.format, args.component, environ)
    user = environ.get("FORGEJO_USER") or FORGEJO_USER_DEFAULT
    if args.dry_run:
        print(
            f"forgejo: would POST {url} (basic auth {user}:<token>, "
            f"multipart package=@{path})",
            flush=True,
        )
        return 0
    token = environ.get("FORGEJO_PACKAGES_TOKEN") or ""
    if not token:
        notice(
            "forgejo mirror skipped: FORGEJO_PACKAGES_TOKEN is unset, so nothing is "
            "mirrored (the Cloudsmith repositories are the install path)"
        )
        return 0
    boundary = f"----blastit{uuid.uuid4().hex}"
    request = urllib.request.Request(
        url,
        data=multipart_body([("package", path.name, path.read_bytes())], boundary),
        method="POST",
    )
    request.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")
    request.add_header(
        "Authorization",
        "Basic " + base64.b64encode(f"{user}:{token}".encode()).decode(),
    )
    try:
        with urllib.request.urlopen(request, timeout=args.timeout) as response:
            status = response.status
    except urllib.error.HTTPError as error:
        # Best effort: the mirror is never the install path, so its failure is a
        # warning and exits 0. Neither branch prints the response body or the
        # request headers, so a credential cannot leak into a log line.
        warning(
            f"forgejo mirror upload failed (HTTP {error.code}) for {path.name}: the "
            "mirror is best effort, so this run continues — Cloudsmith is the install path"
        )
        return 0
    except (urllib.error.URLError, OSError) as error:
        warning(
            f"forgejo mirror upload failed ({type(error).__name__}) for {path.name}: the "
            "mirror is best effort, so this run continues — Cloudsmith is the install path"
        )
        return 0
    print(f"forgejo: mirrored {path.name} to {url} (HTTP {status})", flush=True)
    return 0


# --- retention ----------------------------------------------------------------


def api_url(environ: dict[str, str]) -> str:
    return (environ.get("CLOUDSMITH_API_URL") or DEFAULT_API_URL).rstrip("/")


def api_call(url: str, key: str, method: str = "GET") -> object:
    request = urllib.request.Request(url, method=method)
    request.add_header("X-Api-Key", key)
    request.add_header("Accept", "application/json")
    with urllib.request.urlopen(request, timeout=60) as response:
        raw = response.read()
    return json.loads(raw) if raw.strip() else None


def list_packages(
    fmt: str, key: str, environ: dict[str, str], page_size: int = PAGE_SIZE
) -> list[dict]:
    """Every package in this format's repository, newest pages first.

    It pages until the API answers with fewer rows than it was asked for, and
    **refuses** at `MAX_PAGES` full pages: a full last page is not a promise that
    there is nothing else, and a retention run that cannot see the whole list
    deletes on a partial view. `MAX_PAGES × PAGE_SIZE` is far beyond a year of
    nightlies, so reaching it means this module's assumptions changed.
    """
    repo = cloudsmith_repo(fmt, environ)
    packages: list[dict] = []
    for page in range(1, MAX_PAGES + 1):
        url = f"{api_url(environ)}/packages/{repo}/?page={page}&page_size={page_size}"
        batch = api_call(url, key) or []
        if not isinstance(batch, list) or not batch:
            return packages
        packages.extend(item for item in batch if isinstance(item, dict))
        if len(batch) < page_size:
            return packages
    raise SystemExit(
        f"{cloudsmith_repo(fmt, environ)} answered {MAX_PAGES} full pages "
        f"({MAX_PAGES * page_size} packages): retention cannot see the whole "
        "repository, so it refuses to delete anything (raise MAX_PAGES deliberately)"
    )


def nightly_order(fmt: str, package: dict) -> tuple[str, int, str] | None:
    """`(date, run, version)` for a nightly, `None` for the release it shadows.

    Newest-first by *version* is not newest-first by string: `r12` sorts below
    `r9` lexicographically, and a nightly built at 23:59 must not outlive the one
    built at 00:01 the next day. The two numbers the stamp encodes are read back
    instead of re-parsed from a filename.
    """
    version = str(package.get("version") or "")
    release = str(package.get("release") or "")
    combined = f"{version}-{release}" if release else version
    pattern = (
        r"~nightly(\d{8})\.r(\d+)" if fmt == "deb" else r"\.nightly\.(\d{8})\.r(\d+)"
    )
    match = re.search(pattern, combined)
    if match is None:
        return None
    return (match.group(1), int(match.group(2)), combined)


def nightly_groups(
    fmt: str, packages: list[dict]
) -> list[tuple[tuple[str, int], list[dict]]]:
    """Newest-first `((date, run), packages)` groups — one group per nightly **run**.

    One run ships **two** packages per format (an `amd64` and an `arm64`), and
    they carry identical version strings, so a per-package ordering cuts the tie
    arbitrarily: `--keep 7` over five runs kept three and a half, and deleted
    `r2-arm64` while keeping `r2-amd64`. Retention keeps *runs*, so the
    tie-breaker never runs.
    """
    groups: dict[tuple[str, int], list[dict]] = {}
    for package in packages:
        order = nightly_order(fmt, package)
        if order is None:
            continue
        groups.setdefault((order[0], order[1]), []).append(package)
    return sorted(groups.items(), key=lambda item: item[0], reverse=True)


def cmd_prune(args: argparse.Namespace, environ: dict[str, str]) -> int:
    key = environ.get("CLOUDSMITH_API_KEY") or ""
    if not key:
        notice(
            "prune skipped: CLOUDSMITH_API_KEY is unset, so the repository's package "
            "list cannot be read (nothing was deleted)"
        )
        return 0
    try:
        packages = list_packages(args.format, key, environ)
    except (urllib.error.URLError, OSError, json.JSONDecodeError) as error:
        return fail(
            f"cannot list {cloudsmith_repo(args.format, environ)} packages "
            f"({type(error).__name__}): retention did not run, so nothing was deleted"
        )
    groups = nightly_groups(args.format, packages)
    doomed = groups[args.keep :]
    kept_packages = sum(len(items) for _, items in groups[: args.keep])
    doomed_packages = sum(len(items) for _, items in doomed)
    print(
        f"prune: {args.format}: {len(groups)} nightly run(s), keeping the newest "
        f"{min(args.keep, len(groups))} ({kept_packages} package(s)), deleting "
        f"{len(doomed)} run(s) ({doomed_packages} package(s))",
        flush=True,
    )
    if args.dry_run:
        for (day, run), items in doomed:
            print(
                f"prune: would delete run r{run} of {day} "
                f"({len(items)} package(s): "
                + ", ".join(str(item.get("slug") or item.get("name")) for item in items)
                + ")",
                flush=True,
            )
        return 0
    repo = cloudsmith_repo(args.format, environ)
    deleted = 0
    for (day, run), items in doomed:
        removed = 0
        for package in items:
            slug = str(package.get("slug") or "")
            if not slug:
                warning(
                    f"prune: a r{run}/{day} package carries no slug: cannot delete it "
                    "by name"
                )
                continue
            try:
                api_call(
                    f"{api_url(environ)}/packages/{repo}/{slug}/", key, method="DELETE"
                )
            except urllib.error.HTTPError as error:
                warning(f"prune: deleting {slug} failed (HTTP {error.code}): kept")
                continue
            except (urllib.error.URLError, OSError) as error:
                warning(f"prune: deleting {slug} failed ({type(error).__name__}): kept")
                continue
            deleted += 1
            removed += 1
            print(f"prune: deleted run r{run} of {day}: {slug}", flush=True)
        if removed != len(items):
            warning(
                f"prune: run r{run} of {day} is only {removed}/{len(items)} deleted: "
                "it is an install that works on one architecture only"
            )
    print(
        f"prune: {args.format}: deleted {deleted} of {doomed_packages} package(s)",
        flush=True,
    )
    return 0


# --- the command line ---------------------------------------------------------


def main(argv: list[str] | None = None, environ: dict[str, str] | None = None) -> int:
    env = dict(os.environ if environ is None else environ)
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)

    commands.add_parser("base-version", help="print [workspace.package] version")

    release_parser = commands.add_parser(
        "release-stamp", help="print the four version keys a tagged release carries"
    )
    release_parser.add_argument("--base-version", default="")

    stamp_parser = commands.add_parser("stamp", help="print the nightly version keys")
    stamp_parser.add_argument(
        "--format", choices=("deb", "rpm", "both"), default="both"
    )
    stamp_parser.add_argument("--base-version", default="")
    stamp_parser.add_argument(
        "--date",
        default="",
        help="YYYYMMDD (default: today, UTC) — the day the package is cut",
    )
    stamp_parser.add_argument("--run", default="0", help="the workflow run number")
    stamp_parser.add_argument(
        "--sha", default="", help="the commit the nightly is cut from"
    )

    for name, handler in (("push", cmd_push), ("forgejo", cmd_forgejo)):
        publish = commands.add_parser(name)
        publish.add_argument("--format", choices=("deb", "rpm"), required=True)
        publish.add_argument("--file", required=True)
        publish.add_argument("--component", choices=("main", "nightly"), default="main")
        publish.add_argument(
            "--dry-run",
            action="store_true",
            help="print the command/request and touch no network",
        )
        publish.add_argument("--timeout", type=int, default=60)
        publish.set_defaults(handler=handler)

    prune = commands.add_parser("prune", help="keep the newest nightlies")
    prune.add_argument("--format", choices=("deb", "rpm"), required=True)
    prune.add_argument(
        "--keep",
        type=int,
        default=DEFAULT_KEEP,
        help="how many nightly runs to keep (a run ships one package per architecture)",
    )
    prune.add_argument("--dry-run", action="store_true")
    prune.set_defaults(handler=cmd_prune, timeout=60)

    args = parser.parse_args(argv)

    if args.command == "base-version":
        print(base_version())
        return 0
    if args.command == "release-stamp":
        base = args.base_version or base_version()
        try:
            values = release_stamp(base)
        except SystemExit as error:
            return fail(str(error).replace("error: ", ""), 2)
        for key, value in values.items():
            print(f"{key}={value}")
        return 0
    if args.command == "stamp":
        day = args.date or datetime.now(UTC).strftime("%Y%m%d")
        if not DAY.fullmatch(day):
            return fail(f"--date {day!r} is not YYYYMMDD")
        if not RUN.fullmatch(str(args.run)):
            return fail(f"--run {args.run!r} is not a run number")
        sha = args.sha
        if not SHA.fullmatch(sha):
            return fail(
                f"--sha {sha!r} is not a commit hash (7-40 lowercase hex): the nightly "
                "stamp has to name the commit it was cut from"
            )
        base = args.base_version or base_version()
        try:
            formats = ("deb", "rpm") if args.format == "both" else (args.format,)
            lines: dict[str, str] = {}
            for fmt in formats:
                lines.update(stamp(fmt, base, day, str(args.run), sha))
        except SystemExit as error:
            return fail(str(error).replace("error: ", ""), 2)
        for key, value in lines.items():
            print(f"{key}={value}")
        return 0
    if args.command == "prune" and args.keep < 1:
        return fail(
            "--keep must be at least 1: retention may never delete every nightly"
        )
    return args.handler(args, env)


if __name__ == "__main__":
    raise SystemExit(main())
