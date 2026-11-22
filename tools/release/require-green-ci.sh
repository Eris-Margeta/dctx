#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Refuse (exit 1) unless the full CI workflow for <sha> finished green: the newest push-event CI run
# for that commit must be completed/success and every one of its jobs success (none failed/skipped/
# cancelled). Used by release.yml and the maintainer publish scripts. Needs gh + GH_TOKEN.
# Usage: require-green-ci.sh <sha> [owner/repo]
set -euo pipefail
sha=${1:?usage: require-green-ci.sh <sha> [owner/repo]}
repo=${2:-${GITHUB_REPOSITORY:-Eris-Margeta/dctx}}
run=$(gh api "repos/$repo/actions/workflows/ci.yml/runs?head_sha=$sha&event=push&per_page=20" \
  -q '[.workflow_runs[]|select(.head_sha=="'"$sha"'")]|sort_by(.created_at)|last|[.id,.status,.conclusion]|@tsv')
if [ -z "$run" ] || [ "$run" = $'\t\t' ]; then echo "REFUSED: no CI run found for $sha" >&2; exit 1; fi
IFS=$'\t' read -r id status concl <<<"$run"
if [ "$status" != completed ] || [ "$concl" != success ]; then
  echo "REFUSED: CI run $id for $sha is $status/$concl, not completed/success" >&2; exit 1
fi
bad=$(gh api --paginate "repos/$repo/actions/runs/$id/jobs?per_page=100" \
  -q '.jobs[]|select(.conclusion!="success")|"\(.name)=\(.conclusion)"')
if [ -n "$bad" ]; then echo "REFUSED: CI run $id has non-green jobs: $bad" >&2; exit 1; fi
echo "CI green for $sha (run $id)"
