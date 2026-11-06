# DCTX-SITECHECK result

DID: added the stdlib-only isolated rebuild/integrity checker and its unittest.
Validated on 2026-11-06 UTC with Python 3.13.5 at HEAD
`bfb0f7f7949b6a28b881592dcb63cd5d2cf1d6a7`.
No commit was made, following the final task instruction.

Only `tools/website/check.py`, `tools/website/test_build.py`, and this result
were added. `website/` and `tools/website/build.py` are unchanged.

The checker copies tracked working-tree repository inputs into a temporary
directory and invokes the copied `build.py` with the running Python executable.
It seeds only the builder's static inputs (`website/assets/` and
`website/.gitignore`); old generated pages cannot conceal obsolete routes.
It compares the complete resulting file set and raw bytes against Git HEAD's
committed `website/`. Temporary files are removed automatically.

HTML, SVG, and XML href/src references are checked locally, including relative,
root-relative, same-host absolute and protocol-relative URLs. Directory targets
resolve to `index.html`; queries are ignored and paths/fragments are percent
decoded. Fragment targets include HTML IDs and legacy named anchors. Every HTML
page must have exactly one h1, a nonempty title, and one canonical link under
`https://dctx.internet.technology/`. Every JSON-LD script is parsed with
`json.loads`. Failures are listed with file/line locations where applicable and
return exit 1. No external URLs are fetched.

| Validation | Count / outcome | Exit |
| --- | --- | ---: |
| Required gate, initial and final runs | Both passed; one unittest each | 0 each |
| Generated versus committed files | 19/19 byte-identical, including 5 static assets and `.gitignore` | 0 |
| HTML page requirements | 9/9 pages | 0 |
| Internal href/src references | 322/322 resolve | 0 |
| Nonempty reference fragments | 111/111 exist | 0 |
| JSON-LD blocks | 9/9 parse | 0 |
| Document fixtures | 20/20 expected outcomes | 0 |
| Staleness fixtures | Changed bytes, missing generated file, extra generated file: 3/3 detected | 0 |
| Build failure fixture | Returns 1 and lists failure; fixture assertion passed | 0 |
| Invocation from `/tmp` | Same counts and PASS | 0 |
| Protected-path diff and whitespace checks | No changes / no errors | 0 |

Real site defects: **none found**. No site or generator edits were needed.

Exact validation commands follow. Except for the explicit `/tmp` invocation,
the working directory was `/path/to/workdir/codex/DCTX-SITECHECK`.

Required gate, run twice (exit 0 both times; checker exit 0 and unittest exit 0):

```sh
python3 tools/website/check.py && python3 -m unittest discover -s tools/website -p 'test_*.py' -q
```

Each checker invocation printed:

```text
Checked 19 files, 9 HTML pages, 322 internal href/src references, 111 fragments, 9 JSON-LD blocks.
PASS: generated website matches HEAD and all site checks passed.
```

Each unittest invocation reported `Ran 1 test` and `OK`.

Document/staleness fixtures (exit 0):

