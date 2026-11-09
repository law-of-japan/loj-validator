# 再構築報告

## REBR-01 置き場所と形式

- 再構築報告は、法令リポジトリの最上位の`rebuild.yaml`とする。一つのRecordにつき一つとし、再構築を進めるたびに同じファイルを更新する。過去の内容は、正本のGit履歴で追う。
- 旧リポジトリから移していないRecord（新しく登録したRecord）には、再構築報告を置かない。
- `schemaVersion`は厳密に`loj-rebuild/v1alpha1`でなければならない（MUST）。YAMLのデータモデルは[CORE-03](../../common/v1alpha1/01-overview.md)に従う。schemaは閉じており、未知のフィールドを拒否する。
- 再構築報告は正本に属するが、法令の状態のデータではない（REB-06）。
- 再構築報告だけを変える変更の種類は`metadata`とする（[CHG-01](../../common/v1alpha1/07-changes.md)）。
- 再構築報告の検査（schema、Revision・Source・ノードIDの解決、旧commitの存在（旧リポジトリを取得できる場合））は、適合レベルの外で行う。その結果を、法令データの適合として主張してはならない（MUST NOT）。

## REBR-02 項目

| フィールド | 必須 | 内容 |
|---|---|---|
| `schemaVersion` | 必須 | `loj-rebuild/v1alpha1` |
| `record` | 必須 | 自身のRecordRef |
| `legacy` | 必須 | 旧リポジトリ（`repository`）と、凍結した時点の規定のブランチのcommit（`frozenAt`） |
| `sourceChecks` | 必須（空可） | 使った資料ごとの、文字情報の保持の確認結果（REBR-03） |
| `mappings` | 必須（空可） | 旧commitとRevisionの対応（REBR-04） |
| `pending` | 必須（空可） | 旧リポジトリにあるが、まだ再構築していない改正（REBR-04） |
| `differences` | 必須（空可） | 原典と旧本文の間に見つかった差（REBR-05） |

- 資料は、履歴マニフェストの`sources`の`id`で参照する。再構築報告で資料を定義し直してはならない（MUST NOT）。
- 旧リポジトリのcommitは、完全なハッシュで指さなければならない（MUST）。ブランチ名・タグ名（凍結した`master`を含む）で指してはならない（MUST NOT）。
- 旧リポジトリは凍結するため、見つかった差を旧リポジトリで直してはならない（MUST NOT）。

## REBR-03 資料の確認結果

- `sourceChecks`の各項目は、`source`（Sourceの`id`）、`glyphs`、`evidence`、任意の`note`を持つ。`glyphs`は次のいずれかとする。

  | `glyphs` | 意味 |
  |---|---|
  | `preserved` | 旧字体・異体字を保持している（例: 官報の画像） |
  | `normalized` | 新字体等に置き換えている（例: e-Gov） |
  | `unknown` | 確かめていない |

- `glyphs`が`preserved`でない資料から起こした本文を、原典どおりと主張してはならない（MUST NOT）。その版の`text.glyphs`を`verified`にするには、`preserved`の資料と照合しなければならない（MUST。loj-glyphsのGLYPH-10）。

## REBR-04 旧commitとの対応と未再構築の改正

- `mappings`の各項目は、旧commit（`commit`）、対応するRevision（`revisions`、0件以上）、`verification`、任意の`note`を持つ。一つの旧commitが複数のRevisionに対応してもよい（MAY）。
- Revisionに対応しない旧commit（書式の修正、旧リポジトリでの誤りの訂正等）は、`revisions: []`と理由を記録する。旧commitを省略してはならない（MUST NOT）。省略すると、対応の有無を確かめたかどうかを区別できない。
- 対応は判断である。旧commitのメッセージ・日付だけを根拠に、`confirmed`にしてはならない（MUST NOT）。
- `pending`の各項目は、任意の旧commit（`commit`）、旧リポジトリでの呼び名（`label`）、再構築していない理由（`reason`。例: 「1966年以降の官報を入手していない」）を持つ。

## REBR-05 原典と旧本文の差

- `differences`の各項目は、`revision`、`class`、`legacy`（旧commitとファイルのパス）、`evidence`、`verification`を持つ。`class`ごとに次を記録する。

  | `class` | 内容 | 記録の単位 |
  |---|---|---|
  | `wording` | 文言の差（字体の差を除く） | 箇所ごと。`node`（新しい版のノードID）、`recorded`（新しい本文の該当部分）、`legacyText`（旧本文の該当部分） |
  | `structure` | 構造の差（表を箇条書きにしている、無番号の項に番号を補っている等） | 箇所ごと。`node`と説明（`note`） |
  | `omission` | 旧本文にない部分（附則の欠落等） | 箇所ごと。`node`または対象範囲（`scope`） |
  | `glyph` | 字体だけの差（原典の旧字体を、旧本文が新字体にしている等） | 版ごとに1件。件数（`count`）と、任意の例（`examples`） |

- 字体だけの差かどうかは、派生処理（新字体への変換）で両方をそろえた結果を比べて判定してよい（MAY）。派生処理の結果を本文に書き戻してはならない（MUST NOT。[CHAR-05](../../common/v1alpha1/02-characters.md)）。
- 差の原因が分からないもの（例: 官報正誤による差か、旧データの誤りか）は`verification: unverified`とし、理由に確かめていない点を書く。原因が分かるまで、旧データの誤りと決めつけてはならない（MUST NOT）。
