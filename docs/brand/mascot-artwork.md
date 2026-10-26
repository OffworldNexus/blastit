# Nuthatch mascot artwork

blastit's mascot is a **Eurasian nuthatch** (*Sitta europaea*), drawn in the
house style of the Offworld Nexus birds: flat, geometric, Saul Bass airline
poster, a handful of silkscreen colours.

## Why a nuthatch

A nuthatch nests in an old tree hole and plasters its entrance with mud until
the opening is just wide enough for itself: it shrinks its own attack surface
and walls off its compartment, which is exactly what blastit does to every
route. It also scatter-hoards, wedging each seed into its own crack in the
bark instead of keeping one big store, so losing one cache costs one seed —
a small blast radius. And it is quick and agile, the only bird that runs
head-first *down* a trunk. As a silhouette it is unmistakable: a big
neckless head, a long chisel bill, and a black bandit stripe through the eye
over a white cheek — a mask that suits a bird that guards.

Tagline: **Small bird. Small blast radius.**

Other Offworld Nexus mascots, avoided: spectacled weaver (Tunnel Weaver),
European bee-eater (Screensight). No other project has one.

## Reference

Generated with **Nano Banana Pro** (`gemini-3-pro-image-preview`, Gemini API
`generateContent`, `responseModalities: ["IMAGE", "TEXT"]`, aspect ratio
1:1), resized to 1024 × 1024. Three prompt rounds, eight images:

1. A short brief (one image): came back a kingfisher-like bird — long tail,
   very long bill, no white cheek.
2. Anatomy spelled out (three images): unmistakably nuthatches, but in a soft
   modern flat style rather than poster geometry.
3. Round 2 plus a style paragraph (four images; the prompt is below). Candidate *b* was kept
   as [`mascot-reference.png`](mascot-reference.png): the clearest field marks,
   the most poster-like construction.

The final prompt:

> Flat geometric illustration of a Eurasian nuthatch (Sitta europaea), in the
> style of a 1960s Saul Bass airline travel poster and mid-century silkscreen
> prints. The bird is perched on a short diagonal branch, three-quarter
> profile facing right, alert, head raised.
>
> Anatomy must be unmistakably a nuthatch: compact plump body, big rounded
> head with no visible neck, very short square tail (much shorter than the
> wing), short legs with feet gripping the branch. Straight, sharp,
> wedge-shaped bill about as long as the head, dark grey with a paler lower
> edge. A bold black eye-stripe runs from the base of the bill, through the
> eye, back to the side of the nape. The eye is a solid black dot inside the
> stripe with a tiny white highlight. Slate blue-grey crown, nape, back and
> folded wing. Clean white cheek and throat below the stripe. Pale peach-buff
> breast and belly, and a rich rust-chestnut patch on the flanks and under
> the tail. A couple of small white spots on the corner of the short tail.
>
> Style: bold Saul Bass poster geometry — the bird is built from a few large
> shapes cut from circles, arcs and straight diagonal cuts, like cut paper;
> confident, slightly angular, iconic, simple enough to read as a logo. Feet
> are two simple dark wedge shapes gripping the branch, not detailed claws.
> Big flat colour shapes with crisp hard edges, exactly six flat colours
> (slate blue-grey, darker blue-grey for the wing, black, white, buff,
> rust-chestnut) plus a dark charcoal branch. No gradients, no texture, no
> shading, no outlines, no strokes, no text, no border. Plain flat warm
> off-white background. Bird centred, filling about two thirds of the square
> with a generous margin.

