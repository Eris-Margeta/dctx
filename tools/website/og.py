"""OG images, the hero poster and the favicons, drawn with Pillow from the captured demo data.

One seeded geometry (scene_geometry) is shared by the animated home GIF, the static PNGs, the
in-page poster and the canvas animation (it is embedded in the home page as JSON), so every
version shows the same files, nodes, edges and page lines: the real ones from demo.json.
Deterministic: no clock, seeded LCG, no metadata chunks.
"""
from __future__ import annotations

import io
import math
import re

from PIL import Image, ImageDraw, ImageFont

from common import HERE, HOST, VERSION, demo, rec, read_bytes

W, H = 1200, 630
SS = 2  # supersampling for anti-aliased lines
THEMES = {
    "dark": {"bg": (7, 6, 15), "surface": (15, 13, 26), "ink": (239, 234, 255), "muted": (180, 174, 214), "line": (38, 32, 64),
             "brand": (139, 92, 246), "brand_text": (183, 156, 255), "signal": (34, 211, 238), "relation": (232, 121, 249),
             "warn": (251, 191, 36), "ok": (74, 222, 128), "glow": 0.42},
    "light": {"bg": (251, 250, 255), "surface": (255, 255, 255), "ink": (22, 19, 31), "muted": (91, 86, 110), "line": (228, 224, 239),
              "brand": (109, 61, 240), "brand_text": (91, 47, 214), "signal": (14, 116, 144), "relation": (162, 28, 175),
              "warn": (180, 83, 9), "ok": (21, 128, 61), "glow": 0.20},
}
_fonts: dict = {}


def font(name: str, size: int) -> ImageFont.FreeTypeFont:
    key = (name, size)
    if key not in _fonts:
        data = read_bytes(f"tools/website/fonts/{name}.woff2")
        _fonts[key] = ImageFont.truetype(io.BytesIO(data), size)
    return _fonts[key]


class LCG:
    """The 1664525/1013904223 generator of the old site's field, so seeds mean the same in JS."""

    def __init__(self, seed: int):
        self.s = seed & 0xFFFFFFFF

    def __call__(self) -> float:
        self.s = (self.s * 1664525 + 1013904223) & 0xFFFFFFFF
        return self.s / 4294967296


def _perm() -> list[int]:
    p = list(range(256))
    s = 1337
    for i in range(255, 0, -1):
        s = (s * 1664525 + 1013904223) & 0xFFFFFFFF
        j = int(s / 4294967296 * (i + 1))
        p[i], p[j] = p[j], p[i]
    return p + p


PERM = _perm()


def noise(x: float, y: float) -> float:
    def fade(t):
        return t * t * t * (t * (t * 6 - 15) + 10)

    def grad(h, gx, gy):
        return (gx + gy, -gx + gy, gx - gy, -gx - gy, gx, -gx, gy, -gy)[h & 7]
    xi, yi = math.floor(x), math.floor(y)
    x -= xi
    y -= yi
    xi &= 255
    yi &= 255
    u, v = fade(x), fade(y)
    a, b = PERM[xi] + yi, PERM[xi + 1] + yi
    lerp = lambda p, q, t: p + (q - p) * t  # noqa: E731
    return lerp(lerp(grad(PERM[a], x, y), grad(PERM[b], x - 1, y), u),
                lerp(grad(PERM[a + 1], x, y - 1), grad(PERM[b + 1], x - 1, y - 1), u), v)


def field_angle(x: float, y: float, t: float = 0.0) -> float:
    return (noise(x * .0042 + t * .03, y * .0042 - t * .02) * 3.4 + noise(x * .0013 - t * .012, y * .0013 + t * .01) * 4.6 + .6)


