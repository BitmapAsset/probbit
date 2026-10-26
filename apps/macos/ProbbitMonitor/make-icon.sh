#!/bin/sh
# Regenerates Resources/AppIcon.icns from docs/brand/logo.svg: the white mark on an ink rounded square (the macOS icon grid:
# an 824 px square in a 1024 px canvas), rasterized at 1024 px by headless Chrome, scaled by sips, packed by iconutil.
# Run it when the logo changes and commit the .icns; build.sh uses the committed file and needs no browser.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
chrome=${CHROME:-"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"}
work=$(mktemp -d "${TMPDIR:-/tmp}/probbit-icon.XXXXXX")
trap 'rm -rf "$work"' EXIT

# the logo's paths, inlined into a page of exactly 1024 x 1024
mark=$(sed -e 's/<title>[^<]*<\/title>//' "$root/docs/brand/logo.svg")
cat > "$work/icon.html" <<EOF
<!doctype html><html><head><style>
html,body{margin:0;width:1024px;height:1024px;background:transparent;overflow:hidden}
.tile{position:absolute;left:100px;top:100px;width:824px;height:824px;border-radius:185px;background:#0b0e14}
.tile svg{position:absolute;left:192px;top:132px;width:440px;height:556px}
</style></head><body><div class="tile">$mark</div></body></html>
EOF
"$chrome" --headless=new --disable-gpu --hide-scrollbars --default-background-color=00000000 \
  --window-size=1024,1024 --screenshot="$work/icon-1024.png" "file://$work/icon.html" > /dev/null 2>&1

set_dir="$work/AppIcon.iconset"
mkdir "$set_dir"
for s in 16 32 128 256 512; do
  sips -z $s $s "$work/icon-1024.png" --out "$set_dir/icon_${s}x${s}.png" > /dev/null
  d=$((s * 2))
  sips -z $d $d "$work/icon-1024.png" --out "$set_dir/icon_${s}x${s}@2x.png" > /dev/null
done
iconutil -c icns "$set_dir" -o "$here/Resources/AppIcon.icns"
echo "wrote $here/Resources/AppIcon.icns"
