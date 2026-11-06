# v1alpha1 適合検証結果（レビュー修正後）

- 記録日: 2026-11-06 JST
- 固定仕様: `b2356c6f113f3a02a1a790dc5cf3813d82cf3fe0`、loj/v1alpha1
- 実行環境: Windows x86_64、rustc/cargo 1.99.1、debug build
- vendorの243ファイルをSHA-256検査し、隣接loj-specとも全件一致。規範・期待値は変更していない。

## 適合テストの成否

| 対象 | 実行 | 成功 | 失敗 | 未実行 | 仕様判断待ち |
|---|---:|---:|---:|---:|---:|
| 正式L0 | 11 | 11 | 0 | 0 | 0 |
| 正式L1 | 63 | 63 | 0 | 0 | 0 |
| 正式合計 | 74 | 74 | 0 | 0 | 0 |
| 参考L2 T026 | 0 | 0 | 0 | 1 | 0 |

## 検証器が返した状態（正式74入力）

| 状態 | 件数 |
|---|---:|
| Passed | 31 |
| Failed | 42 |
| Error | 1 |
| Incomplete | 0 |

適合テストの成功74件は、入力の合格74件を意味しない。
31正例は固定ASTと完全一致、指定ノード位置も一致。正例の全65本文要素について、
実出力のバイト位置列から原文を復元し、textとの一致も独立確認した。
通常負例はFailed/終了1/complete=true/ASTなしと期待ruleを要求する。
T058だけは固定入力がloj/v9、固定期待ruleがBASIC-01であり、VAL-01の未対応versionの
処理不能規定に基づきError/終了2/complete=false/ASTなしを要求する。
専用コード`loj/v1alpha1/unsupported-version`の診断のみを許し、IO・internalエラーを代用できない。
Incomplete、予期しないError、仕様判断待ちは適合成功に集計しない。

## 実行と回帰テスト

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
$env:LOJ_CONFORMANCE_REPORT = Join-Path (Get-Location) 'reports/v1alpha1-conformance.json'
cargo test --workspace --locked
```

全コマンド終了0。Rustテスト60関数成功、失敗・ignored・filtered outは0。
既存33関数を維持し、レビュー指摘向けに9関数、引き継ぎ後の境界確認に3関数を追加した。
この60関数のうち1関数が正式74ケースを実行するため、件数を加算しない。
Windowsリンク試験はsandbox内で権限が制限されるため、最終全件実行はsandbox外で行った。
Clippyではsandboxの増分キャッシュのハードリンク不可・コピー代替警告が出たが終了0。
ログ: [fmt](fmt.log)、[clippy](clippy.log)、[cargo test](cargo-test.log)。
実出力: [正式全ケースJSON](v1alpha1-conformance.json)、[検証集計JSON](review-verification.json)。

| 指摘 | 独立した回帰検証 |
|---|---|
| 1 | 不正UTF-8/BOM/NUL/単独CRの総量算入、メタデータによる読込前拒否、無限readerの上限、部分IO失敗の算入、128 MiB実上限と後続読込停止 |
| 2 | 通常文字の角括弧・アスタリスク・非閉じハッシュ等を受理、装飾と同一ファイルの参照リンクを拒否 |
| 3 | caption末尾のASCII空白・タブ・全角空白・NBSPを拒否、内部空白を受理 |
| 4 | 本則・附則の空白だけ/コメント先行/マーカー継続をMAN-03拒否、条/分類開始を受理 |
| 5 | 期待ruleがあってもIncomplete・予期しないErrorを拒否、T058の原因診断を限定 |

## 仕様判断待ち（正式ケース外）

[loj-spec #2](https://github.com/law-of-japan/loj-spec/issues/2)は未解決。
複数ファイル附属資料の開始条件とEOFでのブロック完結条件が対象である。
現実装はL1でIncomplete/終了3/完全ASTなし、専用のspecification-pending診断を返す。
局所テストは通常本文分割・明示閉鎖フェンス・未閉鎖フェンスの3変種で、この保留状態を確認する。
そのテスト成功を、3入力の仕様適合確定として数えない。L0と単一ファイル附属資料は引き続き検証可能。
[#1](https://github.com/law-of-japan/loj-spec/issues/1)のL0正例・診断位置カバレッジ提案もOpen。

## 未実行と限界

| 環境 | 状態 |
|---|---|
| Windows / Rust 1.99.1 | 全60関数を実測 |
| Linux / stable (Rust 1.99.1) | Docker内で全60関数・正式74ケース成功 |
| macOS / stable | CI構成済み、未実行 |
| Linux / Rust 1.88.0 (MSRV) | Docker内で全60関数・正式74ケース成功 |

macOS実行環境がないためmacOSのみ未実行。Linuxは既存Docker上に隔離コンテナを作成し実測した。
ソースは読取専用mountからコンテナ内へコピーし、Unixのhardlink/symlink試験も実行した。
Linuxの2種類の正式実出力JSONはWindowsの出力と完全一致した。
[MSRVログ](linux-msrv.log)、[stableログ](linux-stable.log)、
[MSRV実出力](linux-msrv-conformance.json)、[stable実出力](linux-stable-conformance.json)。
再現スクリプト: `scripts/verify-linux.sh`。イメージ: `rust@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0`。
`/source`へリポジトリをread-only、`/results`へ結果用ディレクトリをmountしてbashで起動する。
stableは実行時にcontainer内だけへ導入し、実測バージョンをログへ記録する。
push・PR作成は行っておらず、リモートActionsの成功を主張しない。
継続fuzzing・ピークメモリ・同時ファイル差替え耐性は未検証。
既存ベンチマークJSONは修正前の参考測定で、今回再実行していない。
入力バイト上限とJSON/ASTを含むプロセスメモリは別物である。

## 全ケース結果

| ID | レベル | 適合テスト結果 | 検証器状態 |
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