# ---------------------------------------------------------------- geometry from demo.json
def scene_geometry() -> dict:
    d = demo()
    rnd = LCG(20261106)
    files = []
    placed: list[tuple[float, float]] = []
    for f in d["sample"]["files"]:
        for _ in range(400):
            x, y = 70 + rnd() * 560, 70 + rnd() * 480
            if all(abs(x - px) > 150 or abs(y - py) > 30 for px, py in placed):
                break
        placed.append((x, y))
        files.append({"p": f["path"], "s": f["status"], "x": round(x, 1), "y": round(y, 1)})
    graph = __import__("json").loads(rec("export-json")["stdout"])
    nodes = graph["nodes"]
    ids = [n["id"] for n in nodes]
    modules = [n["id"] for n in nodes if n["id"].startswith("m:")]
    centre_x, centre_y = 360, 318
    pos: dict[str, tuple[float, float]] = {}
    # Module positions for the committed kiosk sample (presentation only: nodes and edges are the
    # captured ones). A module missing here falls back to the ring below.
    layout = {"m:src/main.rs": (360, 300), "m:src/util.rs": (150, 420), "m:src/store.rs": (560, 430),
              "m:web/app.ts": (190, 150), "m:web/format.ts": (450, 95), "m:tools/report.py": (650, 255),
              "m:README.md": (70, 260), "m:Cargo.toml": (690, 110), "m:config.toml": (360, 545),
              "m:.gitignore": (80, 560), "m:assets/logo.png": (620, 585)}
    # Angle shares grow with the number of symbols a module holds, so busy files get room.
    weight = {m: 1 + 0.7 * sum(1 for i in ids if not i.startswith("m:") and i[2:].split("#")[0] == m[2:]) for m in modules}
    total, acc = sum(weight.values()), 0.0
    for m in modules:
        a = -math.pi / 2 + 2 * math.pi * (acc + weight[m] / 2) / total
        acc += weight[m]
        pos[m] = layout.get(m, (centre_x + math.cos(a) * 240, centre_y + math.sin(a) * 200))
    centre_x, centre_y = pos.get("m:src/main.rs", (centre_x, centre_y))
    for m in modules:
        path = m[2:]
        children = [i for i in ids if not i.startswith("m:") and i[2:].split("#")[0] == path]
        mx, my = pos[m]
        step = min(0.5, 2.4 / max(1, len(children)))
        for k, c in enumerate(children):
            out = math.atan2(my - centre_y, mx - centre_x) if (mx, my) != (centre_x, centre_y) else -math.pi / 2
            a = out + (k - (len(children) - 1) / 2) * step
            r = 44 + 16 * (k % 2)
            pos[c] = (mx + math.cos(a) * r, my + math.sin(a) * r)
    out_nodes = []
    for n in nodes:
        x, y = pos.get(n["id"], (centre_x, centre_y))
        out_nodes.append({"id": n["id"], "k": n.get("kind", ""), "x": round(x, 1), "y": round(y, 1)})
    index = {n["id"]: i for i, n in enumerate(out_nodes)}
    # calls and imports, as in `map export --format mermaid`; contains edges are the clustering
    edges = [[index[e["src"]], index[e["dst"]], e["edge"]] for e in graph["edges"]
             if e["edge"] in ("calls", "imports") and e["src"] in index and e["dst"] in index]
    tree = [[index[e["src"]], index[e["dst"]]] for e in graph["edges"]
            if e["edge"] == "contains" and e["src"] in index and e["dst"] in index]
    hit = index["s:src/util.rs#greet"]
    nbrs = sorted({b for a, b, _ in edges if a == hit} | {a for a, b, _ in edges if b == hit})
    lines = rec("pack-for")["stdout"].rstrip("\n").split("\n")
    longest = max(len(line) for line in lines)
    fx, fy, fw = 745, 112, 330
    bars = []
    current = hit
    for i, line in enumerate(lines):
        for token in re.findall(r"[smd]:[^\s]+", line):
            token = token.split(":", 2)[-1] if token.startswith(("calls:", "contains:")) else token
            if token in index:
                current = index[token]
                break
        bars.append({"w": round(max(4, len(line.expandtabs(4)) / longest * (fw - 28)), 1), "n": current,
                     "h": 1 if line.startswith("#") else 0})
    footer = d["summary"]
    return {"w": W, "h": H, "files": files, "nodes": out_nodes, "edges": edges, "tree": tree, "hit": hit, "nbrs": nbrs,
            "bars": bars, "frame": {"x": fx, "y": fy, "w": fw, "h": 28 + len(lines) * 13},
            "counts": {"disk": footer["files_on_disk"], "listed": footer["files_listed"], "read": footer["files_read"],
                       "nodes": len(out_nodes), "edges": len(edges), "body_tokens": footer["for_body_tokens"],
                       "budget": footer["for_budget"], "dropped": footer["for_dropped"]}}


