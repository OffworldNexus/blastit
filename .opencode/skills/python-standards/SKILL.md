---
name: Python standards
description: Mandatory before writing or reviewing any Python in Blastit (`python/blastit/`, `python/tests/`, `tools/`, `e2e/`, `scripts/`). Covers the zero-dependency package rule, `uv` execution, DI/protocols, and ruff/pytest checks.
---

# Python standards (Blastit)

## 1. Two distinct Python zones

1. **Published runtime & CLI package (`python/blastit/`):**
   - **Zero third-party Python dependencies (`dependencies = []` in `pyproject.toml`).** Users installing `blastit` (`uv add blastit` or `pip install blastit`) must never pull extra Python packages into their application virtualenv!
   - Uses only the Python 3.13+ standard library (`tomllib`, `json`, `argparse`, `pathlib`, `subprocess`, `dataclasses`, `typing`, etc.) plus the compiled Rust extension `blastit._blastit` (and optional framework hooks imported lazily *only* when the user's test suite already uses that framework).
2. **Autonomous harness & repo scripts (`tools/`, `scripts/`, `e2e/`):**
   - Always run via `uv` — **never** call bare `python`, `python3`, or `pip`!
   - Because the root `pyproject.toml` builds the maturin wheel, run standalone scripts with `uv run --no-project tools/<script>.py` (with PEP 723 `# /// script` inline metadata and stdlib-only `dependencies = []` wherever possible so they run offline in milliseconds).

## 2. Design & code quality rules

1. **Explicit composition & zero hidden globals.** Pass configuration, paths, subprocess runners, and output streams explicitly into classes/functions so unit tests run with fast in-memory fakes rather than `unittest.mock` patches on our own code.
2. **Data types:** `@dataclass(frozen=True, slots=True)` for value objects, `enum.StrEnum` for closed sets, `pathlib.Path` instead of string paths, full type annotations everywhere.
3. **DX-quality CLI & error messages:** Every error printed to a developer or AI agent names:
   - **what** failed (file/key/route/query),
   - **why** it failed, and
   - **the exact command or line** that fixes it — while **never** printing secret env values or SQL parameter values.
4. **Formatting & linting:** Check every modified Python file with `uvx --offline ruff check <paths>` and `uvx --offline ruff format --check <paths>` (or `make py-lint`). Never weaken lint configs to get green.
