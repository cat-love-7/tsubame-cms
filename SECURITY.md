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

The default branch, and the newest release. This is a young project (`0.1.x`): fixes land in the
next release rather than in backports of older ones, and the `CHANGELOG.md` entry for a release is
where they are listed.
