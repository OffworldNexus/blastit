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
# (`[label]: target`), and the raw HTML the banner needs — `src=`, `href=`, and
# `srcset=`, whose value is a comma-separated list of candidates.
LINK = re.compile(
    r"""(?P<lead>\]\(|^[ \t]*\[[^\]\n]*\]:[ \t]*|(?:src|href)=["'])(?P<target>[^"')\s>]+)""",
    re.MULTILINE,
)
SRCSET = re.compile(r"""(?P<lead>srcset=["'])(?P<candidates>[^"']*)""")

# Anything left looking like a relative target after a rewrite is a bug here.
STILL_RELATIVE = re.compile(
    r"""\]\((?P<target>[^)\s]+)\)|(?:src|href)=["'](?P<attr>[^"'\s>]+)""",
)

FENCE = re.compile(r"^[ \t]*(?:```|~~~)")


def default_ref() -> str:
    """The ref the absolute links should name.

    A release is built from a tag, so the tag: the description then points at the
    sources of the version it belongs to. A branch build names the branch. A pull
    request builds a *merge* ref — `refs/pull/44/merge`, which GitHub reports as a
    branch named `44/merge` — and nobody can open that, so the branch it targets
    is used instead (`GITHUB_BASE_REF` is set on pull requests only).
    """
    if os.environ.get("GITHUB_BASE_REF"):
        return os.environ["GITHUB_BASE_REF"]
    if os.environ.get("GITHUB_REF_TYPE") in ("tag", "branch"):
        return os.environ.get("GITHUB_REF_NAME") or DEFAULT_REF
    return DEFAULT_REF


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
    could not — a relative target in a shape this does not know."""
    changed: list[tuple[str, str]] = []
    left: list[str] = []

    def replace(match: re.Match[str]) -> str:
        url = absolute(match.group("target"), repo, ref)
        if url is None:
            return match.group(0)
        changed.append((match.group("target"), url))
        return match.group("lead") + url

    def replace_srcset(match: re.Match[str]) -> str:
        # `srcset` is a list — `url 1x, url 2x` — so every candidate is a target
        # of its own, and the descriptors (`2x`, `480w`) stay as they are.
        candidates = []
        for candidate in match.group("candidates").split(","):
            url, _, descriptor = candidate.strip().partition(" ")
            absolute_url = absolute(url, repo, ref)
            if absolute_url is not None:
                changed.append((url, absolute_url))
                url = absolute_url
            candidates.append(" ".join(part for part in (url, descriptor) if part))
        return match.group("lead") + ", ".join(candidates)

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
        line = SRCSET.sub(replace_srcset, LINK.sub(replace, line))
        left.extend(relative_targets(line, repo, ref))
        out.append(line)
    return "".join(out), changed, left


def relative_targets(line: str, repo: str, ref: str) -> list[str]:
    """The targets in `line` that a renderer with no repository around it — PyPI
    — would resolve to nothing: the ones that should have been rewritten."""
    found: list[str] = []
    for match in STILL_RELATIVE.finditer(line):
        target = match.group("target") or match.group("attr")
        if absolute(target, repo, ref) is not None:
            found.append(target)
    for match in SRCSET.finditer(line):
        for candidate in match.group("candidates").split(","):
            target = candidate.strip().partition(" ")[0]
            if target and absolute(target, repo, ref) is not None:
                found.append(target)
    return found


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
