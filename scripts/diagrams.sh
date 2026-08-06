#!/usr/bin/env bash
# Renders the documentation diagrams from their source to PNG.
#
#   scripts/diagrams.sh          render every diagram
#   scripts/diagrams.sh nxm-with-pgprox   render one
#
# The PNGs under `docs/img/` are committed, because a documentation site that
# needs a browser installed before it can show a picture is a site that will one
# day ship without the picture. The source is committed beside them so they stay
# something that can be corrected rather than a binary nobody can edit.
#
# # Why HTML and a browser rather than a diagram format
#
# The two obvious alternatives both cost more than they look. A hand-written SVG
# puts every text position in the file, so a wording change becomes a layout
# change. A diagram-as-code tool is a toolchain, and the last one this repository
# reached for wanted a headless Chrome anyway.
#
# This is HTML and CSS, laid out by the browser, at one fixed width. It is not
# responsive and should not try to be: the output is a picture.
#
# # Why the pixel size is what it is
#
# 1,500 CSS pixels at a device scale of 2, so a 3,000-pixel-wide PNG. Wide
# enough that a reader on a high-density display sees the small type rather than
# a smear of it, and small enough that the files stay well inside the
# large-file hook's limit.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

SRC=docs/img/src
OUT=docs/img
SCALE=2
WIDTH=1500

# The tallest sheet is about 1,050 CSS pixels. Chrome screenshots the window
# rather than the document, so this is a ceiling and the image is cropped back
# to its content afterwards.
HEIGHT=1400

browser=""
for candidate in google-chrome chromium chromium-browser; do
  if command -v "$candidate" >/dev/null 2>&1; then
    browser="$candidate"
    break
  fi
done

if [[ -z "$browser" ]]; then
  fail "no Chrome or Chromium on PATH, and the diagrams are rendered by one"
  printf '       the committed PNGs are still there; this only rebuilds them\n'
  finish
fi

if ! python3 -c 'import PIL' 2>/dev/null; then
  fail "missing Pillow (install: pip install pillow), which crops the render"
  finish
fi

WANTED="${1:-}"
rendered=0

for source in "$SRC"/*.html; do
  [[ -f "$source" ]] || continue
  name="$(basename "$source" .html)"
  if [[ -n "$WANTED" && "$name" != "$WANTED" ]]; then
    continue
  fi

  # A fresh profile per run. Without it Chrome reuses whatever is in the default
  # one, which on a developer machine means a running browser and a screenshot
  # that never arrives.
  profile="$(mktemp -d)"

  if ! "$browser" --headless --disable-gpu --no-sandbox --hide-scrollbars \
      --user-data-dir="$profile" \
      --force-device-scale-factor="$SCALE" \
      --window-size="$WIDTH,$HEIGHT" \
      --screenshot="$OUT/$name.png" \
      "$source" >/dev/null 2>&1; then
    fail "$name: the browser did not produce a screenshot"
    rm -rf "$profile"
    continue
  fi
  rm -rf "$profile"

  # Crop the empty page below the sheet. The window height is a ceiling rather
  # than a measurement, so without this every diagram carries a different
  # amount of blank space and a wording change moves it.
  python3 - "$OUT/$name.png" <<'PY'
import sys
from PIL import Image

path = sys.argv[1]
image = Image.open(path).convert("RGB")
# The page colour, read from the corner rather than hard-coded, so the
# stylesheet stays the one place it is decided.
page = image.getpixel((0, 0))
flat = tuple((channel, channel) for channel in page)

# Scan up from the bottom for the last row that is not all page colour, then
# keep a margin below it that matches the sheet's own padding.
for y in range(image.height - 1, -1, -1):
    if image.crop((0, y, image.width, y + 1)).getextrema() != flat:
        cropped = image.crop((0, 0, image.width, min(image.height, y + 89)))
        # Palette rather than truecolour. These are flat fills and antialiased
        # text over about a dozen hues, so 256 entries hold the whole picture
        # and the file lands around a third of the size. That matters here: at
        # full colour two of the three sat within twenty kilobytes of the
        # large-file hook's limit, so a wording change could have failed a
        # commit for a reason that looked like nothing to do with it.
        cropped.quantize(colors=256, dither=Image.Dither.NONE).save(
            path, optimize=True
        )
        break
PY

  size="$(du -h "$OUT/$name.png" | cut -f1)"
  dims="$(python3 -c "from PIL import Image; print('x'.join(map(str, Image.open('$OUT/$name.png').size)))")"
  ok "$name.png ($dims, $size)"
  rendered=$((rendered + 1))
done

if (( rendered == 0 )); then
  fail "nothing rendered: no source in $SRC${WANTED:+ named $WANTED}"
fi

finish
