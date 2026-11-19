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

The second half of this file guards the *words*, not the markup. ADR-0010 makes
embedded `libphp` the sole PHP backend, so no page a developer reads may
describe Blastit's PHP as the retired daemon/CGI transport, teach its manifest
key, or offer it as a PHP fallback. `docs/decisions/`, `docs/design/`,
`docs/research/` and `docs/testing/` are the record and keep the old vocabulary,
and so do `done`/`superseded` plan briefs: only live claims are scanned.

The third guard is the manifest erasure (`MB.21.5.2`, design §15.2): the retired
v1 file name cannot reappear in a page, a crate, a script, the harness, the
front door, `AGENTS.md` or a live brief. Its one artefact is
`test_the_real_tree_carries_no_retired_toml_manifest`; there is no allowlist,
because a list a lazy edit can widen is the defect the guard exists to prevent.
The name is composed (`"blastit" + ".toml"`) so this module needs no exemption of
itself, and the dated record (`docs/decisions|design|research|knowledge|
testing/`, `docs/changelog.md`, `LOG.md`/`RESULT.md`, finished briefs) is outside
the scanned set by construction — it is why the name can be recognised at all.
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import tomllib
import xml.etree.ElementTree as ET
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "runnable_docs"))

# The one published-page list, shared with `docs-test`.
import fences

ROOT = Path(__file__).resolve().parents[2]
BRAND = ROOT / "docs" / "brand"

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


# --- The retired PHP transport vocabulary (ADR-0010) ------------------------

#: What to say instead. Every failure message ends with this truth, so a
#: developer reading a gate output knows the one supported PHP model.
PHP_TRUTH = "embedded `libphp`, `[route.php]`; see ADR-0010"

#: ADR-0010 section 1: embedded NTS `libphp` is the *sole* PHP backend. The old
#: words survive only where a line names the attack Blastit neutralizes
#: (`php2shell`, `HTTP/FastCGI desync`), never as what Blastit runs. This is the
#: one allowance list: widening it is how the retired transport comes back.
ATTACK_MARKERS = ("attack", "desync", "parameter trick", "smuggling")
#: A live plan brief may also cite the CVE number it neutralizes.
PLAN_ATTACK_MARKERS = (*ATTACK_MARKERS, "cve")

#: `php_fpm`, `php-fpm` and `phpfpm`; `route.fastcgi` is its own spelling so the
#: message can name the manifest key a reader would paste; bare `fastcgi` last,
#: because the alternation is tried left to right.
RETIRED_SPELLING = re.compile(
    r"(?P<daemon>php[-_ ]?fpm)"
    r"|(?P<key>route\.fastcgi)"
    r"|(?P<transport>fastcgi)",
    re.IGNORECASE,
)
#: An *offer* term: the verb ("fall back", "falls back") and the noun/adjective,
#: singular or plural ("fallback", "fallbacks", and so "as a fallback").
FALLBACK_TERM = re.compile(r"\bfalls?\s+back\b|\bfallbacks?\b", re.IGNORECASE)
#: A clause is about PHP when it names the app, the retired transport, or the
#: process/route the offer would run.
PHP_CLAUSE = re.compile(r"php|fastcgi|daemon|libphp|route", re.IGNORECASE)
#: D1 — the negation governs the *noun*: `no`/`never`/`zero` plus `fallback`,
#: `alternative`, `degraded mode` or `second way` within 0-3 words.
DENIAL_NOUN = re.compile(
    r"\b(?:no|never|zero)\b(?:\s+\S+){0,3}?\s+"
    r"(?:fallbacks?|alternatives?|degraded modes?|second ways?)\b",
    re.IGNORECASE,
)
#: D2 — the negation governs the *verb*: `never`/`do not`/`don't`/`must not`
#: plus `run|use|serve|add|fall back to` and then the fallback noun, or the plain
#: "never falls back to" that denies the act itself.
DENIAL_VERB = re.compile(
    r"\b(?:never|do\s+not|don't|must\s+not)\b(?:\s+\S+){0,2}?\s+"
    r"(?:run|use|serve|add|fall\s+back\s+to)\b(?:\s+\S+){0,4}?\s+fallback\b"
    r"|\b(?:never|do\s+not|don't|must\s+not)\s+falls?\s+back\s+to\b",
    re.IGNORECASE,
)
#: A sentence is cut into clauses here, so a denial in one clause never excuses
#: an offer in another ("There is no fallback: fall back to a daemon", or
#: "There is no fallback, and you may use a daemon as a fallback").
CLAUSE_BREAK = re.compile(
    r";|:|\s*(?:—|--)\s*|,\s*but\b|,\s*though\b|,\s*however\b"
    r"|,\s*and\s+yet\b|,\s*(?:and|so|then|therefore)\b",
    re.IGNORECASE,
)

#: A sentence ends after `.`/`!`/`?` and whitespace; a wrapped line is just
#: whitespace once a paragraph is flattened, so a sentence may span two lines.
SENTENCE_BOUNDARY = re.compile(r"(?<=[.!?])\s+")
#: ...but these are not full stops, however the paragraph is laid out.
ABBREVIATIONS = frozenset({"e.g", "i.e", "etc", "vs", "cf", "approx", "resp"})
#: A period closing a token that carries a slash is a path or URL, not a stop:
#: `docs/reference/manifest.md` and `manifest.md)` both keep the sentence open.
LAST_TOKEN = re.compile(r"(?P<token>[A-Za-z0-9_./:#@~%-]+)\.$")
#: A Markdown block starts a new paragraph even without a blank line above it,
#: so a sentence never spans two bullets or two table rows.
BLOCK_START = re.compile(r"[ \t]*(?:[-*+][ \t]|\d+[.)][ \t]|#|>|\||```|~~~)")

RETIRED_DETAIL = f"not Blastit's PHP transport; use {PHP_TRUTH}"
OFFER_DETAIL = f"a PHP fallback is refused, not offered; use {PHP_TRUTH}"
DEAD_LINK_DETAIL = f"cites a deleted design file; use {PHP_TRUTH}"

#: The pages a developer installs from; the vocabulary must not return there.
PRODUCT_DIRS = ("docs/frameworks", "docs/deploy", "docs/reference")
VISION_PAGE = Path("docs/VISION.md")
#: This brief *defines* the guard, so it must be able to name what it bans.
BRIEF_DEFINING_THE_LIST = Path("plan/nodes/MB/MB.3/MB.3.30/NODE.md")
#: A brief in one of these states is history, not a live claim.
RETIRED_STATUSES = frozenset({"done", "superseded"})
#: MB.3.32 deleted this design; a comment citing it as authority is a dead end.
DELETED_DESIGN = "docs/design/php-fastcgi.md"

FRONTMATTER = re.compile(r"\A---\r?\n(?P<body>.*?)\r?\n---\r?\n", re.DOTALL)
STATUS_LINE = re.compile(r"^status:[ \t]*(?P<status>[^\s#]+)", re.MULTILINE)


@dataclass(frozen=True, slots=True)
class Finding:
    """One defect: where it is (`<file>:<line>`), what was matched, and the fix."""

    where: str
    word: str
    detail: str

    def __str__(self) -> str:
        return f"{self.where}: {self.word!r}: {self.detail}"


@dataclass(frozen=True, slots=True)
class Paragraph:
    """Wrapped lines of one Markdown paragraph, flattened into one string.

    `lines[i]` is the 1-based source line of `text[i]`, so a sentence found in
    the flattened text still reports the line a reader has to open. `prose` is
    False for a fenced code block, which has no sentences to read.
    """

    text: str
    lines: tuple[int, ...]
    prose: bool


def text_of(path: Path) -> str:
    """The page or source as UTF-8; a non-UTF-8 doc is itself a defect."""
    return path.read_text(encoding="utf-8")


def label_of(root: Path, path: Path) -> str:
    """`path` relative to `root` in POSIX form, so a message is copy-pasteable."""
    return path.relative_to(root).as_posix()


def names_attack(line: str, markers: tuple[str, ...]) -> bool:
    """Whether `line` names the attack class rather than Blastit's transport."""
    folded = line.casefold()
    return any(marker in folded for marker in markers)


def ends_sentence(text: str, punctuation: int) -> bool:
    """Whether the terminator at `punctuation` really ends a sentence."""
    if text[punctuation] != ".":
        return True
    token = LAST_TOKEN.search(text, 0, punctuation + 1)
    if token is None:
        return True
    word = token.group("token")
    if word.casefold() in ABBREVIATIONS:
        return False
    return "/" not in word


def sentences(text: str) -> Iterator[tuple[int, int]]:
    """The `[left, right)` spans of the sentences of a flattened paragraph.

    A terminator ends a sentence only when `ends_sentence` agrees, so `e.g.` and
    a period closing a path or URL (`docs/reference/manifest.md`) do not split.
    """
    left = 0
    for boundary in SENTENCE_BOUNDARY.finditer(text):
        if ends_sentence(text, boundary.start() - 1):
            yield left, boundary.start()
            left = boundary.end()
    yield left, len(text)


def clauses(text: str, start: int, end: int) -> Iterator[tuple[int, int]]:
    """The `[left, right)` spans of the clauses of `text[start:end]`.

    A denial is only ever read inside its own clause, so an offer in a
    contrasting clause is never excused by the denial next door.
    """
    left = start
    for boundary in CLAUSE_BREAK.finditer(text, start, end):
        yield left, boundary.start()
        left = boundary.end()
    yield left, end


def paragraphs(text: str) -> list[Paragraph]:
    """The Markdown paragraphs of `text`, wrapped lines flattened per paragraph.

    A fenced code block is one non-prose block: it has no sentences, and the
    vocabulary scan (which works line by line) still covers every line of it.
    """
    blocks: list[tuple[list[tuple[int, str]], bool]] = []
    current: list[tuple[int, str]] = []
    fenced = False
    for number, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()
        fence = stripped.startswith(("```", "~~~"))
        if fenced:
            current.append((number, line))
            if fence:
                blocks.append((current, False))
                current = []
                fenced = False
            continue
        if fence:
            if current:
                blocks.append((current, True))
            current = [(number, line)]
            fenced = True
            continue
        if not stripped:
            if current:
                blocks.append((current, True))
                current = []
            continue
        if current and BLOCK_START.match(line):
            blocks.append((current, True))
            current = []
        current.append((number, line))
    if current:
        blocks.append((current, not fenced))

    flattened = []
    for block, prose in blocks:
        characters: list[str] = []
        origins: list[int] = []
        for number, line in block:
            if characters:
                characters.append(" ")
                origins.append(number)
            characters.extend(line)
            origins.extend([number] * len(line))
        flattened.append(Paragraph("".join(characters), tuple(origins), prose))
    return flattened


