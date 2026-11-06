# 現在の作業状態（B案承認後）

2026-11-07、ユーザーがpush・PR・mergeを承認。仕様PR #4はCI成功後にmerge済み。
統合commit b704631b6501a0a01c1eb6faf99cfe16a4bccfa9へvalidatorの固定参照を更新し、
vendor393ファイルが統合後のGit blobと一致することを確認した。Schema・正式期待値に変更なし。
再検証と既存validator PR #1のCI確認後、同PRをmergeする。Issue #1は保留のまま。
以下の「merge未承認」「固定参照は統合時に更新」は以前の記録であり、この追記を優先する。

最新判断（2026-11-07）: Issue #1のL0正例契約は保留。L0〜L4の定義表を仕様へ明記し、
各レベルは独立とする。数字の大きいレベルの合格から小さいレベルの合格を推論しない。
以前の「L0ケースのL1継承テストを追加すべき」というレビュー指摘とmerge前必須提案を撤回。
ランナーのcase.level別実行を維持する。208回の交差検証は観測結果としてのみ保存する。
仕様訂正はspec/independent-conformance-levelsブランチで作業。新規PR・mergeは今回行わない。
validatorの固定仕様commitは仕様訂正の統合時に更新する。以下の以前の指摘はこの判断で読み替える。

定義整合性の確認結果: [level-consistency-review.md](../reports/level-consistency-review.md)。
正式104入力をL0/L1両方で208回実行。レベル入替え・L2以降の偽成功は未検出。
CONF-01のL0正例AST要求、定義表の欠落、L0ケースのL1継承テストの不足を指摘。
現行CI成功をもって定義・契約の完全整合とはせず、仕様契約/説明と継続検証の補強を推奨する。
仕様・コード・正式期待値・PRは今回変更していない。

2026-11-07の最新訂正: プロジェクトのL0〜L4定義をユーザーが明示した。
正確な定義と現行実装との照合は[conformance-levels.md](conformance-levels.md)を参照。
レベルの意味を現行v1alpha1の実装範囲で狭めない。特にL0のMarkdown構文、L2のファイル間参照、
L3の施行時点、L4の他法令参照を引き継ぐ。仕様・コードのレベル配分をこの訂正だけで黙って変更しない。
仕様PR #3はmerge済み（3ea7a60）。固定6295fd7はmainの祖先でツリー一致。
validator PR #1はOpen、head9b8ea1fのCIは全成功。これは固定仕様への検証結果である。

2026-11-07追記: ユーザーが両リポジトリのPR作成を承認した。結果記録を含むブランチをpushし、
loj-specのB案PRとloj-validatorの初期実装PRをmain向けに作成する。merge・Releaseは未承認。
以下の「PR未承認」「結果記録はローカルのみ」はこの追記以前の状態である。

ユーザーの最新指示「CIを利用してよい。#2はB案で進めてよい。」を適用。
CI用の専用ブランチpushとB案の仕様変更は承認済み。main push・PR・merge・Releaseは未承認。
Codex内で判断を完結し、旧handoffの承認待ちを再質問しない。

- 初期実装5ee3472、記録5a71b00はfeat/initial-v1alpha1へpush済み。
- 初期CI 42232317522はWindows/Linux/macOS stable、Linux1.88の4ジョブ成功。
- loj-specはspec/man03-appendix-boundariesの6295fd7769a09eeb0da56bcc6ff8c404ec391db6へcommit/push済み。
  MAN-03・EBNF注記・DDR・CHANGELOG、独立T076〜T105を追加。Schema/旧75定義は不変。
  verify.py成功（105定義、47正例source/tree、11 L0負例）、unittest17成功。
  CI 42234173859も成功。
- validatorの固定vendor393ファイルは上記spec commitのGit blobと全件一致。
  非最終附属資料のCommonMarkフェンス/HTML明示終端、後続の非空白本文を実装し、一律Incompleteを解除。
  フェンス/HTML独立回帰2関数追加、従来の保留テストを採用規範へ更新。
