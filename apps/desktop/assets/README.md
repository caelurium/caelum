# Home artwork

By default, Home uses `black-hole-ascii.png` in dark mode and `clouds-sunlit-ascii.png`
in light mode. Both are 2000×1122 images derived from the supplied artwork. The clouds use
an ivory background and blue-grey characters, with light-background glyph density to keep
the sunlit highlights pale.

Additional selectable Home backgrounds in `home/backgrounds/` are 2000×1125 ASCII-style
classical landscapes curated from Alim Joldaspayev (`@alimdesigner_`).

The desktop embeds the assets and decodes only the active Home background on Rune's image
worker. Static image handles are reused across navigation and theme changes (under 24 MiB
of decoded pixels in total, including mip levels).