def vocabulary_findings(
    label: str,
    text: str,
    *,
    attack_markers: tuple[str, ...] | None,
) -> list[Finding]:
    """Retired PHP spellings in `text`.

    `attack_markers=None` forbids bare `fastcgi` outright (a product page never
    needs it); otherwise a line that names the attack may keep the word, while
    `php-fpm` and `route.fastcgi` fail on every line.
    """
    findings = []
    for number, line in enumerate(text.splitlines(), 1):
        exempt = attack_markers is not None and names_attack(line, attack_markers)
        for match in RETIRED_SPELLING.finditer(line):
            if exempt and match.lastgroup == "transport":
                continue
            findings.append(
                Finding(f"{label}:{number}", match.group(0), RETIRED_DETAIL)
            )
    return findings


def fallback_findings(label: str, text: str) -> list[Finding]:
    """PHP fallback *offers* in `text`, judged one clause at a time.

    An offer term in a clause that names PHP (or the transport, daemon, `libphp`
    or a route) is a defect unless that same clause matches an approved denial
    shape: D1 (a negation governing the noun) or D2 (a negation governing the
    verb). Nothing is inherited across `;`, `:`, an em dash or `, but`: a denial
    in one clause never excuses an offer in the next.
    """
    findings = []
    for paragraph in paragraphs(text):
        if not paragraph.prose:
            continue
        for sentence in sentences(paragraph.text):
            for clause_left, clause_right in clauses(paragraph.text, *sentence):
                clause = paragraph.text[clause_left:clause_right]
                offer = FALLBACK_TERM.search(clause)
                if offer is None:
                    continue
                if not PHP_CLAUSE.search(clause):
                    continue
                if DENIAL_NOUN.search(clause) or DENIAL_VERB.search(clause):
                    continue
                offset = clause_left + offer.start()
                findings.append(
                    Finding(
                        f"{label}:{paragraph.lines[offset]}",
                        offer.group(0),
                        OFFER_DETAIL,
                    )
                )
    return findings


def dead_design_findings(label: str, text: str) -> list[Finding]:
    """Every line of a Rust source citing the design MB.3.32 deleted."""
    return [
        Finding(f"{label}:{number}", DELETED_DESIGN, DEAD_LINK_DETAIL)
        for number, line in enumerate(text.splitlines(), 1)
        if DELETED_DESIGN in line
    ]


def markdown_files(directory: Path) -> list[Path]:
    """Every Markdown page under `directory`, sorted, files only."""
    if not directory.is_dir():
        return []
    return sorted(path for path in directory.rglob("*.md") if path.is_file())


def product_doc_files(root: Path) -> list[Path]:
    """The pages a developer installs from: README plus the user guides."""
    pages = [root / "README.md"]
    for relative in PRODUCT_DIRS:
        pages.extend(markdown_files(root / relative))
    return [path for path in pages if path.is_file()]


def status_of(text: str) -> str:
    """The frontmatter `status` of a plan brief, or "" when it has none."""
    frontmatter = FRONTMATTER.match(text)
    if not frontmatter:
        return ""
    status = STATUS_LINE.search(frontmatter.group("body"))
    return status.group("status").strip("\"'") if status else ""


def live_briefs(root: Path) -> list[Path]:
    """Plan briefs still driving work; `done`/`superseded` briefs are history.

    `LOG.md` and `RESULT.md` are never candidates: they are records too.
    """
    briefs = []
    for path in sorted((root / "plan" / "nodes").rglob("NODE.md")):
        if path.relative_to(root) == BRIEF_DEFINING_THE_LIST:
            continue
        if status_of(text_of(path)) in RETIRED_STATUSES:
            continue
        briefs.append(path)
    return briefs


def rust_sources(root: Path) -> list[Path]:
    """Every Rust source under `crates/`, so a dead citation cannot hide."""
    crates = root / "crates"
    return sorted(crates.rglob("*.rs")) if crates.is_dir() else []


def guard_tree(root: Path) -> list[Finding]:
    """Every retired-vocabulary, fallback-offer and dead-citation defect."""
    findings: list[Finding] = []
    for path in product_doc_files(root):
        label = label_of(root, path)
        text = text_of(path)
        findings += vocabulary_findings(label, text, attack_markers=None)
        findings += fallback_findings(label, text)
    vision = root / VISION_PAGE
    if vision.is_file():
        label = label_of(root, vision)
        text = text_of(vision)
        findings += vocabulary_findings(label, text, attack_markers=ATTACK_MARKERS)
        findings += fallback_findings(label, text)
    for path in live_briefs(root):
        label = label_of(root, path)
        text = text_of(path)
        findings += vocabulary_findings(label, text, attack_markers=PLAN_ATTACK_MARKERS)
        findings += fallback_findings(label, text)
    for path in rust_sources(root):
        findings += dead_design_findings(label_of(root, path), text_of(path))
    return findings


def write_page(root: Path, relative: str, text: str) -> Path:
    """A crafted page at `root/relative`, so a direction test needs no fixtures."""
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def test_a_product_page_may_not_teach_the_retired_php_transport(tmp_path):
    """Every retired spelling is a defect on a page a developer installs from."""
    write_page(
        tmp_path,
        "docs/frameworks/php.md",
        "Run it under PHP-FPM.\n"
        'Use [route.FASTCGI] listen = "/run/php.sock".\n'
        "Tune php-fpm.conf.\n"
        "The FastCGI worker answers.\n"
        "PHP runs embedded `libphp` via [route.php].\n",
    )
    findings = guard_tree(tmp_path)
    assert [finding.where for finding in findings] == [
        "docs/frameworks/php.md:1",
        "docs/frameworks/php.md:2",
        "docs/frameworks/php.md:3",
        "docs/frameworks/php.md:4",
    ]
    assert [finding.word for finding in findings] == [
        "PHP-FPM",
        "route.FASTCGI",
        "php-fpm",
        "FastCGI",
    ]
    for finding in findings:
        assert PHP_TRUTH in str(finding), finding


def test_the_record_keeps_the_superseded_words(tmp_path):
    """ADRs, design, research and audit reports are history, not claims."""
    write_page(tmp_path, "docs/decisions/ADR-0008-fastcgi.md", "A php-fpm pool.\n")
    write_page(
        tmp_path, "docs/design/php-fastcgi.md", "php-fpm.conf, [route.fastcgi].\n"
    )
    write_page(tmp_path, "docs/research/php2shell.md", "A FastCGI parameter trick.\n")
    write_page(tmp_path, "docs/testing/AUDIT-1.md", "The FastCGI worker.\n")
    assert guard_tree(tmp_path) == []
    assert product_doc_files(tmp_path) == []


def test_vision_may_name_the_attack_but_not_our_transport(tmp_path):
    """`docs/VISION.md` keeps the word only on a line naming the attack class.

    The marker excuses bare `fastcgi` only: a daemon name fails even on an
    attack line, which is the whole point of the guard.
    """
    write_page(
        tmp_path,
        "docs/VISION.md",
        "A FastCGI/CGI parameter trick (`php2shell`) hands over the app.\n"
        "HTTP/FastCGI desync is neutralized.\n"
        "Every route runs php-fpm today.\n"
        "A FastCGI surface is fine.\n"
        "The attack exploited php-fpm's CVE.\n",
    )
    assert [(finding.where, finding.word) for finding in guard_tree(tmp_path)] == [
        ("docs/VISION.md:3", "php-fpm"),
        ("docs/VISION.md:4", "FastCGI"),
        ("docs/VISION.md:5", "php-fpm"),
    ]


def test_the_vision_allowance_is_exactly_the_agreed_attack_markers():
    """Widening the allowance is how the retired transport comes back."""
    assert ATTACK_MARKERS == ("attack", "desync", "parameter trick", "smuggling")
    assert PLAN_ATTACK_MARKERS == (*ATTACK_MARKERS, "cve")


def test_a_live_brief_may_not_describe_our_php_as_fastcgi(tmp_path):
    """A ready brief is a live claim; only the attack class may keep the word."""
    write_page(
        tmp_path,
        "plan/nodes/MB/MB.3/MB.3.5/NODE.md",
        "---\n"
        "id: MB.3.5\n"
        "status: ready\n"
        "---\n"
        "\n"
        "PHP routes use `[route.fastcgi]`.\n"
        "The worker speaks FastCGI.\n"
        "It neutralizes the FastCGI desync attack.\n"
        "It closes the FastCGI CVE.\n"
        "The CVE list still covers `[route.fastcgi]`.\n",
    )
    assert [(finding.where, finding.word) for finding in guard_tree(tmp_path)] == [
        ("plan/nodes/MB/MB.3/MB.3.5/NODE.md:6", "route.fastcgi"),
        ("plan/nodes/MB/MB.3/MB.3.5/NODE.md:7", "FastCGI"),
        ("plan/nodes/MB/MB.3/MB.3.5/NODE.md:10", "route.fastcgi"),
    ]


def test_a_finished_brief_is_history_not_a_claim(tmp_path):
    """`done`/`superseded` briefs describe what was, so they keep the words."""
    write_page(
        tmp_path,
        "plan/nodes/MB/MB.3/MB.3.5/NODE.md",
        "---\nstatus: done\n---\nRun php-fpm over FastCGI via [route.fastcgi].\n",
    )
    write_page(
        tmp_path,
        "plan/nodes/MB/MB.3/MB.3.6/NODE.md",
        "---\nstatus: superseded\n---\nA php-fpm pool.\n",
    )
    assert guard_tree(tmp_path) == []


