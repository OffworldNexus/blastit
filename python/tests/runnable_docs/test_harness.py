"""Tests for the static checks the runnable-docs harness trusts.

`make docs-test` only proves those checks on the pages that happen to exist: a
`static` deploy fence is checked once, in the shape the page writes it, and the
tables, the path resolution, the empty-fence rule and the wheel/provenance chain
were never exercised against a *wrong* page (`docs/testing/AUDIT-MA.3.md`,
`F4`/`F11`, and the MA.3.10 review of the flag tables).

These are the gate's own allowed/denied pairs, over the real functions: each
test writes the command, fence, wheel directory or tiny repository tree it is
about and asserts that the harness accepts the honest version and refuses the
drift. Only module-level fixtures are swapped (the `FLAG_AUDIT` tuple and
`REPO`/`DIST`), never a function of the harness.

Run them with:

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest \\
    python/tests/runnable_docs/test_harness.py -q
```
"""

from __future__ import annotations

import contextlib
import sys
import tempfile
import unittest
from collections.abc import Iterator
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import harness
from harness import CliHelp, Failure, Fence, FenceSpec, FlagAudit, ToolFlags

REASON = (
    "The page shows this file and the command names it; the harness resolves "
    "the path statically instead of running the recipe."
)

#: The CLI flags the `static` checks compare against. Only `fold` matters here:
#: `blastit-manifest fold --manifest` is the path flag with no external tool.
CLI = CliHelp(
    blastit=frozenset(),
    dev=frozenset(),
    check=frozenset(),
    fold=frozenset({"--manifest"}),
    presets=frozenset(),
)


def fence(body: str, *, page: str = "docs/deploy.md", index: int = 1) -> Fence:
    """One `console` block as `fences.scan_page` would hand it to a mode."""
    return Fence(page=page, index=index, line=1, body=body)


@contextlib.contextmanager
def tree(files: dict[str, str]) -> Iterator[Path]:
    """A throwaway directory holding exactly `files`."""
    holder = tempfile.TemporaryDirectory()
    root = Path(holder.name)
    try:
        for name, text in files.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8")
        yield root
    finally:
        holder.cleanup()


@contextlib.contextmanager
def audited(*audits: FlagAudit) -> Iterator[None]:
    """Run the real audit over a fixture tuple of tables, then restore it."""
    saved = harness.FLAG_AUDIT
    harness.FLAG_AUDIT = audits
    try:
        yield
    finally:
        harness.FLAG_AUDIT = saved


@contextlib.contextmanager
def rooted(root: Path) -> Iterator[None]:
    """Point the provenance readers at `root` instead of this repository."""
    saved = harness.REPO
    harness.REPO = root
    try:
        yield
    finally:
        harness.REPO = saved


@contextlib.contextmanager
def wheel_directory(root: Path) -> Iterator[None]:
    """Point the wheel lookup at `root` instead of `dist/`."""
    saved = harness.DIST
    harness.DIST = root
    try:
        yield
    finally:
        harness.DIST = saved


def wheel_name(version: str, *, minor: int | None = None) -> str:
    """A host wheel name: the ABI tag is the part `wheel_path` reads."""
    minor = sys.version_info.minor if minor is None else minor
    return f"blastit-{version}-cp3{minor}-cp3{minor}-manylinux_2_39_x86_64.whl"


class HarnessTest(unittest.TestCase):
    """A `Failure` whose message says what to fix, so the text is pinned too."""

    @contextlib.contextmanager
    def refused(self, text: str) -> Iterator[None]:
        with self.assertRaises(Failure) as caught:
            yield
        self.assertIn(text, str(caught.exception))


