#!/usr/bin/env python3
"""Check local links and GitHub heading fragments in every public Markdown file.

Run with Python and tools/website/requirements.txt installed. External URLs are
left to the website checker; code blocks and test fixtures model examples,
not public navigation links.
"""
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit

import markdown

ROOT = Path(__file__).resolve().parents[1]


class Links(HTMLParser):
    def __init__(self, source):
        super().__init__()
        self.links = []
        self.ids = set()
        self.headings = Counter()
        self.heading = None
        self.feed(markdown.markdown(source, extensions=['fenced_code', 'tables']))

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        for key in ('href', 'src'):
            if key in attrs:
                self.links.append(attrs[key])
        for key in ('id', 'name'):
            if attrs.get(key):
                self.ids.add(attrs[key])
        if re.fullmatch(r'h[1-6]', tag):
            self.heading = ''

    def handle_data(self, data):
        if self.heading is not None:
            self.heading += data

    def handle_endtag(self, tag):
        if re.fullmatch(r'h[1-6]', tag) and self.heading is not None:
            slug = re.sub(r'[^\w\- ]', '', self.heading.lower()).replace(' ', '-')
            count = self.headings[slug]
            self.headings[slug] += 1
            self.ids.add(slug + (f'-{count}' if count else ''))
            self.heading = None


def main():
    paths = subprocess.check_output(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT,
    ).decode().split('\0')
    files = sorted({ROOT / p for p in paths if p.lower().endswith(('.md', '.markdown'))
                    and not any(part in ('node_modules', '.venv') for part in Path(p).parts)
                    and not ('tests' in Path(p).parts and 'fixtures' in Path(p).parts)
                    and (ROOT / p).is_file()})
    parsed = {p: Links(p.read_text(encoding='utf-8')) for p in files}
    failures, checked = [], 0
    for source, doc in parsed.items():
        for href in doc.links:
            url = urlsplit(href)
            if url.scheme or url.netloc:
                continue
            checked += 1
            # Leading slashes in generated website Markdown are website routes.
            base = ((ROOT / 'website' if source.is_relative_to(ROOT / 'website') else ROOT)
                    if href.startswith('/') else source.parent)
            target = (base / unquote(url.path).lstrip('/')).resolve() if url.path else source
            if not target.is_relative_to(ROOT) or not target.exists():
                failures.append((source, href, 'missing target'))
                continue
            if url.fragment and target in parsed and unquote(url.fragment) not in parsed[target].ids:
                failures.append((source, href, 'missing heading/anchor'))
    for source, href, why in failures:
        print(f'{source.relative_to(ROOT)}: {href}: {why}')
    print(f'{len(files)} Markdown files; {checked} local links; {len(failures)} errors')
    return bool(failures)


if __name__ == '__main__':
    sys.exit(main())
