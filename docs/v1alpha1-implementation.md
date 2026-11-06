# loj/v1alpha1 implementation

## Investigation and requirements (before implementation)

Baseline validator: 6dbd0b97e3d1d669d6e4949db1876e2a41b2c966.
Specification: 6295fd7769a09eeb0da56bcc6ff8c404ec391db6; normative chapters 01–10,
both grammars, JSON schemas, fixed conformance expectations and DDR 001–010 reviewed.
The syntax evaluation directory is an unversioned experimental reference. It uses
CommonMark protection ranges and a line marker lexer, but its bounded grammar lacks
the normative level attribute and canonical branch-number model; it is not copied.

| Requirements | Planned implementation / validation |
|---|---|
| BASIC-01–04 | Exact version, strict bytes, unsupported declarations, closed schema |
| MAN-01 | YAML token restrictions, duplicate detection, schema with format assertions |
| MAN-02 | Portable paths, case collision, symlink/reparse rejection, physical identity |
| MAN-03 | Manifest scope order; independent files; only classification context persists |
| MD-01–03 | pulldown-cmark 0.13.4 offset events; protected nested regions; raw body slices |
| HEAD-01–04 | Strict ATX labels, semantic ranks, supplement/appendix and deletion |
| NUM-01 / DISP-01 | Canonical Japanese/decimal numbers; first body line display checks |
| DIR-01–02 | Strict attributes and matched stack; legal parents, levels, own text |
| AST-01–02 | Ordered text/children, schema validation, duplicate IDs, no valid partial AST |
| ID-01–02 | Scope article IDs and parent-derived other IDs; partition invariance |
| SRC-01 | Original bytes plus independent node/text half-open byte positions |
| REF-01 | Preserve links; no L2 resolution requirement |
| VAL / CONF | Distinct statuses, all normative cases, exact AST and selected positions |

## Architecture decision

Keep the existing CLI, schematter integration, legacy JSON and library functions intact.
Add `law_semantic::v1alpha1`, with manifest, Markdown/marker parsing and semantic construction
separated. The existing 0.1 AST has different fields, source conventions, implicit paragraphs
and reference policy. Reusing `mapping::merge` would merge already mapped legacy laws and
lose the required classification context; it is therefore not used for v1alpha1.
The new model lives in the same semantic crate, not a duplicate crate or replacement API.
Reuse existing pinned CommonMark parser and JSON Schema infrastructure.

Main risks: HTML comment blocks can span multiple marker lines; parser ranges must be
interpreted before marker recognition. YAML must not silently overwrite keys. Source spans
must include original CRLF. Schema success alone cannot establish semantic conformance.
Formal fixtures are vendored unchanged with license and commit for offline reproducibility.

## CLI and status design

`check DIR` or `check law.yaml` selects v1alpha1; `--profile v1alpha1` is explicit.
Existing `check file.md --structure-schema ...` remains legacy; `--profile legacy` selects
its default mapping profile. L0/L1 are selected with `--level`; default L1.
Passed=0, Failed=1, Error=2, Incomplete=3. Incomplete never exits successfully, including
strict mode. Normative `rule` and implementation `code` are separate diagnostic fields.
The former appendix specification-pending restriction was removed after option B approval. Unknown versions use the
specific code `loj/v1alpha1/unsupported-version` with Error.

## Implemented pipeline and recovery

- `v1alpha1/mod.rs`: public `check(Path, Level)`, status policy and independent AST Schema check.
- `manifest.rs`: strict UTF-8 loading, YAML 1.2 events (duplicate keys checked before insertion),
  closed JSON Schema with format assertions, scope resolution and ordered file loading.
- `markdown.rs`: CommonMark top-level block ranges, exact raw line boundaries, plain ATX checks.
- `directives.rs`: marker grammar, required/unknown/duplicate attributes, canonical numbers.
- `number.rs`: strict canonical Japanese 1–9999 and independent decimal-width normalization.
- `syntax.rs`: rank/context stack, matched container stack, ordered content arena, display
  validation, duplicate IDs and source mapping; export to the normative semantic JSON.

