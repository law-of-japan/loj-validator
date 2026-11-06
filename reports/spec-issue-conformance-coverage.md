## Summary

v1alpha1 fixed conformance data has no successful normative L0 case and no negative-case diagnostic span assertions. Add independent fixtures for these contracts.

## Affected specification

- Specification: loj/v1alpha1 at b2356c6f113f3a02a1a790dc5cf3813d82cf3fe0
- Section: spec/v1alpha1/06-semantic-model.md, 09-validation.md, 10-conformance.md; schemas/v1alpha1/expected.schema.json
- Rule ID: SRC-01, VAL-01, CONF-01, CONF-02

## Problem

The index has 11 normative L0 cases, all negative, and 63 normative L1 cases. An implementation that always rejects L0 cannot be detected by positive L0 fixtures. In particular, unsupportedFeatures must be accepted at L0 but rejected at L1; T042 exercises only L1.

SRC-01 specifies exact locations for malformed attributes, unmatched/unclosed markers and display errors. Negative expected files currently assert rule membership only. The positions array addresses nodeId, not a diagnostic. A validator returning correct rule IDs at incorrect byte ranges can pass these cases. This is a coverage enhancement, not a claim that current fixed expectations contradict normative prose.

## Reproduction

Use T042's law.yaml and main.md with requested level L0: it should pass input validation even though unsupportedFeatures is nonempty. There is no normative L0 success fixture covering this.

For diagnostic location, append CRLF-terminated content:

```markdown
# 第一条
<!-- loj:paragraph number=2 -->
３　本文😀。
<!-- loj:end-paragraph -->
```

The DISP-01 range must cover the original body line, including the CRLF under the existing line-span convention. Rule-only comparison cannot distinguish UTF-16 positions or a range missing LF.

## Expected behavior

Independent fixed tests catch both false-negative L0 implementations and incorrect diagnostic byte positions. L0 success expectations must explicitly say whether AST is absent, since L0 does not require L1 semantic construction.

## Actual behavior / Ambiguity

All 74 normative cases can pass without exercising either contract. CONF-01 currently says success=true carries a complete semantic AST; extending the index to L0 successes therefore requires clarification of level-dependent expectations.

## Options

1. Add L0 success cases with level-dependent expected schema and diagnostic span assertions.
2. Add a separate L0/diagnostic contract suite outside the current index.
3. Leave these checks implementation-specific (weaker interoperability).

## Recommendation

Option 1: permit L0 success without an AST, include T042-like L0 input, and allow negative expected diagnostics to specify optional path/start/end. Add Japanese, 4-byte Unicode, CRLF, mismatched ending and unclosed start cases. Keep rule-only matches for cases where recovery is intentionally unconstrained.

## Impact

- loj-validator: local independent tests can cover the gap now; no fixture-specific parsing is needed.
- Existing law data: no syntax change.
- Compatibility: expected-data schema extension and comparison policy need versioned documentation.
- Conformance tests: current cases remain unchanged; new cases strengthen coverage.
