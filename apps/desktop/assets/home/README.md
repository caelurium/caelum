# Home ASCII artwork

`black-hole.txt` is the supplied 300 × 100 character grid. Leading spaces and empty
rows are significant; omitted trailing spaces/rows are padded to 300 × 100.
`black-hole.rgb` stores one sRGB foreground colour per cell, sampled from the
existing `../black-hole-ascii.png`. Spaces have no ink. `glyphs.r8` is a 160 × 28
single-channel atlas of ` .:-=+*#%@`, rendered with Menlo Regular.

The shader combines counter-flowing characters with rippling bands, local eddies,
slow expansion/contraction and travelling highlights. Characters also transition
between neighbouring density glyphs from the supplied alphabet. All variation is
smooth and bounded, retaining the silhouette, empty centre and original palette.
Adjacent source cells blend on the same glyph footprint to keep the ink crisp.
The motions run at different rates within a seamless 48-second cycle; integer cell
travel and whole-cycle wave frequencies keep the clock wrap continuous.
Reduce Motion renders the original stationary grid. The reference image's aspect
ratio and centre crop are retained.

To regenerate the colour map and atlas, install Pillow and supply a monospace font:

```sh
python3 scripts/design/home_ascii.py --font path/to/monospace.ttf
```

`metadata.json` records the source hashes and font used. The committed data avoids
font loading, image decoding and colour sampling on Home's rendering path.
