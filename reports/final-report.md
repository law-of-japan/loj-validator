# loj-validator v1alpha1 レビュー修正報告

2026-11-06 JST。レビュー指摘1〜5を修正した。MAN-03の附属資料分割には未解決の
仕様判断があるため、全面的な実装完了は主張しない。引き継ぎ採用後の再レビューを完了し、初期実装としてローカルcommit済み（`5ee3472`、ブランチ`feat/initial-v1alpha1`）。push・PR作成は未実施。

## 修正差分

1. `manifest.rs`: law.yamlを含む共有ReadBudgetで正常/不正を問わず読込量を計上。
   メタデータで読込前に拒否し、ストリームも残量とファイル上限に最大1バイトの
   超過検出用先読みを加えた量に制限。超過ファイルを保持せず、後続読込を止める。
   非UTF-8/BOM/NUL/単独CR、部分IO失敗、無限reader、実128 MiB制限の回帰検証を追加。
2. `markdown.rs` / `syntax.rs`: CommonMarkの文書全体のイベントで見出し装飾を判定。
   通常文字としての角括弧等を受理し、同一ファイルの参照定義で成立するリンクは拒否。
3. `syntax.rs`: 規範EBNFに従い、条captionの閉じ括弧直前の空白を拒否。
4. `syntax.rs`: 同scopeの後続本則・附則ファイルについて、最初の非空白内容が
   条または分類見出しであることを検証。MD-03の区切り空行は許す。
5. `v1alpha1_conformance.rs`: status/exit_code/completeと期待診断を検査。
   Incomplete・予期しないErrorは成功集計しない。T058は固定入力loj/v9とVAL-01の
   処理不能規定を根拠とし、専用unsupported-version診断を伴うErrorだけを期待する。
   これはランナーの状態契約であり、パーサーのfixture別例外ではない。

各指摘に独立した回帰テストを追加（計9関数）。引き継ぎ後、混在入力の上限ちょうど/超過、
仕様保留と違反/IOエラーの併存、参照定義のファイル分離の3関数を追加した。既存の附属資料分割テストは、
合格を固定するテストから、仕様判断待ちを成功扱いしないテストへ改めた。
旧API・旧CLI・既存プロファイルは今回変更していない。

## 検証結果

| 検証 | 結果 |
|---|---|
| cargo fmt --all -- --check | 成功・終了0 |
| cargo clippy --workspace --all-targets --locked -- -D warnings | 成功・終了0 |
| cargo test --workspace --locked | Windowsで60関数成功、失敗0、未実行0 |
| 既存機能 | 従来33関数成功 |
| 正式適合 | L0 11/11、L1 63/63、計74/74成功、失敗0、仕様判断待ち0 |
| 参考L2 | T026を未実行、成功数に含めない |
| 正式入力に対する検証器状態 | Passed 31 / Failed 42 / Error 1 / Incomplete 0 |
| AST・位置 | 31正例の固定AST完全一致、指定位置一致、65本文要素の位置復元一致 |
| 固定仕様 | commit b2356c6f113f3a02a1a790dc5cf3813d82cf3fe0、243ファイルのhashと隣接仕様が一致 |

適合成功74件と入力のPassed 31件を区別する。T058のErrorは期待された拒否である。
Windowsのリンク作成制限を避けるため最終全件テストはsandbox外で実行した。
Clippyの増分キャッシュ用ハードリンク不可はコピー代替警告で、検査は終了0。

[詳細と全ケース表](v1alpha1-conformance.md)、[実出力JSON](v1alpha1-conformance.json)、
[機械可読集計](review-verification.json)、[実行ログ](cargo-test.log)を保存した。

## 仕様フィードバックと既知の制限

- 既存のOpen/Closed Issueを検索し、#1だけが存在し、重複しないことを確認した。
- ユーザーの許可に基づき、[loj-spec #2](https://github.com/law-of-japan/loj-spec/issues/2)を作成。
  MAN-03の附属資料の後続ファイル開始条件と、フェンス/HTMLのEOF完結条件の明確化を依頼。
  規範文・両EBNF・適合データを照合した根拠と再現入力は[提出本文](spec-issue-man03-appendix-splitting.md)に保存。
- #2が未解決のため、複数ファイル附属資料のL1はIncomplete・終了3・完全ASTなし。
  `loj/v1alpha1/specification-pending`で識別できる。通常本文分割・閉鎖済みフェンス・未閉鎖フェンスの
  3変種について、この保留方針をテストした。3変種の規範適合を確定したという意味ではない。
  L0と単一ファイル附属資料の検証は継続できる。
- [#1](https://github.com/law-of-japan/loj-spec/issues/1)のL0正例・負例診断位置の正式カバレッジ不足もOpen。
- loj-specの仕様・スキーマ・固定適合テストとvendorは変更していない。
- Linux/Rust 1.99.1 stableとLinux/Rust 1.88.0は隔離Dockerで各60関数成功、正式74ケースも成功。
  macOSは利用環境がなく未実行。リモートActionsを起動するpush/PRも行っていない。
- 継続fuzzing、ピークメモリ、同時ファイル差替えへの保証は未実施。入力バイト上限は
  JSON・AST・複製を含むプロセスメモリ上限ではない。既存性能レポートは修正前の参考値で再測定していない。

## 次に必要な作業

修正差分の再レビューと利用可能なWindows/Linux/MSRV検証は完了。macOSを含むリモートCIはpush承認後に実行する。#2の仕様判断後に、独立した正式期待値を
仕様側で整備し、複数ファイル附属資料の保留を解除できるか判断する。#1のカバレッジ改善も継続する。
現状を保存する場合も、仕様判断待ちと未実行環境を残した状態であり「全仕様の実装完了」とは扱わない。

引き継ぎ方針・再開手順は[作業記録](../docs/codex-progress.md)、#2の選択肢と推奨B案は[仕様判断案](../docs/man03-resolution-proposal.md)参照。いずれも仕様変更そのものではない。