No legacy AST field or public signature was changed. The new API uses the same semantic crate
but exports the v1alpha1 JSON contract, which must not be passed to legacy `validate_document`.
The CLI routes to the legacy two-layer pipeline or the Manifest pipeline explicitly.

Each Markdown file gets a fresh CommonMark parser, article and marker stack. Only classification
indices persist across files of the same scope. Article IDs use the scope, while classifications,
paragraphs, items and subitems use their specified semantic parents. Text slices are captured
between structural events, not reconstructed from rendered Markdown. Adjacent text is coalesced;
source-fragment positions remain separate. `sourceMap` includes empty lists for synthetic roots.

Subsequent main/supplement files must have an article/classification heading as their
first nonblank content (MD-03 separator blank lines are allowed); whitespace-only files,
ordinary comments before a heading and marker-only continuations fail MAN-03.
The actual whole-file CommonMark events determine whether a heading has inline markup,
including reference links resolved elsewhere in that file. Literal punctuation such as
unresolved `[B]`, an unmatched `*`, or a non-closing `#` is not blanket-rejected.
The heading's raw label must still equal parsed plain text. Caption trailing whitespace
is checked inside the fullwidth parentheses, per headings.ebnf, independently of line-end whitespace.

Malformed tokens produce lexical diagnostics without inventing nodes. An unmatched closing token
does not pop a different open container. A heading reports outstanding starts and resets the
container stack. Independent files continue after a read/encoding failure. Bad Manifest shape
stops resolution because paths/parents cannot be trusted. Any diagnostics suppress complete AST
output; positions on failed inputs describe recovery only. Report `complete` distinguishes tool
or resource interruption from a completed rejection; it does not promise a valid AST.

`sources[].text` preserves valid UTF-8 exactly, including marker lines and CRLF. Invalid UTF-8
is retained as `sources[].bytes`. Resource-truncated inputs are not claimed to be fully retained.
Node positions and text-fragment spans refer to original files; no UTF-16 indices are used.

## Dependency and input limits

Pinned new direct dependencies: yaml-rust2 0.10.4 (encoding conversion disabled), same-file 1.0.6.
CommonMark uses existing pulldown-cmark 0.13.4. Full resolved versions are in Cargo.lock.
Schema assets and all 105 upstream cases are vendored at SPEC_COMMIT with 393 SHA-256 entries;
the conformance runner verifies hashes before comparisons. `.gitattributes` disables newline
conversion for these assets. Future spec tracking can run against a separately reviewed vendor
update; routine CI never fetches an unpinned spec head or rewrites expected ASTs.

Implementation bounds: 16 MiB per input file, 128 MiB total loaded input, 4096 documents,
4096 UTF-8 bytes per Manifest string or structural token, 20,000 semantic nodes.
Exceeding these bounds is Incomplete, never Passed. YAML nesting is limited to 32; deeper
Manifest structures are rejected (no conforming Manifest schema structure needs that depth).
Semantic nesting is bounded by ranks plus Paragraph/Item/Subitem1–10; illegal nesting is rejected
before adding recursive children. No legal structures are inferred from list numbering.

One ReadBudget covers law.yaml and every Markdown read, before encoding validation or
source retention. Invalid UTF-8, BOM, NUL, bare CR and bytes read before an IO failure
all debit it. Metadata rejects oversized files before content reading; a bounded stream
read also protects against growth/stale metadata. At most one lookahead byte beyond
the remaining/per-file limit can be read to detect overflow; that file is not retained
and loading stops. Retained original bytes never exceed 128 MiB. This is not a bound
on process memory: AST, copies, JSON values and invalid-byte JSON arrays add overhead.