# ---------------------------------------------------------------- drawing
def smooth(a: float, b: float, x: float) -> float:
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def mix(c1, c2, t):
    return tuple(round(a + (b - a) * t) for a, b in zip(c1, c2))


def background(theme: dict, seed: int, field_strokes: int = 120) -> Image.Image:
    img = Image.new("RGB", (W * SS, H * SS), theme["bg"])
    rnd = LCG(seed)
    gradient = Image.radial_gradient("L").resize((512, 512))
    for _ in range(3):
        cx, cy, r = (0.45 + rnd() * 0.55) * W, rnd() * H, (260 + rnd() * 220)
        size = int(r * 2 * SS)
        # radial_gradient is 0 at the centre and 181 on the inscribed circle
        mask = gradient.resize((size, size)).point(lambda v: int(max(0.0, 1 - v / 181) ** 2 * 255 * theme["glow"]))
        layer = Image.new("RGB", (size, size), theme["brand"])
        img.paste(layer, (int((cx - r) * SS), int((cy - r) * SS)), mask)
    draw = ImageDraw.Draw(img)
    for _ in range(field_strokes):
        x, y = rnd() * W, rnd() * H
        pts = []
        px, py = x, y
        for _ in range(5):
            pts.append((px * SS, py * SS))
            a = field_angle(px, py)
            px, py = px + math.cos(a) * 5, py + math.sin(a) * 5
        draw.line(pts, fill=mix(theme["bg"], theme["muted"], 0.28), width=SS)
    return img