def test_brief_records_are_never_scanned(tmp_path):
    """`LOG.md` and `RESULT.md` are evidence of past attempts, not claims."""
    write_page(tmp_path, "plan/nodes/MB/MB.3/MB.3.5/LOG.md", "php-fpm over FastCGI.\n")
    write_page(
        tmp_path, "plan/nodes/MB/MB.3/MB.3.5/RESULT.md", "php-fpm over FastCGI.\n"
    )
    assert guard_tree(tmp_path) == []


def test_a_brief_without_a_status_is_still_a_claim(tmp_path):
    """Only `done`/`superseded` are history, so the guard fails closed."""
    write_page(tmp_path, "plan/nodes/MB/MB.3/MB.3.7/NODE.md", "# Goal\nphp-fpm.\n")
    assert [finding.word for finding in guard_tree(tmp_path)] == ["php-fpm"]


def test_readme_is_a_product_page(tmp_path):
    """The install page is the first page a developer reads."""
    write_page(tmp_path, "README.md", "It runs php-fpm.\n")
    assert [finding.where for finding in guard_tree(tmp_path)] == ["README.md:1"]


def test_no_rust_source_cites_the_deleted_design(tmp_path):
    """A comment citing `docs/design/php-fastcgi.md` names a file that is gone."""
    write_page(
        tmp_path,
        "crates/blastit-php/src/lib.rs",
        "//! See docs/design/php-fastcgi.md for the wire.\nfn main() {}\n",
    )
    assert [(finding.where, finding.word) for finding in guard_tree(tmp_path)] == [
        ("crates/blastit-php/src/lib.rs:1", "docs/design/php-fastcgi.md"),
    ]


def test_a_rust_source_may_cite_a_live_design(tmp_path):
    """Only the deleted design is a dead end; a live design doc is authority."""
    write_page(
        tmp_path,
        "crates/blastit-view/src/lib.rs",
        "//! See docs/design/backend-runtimes.md for the model.\n",
    )
    write_page(
        tmp_path,
        "crates/blastit-php/src/lib.rs",
        "//! See docs/design/php-fastcgi.md for the wire.\n",
    )
    assert [finding.where for finding in guard_tree(tmp_path)] == [
        "crates/blastit-php/src/lib.rs:1",
    ]


def test_a_fallback_offer_is_refused_not_offered(tmp_path):
    """Both the verb and the noun form of an offer are the defect."""
    write_page(
        tmp_path,
        "docs/frameworks/php.md",
        "If `libphp` is missing, use a daemon as a fallback for PHP.\n"
        "A missing `libphp` means fall back to a daemon for PHP.\n",
    )
    assert [(finding.where, finding.word) for finding in guard_tree(tmp_path)] == [
        ("docs/frameworks/php.md:1", "fallback"),
        ("docs/frameworks/php.md:2", "fall back"),
    ]


def test_a_fallback_offer_survives_an_abbreviation(tmp_path):
    """`e.g.` is not a full stop: the sentence still holds the PHP it offers."""
    write_page(
        tmp_path,
        "docs/frameworks/php.md",
        "Fall back to a daemon, e.g. for PHP routes.\n",
    )
    assert [finding.word for finding in guard_tree(tmp_path)] == ["Fall back"]


def test_a_fallback_offer_survives_a_path_in_between(tmp_path):
    """A dotted path is not a full stop, so the clause keeps its PHP."""
    write_page(
        tmp_path,
        "docs/frameworks/php.md",
        "Use the daemon as a fallback (see docs/reference/manifest.md) for PHP.\n",
    )
    assert [(finding.where, finding.word) for finding in guard_tree(tmp_path)] == [
        ("docs/frameworks/php.md:1", "fallback"),
    ]


def test_a_fallback_offer_survives_a_wrapped_line(tmp_path):
    """A sentence wrapped across two lines is one sentence; the offer names line 2."""
    write_page(
        tmp_path,
        "docs/frameworks/php.md",
        "A PHP route with a `spa` block answers the file first, and\n"
        "its fallback is a daemon.\n",
    )
    assert [(finding.where, finding.word) for finding in guard_tree(tmp_path)] == [
        ("docs/frameworks/php.md:2", "fallback"),
    ]


def test_every_other_php_fallback_offer_fails(tmp_path):
    """A denial never crosses `;`, `:`, an em dash or `, but` into an offer."""
    write_page(
        tmp_path,
        "docs/frameworks/php.md",
        "If `libphp` is not available, fall back to a daemon for PHP.\n"
        "Without `libphp`, fall back to a daemon for PHP routes.\n"
        "Do not stop at PHP: use a daemon as a fallback.\n"
        "There is no fallback — you may fall back to a daemon for PHP.\n"
        "No fallback exists; use a daemon as a fallback for PHP.\n"
        "There is no fallback, but you may use a daemon as a fallback for PHP.\n"
        "There is no fallback: fall back to a daemon for PHP routes.\n"
        "The fallback for PHP routes is a daemon.\n"
        "PHP's fallback is a daemon.\n"
        "PHP does not have a fallback when the daemon is missing.\n"
        "There is no fallback, and you may use a daemon as a fallback for PHP.\n"
        "There is no fallback, so use a daemon as a fallback for PHP routes.\n"
        "PHP fallbacks are a daemon.\n",
    )
    assert [(finding.where, finding.word) for finding in guard_tree(tmp_path)] == [
        ("docs/frameworks/php.md:1", "fall back"),
        ("docs/frameworks/php.md:2", "fall back"),
        ("docs/frameworks/php.md:3", "fallback"),
        ("docs/frameworks/php.md:4", "fall back"),
        ("docs/frameworks/php.md:5", "fallback"),
        ("docs/frameworks/php.md:6", "fallback"),
        ("docs/frameworks/php.md:7", "fall back"),
        ("docs/frameworks/php.md:8", "fallback"),
        ("docs/frameworks/php.md:9", "fallback"),
        ("docs/frameworks/php.md:10", "fallback"),
        ("docs/frameworks/php.md:11", "fallback"),
        ("docs/frameworks/php.md:12", "fallback"),
        ("docs/frameworks/php.md:13", "fallbacks"),
    ]


def test_a_php_fallback_denial_passes(tmp_path):
    """Only D1 (noun) and D2 (verb) denials pass, and only in their own clause."""
    write_page(
        tmp_path,
        "docs/frameworks/php.md",
        "There is no fallback; embedded `libphp` is the only way to run PHP.\n"
        "A PHP route has no fallback and no second way.\n"
        "Never a fallback: `[route.php]` is the only PHP route.\n"
        "Never run a daemon as a fallback for PHP routes.\n"
        "Do not use a daemon as a fallback for PHP.\n"
        "PHP routes never fall back to a daemon.\n"
        "This page does not mention PHP.\n",
    )
    assert guard_tree(tmp_path) == []


def test_a_dotted_path_or_abbreviation_is_not_a_full_stop():
    """A sentence needs a real full stop, not `e.g.` or a file extension."""
    path = "see docs/reference/manifest.md now"
    assert not ends_sentence(path, path.index("."))
    abbreviation = "a daemon, e.g. for PHP"
    assert not ends_sentence(abbreviation, abbreviation.rindex("."))
    sentence = "a `spa` block. Then"
    assert ends_sentence(sentence, sentence.index("."))


def test_the_real_tree_carries_no_retired_php_vocabulary():
    """The residue MB.3.31/MB.3.32 removed cannot come back unnoticed."""
    findings = guard_tree(ROOT)
    assert not findings, "\n".join(str(finding) for finding in findings)


def test_the_guard_scans_a_real_tree():
    """Anti-vacuity: the populations and the exemptions are both load-bearing."""
    assert (ROOT / VISION_PAGE).is_file()
    assert product_doc_files(ROOT), "no product pages found"
    briefs = sorted((ROOT / "plan" / "nodes").rglob("NODE.md"))
    assert 0 < len(live_briefs(ROOT)) < len(briefs)
    finished = [page for page in briefs if status_of(text_of(page)) in RETIRED_STATUSES]
    assert any(RETIRED_SPELLING.search(text_of(page)) for page in finished), (
        "no finished brief still names the retired transport: the exemption is dead"
    )
    assert rust_sources(ROOT), "no Rust sources found"
    decisions = markdown_files(ROOT / "docs" / "decisions")
    assert any(RETIRED_SPELLING.search(text_of(page)) for page in decisions), (
        "no ADR still names the retired transport: the exemption has no subject"
    )


# --- The retired v1 manifest file name (MB.21.5.2, design §15.2) -------------

#: Composed so this module never carries the name contiguously: the guard scans
#: `python/**/*.py`, itself included, and there is no allowlist.
RETIRED_MANIFEST_NAME = "blastit" + ".toml"
RETIRED_MANIFEST = re.compile(r"blastit[.]toml", re.IGNORECASE)
#: What a reader must do instead. Every failure ends with this truth.
RETIRED_MANIFEST_DETAIL = (
    "the v1 manifest format is retired and has no migration path: read "
    "`./Blastfile` or name one with `BLASTIT_MANIFEST`"
)
#: The front door, the living spec and the vision, beside the published pages.
FRONT_DOOR = ("README.md", "ARCHITECTURE.md", "AGENTS.md", "docs/VISION.md")
#: Harness trees are read as text: Python, shell, YAML, Dockerfiles and Markdown.
HARNESS_SUFFIXES = (".py", ".sh", ".yaml", ".yml", ".md", ".toml", ".json", ".txt")
#: The dependency a resurrected reader would need (§15.2).
RETIRED_PACKAGES = ("toml", "toml_edit", "toml_datetime", "toml_parser", "toml_writer")
RETIRED_REFERENCE = "docs/reference/manifest.md"
#: A published page that names the retired format without naming its file: the
#: `v1` token (but not a Kubernetes `apps/v1`, an image tag `:v1.26.1` or
#: `setup-uv@v10`) and a `gone: <retired key>` row.
RETIRED_FORMAT_WORD = re.compile(r"(?<![\w/:@.-])v1(?![\w.-])")
RETIRED_ROW = re.compile(r"\bgone:", re.IGNORECASE)
RETIRED_FORMAT_DETAIL = (
    "names the retired manifest format without naming its file: describe the "
    "`Blastfile` only, and delete the old-format row instead of marking it gone"
)
#: Pages a `zensical build` publishes that are **records**, not claims. The
#: changelog is a published page, but it is the history of what happened (and
#: the release notes name the retired file by necessity); scanning it would
#: either force a rewrite of history or need the allowlist this guard refuses.
#: The module docstring and `NODE.md` both call it a record, so the code does.
RECORD_PAGES = ("docs/changelog.md",)
#: A minimal `zensical.toml` for a synthetic tree, with the dated record excluded
#: exactly as the real site excludes it.
ZENSICAL_EXCERPT = (
    "[project]\n"
    'docs_dir = "docs"\n'
    'exclude_docs = """\n'
    "/design/\n"
    "/decisions/\n"
    "/research/\n"
    "/knowledge/\n"
    "/testing/\n"
    '"""\n'
)


