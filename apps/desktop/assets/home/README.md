# Home ASCII artwork

`black-hole.txt` is the supplied 300 × 100 character grid. Leading spaces and empty
rows are significant; omitted trailing spaces/rows are padded to 300 × 100.
`black-hole.rgb` stores one sRGB foreground colour per cell, sampled from the
existing `../black-hole-ascii.png`. Spaces have no ink. `glyphs.r8` is a 160 × 28
single-channel atlas of ` .:-=+*#%@`, rendered with Menlo Regular.

The source grid stays fixed while a shader moves highlights around the disc and
briefly crossfades neighbouring density glyphs. Colour remains attached to its
source cell. The shader keeps the reference image's aspect ratio and centre crop.

To regenerate the colour map and atlas, install Pillow and supply a monospace font:

```sh
python3 scripts/design/home_ascii.py --font path/to/monospace.ttf
```

`metadata.json` records the source hashes and font used. The committed data avoids
font loading, image decoding and colour sampling on Home's rendering path.
