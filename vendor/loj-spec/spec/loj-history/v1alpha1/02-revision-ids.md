# revision_id

## REV-01 revision_idの性質

- Revisionは、組（Record ID、revision_id）で識別する。revision_idは、一つのRecordの中で一意であればよい。
- revision_idは、Crockford Base32の小文字（`0-9a-hjkmnp-tv-z`）8文字の不透明な識別子とする。
- revision_idから、施行日・改正法の番号・版の順序を導いてはならない（MUST NOT）。revision_idの並べ替えで施行の順を得てはならない。
- 新しいrevision_idは、ランダムに発行し、同じRecordの既存のID（撤回・統合・分割したものを含む）と衝突しないことを確かめなければならない（MUST）。連番や時刻を含む方式を使ってはならない（MUST NOT）。
- 次の変更では、revision_idを変えてはならない（MUST NOT）。施行日・改正法の番号の訂正、成立根拠や出典の追加、本文の記録訂正・復元の進展（記録版の更新）、可読ラベルの変更、施行時点の確定。
- 撤回・統合・分割したRevisionのIDを、別のRevisionに再利用してはならない（MUST NOT）。

## REV-02 可読ラベル

- Revisionは、表示用の可読ラベル`label`（例: `2007-01-01_H17-043`）を持てる（MAY）。e-Govの法令履歴IDの書式を使ってよい。
- ラベルは一意でなくてよく、訂正で変えてよい。
- ラベルから意味を導いてはならない（MUST NOT）。ラベルを参照の解決に使ってはならない（MUST NOT）。

## REV-03 保存のパスとタグ

- 正本の版パッケージのディレクトリ名は、revision_idとする。ラベルの訂正で、ディレクトリを改名してはならない（MUST NOT）。
- 派生ビューのタグの名前も、revision_idとする（[VIEW-04](../../common/v1alpha1/09-views.md)）。

## REV-04 版の粒度

- Revisionの単位は、「一つの施行時点 × 一つの改正法令」とする。
  - 一つの改正法令の改正規定が異なる施行時点を持つ場合は、施行時点ごとに別のRevisionとする。
  - 同じ施行時点に複数の改正法令が施行される場合は、改正法令ごとに別のRevisionとし、相互の順序を`basedOn`で表す。
  - 一つの改正法令が同じ施行時点に複数の範囲を改正する場合は、一つのRevisionとし、範囲の一覧を成立根拠に持つ。
  - 施行時点が`pending`の改正も、一つのRevisionとして登録できる（MAY）。施行時点が確定しても、revision_idは変わらない。
- 官報正誤と記録訂正は法令の改正ではないので、新しいRevisionを作ってはならない（MUST NOT。[CHG-01](../../common/v1alpha1/07-changes.md)）。