def published_page_files(root: Path) -> list[Path]:
    """`README.md` plus every page `zensical build` publishes, by path.

    `RECORD_PAGES` is removed after the published list is built, so the set is
    still "what the site builds" and a record that happens to be published (the
    changelog) cannot make the guard demand a rewrite of history.
    """
    zensical = root / "zensical.toml"
    assert zensical.is_file(), (
        f"{label_of(root, zensical)} is missing: the published-page set cannot be "
        "derived, so the guard would pass vacuously"
    )
    pages = [
        page
        for page in fences.public_pages(root, text_of(zensical))
        if page not in RECORD_PAGES
    ]
    return [root / page for page in pages if (root / page).is_file()]


def live_plan_briefs(root: Path) -> list[Path]:
    """Every plan brief still driving work, with **no** brief exemption.

    The PHP guard exempts the brief that defines its list; this guard has no
    defining brief, because after `MB.21.5.1` every live brief is clean.
    """
    return [
        path
        for path in sorted((root / "plan" / "nodes").rglob("NODE.md"))
        if status_of(text_of(path)) not in RETIRED_STATUSES
    ]


def harness_files(root: Path) -> list[Path]:
    """`scripts/**`, `e2e/**` and the `Makefile`, as text."""
    files: list[Path] = []
    for base in ("scripts", "e2e"):
        directory = root / base
        if not directory.is_dir():
            continue
        for path in sorted(directory.rglob("*")):
            if path.is_file() and (
                path.name.startswith("Dockerfile") or path.suffix in HARNESS_SUFFIXES
            ):
                files.append(path)
    makefile = root / "Makefile"
    if makefile.is_file():
        files.append(makefile)
    return files


def retired_manifest_paths(root: Path) -> list[Path]:
    """Every file the erased name may not appear in, once each."""
    paths = [root / name for name in FRONT_DOOR]
    paths += published_page_files(root)
    crates = root / "crates"
    if crates.is_dir():
        paths += sorted(crates.rglob("*.rs"))
        paths += sorted(crates.rglob("Cargo.toml"))
    python_dir = root / "python"
    if python_dir.is_dir():
        paths += sorted(python_dir.rglob("*.py"))
    inventory = python_dir / "tests" / "runnable_docs"
    if inventory.is_dir():
        paths += sorted(inventory.glob("*.toml"))
    paths += harness_files(root)
    paths += live_plan_briefs(root)
    unique: list[Path] = []
    seen: set[Path] = set()
    for path in paths:
        if path.is_file() and path not in seen:
            seen.add(path)
            unique.append(path)
    return unique


def retired_dependency_findings(root: Path) -> list[Finding]:
    """A `toml`/`toml_edit` dependency, or a lock entry, means a reader came back."""
    findings: list[Finding] = []
    for relative in ("Cargo.toml", "crates/blastit-manifest/Cargo.toml"):
        path = root / relative
        if not path.is_file():
            continue
        for number, line in enumerate(text_of(path).splitlines(), 1):
            stripped = line.strip()
            for package in RETIRED_PACKAGES:
                table = re.fullmatch(
                    rf"\[(?:workspace\.)?dependencies\.{package}\]", stripped
                )
                key = re.fullmatch(rf"['\"]?{package}['\"]?\s*=.*", stripped)
                if table or key:
                    findings.append(
                        Finding(
                            f"{relative}:{number}",
                            package,
                            "the TOML reader is retired with the format; a `Blastfile` "
                            "is read by `blastit-manifest`'s own parser (§15.2)",
                        )
                    )
    lock = root / "Cargo.lock"
    if lock.is_file():
        for number, line in enumerate(text_of(lock).splitlines(), 1):
            match = re.fullmatch(r'name = "(toml(?:_.*)?)"', line.strip())
            if match:
                findings.append(
                    Finding(
                        f"Cargo.lock:{number}",
                        match.group(1),
                        "a retired TOML crate is still reachable from the workspace (§15.2)",
                    )
                )
    return findings


def retired_reference_findings(root: Path) -> list[Finding]:
    """The retired reference page must not be recreated."""
    path = root / RETIRED_REFERENCE
    if not path.is_file():
        return []
    return [
        Finding(
            RETIRED_REFERENCE,
            "reference/manifest.md",
            "the retired reference page must stay deleted; `docs/reference/blastfile.md` "
            "is the reference",
        )
    ]


def retired_manifest_findings(root: Path) -> list[Finding]:
    """Every occurrence of the erased file name, the reader and the dead page."""
    findings: list[Finding] = []
    for path in retired_manifest_paths(root):
        label = label_of(root, path)
        for number, line in enumerate(text_of(path).splitlines(), 1):
            for match in RETIRED_MANIFEST.finditer(line):
                findings.append(
                    Finding(
                        f"{label}:{number}", match.group(0), RETIRED_MANIFEST_DETAIL
                    )
                )
    findings += retired_dependency_findings(root)
    findings += retired_reference_findings(root)
    return findings


def retired_format_word_findings(root: Path) -> list[Finding]:
    """`v1`/`gone:` on a page a developer reads, without naming the file."""
    findings: list[Finding] = []
    for path in published_page_files(root):
        label = label_of(root, path)
        for number, line in enumerate(text_of(path).splitlines(), 1):
            for pattern in (RETIRED_FORMAT_WORD, RETIRED_ROW):
                match = pattern.search(line)
                if match:
                    findings.append(
                        Finding(
                            f"{label}:{number}", match.group(0), RETIRED_FORMAT_DETAIL
                        )
                    )
    return findings


def write_zensical(root: Path) -> None:
    """The minimal site descriptor a synthetic tree needs for `public_pages`."""
    write_page(root, "zensical.toml", ZENSICAL_EXCERPT)


def test_the_real_tree_carries_no_retired_toml_manifest():
    """The name `MB.21.4.5`/`MB.21.5.1` erased cannot come back unnoticed."""
    findings = retired_manifest_findings(ROOT)
    assert not findings, "\n".join(str(finding) for finding in findings)


def test_no_published_page_names_the_retired_format():
    """`v1` or `gone: <key>` leaks the format without naming the file."""
    findings = retired_format_word_findings(ROOT)
    assert not findings, "\n".join(str(finding) for finding in findings)


def test_a_published_page_may_not_name_the_retired_manifest(tmp_path):
    """A page a developer installs from is a product page, not a record."""
    write_zensical(tmp_path)
    write_page(
        tmp_path, "docs/frameworks/php.md", f"Save it as {RETIRED_MANIFEST_NAME}.\n"
    )
    findings = retired_manifest_findings(tmp_path)
    assert [(finding.where, finding.word) for finding in findings] == [
        ("docs/frameworks/php.md:1", RETIRED_MANIFEST_NAME)
    ]
    assert RETIRED_MANIFEST_DETAIL in str(findings[0])


def test_the_record_may_name_the_retired_manifest(tmp_path):
    """ADRs, design notes and finished briefs are history, not claims."""
    write_zensical(tmp_path)
    write_page(
        tmp_path,
        "docs/decisions/ADR-0012-blastfile-dsl.md",
        f"The former {RETIRED_MANIFEST_NAME} became `Blastfile`.\n",
    )
    write_page(
        tmp_path,
        "plan/nodes/MB/MB.21/MB.21.4/NODE.md",
        f"---\nstatus: done\n---\nDelete {RETIRED_MANIFEST_NAME}.\n",
    )
    assert retired_manifest_findings(tmp_path) == []


def test_the_changelog_is_a_published_record(tmp_path):
    """The site builds the changelog, but it is history and keeps the name."""
    write_zensical(tmp_path)
    write_page(tmp_path, "docs/changelog.md", f"- Removed {RETIRED_MANIFEST_NAME}.\n")
    write_page(tmp_path, "docs/frameworks/php.md", f"Save {RETIRED_MANIFEST_NAME}.\n")
    findings = retired_manifest_findings(tmp_path)
    assert [(finding.where, finding.word) for finding in findings] == [
        ("docs/frameworks/php.md:1", RETIRED_MANIFEST_NAME)
    ]
    # The word rule treats the changelog as a record for the same reason.
    write_page(tmp_path, "docs/changelog.md", "v1 is gone: the key was removed.\n")
    assert retired_format_word_findings(tmp_path) == []


def test_a_live_brief_may_not_name_the_retired_manifest(tmp_path):
    """No in-flight brief can carry the retired format back in."""
    write_zensical(tmp_path)
    write_page(
        tmp_path,
        "plan/nodes/MB/MB.21/MB.21.9/NODE.md",
        f"---\nstatus: ready\n---\nKeep {RETIRED_MANIFEST_NAME} working.\n",
    )
    assert [
        (finding.where, finding.word) for finding in retired_manifest_findings(tmp_path)
    ] == [("plan/nodes/MB/MB.21/MB.21.9/NODE.md:4", RETIRED_MANIFEST_NAME)]


def test_a_crate_or_a_script_may_not_name_the_retired_manifest(tmp_path):
    """The readers and the harnesses are scanned too, not only the pages."""
    write_zensical(tmp_path)
    write_page(
        tmp_path,
        "crates/blastit/src/config.rs",
        f"//! Discovery used to read ./{RETIRED_MANIFEST_NAME}.\n",
    )
    write_page(
        tmp_path,
        "scripts/boot.sh",
        f"exec blastit --manifest {RETIRED_MANIFEST_NAME}\n",
    )
    write_page(
        tmp_path, "Makefile", f"serve:\n\tblastit --manifest {RETIRED_MANIFEST_NAME}\n"
    )
    write_page(tmp_path, "AGENTS.md", f"Name {RETIRED_MANIFEST_NAME} nowhere.\n")
    findings = retired_manifest_findings(tmp_path)
    assert [finding.where for finding in findings] == [
        "AGENTS.md:1",
        "crates/blastit/src/config.rs:1",
        "scripts/boot.sh:1",
        "Makefile:2",
    ]


