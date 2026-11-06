# 設計判断

作成日：2026-11-06

## 出所（provenance）

`crates/law-semantic` は、所有者のプロトタイプ `markdown-schema-evaluation/evaluations/semantic-layer`（`src/{main,mapping,number,references}.rs`、`schema.json`、`profiles/*.json`、`cases/`）を移行したもの。評価リポジトリは出所の記録であり、実行時・ビルド時・試験時の依存ではない。評価リポジトリは変更していない。

移行時の対応：

| プロトタイプ | 本リポジトリ | 変更 |
|---|---|---|
| `src/number.rs` | `crates/law-semantic/src/number.rs` | なし |
| `src/references.rs` | 同 `references.rs` | `resolve`・`coverage` を crate 内公開に変更（外部へは入力検査付きの `resolve_references`・`reference_coverage` を公開） |
| `src/mapping.rs` | 同 `mapping.rs` | 正規表現の名前付きグループ取得を `group()` 経由に（任意グループでのパニック防止）。`main.rs` の `map-corpus` 統合処理を `merge()` として移動 |
| `src/main.rs` の `validate`・`Checks` | 同 `validation.rs` | 処理は同一。渡されたスキーマが組込み契約より緩い場合は関係検査に進まず `Err` |
| `src/main.rs` のCLI | `crates/loj-validator` | 新しい `check` コマンドに置換 |
| `schema.json`、`profiles/*.json`、`cases/*.json` | `crates/law-semantic/` 以下 | 内容は同一 |
| `check_cases.py`、`check_mapping_boundaries.py`、`check_references.py` | `crates/law-semantic/tests/*.rs` | 同じ入力と期待値をRust試験へ移植 |

移植しなかったもの：実データコーパス・公的XML派生・GitHub表示HTMLとの照合（`check_mapping.py`、`check_egov_derived.py`、`check_rendered_anchors.py`）とsemantic-md比較。公的データや取得物を本リポジトリへ複製しないため。これらの結果は評価リポジトリの記録として残る。

## 構成

- 意味層（`law-semantic`）と実行層（`loj-validator`）を別crateに分けた。意味層はCLIや入出力に依存せず、マッピング（`mapping`・`number`）と検証（`validation`・`references`）をモジュールで分ける。
- 構造検証は自作せず、上流 `schematter-lib` の公開API（`validate`、`Violation`、`SchemaError`）を使う。crates.ioに0.2.0が公開されていたため `=0.2.0` に固定した（固定gitリビジョン `8cf557b62ff0a244b81246fd6bc9fc51077e303d` の代替は不要だった）。vendorや絶対パス依存は使わない。`pulldown-cmark` と `jsonschema` は上流と同じ系列（0.13.4、0.47）に揃え、Cargo.lockで単一版に解決される。
- 新しい汎用スキーマ言語は作らない。構造は既存のdocument-schema、意味はJSON Schemaと法令固有の関係検査で表す（プロトタイプの方針を維持）。

## 実行の契約

- 両層は独立に実行する。構造層の不合格や設定エラーで意味層を省略しない。構造層だけでは意味ASTが得られず、見出しレベルの正規化など両層の解釈も異なるため。
- 終了コードは 0（合格）、1（検証違反）、2（ツール・設定エラー）。一方の層でも設定エラーなら2を優先する。検査が完了していないため。
- 意味層は、意味ASTのスキーマ違反、関係制約違反、参照エラー、マッピングの未対応（`UNMAPPED`・`AMBIGUOUS`・`INCOMPLETE` 等）が1つでもあれば不合格。
- 範囲外参照（外部URL・別ファイル）は、プロトタイプと同じく警告とし不合格にはしない。代わりに `reference_coverage.complete: false` と警告を必ず出力し、text出力でも `INCOMPLETE` と明示する。範囲外参照を不合格にするオプションは今回追加していない。
- 診断はプロトタイプのフィールドを変えず、`layer`・`stage` を加えるのみ。schematterの診断は `breadcrumb`・`schema_pointer`・`keyword` を保ち、原文位置は推測で付けない。
- 既定値：プロファイルは組込み `law-of-japan`、`--law-id` は固定値 `law`（ファイル名からの推測は、law-of-japanの `article.md` 等で識別子にならないため採らない）。構造スキーマは暗黙に選ばず必須引数とした。
- JSON出力は標準出力に1文書のみ。想定外のパニックも `catch_unwind` でJSONの内部エラー報告（終了コード2）にする。

## 既定の構造スキーマ

`schemas/law-of-japan.structure.yaml` は組込みプロファイルが受理する形（題名1つ、章は任意、章のない法令では条が題名直下、附則は任意）に合わせた最小限のもの。項・号の書式や番号の相関は意味層が担うため、構造スキーマでは制約しない。

## ライセンス

プロジェクトのライセンスは所有者が決める。全crateを `publish = false` とし、`license` フィールドやライセンスファイルは置いていない。上流（schematter、Apache-2.0）の帰属はREADMEに記載した。
