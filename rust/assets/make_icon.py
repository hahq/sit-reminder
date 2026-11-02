"""生成 icon.ico：与托盘图标一致的绿色时钟。需要 Pillow，修改后运行一次即可。"""
from pathlib import Path

from PIL import Image, ImageDraw

ACCENT = (0x4C, 0xC3, 0x8A, 255)
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
SS = 8  # 超采样倍数，缩小后得到抗锯齿边缘


def draw(size: int) -> Image.Image:
    n = size * SS
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.ellipse((0, 0, n - 1, n - 1), fill=ACCENT)
    inset = n * 0.14
    d.ellipse((inset, inset, n - 1 - inset, n - 1 - inset), fill="white")
    c = n / 2
    width = max(n * 0.09, 1.6 * SS)
    for x, y in ((c, c - n * 0.27), (c + n * 0.2, c + n * 0.1)):
        d.line((c, c, x, y), fill=ACCENT, width=round(width))
        r = width / 2
        d.ellipse((x - r, y - r, x + r, y + r), fill=ACCENT)  # 圆头
    r = width * 0.75
    d.ellipse((c - r, c - r, c + r, c + r), fill=ACCENT)
    return img.resize((size, size), Image.LANCZOS)


images = [draw(s) for s in SIZES]
out = Path(__file__).with_name("icon.ico")
images[-1].save(out, sizes=[(s, s) for s in SIZES], append_images=images[:-1])
print(f"wrote {out}")
