# Next specification implementation (loj/v1alpha2 and the new kinds)

Target: loj-spec commit `4ac8f2b47c98e76fa5ccda35d7a3ee60f0816ad4`, vendored as
`spec`, `schemas`, `conformance`, `grammar/v1alpha2` and `LICENSE` in `vendor/loj-spec` with `COMMIT` and
`SHA256SUMS.json`. The v1alpha1 fixtures of `b704631` stay unchanged in
`vendor/loj-spec-v1alpha1`; the v1alpha1 and legacy pipelines are unchanged.

## Modules (`crates/law-semantic/src`)

| Module | Checks | Requirements |
|---|---|---|
| `common` | strict bytes, CORE-03 YAML, report, TimePoint arithmetic, schema-error mapping | CORE-03, VAL-04, VAL-07, TIME-02/03 |
| `v1alpha2` | L0, L1 of one version package (built on the v1alpha1 pipeline) | BASIC, MAN, MD, HEAD, NUM, DIR, DISP, AST, SRC, ID, REF, PARA, TABLE, PRE |
| `repository` | L0/L1 of every version, L3 `historyStructure`, L3 `evidence` | HIST, REV, TIME, TYPE, GLYPH |
| `registry` | registry check; Record resolution | REG, RID |
| `xref` | L4 `relationResolution`, `relationConsistency`, `timeConsistency` | XREF-01..03 |
| `rebuild` | rebuild report check | REBR |
| `change` | change check over `git bundle` commits | CHG-02..04 |
| `fixref` | fixed and Revision reference resolution | FIXREF-02..07 |
| `checks` | input kinds and dispatch (`Input`, `run`) | |

Reserved checks (L2, L3 `sourceMatch`, L4 `clauseReference`) are not implemented and
cannot be selected.

## Results

- Each check (L3/L4: category) is judged independently (VAL-02). Unknown
  `schemaVersion`s and unreadable inputs are `incomplete`, never `fail` or `pass`.
- A schema error is reported under the rule its schema location names: common types
  (`TYPE-0x`, `TIME-01`, `TYPE-05` for `claimRule`), `REV-01` for revision IDs, the
  kind's specific rule where a field belongs to one (e.g. `REG-03` for number claims,
  `GLYPH-05` for note content, `REBR-02` for legacy hashes), otherwise the kind's `-01`.
- Warnings, `undetermined` and `pending` diagnostics never fail a check.
- Glyph notes of a version whose package fails L1 are not judged: `evidence` is
  `incomplete` (GLYPH-11).
- L4 `notFetched` references make the category `incomplete`, never `fail`.
- The registration space is read from `space.yaml` `id` when present; `space.yaml` has
  no defined form in this version, so the default is `loj`, the space of the
  conformance registries (CONF-01).
- Git inputs use the `git` command; blobs are read raw so byte positions hold.
  The change and reference checks need `git` 2.32 or later on `PATH` (for `GIT_CONFIG_GLOBAL`); CI uses the
  runners' git. Without git, those checks are `incomplete` with a "git is not
  available" diagnostic. Only `refs/heads/*` is fetched from a bundle, so tags and
  other refs are not resolvable.
- A git command that fails after the bundle is open (timeout, output cap, any exit
  status other than the "absent" status 1) is never reported as a finding about the
  data. The change check ends as `incomplete` at the first such failure, with a
  "git failed" (or timeout/output) diagnostic; a reference is `notFetched` with that
  reason. A tree refused for its content is noted for that commit only. These
  reports, like the one for a base or head missing from the bundle, carry
  `target.repository`.
- A bundle is untrusted. git runs isolated from the caller's environment, user and
  system configuration and hooks, with `fsckObjects`, a 120 s timeout per command and
  a cap on kept output. Bundle files over 1 GiB are refused. The exported tree is
  checked before anything is written: safe, portable, case-distinct names, regular
  files only, 16 MiB per file, 128 MiB and 100,000 files in all.
- Known limit: the fetch itself is bounded only by the bundle size and the timeout.
  A highly compressible bundle can unpack to more objects than its size suggests.
  Run the change and reference checks on untrusted bundles in a disposable
  environment with a disk quota.

## CLI

```text
loj-validator check PATH [--level L0|L1|L3|L4]... [--category NAME]... [--check NAME]...
                         [--format json|text] [--ast FILE]
loj-validator check --registry DIR --repository DIR... [--category NAME]...
loj-validator check BUNDLE --base COMMIT --head COMMIT
loj-validator check BUNDLE --registry DIR --reference REF
```

The JSON output is a report of `schemas/common/v1alpha1/report.schema.json`.
`law.yaml` and directories now default to the next specification; v1alpha1 is selected
with `--profile v1alpha1`. Exit codes: 0 pass, 1 fail, 2 usage/tool error, 3 incomplete
or nothing passed.

## Conformance runner (`tests/next_conformance.rs`)

- Success: every expected result matches, no `error` diagnostic, the AST matches
  exactly, expected positions are present.
- Failure: every expected `error` diagnostic (`rule` + `schemaVersion`) is present;
  without expected results the case's own check must not pass.
- `references` (CONF-01): every expected entry is reported with the same `target` and
  `result`; extra references are allowed.
- `resolution`: exact.

| Suite | Normative | Passed | Informative (not counted) |
|---|---|---|---|
| loj/v1alpha2 | 123 | 123 | 1 (pkg-026, L2) |
| common/v1alpha1 | 12 | 12 | 0 |
| loj-history/v1alpha1 | 15 | 15 | 0 |
| loj-glyphs/v1alpha1 | 9 | 9 | 0 |
| loj-registry/v1alpha1 | 8 | 8 | 0 |
| loj-rebuild/v1alpha1 | 4 | 4 | 0 |

Passing the fixed cases is a necessary condition only (CONF-02).
