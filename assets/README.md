# Device icon

`icon.svg` is the source. It draws the instrument itself: the body with the shoulder sloping away
from the pin block, and five strings that shorten towards the narrow side.

It is drawn in filled paths only, with no stroke anywhere. Homebrew's ImageMagick carries no rsvg
delegate and falls back to its own renderer, which draws every stroke one pixel wide whatever the
file asks: a stroke here would come out right in the release job and wrong on a Mac.

The four raster files are what goes on the wire, because `<iconList>` and `upnp:albumArtURI` are
PNG and JPEG and a control point renders no SVG. They are rendered from `icon.svg` rather than
drawn, so the source stays the only thing anybody edits:

    for s in 48 120; do
      magick -background none -density 1200 icon.svg -resize ${s}x${s} \
        -depth 8 -define png:color-type=2 -strip icon-$s.png
      magick -background "#101214" -density 1200 icon.svg -resize ${s}x${s} \
        -flatten -depth 8 -quality 92 -strip icon-$s.jpg
      done

The two sizes and the two formats are what the DLNA guidelines ask a media server to publish,
read from the specification and not measured against a device.

JPEG carries no transparency, so the JPEG files are flattened on the colour of the square.

# Configuration page

`web/index.html` is built from `web/` and committed, so a clone builds with no node. See
`CONTRIBUTING.md`.