Field-mark check against the real bird: blue-grey upperparts, black stripe
from bill through eye to the side of the nape, white cheek and throat, buff
underparts, chestnut flanks and undertail (a male's), white spots in the
outer tail, long straight pointed bill, big head with no neck. The tail is
longer and the stripe curls further down the neck than on a real nuthatch;
both are poster licence kept from the reference.

## Drawing

Hand-authored in the reference bitmap's 1024 × 1024 coordinate space, by
reading coordinates off gridded crops of the reference. No auto-traced
paths, embedded bitmaps, gradients, filters or strokes. The full mascot has
**16 paths and 3 circle primitives**, layered so that no colour boundary
is two paths meeting (see below).

Each round was checked twice: a true 50 % overlay of the artwork on the
reference, and a diagnostic render of every path's outline over the
reference (outlines are diagnostic only; the artwork has no strokes). Ten
rounds:

| Round | What changed |
| --- | --- |
| 1 | Every shape blocked in from gridded crops; already the right bird, with a white gap at the bill base and a doubled eye in the overlay. |
| 2 | Eye centre moved 8 px left to (676, 250); stripe shifted ~10 px left at the nape; bill rebuilt to close the gap; legs, feet, tail end and tail spots re-placed. |
| 3 | Learned to read the overlay (a pale rim is *my* shape over background): crown dome pulled back in, cheek shrunk, belly narrowed, undertail edge pulled in, tail's lower edge re-angled. |
| 4 | Cheek, stripe and belly trimmed further. |
| 5 | Switched to the outline diagnostic, which showed rounds 3–4 had over-corrected: stripe top raised to the reference's arc, stripe lower edge carried right of the eye to the bill, cheek bottom lowered, bill's lower edge straightened, throat moved out. |
| 6 | Lower-right belly curve pulled in to the reference; throat edge eased back by 5 px; bill's lower edge given a slight sag; tail spots enlarged. |
| 7 | White seam between undertail and belly removed (undertail now runs under the belly edge); belly tightened. |
| 8 | Cheek start moved onto the stripe's inner edge to close a buff sliver; a belly-edge change tried and rejected. |
| 9 | Final geometry; colours normalised to the palette tokens; crown's lower corner tucked under the stripe (a 2 px seam visible in the head mark). |
| 10 | Review: seams and holes, worst at the bill base. Re-layered (below) with the visible outline unchanged; `check_seams.py` added. |

### Layering: no seams

Two paths that only *meet* never render cleanly: each one's anti-aliased edge
is part-transparent, and the background shows through the line where they
touch. So no boundary between two colours is shared by two paths. Each visible
boundary is the edge of exactly one shape lying over another, and the shape
below runs well under it (typically 5–10 units):

- **`body`** is one slate silhouette under the whole bird: crown, back, wing,
  belly, flank, and forward under the bill. Where slate is the outline (the
  crown and back), its edge *is* the outline. Elsewhere it stops about 6
  units inside the shape that draws the outline (belly, undertail, wing tip),
  so it can't fringe that edge.
- **The flank stack.** The tail runs under `undertail-deep`, which runs under
  `undertail`, which runs under `underparts`. The tops of all three hide
  under `primaries`.
- **The wing.** `primaries` run up under `wing-coverts`, whose lower edge,
  with its notch, is the one you see.
- **`underparts`** runs under the stripe and the cheek, and forward under the
  lower mandible. Its edge along the shoulder line (y 382) is the edge you
  see over the slate back.
- **`cheek`** reaches up under the stripe.
- **`eye-stripe`** runs on under both mandibles.
- **The bill.** `bill-lower` reaches up under the dark `bill`, and both lie
  over the head, which reaches under them.
- **Legs and feet.** The legs start inside the belly and end inside the
  feet, and the feet's heels sit inside the branch. The gap between the
  legs, the belly and the branch is deliberate.

The visible outline is unchanged. A pixel diff of the old and new art at 4×
changes only where seams and slivers were filled: the bill base, the
stripe/cheek crescent, the throat under the bill, the bill tip, the leg
tops, and the heels.

### Seam check

[`check_seams.py`](check_seams.py) proves it. It is a PEP 723 script:
`uv run docs/brand/check_seams.py`. It checks `mascot.svg`, the head mark,
the ink and paper cutouts, `logo.svg`, `favicon.svg`, and the bird inlined in
`favicon.svg`, both banners and the social card. Each is rendered with resvg,
on a transparent background, at 4× (the head marks at 4× and 8×):

1. Flood the clear region (alpha < 128) from the image border.
2. Add the listed deliberate gaps (the space between the legs, the cutouts'
   holes).
3. Widen that region by 1 px to take in the outline's own anti-aliasing.
4. Report every remaining pixel with alpha < 250, grouped into clusters with
   their position in SVG units.

The script exits 1 if it finds any. It also writes renders on magenta and on
the favicon's rust, and zoomed crops of every junction (bill base, mandibles,
eye, stripe, cheek, throat, shoulder, wing notch, flank, undertail, tail
spots, wing tip and tail, leg tops, feet). The bill base is always also
cropped at 8×. A notch in the outline that narrows to a point (the tail meets
the wing tip; the two mandible tips) leaves one anti-aliased pixel at its
apex. Clusters of 3 px or fewer within 2 px of the outside are counted as
"outline corners", not seams.

The favicon's own tile is opaque, so its alpha can't show a seam; the
`favicon-bird` target checks the bird inside it.

Seam pixels before and after the layering:

| Target | Before, 4× | After, 4× | Before, 8× | After, 8× |
| --- | ---: | ---: | ---: | ---: |
| `mascot.svg` | 6 676 (93 clusters) | 0 | — | — |
| `mascot-head.svg` | 3 884 (7) | 0 | 14 668 (7) | 0 |
| `mascot-head-ink.svg` | 0 | 0 | 0 | 0 |
| `mascot-head-paper.svg` | 0 | 0 | 0 | 0 |
| `logo.svg` | 0 | 0 | 0 | 0 |
| `favicon.svg` (whole tile) | 0 | 0 | 0 | 0 |
| bird in `favicon.svg` | 3 855 (4) | 0 | 14 635 (4) | 0 |
| bird in `header-light.svg` | 6 676 (93) | 0 | — | — |
| bird in `header-dark.svg` | 6 676 (93) | 0 | — | — |
| bird in `social-card.svg` | 6 676 (93) | 0 | — | — |

