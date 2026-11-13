"""Protected critical tier: the published package is stdlib-only (R40, R41).

`pip install blastit` or `uv add blastit` must never pull a Python dependency
into a project's environment: the server is the extension
(`blastit._blastit`), and the CLI is standard library code. `pyproject.toml`
says `dependencies = []`, and this file proves it the way a user's resolver
sees it — by parsing the file — and proves the source agrees by walking every
module's top-level imports.

`pytest_plugin.py` is the one exception: it is a pytest entry point, loaded by
pytest itself, so `import pytest` is the plugin's host, not a dependency of the
package. Every other third-party framework hook (Django, psycopg) is imported
lazily inside a function and never at module import time.
"""

from __future__ import annotations

import ast
import sys
import tempfile
import tomllib
import unittest
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

#: `python/tests/critical/test_stdlib_only.py` -> the repository root.
ROOT = Path(__file__).resolve().parents[3]


def package_modules() -> list[Path]:
    """Every module of the published package, in a stable order."""
    return sorted((ROOT / "python" / "blastit").glob("*.py"))


def top_level_imports(path: Path) -> set[str]:
    """The modules a file imports at import time (not inside a function)."""
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    names: set[str] = set()
    for node in tree.body:
        if isinstance(node, ast.Import):
            names.update(alias.name.split(".")[0] for alias in node.names)
        elif (
            isinstance(node, ast.ImportFrom)
            and node.level == 0
            and node.module is not None
        ):
            names.add(node.module.split(".")[0])
    return names


@contextmanager
def temporary_module(source: str) -> Iterator[Path]:
    """Writes `source` to a throwaway module and yields its path."""
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "sample.py"
        path.write_text(source, encoding="utf-8")
        yield path


class TestNoRuntimeDependencies(unittest.TestCase):
    """`dependencies == []`, read through the same parser a build backend uses."""

    def test_pyproject_declares_no_python_dependency(self) -> None:
        text = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
        project = tomllib.loads(text)["project"]
        self.assertEqual(project["dependencies"], [], "the wheel must install nothing")
        self.assertNotIn(
            "optional-dependencies",
            project,
            "a runtime extra is still a dependency a user opts into",
        )

    def test_the_build_backend_is_not_a_runtime_dependency(self) -> None:
        """`maturin` is build-only: it must stay in `build-system.requires`."""
        text = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
        document = tomllib.loads(text)
        self.assertTrue(document["build-system"]["requires"], "the wheel has a backend")
        self.assertEqual(document["project"]["dependencies"], [])

    def test_every_module_imports_only_the_stdlib_or_blastit(self) -> None:
        allowed = set(sys.stdlib_module_names) | {"blastit"}
        for path in package_modules():
            imports = top_level_imports(path)
            if path.name == "pytest_plugin.py":
                # The plugin is loaded by pytest, so pytest is its host.
                imports = imports - {"pytest"}
            offenders = imports - allowed
            self.assertEqual(offenders, set(), f"{path.name} imports {offenders}")

    def test_the_extension_is_not_imported_at_package_import_time(self) -> None:
        """Importing `blastit` starts nothing and loads no extension."""
        init = top_level_imports(ROOT / "python" / "blastit" / "__init__.py")
        self.assertEqual(init, set(), "the package import is inert")

    def test_a_third_party_import_would_be_caught(self) -> None:
        """Negative twin: the walk really sees an import it should reject."""
        with temporary_module("import requests\n") as path:
            self.assertIn("requests", top_level_imports(path))

    def test_a_lazy_import_is_not_a_top_level_one(self) -> None:
        """Negative twin: an import inside a function is not import-time."""
        source = "def hook():\n    import django\n    return django\n"
        with temporary_module(source) as path:
            self.assertEqual(top_level_imports(path), set())


if __name__ == "__main__":
    unittest.main()