```sh
python3 - <<'PY'
import importlib.util
import tempfile
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('sitecheck', 'tools/website/check.py')
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)
page = ('<title>Fixture</title><link rel="canonical" href="' + check.ORIGIN + '/">'
        '<h1 id="ok">Fixture</h1><script type="application/ld+json">{"ok":true}</script>')
cases = [
    ('valid', page, None),
    ('missing h1', page.replace('<h1 id="ok">Fixture</h1>', ''), 'found 0'),
    ('duplicate h1', page + '<h1>Extra</h1>', 'found 2'),
    ('missing title', page.replace('<title>Fixture</title>', ''), 'missing or empty <title>'),
    ('empty title', page.replace('<title>Fixture</title>', '<title> </title>'), 'missing or empty <title>'),
    ('missing canonical', page.replace('rel="canonical"', 'rel="alternate"'), 'canonical link'),
    ('wrong canonical', page.replace(check.ORIGIN, 'https://example.com'), 'canonical link'),
    ('invalid JSON-LD', page.replace('{"ok":true}', '{broken}'), 'invalid JSON-LD'),
    ('second invalid JSON-LD', page + '<script type="application/ld+json">bad</script>', 'invalid JSON-LD'),
    ('missing href', page + '<a href="missing/">Bad</a>', 'missing file'),
    ('missing src', page + '<img src="missing.png">', 'missing file'),
    ('missing fragment', page + '<a href="#absent">Bad</a>', 'missing fragment'),
    ('cross-page fragment', page + '<a href="sub/?q=1#absent">Bad</a>', 'missing fragment'),
    ('same-site absolute', page + '<a href="' + check.ORIGIN + '/missing/">Bad</a>', 'missing file'),
    ('protocol-relative', page + '<a href="//dctx.internet.technology/missing/">Bad</a>', 'missing file'),
    ('path escape', page + '<a href="/%2e%2e/outside">Bad</a>', 'escapes website'),
    ('valid reference forms', page + '<a href="#%6fk">Good</a><a href="sub/?q=1#ok">Good</a>'
     '<a href="sub/index.html#ok">Good</a><a href="https://example.com/missing#x">External</a>'
     '<a href="mailto:test@example.com">Mail</a><img src="asset%20name.svg#icon">', None),
    ('relative base', page + '<base href="sub/"><a href="?q=1#ok">Good</a>', None),
    ('missing rel value', page + '<link rel href="#ok">', None),
    ('unclosed JSON-LD', page + '<script type="application/ld+json">{}', 'unclosed JSON-LD'),
]
with tempfile.TemporaryDirectory(prefix='dctx-sitecheck-fixtures-') as temporary:
    site = Path(temporary)
    (site / 'sub').mkdir()
    (site / 'sub/index.html').write_text(page)
    (site / 'asset name.svg').write_text('<svg><path id="icon"/></svg>')
    for name, html, expected in cases:
        (site / 'index.html').write_text(html)
        errors = []
        check.check_documents(site, errors)
        assert (not errors if expected is None else any(expected in e for e in errors)), (name, errors)
    (site / 'sub/index.html').unlink()
    (site / 'asset name.svg').unlink()
    (site / 'index.html').write_bytes(b'changed')
    (site / 'new.txt').write_bytes(b'new')
    def fake_git(*args):
        return b'website/index.html\0website/deleted.html\0' if args[0] == 'ls-tree' else b'committed'
    errors = []
    with patch.object(check, 'git', fake_git):
        assert check.check_staleness(site, errors) == 2
    assert len(errors) == 3, errors
    for expected in ('no longer generated', 'not committed', 'differs from the committed bytes'):
        assert any(expected in e for e in errors), errors
print(f'PASS: {len(cases)} document fixtures and 3 staleness failure cases')
PY
```

Output: `PASS: 20 document fixtures and 3 staleness failure cases`.

Build failure handling (exit 0 for the assertions; checker returns 1):

```sh
python3 - <<'PY'
import contextlib
import importlib.util
import io
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('sitecheck', 'tools/website/check.py')
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)
output = io.StringIO()
with patch.object(check, 'rebuild', side_effect=RuntimeError('fixture build failure')):
    with contextlib.redirect_stderr(output):
        assert check.main() == 1
assert 'FAIL: 1 failure(s)' in output.getvalue()
assert 'fixture build failure' in output.getvalue()
print('PASS: failed build returns 1 and lists the failure')
PY
```

Output: `PASS: failed build returns 1 and lists the failure`.

Working-directory independence (exit 0):

```sh
cd /tmp && python3 /path/to/workdir/codex/DCTX-SITECHECK/tools/website/check.py
```

Scope and whitespace audit (exit 0; two added Python files at that point):

```sh
git diff --exit-code HEAD -- website tools/website/build.py && git diff --check && git status --short --untracked-files=all
```

Final audit after writing this result (exit 0; exactly the three owned files):

```sh
git diff --exit-code HEAD -- website tools/website/build.py && git diff --check && git status --short --untracked-files=all
python3 - <<'PY'
import subprocess
from pathlib import Path

owned = [
    'tools/website/check.py',
    'tools/website/test_build.py',
    'docs/cards/DCTX-SITECHECK-RESULT.md',
]
status = subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=all'], text=True)
assert sorted(line[3:] for line in status.splitlines()) == sorted(owned), status
for name in owned:
    for number, line in enumerate(Path(name).read_text().splitlines(), 1):
        assert line == line.rstrip(), (name, number)
print('PASS: exactly 3 owned files; no trailing whitespace')
PY
```
