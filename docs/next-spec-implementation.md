# Next specification implementation (loj/v1alpha2 and the new kinds)

Target: loj-spec commit `ec5ade100ec408fa08ab4572db0e86d38ab7db68`, vendored as
`spec`, `schemas`, `conformance`, `grammar/v1alpha2` and `LICENSE` in `vendor/loj-spec` with `COMMIT` and
`SHA256SUMS.json`. The v1alpha1 fixtures of `b704631` stay unchanged in
`vendor/loj-spec-v1alpha1`; the v1alpha1 and legacy pipelines are unchanged.

## Modules (`crates/law-semantic/src`)

| Module | Checks | Requirements |
|---|---|---|
| `common` | strict bytes, CORE-03 YAML, report, TimePoint arithmetic, schema-error mapping | CORE-03, VAL-04, VAL-07, TIME-02/03 |
| `v1alpha2` | L0, L1 of one version package (built on the v1alpha1 pipeline) | BASIC, MAN, MD, HEAD, NUM, DIR, DISP, AST, SRC, ID, REF, PARA, TABLE, PRE, TOC, AMEND |
| `repository` | L0/L1 of every version, L3 `historyStructure`, L3 `evidence` | HIST, REV, TIME, TYPE, GLYPH |
| `registry` | registry check; Record resolution | REG, RID |
| `xref` | L4 `relationResolution`, `relationConsistency`, `timeConsistency`, `displayConsistency` | XREF-01..03, XREF-05/06 |
| `rebuild` | rebuild report check | REBR |
| `change` | change check over `git bundle` commits | CHG-02..04 |
| `fixref` | Record, fixed and Revision references; structured conversion | FIXREF-02..07 |
| `checks` | input kinds and dispatch (`Input`, `run`, `run_with_space`) | |

Reserved checks (L2, L3 `sourceMatch`, L4 `clauseReference`) are not implemented and
cannot be selected.

## Results

- Each check (L3/L4: category) is judged independently (VAL-02). Unknown
  `schemaVersion`s and unreadable inputs are `incomplete`, never `fail` or `pass`.
- A schema error is reported under the rule its schema location names: common types
  (`TYPE-0x`, `TIME-01`, `TYPE-05` for `claimRule`), `REV-01` for revision IDs, the
  kind's specific rule where a field belongs to one (e.g. `REG-03` for number claims,
  `GLYPH-05` for note content, `REBR-02` for legacy hashes), otherwise the kind's `-01`.
- Results aggregate violations first, then unprocessable input or `undetermined`,
  then pass. Warnings and pending findings do not change the result. Only the
  warning-only HIST-06/12 order comparisons exclude `undetermined` from aggregation.
- JSON always includes `target.repository` and `target.commit`. Directory inputs
  have no immutable commit binding: `commit` is the empty string, and their reports
  must not be used to claim conformance of an identified record version. Bundle
  change/reference results include the resolved commit when available.
- L3/L4 retain each input Record's report in the Rust `Report.records` API.
  `--record ID` emits one of these reports; omission emits the aggregate. The
  single-target report schema has no extra top-level `records` property. Selected
  reports judge only that Record, including required counterparts, not unrelated
  input failures. Duplicate or malformed Record identities make the input set
  unprocessable; no overwritten Record is silently skipped. Byte spans use `location.path/start/end`;
  YAML semantic findings use the extension `location.file/pointer`, without
  fabricated byte positions.
- Glyph notes of a version whose package fails L1 are not judged: `evidence` is
  `incomplete` (GLYPH-11).
- GLYPH-05: an `illegible` note (or number-claim glyph) with `basis: otherSource`
  must cite two or more distinct Sources of its file: one of kind `promulgation` or
  `officialCorrection`, and another not of kind `egov` or `index`. A Source that is
  not in the file counts for neither (TYPE-03 reports it). Which locator shows the
  illegible place, and whether the other Source is an original, are left to L3
  source comparison.
- L4 `notFetched` references make the category `incomplete`, never `fail`.
- Registration space is explicit input (`run_with_space`, `--registration-space`).
  There is no implicit `loj` and `space.yaml` is ignored. Omitted-space RecordRefs
  still resolve locally without this context; explicit-space resolution and string
  conversion are undetermined when context is missing. Another explicit space is
  not fetched, except REG-02 replacedBy, which must resolve in the same registry.
