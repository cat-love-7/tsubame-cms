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
- **The only Japanese left is quoted UI wording or sample data.** `i18n.md` §2 records the English
  term beside the wording the Japanese UI shows - that table is the point of the document. Two
  READMEs outside `docs/` keep one string each for the same kind of reason: the Japanese notice text
  an E2E check looks for, and a collection name that shows why a CMS name has to be rewritten.
- **A status line means the document still describes the code.** Three of them open with
  "Status: implemented (2026-09)"; the AWS pair records what was verified and how, and "Remaining
  work" at the end of `aws-decisions.md` §2 is the honest list of what it does not cover yet. The
  two contracts carry no status line because every section states its own.
