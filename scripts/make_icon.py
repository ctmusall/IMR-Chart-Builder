"""Draw the IMR Chart Builder icon: an individuals control chart with limits and one point out of control.

Writes imr_chart/assets/icon.png (256 px, the window icon) and icon.ico (16-256 px, the Windows exe).
Run:  python scripts/make_icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "imr_chart" / "assets"
S = 1024  # drawn large, then scaled down smoothly

NAVY = (20, 33, 61)
CENTER = (143, 163, 191)
LIMIT = (229, 72, 77)
LINE = (245, 166, 35)
WHITE = (255, 255, 255)


def dashed(d: ImageDraw.ImageDraw, y: int, x0: int, x1: int, dash=70, gap=45, width=26, fill=LIMIT):
    x = x0
    while x < x1:
        d.line((x, y, min(x + dash, x1), y), fill=fill, width=width)
        x += dash + gap


def draw() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((32, 32, S - 32, S - 32), radius=190, fill=NAVY)

    left, right = 130, 894
    ucl, center, lcl = 300, 540, 780
    dashed(d, ucl, left, right)
    dashed(d, lcl, left, right)
    d.line((left, center, right, center), fill=CENTER, width=18)

    # Individuals: wander around the centre line, one point breaks the upper limit.
    ys = [600, 470, 560, 420, 200, 500, 640, 520]
    step = (right - left - 60) / (len(ys) - 1)
    pts = [(left + 30 + i * step, y) for i, y in enumerate(ys)]
    d.line(pts, fill=LINE, width=34, joint="curve")
    for i, (x, y) in enumerate(pts):
        out = y < ucl or y > lcl
        r = 50 if out else 34
        d.ellipse((x - r, y - r, x + r, y + r), fill=LIMIT if out else LINE,
                  outline=WHITE if out else NAVY, width=16 if out else 12)
    return img


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    img = draw()
    img.resize((256, 256), Image.LANCZOS).save(OUT / "icon.png")
    img.save(OUT / "icon.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    print(f"Wrote {OUT / 'icon.png'} and {OUT / 'icon.ico'}")


if __name__ == "__main__":
    main()
