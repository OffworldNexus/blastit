# /// script
# requires-python = ">=3.12"
# dependencies = ["numpy", "pillow", "resvg-py", "scipy"]
# ///
"""Find seams and holes in the brand SVGs.

Every colour boundary of the mascot must be one shape lying over another, so
that no anti-aliased edge relies on two paths meeting exactly. Where two paths
merely touch, the renderer leaves pixels that are less than fully opaque, and
the background shows through.

For each target this renders the bird with resvg, on a transparent background,
at 4x (and 8x for the head marks), then:

1. floods the transparent region (alpha < 128) from the image border;
2. adds the deliberate gaps listed for the target (between the legs, the
   cutouts' holes), found by flooding from a point inside each;
3. widens that region by one pixel, to take in the outline's own anti-aliasing
   (and forgives clusters of at most 3 px within two pixels of it: the apex of
   a notch in the outline, reported as "outline corners");
4. reports every remaining pixel with alpha < 250 as a seam or a hole, grouped
   into clusters with their position in SVG units.

It also writes, to ``--out`` (default ``/tmp/opencode/blastit-seams``), every
target on magenta and on the favicon's rust, and zoomed crops of each junction
(the bill base at 8x).

Run from anywhere::

    uv run docs/brand/check_seams.py            # all targets, exit 1 on any seam
    uv run docs/brand/check_seams.py --no-crops # just the counts
"""

from __future__ import annotations

import argparse
import io
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
import resvg_py
from PIL import Image
from scipy import ndimage

BRAND = Path(__file__).resolve().parent
GITHUB_BRAND = BRAND.parent.parent / ".github" / "brand"
MAGENTA = (255, 0, 255, 255)
RUST = (0x9B, 0x47, 0x23, 255)

# Junctions to inspect, in the mascot's 1024-unit coordinates: (name, x, y).
JUNCTIONS = [
    ("bill-base", 770, 232),
    ("mandibles", 845, 212),
    ("eye", 676, 250),
    ("stripe-nape", 530, 330),
    ("stripe-shoulder", 545, 378),
    ("cheek", 700, 300),
    ("throat", 760, 300),
    ("shoulder", 640, 390),
    ("wing-notch", 452, 525),
    ("wing-belly", 600, 495),
    ("flank", 470, 600),
    ("undertail", 400, 640),
    ("tail-spots", 210, 585),
    ("wingtip-tail", 160, 650),
    ("leg-tops", 540, 690),
    ("back-foot", 540, 785),
    ("front-foot", 660, 705),
]
DELIBERATE_LEG_GAP = [(560, 730)]


@dataclass
class Target:
    """One SVG to check: a file, or the bird inlined in a file."""

    name: str
    path: Path
    nested: bool = False  # check the first nested <svg> (the inlined bird)
    scales: tuple[int, ...] = (4,)
    gaps: list[tuple[float, float]] = field(default_factory=list)
    head: bool = False  # only the head junctions apply


TARGETS = [
    Target("mascot", BRAND / "mascot.svg", gaps=DELIBERATE_LEG_GAP),
    Target("mascot-head", BRAND / "mascot-head.svg", scales=(4, 8), head=True),
    Target("mascot-head-ink", BRAND / "mascot-head-ink.svg", scales=(4, 8), head=True,
           gaps=[(650, 310), (676, 250)]),
    Target("mascot-head-paper", BRAND / "mascot-head-paper.svg", scales=(4, 8), head=True,
           gaps=[(600, 262)]),
    Target("logo", BRAND / "logo.svg", scales=(4, 8), head=True, gaps=[(600, 262)]),
    Target("favicon", BRAND / "favicon.svg", scales=(4, 8)),
    Target("favicon-bird", BRAND / "favicon.svg", nested=True, scales=(4, 8), head=True),
    Target("header-light-bird", GITHUB_BRAND / "header-light.svg", nested=True,
           gaps=DELIBERATE_LEG_GAP),
    Target("header-dark-bird", GITHUB_BRAND / "header-dark.svg", nested=True,
           gaps=DELIBERATE_LEG_GAP),
    Target("social-card-bird", BRAND / "social-card.svg", nested=True, gaps=DELIBERATE_LEG_GAP),
]

