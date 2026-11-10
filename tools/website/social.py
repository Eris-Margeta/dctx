#!/usr/bin/env python3
"""Render and record committed social cards. Local only; see requirements-social.txt."""
from __future__ import annotations

import base64
import io
import json
import os
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

from common import ROOT, read_bytes
from og import H, W, og_png, scene_geometry
from social_manifest import KINDS, MANIFEST, card_inputs, file_record, inputs_sha256


def render_cards(routes, geo: dict) -> dict[str, bytes]:
    """Render parameter-driven Chrome frames, shared FFmpeg palette, gifsicle O3.

    PNG fallback is decoded *after* GIF optimization, so frame zero equals it
    pixel for pixel even with lossy GIF optimization. Temporary art never ships.
    """
    from playwright.sync_api import sync_playwright
    files = {}
    with tempfile.TemporaryDirectory(prefix="dctx-social-") as temp, sync_playwright() as p:
        work = Path(temp)
        for name in ("social.js", "social.css"):
            (work / name).write_bytes(read_bytes("tools/website/browser/" + name))
        (work / "index.html").write_text('<!doctype html><html><head><meta charset="utf-8">'
            '<link rel="stylesheet" href="social.css"></head><body><canvas width="1200" height="630"></canvas>'
            '<script src="social.js"></script></body></html>')
        browser = p.chromium.launch(executable_path=os.environ.get("DCTX_CHROME"),
            args=["--no-sandbox", "--force-color-profile=srgb", "--disable-gpu"])
        page = browser.new_page(viewport={"width": W, "height": H}, device_scale_factor=1)
        page.goto((work / "index.html").as_uri())
        cdp = page.context.new_cdp_session(page)
        for slug, inputs in card_inputs(routes, geo).items():
            crumb, title, desc, seed = (inputs[k] for k in ("crumb", "title", "desc", "seed"))
            base = og_png("dark", crumb, title, desc, seed=seed, geo=geo)
            (work / "base.png").write_bytes(base)
            page.evaluate("args => window.loadCard(args[0], args[1])", ["base.png?v=" + str(seed), geo])
            for frame in range(24):
                page.evaluate("t => window.renderFrame(t)", frame / 8)
                shot = cdp.send("Page.captureScreenshot", {"format": "png", "captureBeyondViewport": False, "optimizeForSpeed": True})
                (work / f"frame-{frame:02d}.png").write_bytes(base64.b64decode(shot["data"]))
            # Single palette across all 24 frames, with fixed encoder threading.
            subprocess.run(["ffmpeg", "-v", "error", "-y", "-threads", "1", "-filter_complex_threads", "1",
                "-framerate", "8", "-i", str(work / "frame-%02d.png"), "-filter_complex",
                "[0:v]split[a][b];[a]palettegen=stats_mode=full[p];[b][p]paletteuse=dither=bayer:bayer_scale=3",
                "-loop", "0", str(work / "raw.gif")], check=True, capture_output=True)
            subprocess.run(["gifsicle", "-O3", "--lossy=40", str(work / "raw.gif"), "-o", str(work / "card.gif")],
                           check=True, capture_output=True)
            prefix = f"/assets/og/{slug}"
            files[prefix + ".gif"] = (work / "card.gif").read_bytes()
            with Image.open(work / "card.gif") as gif:
                out = io.BytesIO()
                gif.copy().save(out, "PNG", optimize=True)
                files[prefix + ".png"] = out.getvalue()
            files[prefix + ".twitter.png"] = og_png("dark", crumb, title, desc, seed, geo, height=600)
            print(f"social: {slug} ({len(files[prefix + '.gif']):,} GIF bytes)", flush=True)
        browser.close()
    return files

def main() -> int:
    from pages.news_content import NEWS
    from routes import build_routes

    os.umask(0o022)
    routes = build_routes(NEWS)
    geo = scene_geometry()
    # Finish rendering before replacing any committed assets or the manifest.
    files = render_cards(routes, geo)
    manifest = {}
    for slug, inputs in card_inputs(routes, geo).items():
        entry = {"inputs_sha256": inputs_sha256(inputs)}
        for kind, suffix in KINDS:
            name = slug + suffix
            entry[kind] = file_record(name, files["/assets/og/" + name])
        manifest[slug] = entry
    folder = ROOT / "website/assets/og"
    folder.mkdir(parents=True, exist_ok=True)
    for url, content in files.items():
        (folder / Path(url).name).write_bytes(content)
    MANIFEST.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"social: wrote {len(manifest)} triplets and social-manifest.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
