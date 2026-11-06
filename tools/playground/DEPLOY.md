# Deploying the dctx website and playground

For the operator. Nothing here runs automatically. `$DEPLOY` is the SSH target of the Dokploy host
(for example `user@host`); every path on that host is under `/srv/sites/`. Run from the repository root
of the commit you deploy.

## 0. Merge and verify

This branch rewrites `tools/website/build.py` and `tools/website/check.py`. If `main` has its own commit that
only switched the canonical host to `https://www.dctx.internet.technology`, keep this branch's versions of
both files: they make the same change (`tools/website/common.py` holds `ORIGIN`).

```sh
git merge --no-ff card/DCX-WEB-V2
python3 -m venv tools/website/.venv && tools/website/.venv/bin/pip install -r tools/website/requirements.txt
cargo build --locked --release -p dctx-cli
tools/website/.venv/bin/python tools/website/build.py --refresh-dates   # only if a document changed since
tools/website/.venv/bin/python tools/website/build.py
git add -A website tools/website/data && git commit -m "website: rebuild"  # only if the two steps changed files
DCTX_BIN=target/release/dctx-cli DCTX_DENY_LIST=<publish deny-list> tools/website/.venv/bin/python tools/website/check.py
```

The last line must end with `PASS: 0 errors`. When `docs/WHY-DCTX.md` lands later, the `/why/` page, its home
link, its sidebar entry and its llms entries appear on the next build: run the three build lines again.

## 1. Build the playground image context

```sh
tools/playground/build-dist.sh        # static musl dctx + dctx-play, samples, openapi.json, Dockerfile
(cd tools/playground && cargo test --locked)
cat tools/playground/dist/VERSION     # dctx 1.1.0 (<sha>)
```

Needs the `x86_64-unknown-linux-musl` Rust target and a musl C compiler for sqlite (`musl-gcc`, or `zig`
through `tools/playground/musl-cc.sh`, which the script picks up by itself).

## 2. Upload (site, nginx config, playground context)

```sh
SHA=$(git rev-parse --short HEAD)
rsync -a --delete --chmod=D755,F644 --exclude .gitignore website/ "$DEPLOY:/srv/sites/dctx.next/"
ssh "$DEPLOY" "mkdir -p /srv/sites/dctx-nginx /srv/sites/dctx-play"
rsync -a --chmod=F644 tools/playground/nginx.conf "$DEPLOY:/srv/sites/dctx-nginx/default.conf"
rsync -a --delete tools/playground/dist/ "$DEPLOY:/srv/sites/dctx-play.next/"
ssh "$DEPLOY" "cd /srv/sites && { [ -d dctx ] && mv dctx dctx.prev-$SHA; true; } && mv dctx.next dctx \
  && { [ -d dctx-play ] && mv dctx-play dctx-play.prev; true; } && mv dctx-play.next dctx-play \
  && ls -dt dctx.prev-* | tail -n +4 | xargs -r rm -rf"
```

## 3. Dokploy

One Compose app (`dctx`) with the services in `tools/playground/compose.yaml`: paste the file, then Deploy
(or Redeploy). The domain `www.dctx.internet.technology` points to `dctx-site`, port 80; the bare host keeps
its existing 301 to www. nginx serves the site and proxies `/api/` to `dctx-play:8080`; the playground needs
no domain of its own. nginx starts and serves the site even while `dctx-play` is down.

After changing only the site files, `ssh "$DEPLOY" "docker exec <dctx-site container> nginx -s reload"` is
not needed: nginx reads the files on every request. After changing `default.conf`, redeploy or reload nginx.

## 4. Verify

```sh
curl -sI https://dctx.internet.technology/ | head -3              # 301 to https://www.dctx.internet.technology/
curl -sI https://www.dctx.internet.technology/ | grep -iE '^(HTTP|content-security-policy|strict-transport|cache-control)'
curl -s https://www.dctx.internet.technology/build.json            # version 1.1.0, source_sha256
tools/website/.venv/bin/python -c 'import json;print(json.load(open("website/build.json"))["source_sha256"])'   # the same digest
curl -sI https://www.dctx.internet.technology/assets/$(ls website/assets | grep '^site\.' ) | grep -i cache-control   # immutable
curl -sI https://www.dctx.internet.technology/llms.txt | grep -i content-type   # text/plain; charset=utf-8
curl -s https://www.dctx.internet.technology/api/v1/health         # {"ok":true,"dctx":"1.1.0",...}
tools/playground/smoke.sh https://www.dctx.internet.technology     # 16 checks: recorded bytes, files, MCP, form, 400/413/429
ssh "$DEPLOY" "docker stats --no-stream | grep dctx-play; docker image ls | grep dctx-play"   # memory under 192 MiB; image size
```

`build.json` carries a digest of every input file, not a commit hash: a committed file cannot contain the
hash of its own commit, and the public mirror rewrites commit hashes.

## 5. Rollback

```sh
ssh "$DEPLOY" "cd /srv/sites && mv dctx dctx.bad && mv \$(ls -dt dctx.prev-* | head -1) dctx"
ssh "$DEPLOY" "cd /srv/sites && mv dctx-play dctx-play.bad && mv dctx-play.prev dctx-play"   # then Redeploy in Dokploy
```

Rehearse once after the first deploy: swap to the previous tree, check `curl -sI`, swap back.

## Optional hardening

The playground container needs only inbound traffic from nginx. dctx makes no network calls (the release
check traces every verb), and the image has no shell or curl. If the Dokploy network allows it, attach
`dctx-play` to an internal-only network shared with `dctx-site` instead of `dokploy-network`.
