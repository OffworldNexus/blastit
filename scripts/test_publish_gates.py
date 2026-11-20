#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Positive + mutated-negative cases for the publishing path (`MB.42.3.4`).

Split out of `scripts/test_packaging_gates.py` (which imports and calls this
module, so one command still runs everything): the packaging gate had grown to
2 700 lines and half of it was about *publishing* rather than packaging.

Six claims, each a way the two workflows could ship nothing, the wrong thing, or
a credential:

* the two nightly stamps are the design's strings, `release-stamp` prints the four
  release fields the tag path reads, and **both builders accept the stamps** —
  their version checks run before the bundle check, so "no such bundle" is the
  accepted answer, and a version they refuse is refused;
* `dpkg --compare-versions` agrees a nightly sorts **below** its release (and one
  night before the next), which is the whole reason the `.deb` stamp has a `~`;
  `rpmdev-vercmp`, where it exists, agrees the `-0.` release sorts below `1`;
* the Cloudsmith upload is one `POST https://upload.cloudsmith.io/<owner>/<repo>/
  {deb,rpm}/` with `package_file` plus the format's fields — recorded by a real
  loopback server, with the key in headers and in no log line;
* `CLOUDSMITH_CLI` is an opt-in fallback and runs with a **minimal** environment
  (`FORGEJO_PACKAGES_TOKEN` never reaches the child);
* an unset `CLOUDSMITH_API_KEY` fails closed, and an unset
  `FORGEJO_PACKAGES_TOKEN` prints one `::notice::` and exits 0;
* retention keeps the newest `--keep` **runs**: a run ships one package per
  architecture, so a per-package sort splits it. The case is 5 runs × 2 arches,
  and it also proves `r12` outranks `r9` (a string sort says the opposite) and
  that a repository whose list cannot be read is refused rather than pruned.
