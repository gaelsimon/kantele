# Device icon

`icon.svg` is the source. Its glyph is `library_music` from Google's Material Icons, taken
unmodified from `google/material-design-icons` and recoloured, on a rounded square of this
server's own. Those icons are Apache-2.0, which asks for the notice below to travel with them.

The four raster files are what goes on the wire, because `<iconList>` and `upnp:albumArtURI` are
PNG and JPEG and a control point renders no SVG. They are rendered from `icon.svg` rather than
drawn, so the source stays the only thing anybody edits:

    for s in 48 120; do
      magick -background none -density 1200 icon.svg -resize ${s}x${s} \
        -depth 8 -define png:color-type=2 -strip icon-$s.png
      magick -background "#1f2933" -density 1200 icon.svg -resize ${s}x${s} \
        -flatten -depth 8 -quality 92 -strip icon-$s.jpg
      done

The two sizes and the two formats are what the DLNA guidelines ask a media server to publish,
read from the specification and not measured against a device.

# Configuration page

`web/index.html` is built from `web/` and committed, so a clone builds with no node. See
`CONTRIBUTING.md`.

# Notice

The icon's glyph is Material Icons, Copyright Google LLC, licensed under the Apache License,
Version 2.0. <http://www.apache.org/licenses/LICENSE-2.0>