- Windowsローカルfmt/Clippy/全62テスト成功。正式104/104（L0 11,L1 93）、旧74も成功。
  実状態Passed47/Failed56/Error1/Incomplete0。47AST完全一致、97本文位置復元一致。
  T058の期待Errorは専用原因だけを許可。参考L2 T026は未実行。
- 設計・利用ガイド・適合結果・仕様フィードバックを更新。旧Docker証跡は旧版74ケースの履歴。
- B案実装は1a53cf48884d1e9961830b494e15aa51684ceea9へcommit/push済み。
  追加レビューでHTML終了タグのUnicode case-foldによる誤認を発見。CommonMarkのASCII比較に限定し、
  `</ſcript>`を拒否する独立入力を既存HTML回帰へ追加した。仕様の問題ではなく実装の不具合。
- ASCII比較修正4599a6971975ae5ce4e254b1786ae964c6bdb79cをcommit/push済み。
  最終CI 42235465039は4ジョブ成功。各ログで62テスト・正式104ケースのランナー成功を確認。
  Windows/Linux/macOSはRust1.99.1、Linux MSRVは1.88.0。要求matrixに未実行はない。
  Windows/macOSのMSRV組合せは対象外で未実行。前run42235102010は新pushでcancelled、成功扱いしない。
- CI証跡はreports/validator-ci.jsonとreports/spec-ci.json、集計はreview-verification.json。
  結果を記録する文書だけの後続commitはローカル保存し、不要なCI再起動はしない。
- 今回の作業は完了。次はユーザーの明示承認がある場合だけPR/merge等の統合へ進む。
- #1カバレッジ課題はOpen。#2は承認・実装済みだが統合前でOpen。Issueの投稿/closeはしていない。
- 継続fuzzing、ピークメモリ、同時ファイル差替え、旧ベンチマーク再測定は未実施。

以下は前段階の履歴であり、承認状況・件数・保留条件は上記が優先する。

---

# Codex作業記録

## 現在の目的と承認境界

2026-11-06 JST。ユーザーの明示指示によりDesktopのLOJ-CODEX-HANDOFF.mdを採用し、
原文を[codex-handoff.md](codex-handoff.md)へ保存。従来のcommit停止は本指示で置換された。
通常修正・検証・文書更新と、基準を満たしたローカルcommitは許可されている。
push・PR/merge・正式タグ/Release・loj-spec規範変更・法令一括変更・旧形式削除は別途承認。
判断と相談はCodex内で完結し、ChatGPTへの問い合わせや転記を要求しない。

## 照合した基準と変更範囲

- 開始時validator HEAD: 6dbd0b97e3d1d669d6e4949db1876e2a41b2c966。
- spec HEAD/固定commit: b2356c6f113f3a02a1a790dc5cf3813d82cf3fe0、clean。
- vendor243ファイルは隣接specとSHA-256全件一致。仕様・Schema・正式期待値を変更していない。
- 既存AGENTS.mdと継続文脈ファイルがなかったため作成。独立した元作業依頼書は見つからず、
  引き継ぎ文書とセッションの明示指示を作業範囲の根拠とした。
- 実装・CLI・CI・Cargo差分・9回帰テスト・設計書・実測ログ・履歴を再レビュー。
  5修正に重大な未解決実装不具合は認めず、実装コードの追加修正は不要と判断した。
- 引き継ぎ後は正常/不正混在の予算ちょうど/超過、保留と違反/IO不能の併存、
  参照定義がファイルを越えないことの3テストを追加。文書・ログ・Linux検証スクリプトを更新。

## 最新実測（開始時の57件を更新）

| 環境 | Rustテスト関数 | 正式L0/L1 | 状態 |
|---|---:|---:|---|
| Windows / Rust 1.99.1 | 60成功、0失敗 | 11+63=74成功 | 実行済み |
| Linux Docker / Rust 1.99.1 stable | 60成功、0失敗 | 74成功 | 実行済み |
| Linux Docker / Rust 1.88.0 MSRV | 60成功、0失敗 | 74成功 | 実行済み |
| macOS / stable | 未実行 | 未実行 | 利用環境なし、CIで確認予定 |

