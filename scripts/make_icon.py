#!/usr/bin/env python3
"""Genera le icone di Grabbit (PNG, ICO, ICNS) con Pillow.

Uso:  python3 scripts/make_icon.py
Output: assets/icon.png  assets/icon.ico  assets/icon.icns
"""
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

S = 2048          # risoluzione di lavoro (poi ridotta: antialiasing di qualità)
OUT = Path(__file__).resolve().parent.parent / "assets"
OUT.mkdir(exist_ok=True)


def gradient(size, c1, c2):
    """Gradiente diagonale c1 (alto-sinistra) -> c2 (basso-destra)."""
    base = Image.new("RGB", (size, size))
    px = base.load()
    for y in range(size):
        for x in range(size):
            t = (x + y) / (2 * (size - 1))
            px[x, y] = tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(3))
    return base


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


# --- sfondo: quadrato arrotondato con gradiente corallo -> viola ------------
# (il gradiente si calcola a bassa risoluzione e si ingrandisce: è liscio)
small = gradient(256, (255, 94, 98), (112, 52, 235))
bg = small.resize((S, S), Image.BICUBIC).convert("RGBA")

mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle(
    (int(S * 0.04), int(S * 0.04), int(S * 0.96), int(S * 0.96)),
    radius=int(S * 0.225), fill=255)

icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
icon.paste(bg, (0, 0), mask)

# --- riflesso lucido in alto -------------------------------------------------
gloss = Image.new("RGBA", (S, S), (0, 0, 0, 0))
gd = ImageDraw.Draw(gloss)
gd.ellipse((-S * 0.25, -S * 0.78, S * 1.25, S * 0.50), fill=(255, 255, 255, 34))
gloss = gloss.filter(ImageFilter.GaussianBlur(S * 0.006))
gloss = ImageChops.multiply(gloss, Image.merge("RGBA", (mask, mask, mask, mask)))
icon = Image.alpha_composite(icon, gloss)

# --- simbolo: freccia di download la cui punta è un "play" -------------------
sym = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(sym)
W = (255, 255, 255, 255)
cx = S // 2

# gambo
d.rounded_rectangle((cx - S * 0.075, S * 0.215, cx + S * 0.075, S * 0.52),
                    radius=int(S * 0.04), fill=W)
# punta: triangolo "play" ruotato verso il basso, con angoli stondati
tri = [(cx - S * 0.225, S * 0.465), (cx + S * 0.225, S * 0.465), (cx, S * 0.715)]
tmask = Image.new("L", (S, S), 0)
ImageDraw.Draw(tmask).polygon(tri, fill=255)
# blur + soglia = vertici arrotondati senza sporgenze
tmask = tmask.filter(ImageFilter.GaussianBlur(S * 0.022)).point(
    lambda v: 255 if v > 150 else 0).filter(ImageFilter.GaussianBlur(2))
sym.paste(W, (0, 0), tmask)
d = ImageDraw.Draw(sym)
# vassoio
d.rounded_rectangle((S * 0.255, S * 0.735, S * 0.745, S * 0.80),
                    radius=int(S * 0.032), fill=W)
d.rounded_rectangle((S * 0.255, S * 0.675, S * 0.315, S * 0.80),
                    radius=int(S * 0.030), fill=W)
d.rounded_rectangle((S * 0.685, S * 0.675, S * 0.745, S * 0.80),
                    radius=int(S * 0.030), fill=W)

# ombra morbida sotto il simbolo
shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
shadow.paste((50, 10, 90, 120), (0, int(S * 0.012)), sym.split()[3])
shadow = shadow.filter(ImageFilter.GaussianBlur(S * 0.018))
icon = Image.alpha_composite(icon, shadow)
icon = Image.alpha_composite(icon, sym)

# --- export -----------------------------------------------------------------
png512 = icon.resize((512, 512), Image.LANCZOS)
png512.save(OUT / "icon.png")

sizes = [16, 24, 32, 48, 64, 128, 256]
ico_src = icon.resize((256, 256), Image.LANCZOS)
ico_src.save(OUT / "icon.ico", sizes=[(s, s) for s in sizes])

icon.resize((1024, 1024), Image.LANCZOS).save(OUT / "icon.icns")
print("Icone create in", OUT)
