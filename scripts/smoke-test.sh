#!/usr/bin/env bash
#
# The smoke test for a deployment: talks HTTP to what `terraform apply` and
# `scripts/deploy-frontend.sh` left behind, and answers the one question neither of them can -
# is the thing actually serving?
#
# Everything here is something only a working deployment answers, and each one is a piece that
# can be wrong on its own: the shell coming out of S3 through CloudFront, the fallback that
# turns an app route into that shell, the two cache lifetimes `deploy-frontend.sh` sets, the
# `/api/*` behaviour reaching the function, the function answering in JSON rather than letting
# the fallback swallow it, that nothing here invites a crawler in, the definitions the delivery API
# serves for the schemas it hands out,
# and - where sign-in is Cognito's - the hosted page the deployment advertises. The API checks
# are the ones worth having: a distribution whose `/api/*` behaviour is missing looks perfectly
# healthy from the outside until something asks it a question.
#
# No AWS credentials and no browser are needed when the URL is given, so whoever applied the
# stack can hand the address to anyone and the run is the same.
#
# Usage:
#   scripts/smoke-test.sh https://cms.example.com [https://<function-url>]
#   scripts/smoke-test.sh                       # the app URL from `terraform output`
#
# The second argument is the function URL `terraform output -raw api_url` prints; with it, the
# liveness route is asked directly, past CloudFront. Without it that one check is skipped.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"

app_url="${1:-}"
api_url="${2:-}"

if [ -z "$app_url" ]; then
  # Reading an output needs the state, not the API - this is the only line that wants anything
  # from AWS, and passing the URL skips it.
  app_url="$(terraform -chdir="$root/infra" output -raw frontend_url)"
fi
app_url="${app_url%/}"
api_url="${api_url%/}"

work="$(mktemp -d /tmp/tsubame-smoke-XXXXXX)"
trap 'rm -rf "$work"' EXIT

failures=0
ok() { printf '  ok    %s\n' "$1"; }
bad() {
  printf '  FAIL  %s\n' "$1"
  failures=$((failures + 1))
}
note() { printf '  note  %s\n' "$1"; }

# Fetch into `$work/body` and `$work/headers`, and answer with the status. A connection that
# never happens is "000" rather than a bang, so every check below has one shape to compare.
fetch() {
  local url="$1" status
  shift
  status="$(curl -sS --max-time 30 -o "$work/body" -D "$work/headers" -w '%{http_code}' "$@" "$url" 2>"$work/error")" || status="000"
  printf '%s' "$status"
}

# The last occurrence: one request can carry a header more than once. A header that is not there
# is an empty answer, not a failure - `pipefail` would otherwise make asking the question fatal.
header() {
  grep -i "^$1:" "$work/headers" 2>/dev/null | tail -1 | sed 's/^[^:]*: *//' | tr -d '\r' || true
}

is_json() { printf '%s' "$1" | grep -qi 'application/json'; }

echo "smoke test: $app_url"

echo "== the app =="
status="$(fetch "$app_url/")"
content_type="$(header content-type)"
if [ "$status" = 200 ] && printf '%s' "$content_type" | grep -qi 'text/html' && grep -q '<app-root' "$work/body"; then
  ok "the shell is served as html, with <app-root>"
else
  bad "the shell is served as html, with <app-root> (status $status, content-type ${content_type:-none})"
fi

# An app route has no file behind it, so this is the CloudFront Function rewriting it to
# `/index.html` - the check that a deep link into the admin app works when it is pasted in.
status="$(fetch "$app_url/login")"
content_type="$(header content-type)"
if [ "$status" = 200 ] && grep -q '<app-root' "$work/body"; then
  ok "an app route (/login) falls back to the shell"
else
  bad "an app route (/login) falls back to the shell (status $status, content-type ${content_type:-none})"
fi

