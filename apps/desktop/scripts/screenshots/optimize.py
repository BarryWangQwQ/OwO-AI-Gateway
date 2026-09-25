"""Shrinks screenshots for the README: 256-colour palette PNGs, scaled down only as far as needed
to stay under the size budget (the captures are 2x; 1x is the floor).

    python optimize.py shot.png [more.png ...]
"""

import io
import os
import sys
import time

from PIL import Image

BUDGET = 400 * 1024
SCALES = (1.0, 0.8, 0.65, 0.5)


def encode(image: Image.Image, scale: float) -> bytes:
    if scale != 1.0:
        image = image.resize((round(image.width * scale), round(image.height * scale)), Image.Resampling.LANCZOS)
    palette = image.quantize(colors=256, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.NONE)
    buffer = io.BytesIO()
    palette.save(buffer, format="PNG", optimize=True)
    return buffer.getvalue()


def shrink(path: str) -> None:
    with Image.open(path) as source:
        image = source.convert("RGB")
    before = image.size
    for scale in SCALES:
        data = encode(image, scale)
        if len(data) <= BUDGET:
            break
    temp = f"{path}.tmp"
    with open(temp, "wb") as out:
        out.write(data)
    # A freshly written PNG can be held open for a moment by a virus scanner or the indexer on Windows.
    for attempt in range(20):
        try:
            os.replace(temp, path)
            break
        except OSError:
            if attempt == 19:
                raise
            time.sleep(0.25)
    after = Image.open(io.BytesIO(data)).size
    print(f"{path}: {before[0]}x{before[1]} -> {after[0]}x{after[1]}, {len(data) // 1024} KB")


if __name__ == "__main__":
    for arg in sys.argv[1:]:
        shrink(arg)
