# loj-validator レビュー修正・MAN-03 B案対応

レビュー指摘1〜5の修正を維持し、ユーザーが承認した#2のB案を実装した。
仕様判断待ちによる附属資料の一律Incompleteを解除。規範を先に明確化して独立期待値を追加し、
validatorはそのcommit `6295fd7769a09eeb0da56bcc6ff8c404ec391db6` に固定した。

## 差分と根拠

- 追加レビューでHTML終了タグのUnicode case-foldによる誤認を修正。`</ſcript>`を拒否する境界回帰を追加。仕様側例外は設けない。
- `markdown.rs`: CommonMarkの実イベントからフェンス/HTMLを識別し、非最終附属資料の明示終端を検証。引用・リスト内にも適用する。
- `syntax.rs`: 後続附属資料の非空白本文と非最終ファイルの境界を検証。違反はMAN-03/Failed。最終ファイル・L0は追加終端条件の対象外。
- フェンス・HTMLの独立回帰2関数を追加。既存分割回帰を採用規範に合わせ、原文統合、診断位置、違反/IOエラーを検査。
- 固定vendorは393ファイル。仕様側はMAN-03・文法注記・DDR・CHANGELOGと独立30ケースを変更。旧74正式ケースとSchemaは不変。
- 初期レビューの入力ReadBudget、CommonMark見出し、caption末尾空白、ファイル別開始条件、適合状態区別を再検証。旧API/CLIは維持。

## 検証

Windowsでfmt・Clippy・全62テスト成功。既存33テストを含む。
正式104/104成功（L0 11、L1 93）、失敗0、未実行0、仕様判断待ち0。参考L2 1件は未実行。
検証器状態はPassed 47 / Failed 56 / Error 1 / Incomplete 0。47完全ASTと97本文位置復元が一致。
T058のErrorは未対応versionという規範上の期待された拒否であり、予期しないErrorとは区別する。
[詳細・全ケース](v1alpha1-conformance.md)、[集計](review-verification.json)。

初期実装の3 OS stable/Linux MSRV CIと、B案仕様側CIは成功。
最終コード `4599a6971975ae5ce4e254b1786ae964c6bdb79c` は[CI](https://github.com/law-of-japan/loj-validator/actions/runs/42235465039)の4ジョブすべて成功。
各環境で62テストと正式104ケースを検証。Windows/macOSのMSRV組合せは対象外で未実行。
仕様commit6295fd7、validator実装1a53cf4と追加修正4599a69はCI用ブランチへpush済み。
CI結果の記録は後続のローカル文書commitに保存する。

## 未解決・境界

[#1](https://github.com/law-of-japan/loj-spec/issues/1)の正式L0正例・診断位置のカバレッジ改善は未対応。
#2はB案を承認・実装済みだがGitHub Issueは統合前のためOpen。PR・merge・main push・Releaseは行わない。
継続fuzzing、ピークメモリ、同時差替え耐性、旧性能測定の再実行は未実施。
バイト制限はプロセスメモリ上限ではなく、固定ケース成功も全面準拠の証明ではない。

[作業記録](../docs/codex-progress.md)と[判断記録](../docs/man03-resolution-proposal.md)を更新した。