# The two lifetimes from `deploy-frontend.sh`: the bundles are named after their contents and
# may be kept for a year, the shell is not and must not be.
bundle="$(grep -o 'src="[^"]*\.js"' "$work/body" 2>/dev/null | head -1 | sed 's/^src="//;s/"$//' || true)"
if [ -z "$bundle" ]; then
  bad "the shell points at a bundle (no <script src=...js> in the html)"
elif printf '%s' "$bundle" | grep -Eq -- '-[A-Za-z0-9]{8}\.js$'; then
  case "$bundle" in
    /*) bundle_url="$app_url$bundle" ;;
    *) bundle_url="$app_url/$bundle" ;;
  esac
  status="$(fetch "$bundle_url")"
  cache_control="$(header cache-control)"
  if [ "$status" = 200 ] && printf '%s' "$cache_control" | grep -q 'max-age=31536000'; then
    ok "the bundle ($bundle) is served immutable for a year"
  else
    bad "the bundle ($bundle) is served immutable for a year (status $status, cache-control ${cache_control:-none})"
  fi
else
  # A development server serves `main.js`, which has no hash to keep: the check is about the
  # deployment's cache headers, and this is not one.
  note "the shell points at $bundle, which is not content-hashed - not a production build?"
fi

status="$(fetch "$app_url/index.html")"
cache_control="$(header cache-control)"
if [ "$status" = 200 ] && printf '%s' "$cache_control" | grep -Eq 'no-cache|max-age=0'; then
  ok "the shell is revalidated instead of cached ($cache_control)"
else
  bad "the shell is revalidated instead of cached (status $status, cache-control ${cache_control:-none})"
fi

echo "== the API behind /api =="
# Public, and the one answer that says which deployment is behind this URL at all.
status="$(fetch "$app_url/api/auth/capabilities")"
content_type="$(header content-type)"
capabilities="$(cat "$work/body" 2>/dev/null)"
if [ "$status" = 200 ] && is_json "$content_type" \
  && printf '%s' "$capabilities" | grep -q '"password_login"' \
  && printf '%s' "$capabilities" | grep -q '"image_upload"'; then
  ok "/api reaches the API (capabilities answered as JSON)"
else
  bad "/api reaches the API (capabilities answered as JSON) (status $status, content-type ${content_type:-none})"
fi

if printf '%s' "$capabilities" | grep -Eq '"password_login"[[:space:]]*:[[:space:]]*false'; then
  # Cognito signs people in here, so the endpoint that would take a password says so rather
  # than 404: that 501 is the difference between this adapter and the on-premises one.
  status="$(fetch "$app_url/api/auth/login" -X POST -H 'Content-Type: application/json' -d '{}')"
  if [ "$status" = 501 ]; then
    ok "the password endpoints refuse with 501, as the AWS deployment should"
  else
    bad "the password endpoints refuse with 501, as the AWS deployment should (status $status)"
  fi

  login_url="$(printf '%s' "$capabilities" | sed -n 's/.*"login_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
  if [ -z "$login_url" ]; then
    bad "the deployment advertises a sign-in page (capabilities has no login_url)"
  else
    # The advertised address is a base: the browser adds `redirect_uri`, the PKCE challenge and the
    # state (`core/auth/hosted-login.ts`). Asked for bare, Cognito answers "Required parameters
    # missing" - so this adds the one parameter the *deployment* has to have registered, which
    # makes it a check of the callback URL too: an origin the pool does not know is refused.
    status="$(fetch "$login_url" --get --data-urlencode "redirect_uri=$app_url/auth/callback")"
    content_type="$(header content-type)"
    if [ "$status" = 200 ] && printf '%s' "$content_type" | grep -qi 'text/html'; then
      ok "the advertised sign-in page answers, with $app_url/auth/callback registered"
    else
      bad "the advertised sign-in page answers, with $app_url/auth/callback registered (status $status, content-type ${content_type:-none})"
    fi
  fi
elif printf '%s' "$capabilities" | grep -Eq '"password_login"[[:space:]]*:[[:space:]]*true'; then
  note "this deployment signs people in itself (on-premises): no hosted page to check"
else
  bad "capabilities says whether the CMS handles passwords"
fi

# Where a signed preview link is opened. The deployment advertises it whether or not the preview site
# has been deployed, so this is also the check that `scripts/deploy-preview.sh` ran - an advertised
# name that answers nothing is a broken shared link, not a missing extra.
preview_site_url="$(printf '%s' "$capabilities" | sed -n 's/.*"preview_site_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
if [ -z "$preview_site_url" ]; then
  note "the deployment advertises no preview site: a shared link would stay JSON (docs/preview-site.md §8)"
else
  status="$(fetch "$preview_site_url/")"
  robots="$(header x-robots-tag)"
  if [ "$status" = 200 ] && printf '%s' "$robots" | grep -qi 'noindex'; then
    ok "the preview site answers on its own name and stays out of search ($preview_site_url)"
  else
    bad "the preview site answers on its own name and stays out of search (status $status, x-robots-tag ${robots:-none}; has scripts/deploy-preview.sh run?)"
  fi

  # The preview lives in the app's bucket under `preview/`, so the app's origin has to refuse it:
  # the app's origin holds the editor's token in localStorage and renders no CMS HTML, and the
  # preview renders a draft (`docs/preview-site.md` §7, `infra/app-routing.js`).
  status="$(fetch "$app_url/preview/index.html")"
  if [ "$status" = 404 ]; then
    ok "the app's origin refuses the preview site, which shares its bucket"
  else
    bad "the app's origin refuses the preview site, which shares its bucket (status $status; see infra/app-routing.js)"
  fi
fi

# A token is required, so this is 401 - which proves the request reached the router rather than
# the shell, and that the route exists.
status="$(fetch "$app_url/api/models/collections")"
content_type="$(header content-type)"
if [ "$status" = 401 ] && is_json "$content_type" && grep -q '"code"' "$work/body"; then
  ok "a protected route answers 401 in the refusal shape"
else
  bad "a protected route answers 401 in the refusal shape (status $status, content-type ${content_type:-none})"
fi

# The delivery API has to be readable on its own: a schema names the composite definitions it
# embeds by id, so the definitions are public too (a build has no token to spend on them).
status="$(fetch "$app_url/api/content/composite-fields")"
content_type="$(header content-type)"
if [ "$status" = 200 ] && is_json "$content_type" && grep -q '^{' "$work/body"; then
  ok "the content API serves composite definitions without a token"
else
  bad "the content API serves composite definitions without a token (status $status, content-type ${content_type:-none})"
fi

# This is an editor's screen and a site's API, not a public site, and an SPA answers 200 for every
# route it is asked about: without the header a crawler would index the shell under whatever URL it
# guessed, and `robots.txt` alone is only a request.
status="$(fetch "$app_url/robots.txt")"
if [ "$status" = 200 ] && grep -q '^Disallow: /' "$work/body"; then
  ok "robots.txt asks crawlers to stay out"
else
  bad "robots.txt asks crawlers to stay out (status $status)"
fi

status="$(fetch "$app_url/login")"
robots_tag="$(header x-robots-tag)"
if [ "$status" = 200 ] && printf '%s' "$robots_tag" | grep -qi 'noindex'; then
  ok "the app is served with X-Robots-Tag: $robots_tag"
else
  bad "the app is served with X-Robots-Tag (status $status, header ${robots_tag:-none})"
fi

# The one a distribution gets wrong: with the fallback applied to `/api/*` as well, an unknown
# API path comes back as the app's html with 200, and every client reads it as success. The
# router's own miss is an empty 404 - the refusal shape belongs to the errors the CMS raises, and
# the 401 above is one.
status="$(fetch "$app_url/api/this-path-does-not-exist")"
content_type="$(header content-type)"
if [ "$status" = 404 ] && ! grep -q '<app-root' "$work/body" && ! printf '%s' "$content_type" | grep -qi 'text/html'; then
  ok "an unknown /api path is the API's 404, not the app's html"
else
  bad "an unknown /api path is the API's 404, not the app's html (status $status, content-type ${content_type:-none})"
fi

echo "== the function, past CloudFront =="
if [ -z "$api_url" ]; then
  note "no function URL given: the liveness route was not asked (pass \`terraform output -raw api_url\`)"
else
  status="$(fetch "$api_url/")"
  if [ "$status" = 200 ] && grep -q 'Tsubame API' "$work/body"; then
    ok "the function's liveness route answers"
  else
    bad "the function's liveness route answers (status $status)"
  fi
fi

echo
if [ "$failures" -eq 0 ]; then
  echo "smoke test passed: $app_url"
else
  echo "smoke test failed: $failures check(s) against $app_url" >&2
  exit 1
fi
