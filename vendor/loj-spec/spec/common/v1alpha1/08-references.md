# 固定参照

Record・Revision・記録版を指す参照の形式と、解決の規則を定める。

## FIXREF-01 参照の種類と構造化形式

| 種類 | 指すもの | 指す内容が変わるか | 用途 |
|---|---|---|---|
| Record参照 | Record | — | 法令そのものを指す（RecordRef、[TYPE-01](05-types.md)） |
| Revision参照 | 法令の状態 | 記録訂正・復元の進展で変わり得る | 「この法令のこの状態」を指す。派生ビューが版ごとに付ける名前（タグ等）と同じ意味 |
| 固定参照 | 記録版 | 変わらない | 引用・検証の対象になった本文・根拠・判断を固定する |

データの中では、次の構造化形式で書く。RevisionRefとFixedRefは[共通の型のschema](../../../schemas/common/v1alpha1/types.schema.json)で定義する。時点値の`sameAs`（[TIME-07](06-time.md)）はRevisionRef、Sourceの`fixed`（[TYPE-02](05-types.md)）でLoJの中の資料を指すものはFixedRefである。

| 型 | フィールド | 要件 |
|---|---|---|
| RevisionRef | `record`（RecordRef）、`revision`（revision_id） | revision_idで指す。可読ラベルや施行日で指してはならない（MUST NOT）。`node`を持たない |
| FixedRef | RevisionRefのフィールドと、`commit`（必須）、`node`（任意）、`path`（任意）、`repository`（任意） | `commit`は正本のcommitの完全なハッシュ。`path`と`repository`は検査と解決の補助であり、参照の同一性に含めない。`repository`の書式は定めない |

