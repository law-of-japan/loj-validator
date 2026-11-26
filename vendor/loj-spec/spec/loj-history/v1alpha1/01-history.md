# loj-history/v1alpha1 — 履歴マニフェスト

本ディレクトリは、法令リポジトリの構成と、履歴マニフェスト（`history.yaml`）の要件を定める。本ディレクトリの要件と[履歴マニフェストのSchema](../../../schemas/loj-history/v1alpha1/history.schema.json)を規範とする。共通の型（RecordRef、Source、Evidence、Verification、Scope）は[共通の型](../../common/v1alpha1/05-types.md)、時点値（TimePoint）は[時点値](../../common/v1alpha1/06-time.md)による。

## HIST-01 ファイルと形式

- 法令リポジトリは、一つのRecordにつき一つとし、次の構成とする。

  ```text
  <法令リポジトリ>/
    history.yaml                 # 履歴マニフェスト
    rebuild.yaml                 # 再構築報告（旧リポジトリから移したRecordだけ。loj-rebuild）
    versions/
      <revision_id>/             # 版パッケージ（loj/v1alpha2）と字体注記
        law.yaml
        main.md
        glyphs.yaml
  ```

- 履歴マニフェストは、リポジトリの最上位の`history.yaml`とし、一つのRecordの履歴（Revisionの集合、そのRecordを終点とする関係、収録範囲、効力の終了）を記述する。
- `schemaVersion`は厳密に`loj-history/v1alpha1`でなければならない（MUST）。ほかの値（知らない版を含む）のファイルは、処理不能として未完了とし、不合格にしない（[VAL-04](../../common/v1alpha1/03-validation.md)）。YAMLのデータモデルは[CORE-03](../../common/v1alpha1/01-overview.md)に従う。ファイルは、その`schemaVersion`のschemaに合わなければならない（MUST）。schemaは閉じており、未知のフィールドを拒否する。schemaに合わない箇所の診断の規則は、[VAL-07](../../common/v1alpha1/03-validation.md)による。
- Record自体の属性（番号主張、外部識別子、同一性）は登録台帳に置き、履歴マニフェストに写してはならない（MUST NOT）。
- 本文を収録していないRecordも、関係の終点になる場合や、Revisionを記録する場合は、法令リポジトリを持てる（MAY）。そのリポジトリには`history.yaml`だけを置き、`versions/`を置かなくてよい。その履歴マニフェストのRevisionは、本文のない起点（`origin: coverageStart`、`text.status: absent`）一つでよく、`coverage.state`は`unconfirmed`とする。
- 履歴マニフェストの`label`等の表示用の値は、参照の解決・同一性の判断・順序の決定に使ってはならない（MUST NOT）。

## HIST-02 トップレベル

| フィールド | 必須 | 内容 |
|---|---|---|
| `schemaVersion` | 必須 | `loj-history/v1alpha1` |
| `record` | 必須 | 自身のRecordRef |
| `sources` | 必須（空可） | Sourceの一覧 |
| `relations` | 必須（空可） | このRecordを終点とする関係の一覧（HIST-03） |
| `revisions` | 必須（一つ以上） | Revisionの一覧（HIST-04〜10）。撤回・統合・分割したものも残す |
| `coverage` | 必須 | 収録範囲（HIST-11） |
| `termination` | 任意 | 効力の終了（HIST-12） |

- `record`のRecord IDは、各版パッケージの`law.yaml`の`law.id`と一致しなければならない（MUST）。
- `sources`のSourceの`id`は、ファイルの中で一意でなければならない（MUST）。

## HIST-03 関係と内訳

| フィールド | 必須 | 内容 |
|---|---|---|
| `id` | 必須 | ファイルの中で一意な不透明な識別子。撤回した後も再利用しない |
| `type` | 必須 | `amends`（改正）／`totallyRevises`（全部改正）／`repeals`（廃止）／`succeeds`（後継） |
| `from` | 必須 | 起点のRecordRef（改正法令、全部改正後の新しいRecord、廃止を定める法令、後継の法令） |
| `evidence`、`verification` | 必須 | 関係全体の根拠と確認状態 |
| `promulgated` | 任意 | 起点の法令（改正法令、廃止を定める法令等）の公布の時点（TimePoint）。`amends`・`repeals`・`totallyRevises`では持つことを推奨する（SHOULD） |
| `items` | `amends`・`repeals`・`totallyRevises`で必須、`succeeds`で任意 | 対象範囲ごとの内訳。各項目は`id`、`scope`（Scope）、`effective`（TimePoint）、`evidence`、`verification`と、任意の`retroactive`（遡及の根拠。`raw`（遡及を定める規定の原文）と`evidence`） |
| `status` | 必須 | `active`／`retracted` |

