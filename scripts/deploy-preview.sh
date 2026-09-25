#!/usr/bin/env bash
#
# Put the built preview site where the preview distribution serves it from: the `preview/` prefix of
# the app's bucket (`infra/preview.tf`). The site's bytes are a build artifact, so `aws s3 sync` owns
# them, exactly as `scripts/deploy-frontend.sh` owns the app's.
#
# The preview site itself is not in this repository. A deployment's own site builds it in a preview
# mode - no source plugin, client-only routes, see `packages/gatsby-source-tsubame/README.md` §6 and
# `doc/preview-site.md` §8 - and this script only delivers the result. That is also why it does not
# build: there is nothing here to run a site's build with.
#
# Usage: scripts/deploy-preview.sh --dist <dir>
#        TSUBAME_PREVIEW_DIST=<dir> scripts/deploy-preview.sh
#
# Needs: AWS credentials that may write to the bucket and invalidate the preview distribution, and
# the deployment applied (the bucket, distribution and site URL are read from its state).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
infra="$root/infra"

dist=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dist)
      dist="${2:?--dist needs the directory the preview site was built into}"
      shift 2
      ;;
    *)
      echo "usage: scripts/deploy-preview.sh --dist <dir>" >&2
      exit 2
      ;;
  esac
done
dist="${dist:-${TSUBAME_PREVIEW_DIST:-}}"

if [ -z "$dist" ]; then
  echo "no build to deploy: pass --dist <dir> or set TSUBAME_PREVIEW_DIST." >&2
  echo "The preview site is built by the deployment's own site, not by this repository." >&2
  exit 1
fi
if [ ! -d "$dist" ]; then
  echo "no build at $dist." >&2
  exit 1
fi

# From the state rather than from arguments: the bucket has a generated name, and asking the operator
# to paste an id is how a deployment ends up in the wrong account's bucket.
bucket="$(terraform -chdir="$infra" output -raw frontend_bucket)"
distribution="$(terraform -chdir="$infra" output -raw preview_distribution_id)"
preview_url="$(terraform -chdir="$infra" output -raw preview_site_url)"

# `--delete` is scoped to `preview/`, so the app's own objects are not this deployment's business.
echo "== syncing to s3://$bucket/preview =="
# The bundles name their own contents, so a file that changes gets a new name and the old one can be
# kept for a year: `immutable` is what makes a returning reader skip even a conditional request.
aws s3 sync "$dist" "s3://$bucket/preview" --delete \
  --exclude "*" --include "*.js" --include "*.css" --include "*.woff" --include "*.woff2" \
  --cache-control "public, max-age=31536000, immutable"

# Everything else keeps its name across deployments - `index.html`, the notices - so it is
# revalidated on every request: a preview deployment must not have to wait for a TTL. The
# distribution's cache policy has `min_ttl = 0` for exactly this (`infra/preview.tf`).
aws s3 sync "$dist" "s3://$bucket/preview" \
  --exclude "*.js" --exclude "*.css" --exclude "*.woff" --exclude "*.woff2" \
  --cache-control "no-cache"

# The site is one document: every route is answered with `/index.html`, so invalidating it is what
# makes a deployment visible. `/preview/*` is not needed - the viewer-request function rewrites those
# to `/index.html` before the cache is consulted - but the preview site's own routes are served from
# the same object, so nothing else has to be named either.
echo "== invalidating =="
aws cloudfront create-invalidation \
  --distribution-id "$distribution" \
  --paths "/" "/index.html" \
  --query 'Invalidation.Status' \
  --output text

echo "deployed: $preview_url"
echo "The reviewer's link is ${preview_url}/preview/... (a signed link carries the rest)."
