<!-- One change per commit, the subject in the imperative and the body saying why (CONTRIBUTING.md
     has the conventions a change is reviewed against). This template is the short version. -->

## What this changes, and why

<!--
  What was wrong, missing or expensive, and what this does about it. A measurement, a response
  body or a screenshot belongs here when it is what convinced you.
-->

## Checks

- [ ] The suites this can affect are green (`CONTRIBUTING.md` lists them by area)
- [ ] `cargo fmt --check`, and/or `npm run format:check` with `npm run lint`, for the code touched
- [ ] A reader-visible difference is in `docs/`, the README or `CHANGELOG.md`
- [ ] A bug fix comes with the test that would have caught it
- [ ] The commit is one change, and its body says why rather than restating the diff

## Anything for the reviewer

<!-- A decision you want a second opinion on, something deliberately left out, or nothing. -->
