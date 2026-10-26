# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "readme_renderer[md]",  # what PyPI renders a long description with (comrak, GFM)
#     "nh3",                  # readme_renderer's sanitizer: the tag allowlist that decides
#                             # what survives, `srcset` inside `<picture>` being one casualty
# ]
# ///
"""Checks the metadata and the description PyPI will show for a distribution.

    uv run --no-project scripts/check-long-description.py dist/*

`Publish` runs this on everything a release would upload — both wheels and the
sdist — so a description that renders to nothing, a link that only works inside
the repository, or a banner PyPI cannot fetch fails the build instead of
appearing on the project page.

This is deliberately not `twine check`: twine does not render a Markdown
description at all (`twine.commands.check._RENDERERS["text/markdown"]` is
`None`, "rendering cannot fail"), so it cannot see a broken one. The renderer
below is the one PyPI itself uses — `readme_renderer` with `comrak` for GFM and
`nh3` for the tag allowlist — which is what makes the result meaningful.

For each file it checks:

* the metadata fields a release needs (and the values that are not free to
  change: the license, the Python requirement, the project URLs);
* `Description-Content-Type: text/markdown`, GFM;
* that the description renders to non-empty HTML;
* that no link in the rendered HTML is relative, `src` and `href` alike;
* that the first image — the README banner — comes from
  `raw.githubusercontent.com`, since PyPI serves nothing from the repository.

Files are also checked against *each other*: the same name, version and license
in every wheel and in the sdist.
"""
from __future__ import annotations

import email
import pathlib
import re
import sys
import tarfile
import warnings
import zipfile

import readme_renderer.markdown

# A field is checked for presence, or for an exact value when one is given.
REQUIRED = {
    "Metadata-Version": "2.4",
    "Name": "blastit",
    "Version": None,
    "Summary": None,
    "Requires-Python": ">=3.13",
    "License-Expression": "Apache-2.0",
    "License-File": "LICENSE",
    "Author-email": None,
    "Maintainer-email": None,
    "Keywords": None,
    "Classifier": None,
}
PROJECT_URLS = ("Homepage", "Documentation", "Source", "Issues", "Changelog")
AGREE = ("Name", "Version", "License-Expression")

CONTENT_TYPE = "text/markdown"
VARIANT = "GFM"
BANNER_HOST = "https://raw.githubusercontent.com/"

# Absolute already, and fine to keep: a scheme, a fragment, an anchor.
ABSOLUTE = re.compile(r"^(?:https?://|mailto:|#)")
ATTRIBUTE = re.compile(r"""(?P<name>src|href)=["'](?P<url>[^"']*)["']""")
IMAGE = re.compile(r"<img\b[^>]*>")


class Failure(Exception):
    """Something a release must not ship."""


def metadata_of(path: pathlib.Path) -> tuple[email.message.Message, str]:
    """The headers and the description of a wheel's METADATA or an sdist's
    PKG-INFO.

    The description is the message *body*, which `email` would hand back through
    a charset guess: splitting the bytes ourselves keeps UTF-8 exact (the README
    has em dashes, and they matter to the rendered page).
    """
    if path.suffix == ".whl":
        with zipfile.ZipFile(path) as archive:
            name = next((n for n in archive.namelist() if n.endswith(".dist-info/METADATA")), None)
            if name is None:
                raise Failure("no `.dist-info/METADATA` in the wheel")
            raw = archive.read(name)
    elif path.name.endswith((".tar.gz", ".tgz")):
        with tarfile.open(path) as archive:
            member = next((m for m in archive.getmembers() if m.name.endswith("/PKG-INFO")), None)
            if member is None:
                raise Failure("no `PKG-INFO` in the sdist")
            handle = archive.extractfile(member)
            if handle is None:
                raise Failure("`PKG-INFO` is not a regular file in the sdist")
            raw = handle.read()
    else:
        raise Failure("not a wheel (`.whl`) or a source distribution (`.tar.gz`)")

    head, separator, body = raw.partition(b"\n\n")
    if not separator:
        raise Failure("no description: the metadata has no blank line after the headers")
    return email.message_from_bytes(head), body.decode("utf-8")


