# loj-registry/v1alpha1 — 登録台帳

本ディレクトリは、法令を同定するRecordの集合である登録台帳と、Record IDの要件を定める。本ディレクトリの要件と[Recordのファイルのschema](../../../schemas/loj-registry/v1alpha1/record.schema.json)を規範とする。共通の型は[共通の型](../../common/v1alpha1/05-types.md)による。

## REG-01 置き場所と形式

- 登録台帳は、法令リポジトリとは別の一つのリポジトリに置く。

  ```text
  <登録台帳のリポジトリ>/
    space.yaml                  # 登録空間の識別子（RID-04）
    records/
      <Record ID>.yaml          # Recordごとに一つ
  ```

- Recordごとに一つのファイルとし、ファイル名はRecord IDとしなければならない（MUST）。ファイルの`id`は、ファイル名と一致しなければならない。
- `schemaVersion`は厳密に`loj-registry/v1alpha1`でなければならない（MUST）。YAMLのデータモデルは[CORE-03](../../common/v1alpha1/01-overview.md)に従う。schemaは閉じており、未知のフィールドを拒否する。
- 正本ブランチのGit履歴を、登録台帳の記録版の履歴とする（[CORE-01](../../common/v1alpha1/01-overview.md)）。
- 撤回・統合・分割したRecordのファイルを削除してはならない（MUST NOT）。
- `space.yaml`の形は本版では定めない。
- 登録台帳の検査（schema、ファイル名とID、`supersedes`・`replacedBy`の解決、採用中の番号主張の数、IDの衝突）は、L4の入力の前提として行い、適合レベルとは別に報告する。その結果を、法令データのL0〜L3の適合として主張してはならない（MUST NOT）。

## REG-02 Recordのファイル

| フィールド | 必須 | 内容 |
|---|---|---|
| `schemaVersion` | 必須 | `loj-registry/v1alpha1` |
| `id` | 必須 | Record ID。ファイル名と一致する |
| `label` | 任意 | 表示用のラベル。参照の解決・同一性の判断に使わない |
| `aliases` | 任意 | 旧リポジトリの以前の名前の一覧。各項目は`id`、`evidence`、`verification` |
| `status` | 必須 | `provisional`（仮登録）／`active`／`merged`／`split`／`withdrawn` |
| `identity` | 必須 | 同一性の確認。対象を指す資料・調査の記録（`evidence`）と`verification` |
| `text` | 必須 | 本文の取得状況`state`。`collected`（法令リポジトリに本文がある）／`notCollected`（取得していない）／`nonexistentConfirmed`（本文が存在しないことを確認した） |
| `repository` | 条件付き | 法令リポジトリの`location`（例: `github.com/law-of-japan/19480720-law-178`）と`canonicalBranch`（正本ブランチ。例: `main`） |
| `sources` | 必須（空可） | Sourceの一覧 |
| `numbers` | 必須（空可） | 番号主張の一覧（REG-03） |
| `externalIds` | 必須（空可） | 外部識別子の一覧（REG-05） |
| `replacedBy` | `merged`・`split`で必須 | 統合先、または分割先の候補の一覧。各項目は`record`（RecordRef）、`evidence`、`verification` |

- 本文のパッケージのないRecordも登録できる。
- `repository`は、`text.state`が`collected`のとき、または法令リポジトリに関係・Revisionを記録するとき、持たなければならない（MUST）。法令リポジトリを持たないRecordは`repository`を持たない。リポジトリが移転したら更新する。
- `provisional`のRecordの`identity.verification.state`を、`confirmed`にしてはならない（MUST NOT）。
- `notCollected`から、本文の不存在を推論してはならない（MUST NOT）。
- 題名と法令の種別は、登録台帳に持たない。版ごとに版パッケージ（`law.yaml`の`title`・`type`）にあり、改正で変わり得るためである。表示には`label`を使う。
- 分割先が確定していない候補は、`verification.state: unverified`で残す。
- 効力の終了（廃止・失効）を`status`で表してはならない（MUST NOT。履歴マニフェストのHIST-12）。

