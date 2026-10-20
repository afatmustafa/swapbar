#!/usr/bin/env python3
"""Regenerate assets/AppIcon.ico, the Windows exe icon, from assets/AppIcon.icns (needs Pillow).

The .icns follows the macOS icon grid (an 824 px tile inside a 1024 px canvas); Windows icons
fill more of their canvas, so the tile is cropped with a thin margin first.
"""

from pathlib import Path

from PIL import Image

root = Path(__file__).resolve().parent.parent
# the largest size, 1024 px; the PNGs inside are palette images, which ICO frames can't be
icon = Image.open(root / "assets" / "AppIcon.icns").convert("RGBA")
margin = 100 - 28
tile = icon.crop((margin, margin, 1024 - margin, 1024 - margin))
sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
out = root / "assets" / "AppIcon.ico"
tile.save(out, sizes=[(s, s) for s in sizes])
print(f"wrote {out.relative_to(root)} ({out.stat().st_size} bytes)")