### Path inventory

“Anchors” counts move/line/Bézier endpoints, including the initial point of
each subpath, not `Z` closures or control handles. Hidden vertices (the parts
that run under a neighbour) are included. Bottom to top:

| SVG shape ID | Fill | Subpaths | Anchors | Bézier segments | Runs under |
| --- | --- | ---: | ---: | ---: | --- |
| `legs` | `umber` | 2 | 8 | 0 | belly, undertail, feet |
| `body` | `slate` | 1 | 17 | 9 | everything above |
| `tail` | `slate-deep` | 1 | 4 | 0 | `undertail-deep`, `primaries` |
| `tail-light` | `slate-mid` | 1 | 4 | 0 | `undertail-deep`, `primaries` |
| `undertail-deep` | `rust-deep` | 1 | 7 | 1 | `undertail`, `primaries` |
| `undertail` | `rust` | 1 | 6 | 3 | `underparts`, `primaries` |
| `underparts` | `buff` | 1 | 16 | 6 | wing, `cheek`, `eye-stripe`, mandibles |
| `primaries` | `slate-deep` | 1 | 11 | 1 | `wing-coverts` |
| `wing-coverts` | `slate-mid` | 1 | 7 | 1 | — |
| `tail-spots` | `cheek` | 3 | 12 | 0 | — |
| `cheek` | `cheek` | 1 | 7 | 4 | `eye-stripe`, eye |
| `eye-stripe` | `stripe` | 1 | 10 | 4 | `bill`, `bill-lower`, eye |
| `bill-lower` | `bill-pale` | 1 | 5 | 1 | `bill` |
| `bill` | `bill` | 1 | 5 | 0 | — |
| `eye-ring` | `eye-ring` | circle r 28 | — | — | `eye` |
| `eye` | `stripe` | circle r 22 | — | — | `eye-glint` |
| `eye-glint` | `cheek` | circle r 5.5 | — | — | — |
| `branch` | `branch` | 1 | 4 | 0 | `feet` |
| `feet` | `umber` | 2 | 13 | 5 | — |

The head mark, the favicon, the banners and the social card are derived from
`mascot.svg`. The head mark takes the nine head shapes (`body` through the
eye) in the same order, under the neck clip; the others inline the full
mascot. Regenerate them all whenever `mascot.svg` changes, then run the check.

## Deliverables

| File | What it is |
| --- | --- |
| [`mascot.svg`](mascot.svg) | Full mascot, transparent, `viewBox="0 0 1024 1024"`. |
| [`mascot-head.svg`](mascot-head.svg) | Head mark: the nine head shapes, clipped at the nape and the shoulder line; `viewBox="446 150 500 240"`, geometry in the full mascot's coordinates. 16–64 px tall. |
| [`mascot-head-ink.svg`](mascot-head-ink.svg) | One compound path in `ink`, cheek and pupil cut out (even-odd). For light grounds. |
| [`mascot-head-paper.svg`](mascot-head-paper.svg) | One compound path in `cheek` white, eye-stripe and eye cut out, glint kept. For dark grounds. |
| [`logo.svg`](logo.svg) | The paper head, for the docs header (primary `#223846`, 12:1). |
| [`favicon.svg`](favicon.svg) | Head mark on a rounded `rust` tile, so it reads on light and dark tabs. |
| `favicon-32.png`, `favicon.ico` (16/32/48), `apple-touch-icon.png` (180, square), `icon-512.png` | Renders of the favicon. |
| [`mascot-compare.png`](mascot-compare.png) | Reference \| 50 % overlay \| flat artwork. |
| [`mascot-side-by-side.png`](mascot-side-by-side.png) | Reference and flat artwork. |
| [`check_seams.py`](check_seams.py) | The seam and hole check (see *Seam check*). |
| [`palette.svg`](palette.svg), [`brand.css`](brand.css) | The palette, and the same tokens as CSS custom properties. |
| [`social-card.svg`](social-card.svg) / [`social-card.png`](social-card.png) | OpenGraph card, 1280 × 640. |
| [`../../.github/brand/header-light.svg`](../../.github/brand/header-light.svg), [`header-dark.svg`](../../.github/brand/header-dark.svg) | README banner, 960 × 260, mascot inline as vectors. |

## Palette

Mascot colours are the same in both schemes. OKLCH is `L C H`.

