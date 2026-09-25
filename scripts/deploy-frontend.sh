#!/usr/bin/env bash
#
# Build the admin app and put it where CloudFront serves it from: `terraform output
# frontend_bucket` and `frontend_distribution_id`.
#
# The two halves are separate on purpose. Terraform owns the bucket, the distribution and the
# policies; this owns what is *in* the bucket - a build artifact, which is what `aws s3 sync
# --delete` is for. Terraform would either have to hash every file or re-upload all of them.
#
# Usage: scripts/deploy-frontend.sh [--skip-build]
#
# Needs: npm dependencies installed in `frontend` (`npm ci`), and AWS credentials that may
# write to the bucket and invalidate the distribution.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
frontend="$root/frontend"
infra="$root/infra"

skip_build=false
if [ "${1:-}" = "--skip-build" ]; then
  skip_build=true
fi

# From the state rather than from arguments: the bucket has a generated name, and asking the
# operator to paste an id is how a deployment ends up in the wrong account's bucket.
bucket="$(terraform -chdir="$infra" output -raw frontend_bucket)"
distribution="$(terraform -chdir="$infra" output -raw frontend_distribution_id)"
app_url="$(terraform -chdir="$infra" output -raw frontend_url)"

if [ "$skip_build" = false ]; then
  echo "== building =="
  # The production configuration is the default for `ng build`; the app talks to its own origin
  # (`/api/...`), so nothing about the deployment goes into the bundle.
  (cd "$frontend" && npx ng build)
fi

# The Angular application builder writes the app into `browser/` next to its licences and
# prerender metadata; only that directory is the site.
dist="$frontend/dist/sl_cms/browser"
if [ ! -d "$dist" ]; then
  echo "no build at $dist: run without --skip-build" >&2
  exit 1
fi

echo "== syncing to s3://$bucket =="
# The bundles name their own contents (`main-3Z4V2JVF.js`), so a file that changes gets a new name
# and the old one can be kept for a year: `immutable` is what makes a returning reader skip even a
# conditional request.
#
# `--delete` is scoped to the app: the preview site lives in this same bucket under `preview/`
# (`infra/preview.tf`, `scripts/deploy-preview.sh`), and the include filters would otherwise treat
# its bundles as files this deployment removed. The last filter wins in the CLI, so the exclude goes
# after the includes.
aws s3 sync "$dist" "s3://$bucket" --delete \
  --exclude "*" --include "*.js" --include "*.css" --include "*.woff" --include "*.woff2" \
  --exclude "preview/*" \
  --cache-control "public, max-age=31536000, immutable"

# Everything else keeps its name across deployments - `index.html`, `favicon.ico`, the notices - so
# it is revalidated on every request: a deployment must not have to wait for a TTL. The
# distribution's cache policy has `min_ttl = 0` for exactly this (`infra/frontend.tf`).
aws s3 sync "$dist" "s3://$bucket" \
  --exclude "*.js" --exclude "*.css" --exclude "*.woff" --exclude "*.woff2" \
  --cache-control "no-cache"

# `index.html` is what a browser that already has the old one asks about. It is uploaded again by
# the sync above whenever the build changed it, and this is the belt to that braces: a deployment
# that changed nothing else still has to be visible.
echo "== invalidating =="
aws cloudfront create-invalidation \
  --distribution-id "$distribution" \
  --paths "/" "/index.html" \
  --query 'Invalidation.Status' \
  --output text

echo "deployed: $app_url"