## REG-03 番号主張

| フィールド | 必須 | 内容 |
|---|---|---|
| `id` | 必須 | ファイルの中で一意な識別子 |
| `state` | 必須 | `exists`（番号がある）／`nonexistent`（番号がないことを確認した）／`unknown`／`illegible`（判読不能）／`conflicting`（資料の間で食い違う） |
| `raw` | `exists`・`illegible`・`conflicting`で必須 | 出典で観測した表記（例: `法律第百七十八号`）。`conflicting`では、資料ごとの表記を`{source, raw}`の一覧（2件以上）とし、ほかの`state`では文字列とする |
| `structured` | 任意 | `system`（採番の体系。例: 法律）、`period`（採番の期間。原表記の`raw`と、明治6年以後はグレゴリオ暦の範囲`gregorian: {from, to}`）、`issuer`（発令機関。原記載の順の一覧）、`series`（`state`: `regular`／`none`／`unknown`と原表記）、`number`（番号の成分。v1alpha2のNUM-01の正規の形） |
| `evidence` | 必須 | 出典と箇所。索引の転記（Sourceの`kind: index`・`egov`）と公布の原典（`promulgation`）を区別できる |
| `verification` | 必須 | 確認状態 |
| `decision` | 必須 | `adopted`（採用）／`pending`（保留）／`retracted`（撤回）。理由を`reason`に書く |
| `supersedes` | 任意 | 訂正前の主張の`id` |
| `glyphs` | 任意 | `raw`の字体注記。形は字体注記（loj-glyphs）のGLYPH-04・05により、`node`の代わりに主張の`id`で指し、`offset`は`raw`の中の位置とする |

- 一つの欄の上書きで、番号を訂正してはならない（MUST NOT）。訂正はREG-04による。
- `raw`を正規化した値で置き換えてはならない（MUST NOT）。
- 採番の期間を、西暦年だけに縮めてはならない（MUST NOT）。一つの暦年が二つの元号の期間に分かれる場合（昭和64年と平成元年等）も、`gregorian`の範囲で表す。
- 発令機関は、公布・発令の時のものとする。現在の所管から逆算してはならない（MUST NOT）。
- `decision: adopted`の主張は、同時に二つ以上あってはならない（MUST NOT）。資料の間の食い違いで決められないときは、`conflicting`の主張を一つ採用するか、すべてを`pending`にする。
- `unknown`の番号を持つRecord同士を、番号の一致で同一と判断してはならない（MUST NOT）。番号の成分の完全な一致は照合の候補にとどめ、自動で統合しない。

## REG-04 訂正と履歴

- 番号主張の訂正は、新しい主張を加えて`supersedes`で訂正前の主張を指し、訂正前の主張を`decision: retracted`にする。訂正前の主張を削除してはならない（MUST NOT）。
- `supersedes`は、同じファイルの主張を指さなければならない（MUST）。
- 番号以外の値（`label`、`repository`等）の訂正は、Git履歴で追う。
- 判断を記録した日時は、その主張を加えた・変えたcommitの日時とする。データに日時の欄を設けない。

## REG-05 外部識別子

- `externalIds`の各項目は、`issuer`（発行元。例: `egov`）、`kind`（対象の種別。例: `law`）、`value`、`evidence`、`verification`を持つ。
- 版を対象とする外部識別子（e-Govの法令履歴ID等）は、履歴マニフェストのRevisionの`externalIds`に置き（HIST-09）、登録台帳に置いてはならない（MUST NOT）。
- 外部の収録の単位がLoJのRecordと一致すると仮定してはならない（MUST NOT）。一つの外部IDが複数のRecordに対応する場合は、候補をすべて`unverified`で残す。
- 外部IDの変更で、Record IDを変えてはならない（MUST NOT）。