def test_the_retired_reader_and_reference_are_refused(tmp_path):
    """A dependency on a TOML crate, or the deleted page, is the format again."""
    write_zensical(tmp_path)
    write_page(tmp_path, "Cargo.toml", '[workspace.dependencies]\ntoml = "0.8"\n')
    write_page(tmp_path, "Cargo.lock", '[[package]]\nname = "toml_edit"\n')
    write_page(tmp_path, RETIRED_REFERENCE, "# The old manifest\n")
    findings = retired_manifest_findings(tmp_path)
    assert [(finding.where, finding.word) for finding in findings] == [
        ("Cargo.toml:2", "toml"),
        ("Cargo.lock:2", "toml_edit"),
        (RETIRED_REFERENCE, "reference/manifest.md"),
    ]


def test_a_published_page_may_not_say_v1_or_gone(tmp_path):
    """Both word shapes the `MB.21.1.1` review found are refused."""
    write_zensical(tmp_path)
    write_page(
        tmp_path,
        "docs/reference/blastfile.md",
        "| v1's global `groups` key | gone: every attach is written on the view |\n",
    )
    findings = retired_format_word_findings(tmp_path)
    assert [(finding.where, finding.word) for finding in findings] == [
        ("docs/reference/blastfile.md:1", "v1"),
        ("docs/reference/blastfile.md:1", "gone:"),
    ]


def test_the_word_rule_does_not_ban_a_real_version_tag(tmp_path):
    """A Kubernetes `apps/v1`, an image tag and a pinned action are not the format."""
    write_zensical(tmp_path)
    write_page(
        tmp_path,
        "docs/deploy/anything-else.md",
        "apiVersion: apps/v1\n"
        "apiVersion: networking.k8s.io/v1\n"
        "image: edoburu/pgbouncer:v1.26.1-p0\n"
        "      - uses: astral-sh/setup-uv@v10.3.2\n",
    )
    assert retired_format_word_findings(tmp_path) == []


def test_the_retired_manifest_guard_scans_a_real_tree():
    """Anti-vacuity: every scanned population and the record both have a subject."""
    assert (ROOT / "README.md").is_file()
    assert (ROOT / "ARCHITECTURE.md").is_file()
    assert (ROOT / "AGENTS.md").is_file()
    pages = [label_of(ROOT, path) for path in published_page_files(ROOT)]
    assert pages, "no published pages found"
    assert not any(page.startswith("docs/decisions/") for page in pages)
    assert "docs/changelog.md" not in pages, (
        "the changelog is a published record; scanning it would force a rewrite of history"
    )
    assert RECORD_PAGES == ("docs/changelog.md",), (
        "RECORD_PAGES is the guard's only exemption; widening it must be a deliberate, "
        "reviewed edit that also updates this assertion and the docstring"
    )
    assert rust_sources(ROOT), "no Rust sources found"
    assert any((ROOT / "scripts").iterdir()), "no scripts found"
    briefs = sorted((ROOT / "plan" / "nodes").rglob("NODE.md"))
    assert 0 < len(live_plan_briefs(ROOT)) < len(briefs)
    finished = [page for page in briefs if status_of(text_of(page)) in RETIRED_STATUSES]
    assert any(RETIRED_MANIFEST.search(text_of(page)) for page in finished), (
        "no finished brief still names the retired manifest: the exemption is dead"
    )
    assert RETIRED_MANIFEST.search(
        text_of(ROOT / "docs" / "decisions" / "ADR-0012-blastfile-dsl.md")
    ), "no record still names the retired manifest: the exemption has no subject"
    assert RETIRED_MANIFEST_NAME not in text_of(Path(__file__)), (
        "this module must compose the name, not carry it"
    )


# --- The Blastfile highlighter (MB.32.1) ------------------------------------
#
# Pygments has no `Blastfile` lexer, so every `Blastfile` fence on the site is
# `language-text` and renders as one grey run. `MB.32.1` ships one offline
# script that wraps the tokens in the span classes Zensical's own Pygments CSS
# styles, and this half of the file is the record of what it must do: which
# fences it may touch, that it never rebuilds the markup, that its SQL interior
# tokens are the contract's, and that every colour it paints reaches WCAG AA on
# the code surface of its own scheme.
#
# The rules live in the asset, not here. The lexer is executed through Node in a
# sandbox (`globalThis.BlastfileLexer`), so these tests cannot drift into a
# second, divergent implementation: a hand edit to the lexer changes what the
# assertions see.

HIGHLIGHTER = ROOT / "docs" / "javascripts" / "blastfile-highlight.js"
EXTRA_CSS = ROOT / "docs" / "stylesheets" / "extra.css"
ZENSICAL = ROOT / "zensical.toml"
SITE_HIGHLIGHTER = ROOT / "site" / "javascripts" / "blastfile-highlight.js"
#: WCAG 2.2 SC 1.4.3 (text contrast) is 4.5:1; the code surface is the body it
#: sits on, `--md-code-bg-color`, which `brand.css` maps to `--blastit-surface`.
CODE_CONTRAST = 4.5
#: The one class set the lexer may emit, and the one `extra.css` must colour.
#: Every name is styled by Zensical's bundled Pygments CSS (`.highlight .X`).
TOKEN_CLASSES = (
    "k",
    "kd",
    "kc",
    "kt",
    "nf",
    "nv",
    "s1",
    "s2",
    "se",
    "c1",
    "p",
    "mi",
    "o",
)
#: One SQL keyword set, matching the reference page and the Zed grammar. The
#: asset test reads the array out of the asset and compares it to this tuple, so
#: a second list (or a dropped word) is a failing name, never a quiet fork.
SQL_KEYWORDS = (
    "ALL",
    "AND",
    "AS",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASE",
    "COMMIT",
    "CONFLICT",
    "COUNT",
    "DELETE",
    "DISTINCT",
    "DO",
    "ELSE",
    "END",
    "EXISTS",
    "FROM",
    "GROUP",
    "HAVING",
    "IN",
    "INNER",
    "INSERT",
    "INTO",
    "IS",
    "JOIN",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NOT",
    "NOTHING",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "RETURNING",
    "RIGHT",
    "ROLLBACK",
    "SELECT",
    "SET",
    "THEN",
    "UNION",
    "UPDATE",
    "VALUES",
    "WHEN",
    "WHERE",
    "WITH",
)

#: `page -> its `text` fences in reading order`, each `(first significant line,
#: is a Blastfile)`. The 34 blocks measured with
#: `python/tests/runnable_docs/fences.py`: 29 carry a Blastfile (12
#: `text title="Blastfile"` documents and 17 keyword-led statement lists) and
#: the five named below stay plain.
#:
#: The key is the fence's first non-blank, non-comment line, **never its line
#: number**: another node edits the prose above these fences (MB.34.8 already
#: moved seven of them), and a `(page, line)` pin is stale the moment it does.
#: A prose edit therefore changes nothing here; a fence that is added, removed,
#: reordered or reclassified still fails, naming the page and the line it sits
#: on now.
EXPECTED_FENCES: dict[str, tuple[tuple[str, bool], ...]] = {
    "README.md": (
        ('app "hello"', True),
        ("blastit check: 4 requests, 2 statements, 3/4 views, 2 proxies", False),
    ),
    "docs/blastfile.md": (('app "shop"', True),),
    "docs/ci.md": (("trait connection {", True),),
    "docs/deploy/digitalocean-paas.md": (("postgres {", True),),
    "docs/frameworks/php/index.md": (
        ('app "shop"', True),
        (
            "sudo apt install libphp-embed php-pgsql # Debian 13, Ubuntu 24.04+",
            False,
        ),
    ),
    "docs/frameworks/php/laravel.md": (('app "shop"', True),),
    "docs/frameworks/php/symfony.md": (('app "shop"', True),),
    "docs/frameworks/python/django.md": (('app "myshop"', True),),
    "docs/frameworks/python/flask.md": (('app "shop"', True),),
    "docs/frameworks/python/index.md": (('app "shop"', True),),
    "docs/frameworks/static-and-spa.md": (('static "/" {', True),),
    "docs/getting-started.md": (('app "hello"', True),),
    "docs/how-it-works.md": (
        ("/run/blastit/views/<uid>/ 0700, owned by the view uid", False),
    ),
    "docs/index.md": (('app "myshop"', True),),
    "docs/local.md": (
        ("15:41:02.119 INFO reload 2 directories", False),
        ('app "shop"', True),
        ("view products regex '/products/.*' {", True),
    ),
    "docs/reference/blastfile.md": (
        ("file \u2192 app? entrypoint? section*", False),
        ('static "/" {', True),
        ('app "shop"', True),
        ('entrypoint php "public/index.php"', True),
        ('app "shop"', True),
        ("postgres $DATABASE_URL # the default store, one libpq URL", True),
        ('view products GET "/products", "/products/{id:int}" {', True),
        ("view cart {", True),
        ('view catalog GET "/products/{id:int}" {', True),
        ("trait connection {", True),
        ("trait session {", True),
        ('view products GET "/products" {', True),
        ('static "/" {', True),
        ('app "shop"', True),
        ('view products GET "/products/{id:int}" {', True),
        (
            (
                "Blastfile:3:9: `=` is not Blastfile syntax; write the value after "
                "the keyword"
            ),
            False,
        ),
    ),
    "docs/reference/cli.md": (
        ("tar -xzf blastit-lsp-aarch64-apple-darwin.tar.gz", False),
    ),
}

HIGHLIGHT_DETAIL = (
    "the Blastfile highlighter is progressive enhancement: fix the classifier, "
    "not the page"
)
SQL_KIND = (
    "the SQL a `query` string carries: one statement, bound values, no inline literal"
)

