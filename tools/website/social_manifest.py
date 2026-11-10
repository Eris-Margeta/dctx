"""Browser-free inputs and integrity checks for the committed social triplets."""
from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path

from common import HERE, ROOT, read_bytes

MANIFEST = HERE / "social-manifest.json"
KINDS = (("gif", ".gif"), ("png", ".png"), ("twitter", ".twitter.png"))


class SocialStale(ValueError):
    def __init__(self, reason: str):
        super().__init__(f"social images stale: run tools/website/social.py ({reason})")


def card_inputs(routes, geo: dict) -> dict[str, dict]:
    """Use the first route for each slug, retaining its original route-table seed."""
    cards = {}
    for i, route in enumerate(routes):
        if route.slug not in cards:
            cards[route.slug] = {
                "title": "Give AI the right code, not all of it." if route.kind == "home" else route.title,
                "crumb": "" if route.kind == "home" else route.path.strip("/").replace("/", " / "),
                "desc": route.desc,
                "seed": 101 + i,
                "geo": geo,
            }
    return cards


def art_sources() -> tuple[bytes, ...]:
    # Like the prebuilt images, og.py's source is checked here; the build_id covers
    # the shipped bytes. Keep JS/CSS in build.json's content-source digest too.
    return ((HERE / "og.py").read_bytes(),
            read_bytes("tools/website/browser/social.js"),
            read_bytes("tools/website/browser/social.css"))


def inputs_sha256(inputs: dict, sources: tuple[bytes, ...] | None = None) -> str:
    """Canonical copy/seed/geometry followed by length-framed raw art source bytes.

    Only title, crumb, desc, seed, geo, og.py, social.js and social.css enter this
    digest. Length prefixes prevent ambiguous boundaries between the inputs.
    """
    digest = hashlib.sha256()
    parts = (json.dumps(inputs, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8"),
             *(art_sources() if sources is None else sources))
    for part in parts:
        digest.update(len(part).to_bytes(8, "big"))
        digest.update(part)
    return digest.hexdigest()


def file_record(name: str, content: bytes) -> dict:
    return {"file": name, "sha256": hashlib.sha256(content).hexdigest(), "bytes": len(content)}


def load_cards(routes, geo: dict, *, manifest_path: Path = MANIFEST,
               folder: Path | None = None) -> tuple[dict, dict[str, bytes]]:
    """Validate the whole set before build writes anything; no rendering imports."""
    folder = ROOT / "website/assets/og" if folder is None else folder
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        cards = card_inputs(routes, geo)
        if not isinstance(manifest, dict) or set(manifest) != set(cards):
            raise SocialStale("manifest must cover every card slug exactly")
        sources = art_sources()
        files = {}
        for slug, inputs in cards.items():
            entry = manifest[slug]
            if not isinstance(entry, dict) or entry.get("inputs_sha256") != inputs_sha256(inputs, sources):
                raise SocialStale(f"{slug}: inputs digest differs")
            if not re.fullmatch(r"[a-z0-9-]+", slug):
                raise SocialStale(f"invalid card slug: {slug}")
        for slug, entry in manifest.items():
            for kind, suffix in KINDS:
                record = entry.get(kind)
                name = slug + suffix
                if not isinstance(record, dict) or record.get("file") != name:
                    raise SocialStale(f"{slug}: invalid {kind} file name")
                content = (folder / name).read_bytes()
                if record != file_record(name, content):
                    raise SocialStale(f"{name}: file sha256 or byte size differs")
                files["/assets/og/" + name] = content
        return manifest, files
    except (OSError, ValueError, TypeError, KeyError) as error:
        if isinstance(error, SocialStale):
            raise
        raise SocialStale(f"cannot read manifest or images: {error}") from error
