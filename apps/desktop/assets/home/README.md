# Home ASCII artwork

`black-hole.txt` is the supplied 300 × 100 character grid. Leading spaces and empty
rows are significant; omitted trailing spaces/rows are padded to 300 × 100.
`black-hole.rgb` stores one sRGB foreground colour per cell, sampled from the
existing `../black-hole-ascii.png`. Spaces have no ink. `glyphs.r8` is a 160 × 28
single-channel atlas of ` .:-=+*#%@`, rendered with Menlo Regular.

The shader slides the characters across the disc, with opposite flow on either
side of the centre. Each moving character samples the supplied grid's density and
colour at its new position, retaining the silhouette, void and palette. Adjacent
source cells blend on the same glyph footprint to keep the motion smooth and the
ink crisp. Integer cell travel makes the eight-second clock wrap seamless.
Reduce Motion renders the original stationary grid. The reference image's aspect
ratio and centre crop are retained.

To regenerate the colour map and atlas, install Pillow and supply a monospace font:

```sh
python3 scripts/design/home_ascii.py --font path/to/monospace.ttf
```

`metadata.json` records the source hashes and font used. The committed data avoids
font loading, image decoding and colour sampling on Home's rendering path.