- 関係は、終点のRecordの履歴マニフェストに置かなければならない（MUST）。起点のRecordに写してはならない（MUST NOT）。起点からは、RecordRefと関係の`id`の組で参照する。
- 内訳の`id`は、その関係の中で一意とし、再利用しない。
- 関係の撤回は、削除せず`status: retracted`とし、理由を`verification.reason`に残す。内訳の一部だけの撤回は、その項目の`verification.state`を`retracted`とし、関係全体の撤回と区別する。
- `repeals`・`totallyRevises`の`items`は、通常、終点のRecord全体を対象範囲（`scope.text`例: 「全部」）とする一項目で、`effective`に廃止・全部改正の効力の発生の時点を書く。一部だけを廃止する規定は`amends`（削る改正）とする。
- `status: active`の`repeals`・`totallyRevises`の関係は、`verification.state`が`retracted`でない内訳を二つ以上持ってはならない（MUST NOT）。廃止・全部改正の効力の発生の時点は一つであり、効力の終了の後の適用は適用関係（HIST-07）で表す。内訳を差し替えるときは、古い内訳を撤回し、新しい内訳を加える。すべての内訳を撤回した関係の扱いは、HIST-12による。
- 改正法の附則のうち、被改正法令の状態に関わる規定（施行期日、経過措置）は、関係・内訳の根拠として`evidence`で参照する（`locator`に「附則第一項」等）。改正法の附則は、版パッケージに写さない（[BASIC-03](../../v1alpha2/01-overview.md)）。

## HIST-04 起点と基礎状態

- 各Revisionは、直前の状態を`basedOn`（基礎状態のrevision_id）で参照する。版の順序は、施行日の比較ではなく、この参照で表す。
- `basedOn`を持たないRevisionは起点とし、`origin`を持たなければならない（MUST）。

  | `origin` | 意味 |
  |---|---|
  | `enactment` | 制定 |
  | `totalRevision` | 全部改正による新しいRecordの最初の状態。`totallyRevises: {record, relation}`で、旧Recordの全部改正の関係（RecordRefと関係の`id`）を参照しなければならない（MUST） |
  | `coverageStart` | 収録の開始。それより前の状態が未収録・不明 |

- 起点以外のRevisionは`origin`を持ってはならない（MUST NOT）。`origin`が`totalRevision`でないRevisionは、`totallyRevises`を持ってはならない（MUST NOT）。履歴マニフェストは一つ以上の起点を持つ。
- 一つのRevisionを、複数のRevisionが`basedOn`として参照してよい（MAY）。経過措置による新旧の状態の併存は、この分岐と適用関係（HIST-07）で表す。
- 適用の順が不明な場合は、`basedOn`を`{state: unknown, reason}`とし、推測で順序を補ってはならない（MUST NOT）。改正法令の法令番号の順などで、機械的に順序を決めてはならない（MUST NOT）。
- `basedOn`は、同じ履歴マニフェストに存在するrevision_idを指さなければならない（MUST）。`basedOn`の連なりは、循環してはならない（MUST NOT）。
- `status: active`のRevisionの`basedOn`は、`status: active`のRevisionを参照しなければならない（MUST）。撤回・統合・分割したRevision（HIST-10）を基礎状態とする参照は違反とする。基礎状態としていた版は、同じ`revisionFix`で付け替える。統合・分割したRevisionからは`replacedBy`の版へ、撤回したRevisionからは有効な版へ付け替える。

## HIST-05 成立根拠

