# v1alpha1 適合検証結果（MAN-03 B案対応）

固定仕様: `6295fd7769a09eeb0da56bcc6ff8c404ec391db6`。vendor 393ファイルをcommitのGit blobとSHA-256で照合。
旧T001〜T075・Schemaは不変。承認された仕様変更による独立30ケースを追加した。

| 対象 | 実行 | 成功 | 失敗 | 未実行 | 仕様判断待ち |
|---|---:|---:|---:|---:|---:|
| 正式L0 | 11 | 11 | 0 | 0 | 0 |
| 正式L1 | 93 | 93 | 0 | 0 | 0 |
| 正式合計 | 104 | 104 | 0 | 0 | 0 |
| 参考L2 T026 | 0 | 0 | 0 | 1 | 0 |

検証器の状態は **Passed 47 / Failed 56 / Error 1 / Incomplete 0**。
旧正式74ケースも全件成功、新規30ケースも全件成功。適合成功数と入力の合格数は異なる。
47正例の固定AST完全一致、指定ノード位置一致。97本文要素でUTF-8位置列から原文を復元し一致。
通常負例はFailed/1/complete=true/ASTなしと期待ruleを要求する。
T058だけはloj/v9とVAL-01に基づくError/2/complete=falseを期待し、unsupported-version診断のみに限定。
予期しないError・Incompleteを成功扱いしない。

## ローカル検証

Windows Rust 1.99.1で `cargo fmt --all -- --check`、
`cargo clippy --workspace --all-targets --locked -- -D warnings`、
`cargo test --workspace --locked` が終了0。62テスト関数成功（既存33を含む）、失敗/ignored 0。
このうち1関数が正式104ケースを実行するため、件数を合算しない。
Windowsリンク検証は許可された通常ホスト権限で実行。sandbox増分キャッシュの
ハードリンク不可警告はコピー代替となり、fmt/Clippyは終了0。

[fmt](fmt.log)、[Clippy](clippy.log)、[全テスト](cargo-test.log)、
[正式実出力](v1alpha1-conformance.json)、[集計](review-verification.json)。
元の指摘1〜5の独立回帰を維持。B案はフェンスとHTMLそれぞれの追加回帰を持ち、
引用・リスト内、閉鎖文字種/長さ/インデント、CRLF、最終EOF、空白だけの後続、
診断の開始行、AST原文統合、L0非適用、既知違反とIOエラーの併存を検査する。

## CIと限界

