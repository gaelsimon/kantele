#!/bin/bash
# Builds the Synology package: one .spk per architecture under dist/, from the static musl binary.
# usage: tools/package-spk.sh [x86_64] [armv8]
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)-1"
ARCHES=("${@:-x86_64}")
SRC=packaging/synology
OUT=dist
mkdir -p "$OUT"

# The page is built before the binary that embeds it, so a package never carries yesterday's page.
echo "== page"
(cd web && npm ci --silent && npm run build --silent)
if ! git diff --quiet -- assets/web; then
  echo "assets/web is not what web/ builds; commit the built page before packaging" >&2
  git --no-pager diff --stat -- assets/web >&2
  exit 1
fi

target_for() {
  case "$1" in
    x86_64) echo x86_64-unknown-linux-musl ;;
    armv8) echo aarch64-unknown-linux-musl ;;
    *) echo "unknown architecture $1" >&2; exit 1 ;;
  esac
}

MAGICK="$(command -v magick || command -v convert || true)"
if [ -z "$MAGICK" ]; then
  echo "the icons are rendered with ImageMagick: brew install imagemagick, or apt install imagemagick" >&2
  exit 1
fi

# DSM wants the package icon in two sizes and the desktop icon in several, all from the SVG, which
# is the source the four shipped rasters are rendered from too.
icons() {
  local work="$1"
  mkdir -p "$work/icons"
  for size in 16 24 32 48 64 72 256; do
    "$MAGICK" -background none -density 1200 assets/icon.svg -resize "${size}x${size}" \
      -depth 8 -define png:color-type=6 -strip "$work/icons/kantele_$size.png"
  done
}

for arch in "${ARCHES[@]}"; do
  target="$(target_for "$arch")"
  echo "== $arch: building $target"
  cargo zigbuild --release --target "$target"
  work="$(mktemp -d)"
  payload="$work/payload"
  mkdir -p "$payload/bin" "$payload/share" "$payload/ui/images"
  cp "target/$target/release/kantele" "$payload/bin/kantele"
  cp kantele.example.toml "$payload/share/kantele.example.toml"
  cp "$SRC/ui/config" "$payload/ui/config"
  icons "$work"
  cp "$work"/icons/kantele_*.png "$payload/ui/images/"
  spk="$work/spk"
  mkdir -p "$spk/scripts" "$spk/conf"
  tar -czf "$spk/package.tgz" -C "$payload" .
  checksum="$(md5 -q "$spk/package.tgz" 2>/dev/null || md5sum "$spk/package.tgz" | cut -d' ' -f1)"
  sed -e "s/@VERSION@/$VERSION/" -e "s/@ARCH@/$arch/" -e "s/@CHECKSUM@/$checksum/" "$SRC/INFO.in" > "$spk/INFO"
  cp "$SRC/scripts/start-stop-status" "$spk/scripts/"
  for script in preinst postinst preuninst postuninst preupgrade postupgrade; do
    printf '#!/bin/sh\nexit 0\n' > "$spk/scripts/$script"
  done
  chmod 755 "$spk"/scripts/*
  cp "$SRC/conf/privilege" "$spk/conf/privilege"
  cp "$work/icons/kantele_72.png" "$spk/PACKAGE_ICON.PNG"
  cp "$work/icons/kantele_256.png" "$spk/PACKAGE_ICON_256.PNG"
  out="$OUT/kantele-$VERSION-$arch.spk"
  tar -cf "$out" -C "$spk" INFO PACKAGE_ICON.PNG PACKAGE_ICON_256.PNG package.tgz scripts conf
  rm -rf "$work"
  echo "== wrote $out ($(du -h "$out" | cut -f1))"
done
