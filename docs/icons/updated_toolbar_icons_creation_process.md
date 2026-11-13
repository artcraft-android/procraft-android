# Designing a Custom Icon Set from Scratch

Using Adobe Illustrator, I (github.com/lork-fork) designed a set of eight solid-glyph icons for the procraft toolbar: Brush, Smudge, Adjustments, Selection, Actions, Transform, Erase, Layers.

---

## Inspiration

I collected inspiration from several icon libraries and from the tool icons used in common creative software.

Then I closed the references and drew from scratch. I did not place any of them on the artboard, run Image Trace, or copy a path. Because every icon was built from primitive shapes and hand-placed anchor points, none of them matches any single source. The set's character comes from the choices described below.

I built each icon from basic shapes: rectangles, ellipses and rounded rectangles. Using primitives keeps curves smooth and geometry consistent across the set.

### Combining and Cutting

Using the Pathfinder panel:

- Unite to fuse a shape's parts into one silhouette
- Minus Front to carve negative-space details
- Divide / Intersect to split shapes and trim overlaps

### Refining Anchor Points

This is where the personality comes from. With the Direct Selection tool I:

- removed redundant anchors
- put anchors at extrema (top, bottom, left, right of curves)
- nudged handles so curves flow smoothly
- adjusted corner radii by hand, instead of leaving everything at one default



```
new icons/
├── brush.svg
├── smudge.svg
├── adjustments.svg
├── selection.svg
├── actions.svg
├── transform.svg
├── erase.svg
└── layers.svg
```

Each file is a 24 × 24 viewBox with no fixed fill color, so it inherits `currentColor` or can be styled with CSS.
