"""Regression tests for the brand assets the docs site and the wheel ship.

`docs/brand/logo.svg` is loaded by the site header as an `<img>` (zensical.toml
`logo = "brand/logo.svg"`), so page CSS cannot reach inside it: the light-scheme
cutout is drawn by an inline `@media` rule in the file itself. `MB.1.7` shipped
that rule after two real defects, and neither has a test:

* the first draft's `<style>` comment contained a literal `<img>`, which makes
  the whole SVG ill-formed XML. The browser then draws alt text (`naturalWidth
  0`) while `make docs-check` still prints "No issues found", because Zensical
  neither parses nor renders the asset;
* the paper-white cutout (`#fbfaf6`) is invisible on the near-white light
  header, so the light scheme needs the brand ink instead.

The contrast is computed here from the two hex values and the site's own
`--blastit-background` tokens, not from the ratio the asset's comment remembers,
so a palette change or a hand-edited fill fails with the number it produced.
Nothing here touches the network, a wheel or a browser.
"""

from __future__ import annotations

import re
import xml.etree.ElementTree as ET
from pathlib import Path

BRAND = Path(__file__).resolve().parents[2] / "docs" / "brand"

SVG = "{http://www.w3.org/2000/svg}"
#: WCAG 2.2 SC 1.4.11 (non-text contrast): a graphical object needs 3:1.
MIN_CONTRAST = 3.0
#: The scheme background tokens from `docs/brand/brand.css` the header sits on.
LIGHT_HEADER = "#f4efe3"
DARK_HEADER = "#121820"

HEX = re.compile(r"^#(?P<hex>[0-9a-fA-F]{6})$")
LIGHT_RULE = re.compile(r"@media\s*\(\s*prefers-color-scheme\s*:\s*light\s*\)\s*\{")
CUTOUT_RULE = re.compile(
    r"#(?P<id>[A-Za-z][\w-]*)\s*\{\s*fill\s*:\s*(?P<fill>#[0-9a-fA-F]{6})\s*;?\s*\}"
)


def parse(path: Path) -> ET.Element:
    """The SVG root of `path`, refusing a different root element."""
    root = ET.parse(path).getroot()
    assert root.tag == f"{SVG}svg", (
        f"{path.name}: root element is {root.tag!r}, not an SVG"
    )
    return root


def style_of(root: ET.Element) -> str:
    """The text of the document's `<style>` element (all of them, joined)."""
    texts = [element.text or "" for element in root.iter(f"{SVG}style")]
    assert texts, "logo.svg has no <style> element to carry the light-scheme rule"
    return "\n".join(texts)


def block_at(text: str, opening: int) -> str:
    """The contents of the brace block whose `{` is at `opening`."""
    depth = 0
    for index in range(opening, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[opening + 1 : index]
    raise AssertionError("a CSS block in the <style> element is not closed")


def cutout_fills(root: ET.Element) -> tuple[str, str]:
    """The `#head-cutout` path's own `fill`, and the fill the light rule gives it."""
    paths = [element for element in root.iter(f"{SVG}path") if element.get("id")]
    assert len(paths) == 1, f"expected exactly one identified path, found {len(paths)}"
    path = paths[0]
    default = path.get("fill", "")
    assert HEX.match(default), f"#{path.get('id')} has no plain hex fill: {default!r}"

    style = style_of(root)
    rule = LIGHT_RULE.search(style)
    assert rule, (
        "logo.svg has no `@media (prefers-color-scheme: light)` rule: the header "
        "loads the file as an <img>, so page CSS cannot redraw the cutout"
    )
    light = CUTOUT_RULE.search(block_at(style, rule.end() - 1))
    assert light, "the light-scheme rule does not set a fill for the cutout"
    assert light.group("id") == path.get("id"), (
        f"the light-scheme rule targets #{light.group('id')} but the path is "
        f"#{path.get('id')}: the rule is dead"
    )
    return default, light.group("fill")


def luminance(colour: str) -> float:
    """WCAG relative luminance of a `#rrggbb` colour."""
    match = HEX.match(colour)
    assert match, f"not a plain #rrggbb colour: {colour!r}"
    raw = match.group("hex")
    channels = (int(raw[0:2], 16), int(raw[2:4], 16), int(raw[4:6], 16))
    linear = []
    for channel in channels:
        value = channel / 255
        linear.append(
            value / 12.92 if value <= 0.03928 else ((value + 0.055) / 1.055) ** 2.4
        )
    red, green, blue = linear
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


def contrast(one: str, other: str) -> float:
    """WCAG contrast ratio between two `#rrggbb` colours, >= 1."""
    high, low = sorted((luminance(one), luminance(other)), reverse=True)
    return (high + 0.05) / (low + 0.05)


def test_every_brand_svg_is_well_formed_xml():
    """An unescaped `<` in the `<style>` text is invisible to the docs gate, not to XML."""
    svgs = sorted(BRAND.glob("*.svg"))
    assert svgs, f"no SVG found under {BRAND}"
    for svg in svgs:
        root = parse(svg)
        assert root.get("viewBox") or (root.get("width") and root.get("height")), (
            f"{svg.name} has neither a viewBox nor width and height: nothing sizes the image"
        )


def test_the_logo_draws_a_light_scheme_cutout():
    default, light = cutout_fills(parse(BRAND / "logo.svg"))
    assert light != default, (
        f"the light-scheme fill is the default {default}: the rule changes nothing, "
        "so the light header still shows the paper cutout"
    )


def test_the_light_cutout_reaches_non_text_contrast():
    _, light = cutout_fills(parse(BRAND / "logo.svg"))
    css = (BRAND / "brand.css").read_text()
    assert LIGHT_HEADER in css, (
        f"{LIGHT_HEADER} is not a colour in brand.css: the header this test measures "
        "against is no longer the site's light background"
    )
    ratio = contrast(light, LIGHT_HEADER)
    assert ratio >= MIN_CONTRAST, (
        f"the light-scheme cutout {light} is {ratio:.2f}:1 on {LIGHT_HEADER}; "
        f"WCAG non-text contrast needs {MIN_CONTRAST}:1"
    )


def test_the_paper_cutout_stays_readable_on_the_dark_header():
    default, _ = cutout_fills(parse(BRAND / "logo.svg"))
    css = (BRAND / "brand.css").read_text()
    assert DARK_HEADER in css, (
        f"{DARK_HEADER} is not a colour in brand.css: the header this test measures "
        "against is no longer the site's dark background"
    )
    ratio = contrast(default, DARK_HEADER)
    assert ratio >= MIN_CONTRAST, (
        f"the default cutout {default} is {ratio:.2f}:1 on {DARK_HEADER}; "
        f"WCAG non-text contrast needs {MIN_CONTRAST}:1"
    )
