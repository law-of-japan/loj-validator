# Record間の検査（L4）

L4「他法令との参照整合性」の検査を、Record間の参照について定める。区分と結果の値は[VAL-06](03-validation.md)による。

## XREF-01 関係の解決

- 関係（改正・廃止・全部改正等）の起点・終点のRecordを、登録台帳で解決できるかを検査する。他の登録空間のRecordは、登録空間とRecord IDの組で解決する。本版の入力は一つの登録台帳だけなので、他の登録空間のRecordは未取得になる（[VAL-03](03-validation.md)）。
- 解決の対象はRecordであり、リポジトリの存在ではない。本文を収録していないRecordも、登録台帳にあれば解決済みとする。
- 統合したRecordへの参照は統合先へ解決する。分割したRecordへの参照は候補を返し、解決済みにしてはならない（MUST NOT）。
- 登録台帳で`status: withdrawn`（[REG-02](../../loj-registry/v1alpha1/01-registry.md)）のRecordを、不存在確認済みのRecordとする。確定した関係（`status: active`で、`verification.state`が`confirmed`の関係）が、起点または終点に不存在確認済みのRecordを参照している場合は、違反とする。
- 登録台帳にないRecord IDは、不存在確認済みとせず、未取得として報告する。登録台帳が網羅的である保証はないためである。未取得の参照（登録台帳にないIDと、登録台帳を取得できない場合）は、違反にしない。

## XREF-02 関係の整合

- 起点（`origin: totalRevision`）のRevisionの`totallyRevises: {record, relation}`（[HIST-04](../../loj-history/v1alpha1/01-history.md)）が指す関係を、指されたRecordの履歴マニフェストで解決し、次のすべてを満たすかを検査する。
  - 指す`id`の関係が存在する。
  - 関係の`type`が`totallyRevises`である。
  - 関係の`status`が`active`である。
  - 関係の`from`が、当該`totallyRevises`を持つRevisionの属する履歴マニフェストのRecord（全部改正後のRecord）である。
- `from`が同じRecordかは、`from`を[XREF-01](#xref-01-関係の解決)で解決した結果で決める。統合したRecordは、統合先と同じRecordとして扱う。分割したRecordに解決した場合は、判定不能として報告する（[VAL-04](03-validation.md)の表で区分の結果を未完了にする）。
- いずれかを満たさなければ違反とする。指されたRecordの履歴マニフェストを取得できない場合は、未取得として報告する。
- 起点の側には関係への参照を記録しない（[HIST-03](../../loj-history/v1alpha1/01-history.md)）。本版で、関係と起点の記録を比べるのは、本要件の全部改正の対応だけである。関係の対象範囲は、比べる相手がないため検査しない。

## XREF-03 時点の整合

- 時点値の`sameAs`（[TIME-07](06-time.md)）が指すRevisionを解決し、両方の時点を比べる。
- 両方が確定日で食い違えば、違反とする。参照先のRecordを収録していない場合は、未取得として報告する。一方が確定日でない場合は、判定不能として報告する。

## XREF-04 条項参照の解決

- 他法令の条項・版を指定した参照の解決を、L4の区分として予約する。本版では正式化しない。
- L2の参照の設計を定めた後の版で定める。

## XREF-05 表示の整合

- 版パッケージの`law.number`（[MAN-01](../../v1alpha2/02-manifest.md)）を、登録台帳の採用中の番号主張（[REG-03](../../loj-registry/v1alpha1/01-registry.md)）と比べる。対象は、Recordの法令リポジトリのすべての版パッケージとする。
- 採用中の主張の`state`が`exists`または`illegible`の場合、`law.number`は、`raw`と一致するか、`structured.period.raw`の後に区切りを入れずに`raw`を続けた文字列と一致しなければならない（`structured.period`がなければ、`raw`との一致だけを見る）。一致しない場合（`law.number`が`null`の場合を含む）は、警告とする。
- 採用中の主張の`state`が`nonexistent`または`unknown`の場合、`law.number`が`null`でなければ、警告とする。
- 採用中の主張がない場合と、採用中の主張の`state`が`conflicting`の場合は、判定しない。
- 表示用の値の誤りであり、不合格にしない。字は正規化せずに比べる。

## XREF-06 公布の時点の整合

- 同じ起点のRecord（`from`）を持つ関係が、複数のRecordの履歴マニフェストにある場合に、関係の`promulgated`（[HIST-03](../../loj-history/v1alpha1/01-history.md)）を比べる。一つの改正法令の公布の時点は一つだからである。
- 起点が同じかは、`from`を[XREF-01](#xref-01-関係の解決)で解決した結果（登録空間とRecord IDの組。統合したRecordは統合先）で決める。分割したRecordに解決した関係は、比べない。
- 両方が確定日（[TIME-03](06-time.md)）で食い違えば、違反とする。一方が確定日でない場合は、判定不能として報告する。一方だけが`promulgated`を持つ場合と、比べる関係のRecordを収録していない場合は、判定しない。
- 撤回した関係（`status: retracted`）は比べない。同じRecordの履歴マニフェストの中の関係どうしも、本要件では比べない。
