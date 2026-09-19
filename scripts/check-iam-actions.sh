#!/usr/bin/env bash
#
# Check every action named in `infra/deployer-policy*.json` against AWS's own list of actions.
#
# The applying side is split across three files because IAM caps one managed policy at 6144
# characters; this checks them as one set, and insists the read-only one stays a subset of it.
#
# An action name that does not exist is not an error anyone sees when the policy is attached: IAM
# accepts it and it grants nothing, so the deployment fails later with AccessDenied on the *real*
# action - which reads like "the permission is missing" rather than "the permission is misspelt".
# (`s3:PutBucketLifecycleConfiguration` was one: the action is `s3:PutLifecycleConfiguration`.)
#
# The list comes from the policy generator's configuration, which is what the IAM console's editor
# uses, and it is fetched rather than vendored: a copy in the repository would be the thing that
# goes stale.
#
# Usage: scripts/check-iam-actions.sh [path/to/policies.js]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
source_file="${1:-}"

if [ -z "$source_file" ]; then
  source_file="$(mktemp)"
  trap 'rm -f "$source_file"' EXIT
  echo "== fetching the action list =="
  curl -fsS -o "$source_file" https://awspolicygen.s3.amazonaws.com/js/policies.js
fi

echo "== checking the policies =="
python3 - "$source_file" "$root" <<'PY'
import collections
import fnmatch
import json
import pathlib
import sys

source, root = sys.argv[1], pathlib.Path(sys.argv[2])
raw = pathlib.Path(source).read_text()
config = json.loads(raw[raw.index("=") + 1 :])

actions = collections.defaultdict(set)
for service in config["serviceMap"].values():
    actions[service["StringPrefix"]].update(service.get("Actions") or [])

def named(policy):
    """Every action in a policy, with the statement it is in."""
    for statement in policy["Statement"]:
        listed = statement["Action"]
        for action in listed if isinstance(listed, list) else [listed]:
            yield statement["Sid"], action

problems = []
policies = {}
for path in sorted((root / "infra").glob("deployer-policy*.json")):
    policies[path.name] = json.loads(path.read_text())
    for sid, action in named(policies[path.name]):
        prefix, _, name = action.partition(":")
        known = actions.get(prefix)
        if known is None:
            problems.append(f"{path.name}: {sid}: no such service prefix in {action!r}")
        elif "*" in name or "?" in name:
            if not any(fnmatch.fnmatch(k, name) for k in known):
                problems.append(f"{path.name}: {sid}: {action!r} matches no action")
        elif name not in known:
            problems.append(f"{path.name}: {sid}: {action!r} is not an action of {prefix}")

# The read-only policy has to stay a subset: an action there that the applying identity lacks is a
# plan that cannot run, or a permission nobody reviewed. "Applying" is every other policy here -
# IAM caps one managed policy at 6144 characters, so the applying side is split in three.
apply_actions = {
    action
    for name, policy in policies.items()
    if name != "deployer-policy-plan.json"
    for _, action in named(policy)
}
for sid, action in named(policies["deployer-policy-plan.json"]):
    if action not in apply_actions:
        problems.append(f"deployer-policy-plan.json: {sid}: {action!r} is not in the apply policy")

if problems:
    print("\n".join(problems))
    sys.exit(1)
print(f"ok: {len(apply_actions)} actions, in {len(policies)} policies")
PY