- Git inputs use the `git` command; blobs are read raw so byte positions hold.
  The change and reference checks need `git` 2.32 or later on `PATH` (for `GIT_CONFIG_GLOBAL`); CI uses the
  runners' git. Without git, those checks are `incomplete` with a "git is not
  available" diagnostic. Only `refs/heads/*` is fetched from a bundle, so tags and
  other refs are not resolvable.
- A git command that fails after the bundle is open (timeout, output cap, any exit
  status other than the "absent" status 1) is never reported as a finding about the
  data. The change check ends as `incomplete` at the first such failure, with a
  "git failed" (or timeout/output) diagnostic; a reference is `notFetched` with that
  reason. A tree refused for its content is noted for that commit only. A commit
  whose message is not UTF-8 is a CHG-03 error, and its trees are not compared
  (its change kind is unknown, as with a missing `LoJ-Change`). These
  reports, like the one for a base or head missing from the bundle, carry
  `target.repository`.
- A bundle is untrusted. git runs isolated from the caller's environment, user and
  system configuration and hooks. Automatic maintenance and GC are disabled so
  fetch cannot leave background repacking racing the object-store check. Git uses
  `fsckObjects`, a 120 s timeout per command and
  a cap on kept output. Bundle files over 1 GiB are refused. After fetch, the
  stored file bytes under `objects/` (packs, indexes and loose objects) are
  checked against a separate 1 GiB limit, with its own 120 s traversal deadline
  starting after fetch (in addition to the per-command timeout).
  Each regular-file path is counted, including hardlink aliases. Unreadable or
  non-regular entries stop the check as incomplete. The exported tree is
  checked before anything is written: safe, portable, case-distinct names, regular
  files only, 16 MiB per file, 128 MiB and 100,000 files in all.
  Export reads the checked object IDs with one `git cat-file --batch` process,
  keeps at most 128 MiB of blob data plus batch headers in memory, verifies its
  response sizes, and shares a 120 s deadline across tree listing,
  validation, blob retrieval and file writes. The deadline is checked between
  filesystem operations; a blocked filesystem call cannot be interrupted.
- Known limit: the post-fetch store check does not bound peak disk use during
  fetch, allocated filesystem blocks, or decompressed object sizes. A highly
  compressible bundle can unpack to more objects than its size suggests. Run
  checks on untrusted bundles in a disposable environment with a disk quota.
- On Unix, git starts in a dedicated process group; timeout kills that group,
  including descendants retaining pipes after the direct child exits. The pinned
  Unix-only `nix` dependency supplies safe signal APIs. On Windows, only the
  direct child is killed; inherited-pipe waits still have a deadline.
- `Bundle::Drop` removes its private `loj-bundle-*` and `loj-tree-*` temporary
  directories on normal return and panic unwinding (best effort if removal fails).
  The Unix next-spec CLI registers SIGINT/SIGTERM through pinned `signal-hook`
  and passes an atomic flag in a thread-local cancellation scope. Git checks it
  before spawning and at intervals of at most 50 ms while waiting. The owning thread kills
  the group before reaping its leader (preventing PID reuse), then returns through
  normal Drop cleanup. Cancellation wins over timeout when both are observed.
  Subsequent git calls in that scope cannot start. Nested scopes restore the
  previous flag, including on unwind; other threads do not inherit the flag.
  Other work polls the same flag at checkpoints in loops that grow with the
  input: per document, Revision, Record file, repository, exported file, commit
  and reference, and per Markdown token and tree node. A single YAML or
  CommonMark parse, a single hash and a blocked filesystem call are not
  interrupted and finish before the next checkpoint. A check that observes
  cancellation leaves its loop and returns through normal Drop cleanup; there
  is no unwinding. Because the loops stop early, the public entry points
  (`checks::run`, `checks::run_with_space`, `v1alpha2::check` and the `fixref`
  checks) check the flag once more before returning: when it is set, every
  selected result (and every Record report) is `incomplete`, partial
  diagnostics, references, resolutions and ASTs are discarded, and one
  undetermined VAL-04 diagnostic remains. Outside a scope checkpoints never
  cancel, so results are unchanged. A flag set after the last checkpoint but
  before that final check also cancels the report, even if every check had
  completed. A cancelled `--ast` run writes no AST.
  After execute returns, the CLI checks the flag once: cancellation produces
  exit 3 and a JSON error with kind `incomplete`; later signals do not change
  the result. Registrations remain until exit, so repeated signals cannot
  interrupt cleanup or output. Windows, legacy and v1alpha1 are unchanged.
  This interruption boundary precedes publication of any validation report:
  a cancelled invocation reports interruption even if it has already found a
  violation. The usual fail-over-incomplete aggregation applies to completed
  validation reports, not to this CLI interruption error. Signal registration
  failure also returns 3 because the environment could not support a safely
  interruptible check; it is neither an input violation nor an argument error.
  Abort, SIGKILL and host failure can still bypass Drop and leave files behind.
  No global signal handler or automatic stale-directory sweep is installed
  by the library. Use a dedicated temporary volume/directory for untrusted checks
  and reclaim it after all validator and child processes have stopped. Do not
  delete matching directories that may belong to a live check.