#: The Node bridge. It runs the shipped asset in a sandbox and reports only what
#: the asset's own API returns. `dom` mode builds the shape Zensical renders
#: (`<code><span id="__span-0-N"><a id="__codelineno-0-N"></a>line\n</span>`),
#: so the text-node walk is exercised, not just the tokenizer.
NODE_DRIVER = r"""
const fs = require("fs");
const vm = require("vm");

const source = fs.readFileSync(process.argv[2], "utf8");
const payload = JSON.parse(fs.readFileSync(process.argv[3], "utf8"));

function textOf(node) {
  if (node.nodeType === 3) {
    return node.nodeValue;
  }
  return node.childNodes.map(textOf).join("");
}

function makeNode(tag) {
  const node = {
    nodeType: 1,
    childNodes: [],
    parentNode: null,
    className: "",
    id: "",
    attrs: {},
    appendChild(child) {
      child.parentNode = node;
      node.childNodes.push(child);
      return child;
    },
    insertBefore(child, before) {
      const at = node.childNodes.indexOf(before);
      child.parentNode = node;
      node.childNodes.splice(at < 0 ? node.childNodes.length : at, 0, child);
      return child;
    },
    removeChild(child) {
      const at = node.childNodes.indexOf(child);
      if (at >= 0) {
        node.childNodes.splice(at, 1);
        child.parentNode = null;
      }
      return child;
    },
    hasAttribute(name) {
      return Object.prototype.hasOwnProperty.call(node.attrs, name);
    },
    setAttribute(name, value) {
      node.attrs[name] = value;
    },
  };
  Object.defineProperty(node, "textContent", {
    get() {
      return textOf(node);
    },
  });
  Object.defineProperty(node, "innerHTML", {
    get() {
      return textOf(node);
    },
    set() {
      throw new Error("innerHTML rebuilds the line anchors");
    },
  });
  return node;
}

function makeText(value) {
  return { nodeType: 3, nodeValue: value, data: value, parentNode: null };
}

function walk(node, visit) {
  for (const child of node.childNodes) {
    if (child.nodeType === 1) {
      visit(child);
      walk(child, visit);
    }
  }
}

const context = {
  document: {
    querySelectorAll: () => [],
    addEventListener: () => {},
    createElement: (tag) => makeNode(tag),
    createTextNode: (value) => makeText(value),
  },
};
vm.createContext(context);
vm.runInContext(source, context);
const lexer = context.BlastfileLexer;
if (!lexer) {
  throw new Error("the asset did not publish globalThis.BlastfileLexer");
}

const answer = {};
if (payload.classify) {
  answer.classify = payload.classify.map((text) => lexer.classify(text));
}
if (payload.roundtrip) {
  answer.roundtrip = payload.roundtrip.map((text) => lexer.tokenize(text));
}
if (payload.dom) {
  const code = makeNode("code");
  payload.dom.forEach((line, index) => {
    const span = makeNode("span");
    span.id = "__span-0-" + (index + 1);
    const anchor = makeNode("a");
    anchor.id = "__codelineno-0-" + (index + 1);
    span.appendChild(anchor);
    span.appendChild(makeText(line));
    code.appendChild(span);
  });
  const idsBefore = [];
  walk(code, (node) => {
    if (node.id) {
      idsBefore.push(node.id);
    }
  });
  const before = textOf(code);
  lexer.highlightCode(code);
  const idsAfter = [];
  const classes = {};
  walk(code, (node) => {
    if (node.id) {
      idsAfter.push(node.id);
    }
    if (node.className) {
      classes[node.className] = (classes[node.className] || 0) + 1;
    }
  });
  answer.dom = {
    idsBefore: idsBefore,
    idsAfter: idsAfter,
    before: before,
    after: textOf(code),
    classes: classes,
    marked: code.hasAttribute(lexer.MARK),
  };
}
answer.tokenClasses = lexer.TOKEN_CLASSES;
process.stdout.write(JSON.stringify(answer));
"""


