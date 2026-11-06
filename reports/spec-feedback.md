# Specification feedback

| Issue | Summary | Specification / cases | Status |
|---|---|---|---|
| [#1](https://github.com/law-of-japan/loj-spec/issues/1) | Add positive L0 and negative diagnostic byte-span coverage; clarify L0 success expectations | SRC-01, VAL-01, CONF-01/02, expected.schema.json; all 11 L0 cases, T042 and negative diagnostic cases | Open; enhancement; no official case blocked |
| [#2](https://github.com/law-of-japan/loj-spec/issues/2) | Clarify subsequent appendix file starts and whether EOF termination completes fenced/HTML blocks at splits | MAN-03, MD-01/03, HEAD-03, both grammars; T052 does not cover multi-file appendices | Open; specification decision pending; local multi-file appendix L1 returns Incomplete; no official case blocked |

Checked all existing issues before creation (none existed). Used the existing `enhancement`
label; no labels or organization settings were created. The complete submitted body is
[spec-issue-conformance-coverage.md](spec-issue-conformance-coverage.md).

For #2, searched all open and closed issues via `gh issue list --state all --limit 100`
on 2026-11-06 JST; only #1 existed, with no duplicate. Created #2 with the user's explicit
authorization. Submitted text: [spec-issue-man03-appendix-splitting.md](spec-issue-man03-appendix-splitting.md).
The initial sandbox network attempt failed; the authorized outside-sandbox request succeeded.

No contradictions requiring a fixture exception were found in the 74 formal cases.
No pending unregistered issue drafts remain. The original specification, schemas and fixtures
were not edited. The EOF byte-boundary bug found by additional tests was a validator
implementation defect and was fixed locally, not attributed to the specification.

Review findings 1–5 are implementation/test defects, now corrected with independent
regressions. They are not proposed specification exceptions. #2 is a separate unresolved
normative interpretation question; tests ensure it is not claimed as successful L1 validation.

Handoff re-review (2026-11-06 JST): fetched the current bodies and confirmed both issues remain Open.
The unadopted options and recommended B proposal for #2 are in
[man03-resolution-proposal.md](../docs/man03-resolution-proposal.md); no normative change was made.