def draw_scene(img: Image.Image, geo: dict, theme: dict, t: float, box=(0, 0, W, H)) -> None:
    """The hero scene at time t (seconds, 0..12; >= 12 is the final frame) into box (x, y, w, h)."""
    bx, by, bw, bh = box
    sx, sy = bw / geo["w"], bh / geo["h"]
    s = min(sx, sy)
    P = lambda x, y: ((bx + x * sx) * SS, (by + y * sy) * SS)  # noqa: E731
    draw = ImageDraw.Draw(img)
    mono = font("jetbrains-mono-500", max(9, round(13 * s)) * SS)
    small = font("jetbrains-mono-400", max(8, round(11 * s)) * SS)
    p_policy, p_graph, p_page = smooth(3, 5, t), smooth(5, 7.5, t), smooth(9, 11.5, t)
    p_query = smooth(7, 8, t)
    nodes = geo["nodes"]
    module_of = {n["id"][2:]: i for i, n in enumerate(nodes) if n["id"].startswith("m:")}
    # containment (module -> symbol), faint
    if p_graph > 0:
        for a, b in geo["tree"]:
            na, nb = nodes[a], nodes[b]
            draw.line([P(na["x"], na["y"]), P(nb["x"], nb["y"])], fill=mix(theme["bg"], theme["line"], p_graph), width=SS)
    # edges
    if p_graph > 0:
        for k, (a, b, kind) in enumerate(geo["edges"]):
            reveal = smooth(5 + k * 0.15, 6.2 + k * 0.15, t)
            if reveal <= 0:
                continue
            na, nb = nodes[a], nodes[b]
            hot = p_query > 0 and geo["hit"] in (a, b)
            col = theme["signal"] if hot else (theme["relation"] if kind == "calls" else theme["muted"])
            x0, y0 = P(na["x"], na["y"])
            x1, y1 = P(nb["x"], nb["y"])
            draw.line([(x0, y0), (x0 + (x1 - x0) * reveal, y0 + (y1 - y0) * reveal)], fill=mix(theme["bg"], col, 0.75), width=(2 if hot else 1) * SS)
    # nodes
    if p_graph > 0:
        for i, n in enumerate(nodes):
            x, y = P(n["x"], n["y"])
            r = (5 if n["id"].startswith("m:") else 3.2) * s * SS * p_graph
            col = theme["muted"]
            if n["id"].startswith("s:") and n["id"][2:].split("#")[0].startswith(("src/", "web/", "tools/")):
                col = theme["brand"]
            if p_query > 0 and i == geo["hit"]:
                col, r = theme["signal"], r * (1 + 0.6 * p_query)
            elif p_query > 0 and i in geo["nbrs"]:
                col = mix(col, theme["signal"], p_query * 0.7)
            draw.ellipse([x - r, y - r, x + r, y + r], fill=mix(theme["bg"], col, 0.95))
            if p_query > 0 and i in geo["nbrs"] and n["id"].startswith("s:"):
                draw.text((x + 6 * SS * s, y - 7 * SS * s), n["id"].split("#")[-1], font=small,
                          fill=mix(theme["bg"], theme["signal"], p_query * 0.85))
        if p_query > 0:
            n = nodes[geo["hit"]]
            x, y = P(n["x"], n["y"])
            draw.text((x - 6 * SS * s, y + 12 * SS * s), "find greet → " + n["id"], font=small, fill=mix(theme["bg"], theme["signal"], p_query))
    # file labels: visible in phases 1-2, fold into their module node in phase 3
    for k, f in enumerate(geo["files"]):
        appear = smooth(0.2 + k * 0.18, 0.8 + k * 0.18, t)
        if appear <= 0:
            continue
        x, y = f["x"], f["y"]
        alpha = appear
        col = theme["ink"]
        label = f["p"]
        if f["s"] in ("ignored", "binary"):
            col = mix(theme["ink"], theme["warn"], p_policy)
            a = field_angle(x, y)
            x, y = x + math.cos(a) * 120 * p_policy, y + math.sin(a) * 120 * p_policy
            alpha *= 1 - p_policy
            label += ("  · ignored" if f["s"] == "ignored" else "  · binary") if p_policy > 0.05 else ""
        elif f["s"] == "listed-not-read":
            col = mix(theme["ink"], theme["warn"], p_policy)
            if p_policy > 0.05:
                label += "  · name only, never read"
            # out of the graph's way: the file has no node, it is listed by name only
            x, y = x + (24 - x) * p_graph, y + (geo["h"] - 52 - y) * p_graph
        if p_graph > 0 and f["p"] in module_of:
            m = nodes[module_of[f["p"]]]
            x, y = x + (m["x"] - x) * p_graph, y + (m["y"] - y) * p_graph
            alpha *= 1 - 0.55 * p_graph
        if alpha <= 0.02:
            continue
        px, py = P(x, y)
        draw.text((px + 7 * SS * s, py - 8 * SS * s), label, font=mono if p_graph < 0.5 else small, fill=mix(theme["bg"], col, alpha))
    # bounded page
    if p_page > 0:
        fr = geo["frame"]
        x0, y0 = P(fr["x"], fr["y"])
        x1, y1 = P(fr["x"] + fr["w"], fr["y"] + fr["h"])
        frame_col = mix(theme["bg"], theme["brand"], p_page)
        for xx in range(int(x0), int(x1), 9 * SS):
            draw.line([(xx, y0), (min(xx + 5 * SS, x1), y0)], fill=frame_col, width=SS)
            draw.line([(xx, y1), (min(xx + 5 * SS, x1), y1)], fill=frame_col, width=SS)
        for yy in range(int(y0), int(y1), 9 * SS):
            draw.line([(x0, yy), (x0, min(yy + 5 * SS, y1))], fill=frame_col, width=SS)
            draw.line([(x1, yy), (x1, min(yy + 5 * SS, y1))], fill=frame_col, width=SS)
        draw.text((x0, y0 - 20 * SS * s), "≤ BUDGET · STABLE ORDER", font=small, fill=mix(theme["bg"], theme["brand_text"], p_page))
        for i, bar in enumerate(geo["bars"]):
            e = smooth(9 + i * 0.07, 10.2 + i * 0.07, t)
            if e <= 0:
                continue
            src = nodes[bar["n"]]
            tx, ty = fr["x"] + 14, fr["y"] + 16 + i * 13
            x, y = src["x"] + (tx - src["x"]) * e, src["y"] + (ty - src["y"]) * e
            w = 6 + (bar["w"] - 6) * e
            col = theme["muted"] if bar["h"] else (theme["signal"] if bar["n"] == geo["hit"] else theme["brand"])
            ax, ay = P(x, y)
            bx2, by2 = P(x + w, y + 6)
            draw.rounded_rectangle([ax, ay, bx2, by2], radius=2 * SS, fill=mix(theme["bg"], col, 0.9))
        c = geo["counts"]
        tokens = round(c["body_tokens"] * smooth(9.4, 11.6, t))
        draw.text((x0, y1 + 10 * SS * s), f"body_tokens={tokens} budget={c['budget']} dropped={c['dropped']}", font=small,
                  fill=mix(theme["bg"], theme["muted"], p_page))


