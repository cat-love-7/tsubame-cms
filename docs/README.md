# The design documents

The reasoning behind the code, rather than a summary of it: what was decided, what was rejected, and
what was measured. They are in English, like the code and its comments; the conventions in short
form are `.github/copilot-instructions.md`, and the front door is [`../README.md`](../README.md).

| Document | What it answers |
|---|---|
| [`content-api.md`](content-api.md) | the whole public API: routes, query parameters, drafts and publishing, preview links, images |
| [`relations-design.md`](relations-design.md) | items referring to items: the picker, `inverse_name`, the referencing panel, delivery, the publish check |
| [`frontend-design.md`](frontend-design.md) | the shared vocabulary the screens are built from, and the boundaries between them |
| [`i18n.md`](i18n.md) | Transloco, language resolution, the switch, and the project's term list |
| [`preview-site.md`](preview-site.md) | the contract between the API and a site that renders a signed preview link |
| [`aws-decisions.md`](aws-decisions.md) | why the AWS deployment is shaped the way it is |
| [`aws-dynamodb-design.md`](aws-dynamodb-design.md) | the one-table DynamoDB layout behind the same domain |
| [`../infra/README.md`](../infra/README.md) | the Terraform stack, and what has actually been deployed |

## How to read them

- **Source comments cite these documents by section** (`docs/aws-decisions.md` §4,
  `docs/relations-design.md` §5). Section numbers are part of the interface: renumbering one means
  grepping the repository for it, not just editing the document.
- **`i18n.md` §2 is the term list.** Where a document names a concept - Collection, Single page,
  Composite field, Item, Draft, Preview link - it uses the term from that table, and so does the UI.
- **The `aws-*` pair is a record, not a plan.** Both were written before the deployment and updated
  after it ran; the `[x]` marks say "decided, built, and checked this way", and the notes beside
  them are how it was checked.
- **Each document opens with its status.** "Implemented (2026-09)" means the code matches it; a
  section that is still ahead of the code says so itself.
