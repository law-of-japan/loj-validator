# Next specification implementation (loj/v1alpha2 and the new kinds)

Target: loj-spec commit `c4149e7e8ad54f3262bd592ea4bde493cce54e26`, vendored as
`spec`, `schemas`, `conformance` and `LICENSE` in `vendor/loj-spec` with `COMMIT` and
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
  no defined form in this version, so the default is `loj`.
- Git inputs use the `git` command; blobs are read raw so byte positions hold.

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
- `references`: every expected entry is present (the guide does not define an exact
  comparison; common-001 lists only the Record reference although its
  `timeConsistency: pass` requires the `sameAs` Revision reference to be resolved too).
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
