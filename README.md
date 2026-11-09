# loj-validator

適合レベルは検証対象ごとに独立しています。L1等の合格は、番号の小さいレベルの合格を保証しません。
L0〜L4の定義と現行仕様との対応は[適合レベル定義](docs/conformance-levels.md)を参照してください。

日本の法令Markdown（law-of-japan形式）を、**構造**と**意味**の2層で検証するRust製CLIとライブラリ。

- 構造層: 上流の [schematter](https://github.com/iwe-org/schematter)（`schematter-lib` 0.2.0）でdocument-schemaに照らし、正規化した見出し木とブロックを検査する。
- 意味層: 本リポジトリの `law-semantic` が、Markdownを宣言的プロファイルで意味AST（Law / Chapter / Article / Paragraph / Item / Supplement）へ写像し、JSON Schema・関係制約・同一ファイル内参照を検査する。

状態: 初期実装。crates.ioへは公開しない（全crateが `publish = false`）。

## loj-spec v1alpha1（Manifest形式）

`law.yaml`と列挙されたMarkdownを、固定仕様commit
`b704631b6501a0a01c1eb6faf99cfe16a4bccfa9`のL0/L1で検証します。

```sh
loj-validator check .
loj-validator check law.yaml --level L0
loj-validator check . --level L1 --format json --strict
loj-validator check path/to/manifest.yaml --profile v1alpha1
loj-validator check legacy.md --profile legacy --structure-schema schemas/law-of-japan.structure.yaml
```

ディレクトリまたは`law.yaml`は新形式を自動選択します。既定はL1。
`--profile v1alpha1`で明示選択できます。新形式では法令IDと構造をManifestから取得するため、
`--law-id`と`--structure-schema`は併用できません。既存のプロファイルJSON指定と旧APIは維持しています。
以下の従来の「使い方」はlegacy形式の説明です。

| 新形式の状態 | 終了コード | 意味 |
|---|---:|---|
| Passed | 0 | 指定レベルの検査に違反なし |
| Failed | 1 | 仕様違反を検出 |
| Error | 2 | 読込不能・未対応version等 |
| Incomplete | 3 | 入力サイズ等の実装上限により検査未完了 |

`--strict`の有無によらずIncompleteは非ゼロです。引数エラーは従来のエラー形式・終了コード2です。
JSONの`ast`はL1合格時のみ完全な規範ASTを返し、L0または不合格時はnullです。
`diagnostics[].rule`は規範要件ID、`code`は実装の診断コードです。例えば`DISP-01`は表示番号の不整合です。
`file`と`span.start/end`は元ファイルのUTF-8バイト半開区間で、日本語・絵文字・CRLFを含めて数えます。
`sources`は元入力、`sourceMap`はIDごとのノード位置列、`positions`はノード位置と本文片の対応です。
失敗時の位置は回復過程の情報であり、完全ASTの存在を示しません。

複数ファイル附属資料はMAN-03に対応しています。後続ファイルは空白以外のMarkdown本文で開始でき、条見出しは不要です。
非最終ファイルのフェンスコードとHTMLブロックには明示終端が必要です（HTML種類6〜7は空行）。
最終ファイルは通常のCommonMark EOF終端を許します。違反はL1でMAN-03/Failedとなり、L0ではこの意味検査を行いません。

入力は正常・不正の別なく16 MiB/ファイル、law.yamlを含む128 MiB総量で制限します。
読込前にサイズを確認し、ストリーム読込にも残量上限を適用します。
ファイル伸長の検出には最大1バイトの先読みを許し、超過入力を保持せず読込を停止します。
保持する元入力バイト量の上限であり、JSONやASTを含むプロセスメモリの上限ではありません。

v1alpha1の正式適合データはライセンス・commit・SHA-256一覧とともに`vendor/loj-spec-v1alpha1`へ、
次期仕様（`loj/v1alpha2`ほか）の`spec`・`schemas`・`conformance`は`vendor/loj-spec`へ固定し、
`cargo test --workspace --locked`で全正式ケースを検査します。
個別実行は`cargo test -p law-semantic --test v1alpha1_conformance --locked`です。
`LOJ_CONFORMANCE_REPORT`に絶対パスを設定すると、ケース別JSON結果を保存します。

[設計と制限](docs/v1alpha1-implementation.md)を参照してください。
次期仕様（`loj/v1alpha2`、履歴マニフェスト、字体注記、登録台帳、再構築報告、L3・L4、変更・参照の検査）は
[次期仕様の実装](docs/next-spec-implementation.md)を参照してください。
`law.yaml`とディレクトリの既定は次期仕様で、v1alpha1は`--profile v1alpha1`で選びます。
次期仕様の適合ケースは`cargo test -p law-semantic --test next_conformance --locked`で実行し、
`LOJ_CONFORMANCE_REPORT_DIR`にディレクトリを設定すると、suiteごとのJSON結果を保存します。
固定テストの成功は、未知の全入力への完全適合や法的正確性の証明ではありません。

## 使い方

```powershell
cargo build --locked --release
target/release/loj-validator check examples/chaptered-law.md --structure-schema schemas/law-of-japan.structure.yaml
target/release/loj-validator check examples/simple-law.md --structure-schema schemas/law-of-japan.structure.yaml --format json
```

ビルドせずに実行する場合：

```powershell
cargo run --locked -p loj-validator -- check INPUT.md --structure-schema schemas/law-of-japan.structure.yaml
```

書式：

```text
loj-validator check INPUT.md --structure-schema PATH [--profile PATH] [--law-id ID] [--format json|text]
```

| オプション | 既定値 | 説明 |
|---|---|---|
| `--structure-schema PATH` | なし（必須） | schematterのdocument-schema（YAML/JSON） |
| `--profile PATH` | 組込み `law-of-japan` | 意味層のマッピングプロファイル（JSON） |
| `--law-id ID` | `law` | 意味ASTの法令識別子。ノードIDは `law/article/1` のようになる |
| `--format json\|text` | `text` | 出力形式 |

`--law-id` の既定値はファイル名などから推測せず、固定値 `law` とする。結果を複数ファイルで比較・統合するときは明示的に指定する。

### 終了コード

| コード | 意味 |
|---|---|
| 0 | 両層とも合格 |
| 1 | 検証違反（どちらかの層が不合格） |
| 2 | ツール・設定エラー（引数誤り、入力が読めない・UTF-8でない、構造スキーマや プロファイルが壊れている等） |

入力が読める限り、**両層を常に実行する**。構造層が不合格でも、構造スキーマが壊れていても、意味層は実行され報告される（逆も同様）。設定エラーが一方にあれば全体は2になる。

### 出力

`--format json` では標準出力にJSONを1つだけ出す（ログは出さない）。引数誤り・入力読込失敗・設定エラーでもJSONとして解析できる。主なフィールド：

- `status`（`passed` / `failed` / `error`）、`exit_code`、`input`、`law_id`、`errors`（実行前エラー）、`summary`（error/warning件数）
- `layers.structure`: `status`、`engine`、`schema`、`diagnostics`、`errors`
  - 診断は `layer: "structure"`、`code: "STRUCTURE_VIOLATION"`、`keyword`、`message`、`hint`、`breadcrumb`、`schema_pointer`。schematterは原文位置を返さないため、原文範囲は付けない。
- `layers.semantic`: `status`、`profile`、`ast_version`、`mapping_complete`、`nodes`（種別ごとのノード数）、`reference_coverage`、`diagnostics`、`errors`
  - 診断はプロトタイプの形式（`code`、`severity`、`message`、`rule_id`、`node_id`、`primary_span`、`related_spans`、スキーマ違反では `instance_path`）をそのまま保ち、`layer: "semantic"` と `stage`（`mapping` / `schema` / `relational` / `reference`、`rule_id` から導出）を加える。
  - `primary_span` はUTF-8バイト範囲。text出力では行:列（文字単位）も表示する。

### 参照の扱い

対象は同一ファイル内のフラグメントリンク（`[第二条](#第二条)`）のみ。アンカー名はGitHubの基本的な見出しスラグ規則で生成する。参照先の存在・一意性・型（既定はArticle）・ラベルの条番号と参照先番号の一致を検査する。

外部URL・別ファイルへのリンクは検証しない。`REF_OUT_OF_SCOPE` 警告として診断に出し、`reference_coverage` の `out_of_scope` に数え、`complete: false` とする。この場合も終了コードは0になり得るが、「全参照の検証に合格した」ことは意味しない。text出力にも `INCOMPLETE` と表示する。意味ASTがスキーマ不適合の場合、`reference_coverage` は `null`（未評価）。

## 構成

```text
Cargo.toml                         ワークスペース（Cargo.lockで依存を固定）
crates/law-semantic/               意味層ライブラリ
  schema.json                      意味ASTのJSON Schema（2020-12、version 0.1）
  profiles/*.json                  マッピングプロファイル（law-of-japan、paragraph-headings、egov-derived-bare）
  src/mapping.rs, number.rs        Markdown→意味AST、番号正規化（マッピング）
  src/validation.rs, references.rs スキーマ・関係制約・参照解決（検証）
  tests/                           意味AST 28例、マッピング境界、参照、API
crates/loj-validator/              2層の実行とCLI
  src/lib.rs                       check / check_structure / check_semantic / 報告生成
  src/main.rs                      引数解析と出力のみ
  tests/cli.rs                     CLI統合試験
schemas/law-of-japan.structure.yaml  組込みプロファイルに合わせた既定の構造スキーマ
examples/                          架空法令の合成例
docs/                              適合レベル・実装設計と制限
```

`law-semantic` の主な公開API：

- `map_markdown(source, path, law_id, profile)` — Markdown→意味AST。`Err` はプロファイル設定エラー。Markdown側の問題はASTの `diagnostics` に入る。
- `validate(schema, data)` / `validate_document(data)` — スキーマ検査と関係検査。違反の一覧を返す。
- `resolve_references(&mut data)` — 参照解決状態を再計算してASTに書き込む。
- `reference_coverage(data)` — 参照カバレッジ。
- `normalize_number(s)`、`mapping::merge(parts)`（同一法令の複数ファイル統合）、`references::slug`。
- 定数 `SEMANTIC_SCHEMA`、`LAW_OF_JAPAN_PROFILE` ほか。

AST全体を走査する関数は入力がスキーマに適合しない場合 `Err` を返し、パニックしない。

## 既定の構造スキーマ

`schemas/law-of-japan.structure.yaml` は組込みプロファイルに対応する最小限のスキーマ：

- 法令題名（`#`）が1つ。題名より前の本文は不可。
- 題名の下に、任意の章（`第N章`）、章のない場合の条（`第N条`、`第N条のM`、`（見出し）` 付き可）、任意の附則（`附則`）。
- 章の下は条のみ。条は本文ブロック（段落、`1.` 形式の項リスト、`- 一　` 形式の号リスト、引用、表、コード）を1つ以上持ち、下位見出しを持たない。
- 附則は直接の本文、または条を持つ。

schematterは見出しレベルを正規化する（`#` の直後の `#####` は子扱い）。生の見出しレベル（プロファイルの `levels`）は意味層が検査するため、`#####` の条は構造層で合格し意味層で `UNMAPPED` となる。

## テスト

```powershell
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- `law-semantic`: プロトタイプの単体試験4件（番号正規化の表形式試験2件、スラグ、フラグメント）、意味AST 28例（期待診断コード集合の完全一致）、マッピング境界17例と2表現の同等性、参照12仕様×2表現と対の同等性・アンカー重複・ソース分離、API・不正設定の試験。
- `loj-validator`: 例の両層合格、構造のみ不合格でも意味層を実行、構造を緩めた場合の意味層の項欠落・重複・リンク切れ検出、両層不合格、見出しレベル正規化の不一致、範囲外参照のカバレッジ表示、壊れたYAML/JSON/プロファイル/ファイルでの終了コード2とJSON出力、引数誤り。

試験の入力はすべて合成例。公的な法令XML/HTML、取得済みアーカイブ、評価結果の全文は含めていない。

## 対応範囲と制限

- 1回の実行で1ファイル。複数ファイルの統合は `mapping::merge` をライブラリとして提供するが、CLIにはない。
- 参照は同一ファイルのフラグメントリンクのみ。「前条」「同項」等の暗黙・相対参照、他法令・別ファイル・改正時点をまたぐ参照は対象外。
- 意味層はFront Matterを扱わない（構造層のschematterはFront Matterを分離して検査する）。
- 表のセル構造、号の入れ子（細分）の意味型付けは未対応。入れ子の号リストは `INCOMPLETE` として保持・診断する。未対応のHTML・見出し等は黙って捨てず、診断する。
- 漢数字は位取り式・数字列式と全角/半角数字のみ。大字（壱など）や曖昧な表記は拒否する。
- アンカー生成はGitHubの基本規則の実装であり、実際のレンダラーとの照合はこのリポジトリでは再実行しない。
- 合格は、このプロファイルとスキーマで表現した規則に適合することを意味し、法令としての妥当性や全法令への適合を意味しない。

## 上流と帰属

- 構造検証は [iwe-org/schematter](https://github.com/iwe-org/schematter) の `schematter-lib` 0.2.0（crates.ioの公開版、Cargo.tomlで `=0.2.0` に固定、公開APIのみ使用）。上流のライセンスはApache-2.0。上流のソースは本リポジトリに複製（vendor）していない。依存として取得される上流・推移的依存のライセンスはそれぞれの配布物に従う。
- `law-semantic` の実装・スキーマ・プロファイル・試験ケースは、所有者のプロトタイプ（評価リポジトリ `markdown-schema-evaluation` の `evaluations/semantic-layer`）から移行した。実行時・ビルド時にそのリポジトリへは依存しない。

## ライセンス

本リポジトリは [MIT License](LICENSE) で公開する。依存ライブラリにはそれぞれのライセンスが適用される。

### ビルド環境とマッピング状態

依存関係の宣言に合わせ、必要なRust版は1.88以上とする。Cargo.lockを使用する。mapping_completeはマッピング診断にerrorがないことを表し、意味ASTのスキーマ適合や参照検証完了とは別の状態である。スキーマ不適合でも、既に得られたマッピング診断を消さずに返す。

## CI

GitHub Actionsはpush・pull request・手動実行で起動する。

- Linux / Windows / macOSのstable Rustでビルド・全試験。
- LinuxのRust 1.88.0で最小対応版のビルド・全試験。
- Linuxのstable Rustで整形とClippy（警告をエラー扱い）。

すべてCargo.lockを使う。ActionはコミットSHAに固定し、権限はcontents: readのみ。公開・配布処理は含まない。