def check_metadata(headers: email.message.Message) -> None:
    for field, expected in REQUIRED.items():
        values = headers.get_all(field)
        if not values:
            raise Failure(f"`{field}` is missing")
        if expected is not None and expected not in values:
            raise Failure(f"`{field}` is {values!r}, expected `{expected}`")

    urls = {}
    for value in headers.get_all("Project-URL", []):
        label, _, url = value.partition(",")
        urls[label.strip()] = url.strip()
    for label in PROJECT_URLS:
        if label not in urls:
            raise Failure(f"no `Project-URL` for {label}")
        if not ABSOLUTE.match(urls[label]):
            raise Failure(f"`Project-URL` {label} is not absolute: {urls[label]!r}")

    content_type = headers.get("Description-Content-Type", "")
    if not content_type.startswith(CONTENT_TYPE):
        raise Failure(f"`Description-Content-Type` is {content_type!r}, expected `{CONTENT_TYPE}`")
    parameters = dict(
        part.split("=", 1) for part in (p.strip() for p in content_type.split(";")[1:]) if "=" in part
    )
    if parameters.get("variant") != VARIANT:
        raise Failure(f"the description is not rendered as {VARIANT}: {content_type!r}")


def check_description(description: str) -> None:
    if not description.strip():
        raise Failure("the description is empty")

    # The renderer is PyPI's, so its complaints are worth failing on; it returns
    # `None` (or warns) when it cannot render the variant at all.
    with warnings.catch_warnings(record=True) as complaints:
        warnings.simplefilter("always")
        html = readme_renderer.markdown.render(description, variant=VARIANT)
    if complaints:
        raise Failure(
            "the renderer complained: " + "; ".join(str(complaint.message) for complaint in complaints)
        )
    if not html or not html.strip():
        raise Failure("the description renders to nothing")

    for match in ATTRIBUTE.finditer(html):
        url = match.group("url")
        if not ABSOLUTE.match(url):
            raise Failure(f"`{match.group('name')}` in the rendered description is relative: {url!r}")

    images = IMAGE.findall(html)
    if not images:
        raise Failure("no `<img>` in the rendered description: the README banner is gone")
    banner = ATTRIBUTE.search(images[0])
    if banner is None:
        raise Failure("the first image in the rendered description has no `src`")
    if not banner.group("url").startswith(BANNER_HOST):
        raise Failure(
            f"the banner is not served by {BANNER_HOST}: {banner.group('url')!r}\n"
            f"    it has to be rewritten by scripts/readme-absolute-links.py before the build"
        )


def check(path: pathlib.Path) -> dict[str, str]:
    headers, description = metadata_of(path)
    check_metadata(headers)
    check_description(description)
    return {field: headers.get(field, "") for field in AGREE}


def main(argv: list[str]) -> int:
    paths = [pathlib.Path(argument) for argument in argv]
    if not paths:
        print("usage: check-long-description.py DIST...", file=sys.stderr)
        return 2

    seen: dict[str, tuple[pathlib.Path, str]] = {}
    failures = []
    for path in paths:
        try:
            fields = check(path)
            for field, value in fields.items():
                if field in seen and seen[field][1] != value:
                    raise Failure(
                        f"`{field}` is {value!r}, but {seen[field][0].name} says {seen[field][1]!r}"
                    )
                seen[field] = (path, value)
        except Failure as failure:
            failures.append(f"{path.name}: {failure}")
            print(f"{path.name}: FAILED", file=sys.stderr)
        else:
            print(f"{path.name}: ok")

    if failures:
        print("\nthe description PyPI would show is not the one we want:\n", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1
    print(f"\n{len(paths)} file(s), rendered the way PyPI renders them")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
