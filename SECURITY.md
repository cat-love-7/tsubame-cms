# Security policy

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private advisory form:

<https://github.com/cat-love-7/tsubame-cms/security/advisories/new>

Say what you did, what happened and what you expected: a request and its response, or the steps and
a screenshot, is worth more than a description. We will confirm the report, tell you what we make of
it, and agree with you on when the fix and the write-up go out. This is a small project without a
bounty programme; what it can promise is a prompt answer and credit in the release notes, unless you
would rather stay anonymous.

**Do not** open a draft advisory for something already public, and do not test a deployment you do
not own.

## What is in scope

This repository: the API (`backend/`, both the on-premises and the AWS adapter), the admin
interface (`frontend/`), the two npm packages (`packages/`) and the Terraform stack (`infra/`).
`docs/content-api.md` is the API contract, so a request that breaks a promise made there is a bug
worth reporting - including one that only leaks content, since that is what the contract is about.

A **deployment** is its operator's: their identity provider, IAM policies, DNS records and Terraform
state are not in this repository, and a mistake in that configuration is not a vulnerability in this
code. The same goes for content an operator chose to publish, and for accounts and roles they handed
out.

## Supported versions

The default branch, and the newest release. This is a young project (`0.x`): fixes land in the
next release rather than in backports of older ones, and the `CHANGELOG.md` entry for a release is
where they are listed.

## Dependency alerts that stay open

Dependabot reports every advisory in a lock file, whether or not the code around it can be reached,
so the count on the Security tab is not the number of things to fix. Three kinds of alert are open
here on purpose, and each is dismissed with the reason below rather than left unexplained:

- **`rustls-webpki` 0.101 and `h2` 0.3** come with `aws-smithy-http-client` under the
  `legacy-client` feature this workspace asks for on purpose: the default AWS client pulls
  `aws-lc-rs`, which needs CMake in the Lambda build. No version the SDK allows contains the fixed
  releases, so the alert is the price of a build with no C toolchain - written out in
  `backend/Cargo.toml`, above `aws-config`.
- **`rsa`** is used by `jsonwebtoken` to *verify* the RS256 ID tokens a Cognito deployment issues.
  RUSTSEC-2023-0071 is about private-key decryption, and this process never holds a private key for
  RSA: the CMS signs its own tokens with HMAC.
- **The `gatsby` tree in `packages/gatsby-source-tsubame`** is a devDependency, installed for one
  test - the real `gatsby build` in `scripts/test-gatsby-build.sh`. The plugin ships no dependencies
  at all (`dependencies: {}`), so a site that installs it gets none of this, and the advisories sit
  in Gatsby's own pinned sub-dependencies, where this repository cannot move them.

`scripts/dismiss-dependency-alerts.sh` dismisses exactly those, with those reasons, and prints
anything else it finds instead of touching it. A critical alert is never dismissed automatically.
An alert that is none of the three above is a real one: it wants a fix, a dismissal of its own, or
a line in this list.