def caption(geo: dict, t: float) -> str:
    c = geo["counts"]
    if t < 3:
        return f"1 repository · {c['disk']} files"
    if t < 5:
        return f"2 policy · {c['disk']} → {c['listed']} listed · {c['read']} read"
    if t < 9:
        return f"3 graph · {c['nodes']} nodes · {c['edges']} call and import edges"
    return f"4 bounded page · {c['body_tokens']} of {c['budget']} tokens"


def finish(img: Image.Image) -> Image.Image:
    return img.resize((W, H), Image.LANCZOS)


def png_bytes(img: Image.Image) -> bytes:
    out = io.BytesIO()
    img.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.FLOYDSTEINBERG).save(out, "PNG", optimize=True)
    return out.getvalue()


def text_block(img: Image.Image, theme: dict, crumb: str, title: str, desc: str, width: int) -> None:
    draw = ImageDraw.Draw(img)
    draw.text((64 * SS, 54 * SS), HOST + (f"  /  {crumb}" if crumb else ""), font=font("jetbrains-mono-400", 22 * SS),
              fill=theme["muted"])
    size = 56
    for size in (56, 50, 44):
        lines = wrap(title, font("geist-600", size * SS), width * SS)
        if len(lines) <= 2 or size == 44:
            break
    lines = lines[:3]
    y = 130
    for line in lines:
        draw.text((64 * SS, y * SS), line, font=font("geist-600", size * SS), fill=theme["ink"])
        y += round(size * 1.15)
    y += 14
    for line in wrap(desc, font("geist-400", 28 * SS), width * SS)[:3]:
        draw.text((64 * SS, y * SS), line, font=font("geist-400", 28 * SS), fill=theme["muted"])
        y += 38
    mark(draw, 64, 540, 40, theme)
    draw.text((118 * SS, 548 * SS), f"dctx {VERSION}", font=font("jetbrains-mono-500", 22 * SS), fill=theme["ink"])
    tejl = font("geist-600", 26 * SS)
    tw = draw.textlength("TEJL", font=tejl)
    draw.text((W * SS - 64 * SS - tw, 546 * SS), "TEJL", font=tejl, fill=theme["ink"])


def wrap(text: str, f: ImageFont.FreeTypeFont, width: float) -> list[str]:
    words, lines, line = text.split(), [], ""
    for word in words:
        trial = (line + " " + word).strip()
        if f.getlength(trial) <= width or not line:
            line = trial
        else:
            lines.append(line)
            line = word
    if line:
        lines.append(line)
    return lines


def mark(draw: ImageDraw.ImageDraw, x: float, y: float, size: float, theme: dict | None = None) -> None:
    """The dctx mark: rounded square, two brackets, a violet dot (the ICON geometry, 32-unit grid)."""
    k = size / 32 * SS
    X = lambda v: x * SS + v * k  # noqa: E731
    Y = lambda v: y * SS + v * k  # noqa: E731
    draw.rounded_rectangle([X(0), Y(0), X(32), Y(32)], radius=8 * k, fill=(20, 16, 31))
    w = max(1, round(2.2 * k))
    draw.line([(X(11.5), Y(9)), (X(8.5), Y(9)), (X(8.5), Y(23)), (X(11.5), Y(23))], fill=(255, 255, 255), width=w, joint="curve")
    draw.line([(X(20.5), Y(9)), (X(23.5), Y(9)), (X(23.5), Y(23)), (X(20.5), Y(23))], fill=(255, 255, 255), width=w, joint="curve")
    draw.ellipse([X(16 - 2.6), Y(16 - 2.6), X(16 + 2.6), Y(16 + 2.6)], fill=(139, 92, 246))


def og_png(theme_name: str, crumb: str, title: str, desc: str, seed: int, geo: dict | None = None) -> bytes:
    theme = THEMES[theme_name]
    img = background(theme, seed)
    if geo is not None:
        draw_scene(img, geo, theme, 13, box=(560, 70, 620, 326))
        text_block(img, theme, crumb, title, desc, 500)
    else:
        text_block(img, theme, crumb, title, desc, 1060)
    return png_bytes(finish(img))


