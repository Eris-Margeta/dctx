#!/usr/bin/env python3
"""One-time font step: variable TTF -> static, subset WOFF2 files for the website.

    tools/website/.venv/bin/python tools/website/fonts.py presentation/video/fonts

Inputs are the Geist and JetBrains Mono variable fonts (both SIL OFL 1.1). Outputs go to
tools/website/fonts/ and are committed (four weights: fewer requests;
CSS maps 500 to 400 and 700 to 600): the website build copies them, content-hashed,
into website/assets/ and never runs this script. Needs fontTools with WOFF2 (brotli).
"""
from __future__ import annotations

import io
import shutil
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

HERE = Path(__file__).resolve().parent
OUT = HERE / "fonts"
# Latin, Latin-1, general punctuation, arrows, maths operators the docs use, box drawing,
# geometric shapes and the check mark.
UNICODES = ("U+0020-007E,U+00A0-00FF,U+0131,U+0152-0153,U+02C6,U+02DA,U+02DC,U+2000-206F,"
            "U+20AC,U+2122,U+2190-21FF,U+2212,U+2260,U+2264-2265,U+2500-257F,U+25A0-25FF,U+2713")
FEATURES = "kern,liga,calt,tnum,case,ccmp,locl,mark,mkmk"
FAMILIES = {
    "geist": ("Geist-Variable.ttf", "Geist-OFL.txt", (400, 600)),
    "jetbrains-mono": ("JetBrainsMono-Variable.ttf", "JetBrainsMono-OFL.txt", (400, 500)),
}
LICENCE_NAMES = {"geist": "OFL-Geist.txt", "jetbrains-mono": "OFL-JetBrainsMono.txt"}


def static_woff2(source: Path, weight: int) -> bytes:
    font = TTFont(source, recalcTimestamp=False)
    font = instancer.instantiateVariableFont(font, {"wght": weight}, updateFontNames=False)
    raw = io.BytesIO()
    font.save(raw)
    options = subset.Options()
    options.flavor = "woff2"
    options.layout_features = FEATURES.split(",")
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    options.notdef_outline = True
    options.hinting = False
    options.desubroutinize = True
    raw.seek(0)
    font = TTFont(raw, recalcTimestamp=False)
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=subset.parse_unicodes(UNICODES))
    subsetter.subset(font)
    font["head"].modified = font["head"].created  # no build clock in the file
    out = io.BytesIO()
    font.flavor = "woff2"
    font.save(out)
    return out.getvalue()


def main(source_dir: str) -> int:
    src = Path(source_dir)
    OUT.mkdir(exist_ok=True)
    for family, (ttf, licence, weights) in FAMILIES.items():
        for weight in weights:
            data = static_woff2(src / ttf, weight)
            (OUT / f"{family}-{weight}.woff2").write_bytes(data)
            print(f"{family}-{weight}.woff2 {len(data)} bytes")
        shutil.copyfile(src / licence, OUT / LICENCE_NAMES[family])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "presentation/video/fonts"))