"""

from __future__ import annotations

import base64
import http.server
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PUBLISH_PACKAGES = REPO / "scripts" / "publish_packages.py"
BUILD_DEB = REPO / "scripts" / "build-deb.sh"
BUILD_RPM = REPO / "scripts" / "build-rpm.sh"

#: The key the cases use. It never leaves the child's headers, so a match in any
#: output is the failure these cases exist for.
FAKE_KEY = "not-a-real-cloudsmith-key"

#: The upload root the dry-run prints when nothing overrides it.
PUBLISH_UPLOAD = "https://upload.cloudsmith.io"


def _expect(condition: bool, message: str, detail: str = "") -> None:
    if not condition:
        suffix = f"\n{detail}" if detail else ""
        raise AssertionError(f"{message}{suffix}")


def _loopback(
    packages: list[dict],
) -> tuple[http.server.ThreadingHTTPServer, str, list[dict]]:
    """A real HTTP server on loopback, recording every request it receives."""
    seen: list[dict] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def _record(self) -> None:
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length) if length else b""
            seen.append(
                {
                    "method": self.command,
                    "path": self.path,
                    "headers": {
                        key.lower(): value for key, value in self.headers.items()
                    },
                    "body": body,
                }
            )

        def do_POST(self) -> None:
            self._record()
            self.send_response(201)
            self.send_header("Content-Length", "0")
            self.end_headers()

        def do_DELETE(self) -> None:
            self._record()
            self.send_response(200)
            self.send_header("Content-Length", "0")
            self.end_headers()

        def do_GET(self) -> None:
            self._record()
            payload = json.dumps(packages).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *args: object) -> None:
            return

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, f"http://127.0.0.1:{server.server_address[1]}", seen


def _fake_cli(directory: Path) -> tuple[Path, Path]:
    """A stand-in `cloudsmith` that records its argv and the env it inherited.

    The opt-in `CLOUDSMITH_CLI` branch is the one place a third party holds the
    key, so the case proves two things at once: the exact argv (the design's
    command) and that the child saw *only* the allowlisted variables. The record
    path is baked into the script because the child's environment is filtered —
    which is the property under test.
    """
    script = directory / "cloudsmith"
    recorded = directory / "cloudsmith-child.txt"
    script.write_text(
        "#!/bin/sh\n"
        f'printf "ARG %s\\n" "$@" > "{recorded}"\n'
        "env | sed -n "
        "-e 's/^CLOUDSMITH_API_KEY=.*/ENV CLOUDSMITH_API_KEY/p' "
        "-e 's/^FORGEJO_PACKAGES_TOKEN=.*/ENV FORGEJO_PACKAGES_TOKEN/p' "
        f'>> "{recorded}"\n'
        "exit 0\n",
        encoding="utf-8",
    )
    script.chmod(0o755)
    return script, recorded


def _deb_nightly(day: int, run: int, arch: str) -> dict:
    """One package of one nightly **run**: two of these share a version string."""
    return {
        "version": f"0.2.0~nightly202611{day:02d}.r{run}+gabc1234",
        "slug": f"deb-{arch}-r{run}-day{day:02d}",
        "name": f"blastit_{arch}_0.2.0~nightly202611{day:02d}.r{run}",
    }


def test_publish_cases() -> int:
    """The stamps, the upload, the mirror and retention — all without a publish."""
    cases_run = 0
    with tempfile.TemporaryDirectory(prefix="blastit-publish-") as tmp:
        root = Path(tmp)
        package = root / "blastit_0.2.0_amd64.deb"
        package.write_bytes(b"!<arch>\ndebian-binary\n")
        rpm_package = root / "blastit-0.2.0-1.x86_64.rpm"
        rpm_package.write_bytes(b"\xed\xab\xee\xdb rpm-payload\n")

        def run(*args: str, env: dict[str, str] | None = None):
            return subprocess.run(
                [sys.executable, str(PUBLISH_PACKAGES), *args],
                capture_output=True,
                text=True,
                env={**os.environ, **(env or {})},
                check=False,
            )

        # 1. The nightly stamps, exactly.
        stamped = run(
            "stamp",
            "--format",
            "both",
            "--base-version",
            "0.2.0",
            "--date",
            "20261120",
            "--run",
            "12",
            "--sha",
            "abc1234def5678",
        )
        _expect(
            stamped.returncode == 0,
            "the nightly stamp must succeed",
            stamped.stdout + stamped.stderr,
        )
        values = dict(
            line.split("=", 1) for line in stamped.stdout.splitlines() if "=" in line
        )
        _expect(
            values
            == {
                "deb_version": "0.2.0~nightly20261120.r12+gabc1234",
                "deb_release": "",
                "rpm_version": "0.2.0",
                "rpm_release": "0.nightly.20261120.r12.gabc1234",
            },
            "the two nightly stamps must be the design's strings (design §6.1)",
            stamped.stdout,
        )
        cases_run += 1

        # 2. ... and the release stamp both workflows read instead of writing
        #    `rpm_release=1` into a YAML step.
        release = run("release-stamp", "--base-version", "0.2.0")
        _expect(
            release.returncode == 0
            and dict(
                line.split("=", 1)
                for line in release.stdout.splitlines()
                if "=" in line
            )
            == {
                "deb_version": "0.2.0",
                "deb_release": "",
                "rpm_version": "0.2.0",
                "rpm_release": "1",
            },
            "release-stamp must print the four release fields from one place",
            release.stdout + release.stderr,
        )
        cases_run += 1

        bad_base = run(
            "stamp",
            "--format",
            "deb",
            "--base-version",
            "0.2.0-rc1",
            "--run",
            "1",
            "--sha",
            "abc1234def",
        )
        _expect(
            bad_base.returncode == 2 and "not a version" in bad_base.stderr,
            "a stamp the builders would refuse must be refused where it is made",
            bad_base.stdout + bad_base.stderr,
        )
        cases_run += 1

        # 3. The builders' own version checks are what the stamp must survive.
        if shutil.which("readelf") and shutil.which("dpkg-deb"):
            missing = root / "no-such-bundle"
            res = subprocess.run(
                [
                    "bash",
                    str(BUILD_DEB),
                    "--version",
                    values["deb_version"],
                    "--bundle",
                    str(missing),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            _expect(
                res.returncode == 1
                and "is not a staged bundle" in res.stderr
                and "not a Debian version" not in res.stderr,
                "build-deb.sh must accept the nightly `.deb` version (with a literal `~`)",
                res.stdout + res.stderr,
            )
            cases_run += 1
            res = subprocess.run(
                [
                    "bash",
                    str(BUILD_RPM),
                    "--version",
                    values["rpm_version"],
                    "--release",
                    values["rpm_release"],
                    "--bundle",
                    str(missing),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            _expect(
                res.returncode == 1
                and "is not a staged bundle" in res.stderr
                and "not an RPM" not in res.stderr,
                "build-rpm.sh must accept `Version: 0.2.0` + `Release: 0.nightly…`",
                res.stdout + res.stderr,
            )
            cases_run += 1
            refused = subprocess.run(
                [
                    "bash",
                    str(BUILD_DEB),
                    "--version",
                    "0.2.0-rc1",
                    "--bundle",
                    str(missing),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            _expect(
                refused.returncode == 1 and "not a Debian version" in refused.stderr,
                "the builder's version check must still refuse a `-rc1` suffix",
                refused.stdout + refused.stderr,
            )
            cases_run += 1
        else:
            print(
                "publish gates: builder version acceptance skipped (no readelf/dpkg-deb)"
            )

        # 4. Ordering, measured instead of asserted: `~` before the release, and
        #    one night before the next. `dpkg` ships with the gate's own host.
        dpkg = shutil.which("dpkg")
        if dpkg:
            for left, right in (
                (values["deb_version"], "0.2.0"),
                (
                    "0.2.0~nightly20261119.r7+gabc1234",
                    "0.2.0~nightly20261120.r7+gabc1234",
                ),
            ):
                res = subprocess.run(
                    [dpkg, "--compare-versions", left, "lt", right],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                _expect(
                    res.returncode == 0,
                    f"dpkg must agree that {left} sorts below {right}: that is the "
                    "whole reason the `.deb` stamp carries a `~`",
                    res.stdout + res.stderr,
                )
                cases_run += 1
        else:
            print("publish gates: dpkg --compare-versions skipped (no dpkg)")

        rpmdev = shutil.which("rpmdev-vercmp")
        if rpmdev:
            res = subprocess.run(
                [rpmdev, "0.2.0-1", values["rpm_release"]],
                capture_output=True,
                text=True,
                check=False,
            )
            # rpmdev-vercmp: 0 equal, 11 first newer, 12 first older.
            _expect(
                res.returncode == 11,
                "the released `Release: 1` must be newer than the nightly's `0.…`",
                res.stdout + res.stderr,
            )
            cases_run += 1
        else:
            print("publish gates: rpmdev-vercmp skipped (rpm not installed)")

        # 5. The upload is one HTTP request; the key rides in headers only.
        server, url, seen = _loopback([])
        try:
            pushed = run(
                "push",
                "--format",
                "deb",
                "--file",
                str(package),
                "--component",
                "main",
                env={"CLOUDSMITH_UPLOAD_URL": url, "CLOUDSMITH_API_KEY": FAKE_KEY},
            )
            _expect(
                pushed.returncode == 0 and "published" in pushed.stdout,
                "the upload must succeed against the endpoint",
                pushed.stdout + pushed.stderr,
            )
            _expect(
                len(seen) == 1
                and seen[0]["method"] == "POST"
                and seen[0]["path"] == "/offworld-nexus/apt/deb/",
                "the `.deb` upload must POST to upload.cloudsmith.io/<owner>/<repo>/deb/",
                json.dumps([(item["method"], item["path"]) for item in seen]),
            )
            headers = seen[0]["headers"]
            _expect(
                headers.get("authorization") == f"Bearer {FAKE_KEY}"
                and headers.get("x-api-key") == FAKE_KEY,
                "the key must travel as `Authorization: Bearer` and `X-Api-Key`",
                sorted(headers),
            )
            body = seen[0]["body"]
            for needle, what in (
                (
                    b'name="package_file"; filename="blastit_0.2.0_amd64.deb"',
                    "the file part",
                ),
                (b"debian-binary", "the file's bytes"),
                (b'name="distribution"', "`distribution`"),
                (b"any-distro", "`distribution=any-distro`"),
                (b'name="release"', "`release`"),
                (b"any-version", "`release=any-version`"),
                (b'name="component"', "`component`"),
                (b'name="republish"', "`republish`"),
            ):
                _expect(
                    needle in body,
                    f"the `.deb` multipart body must carry {what}",
                    body[:400].decode("latin-1"),
                )
            _expect(
                FAKE_KEY not in pushed.stdout + pushed.stderr,
                "the Cloudsmith key must never appear in a log line",
                pushed.stdout,
            )
            cases_run += 1

            seen.clear()
            pushed = run(
                "push",
                "--format",
                "rpm",
                "--file",
                str(rpm_package),
                "--component",
                "nightly",
                env={"CLOUDSMITH_UPLOAD_URL": url, "CLOUDSMITH_API_KEY": FAKE_KEY},
            )
            _expect(
                pushed.returncode == 0
                and seen
                and seen[0]["path"] == "/offworld-nexus/rpm/rpm/",
                "the `.rpm` upload must POST to <owner>/<repo>/rpm/ (the repo is `rpm`)",
                json.dumps([item["path"] for item in seen]),
            )
            _expect(
                b'name="component"' not in seen[0]["body"],
                "the RPM body must carry no `component`: its repository is one component",
                seen[0]["body"][:400].decode("latin-1"),
            )
            cases_run += 1
        finally:
            server.shutdown()
            server.server_close()

        # 6. The opt-in CLI branch: the design's argv, and a child that sees only
        #    the allowlisted variables (`FORGEJO_PACKAGES_TOKEN` must not appear).
        cli, recorded_file = _fake_cli(root)
        child = run(
            "push",
            "--format",
            "deb",
            "--file",
            str(package),
            "--component",
            "main",
            env={
                "CLOUDSMITH_CLI": str(cli),
                "CLOUDSMITH_API_KEY": FAKE_KEY,
                "FORGEJO_PACKAGES_TOKEN": "not-a-real-forgejo-token",
            },
        )
        recorded = recorded_file.read_text(encoding="utf-8").splitlines()
        _expect(
            child.returncode == 0
            and [line for line in recorded if line.startswith("ARG ")]
            == [
                "ARG push",
                "ARG deb",
                "ARG offworld-nexus/apt/any-distro/any-version",
                f"ARG {package}",
                "ARG --component",
                "ARG main",
                "ARG --republish",
            ],
            "the opt-in CLI must get the design's exact `cloudsmith push deb` argv",
            "\n".join(recorded) + child.stderr,
        )
        _expect(
            "ENV CLOUDSMITH_API_KEY" in recorded
            and "ENV FORGEJO_PACKAGES_TOKEN" not in recorded,
            "a child CLI must inherit the Cloudsmith key and nothing else from the job",
            "\n".join(recorded),
        )
        cases_run += 1

        # 7. Dry-run prints the request *and* the design's command; no network.
        dry = run(
            "push",
            "--format",
            "deb",
            "--file",
            str(package),
            "--component",
            "main",
            "--dry-run",
            env={"CLOUDSMITH_UPLOAD_URL": "", "CLOUDSMITH_CLI": "cloudsmith"},
        )
        _expect(
            dry.returncode == 0
            and f"cloudsmith: would POST {PUBLISH_UPLOAD}/offworld-nexus/apt/deb/"
            in dry.stdout
            and "cloudsmith: equivalent CLI: cloudsmith push deb "
            "offworld-nexus/apt/any-distro/any-version"
            in dry.stdout,
            "--dry-run must print the request and the design's command",
            dry.stdout + dry.stderr,
        )
        cases_run += 1

        missing_key = run(
            "push",
            "--format",
            "deb",
            "--file",
            str(package),
            "--component",
            "main",
            env={"CLOUDSMITH_API_KEY": ""},
        )
        _expect(
            missing_key.returncode == 1
            and "CLOUDSMITH_API_KEY is not set" in missing_key.stderr
            and package.name in missing_key.stderr
            and "nothing was uploaded" in missing_key.stderr,
            "a push without CLOUDSMITH_API_KEY must fail closed, naming the file",
            missing_key.stdout + missing_key.stderr,
        )
        cases_run += 1

        server, url, seen = _loopback([])
        try:
            absent = run(
                "forgejo",
                "--format",
                "deb",
                "--file",
                str(package),
                "--component",
                "main",
                env={"FORGEJO_URL": url, "FORGEJO_PACKAGES_TOKEN": ""},
            )
            _expect(
                absent.returncode == 0
                and "::notice::" in absent.stdout
                and "FORGEJO_PACKAGES_TOKEN is unset" in absent.stdout
                and not seen,
                "an unset mirror token must print one ::notice::, exit 0 and send nothing",
                absent.stdout + absent.stderr,
            )
            cases_run += 1

            token = "not-a-real-forgejo-token"
            posted = run(
                "forgejo",
                "--format",
                "deb",
                "--file",
                str(package),
                "--component",
                "main",
                env={"FORGEJO_URL": url, "FORGEJO_PACKAGES_TOKEN": token},
            )
            _expect(
                posted.returncode == 0 and "mirrored" in posted.stdout,
                "a set token must upload the package to the mirror",
                posted.stdout + posted.stderr,
            )
            _expect(
                len(seen) == 1
                and seen[0]["method"] == "POST"
                and seen[0]["path"]
                == "/api/packages/OffworldNexus/debian/pool/any-distro/main/upload",
                "the mirror URL must be the design's upload endpoint",
                json.dumps([item["path"] for item in seen]),
            )
            auth = seen[0]["headers"].get("authorization", "")
            _expect(
                auth.startswith("Basic ")
                and base64.b64decode(auth.split(" ", 1)[1]).decode()
                == f"Xowap:{token}",
                "the mirror must authenticate as Xowap:<token> (basic auth)",
                auth,
            )
            body = seen[0]["body"]
            _expect(
                b'name="package"' in body
                and package.name.encode() in body
                and package.read_bytes() in body,
                "the mirror body must be multipart `package=@<file>` with the file's bytes",
                body[:200].decode("latin-1"),
            )
            _expect(
                token not in posted.stdout + posted.stderr,
                "the Forgejo token must never appear in a log line",
                posted.stdout,
            )
            cases_run += 1
        finally:
            server.shutdown()
            server.server_close()

        # 8. Retention counts **runs**. Five runs × two architectures, keep three:
        #    runs 1 and 2 go entirely, and no run is ever split (`r2-arm64` deleted
        #    while `r2-amd64` stayed was the defect).
        nightlies = [
            _deb_nightly(day=run, run=run, arch=arch)
            for run in range(1, 6)
            for arch in ("amd64", "arm64")
        ] + [{"version": "0.2.0", "slug": "deb-release", "name": "release"}]
        server, url, seen = _loopback(nightlies)
        api = {
            "CLOUDSMITH_API_URL": f"{url}/v1",
            "CLOUDSMITH_API_KEY": FAKE_KEY,
        }
        try:
            pruned = run("prune", "--format", "deb", "--keep", "3", env=api)
            deleted = sorted(
                item["path"] for item in seen if item["method"] == "DELETE"
            )
            expected = sorted(
                f"/v1/packages/offworld-nexus/apt/deb-{arch}-r{run}-day{run:02d}/"
                for run in (1, 2)
                for arch in ("amd64", "arm64")
            )
            _expect(
                pruned.returncode == 0 and deleted == expected,
                "retention must delete the two oldest *runs* whole (4 packages) and "
                f"never split one; deleted {deleted}",
                pruned.stdout + pruned.stderr,
            )
            _expect(
                "deb-release" not in " ".join(deleted)
                and "5 nightly run(s)" in pruned.stdout,
                "the release in the same repository is never a nightly candidate",
                pruned.stdout,
            )
            _expect(
                FAKE_KEY not in pruned.stdout + pruned.stderr,
                "the API key must never appear in a log line",
                pruned.stdout,
            )
            cases_run += 1

            seen.clear()
            dry_prune = run(
                "prune", "--format", "deb", "--keep", "3", "--dry-run", env=api
            )
            _expect(
                dry_prune.returncode == 0
                and "would delete run r1" in dry_prune.stdout
                and not any(item["method"] == "DELETE" for item in seen),
                "--dry-run retention must print the runs it would delete and delete nothing",
                dry_prune.stdout + dry_prune.stderr,
            )
            cases_run += 1
        finally:
            server.shutdown()
            server.server_close()

        # 9. Two runs on one date: `r12` outranks `r9`, which a string sort gets
        #    backwards. Keeping one run keeps both architectures of `r12`.
        same_day = [
            _deb_nightly(day=5, run=run, arch=arch)
            for run in (9, 12)
            for arch in ("amd64", "arm64")
        ]
        server, url, seen = _loopback(same_day)
        try:
            pruned = run(
                "prune",
                "--format",
                "deb",
                "--keep",
                "1",
                env={"CLOUDSMITH_API_URL": f"{url}/v1", "CLOUDSMITH_API_KEY": FAKE_KEY},
            )
            deleted = sorted(
                item["path"] for item in seen if item["method"] == "DELETE"
            )
            _expect(
                pruned.returncode == 0
                and deleted
                == [
                    "/v1/packages/offworld-nexus/apt/deb-amd64-r9-day05/",
                    "/v1/packages/offworld-nexus/apt/deb-arm64-r9-day05/",
                ],
                "run 12 must outrank run 9 on the same date (a string sort says the "
                f"opposite); deleted {deleted}",
                pruned.stdout + pruned.stderr,
            )
            cases_run += 1
        finally:
            server.shutdown()
            server.server_close()

        # 10. A list that cannot be read to the end is refused, not pruned: ten
        #     full pages means this module's assumptions changed, and deleting on a
        #     partial view is how the newest nightly disappears.
        full_page = [
            _deb_nightly(day=1, run=index + 1, arch="amd64") for index in range(100)
        ]
        server, url, seen = _loopback(full_page)
        try:
            capped = run(
                "prune",
                "--format",
                "deb",
                "--keep",
                "7",
                env={"CLOUDSMITH_API_URL": f"{url}/v1", "CLOUDSMITH_API_KEY": FAKE_KEY},
            )
            _expect(
                capped.returncode == 1
                and "cannot see the whole repository" in capped.stderr
                and not any(item["method"] == "DELETE" for item in seen),
                "a repository that answers only full pages must be refused, not pruned",
                capped.stdout + capped.stderr,
            )
            cases_run += 1
        finally:
            server.shutdown()
            server.server_close()
    return cases_run


def cases() -> int:
    """Run every case and return how many passed (the packaging gate's entry)."""
    return test_publish_cases()


def main() -> int:
    ran = test_publish_cases()
    print(
        f"publish-gates-test: ok — {ran} cases passed (stamps, upload, mirror, retention)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
