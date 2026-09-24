"""Generate the deterministic chart used by the benchmark document.

Run from this directory:  python make_chart.py
Requires Pillow. The output, ``energy-chart.png``, is committed so the benchmark
has no build-time dependency.
"""

from PIL import Image, ImageDraw, ImageFont

WIDTH, HEIGHT = 720, 400
BG = (255, 255, 255)
AXIS = (120, 130, 140)
TEXT = (45, 55, 65)
GRID = (226, 232, 238)

SOURCES = ["Solar", "Wind", "Hydro", "Nuclear", "Gas"]
VALUES = [245, 118, 26, 12, 41]
COLORS = [(240, 170, 40), (60, 150, 190), (70, 170, 120), (150, 120, 190), (150, 150, 150)]

img = Image.new("RGB", (WIDTH, HEIGHT), BG)
draw = ImageDraw.Draw(img)

try:
    font = ImageFont.truetype("arial.ttf", 16)
    small = ImageFont.truetype("arial.ttf", 13)
except OSError:
    font = ImageFont.load_default()
    small = font

left, right, top, bottom = 70, 30, 40, 60
plot_w = WIDTH - left - right
plot_h = HEIGHT - top - bottom

max_value = 260
# Horizontal gridlines and y labels.
for step in range(0, max_value + 1, 65):
    y = top + plot_h - int(plot_h * step / max_value)
    draw.line([(left, y), (WIDTH - right, y)], fill=GRID, width=1)
    draw.text((left - 12, y), str(step), fill=TEXT, font=small, anchor="rm")

# Axes.
draw.line([(left, top), (left, top + plot_h)], fill=AXIS, width=2)
draw.line([(left, top + plot_h), (WIDTH - right, top + plot_h)], fill=AXIS, width=2)

# Bars.
slot = plot_w / len(SOURCES)
bar_w = slot * 0.55
for i, (name, value, color) in enumerate(zip(SOURCES, VALUES, COLORS)):
    cx = left + slot * (i + 0.5)
    x0 = cx - bar_w / 2
    x1 = cx + bar_w / 2
    y0 = top + plot_h - int(plot_h * value / max_value)
    draw.rectangle([x0, y0, x1, top + plot_h], fill=color)
    draw.text((cx, y0 - 8), str(value), fill=TEXT, font=small, anchor="mb")
    draw.text((cx, top + plot_h + 8), name, fill=TEXT, font=font, anchor="ma")

draw.text((left, 14), "Capacity additions by source (GW, 2025)", fill=TEXT, font=font)

img.save("energy-chart.png")
print("wrote energy-chart.png")
