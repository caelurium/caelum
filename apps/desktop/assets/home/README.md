# Home ASCII artwork

`black-hole.txt` is the supplied 300 × 100 character grid. Leading spaces and empty
rows are significant; omitted trailing spaces/rows are padded to 300 × 100.
`black-hole.rgb` stores one sRGB foreground colour per cell, sampled from the
existing `../black-hole-ascii.png`. Spaces have no ink. `glyphs.r8` is a 160 × 28
single-channel atlas of ` .:-=+*#%@`, rendered with Menlo Regular.

The shader spins counter-flowing characters around a fixed silhouette, with slow,
independent changes in individual character brightness. The rows and glyph density
stay steady, preserving the black hole's shape, empty centre and original palette.
Adjacent source cells blend on the same glyph footprint to keep the ink crisp.
Sparse stationary stars use `.` and occasional `+` glyphs in the background, with
gentle twinkling. They are excluded from the artwork and central void.
Integer cell travel and whole-cycle brightness frequencies keep the 48-second
clock wrap continuous. Reduce Motion holds the original grid and stars still.
The reference image's aspect ratio and centre crop are retained.

To regenerate the colour map and atlas, install Pillow and supply a monospace font:

```sh
python3 scripts/design/home_ascii.py --font path/to/monospace.ttf
```

`metadata.json` records the source hashes and font used. The committed data avoids
font loading, image decoding and colour sampling on Home's rendering path.