HEAD_JUNCTIONS = {"bill-base", "mandibles", "eye", "stripe-nape", "stripe-shoulder", "cheek",
                  "throat"}


def standalone(target: Target) -> tuple[str, tuple[float, float, float, float]]:
    """Return the SVG markup to render and its viewBox (x, y, w, h)."""
    text = target.path.read_text()
    if target.nested:
        m = re.search(r"<svg(?![^>]*xmlns)[^>]*viewBox=\"([^\"]+)\"[^>]*>(.*)</svg>\s*</svg>",
                      text, re.S)
        if not m:
            raise SystemExit(f"{target.path}: no nested <svg> found")
        vb, body = m.group(1), m.group(2)
        text = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}">{body}</svg>'
    else:
        vb = re.search(r"viewBox=\"([^\"]+)\"", text).group(1)
    x, y, w, h = (float(v) for v in vb.split())
    return text, (x, y, w, h)


def with_viewbox(svg: str, vb: tuple[float, float, float, float]) -> str:
    """Crop: replace the root viewBox (and drop width/height) of a standalone SVG."""
    head, rest = svg.split(">", 1)
    head = re.sub(r'\s(width|height)="[^"]*"', "", head)
    head = re.sub(r'viewBox="[^"]*"', 'viewBox="{} {} {} {}"'.format(*vb), head)
    return head + ">" + rest


def render(svg: str, width: int, height: int) -> Image.Image:
    png = resvg_py.svg_to_bytes(svg_string=svg, width=width, height=height)
    return Image.open(io.BytesIO(bytes(png))).convert("RGBA")


def seams(img: Image.Image, vb, scale: float, gaps, corners: list[int]) -> tuple[np.ndarray, int]:
    """Return (seam mask, number of deliberate gaps found); count outline corners in ``corners[0]``."""
    # Pad with clear pixels, so that an edge running along the canvas border
    # still has "outside" next to it.
    alpha = np.pad(np.asarray(img)[:, :, 3], 2)
    clear = alpha < 128
    labels, _ = ndimage.label(clear)
    border = np.unique(np.concatenate([labels[0], labels[-1], labels[:, 0], labels[:, -1]]))
    keep = set(border[border > 0].tolist())
    found = 0
    for gx, gy in gaps:
        px, py = int((gx - vb[0]) * scale) + 2, int((gy - vb[1]) * scale) + 2
        if 0 <= py < alpha.shape[0] and 0 <= px < alpha.shape[1] and labels[py, px] > 0:
            keep.add(int(labels[py, px]))
            found += 1
    outside = np.isin(labels, list(keep))
    near = ndimage.binary_dilation(outside, structure=np.ones((3, 3)), iterations=1)
    mask = (alpha < 250) & ~near
    # A concave corner of the outline (a notch narrowing to a point, like the
    # one between the tail and the wing tip) leaves an anti-aliased pixel or
    # two at its apex that is one pixel too far from the clear notch. Those are
    # the outline, not seams: tiny clusters within two pixels of the outside.
    nearer = ndimage.binary_dilation(outside, structure=np.ones((3, 3)), iterations=2)
    lab, _ = ndimage.label(mask, structure=np.ones((3, 3)))
    for i, sl in enumerate(ndimage.find_objects(lab), start=1):
        part = lab[sl] == i
        if part.sum() <= 3 and nearer[sl][part].all():
            mask[sl][part] = False
            corners[0] += 1
    return mask[2:-2, 2:-2], found


def clusters(mask: np.ndarray, vb, scale: float) -> list[tuple[int, float, float]]:
    labels, n = ndimage.label(mask, structure=np.ones((3, 3)))
    out = []
    for i, sl in enumerate(ndimage.find_objects(labels), start=1):
        count = int((labels[sl] == i).sum())
        cy = (sl[0].start + sl[0].stop) / 2 / scale + vb[1]
        cx = (sl[1].start + sl[1].stop) / 2 / scale + vb[0]
        out.append((count, cx, cy))
    return sorted(out, reverse=True)


