#!/usr/bin/env python3
"""One tokenizer for every tool: tiktoken cl100k_base, version and BPE file pinned in repos.json (v1 card 6).

setup.sh downloads the BPE file once into $TIKTOKEN_CACHE_DIR; Counter checks its sha256 and the tiktoken version, and
fails instead of falling back to a byte estimate. Tokens are counted over a command's raw stdout, banners included,
decoded as UTF-8 with replacement, with special-token text treated as ordinary text.
"""
import hashlib
import os


def check_bpe(path, sha256):
    """Raise RuntimeError unless the file at `path` has the pinned sha256."""
    with open(path, "rb") as f:
        got = hashlib.sha256(f.read()).hexdigest()
    if got != sha256:
        raise RuntimeError(f"tokenizer BPE file {path}: sha256 {got}, pinned {sha256}")


def cached_bpe(url):
    """Where tiktoken keeps the downloaded BPE file (sha1 of the URL inside $TIKTOKEN_CACHE_DIR)."""
    d = os.environ.get("TIKTOKEN_CACHE_DIR")
    if not d:
        raise RuntimeError("TIKTOKEN_CACHE_DIR is not set; run setup.sh first")
    return os.path.join(d, hashlib.sha1(url.encode()).hexdigest())


class Counter:
    """Token counter for the pinned encoding; `pin` is repos.json pins.tokenizer."""

    def __init__(self, pin):
        try:
            import tiktoken
        except ImportError as e:
            raise RuntimeError(f"tiktoken {pin['version']} is not installed ({e}); run setup.sh, there is no fallback") from e
        from importlib import metadata
        v = metadata.version("tiktoken")
        if v != pin["version"]:
            raise RuntimeError(f"tiktoken {v} installed, {pin['version']} pinned")
        p = cached_bpe(pin["bpe_url"])
        if not os.path.exists(p):
            raise RuntimeError(f"BPE file missing at {p}; run setup.sh")
        check_bpe(p, pin["bpe_sha256"])
        self.enc = tiktoken.get_encoding(pin["name"])
        self.info = {"tokenizer": pin["name"], "tiktoken": v, "bpe_sha256": pin["bpe_sha256"]}

    def count(self, data):
        """Tokens in bytes or str."""
        s = data.decode("utf-8", "replace") if isinstance(data, bytes) else data
        return len(self.enc.encode(s, disallowed_special=()))
