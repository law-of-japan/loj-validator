# Specification feedback

2026-11-07更新: 仕様PR #3（B案）と#4（レベル定義・独立性）はmerge済み。
現在の固定仕様はb704631b6501a0a01c1eb6faf99cfe16a4bccfa9。#1はユーザー判断で保留し、今回のmergeを阻害しない。
以下の#2の「統合前」はvalidator側の統合完了前の状態を表す。

| Issue | Current status |
|---|---|
| [#1](https://github.com/law-of-japan/loj-spec/issues/1) | Open: positive L0 / negative diagnostic-span formal coverage remains incomplete. Local regressions supplement it. |
| [#2](https://github.com/law-of-japan/loj-spec/issues/2) | Option B approved by user; implemented in spec commit `6295fd7769a09eeb0da56bcc6ff8c404ec391db6` and validator. GitHub Issue remains Open pending integration; no PR/merge/closure performed. |

MAN-03 distinguishes main/supplement heading starts from appendix body starts. Nonfinal appendix
fences and HTML need explicit CommonMark terminators; the final file permits ordinary EOF termination.
The clarification is in spec/grammar/DDR/changelog; no Schema shape changed.
T076–T105 are independently authored inputs/trees, not validator snapshots. T001–T075 remain unchanged.
This removes the blanket multi-file appendix Incomplete restriction. All 104 formal cases pass.
Review findings 1–5 are implementation/test defects, not specification exceptions.

The initial issue search and submitted bodies are preserved in
[coverage issue](spec-issue-conformance-coverage.md) and [MAN-03 issue](spec-issue-man03-appendix-splitting.md).
Those describe the original ambiguity, not the current approved rule.
[#2 decision record](../docs/man03-resolution-proposal.md).
