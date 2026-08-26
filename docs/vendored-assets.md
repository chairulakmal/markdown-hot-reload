# Vendored assets

This document is the maintenance procedure for the files in `assets/`, the frontend dependencies that `rust-embed` compiles into the `mhr` binary. The point that matters most: every one of these files is refreshed by hand, and two of them are version-locked to something else in the repository, so a mismatch produces a wrong render rather than a build error. Below: the inventory table, how `highlight.css` is generated, the icon set and its design rules, how to regenerate the window icon bitmap, and the pairing rule for the math stylesheet and fonts.

- [Inventory](#inventory)
- [highlight.css](#highlightcss)
- [The icon set](#the-icon-set)
- [window-icon.rgba](#window-iconrgba)
- [latex.css and the fonts](#latexcss-and-the-fonts)

To refresh any file, download it, replace it in place, and rebuild.

## Inventory

| File | Version | Source |
| --- | --- | --- |
| `idiomorph.min.js` | 0.7.3 | `cdn.jsdelivr.net/npm/idiomorph@0.7.3/dist/idiomorph.min.js` |
| `mermaid.min.js` | 11.x | `cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js` |
| `highlight.css` | generated | syntect 5.3.0, see below |
| `latex.css` | 0.8.0 | `github.com/carloskiki/pulldown-latex` release `0.8.0`, `styles.css` |
| `font/*.woff2` | 0.8.0 | `github.com/carloskiki/pulldown-latex` release `0.8.0`, `font/` |
| `icon/*.svg` | original | the project's own mark, four variants, see below |
| `icon/window-icon.rgba` | generated | rasterised from `icon/mhr-icon.svg`, see below |

Mermaid is 3.5 MB raw and dominates the binary, so it is loaded only when a document actually contains a diagram.

## highlight.css

`highlight.css` is generated rather than downloaded. It is the two syntax-highlighting palettes, `InspiredGitHub` for a light page and `base16-ocean.dark` for a dark one, produced by syntect's `css_for_theme_with_class_style` with `ClassStyle::SpacedPrefixed { prefix: "hl-" }`. Each palette sits inside its own `prefers-color-scheme` media query, because the two themes do not emit the same selector set: the light theme has rules the dark one does not cover, and some of them are specific enough to take precedence. Regenerate it with a throwaway crate depending on `syntect` at the version in `Cargo.lock`; the file's own header comment records the exact call. The dead `.hl-code` rule is removed during generation, since the `<pre>` never carries that class and the rule's `background-color` would conflict with `--code-bg`.

## The icon set

The icon set is the project's own mark: a 100x100 grid split into an ink pane holding an `m` and a red pane holding a `d`, all strokes 7 units, both letters on one baseline. `mhr-icon.svg` is the primary, `mhr-icon-mono.svg` is one-colour, `mhr-icon-on-dark.svg` inverts the left pane, and `mhr-icon-16.svg` thickens the strokes to 8 units and sets `shape-rendering="crispEdges"` for 16px. The palette is ink `#201e1d`, red `#ec3013`, ground `#f3f2f2`, and nothing is rounded: corner radius is 0 everywhere, including any app-icon mask. The 4-unit channel between the panes is part of the mark, so never close it, never recolour the panes into two tints of red, and never letterbox the mark inside a rounded container.

## window-icon.rgba

`icon/window-icon.rgba` exists because `tao::window::Icon::from_rgba` takes pixels, not an encoded image, and decoding a PNG or an SVG at runtime would mean a dependency for one 128px bitmap. It is 128x128x4 raw bytes, so a size change means editing `assets::ICON_SIZE` too; a test asserts the two agree. `rust-embed` compresses it down to roughly 4 KB in the binary. Regenerate it from the SVG, from the repository root, after any change to the mark:

```
python3 -c "
import gi; gi.require_version('GdkPixbuf', '2.0')
from gi.repository import GdkPixbuf
pb = GdkPixbuf.Pixbuf.new_from_file_at_size('assets/icon/mhr-icon.svg', 128, 128)
s, w, px = pb.get_rowstride(), pb.get_width(), pb.get_pixels()
open('assets/icon/window-icon.rgba', 'wb').write(
    b''.join(px[y * s : y * s + w * 4] for y in range(pb.get_height())))
"
```

That uses the librsvg loader behind GdkPixbuf, which is already present on any machine that can build this app. `rsvg-convert` or Inkscape would do as well; what matters is that the bitmap is rasterised from the SVG rather than drawn again by hand.

## latex.css and the fonts

`latex.css` and the four Latin Modern fonts come from the same `pulldown-latex` release as the crate version in `Cargo.toml`, and must be refreshed together with it: the stylesheet targets classes the crate emits (`menv-align`, `menv-cases` and friends), so a version mismatch silently mis-aligns matrices and align environments rather than failing. The fonts are 528 KB and are vendored rather than left to the system on purpose, for the same reason the app has no network access: a machine with no math font installed should not render math differently from one that has.
