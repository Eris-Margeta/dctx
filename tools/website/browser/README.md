# Browser checks (optional)

Headless Chromium checks used to verify the site; they need Playwright (`pip install playwright==1.48.0`
and `playwright install chromium`), which the build does not. Serve the site and the playground locally:

```sh
tools/playground/dist/bin/dctx-play --listen 127.0.0.1:9180 --samples tools/playground/dist/samples \
  --dctx "$PWD/tools/playground/dist/bin/dctx" --work /tmp/dctx-play --nproc 0 &
python3 tools/website/browser/devserver.py website 9190 9180 &     # static files + /api/ proxy, like nginx.conf
python tools/website/browser/e2e.py http://127.0.0.1:9190          # 27 checks: hero, terminal, motion, no-JS, playground, search
python tools/website/browser/measure.py http://127.0.0.1:9190/     # requests, bytes, FCP, LCP, CLS
python tools/website/browser/density.py                            # data-bearing items above the fold at 1440x900
python tools/website/browser/slowfont.py                           # CLS with web fonts delayed by 0.8 s
```

`e2e.py offline` checks the playground page with the service stopped.