def run_lexer(tmp_path: Path, payload: dict) -> dict:
    """Run the shipped asset's own API in Node and return its JSON answer."""
    node = shutil.which("node")
    assert node, (
        f"node is not installed, so {label_of(ROOT, HIGHLIGHTER)} cannot be "
        "executed; install Node.js 20 or newer (the docs CI runner ships it)"
    )
    driver = tmp_path / "drive.js"
    driver.write_text(NODE_DRIVER, encoding="utf-8")
    inputs = tmp_path / "payload.json"
    inputs.write_text(json.dumps(payload), encoding="utf-8")
    done = subprocess.run(
        [node, str(driver), str(HIGHLIGHTER), str(inputs)],
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    assert done.returncode == 0, (
        f"{label_of(ROOT, HIGHLIGHTER)} did not run:\n{done.stderr}"
    )
    return json.loads(done.stdout)


def segments(answer: list[dict]) -> list[tuple[str, str | None]]:
    """The `(text, class)` pairs of one tokenized fence, in document order."""
    return [(item["text"], item["cls"]) for item in answer]


def classes_of(answer: list[dict]) -> list[tuple[str, str]]:
    """Only the coloured pairs; the plain runs carry no evidence."""
    return [(text, cls) for text, cls in segments(answer) if cls is not None]


def joined(answer: list[dict]) -> str:
    """The input the asset read back, rebuilt from its own segments."""
    return "".join(text for text, _ in segments(answer))


def css_block(css: str, selector: str) -> str:
    """The declarations of the rule whose selector starts at `selector`."""
    at = css.index(selector) + len(selector)
    return block_at(css, css.index("{", at))


def text_blocks() -> list[tuple[str, int, str]]:
    """Every `text` fence of the public pages: `(page, line, body)`."""
    blocks = []
    for page in fences.public_pages(ROOT, text_of(ZENSICAL)):
        text = text_of(ROOT / page)
        for block in fences.scan_page(page, text)[0]:
            if block.language == "text":
                blocks.append((page, block.line, block.body))
    return blocks


def fence_key(body: str) -> str:
    """A fence's first non-blank, non-comment line, whitespace-normalised."""
    for line in body.splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            return " ".join(stripped.split())
    return ""


def fence_rows() -> list[tuple[str, int, str, str]]:
    """Every `text` fence as `(page, line, key, body)`, in reading order."""
    return [(page, line, fence_key(body), body) for page, line, body in text_blocks()]


def blocks_for(highlight: bool) -> list[tuple[str, int, str]]:
    """The measured fences that `EXPECTED_FENCES` marks highlighted (or plain)."""
    wanted = [
        (page, key)
        for page, entries in EXPECTED_FENCES.items()
        for key, verdict in entries
        if verdict is highlight
    ]
    found = []
    for page, line, key, body in fence_rows():
        if (page, key) in wanted:
            wanted.remove((page, key))
            found.append((page, line, body))
    assert not wanted, (
        f"`EXPECTED_FENCES` names fences the pages no longer carry: {wanted}"
    )
    return found


#: The statement the README's `blastit check` block prints under the denied
#: grant. `audit_v1::human` prints the normalised statement **verbatim**, so a
#: placeholder such as `SELECT 2` claims an output the binary never produces.
#: The census keys a fence by its first line (the summary) and cannot see this
#: one, which is why it is pinned by its own assertion.
README_CHECK_SUMMARY = "blastit check: 4 requests, 2 statements, 3/4 views, 2 proxies"
README_CHECK_STATEMENT = (
    '    SELECT "shop_product"."price" FROM "shop_product" '
    'WHERE "shop_product"."id" = $?'
)


def test_the_readme_check_block_shows_a_real_statement():
    """The illustrative block prints a statement `check` would really print."""
    page = "README.md"
    blocks = [
        block.body
        for block in fences.scan_page(page, text_of(ROOT / page))[0]
        if block.language == "text"
    ]
    matched = [body for body in blocks if body.startswith(README_CHECK_SUMMARY)]
    assert len(matched) == 1, (
        f"{page} carries no `{README_CHECK_SUMMARY}` block (found {len(blocks)} "
        "`text` blocks): the check output moved, so this pin no longer has a subject"
    )
    assert README_CHECK_STATEMENT in matched[0], (
        f"{page}'s check block no longer shows the statement the binary prints:\n"
        f"expected {README_CHECK_STATEMENT!r} in\n{matched[0]}"
    )


def excluded_asset_paths() -> list[str]:
    """The `exclude_docs` patterns the site build applies to `docs/`."""
    project = tomllib.loads(text_of(ZENSICAL))["project"]
    return fences._exclude_patterns(str(project.get("exclude_docs", "")))


def test_the_highlighter_is_one_offline_script():
    """No network, no module system, no dynamic code: one classic script."""
    source = text_of(HIGHLIGHTER)
    forbidden = {
        "a URL": re.compile(r"https?://"),
        "fetch": re.compile(r"\bfetch\s*\("),
        "a module import": re.compile(r"(?m)^\s*import\b"),
        "a module require": re.compile(r"\brequire\s*\("),
        "a dynamic import": re.compile(r"\bimport\s*\("),
        "eval": re.compile(r"\beval\s*\("),
        "new Function": re.compile(r"\bnew\s+Function\b"),
        "XMLHttpRequest": re.compile(r"\bXMLHttpRequest\b"),
        "WebSocket": re.compile(r"\bWebSocket\b"),
        "sendBeacon": re.compile(r"\bnavigator\s*\.\s*sendBeacon\s*\("),
    }
    found = [where for where, pattern in forbidden.items() if pattern.search(source)]
    assert not found, (
        f"{label_of(ROOT, HIGHLIGHTER)} uses {', '.join(found)}: the site is "
        "built and served offline, so the asset must be self-contained"
    )
    assert len(source.splitlines()) > 40, "the highlighter is a stub, not a lexer"


def test_the_highlighter_runs_on_first_load_and_instant_navigation():
    """`navigation.instant` swaps the document, so both entry paths are needed."""
    source = text_of(HIGHLIGHTER)
    subscribes = re.search(r"document\$\.subscribe\(\s*highlight\s*\)", source)
    assert subscribes, (
        "no `document$.subscribe(highlight)` call: an instant-navigation page "
        "swap would render every Blastfile plain"
    )
    first_load = re.search(
        r"addEventListener\(\s*\"DOMContentLoaded\"\s*,\s*highlight\s*\)", source
    )
    assert first_load, (
        "no `DOMContentLoaded` listener for `highlight`: the first page load "
        "would render plain"
    )
    assert "data-blastfile-highlighted" in source, (
        "no idempotence marker: instant navigation runs the lexer twice and the "
        "second run would wrap the spans it already made"
    )
    assert "setAttribute(MARK" in source, (
        "nothing sets the idempotence marker, so a second run re-wraps the spans"
    )
    assert "hasAttribute(MARK)" in source, (
        "the marker is never read, so it does not make the lexer idempotent"
    )


def test_the_highlighter_never_rebuilds_the_markup():
    """`code.innerHTML = …` destroys `<span id="__span-…">` and the copy source."""
    source = text_of(HIGHLIGHTER)
    rebuilds = {
        "an innerHTML assignment": re.compile(r"\.\s*innerHTML\s*="),
        "an outerHTML assignment": re.compile(r"\.\s*outerHTML\s*="),
        "insertAdjacentHTML": re.compile(r"\binsertAdjacentHTML\s*\("),
        "document.write": re.compile(r"\bdocument\s*\.\s*write\s*\("),
    }
    found = [where for where, pattern in rebuilds.items() if pattern.search(source)]
    assert not found, (
        f"{label_of(ROOT, HIGHLIGHTER)} uses {', '.join(found)}: rebuilding the "
        "markup drops the line anchors and the copy button's text"
    )
    assert "createTextNode" in source and "insertBefore" in source, (
        "the highlighter does not split Text nodes, so it cannot preserve the "
        "rendered markup it was given"
    )


def test_the_highlighter_is_wired_and_published():
    """`extra_javascript` is what loads it, and nothing excludes it from `site/`."""
    project = tomllib.loads(text_of(ZENSICAL))["project"]
    assert project.get("extra_javascript") == ["javascripts/blastfile-highlight.js"], (
        "zensical.toml must name the asset in `extra_javascript`, or no page "
        f"ever loads it (found {project.get('extra_javascript')!r})"
    )
    assert HIGHLIGHTER.is_file(), f"{label_of(ROOT, HIGHLIGHTER)} is missing"
    relative = HIGHLIGHTER.relative_to(ROOT / "docs").as_posix()
    assert not fences._excluded(relative, excluded_asset_paths()), (
        f"an `exclude_docs` rule drops {relative} from the build, so the site "
        "would link a script that was never copied"
    )
    if SITE_HIGHLIGHTER.is_file():
        assert SITE_HIGHLIGHTER.read_bytes() == HIGHLIGHTER.read_bytes(), (
            "site/javascripts/blastfile-highlight.js is stale: rebuild the site "
            "(`make docs-check`)"
        )


def test_every_text_block_classifies_as_measured(tmp_path):
    """The 29 + 7 split, matched by first line so a prose edit cannot break it."""
    rows = fence_rows()
    verdicts = run_lexer(tmp_path, {"classify": [body for *_, body in rows]})[
        "classify"
    ]
    measured: dict[str, list[tuple[str, bool]]] = {}
    where: dict[tuple[str, int], int] = {}
    for row, verdict in zip(rows, verdicts, strict=True):
        page, line, key, _body = row
        measured.setdefault(page, []).append((key, verdict))
        where[(page, len(measured[page]) - 1)] = line
    problems = []
    for page in sorted(set(measured) | set(EXPECTED_FENCES)):
        got = measured.get(page, [])
        expected = list(EXPECTED_FENCES.get(page, ()))
        for index in range(max(len(got), len(expected))):
            actual = got[index] if index < len(got) else None
            pinned_entry = expected[index] if index < len(expected) else None
            if actual != pinned_entry:
                problems.append(
                    f"{page}:{where.get((page, index), '?')}: expected "
                    f"{pinned_entry}, classified {actual}"
                )
    pinned = sum(len(entries) for entries in EXPECTED_FENCES.values())
    unmatched = [
        (page, key)
        for page, entries in EXPECTED_FENCES.items()
        for key, _verdict in entries
    ]
    surplus = []
    for page, line, key, _body in rows:
        if (page, key) in unmatched:
            unmatched.remove((page, key))
        else:
            surplus.append(f"{page}:{line} {key!r}")
    assert len(rows) == pinned, (
        f"the public pages carry {len(rows)} `text` blocks, not {pinned}: a fence "
        f"was added or removed, and this task ships none. Surplus "
        f"{surplus or 'none'}, missing {unmatched or 'none'}"
    )
    assert not problems, "\n".join(problems) + f"\n{HIGHLIGHT_DETAIL}"
    highlighted = sum(
        1 for entries in measured.values() for _, verdict in entries if verdict
    )
    assert (highlighted, len(rows) - highlighted) == (29, 7), (
        f"{highlighted} fences highlighted and {len(rows) - highlighted} plain, "
        "not 29 + 7"
    )


def test_every_highlighted_fence_really_colours_something(tmp_path):
    """Anti-vacuity: the classifier says yes, so the lexer must paint a token."""
    blocks = blocks_for(highlight=True)
    assert len(blocks) == 29, f"{len(blocks)} highlighted fences, not 29"
    answers = run_lexer(tmp_path, {"roundtrip": [body for _, _, body in blocks]})[
        "roundtrip"
    ]
    for (page, line, body), answer in zip(blocks, answers, strict=True):
        assert joined(answer) == body, f"{page}:{line}: the lexer lost or added text"
        coloured = classes_of(answer)
        assert any(cls == "k" for _, cls in coloured), (
            f"{page}:{line} is classified as a Blastfile but no keyword was "
            f"coloured: {coloured}"
        )


def test_every_token_colour_meets_aa_in_both_schemes():
    """Each token, in each scheme, on the code surface it actually renders on."""
    css = text_of(EXTRA_CSS)
    scheme_only = re.findall(
        r'\[data-md-color-scheme="[a-z]+"\]\s+\.highlight\s+\.', css
    )
    assert not scheme_only, (
        f"{len(scheme_only)} palette rule(s) are scheme-scoped only, so they "
        "would recolour every `console`, `python` and `json` fence on the site: "
        "each rule needs `code[data-blastfile-highlighted]` between `.highlight` "
        "and the token class"
    )
    rule = re.compile(
        r'\[data-md-color-scheme="(?P<scheme>default|slate)"\]\s+'
        r"\.highlight\s+code\[data-blastfile-highlighted\]\s+"
        r"\.(?P<token>[a-z0-9]+)\s*\{[^}]*?"
        r"color:\s*(?P<colour>#[0-9a-fA-F]{6})"
    )
    found = [
        (match["scheme"], match["token"], match["colour"])
        for match in rule.finditer(css)
    ]
    palette: dict[str, dict[str, str]] = {"default": {}, "slate": {}}
    for scheme, token, colour in found:
        assert token not in palette[scheme], (
            f".{token} is coloured twice in {scheme}: the later rule wins, so "
            "the tested value is not the rendered one"
        )
        palette[scheme][token] = colour
    for scheme in ("default", "slate"):
        assert set(palette[scheme]) == set(TOKEN_CLASSES), (
            f"{scheme} colours {sorted(palette[scheme])}, not "
            f"{sorted(TOKEN_CLASSES)}: every class the lexer emits needs a value"
        )

    brand = text_of(BRAND / "brand.css")
    assert "--md-code-bg-color: var(--blastit-surface)" in brand, (
        "the code surface is no longer `--blastit-surface`: this test measures "
        "the wrong colour"
    )
    surface_rule = re.compile(r"--blastit-surface:\s*(#[0-9a-fA-F]{6})")
    surface = {
        scheme: surface_rule.search(css_block(brand, selector)).group(1)
        for scheme, selector in (
            ("default", ':root,\n[data-md-color-scheme="default"]'),
            ("slate", '[data-md-color-scheme="slate"]'),
        )
    }
    assert surface == {"default": "#fffcf5", "slate": "#1b232c"}, (
        f"the code surfaces moved to {surface}: re-measure every token"
    )
    short = [
        f"{scheme} .{token} {colour} is {contrast(colour, surface[scheme]):.2f}:1 "
        f"on {surface[scheme]}"
        for scheme in ("default", "slate")
        for token in TOKEN_CLASSES
        if (colour := palette[scheme][token])
        and contrast(colour, surface[scheme]) < CODE_CONTRAST
    ]
    assert not short, (
        f"WCAG AA needs {CODE_CONTRAST}:1 on the code surface; below that are: "
        + "; ".join(short)
    )


def test_the_sql_keyword_set_is_one_set():
    """The keyword list is the contract's; a second list is a fork."""
    source = text_of(HIGHLIGHTER)
    declared = re.findall(r"SQL_KEYWORDS\s*=\s*lookup\(\[(.*?)\]\)", source, re.DOTALL)
    assert len(declared) == 1, (
        f"the SQL keyword set is declared {len(declared)} times: one set, shared "
        "by the lexer and the reference page"
    )
    words = re.findall(r'"([A-Z_]+)"', declared[0])
    repeated = sorted({word for word in words if words.count(word) > 1})
    assert not repeated, (
        f"the SQL keyword set repeats {repeated}: `lookup` would silently "
        "collapse the duplicates"
    )
    assert tuple(sorted(words)) == SQL_KEYWORDS, (
        "the SQL keyword set changed: "
        f"missing {sorted(set(SQL_KEYWORDS) - set(words))}, "
        f"extra {sorted(set(words) - set(SQL_KEYWORDS))}"
    )


#: `(fence, expected coloured segments)` for the SQL interior. The expected
#: pairs are the whole coloured sequence: a token that loses its class or gains
#: the wrong one changes the list, and the wrapper proves the closing quote is
#: still the string's own token.
SQL_FIXTURES = [
    (
        "uppercase keywords, an IN list and the variadic marker",
        (
            "view v {\n    postgres {\n        query 'SELECT id FROM products "
            "WHERE id IN ($?, ...)'\n    }\n}\n"
        ),
        [
            ("view", "k"),
            ("v", "nf"),
            ("{", "p"),
            ("postgres", "k"),
            ("{", "p"),
            ("query", "k"),
            ("'", "s1"),
            ("SELECT", "k"),
            ("FROM", "k"),
            ("WHERE", "k"),
            ("IN", "k"),
            ("$?", "se"),
            ("...", "nv"),
            ("'", "s1"),
            ("}", "p"),
            ("}", "p"),
        ],
    ),
    (
        "lowercase keywords, `$1` and `$12`",
        "query 'select * from products where price > $1 and id = $12'\n",
        [
            ("query", "k"),
            ("'", "s1"),
            ("select", "k"),
            ("*", "o"),
            ("from", "k"),
            ("where", "k"),
            (">", "o"),
            ("$1", "nv"),
            ("and", "k"),
            ("=", "o"),
            ("$12", "nv"),
            ("'", "s1"),
        ],
    ),
    (
        "UPDATE … SET with bound values",
        "query 'UPDATE t SET a = $1 WHERE b = $12'\n",
        [
            ("query", "k"),
            ("'", "s1"),
            ("UPDATE", "k"),
            ("SET", "k"),
            ("=", "o"),
            ("$1", "nv"),
            ("WHERE", "k"),
            ("=", "o"),
            ("$12", "nv"),
            ("'", "s1"),
        ],
    ),
    (
        "INSERT … VALUES rows",
        "query 'INSERT INTO t (a, b) VALUES ($?, $?), ...'\n",
        [
            ("query", "k"),
            ("'", "s1"),
            ("INSERT", "k"),
            ("INTO", "k"),
            ("VALUES", "k"),
            ("$?", "se"),
            ("$?", "se"),
            ("...", "nv"),
            ("'", "s1"),
        ],
    ),
    (
        "operator soup, a function and a `--` comment",
        "query 'SELECT count(*), x FROM t -- note'\n",
        [
            ("query", "k"),
            ("'", "s1"),
            ("SELECT", "k"),
            ("count", "k"),
            ("*", "o"),
            ("FROM", "k"),
            ("-- note", "c1"),
            ("'", "s1"),
        ],
    ),
    (
        "quoted identifiers, IS NOT NULL, `<>`, `||`, `::` and a number",
        (
            'query \'select "Order" from "orders" where total >= 10.5 '
            "and a IS NOT NULL and b <> 1 || c::text'\n"
        ),
        [
            ("query", "k"),
            ("'", "s1"),
            ("select", "k"),
            ("from", "k"),
            ("where", "k"),
            (">=", "o"),
            ("10.5", "mi"),
            ("and", "k"),
            ("IS", "k"),
            ("NOT", "k"),
            ("NULL", "k"),
            ("and", "k"),
            ("<>", "o"),
            ("1", "mi"),
            ("||", "o"),
            ("::", "o"),
            ("'", "s1"),
        ],
    ),
]


def test_the_sql_inside_a_query_string_is_tokenized(tmp_path):
    """Keywords, `$?`, `$n`, `...`, numbers, operators and comments, in order."""
    answers = run_lexer(
        tmp_path, {"roundtrip": [fixture[1] for fixture in SQL_FIXTURES]}
    )["roundtrip"]
    for (label, fence, expected), answer in zip(SQL_FIXTURES, answers, strict=True):
        assert joined(answer) == fence, f"{label}: the lexer lost or added text"
        assert classes_of(answer) == expected, f"{label}: {SQL_KIND}"


def test_a_query_comment_never_swallows_the_closing_quote(tmp_path):
    """`--` runs to the end of the line, and the string still ends on its quote."""
    fence = "query 'SELECT id -- note'\n}\n"
    answer = run_lexer(tmp_path, {"roundtrip": [fence]})["roundtrip"][0]
    assert classes_of(answer) == [
        ("query", "k"),
        ("'", "s1"),
        ("SELECT", "k"),
        ("-- note", "c1"),
        ("'", "s1"),
        ("}", "p"),
    ], SQL_KIND
    assert joined(answer) == fence


def test_an_escaped_query_finds_its_tokens_and_ends_on_its_quote(tmp_path):
    """`\\"` is an escape, not the end of the string; the closing `"` still ends it."""
    fence = 'query "SELECT \\"name\\" FROM t WHERE a = $?"\n'
    answer = run_lexer(tmp_path, {"roundtrip": [fence]})["roundtrip"][0]
    assert classes_of(answer) == [
        ("query", "k"),
        ('"', "s2"),
        ("SELECT", "k"),
        ('\\"', "se"),
        ('\\"', "se"),
        ("FROM", "k"),
        ("WHERE", "k"),
        ("=", "o"),
        ("$?", "se"),
        ('"', "s2"),
    ], SQL_KIND
    assert joined(answer) == fence


def test_nothing_outside_a_query_string_is_sql(tmp_path):
    """A view named `select`, and a `query` inside a comment, colour no SQL."""
    fixtures = [
        (
            "a view named `select`",
            "view select {\n    postgres { select carts(id) }\n}\n",
            [
                ("view", "k"),
                ("select", "nf"),
                ("{", "p"),
                ("postgres", "k"),
                ("{", "p"),
                ("select", "k"),
                ("(", "p"),
                (")", "p"),
                ("}", "p"),
                ("}", "p"),
            ],
        ),
        (
            "a `query` inside a comment",
            "# query 'SELECT 1'\nview v { }\n",
            [
                ("# query 'SELECT 1'", "c1"),
                ("view", "k"),
                ("v", "nf"),
                ("{", "p"),
                ("}", "p"),
            ],
        ),
    ]
    answers = run_lexer(tmp_path, {"roundtrip": [fence for _, fence, _ in fixtures]})[
        "roundtrip"
    ]
    for (label, fence, expected), answer in zip(fixtures, answers, strict=True):
        assert joined(answer) == fence, f"{label}: the lexer lost or added text"
        assert classes_of(answer) == expected, f"{label}: only a `query` string is SQL"


def test_the_plain_fences_stay_plain_with_a_query_line_appended(tmp_path):
    """The refusal is the block's, not the line's: a `query` line cannot rescue it."""
    bodies = blocks_for(highlight=False)
    assert len(bodies) == 7, f"{len(bodies)} plain fences, not 7"
    appended = [body + "\nquery 'SELECT 1'\n" for _, _, body in bodies]
    verdicts = run_lexer(tmp_path, {"classify": appended})["classify"]
    wrong = [
        f"{page}:{line}"
        for (page, line, _body), verdict in zip(bodies, verdicts, strict=True)
        if verdict
    ]
    assert not wrong, (
        "a plain `text` block that gains a `query` line is still not a Blastfile: "
        f"{wrong}\n{HIGHLIGHT_DETAIL}"
    )


def test_the_dom_walk_keeps_every_anchor_and_every_byte(tmp_path):
    """The line ids, the `__span` wrappers and `code.textContent` all survive."""
    lines = [
        'app "shop"\n',
        "\n",
        'view products GET "/products/{id:int}" {\n',
        "    postgres { select products(id, name, price) }\n",
        "}\n",
    ]
    answer = run_lexer(tmp_path, {"dom": lines})["dom"]
    assert answer["marked"] is True, (
        "the processed `<code>` carries no idempotence marker, so instant "
        "navigation would tokenize it twice"
    )
    assert answer["idsAfter"] == answer["idsBefore"], (
        f"the line anchors changed: {answer['idsBefore']} became {answer['idsAfter']}"
    )
    assert answer["after"] == answer["before"], (
        "`code.textContent` changed, so the copy button would read different "
        f"text: {answer['before']!r} became {answer['after']!r}"
    )
    assert set(answer["classes"]) <= set(TOKEN_CLASSES), (
        f"the walk emitted classes the theme does not style: "
        f"{sorted(set(answer['classes']) - set(TOKEN_CLASSES))}"
    )
    assert answer["classes"].get("k"), "no keyword span was created at all"


def test_the_emitted_classes_are_the_styled_and_coloured_set(tmp_path):
    """One class list: what the lexer emits is exactly what `extra.css` colours."""
    answer = run_lexer(tmp_path, {"roundtrip": [SQL_FIXTURES[0][1]]})
    assert tuple(answer["tokenClasses"]) == TOKEN_CLASSES, (
        f"the lexer emits {answer['tokenClasses']}, not {list(TOKEN_CLASSES)}: "
        "a class outside this list renders as plain text"
    )


def test_a_raw_regex_string_keeps_its_backslash(tmp_path):
    r"""`'…'` has no escapes, so `'\.php$'` is one `.s1` run, not a `.se` escape."""
    fence = 'static "/" {\n    root "public"\n    immutable regex \'\\.php$\'\n}\n'
    answer = run_lexer(tmp_path, {"roundtrip": [fence]})["roundtrip"][0]
    assert joined(answer) == fence, "the lexer lost or added text"
    assert classes_of(answer) == [
        ("static", "k"),
        ('"', "s2"),
        ("/", "s2"),
        ('"', "s2"),
        ("{", "p"),
        ("root", "kd"),
        ('"', "s2"),
        ("public", "s2"),
        ('"', "s2"),
        ("immutable", "kd"),
        ("'", "s1"),
        ("\\.php$", "s1"),
        ("'", "s1"),
        ("}", "p"),
    ], "a raw string has no escapes: the whole body is .s1"


def test_a_prose_edit_above_the_fences_keeps_every_identity(tmp_path):
    """The table keys on the first line, so prose above a fence shifts nothing.

    `MB.34.8` edits `docs/reference/blastfile.md` while this node runs and has
    already moved seven of its fences. Re-parsing the real page with a
    paragraph prepended — in memory only, the page is never written — must
    yield the same identity sequence and the same verdicts.
    """
    page = "docs/reference/blastfile.md"
    shifted = "## A new section\n\nOne more paragraph above the fences.\n\n" + text_of(
        ROOT / page
    )
    blocks = [
        block
        for block in fences.scan_page(page, shifted)[0]
        if block.language == "text"
    ]
    assert [block.line for block in blocks] != [
        block.line
        for block in fences.scan_page(page, text_of(ROOT / page))[0]
        if block.language == "text"
    ], "the prepended prose did not move the fences: this test proves nothing"
    keys = tuple(fence_key(block.body) for block in blocks)
    assert keys == tuple(key for key, _ in EXPECTED_FENCES[page]), (
        "a prose edit changed a fence identity: the key must be the fence's own "
        "first line, and nothing positional"
    )
    verdicts = run_lexer(tmp_path, {"classify": [block.body for block in blocks]})[
        "classify"
    ]
    assert tuple(verdicts) == tuple(verdict for _, verdict in EXPECTED_FENCES[page]), (
        "a prose edit changed a fence verdict"
    )
