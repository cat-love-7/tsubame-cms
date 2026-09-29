#!/usr/bin/env bash
#
# Dismiss the dependency alerts this project has already decided about, with the reason it decided,
# and leave every other one for a person.
#
# The Security tab of a public repository is read by strangers, and twenty alerts that sit open with
# no explanation say less than four dismissed ones whose reason somebody can check. The reasoning is
# in `SECURITY.md` ("Dependency alerts that stay open") and in `backend/Cargo.toml`, where the
# `legacy-client` choice that brings two of them is written down.
#
# Two rules stop this from being a way to make the tab quiet:
#
# - Only the manifests and packages listed below are dismissed, and a critical alert is never
#   dismissed automatically. Anything unrecognised is printed and left alone - that is the alert
#   that wants a fix, a dismissal of its own, or a line in `SECURITY.md`.
# - Every dismissal carries a reason and a comment, which is what an auditor reads.
#
# Needs an authenticated `gh` whose account may write security events (the repository owner can).
# The alerts API is not public, so without that this stops rather than guessing.
#
# Usage: scripts/dismiss-dependency-alerts.sh [--dry-run]
set -euo pipefail

repo="${REPO:-cat-love-7/tsubame-cms}"
dry_run=0
if [ "${1:-}" = "--dry-run" ]; then
  dry_run=1
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "gh is not authenticated: 'gh auth login' first (the alerts API is not public)." >&2
  exit 1
fi

alerts="$(
  gh api --paginate "/repos/$repo/dependabot/alerts?state=open&per_page=100" --jq \
    '.[] | [.number, .dependency.manifest_path, .dependency.package.name, .security_advisory.severity, .security_advisory.ghsa_id] | @tsv'
)"

dismissed=0
left=0
while IFS=$'\t' read -r number manifest package severity ghsa; do
  [ -n "$number" ] || continue

  reason=""
  comment=""
  case "$manifest" in
    backend/Cargo.lock)
      case "$package" in
        rustls-webpki | h2)
          reason="tolerable_risk"
          comment="AWS SDK legacy-client stack: no version the SDK allows has the fix, and the default client would need CMake in the Lambda build (see SECURITY.md and backend/Cargo.toml)."
          ;;
        rsa)
          reason="not_used"
          comment="Used by jsonwebtoken to verify Cognito RS256 tokens; the advisory is about private-key decryption, and this process holds no RSA private key (see SECURITY.md)."
          ;;
      esac
      ;;
    packages/gatsby-source-tsubame/package-lock.json)
      reason="tolerable_risk"
      comment="DevDependency of the plugin, installed for its real gatsby build test only; the published package has no dependencies (see SECURITY.md)."
      ;;
  esac

  # A critical alert is a person's decision, whatever the manifest says.
  if [ "$severity" = "critical" ] || [ -z "$reason" ]; then
    echo "left alone: $package $ghsa ($severity, $manifest)"
    left=$((left + 1))
    continue
  fi

  if [ "$dry_run" = 1 ]; then
    echo "would dismiss: $package $ghsa ($severity) as $reason"
  else
    gh api --method PATCH "/repos/$repo/dependabot/alerts/$number" \
      -f state=dismissed -f dismissed_reason="$reason" -f dismissed_comment="$comment" >/dev/null
    echo "dismissed: $package $ghsa ($severity) as $reason"
  fi
  dismissed=$((dismissed + 1))
done <<< "$alerts"

echo
echo "$dismissed dismissed, $left left for a person"
