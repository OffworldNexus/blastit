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

import re
import sys
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