初期実装 `5a71b00` は[CI](https://github.com/law-of-japan/loj-validator/actions/runs/42232317522)で
Windows/Linux/macOS stableとLinux Rust 1.88.0の4ジョブすべて成功。
B案対応後のvalidator CIはcommit/push後に確認する（現時点で未実行）。
仕様側B案は[CI](https://github.com/law-of-japan/loj-spec/actions/runs/42234173859)成功。
旧Dockerログlinux-msrv/stableおよび対応JSONは旧74ケース・60関数の履歴証跡であり、今回の104ケースとは別。

#2は承認・実装済み、GitHubでは統合前のためOpen。#1の正式L0正例・負例診断位置不足は残る。
L2は対象外。継続fuzzing、ピークメモリ、同時ファイル差替え耐性、旧ベンチマーク再測定は未実施。
入力バイト制限はプロセスメモリ上限を保証しない。正式ケース成功は全入力への準拠証明ではない。

## 全ケース

| ケース | レベル | テスト結果 | 検証器状態 |
|---|---|---|---|
| T001 | L1 | 成功 | Passed |
| T002 | L1 | 成功 | Passed |
| T003 | L1 | 成功 | Passed |
| T004 | L1 | 成功 | Passed |
| T005 | L1 | 成功 | Passed |
| T006 | L1 | 成功 | Failed |
| T007 | L1 | 成功 | Failed |
| T008 | L1 | 成功 | Failed |
| T009 | L1 | 成功 | Passed |
| T010 | L1 | 成功 | Failed |
| T011 | L1 | 成功 | Passed |
| T012 | L1 | 成功 | Passed |
| T013 | L1 | 成功 | Failed |
| T014 | L1 | 成功 | Failed |
| T015 | L1 | 成功 | Failed |
| T016 | L1 | 成功 | Passed |
| T017 | L1 | 成功 | Passed |
| T018 | L1 | 成功 | Passed |
| T019 | L1 | 成功 | Passed |
| T020 | L0 | 成功 | Failed |
| T021 | L1 | 成功 | Passed |
| T022 | L1 | 成功 | Failed |
| T023 | L1 | 成功 | Failed |
| T024 | L1 | 成功 | Passed |
| T025 | L1 | 成功 | Passed |
| T026 | L2 | 未実行（参考） | — |
| T027 | L0 | 成功 | Failed |
| T028 | L1 | 成功 | Failed |
| T029 | L1 | 成功 | Failed |
| T030 | L1 | 成功 | Failed |
| T031 | L1 | 成功 | Failed |
| T032 | L1 | 成功 | Failed |
| T033 | L1 | 成功 | Failed |
| T034 | L1 | 成功 | Passed |
| T035 | L1 | 成功 | Passed |
| T036 | L1 | 成功 | Failed |
| T037 | L1 | 成功 | Passed |
| T038 | L0 | 成功 | Failed |
| T039 | L0 | 成功 | Failed |
| T040 | L0 | 成功 | Failed |
| T041 | L0 | 成功 | Failed |
| T042 | L1 | 成功 | Failed |
| T043 | L1 | 成功 | Failed |
| T044 | L1 | 成功 | Failed |
| T045 | L1 | 成功 | Passed |
| T046 | L1 | 成功 | Failed |
| T047 | L1 | 成功 | Failed |
| T048 | L1 | 成功 | Passed |
| T049 | L1 | 成功 | Failed |
| T050 | L1 | 成功 | Failed |
| T051 | L1 | 成功 | Passed |
| T052 | L1 | 成功 | Passed |
| T053 | L1 | 成功 | Passed |
| T054 | L1 | 成功 | Passed |
| T055 | L1 | 成功 | Failed |
| T056 | L0 | 成功 | Failed |
| T057 | L0 | 成功 | Failed |
| T058 | L0 | 成功 | Error |
| T059 | L0 | 成功 | Failed |
| T060 | L0 | 成功 | Failed |
| T061 | L1 | 成功 | Passed |
| T062 | L1 | 成功 | Passed |
| T063 | L1 | 成功 | Passed |
| T064 | L1 | 成功 | Passed |
| T065 | L1 | 成功 | Passed |
| T066 | L1 | 成功 | Failed |
| T067 | L1 | 成功 | Failed |
| T068 | L1 | 成功 | Failed |
| T069 | L1 | 成功 | Failed |
| T070 | L1 | 成功 | Failed |
| T071 | L1 | 成功 | Failed |
| T072 | L1 | 成功 | Failed |
| T073 | L1 | 成功 | Passed |
| T074 | L1 | 成功 | Failed |
| T075 | L1 | 成功 | Passed |
| T076 | L1 | 成功 | Passed |
| T077 | L1 | 成功 | Failed |
| T078 | L1 | 成功 | Passed |
| T079 | L1 | 成功 | Failed |
| T080 | L1 | 成功 | Failed |
| T081 | L1 | 成功 | Failed |
| T082 | L1 | 成功 | Passed |
| T083 | L1 | 成功 | Failed |
| T084 | L1 | 成功 | Passed |
| T085 | L1 | 成功 | Passed |
| T086 | L1 | 成功 | Failed |
| T087 | L1 | 成功 | Passed |
| T088 | L1 | 成功 | Failed |
| T089 | L1 | 成功 | Passed |
| T090 | L1 | 成功 | Failed |
| T091 | L1 | 成功 | Passed |
| T092 | L1 | 成功 | Failed |
| T093 | L1 | 成功 | Passed |
| T094 | L1 | 成功 | Failed |
| T095 | L1 | 成功 | Passed |
| T096 | L1 | 成功 | Failed |
| T097 | L1 | 成功 | Passed |
| T098 | L1 | 成功 | Failed |
| T099 | L1 | 成功 | Passed |
| T100 | L1 | 成功 | Failed |
| T101 | L1 | 成功 | Passed |
| T102 | L1 | 成功 | Failed |
| T103 | L1 | 成功 | Passed |
| T104 | L1 | 成功 | Passed |
| T105 | L1 | 成功 | Passed |
