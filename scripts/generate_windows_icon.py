#!/usr/bin/env python3
"""Generate packaging/windows/moeka.ico from the 1024 app-icon master.

The master is drawn for macOS: the artwork sits inside the standard margin and
carries a baked drop shadow. Windows draws its own shadow and expects the
artwork to fill the canvas, so keeping the margin is what makes the taskbar icon
look a size smaller than everything beside it. Crop to the opaque body first.

Usage: generate_windows_icon.py [source-1024.png] [output.ico]
"""

import sys
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    sys.exit("error: Pillow is required (pip3 install --user Pillow)")

SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]

source = Path(sys.argv[1] if len(sys.argv) > 1 else "packaging/macos/AppIcon-1024.png")
output = Path(sys.argv[2] if len(sys.argv) > 2 else "packaging/windows/moeka.ico")

master = Image.open(source).convert("RGBA")
# The shadow fades out well past the artwork, so threshold it away first.
body = master.getchannel("A").point(lambda value: 255 if value > 64 else 0).getbbox()
if body is None:
    sys.exit(f"error: {source} is fully transparent")

output.parent.mkdir(parents=True, exist_ok=True)
master.crop(body).save(output, sizes=[(size, size) for size in SIZES])
print(f"Created {output} from {source} cropped to {body}")