def on(img: Image.Image, colour) -> Image.Image:
    base = Image.new("RGBA", img.size, colour)
    base.alpha_composite(img)
    return base


def crops(name, svg, vb, junctions, out: Path, scale: int, half: float = 40) -> None:
    """Junction crops on magenta and rust, as one sheet per background."""
    tiles = {"magenta": [], "rust": []}
    size = int(2 * half * scale)
    for jname, jx, jy in junctions:
        box = (jx - half, jy - half, 2 * half, 2 * half)
        if box[0] + box[2] < vb[0] or box[0] > vb[0] + vb[2] or box[1] + box[3] < vb[1] \
                or box[1] > vb[1] + vb[3]:
            continue
        img = render(with_viewbox(svg, box), size, size)
        for bg, colour in (("magenta", MAGENTA), ("rust", RUST)):
            tile = on(img, colour)
            if jname == "bill-base":
                tile.convert("RGB").save(out / f"{name}-bill-base-{scale}x-{bg}.png")
                if scale < 8:  # the bill base, always also at 8x
                    big = render(with_viewbox(svg, box), int(2 * half * 8), int(2 * half * 8))
                    on(big, colour).convert("RGB").save(out / f"{name}-bill-base-8x-{bg}.png")
            tiles[bg].append(tile)
    for bg, row in tiles.items():
        if not row:
            continue
        cols = 6
        sheet = Image.new("RGBA", (cols * (size + 8), ((len(row) + cols - 1) // cols) * (size + 8)),
                          (60, 60, 60, 255))
        for i, t in enumerate(row):
            sheet.paste(t, ((i % cols) * (size + 8), (i // cols) * (size + 8)))
        sheet.convert("RGB").save(out / f"{name}-junctions-{scale}x-{bg}.png")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--out", type=Path, default=Path("/tmp/opencode/blastit-seams"))
    parser.add_argument("--no-crops", action="store_true")
    parser.add_argument("only", nargs="*", help="target names (default: all)")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    total = 0
    for t in TARGETS:
        if args.only and t.name not in args.only:
            continue
        svg, vb = standalone(t)
        for scale in t.scales:
            w, h = round(vb[2] * scale), round(vb[3] * scale)
            img = render(svg, w, h)
            corners = [0]
            mask, found = seams(img, vb, scale, t.gaps, corners)
            found_note = f", {found}/{len(t.gaps)} deliberate gaps" if t.gaps else ""
            if corners[0]:
                found_note += f", {corners[0]} outline corners"
            cl = clusters(mask, vb, scale)
            total += int(mask.sum())
            print(f"{t.name:20} {scale}x  {int(mask.sum()):6} seam px in {len(cl):3} clusters"
                  f"{found_note}")
            for count, cx, cy in cl[:12]:
                print(f"    {count:5} px at ({cx:6.1f}, {cy:6.1f})")
            if len(cl) > 12:
                print(f"    … {len(cl) - 12} more")
            if not args.no_crops:
                stem = f"{t.name}-{scale}x"
                on(img, MAGENTA).convert("RGB").save(args.out / f"{stem}-magenta.png")
                on(img, RUST).convert("RGB").save(args.out / f"{stem}-rust.png")
                vis = on(img, MAGENTA)
                px = np.asarray(vis).copy()
                px[mask] = (0, 255, 0, 255)
                Image.fromarray(px).convert("RGB").save(args.out / f"{stem}-seams.png")
                if t.name in {"mascot", "mascot-head", "favicon-bird"}:
                    js = [j for j in JUNCTIONS if not t.head or j[0] in HEAD_JUNCTIONS]
                    crops(t.name, svg, vb, js, args.out, 8 if scale >= 8 else scale)
    print(f"total: {total} seam px")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
