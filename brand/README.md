# Tsubame — brand assets

The swallow. One concept, drawn at two densities; there is no separate wordmark to keep in sync.

| File | Use | Minimum |
|---|---|---|
| `tsubame.svg` | the mark. Favicon, GitHub avatar, docs header, anything ≥ 24px | 24px |
| `tsubame-16.svg` | the same bird redrawn for a 16px raster — shorter wing, shorter tail lobes | 16px |
| `tsubame-lockup.svg` | mark + name, for a docs header or a README banner | 150px wide |

Every file paints with `currentColor`, so set `color` on the container and the mark follows light
and dark themes on its own. Inlined in HTML is best; as an `<img>` the colour falls back to black.

## What the mark is

A swallow in flight: a flat wedge of a head, one smooth tapered wing, a forked tail. Two
proportions are load-bearing, and both were arrived at by drawing them wrong first:

- **The head's top edge runs level with the wing root**, continuing the line of the back. Raising
  the crown turns the head into a blob and the mark stops reading as a bird. The beak is simply
  where that wedge comes to a point; it needs no separate notch or hook.
- **The wing tapers as one smooth arc** from shoulder to tip. A joint at the wrist was tried and
  rejected: it made the bird read as a paper plane, and slimming the body around it turned the
  whole mark into a check mark. The sweep is carried by the taper alone — do not add a joint back.

The tail's fork is what makes the bird a **swallow** rather than a bird. It is the last thing to
give up if the mark ever has to shrink further.

## The name

Always `Tsubame`. Sentence case — not `tsubame`, not `TSUBAME`, not `Tsubame CMS`.

Type is the system UI sans (`-apple-system, Segoe UI, Roboto, …`), weight 600, letter-spacing
`-1` at 40px. That single line is the whole typographic standard; match it rather than inventing
a second treatment.

## Using it

- One flat colour. No gradient, no outline, no drop shadow, no rotation.
- Clear space on every side: at least a quarter of the mark's height.
- The one exception is the browser tab icon (`frontend/public/favicon.svg` and the `.ico` rasterised
  from it): it stands on a rounded plate of the ink, with the mark in the paper colour. A tab strip is
  painted by the browser and does not reliably say whether it is light or dark, so a bare mark is
  legible on one and vanishes on the other. Its clear space is the plate's own margin rather than the
  quarter above - at 16px there is no room for both a plate and that much air, and the mark is what
  has to survive there.
- Do not stretch, and do not place the mark on a busy photograph.
- Below 24px use `tsubame-16.svg`; at 16px the tail fork closes up and the bird reads as a solid
  diagonal mass. That is expected — the silhouette is doing the work, not the fork.

## The kanji

`燕` is an optional accent for large, Japanese-language surfaces. Set it in your display font as
text — it is deliberately **not** shipped as an SVG asset, because a glyph depends on the font
that happens to be installed and would render differently for every viewer.

Do not run it as a third mark alongside the silhouette: one concept, two renderings.