def poster_png(theme_name: str, geo: dict) -> bytes:
    theme = THEMES[theme_name]
    img = Image.new("RGB", (W * SS, H * SS), theme["surface"])
    draw_scene(img, geo, theme, 13)
    ImageDraw.Draw(img).text((16 * SS, (H - 30) * SS), caption(geo, 13), font=font("jetbrains-mono-500", 16 * SS), fill=theme["muted"])
    return png_bytes(finish(img))


def home_gif(geo: dict, title: str, desc: str) -> bytes:
    theme = THEMES["dark"]
    base = background(theme, 7)
    text_block(base, theme, "", title, desc, 500)
    times = [13.0] + [0.6 + i * 0.5 for i in range(5)] + [3.2 + i * 0.4 for i in range(5)] + \
            [5.2 + i * 0.6 for i in range(6)] + [9.0 + i * 0.5 for i in range(6)] + [13.0]
    frames = []
    for t in times:
        img = base.copy()
        draw_scene(img, geo, theme, t, box=(560, 70, 620, 326))
        # loop progress along the bottom edge: it also keeps every frame distinct (GIF writers merge equal frames)
        k = len(frames)
        ImageDraw.Draw(img).rectangle([0, (H - 3) * SS, int(W * SS * (k + 1) / len(times)), H * SS], fill=theme["brand"])
        frames.append(finish(img))
    palette = frames[0].quantize(colors=128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    q = [f.quantize(palette=palette, dither=Image.Dither.NONE) for f in frames]
    out = io.BytesIO()
    q[0].save(out, "GIF", save_all=True, append_images=q[1:], duration=100, loop=0, optimize=False, disposal=1)
    return out.getvalue()


def icon_png(size: int) -> bytes:
    img = Image.new("RGBA", (size * SS * 4, size * SS * 4), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    k = size * 4 / 32
    mark(draw, 0, 0, size * 4)
    del k
    img = img.resize((size, size), Image.LANCZOS)
    out = io.BytesIO()
    img.save(out, "PNG", optimize=True)
    return out.getvalue()


def all_images(routes, geo: dict) -> dict[str, bytes]:
    """Published path -> bytes for every OG image, the poster and the icons."""
    files: dict[str, bytes] = {}
    for i, route in enumerate(r for r in routes if r.kind != "error"):
        slug = route.slug
        if f"/assets/og/{slug}.dark.png" in files:
            continue
        crumb = "" if route.kind == "home" else route.path.strip("/").replace("/", " / ")
        title = "Context, engineered." if route.kind == "home" else route.title
        desc = route.desc if route.kind != "home" else "Deterministic, bounded code context for agents and chat. Offline."
        geo_arg = geo if route.kind == "home" else None
        for theme in ("dark", "light"):
            files[f"/assets/og/{slug}.{theme}.png"] = og_png(theme, crumb, title, desc, seed=101 + i, geo=geo_arg)
    # Owner 07.11: the classic pre-v2 card (2093ea4f website/assets/og.png) is the home share image.
    files["/assets/og/home.png"] = (HERE / "data/og/home-classic.png").read_bytes()
    files["/assets/og/home.gif"] = home_gif(geo, "Context, engineered.", "Deterministic, bounded code context for agents and chat. Offline.")
    for theme in ("dark", "light"):
        files[f"/assets/og/scene.{theme}.png"] = poster_png(theme, geo)
    for size, name in ((32, "/favicon-32.png"), (180, "/apple-touch-icon.png"), (192, "/assets/icon-192.png"), (512, "/assets/icon-512.png")):
        files[name] = icon_png(size)
    return files


if __name__ == "__main__":
    import sys
    geo = scene_geometry()
    target = HERE / "og-preview"
    target.mkdir(exist_ok=True)
    (target / "home.dark.png").write_bytes(og_png("dark", "", "Context, engineered.", "Deterministic, bounded code context.", 101, geo))
    (target / "scene.dark.png").write_bytes(poster_png("dark", geo))
    (target / "scene.light.png").write_bytes(poster_png("light", geo))
    (target / "home.gif").write_bytes(home_gif(geo, "Context, engineered.", "Deterministic, bounded code context."))
    print("written", target, file=sys.stderr)