## CLI

```text
loj-validator check PATH [--level L0|L1|L3|L4]... [--category NAME]... [--check NAME]...
                         [--format json|text] [--ast FILE] [--registration-space SPACE] [--record ID]
loj-validator check --registry DIR --repository DIR... [--category NAME]...
loj-validator check BUNDLE --base COMMIT --head COMMIT
loj-validator check BUNDLE --registry DIR --reference REF
```

The JSON output is a report of `schemas/common/v1alpha1/report.schema.json`.
`law.yaml` and directories now default to the next specification; v1alpha1 is selected
with `--profile v1alpha1`. Exit codes: 0 pass, 1 fail, 2 usage/tool error, 3 incomplete
or nothing passed.

## Conformance runner (`tests/next_conformance.rs`)

- Every listed expected result and diagnostic must match; unspecified checks may
  have extra results. Success rejects errors of the case's own check/category,
  excluding unprocessable-input reasons. A successful case may expect incomplete.
  An expected AST matches exactly; no AST is required for an L0-only case.
- Diagnostics in either kind of case match `rule`, `schemaVersion` and `class`.
  When an expected diagnostic specifies `location`, its `path`, `start` and `end`
  must match; other diagnostic fields and additional diagnostics are allowed.
- Negative cases require all listed diagnostics; expected result values govern aggregation.
- `references` (CONF-01): every expected entry is reported with the same `target` and
  `result`; extra references are allowed.
- `resolution`: exact.

| Suite | Normative | Passed | Informative (not counted) |
|---|---|---|---|
| loj/v1alpha2 | 221 | 221 | 1 (pkg-026, L2) |
| common/v1alpha1 | 55 | 55 | 0 |
| loj-history/v1alpha1 | 61 | 61 | 0 |
| loj-glyphs/v1alpha1 | 20 | 20 | 0 |
| loj-registry/v1alpha1 | 13 | 13 | 0 |
| loj-rebuild/v1alpha1 | 5 | 5 | 0 |

Passing the fixed cases is a necessary condition only (CONF-02).

## Normative boundaries

- There are 375 normative cases and one informative L2 case. `derivedFrom` and
  `rejectedBy` are audit metadata, never instructions to regenerate or skip inputs.
- MD-04 emptiness uses the 25 Unicode 15.1 White_Space code points. Paragraph
  captions require `caption=present`; Toc placement and mixed number scripts are
  checked. TABLE-04 uses occupied intervals, not a grid proportional to span values.
- PARA-03: paragraphs at main-scope document level are children of MainProvision;
  after a classification without an article they belong to the classification
  (DIR-02). Mixing, more than one main file in the paragraph form, and an empty
  main provision are PARA-03; MAN-03's article-boundary check is skipped for the
  paragraph form, and PRE/TOC placement also requires no main paragraph yet.
- AMEND-01/02: an open `amendment-content` is a frame with its own classification
  stack and current article. A heading whose innermost open container is the
  content is read as a classification or article of that scope and closes no
  marker; a supplement label there is HEAD-01. The amendment shape, its numbering,
  the single group of a content, and an amendment inside a content are checked
  on the finished tree (AMEND-01/02). The instruction text is never parsed.
