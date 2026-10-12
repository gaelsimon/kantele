#!/bin/bash
# Builds the site Pages serves: the page, its icon read off the package, and the catalogue Package
# Center reads, which points at the release that carries the package.
# usage: tools/package-pages.sh <the .spk> <its release tag> [out, pages by default]
set -euo pipefail
cd "$(dirname "$0")/.."
spk="$1"
tag="$2"
out="${3:-pages}"
repository="${GITHUB_REPOSITORY:-gaelsimon/kantele}"
server="${GITHUB_SERVER_URL:-https://github.com}"
owner="${repository%%/*}"
name="${repository#*/}"

mkdir -p "$out"
cp -R packaging/pages/. "$out/"
tar -xOf "$spk" PACKAGE_ICON_256.PNG > "$out/kantele.png"
python3 tools/package-index.py "$spk" \
  --link "$server/$repository/releases/download/$tag/$(basename "$spk")" \
  --thumbnail "https://$owner.github.io/$name/kantele.png" \
  --out "$out/index.json"