- 版の中の特定のノードを指せるのは、固定参照だけである（[FIXREF-07](#fixref-07-ノードの指定)）。

## FIXREF-02 文字列表記

データの外（文献、Issue、外部サイト等）で引用するときは、次の表記を使う。

```text
loj:<space>/<record>                            # Record参照
loj:<space>/<record>/<revision>                 # Revision参照
loj:<space>/<record>/<revision>@<commit>        # 固定参照
loj:<space>/<record>/<revision>@<commit>#<node> # 固定参照とノード
```

- `@<commit>`のない`#<node>`を使ってはならない（MUST NOT）。
- 構造化形式と文字列表記は相互に変換でき、参照の同一性に含まれるフィールド（登録空間、Record ID、revision_id、commit、ノード）を落とさない。表示と解決の補助のフィールド（RecordRefの`label`、FixedRefの`path`と`repository`）は、文字列表記に書けず、変換で落ちる。
- 構造化形式で`space`を省略した参照を文字列表記に変換するときは、参照する側の登録空間を補う。法令リポジトリの中の参照では、そのRecordを登録した登録台帳の登録空間とする。登録空間を宣言するファイル（`space.yaml`）の形を定めるまで、検査器はこの登録空間を入力として受け取る（[VAL-03](03-validation.md)）。入力がなければ、変換を判定不能とする。
- 適合ケースの期待結果の参照（`references[].target`）は、この文字列表記で書く（[CONF-01](04-conformance.md)）。
- 登録空間の識別子、Record ID、revision_idは、`/`、`@`、`#`を含まない。
- `loj:`は本章の解決規則の名前であり、URIスキームの登録を前提にしない。GitHub等のURLは解決結果の一つであり、参照そのものとして案内してはならない（MUST NOT）。

## FIXREF-03 固定参照の解決

- 固定参照は、登録台帳からRecordのリポジトリを求め、指定のcommitの`history.yaml`で`revision`を探し、その`text.package`の版パッケージを対象とする。リポジトリが移転しても、commitが同じなら同じ記録版を指す。
- 固定参照が固定するのは、そのcommitにおける版パッケージ、履歴マニフェストの当該Revisionの記述、`basis`が参照する関係とその内訳、それらが参照するSourceである。本文だけを固定するものではない。
- `path`がある場合は、指定のcommitの`text.package`（省略時は既定の`versions/<revision_id>/law.yaml`、[HIST-08](../../loj-history/v1alpha1/01-history.md)）と一致しなければならない（MUST）。一致しなければ解決の誤りとし、`path`を優先して読んではならない（MUST NOT）。
- 固定参照は、`revision`を持たなければならない（MUST）。commitだけでは対象の版が定まらないためである。
- LoJの中の資料をSourceの`fixed`で指すときは、FixedRefを使う。

## FIXREF-04 commitの条件

- `commit`は完全なハッシュとする。短縮形、ブランチ名、タグ名を使ってはならない（MUST NOT）。ハッシュの長さでGitのオブジェクト形式（SHA-1は40桁、SHA-256は64桁）を区別する。ハッシュは小文字の16進数で書く（大文字を含むものは不合格とする）。
- 固定参照に使えるcommitは、正本ブランチの履歴から到達できるものに限る。作業ブランチや派生ビューのcommitを、固定参照にしてはならない（MUST NOT）。作業ブランチのcommitは、統合の操作（squash・rebase等）によって、同じハッシュで正本ブランチに残るとは限らない。したがって、統合した後も正本ブランチから到達できるとは限らない。
- 本版では、正本ブランチのGitのオブジェクト形式（SHA-1、SHA-256）を変えてはならない（MUST NOT）。変えると、移行前のcommitへの固定参照を解決できなくなる。移行の規則は、新しい仕様の版で定める。

## FIXREF-05 Revision参照の解決

- Revision参照は、正本ブランチの最新のcommitにおける当該Revisionの記録版に解決する。解決結果には、解決に使ったcommitを付けて返さなければならない（MUST）。
- 撤回したRevisionへの参照は、撤回されたことを報告する。統合・分割したRevisionへの参照は、統合・分割の先を示し、黙って別のRevisionへ転送してはならない（MUST NOT）。
- 撤回・統合・分割したRevisionへの固定参照は、当時の記録版に解決する。そのうえで、現在の状態を併せて示す。

## FIXREF-06 解決できない場合

解決の結果は、次のいずれかで報告する。

| 結果 | 条件 |
|---|---|
| 解決済み | 本章の手順で対象を特定できた |
| 未取得 | リポジトリまたはcommitを取得できない。未取得のまま、不存在とみなしてはならない（MUST NOT） |
| 解決の誤り | commitを取得できたが、そのcommitに当該Revisionがない。`path`が一致しない。commitが正本ブランチから到達できない。または`commit`のない参照に`node`がある |

- 解決の誤りを、最新の記録版へ黙って解決してはならない（MUST NOT）。
- 他のRecordへの参照も、自身のRecordへの参照も、参照の解決の検査（適合レベルとは別に報告する。[VAL-07](03-validation.md)）で扱う。対象は、Sourceの`fixed`のFixedRefと、時点値の`determinedBy`の`record`（RecordRef）である。固定参照の解決は指定のcommitの取得を伴い、L3・L4の入力（記録版）に収まらないため、L4に区分を設けない。
- 時点値の`sameAs`は、時点の比較を伴うため、L4の時点の整合（[XREF-03](10-cross-record.md)）で扱う。

## FIXREF-07 ノードの指定

- `node`は、その版のノードIDとする。ノードIDは版の中の位置を示し、条の歴史的同一性を主張しない（[ID-02](../../v1alpha2/07-identifiers.md)）。
- Revision参照にノードを付けてはならない（MUST NOT）。ノードIDは番号から作るため、番号・附則のkeyの記録訂正、復元の進展、表の行の追加、版の訂正、仕様の版の移行で、同じIDが別の規定を指すことがあるためである。
- 固定参照のノードは、常に指定のcommitの版の同じノードに解決する。
- 固定参照から、正本ブランチの最新で同じIDのノードを示す機能を、実装の補助として提供してよい（MAY）。その場合は、指定のcommitから内容・位置が変わったかを併せて報告する。この機能は規範にしない。
- 履歴マニフェストのScopeの`nodes`は、同じcommitの版のノードを指すため、本要件の対象外とする（[TYPE-08](05-types.md)）。