正式入力の検証器状態は各環境Passed31 / Failed42 / Error1 / Incomplete0。
正式ケースに仕様判断待ち0。参考T026は未実行。60関数のうち1関数が正式74ケースを検査する。
旧機能33関数を含む。3環境の正式実出力JSONは完全一致、31正例AST一致、65本文位置復元一致。

実行コマンド：`cargo fmt --all -- --check`、
`cargo clippy --workspace --all-targets --locked -- -D warnings`、
`cargo test --workspace --locked`は終了0。ログはreportsへ保存。
Windowsリンク試験は既に利用可能な通常権限環境で実行。sandboxの権限失敗を成功と数えていない。
WSL Ubuntuは実在したがRust未導入。Docker daemonは既存の29.8.2を利用。
公式Rustイメージ `rust@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0`
を取得し、読取専用/sourceと結果用/resultsだけをmount、scripts/verify-linux.shで検証した。
コンテナ内のnative filesystemへコピーしてUnixリンク試験を実行。コンテナは終了後自動削除。
stable導入はcontainer内のみ。ホストRust設定と他の稼働コンテナは変更していない。

## 仕様判断と保留の再レビュー

GitHubから#1/#2の現在の本文を取得し、いずれもOpenと確認した。
#1はL0成功期待と診断位置カバレッジ不足。#2は附属資料分割条件。
[選択肢と推奨B案](man03-resolution-proposal.md)を準備したが、採用・仕様変更は未承認。

#2解決前は全複数ファイル附属資料のL1を保留する。通常段落や明示閉鎖済みブロックも含むが、
未確定の規則で一部だけを合格にすることを避ける初期実装の制限である。
状態優先順位Error > Incomplete > Failed > Passedを実測。保留と併存する既知違反は診断に残り、
IO不能があればErrorが優先される。L0と単一ファイル附属資料は影響を受けない。
これは「全分割が仕様違反」という判定ではない。完全ASTは返さない。

## commit前評価と履歴の参照

実行可能な必須チェックが成功し、重大な未解決実装不具合はなく、制限を文書化できたため、
初期実装のローカルcommit基準を満たすと判断した。
対象はv1alpha1実装・互換CLI・固定vendor・テスト・CI・文書/証跡に限定。targetは含めない。
固定vendorには意図的なCRLF・不正空白の標本があるため、改行変換とGit空白警告だけを除外し、
代わりにステージ済みblobの243 SHA-256一致を検査する。実装側の空白検査は維持する。
ローカルブランチfeat/initial-v1alpha1に初期実装をcommit済み。
実装commit: `5ee3472fa972d82b6fff898323624fc8305c6df9`
件名: `feat: add initial loj-spec v1alpha1 validation`。
commit後に作業ツリーcleanとspec側cleanを確認した。本段落の完了記録のみを後続のdocs commitに保存する。
全面準拠・正式リリースとは表現しない。

## 次の具体的作業・承認待ち

1. ユーザーがpushを承認した場合のみ、ローカルのfeat/initial-v1alpha1をoriginの同名ブランチへpushする。
   mainへはpushせずPRも作らない。既存CIのWindows/Linux/macOS stableとLinux1.88を追跡し、
   macOSの未実行を解消する。失敗があれば原因別に修正・再検証する。
2. #2の推奨B案を採用して規範を変更するか、ユーザーにCodex内で判断を求める。
   承認があるまでloj-spec側の規範・Schema・固定期待値を変更せず保留を維持する。
3. 仕様側で独立した期待値が確定した後、固定commit更新とvalidator対応を別作業として行う。

macOS、継続fuzzing、ピークメモリ、同時ファイル差替え耐性は未検証。
元入力バイト制限をプロセスメモリ上限と混同しない。旧ベンチマークは今回再測定していない。
