"""Makes the README's links absolute, so PyPI can render it.

PyPI shows `README.md` as the project description, and it renders that Markdown
with no repository around it: a relative link written for GitHub is a 404 there,
and a relative *image* is not rendered at all. Every published wheel and sdist
carries the README as its long description, so the links have to point at
github.com before the build.

    uv run --no-project scripts/readme-absolute-links.py           # rewrite in place
    uv run --no-project scripts/readme-absolute-links.py --check   # say what would change

What it rewrites, in the whole file (fenced code blocks are left alone):

    [text](ARCHITECTURE.md#82-configuration-reference)
      → https://github.com/OffworldNexus/blastit/blob/<ref>/ARCHITECTURE.md#82-configuration-reference
    ![banner](.github/brand/banner-light.svg)
      → https://raw.githubusercontent.com/OffworldNexus/blastit/<ref>/.github/brand/banner-light.svg
    [docs]: docs/

Images go to `raw.githubusercontent.com` because that is the URL that serves the
bytes; everything else goes to the blob view. Targets that already start with a
scheme (`https:`, `mailto:`), a fragment (`#`), a host (`//`) or a site path
(`/`) are left as they are.

The ref defaults to the tag a release is built from (`$GITHUB_REF_NAME` when
`$GITHUB_REF_TYPE` is `tag`), then to the branch, then to the branch a pull
request targets, and to `develop` outside CI. The repository defaults to
`$GITHUB_REPOSITORY`, then to `OffworldNexus/blastit`.
"""

from __future__ import annotations

import argparse
import os
import pathlib
import re
import sys

DEFAULT_REPO = "OffworldNexus/blastit"
DEFAULT_REF = "develop"

# An image is served by raw.githubusercontent.com; anything else is a page.
IMAGE_SUFFIXES = (".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".ico")

# Absolute already: a scheme, a fragment, a protocol-relative or site-absolute
# path, or nothing at all.
ABSOLUTE = re.compile(r"(?:[a-zA-Z][a-zA-Z0-9+.-]*:|//|/|#|$)")

# Every place a target can hide in this README: an inline link or image
# (`[text](target)`, `![alt](target)`), a reference definition
# (`[label]: target`), and the raw HTML `src=`/`href=`.
LINK = re.compile(
    r"""(?P<lead>\]\(|^[ \t]*\[[^\]\n]*\]:[ \t]*|(?:src|href)=["'])(?P<target>[^"')\s>]+)""",
    re.MULTILINE,
)

# Anything left looking like `](relative)` after a rewrite is a bug here.
STILL_RELATIVE = re.compile(r"\]\((?P<target>[^)\s]+)\)")

FENCE = re.compile(r"^[ \t]*(?:```|~~~)")


def default_ref() -> str:
    """The ref the absolute links should name.

    A release is built from a tag, so the tag: the description then points at the
    sources of the version it belongs to. A branch build names the branch. A pull
    request builds a *merge* ref (`44/merge`), which nobody can open, so the
    branch it targets is used instead.
    """
    if os.environ.get("GITHUB_REF_TYPE") in ("tag", "branch"):
        return os.environ.get("GITHUB_REF_NAME") or DEFAULT_REF
    return os.environ.get("GITHUB_BASE_REF") or DEFAULT_REF


def absolute(target: str, repo: str, ref: str) -> str | None:
    """The URL for a repository-relative target, or None if it is not one."""
    if ABSOLUTE.match(target):
        return None
    path, _, fragment = target.partition("#")
    raw = path.lower().endswith(IMAGE_SUFFIXES)
    host = "raw.githubusercontent.com" if raw else "github.com"
    tail = "" if raw else "/blob"
    return f"https://{host}/{repo}{tail}/{ref}/{path}" + (f"#{fragment}" if fragment else "")


def rewrite(text: str, repo: str, ref: str) -> tuple[str, list[tuple[str, str]], list[str]]:
    """Returns the rewritten text, the links it made absolute, and anything it
    could not (`](target)` with a relative target is the only shape it knows, and
    the only shape the README uses)."""
    changed: list[tuple[str, str]] = []
    left: list[str] = []

    def replace(match: re.Match[str]) -> str:
        url = absolute(match.group("target"), repo, ref)
        if url is None:
            return match.group(0)
        changed.append((match.group("target"), url))
        return match.group("lead") + url

    out: list[str] = []
    in_fence = False
    for line in text.splitlines(keepends=True):
        if FENCE.match(line):
            in_fence = not in_fence
            out.append(line)
            continue
        if in_fence:
            # Commands and TOML in a code block, not links: leave them alone.
            out.append(line)
            continue
        line = LINK.sub(replace, line)
        left.extend(
            match.group("target") for match in STILL_RELATIVE.finditer(line) if not ABSOLUTE.match(match.group("target"))
        )
        out.append(line)
    return "".join(out), changed, left


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("paths", nargs="*", default=["README.md"], help="files to rewrite (default: README.md)")
    parser.add_argument("--check", action="store_true", help="change nothing, report what it would change")
    parser.add_argument("--ref", default=default_ref(), help="git ref the links point at")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY") or DEFAULT_REPO, help="OWNER/NAME")
    args = parser.parse_args(argv)

    status = 0
    for path in (pathlib.Path(p) for p in args.paths):
        text, changed, left = rewrite(path.read_text(), args.repo, args.ref)
        for target, url in changed:
            print(f"{path}: {target} → {url}", flush=True)
        if left:
            print(f"{path}: still relative, so PyPI would break: {', '.join(left)}", file=sys.stderr, flush=True)
            status = 1
        if changed and not args.check:
            path.write_text(text)
        elif changed:
            print(f"{path}: {len(changed)} link(s) would be rewritten", file=sys.stderr, flush=True)
        elif not left:
            print(f"{path}: already absolute", flush=True)
    return status


if __name__ == "__main__":
    sys.exit(main())