- 起点以外のRevisionは、成立根拠`basis`を持たなければならない（MUST）。起点（`origin`を持つRevision）は、`basis`を持ってはならない（MUST NOT）。
- `basis`は、項目を一つだけ持つ配列とする。項目は、関係の`id`（`relation`）とその内訳の`id`の一覧（`items`）の組か、制定時の段階施行（`enactment`）のどちらかである。一つのRevisionの`basis`は、一つの関係の内訳（同じ改正法令）に限る。
- 制定時に施行時点が複数ある法令（段階施行）は、最初の施行の版を起点（`origin: enactment`）とし、起点に続くRevisionの`basis`を`enactment`（`scope`、`evidence`）とする。
- 段階施行の法令（起点が`origin: enactment`で、`basis`が`enactment`の`status: active`のRevisionを持つ法令）の版のうち、版パッケージを持つ版（`text.status`が`complete`・`partial`）の本文は、制定時に公布された本文の全体に、その版までの改正を反映したものでなければならない（MUST）。`partial`の版は、`gaps`の範囲だけを欠いてよい。その版の施行時点で後の施行時点を待つ規定（未施行の規定）を、未施行であることを理由に本文から除いてはならない（MUST NOT）。未施行の規定を除くと、版の中の参照（「第四條の規定」等）が解決しなくなり、版ごとにノードの集合が変わるためである。
- この要求は、起点の版と、起点と後の施行時点のRevisionの間にある改正の版にも及ぶ。後の施行時点のRevisionをまだ記録していない段階でも、版の本文は同じである。
- 未施行の規定は本文に書くので、収録できていない本文の範囲（`text.status: partial`の`gaps`。[HIST-08](#hist-08-本文の状態)）に当たらない。
- 未施行の範囲は、後の施行時点のRevisionの`basis`の`enactment`の`scope`で表し、前の版の側には記録しない。版の本文だけからは、どの規定がその版の施行時点で施行されているかは分からない。派生ビューで示す内容は、[VIEW-02](../../common/v1alpha1/09-views.md#view-02-選択規則と主張の限界)による。
- `basis`が参照する関係と内訳は、同じ履歴マニフェストに存在しなければならない（MUST）。
- `basis`が参照する内訳の`effective`と、Revisionの`effective`は一致しなければならない（MUST）。両方が`date`の場合に検査する。
- 同じ関係・同じ施行時点のRevisionが二つ以上あってはならない（MUST NOT）。`status: active`のRevisionだけを数え、施行時点は、両方が確定日（[TIME-03](../../common/v1alpha1/06-time.md)）のときに日付（`value`）の一致で比べる。確定日は精度が日の値だけなので、精度が月・年の値や`pending`等、確定日でない施行時点を含む組は、この規則では比べない。
- `status: active`のRevisionの`basis`は、`status: retracted`の関係と、`verification.state: retracted`の内訳を参照してはならない（MUST NOT）。
- 改正法の公布の時点をRevisionに写さない。関係の`promulgated`から得る。

## HIST-06 版の時間の情報と順序

- 施行時点`effective`（TimePoint）は、すべてのRevisionが持たなければならない（MUST）。
- 記録上の時間は、正本のGit履歴から得る。履歴マニフェストに記録の日時を写さない。commitの日時を法的な時点に使ってはならない（MUST NOT）。
- 順序について、次を報告する。

  | 状況 | 結果 |
  |---|---|
  | 基礎状態の施行時点が自身より後（両方とも確定日。[TIME-03](../../common/v1alpha1/06-time.md)） | 警告。`basis`が参照する内訳が`retroactive`を持てば警告しない |
  | `basis`が参照する関係の`promulgated`が、自身の施行時点より後（両方とも確定日） | 警告。`basis`が参照する内訳が`retroactive`を持てば警告しない |
  | 上の二行で、比べる一方が確定日でない。2行目では、`basis`が参照する関係が`promulgated`を持たない場合も含む | 判定不能。合格にも不合格にもしない |
  | `basedOn`が`unknown` | 保留 |

- 基礎状態が収録開始の起点（`origin: coverageStart`）で、その施行時点が確定日でない場合は、1行目を比べず、報告しない。収録開始は法令上の出来事ではなく、施行時点が不明なのが通常だからである。
- 順序の検査は警告しか出さないので、その判定不能は区分の結果に影響しない（[VAL-04](../../common/v1alpha1/03-validation.md)）。

## HIST-07 適用関係

- 各Revisionは、`applicability`を持たなければならない（MUST）。

  | `state` | 意味 | 必須の情報 |
  |---|---|---|
  | `noneConfirmed` | 適用の条件がないことを確認した | `evidence` |
  | `unsurveyed` | 適用の条件を調べていない | — |
  | `conditions` | 適用の条件がある | `conditions`（各項目は`scope`、`text`（条件の原文）、`evidence`、`verification`、任意の`machineEvaluable`） |

- `noneConfirmed`と`unsurveyed`を区別しなければならない（MUST）。条件がないことの確認には根拠を要する。
- `state`が`conditions`でない`applicability`は、`conditions`を持ってはならない（MUST NOT）。
- 効力が終了した後の適用（経過措置で「なお従前の例による」等）は、`applicability`で表す（HIST-12）。
- 本版は、時点照会（指定した日・対象・条件に適用されるRevisionを求めること）の入力・手順・結果を定めない。L3（履歴構造）の合格は、ある日に適用されるRevisionが一つに決まることを保証しない。
- 履歴マニフェストを読んで時点照会を提供する実装は、日付だけから一つのRevisionが決まると仮定してはならない（MUST NOT）。基礎状態の分岐（HIST-04）と適用の条件により、複数の`status: active`のRevisionが、同じ日に施行済みで、適用され得る状態（候補）になり得る。
- 時点照会を提供する実装は、`unsurveyed`のRevisionを、適用の条件がないRevisionとして扱ってはならず、適用されないRevisionとして候補から除いてもならない（MUST NOT）。照会の結果でも、`noneConfirmed`と`unsurveyed`の区別を保つ。

## HIST-08 本文の状態

| フィールド | 必須 | 内容 |
|---|---|---|
| `status` | 必須 | `complete`（本文がそろっている）／`partial`（一部が欠けている）／`absent`（本文がない）。本文の状態であり、Revisionの`status`（記録の生存の状態）とは別である（名前の例外は[DDR-022](../../../docs/design-decisions/022-spec-structure.md)） |
| `package` | 任意 | 版パッケージの`law.yaml`のパス。書く場合は`versions/<revision_id>/law.yaml`（自身のrevision_id）でなければならない（MUST）。省略時も同じパスとする |
| `gaps` | `partial`で必須 | 欠落の範囲（Scopeの一覧） |
| `sources` | `complete`・`partial`で必須 | 本文を起こした原典資料（Evidenceの一覧） |
| `glyphCheck` | `complete`・`partial`で必須 | 字体の照合状態。`verified`（原典の字体と照合した）／`unverified`（新字体化された資料等に由来し、照合していない） |
| `glyphNotes` | 注記がある場合は必須 | 字体注記のファイルのパス。`versions/<revision_id>/glyphs.yaml`（自身のrevision_id）でなければならない（MUST） |
| `variances` | 任意 | 原典資料の間の文言の食い違い（loj-glyphsのGLYPH-07）。各項目は`scope`（Scope）、`readings`（資料ごとの文言。2件以上。各項目は`source`、`locator`、`raw`）、`verification`と、任意の`resolvedBy`（食い違いを解いた官報正誤のEvidence） |
| `correctionsSurveyed` | 任意 | 官報正誤を確かめたか。`state`（`surveyed`／`unsurveyed`）と、`surveyed`では`evidence`（確かめた資料）、`unsurveyed`では`reason`。`surveyed`は`reason`を、`unsurveyed`は`evidence`を持たない |

- `complete`・`partial`のRevisionの版パッケージは、存在しなければならない（MUST）。存在しなければ違反とする。版パッケージが存在するが読めない場合は、L3では判定せず、版パッケージのL0で扱う。
- `absent`のRevisionに、版パッケージを置いてはならない（MUST NOT）。
- 版パッケージのディレクトリ`versions/<revision_id>/`は、履歴マニフェストの`complete`・`partial`のRevisionと一対一に対応しなければならない（MUST）。`absent`のRevisionは、ディレクトリを持たない。
- `glyphCheck: verified`の意味は、字体注記の要件（loj-glyphs）による。
- 字体注記の検査（loj-glyphs。L3（根拠））にかけるファイルは、`complete`・`partial`のRevisionごとに、`versions/<自身のrevision_id>/glyphs.yaml`とする。`glyphNotes`の値では決めない。
  - 自身のパスに`glyphs.yaml`があるRevisionは、`glyphNotes`を持たなければならない（MUST）。値は、表のとおり自身のパスとする。
  - `glyphNotes`が自身のパスでない場合は、本要件の違反とし、指す先のファイルを、このRevisionの字体注記として読まない。誤った指定を優先しないためである（[FIXREF-03](../../common/v1alpha1/08-references.md)の`path`と同じ考え方）。
  - `absent`のRevisionのディレクトリ（持たないとした`versions/<revision_id>/`）に`glyphs.yaml`があっても、字体注記の検査にかけない。ディレクトリがあることを、本要件の違反として報告する。
  - `complete`・`partial`のRevisionのどれにも当たらない`glyphs.yaml`は、字体注記の検査にかけない。
- `variances`の項目は、官報正誤が見つからない間は`verification.state`を`disputed`とする。官報正誤で解いた場合は`resolvedBy`を持ち、本文を正誤後の文言にする（GLYPH-07）。`readings`の`source`は、同じ履歴マニフェストのSourceを指す（TYPE-03）。
- `variances`と`correctionsSurveyed`は、本文のあるRevision（`complete`・`partial`）に書く。`absent`のRevisionは、`package`・`sources`・`glyphCheck`・`glyphNotes`・`variances`・`correctionsSurveyed`を持ってはならない（MUST NOT）。
- `partial`でないRevisionは、`gaps`を持ってはならない（MUST NOT）。`complete`・`partial`のRevisionの`sources`は、一つ以上のEvidenceを持たなければならない（MUST）。
- 本文の状態の確認の状況（官報正誤を確かめていない等）を、`sources`の`locator`に書いてはならない（MUST NOT）。`locator`は箇所の原文表記である（TYPE-03）。官報正誤の確認の状況は`correctionsSurveyed`に書く。

## HIST-09 外部識別子

- Revisionは、版を対象とする外部識別子（e-Govの法令履歴ID等）を`externalIds`に持てる（MAY）。各項目は`issuer`（発行元。例: `egov`）、`kind`、`value`、`evidence`、`verification`を持つ。`kind`は`revision`とする。
- 外部識別子をrevision_idの代わりに使ってはならない（MUST NOT）。
- 版の粒度をそろえているため、e-Govの版とRevisionは原則として一対一に対応する。ただし、LoJが確かめた施行時点・順序とe-Govの収録が異なる場合や、e-Govが基準日の時点から収録している場合は、対応なし・複数の候補を許す。
- Recordを対象とする外部識別子は、登録台帳に置き、履歴マニフェストには写さない（[HIST-01](#hist-01-ファイルと形式)）。履歴マニフェストの外部識別子の`kind`が`revision`に限られるのは、このためである。`kind`が`revision`でない外部識別子は、HIST-01の違反として報告する。

## HIST-10 Revisionの状態

- 各Revisionは`status`（`active`／`retracted`／`merged`／`split`）を持たなければならない（MUST）。
- 撤回・統合・分割したRevisionを、履歴マニフェストから削除してはならない（MUST NOT）。
- `merged`・`split`のRevisionは、`replacedBy`（`revisions`＝統合先・分割先のrevision_idの一覧、`evidence`、`verification`）を持たなければならない（MUST）。`replacedBy`のrevision_idは、同じ履歴マニフェストに存在しなければならない（MUST）。`merged`・`split`でないRevisionは、`replacedBy`を持ってはならない（MUST NOT）。
- 撤回・統合・分割の変更は、変更の種類`revisionFix`（[CHG-01](../../common/v1alpha1/07-changes.md)）で記録する。

## HIST-11 収録範囲

| フィールド | 必須 | 内容 |
|---|---|---|
| `state` | 必須 | `confirmed`／`unconfirmed`。収録の網羅性の確認状態 |
| `surveyed` | 任意 | 確認した資料・期間・対象範囲 |
| `knownGaps` | 任意 | 既知の欠落（未収録の改正、未取得の版等） |

- Revisionの数を、収録の網羅性の根拠にしてはならない（MUST NOT）。

## HIST-12 効力の終了

- `termination`の各項目は、`type`と根拠を持たなければならない（MUST）。

  | `type` | 根拠 | 時点 |
  |---|---|---|
  | `repealed` | `repeals`の関係の`id`（`relation`） | 関係の`items`の`effective`。`termination`に写さない |
  | `totallyRevised` | `totallyRevises`の関係の`id`（`relation`） | 同上 |
  | `expired` | 自身の規定による失効（時限、条件付きの失効）。`evidence`（例: 附則第二条）と`verification` | `effective`（TimePoint）。条件付きで時点が決まらないものは`pending`とし、条件の原文を`delegation`に書く |

- `repealed`・`totallyRevised`の項目は、`effective`、`evidence`、`verification`を持ってはならない（MUST NOT）。時点と根拠・確認状態は、根拠とする関係の側にある。`expired`の項目は、`relation`を持ってはならない（MUST NOT）。
- `repealed`・`totallyRevised`の`relation`は、同じ履歴マニフェストに存在し、種類が合う関係でなければならない（MUST）。
- `repealed`・`totallyRevised`の`relation`は、`status: active`の関係でなければならない（MUST）。撤回した関係（`status: retracted`）を根拠とする項目は違反とする。
- 終了の時点は、関係の`items`のうち、`verification.state`が`retracted`でない内訳の`effective`から得る。撤回した内訳の`effective`は使わない。すべての内訳を撤回した関係を根拠としてはならない（MUST NOT）。撤回していない内訳は一つに限られる（[HIST-03](#hist-03-関係と内訳)）ので、終了の時点はその内訳の`effective`である。撤回していない内訳が二つ以上ある関係（HIST-03の違反）では、終了の時点を決めず、次の警告を判定しない。
- 関係を撤回するときは、それを根拠とする`termination`の項目を、同じ変更（`metadata`、[CHG-01](../../common/v1alpha1/07-changes.md)）で削る。関係の撤回は、Revisionの撤回・統合・分割（`revisionFix`）ではなく、関係の更新である。効力の終了を別の関係で表せる場合は、その関係を根拠とする項目に置き換える。撤回の事実と理由は、関係の側に残る（HIST-03）。
- 登録台帳の`status`は登録の状態であり、効力の終了で変えない。効力の終了を登録台帳に写してはならない（MUST NOT）。
- 終了の時点（`repealed`・`totallyRevised`では、撤回した内訳を除いて得た時点）より後に施行されるRevisionがあれば、警告とする。施行前に廃止された等の場合があるため、不合格にしない。
  - 比べるのは、`status: active`のRevisionの施行時点`effective`と、終了の時点である。両方が確定日（[TIME-03](../../common/v1alpha1/06-time.md)）のときに日付（`value`）で比べ、Revisionの施行時点が終了の時点より後（同じ日を含めない）であれば警告とする。
  - 一方が確定日でない場合（`expired`の`pending`、Revisionの`unknown`等）は、判定不能とする。この検査は警告しか出さないので、その判定不能は区分の結果に影響しない（[VAL-04](../../common/v1alpha1/03-validation.md)）。

## HIST-13 表の行のkeyの再使用

- 同じ履歴マニフェストの`basedOn`の連なり（起点から各Revisionまで）で、版パッケージの表の行の`key`（v1alpha2の[TABLE-02](../../v1alpha2/09-structures.md)）の再使用を検査する。同じ表のID（v1alpha2のID-01）の下で、ある版にあった`key`が、後の版でなくなり、さらに後の版に再び現れたら違反とする。`basedOn`が分岐する場合は、起点から各Revisionまでの連なりごとに独立して比べ、別の枝の版とは比べない。
- 付け方の誤りを記録訂正で直して消えた`key`（TABLE-02）も、削った`key`と同じに扱う。後の版でその`key`を戻すと違反になる。
- 比べるのは、`status: active`で、`text.status: complete`で、版パッケージがL1に合格した版だけとする。本文のない版、本文が一部しかない版、L1が不合格の版は、行がないことの根拠にならないので飛ばす。飛ばした版があっても、判定不能とはしない。このため、飛ばした版の間で`key`が消えて再び現れた場合は、見逃すことがある。
- 表の番号が変わると表のIDが変わるので、別の表として扱う。
- 既存の行の`key`の維持と、基礎の版の同じ行が同じ`key`を持つことは、本要件では検査しない（TABLE-02）。