class FlagTableDriftTest(HarnessTest):
    """A flag the table does not hold is refused; the ones it holds pass."""

    def test_a_flag_no_table_holds_is_refused(self) -> None:
        with self.refused("is not a flag `docker run` accepts"):
            harness._static_command(
                "docker run --rm --frobnicate ghcr.io/shop/app:1", CLI
            )

    def test_every_flag_the_table_holds_is_accepted(self) -> None:
        self.assertEqual(
            harness._static_command(
                "docker run --rm -p 8080:8080 -e SHOP=1 ghcr.io/shop/app:1", CLI
            ),
            0,
        )

    def test_an_invented_doctl_subcommand_is_refused(self) -> None:
        with self.refused(
            "`doctl apps frobnicate` is not a command this harness checks"
        ):
            harness._static_command("doctl apps frobnicate --spec app.yaml", CLI)

    def test_an_invented_doctl_flag_is_refused(self) -> None:
        with self.refused("is not a flag `doctl apps delete` accepts"):
            harness._static_command("doctl apps delete --frobnicate shop", CLI)

    def test_the_audit_refuses_a_flag_the_tool_rejects(self) -> None:
        invented = ToolFlags(booleans=frozenset({"--blastit-not-a-python-flag"}))
        audit = FlagAudit("python", (sys.executable,), invented, gate=True)
        with audited(audit), self.refused("in the harness's `python` flag table"):
            harness.assert_flag_tables()

    def test_the_audit_accepts_the_tools_real_flags(self) -> None:
        real = ToolFlags(booleans=frozenset({"-h", "--help", "-V"}))
        audit = FlagAudit("python", (sys.executable,), real, gate=True)
        with audited(audit):
            self.assertEqual(
                harness.assert_flag_tables(),
                "flag tables checked against --help: python",
            )

    def test_gate_only_skips_a_tool_that_is_not_here(self) -> None:
        present = FlagAudit(
            "python", (sys.executable,), ToolFlags(booleans=frozenset({"-h"})), True
        )
        ghost = FlagAudit(
            "doctl", ("blastit-flag-audit-ghost",), ToolFlags(), gate=False
        )
        with audited(present, ghost):
            summary = harness.assert_flag_tables(gate_only=True)
            self.assertEqual(summary, "flag tables checked against --help: python")
            self.assertNotIn("ghost", summary)
            full = harness.assert_flag_tables()
        self.assertIn("(not installed here: blastit-flag-audit-ghost)", full)


class DoctlAuditTest(unittest.TestCase):
    """The doctl tables are audited, and can never red a gate on a host."""

    def test_every_doctl_table_is_in_the_audit(self) -> None:
        by_argv = {audit.argv: audit for audit in harness.FLAG_AUDIT}
        self.assertEqual(by_argv[("doctl",)].table, harness.DOCTL_GLOBAL)
        for command, table in harness.DOCTL_COMMANDS.items():
            with self.subTest(command=" ".join(command)):
                audit = by_argv[("doctl", *command)]
                self.assertEqual(audit.table, table)
                self.assertEqual(audit.label, f"doctl {' '.join(command)}")

    def test_doctl_never_gates(self) -> None:
        entries = [audit for audit in harness.FLAG_AUDIT if audit.argv[0] == "doctl"]
        self.assertTrue(entries)
        for audit in entries:
            with self.subTest(label=audit.label):
                self.assertFalse(audit.gate)


class DocumentedPathTest(HarnessTest):
    """A file a flag names must exist, or be a file the page itself shows."""

    def test_a_compose_file_the_page_shows_is_accepted(self) -> None:
        self.assertEqual(
            harness._static_command(
                "docker compose -f compose.yaml up -d", CLI, frozenset({"compose.yaml"})
            ),
            0,
        )

    def test_a_psql_file_the_page_shows_is_accepted(self) -> None:
        self.assertEqual(
            harness._static_command(
                "psql --file db/grants.sql postgresql://shop@127.0.0.1:5432/shop",
                CLI,
                frozenset({"db/grants.sql"}),
            ),
            0,
        )

    def test_the_reader_writes_blastit_toml_themselves(self) -> None:
        self.assertEqual(
            harness._static_command(
                "blastit-manifest fold --manifest blastit.toml", CLI
            ),
            0,
        )

    def test_a_missing_file_is_refused(self) -> None:
        for command, flag in (
            ("docker compose -f nope/compose.yaml up -d", "-f"),
            ("docker build --file nope/Dockerfile .", "--file"),
            (
                "psql --file db/nope.sql postgresql://shop@127.0.0.1:5432/shop",
                "--file",
            ),
            ("doctl apps create --spec deploy/nope.yaml", "--spec"),
            ("blastit-manifest fold --manifest nope/blastit.toml", "--manifest"),
        ):
            with (
                self.subTest(flag=flag),
                self.refused("names a file that neither exists"),
            ):
                harness._static_command(command, CLI)

    def test_an_absolute_path_outside_the_tree_is_left_to_the_host(self) -> None:
        self.assertEqual(
            harness._static_command("docker build -f /nope/Dockerfile .", CLI), 1
        )


