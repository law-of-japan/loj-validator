# 時点値

公布・施行・適用開始・失効等の法的な時点は、時点値（TimePoint）で表す。

## TIME-01 時点値の状態

| `state` | 意味 | 必須のフィールド | 例 |
|---|---|---|---|
| `date` | 日が確定している | `value`、`raw`、`precision`、`evidence`、`verification` | 平成十九年一月一日 |
| `derived` | 規則により確定日を計算できる | `rule`、`base`、`raw`、`evidence`、`verification` | 公布の日から起算して三十日を経過した日 |
| `pending` | 法令上まだ定まっていない | `raw`、`delegation`、`evidence`、`verification` | 政令で定める日、条約が効力を生ずる日 |
| `unknown` | 調査上不明 | `reason` | 原典未取得 |
| `notApplicable` | 該当しない | `reason` | 公布行為を伴わない状態 |

- 時点値は、状態ごとの必須のフィールドを持たなければならない（MUST）。
- `pending`・`unknown`・`notApplicable`に`value`を、`derived`以外の状態に`rule`・`calc`・`base`を置いてはならない（MUST NOT）。`unknown`・`notApplicable`に`raw`を置いてはならない。

## TIME-02 date

- `value`はグレゴリオ暦の日付で、`precision`（`day`／`month`／`year`）に合わせた書式（`YYYY-MM-DD`／`YYYY-MM`／`YYYY`）で書く。
- `raw`に原表記（和暦・漢数字等）を保存し、グレゴリオ暦の値で置き換えてはならない（MUST NOT）。
- 精度が日に満たない値を、確定日として順序の比較に使ってはならない（MUST NOT）。暦や精度が不明な値を、推定で`date`にしてはならない（MUST NOT）。
- グレゴリオ暦への換算は、明治6年1月1日以後の日付に限って必須とする。それより前の日付の換算規則は本版では定めない。換算の根拠を示せない日付を推定で`date`にせず、`unknown`とし、原表記を`reason`に書く。

## TIME-03 derivedとcalc

- `rule`は計算規則の原文、`base`は基準となる時点値である。
- `base`が確定日（精度が日の`date`、または確定日を計算できる`derived`）なら、計算結果の日付を`value`に書かなければならない（MUST）。
- `derived`は、任意の`calc`で計算を機械が読める形で示せる（MAY）。

  | `calc` | 意味 | 原文の例 | 計算 |
  |---|---|---|---|
  | `{op: sameDay}` | 基準の時点と同じ日 | 公布の日から施行する | 基準の日 |
  | `{op: nextDay}` | 基準の時点の翌日 | この法律の施行の日の翌日から施行する | 基準の日の翌日 |
  | `{op: afterElapsed, unit: day, count: N}` | 基準の日から起算してN日を経過した日 | 公布の日から起算して三十日を経過した日から施行する | 基準の日を1日目として数え、N日目の翌日（基準の日＋N日） |
  | `{op: afterElapsed, unit: month, count: N}`、`unit: year` | 基準の日から起算してNか月（N年）を経過した日 | 公布の日から起算して六月を経過した日から施行する | 暦に従って計算し（民法第143条）、期間の末日の翌日 |

- `calc`があり、`base`が確定日なら、`value`は`calc`による計算結果と一致しなければならない（MUST）。`calc`がない`derived`の計算結果は、未検査として報告する。
- `base`が`date`でない場合、計算結果を確定日として扱ってはならない（MUST NOT）。
- `calc`は`rule`の代わりにしない。`calc`と`rule`の対応は入力者の判断であり、L3の原典照合で確かめる。
- 期日が別に定まる規則（「…から起算して一年を超えない範囲内において政令で定める日」等）は、`derived`ではなく`pending`である。

## TIME-04 pendingと確定

- `pending`は、定め方の原文を`raw`に、委任先・条件を`delegation`（原文と出典）に持つ。
- `pending`の時点が法令上確定した場合は、`state: date`とし、確定前の定め方を`delegation`に、確定させた根拠（施行期日政令等のRecordと出典）を`determinedBy`に保持しなければならない（MUST）。確定前の値を消してはならない（MUST NOT）。
- `determinedBy`を持つ時点値は、`delegation`を持たなければならない（MUST）。

## TIME-05 unknownとnotApplicable

- `unknown`の`reason`には、不明の理由（未調査、資料不足、判読不能、資料間不一致）を書く。確認した資料の範囲を`surveyed`（Evidenceの一覧）に書くことを推奨する（SHOULD）。
- `notApplicable`の`reason`には、該当しないと判断した根拠を書く。

## TIME-06 時点値の確認状態と出典

- すべての時点値は、確認状態（`verification`）と出典（`evidence`）を持てる。確認済みの時点値は、[TYPE-05](05-types.md)を満たさなければならない（MUST）。
- 時点値の誤りの訂正は、時点値の上書きと、記録版の履歴（`metadata`の変更、[CHG-01](07-changes.md)）で追跡する。

## TIME-07 sameAs

- 時点値は、任意の`sameAs`（`record`と`revision`）で、同じ時点を定める他のRecordのRevisionを指せる（MAY）。例: 「特例法の施行の日」は、特例法の最初のRevisionの施行時点と同じ。
- `sameAs`があっても、時点値自身の状態・値・原表記を省略してはならない（MUST NOT）。参照先のRecordを収録していない場合にも、時点が分かるようにするためである。
- 参照先との一致は、L4の時点の整合（[XREF-03](10-cross-record.md)）で検査する。