- AMEND-01 `instruction=implicit`: `implicit` is the only value of the
  attribute (`explicit` is a DIR-01 error); every Amendment of the AST carries
  `instruction`, `explicit` when the attribute is omitted. An implicit amendment
  holds exactly one content and no text before it, not even a blank line, and
  only blank text after it. One that follows another implicit amendment of the
  same parent with only blank parent text between them is AMEND-01; other
  children between them do not separate them, as only parent text does.
- HEAD-04: `削除` is followed by a blank line or the end of the file. When the
  next line is a heading or a closing marker (in an amendment content as at the
  document level), the article is still deleted and HEAD-04 is reported.
- LIST-01/02: a `list` takes `number` only; a branch number is accepted by the
  lexer and reported as LIST-01, since list numbers are the order 1..n among the
  sibling lists. A list holds text only (a marker in it is a DIR-02 parent error),
  has no display number (`display` is empty, a leading 一 stays text) and is
  `{parent}/list/{number}`. A paragraph, item or subitem with both items or
  subitems and lists is LIST-02, reported once at its first list; directly in an
  amendment content the same mix is AMEND-02, where lists are a group of their own.
- HIST-13 tracks deleted row keys along each basedOn branch; partial, absent and
  invalid-L1 revisions are skipped without treating their rows as deletions.
  Glyph candidates come only from the revision's default path. The Unicode 15.1
  compatibility decomposition set excludes the twelve non-decomposing ideographs.
- `sourceUris`, `glyphCheck`, `glyphPreservation` and `collection` are the current
  field names; the closed schemas reject their former names.
- CHG-02 checks dependent metadata and file boundaries as well as AST equality.
  Declared corrections and restorations do not build or compare their ASTs;
  cached snapshots build them only if a later change needs the comparison.
  Evidence is checked in the resulting history, without requiring new evidence.
  CHG-03 reads only a syntactically valid final trailer paragraph, with ASCII
  space/tab trimming and LF splitting; merge commits and duplicate `LoJ-Change`
  or `LoJ-Revisions` trailers are rejected without comparison, for every kind.
  CHG-04 reports each uncomparable AST, even when a format violation also exists.
  A review trailer removes only the automatic-format violation, not incompleteness.
  No normative migration mapping exists for this specification pair.
  The canonical data files are `history.yaml`, `rebuild.yaml`, `space.yaml`,
  `versions/**` and `records/**` (CHG-01); other files may change under any kind
  and are not checked. `auxiliary` requires every canonical data file to keep its
  path, bytes and Git file mode (`100644`/`100755`, from the tree listing), builds
  and compares no AST, and so is never CHG-04 undetermined. A tree with any other
  mode (a symbolic link, a submodule) is still refused, which leaves the check
  incomplete.
  CHG-02 names the file mode for `auxiliary` only. For the other kinds, a
  canonical data file that the kind may not change, kept byte for byte with only
  its mode changed, is reported undetermined (CHG-02), since the specification does
  not say whether that is within the kind's range; a mode change to a file the
  kind may change is no finding (law-of-japan/loj-validator#50).
  `auxiliary` reads no history manifest either: a commit is decided by the bytes
  even when `history.yaml` is unreadable, has an unsupported `schemaVersion` or an
  invalid Revision ID. Every other kind reads the manifest, so such a commit (as
  either side) leaves that comparison incomplete, noted once per commit.
- `fixref::to_string_reference` and `to_structured` preserve identity fields;
  `check_structured` checks the auxiliary path without following it. Source.fixed
  structural validation belongs to its containing file; fetching remains outside
  L3/L4, while sameAs comparison belongs to L4.
- Glyph checks verify node resolution and recorded text at the declared offset;
  they cannot detect an accidental match at a different intended location.
  Source matching, view generation, time queries and lossless file rewriting are
  not provided, and their conditional requirements are not claimed as implemented.

Date arithmetic in the next-specification API (`Date::add_days`, `after_months`)
returns `Option<Date>` for invalid dates or unrepresentable years. Intermediate
arithmetic uses i128; TimePoint calc evaluation outside years 1..=9999 or its
integer-count representation returns None and a TIME-03 undetermined finding.