class ZeroCommandTest(HarnessTest):
    """An executing mode may never pass a fence it read no command from."""

    def test_a_fence_without_prompts_is_refused(self) -> None:
        with self.refused("none of its 2 non-blank lines starts with `$ `"):
            harness._require_commands(
                fence("Starting blastit\nlistening on 127.0.0.1"),
                FenceSpec(index=1, mode="check"),
            )

    def test_an_empty_fence_is_refused(self) -> None:
        with self.refused("the fence shows nothing"):
            harness._require_commands(fence("\n \t\n"), FenceSpec(1, "dev"))

    def test_a_fence_with_prompts_is_accepted(self) -> None:
        self.assertIsNone(
            harness._require_commands(
                fence("$ blastit check\nok"), FenceSpec(1, "check")
            )
        )

    def test_an_accounted_mode_needs_its_reason(self) -> None:
        with self.refused("must carry a written `reason`"):
            harness._require_commands(fence("Starting blastit"), FenceSpec(1, "static"))

    def test_an_accounted_mode_may_show_no_command(self) -> None:
        self.assertIsNone(
            harness._require_commands(
                fence("Starting blastit"),
                FenceSpec(1, "static", reason=REASON),
            )
        )


class ProvenanceTest(HarnessTest):
    """`Cargo.toml` and the newest `CHANGELOG.md` release must agree."""

    CARGO = '[workspace.package]\nversion = "1.2.3"\n'

    def test_matching_versions_are_reported(self) -> None:
        with (
            tree(
                {
                    "Cargo.toml": self.CARGO,
                    "CHANGELOG.md": "# Changelog\n\n## 1.2.3 (2026-01-01)\n\n- notes\n",
                }
            ) as root,
            rooted(root),
        ):
            self.assertEqual(harness.assert_provenance(), "1.2.3")

    def test_a_changelog_behind_the_workspace_is_refused(self) -> None:
        with (
            tree(
                {
                    "Cargo.toml": self.CARGO,
                    "CHANGELOG.md": "# Changelog\n\n## 1.2.2 (2026-01-01)\n",
                }
            ) as root,
            rooted(root),
            self.refused(
                "workspace version is 1.2.3, but the newest release "
                "CHANGELOG.md announces is 1.2.2"
            ),
        ):
            harness.assert_provenance()

    def test_a_version_that_is_not_semver_is_refused(self) -> None:
        with (
            tree({"Cargo.toml": '[workspace.package]\nversion = "1.2"\n'}) as root,
            rooted(root),
            self.refused("not `X.Y.Z`"),
        ):
            harness.planned_version()


class WheelTest(HarnessTest):
    """The wheel under test is the workspace version's, chosen not guessed."""

    def test_the_one_wheel_for_the_workspace_version_is_chosen(self) -> None:
        version = harness.planned_version()
        name = wheel_name(version)
        with tree({name: ""}) as root, wheel_directory(root):
            self.assertEqual(harness.wheel_path(), root / name)

    def test_a_dist_without_that_version_is_refused(self) -> None:
        version = harness.planned_version()
        stray = "9.9.9" if version != "9.9.9" else "8.8.8"
        with (
            tree({wheel_name(stray): ""}) as root,
            wheel_directory(root),
            self.refused(f"has no blastit-{version} wheel"),
        ):
            harness.wheel_path()

    def test_two_wheels_for_the_host_are_refused(self) -> None:
        version = harness.planned_version()
        first = wheel_name(version)
        second = wheel_name(version).replace("manylinux_2_39", "manylinux_2_28")
        with (
            tree({first: "", second: ""}) as root,
            wheel_directory(root),
            self.refused(f"holds 2 wheels for version {version}"),
        ):
            harness.wheel_path()


if __name__ == "__main__":
    unittest.main()
