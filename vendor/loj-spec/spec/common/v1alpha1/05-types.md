# 共通の型

本章の型は、[共通の型のschema](../../../schemas/common/v1alpha1/types.schema.json)で定義し、各種類のschemaから参照する。

## TYPE-01 RecordRef

| フィールド | 必須 | 内容 |
|---|---|---|
| `id` | 必須 | Record ID（loj-registryのRID） |
| `space` | 任意 | 登録空間の識別子。形は[RID-04](../../loj-registry/v1alpha1/02-record-ids.md)による。省略時は、参照する側と同じ登録空間 |
| `label` | 任意 | 表示用の名前（例: 平成十七年法律第四十三号） |

- Recordは、登録空間とRecord IDの組で指さなければならない（MUST）。リポジトリのURLで指してはならない（MUST NOT）。
- `label`は表示用である。番号主張の代わりにしてはならない（MUST NOT）。参照の解決に使わない。

## TYPE-02 Source

| フィールド | 必須 | 内容 |
|---|---|---|
| `id` | 必須 | ファイルの中で一意な識別子 |
| `kind` | 必須 | `promulgation`（公布の原典）／`officialCorrection`（官報正誤）／`index`（索引）／`egov`（e-Gov）／`other` |
| `title` | 必須 | 資料の名前 |
| `uri` | 任意 | 資料の所在 |
| `glyphPreservation` | 任意 | 資料が字体を保持するか。`preserved`（旧字体・異体字を保持している。例: 官報の画像）／`normalized`（新字体等に置き換えている。例: e-Gov）／`unknown`（確かめていない）。省略は`unknown`と同じ |
| `fixed` | 任意 | 資料の内容を固定する手段。LoJの中の資料はFixedRef（[FIXREF-01](08-references.md)）のmappingで、LoJの外の資料は固定の手段を記述した文字列（ハッシュ、保存先とcommit等）で書く |

- `uri`だけでは、過去の内容の固定を保証しない。
- LoJの中の資料を、`fixed`の文字列（文字列表記の`loj:`で始まるものを含む）で固定してはならない（MUST NOT）。`loj:`で始まる文字列は、schemaで拒否する。
- 旧形式から移したURIを、個別の主張の根拠に自動で認定してはならない（MUST NOT）。

## TYPE-03 Evidence

| フィールド | 必須 | 内容 |
|---|---|---|
| `source` | 必須 | Sourceの`id` |
| `locator` | 必須 | 資料の中の箇所の原文表記（例: 附則第一項、1コマ） |

- `source`は、同じファイルのSourceを指さなければならない（MUST）。Sourceを持たない字体注記と再構築報告では、同じ記録版の履歴マニフェストのSourceを指す。
- 箇所のない出典の参照だけで、主張を確認済みにしてはならない（MUST NOT）。

## TYPE-04 Verification

| フィールド | 必須 | 内容 |
|---|---|---|
| `state` | 必須 | `unverified`（未確認）／`confirmed`（確認済み）／`disputed`（疑義あり）／`retracted`（撤回） |
| `reason` | 任意 | 理由 |

- `unverified`・`disputed`には、理由を書くことを推奨する（SHOULD）。

## TYPE-05 確認済みの主張の根拠

- 確認状態を持つ主張（`evidence`と`verification`を持つもの）は、`verification.state: confirmed`なら一つ以上のEvidenceを持たなければならない（MUST）。
- Evidenceの数は、記載したEvidenceの数で数え、`source`が解決できるかを問わない。`source`が解決できないことは、[TYPE-03](#type-03-evidence)の違反として報告する。
- 未確認・不明とした主張の理由は、本要件では求めない。確認状態の理由（`verification.reason`）は[TYPE-04](#type-04-verification)（`unverified`・`disputed`に書くことの推奨）による。不明・該当しないとした時点値の`reason`は[TIME-01](06-time.md)（状態の必須のフィールド）により、書く内容は[TIME-05](06-time.md)による。
- この要件の検査は、出典の記載と形式を確かめる。出典の内容が主張を裏付けるかは判定しない。

## TYPE-06 Scope

| フィールド | 必須 | 内容 |
|---|---|---|
| `text` | 必須 | 対象範囲の原文表記（改め文等の原文のまま） |
| `nodes` | 任意 | 対象の版のノードIDの一覧 |

- `text`は原文のまま記録し、版の字体に合わせて書き換えてはならない（MUST NOT）。
- `nodes`は位置の補助情報であり、条の歴史的同一性を主張しない（[ID-02](../../v1alpha2/07-identifiers.md)）。

## TYPE-07 Scopeとノードの照合

- `text`と版のノードの照合は派生処理である（[CHAR-05](02-characters.md)）。照合のために変換した文字列を、`text`や版の本文に書き戻してはならない（MUST NOT）。
- 番号で指す部分（「第二条」「第三項」）は、構造ラベルの字体と番号の正規化で番号の経路に直し、ノードの番号と比べる。見出しの`display`の文字列とは比べない。
- 内容で指す部分（表の行の「成人の日の項」等）は、自動では照合しない。照合の補助に、新字体への変換等の派生処理を使ってよい（MAY）。
- 番号で照合できない部分を含む対象範囲の`nodes`は、人が確かめて書く。照合の補助だけで`nodes`を確定してはならない（MUST NOT）。
- 検査では、`nodes`が対象の版に存在すること、番号で指す部分が`nodes`の番号の経路と一致することを確かめる。内容で指す部分は判定不能として報告し、不合格にしない。
- `nodes`が指す版（[TYPE-08](#type-08-scopeのノードの記録版)）の版パッケージのL1が不合格の場合は、その`nodes`の照合を判定せず、判定不能として報告する。不合格にしない。ノードIDはL1で導くためである（[VAL-02](03-validation.md)）。判定不能により、L3（根拠）は未完了になる（[VAL-04](03-validation.md)）。

## TYPE-08 Scopeのノードの記録版

- `nodes`は、その対象範囲を成立根拠に持つRevisionの版パッケージのノードIDとし、同じcommitの版パッケージで解決する。
- 版パッケージのノードIDが変わる変更（記録訂正・復元等）では、同じcommitで`nodes`を更新しなければならない（MUST）。
- 固定参照で関係を固定した場合は、`nodes`も、そのcommitの版パッケージで解決する。