| Token | Hex | OKLCH | Used for |
| --- | --- | --- | --- |
| `slate` | `#6a808e` | `oklch(58.7% 0.033 236)` | crown, nape, back |
| `slate-mid` | `#4b6272` | `oklch(48.4% 0.038 239)` | wing coverts, tail edge |
| `slate-deep` | `#223846` | `oklch(32.9% 0.037 237)` | primaries, tail; UI `primary` |
| `stripe` | `#101317` | `oklch(18.5% 0.009 256)` | eye-stripe, eye |
| `cheek` | `#fbfaf6` | `oklch(98.5% 0.005 95)` | cheek, glint, tail spots |
| `buff` | `#f3c997` | `oklch(86.1% 0.081 71)` | breast, belly |
| `rust` | `#9b4723` | `oklch(50.0% 0.123 42)` | undertail; UI light `accent`; favicon tile |
| `rust-deep` | `#81371c` | `oklch(43.3% 0.110 40)` | undertail shadow |
| `bill` | `#30383b` | `oklch(33.4% 0.012 222)` | upper mandible |
| `bill-pale` | `#a1a5a6` | `oklch(71.9% 0.005 214)` | lower mandible |
| `eye-ring` | `#3d4246` | `oklch(37.6% 0.010 242)` | eye ring |
| `umber` | `#3f3a36` | `oklch(35.2% 0.010 61)` | legs, feet |
| `branch` | `#2a2d32` | `oklch(29.6% 0.010 261)` | perch |

Interface tokens (`brand.css` maps them onto the Material-style `--md-*`
variables for `[data-md-color-scheme="default"]` and `="slate"`):

| Token | Light (`default`) | OKLCH | Dark (`slate`) | OKLCH |
| --- | --- | --- | --- | --- |
| `background` | `#f4efe3` | `oklch(95.3% 0.017 88)` | `#121820` | `oklch(20.7% 0.018 256)` |
| `surface` | `#fffcf5` | `oklch(99.1% 0.010 87)` | `#1b232c` | `oklch(25.2% 0.021 252)` |
| `ink` | `#161a20` | `oklch(21.6% 0.013 258)` | `#f1ece0` | `oklch(94.4% 0.017 88)` |
| `muted` | `#56616b` | `oklch(48.7% 0.021 246)` | `#a5b0b8` | `oklch(75.1% 0.017 239)` |
| `primary` | `#223846` | `oklch(32.9% 0.037 237)` | `#223846` | `oklch(32.9% 0.037 237)` |
| `primary-ink` | `#ffffff` | `oklch(100% 0 0)` | `#f1ece0` | `oklch(94.4% 0.017 88)` |
| `accent` | `#9b4723` | `oklch(50.0% 0.123 42)` | `#eb9a6c` | `oklch(75.8% 0.115 50)` |
| `accent-ink` | `#ffffff` | `oklch(100% 0 0)` | `#1c0f08` | `oklch(18.5% 0.026 49)` |

### Contrast (WCAG 2.x)

Every text pair passes AA (4.5:1) for body text.

| Text on ground | Light | Dark |
| --- | ---: | ---: |
| `ink` on `background` | 15.22:1 AAA | 15.13:1 AAA |
| `ink` on `surface` | 17.04:1 AAA | 13.46:1 AAA |
| `muted` on `background` | 5.52:1 AA | 8.07:1 AAA |
| `muted` on `surface` | 6.18:1 AA | 7.18:1 AAA |
| `accent` on `background` (links) | 5.51:1 AA | 7.96:1 AAA |
| `accent` on `surface` | 6.17:1 AA | 7.08:1 AAA |
| `accent-ink` on `accent` (buttons) | 6.33:1 AA | 8.36:1 AAA |
| `primary-ink` on `primary` (header) | 12.19:1 AAA | 10.34:1 AAA |

## Usage

- **Minimum sizes.** Full mascot: 96 px tall (below that the feet and tail
  spots turn to noise; use the head). Head mark and cutouts: 16 px tall.
  Favicon: 16 px.
- **Clear space.** Around the full mascot and the head mark, keep at least
  the eye-ring's diameter (56 units, about 1/15 of the mascot’s width) free
  of text and other marks; around the favicon tile, 1/8 of its side.
- **Grounds.** Full colour on `background`, `surface`, white or the dark
  `background`. On `primary` or other mid/dark slates use the paper cutout
  (the crown is too close in value to `slate-deep`). On photographs use a
  tile.
- **Don't** recolour the bird outside the mascot tokens, add gradients,
  strokes, shadows or outlines, flip it to face left, rotate it, stretch it,
  crop through the eye or the bill, separate the stripe from the eye, put
  text over it, or set the tagline in anything other than the header's
  type. Don't trace the reference again: edit the named shapes.
