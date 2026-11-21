"""Draw Ratatui buffer proof exported by monitor_export_native_render.

Usage: python scripts/render_monitor_buffer.py CELLS.json OUTPUT.png
Requires Pillow. Uses installed Consolas; does not run a client or send input.
"""
import json
import os
from pathlib import Path
import re
import sys
from PIL import Image, ImageDraw, ImageFont

source = json.loads(Path(sys.argv[1]).read_text())
width, height = source['width'], source['height']
image = Image.new('RGB', (width * 10, height * 20), '#191923')
draw = ImageDraw.Draw(image)
font = ImageFont.truetype(str(Path(os.environ['WINDIR']) / 'Fonts/consola.ttf'), 17)
def color(value, fallback):
    match = re.fullmatch(r'Rgb\((\d+), (\d+), (\d+)\)', value)
    return tuple(map(int, match.groups())) if match else fallback
for index, cell in enumerate(source['cells']):
    x, y = index % width * 10, index // width * 20
    draw.rectangle((x, y, x + 9, y + 19), fill=color(cell['bg'], '#191923'))
    draw.text((x, y), cell['symbol'], font=font, fill=color(cell['fg'], '#dddddd'))
image.save(sys.argv[2])