Paths are validated before access. Every document component is checked for symlink/reparse points;
canonical containment and physical handles reject escapes and hardlink aliases. Nonregular files
are checked before opening so a FIFO is not deliberately read. Windows junction and hardlink
rejection has an OS-specific test; Unix symlink/hardlink rejection is configured for CI.
This is not a filesystem sandbox against a concurrent attacker replacing files between checks
and open. Run against a stable checkout. HTML is retained, not sanitized for a renderer.
Reports can be larger than source inputs because they include original inputs, AST and mappings.

## Scope and remaining work

L0/L1 are implemented within the explicit resource limits. L2–L4, XML round-trip conversion,
legal authenticity, undeclared semantic omissions, preambles, unnumbered paragraphs, lettered
subitems and legal amendment history are outside v1alpha1. `unsupportedFeatures` is accepted
at L0 and rejected at L1. Legacy same-file reference resolution is not applied to v1alpha1.

The fixed suite has no L0 success cases and asserts no negative diagnostic byte ranges;
[loj-spec #1](https://github.com/law-of-japan/loj-spec/issues/1) records the coverage gap.
Local independent tests cover L0 success, unsupported-feature levels and diagnostic locations.
There are no currently deferred official cases. Future work: add upstream fixture coverage after specification review, and introduce sustained fuzzing and
large-corpus memory benchmarks. Fixed fixture success is not proof over all possible inputs.

[loj-spec #2](https://github.com/law-of-japan/loj-spec/issues/2): the user approved option B.
MAN-03 permits any nonblank Markdown in subsequent appendix files and preserves leading blank lines.
For each nonfinal appendix file, nested CommonMark events identify fenced code and HTML blocks.
A fenced block's end range must extend past its last content event: EOF/container termination alone fails.
HTML types 1–5 require their terminator; types 6–7 require a blank line, allowing parser container prefixes.
Diagnostics cover the opening physical line; whitespace-only subsequent files report the whole file.
Final/sole appendix files retain normal CommonMark EOF rules. L0 does not run these L1 checks.
Ordinary paragraphs, lists, quotes and indented code may end at EOF. Reference definitions stay file-local.
The spec change and 30 independently authored fixtures are pinned, not inferred from validator outputs.

## Conformance runner outcome policy after review

The runner checks status, exit code and completion as well as expected rules and AST.
Positive cases require Passed/0/complete; ordinary negatives require Failed/1/complete
and no AST. Incomplete, specification-pending, and unexpected Error never count as passes.
The sole expected Error is pinned T058: its input declares loj/v9 and its fixed expected
rule is BASIC-01. VAL-01 calls unsupported versions unprocessable. The runner verifies
that fixture premise, then requires Error/2/not-complete, no AST and only the specific
unsupported-version diagnostic. This is a harness policy for the implementation's
status contract, not a change to the upstream expected JSON or a parser case exception.

Review regression mapping: input-budget unit and integration tests (finding 1),
heading punctuation/whole-file reference markup (2), caption whitespace (3), subsequent
main/supplement file starts (4), incomplete/unexpected-error and T058 cause tests (5).
Windows Rust 1.99.1: 62 tests passed, including 104 formal cases. New fence and HTML
regressions cover nested containers, malformed closers, CRLF and protected code.
The initial implementation passed Windows/Linux/macOS stable and Linux MSRV CI.
Current branch CI results are recorded in reports/v1alpha1-conformance.md.
Status precedence remains Error > Incomplete > Failed > Passed. No case-specific parser exceptions.

## Reference review

The management guideline README/CONTRIBUTING were read via GitHub; its historical law-repository
naming/commit conventions do not replace v1alpha1 IDs. Reference inputs were not modified.
CommonMark [0.31.2 HTML blocks](https://spec.commonmark.org/0.31.2/#html-blocks) determine protected
regions (e.g. a div block continues to a blank line, even after `</div>`).
[GFM](https://github.github.io/gfm/) is supplementary, not the required parser mode.
[e-Gov XML documentation](https://laws.e-gov.go.jp/docs/law-data-basic/419a603-xml-schema-for-japanese-law/)
was checked as structural background; this implementation follows loj-spec's independent AST.
