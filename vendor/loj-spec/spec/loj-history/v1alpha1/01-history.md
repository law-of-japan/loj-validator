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
- `schemaVersion`は厳密に`loj-history/v1alpha1`でなければならない（MUST）。YAMLのデータモデルは[CORE-03](../../common/v1alpha1/01-overview.md)に従う。schemaは閉じており、未知のフィールドを拒否する。
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
| `items` | `amends`・`repeals`・`totallyRevises`で必須、`succeeds`で任意 | 対象範囲ごとの内訳。各項目は`id`、`scope`（Scope）、`effective`（TimePoint）、`evidence`、`verification` |
| `status` | 必須 | `active`／`retracted` |

- 関係は、終点のRecordの履歴マニフェストに置かなければならない（MUST）。起点のRecordに写してはならない（MUST NOT）。起点からは、RecordRefと関係の`id`の組で参照する。
- 内訳の`id`は、その関係の中で一意とし、再利用しない。
- 関係の撤回は、削除せず`status: retracted`とし、理由を`verification.reason`に残す。内訳の一部だけの撤回は、その項目の`verification.state`を`retracted`とし、関係全体の撤回と区別する。
- `repeals`・`totallyRevises`の`items`は、通常、終点のRecord全体を対象範囲（`scope.text`例: 「全部」）とする一項目で、`effective`に廃止・全部改正の効力の発生の時点を書く。一部だけを廃止する規定は`amends`（削る改正）とする。
- 改正法の附則のうち、被改正法令の状態に関わる規定（施行期日、経過措置）は、関係・内訳の根拠として`evidence`で参照する（`locator`に「附則第一項」等）。改正法の附則を版パッケージに写してはならない（MUST NOT。v1alpha2のBASIC-03）。

## HIST-04 起点と基礎状態

- 各Revisionは、直前の状態を`basedOn`（基礎状態のrevision_id）で参照する。版の順序は、施行日の比較ではなく、この参照で表す。
- `basedOn`を持たないRevisionは起点とし、`origin`を持たなければならない（MUST）。

  | `origin` | 意味 |
  |---|---|
  | `enactment` | 制定 |
  | `totalRevision` | 全部改正による新しいRecordの最初の状態。`totallyRevises: {record, relation}`で、旧Recordの全部改正の関係（RecordRefと関係の`id`）を参照しなければならない（MUST） |
  | `coverageStart` | 収録の開始。それより前の状態が未収録・不明 |

- 起点以外のRevisionは`origin`を持ってはならない（MUST NOT）。履歴マニフェストは一つ以上の起点を持つ。
- 一つのRevisionを、複数のRevisionが`basedOn`として参照してよい（MAY）。経過措置による新旧の状態の併存は、この分岐と適用関係（HIST-07）で表す。
- 適用の順が不明な場合は、`basedOn`を`{state: unknown, reason}`とし、推測で順序を補ってはならない（MUST NOT）。改正法令の法令番号の順などで、機械的に順序を決めてはならない（MUST NOT）。
- `basedOn`の循環と、存在しないrevision_idへの参照は違反とする。

## HIST-05 成立根拠

- 起点以外のRevisionは、成立根拠`basis`を持たなければならない（MUST）。`basis`の各項目は、関係の`id`（`relation`）と、その内訳の`id`の一覧（`items`）の組である。
- 制定時に施行時点が複数ある法令（段階施行）は、起点に続くRevisionの`basis`を`enactment`（`scope`、`evidence`）とする。
- 一つのRevisionの`basis`は、一つの関係の内訳（同じ改正法令）に限る。
- `basis`が参照する関係と内訳は、同じ履歴マニフェストに存在しなければならない（MUST）。
- `basis`が参照する内訳の`effective`と、Revisionの`effective`は一致しなければならない（MUST）。両方が`date`の場合に検査する。
- 同じ関係・同じ施行時点のRevisionが二つ以上あってはならない（MUST NOT）。
- 改正法の公布日をRevisionに写さない。改正法のRecordと関係から得る。

## HIST-06 版の時間の情報と順序

- 施行時点`effective`（TimePoint）は、すべてのRevisionが持たなければならない（MUST）。
- 記録上の時間は、正本のGit履歴から得る。履歴マニフェストに記録の日時を写さない。commitの日時を法的な時点に使ってはならない（MUST NOT）。
- 順序について、次を報告する。

  | 状況 | 結果 |
  |---|---|
  | 基礎状態の施行時点が自身より後（両方とも`date`） | 警告。遡及等の根拠が記録されていれば警告しない |
  | 改正法の公布日が施行時点より後（両方とも`date`） | 警告。遡及の根拠が記録されていれば警告しない |
  | 比べる一方が`date`でない | 判定不能。合格にも不合格にもしない |
  | `basedOn`が`unknown` | 保留 |

## HIST-07 適用関係

- 各Revisionは、`applicability`を持たなければならない（MUST）。

  | `state` | 意味 | 必須の情報 |
  |---|---|---|
  | `noneConfirmed` | 適用の条件がないことを確認した | `evidence` |
  | `unsurveyed` | 適用の条件を調べていない | — |
  | `conditions` | 適用の条件がある | `conditions`（各項目は`scope`、`text`（条件の原文）、`evidence`、`verification`、任意の`machineEvaluable`） |

- `noneConfirmed`と`unsurveyed`を区別しなければならない（MUST）。条件がないことの確認には根拠を要する。
- 効力が終了した後の適用（経過措置で「なお従前の例による」等）は、`applicability`で表す（HIST-12）。

## HIST-08 本文の状態

| フィールド | 必須 | 内容 |
|---|---|---|
| `status` | 必須 | `complete`（本文がそろっている）／`partial`（一部が欠けている）／`absent`（本文がない） |
| `package` | 任意 | 版パッケージの`law.yaml`のパス。省略時は`versions/<revision_id>/law.yaml` |
| `gaps` | `partial`で必須 | 欠落の範囲（Scopeの一覧） |
| `sources` | `complete`・`partial`で必須 | 本文を起こした原典資料（Evidenceの一覧） |
| `glyphs` | `complete`・`partial`で必須 | 字体の照合状態。`verified`（原典の字体と照合した）／`unverified`（新字体化された資料等に由来し、照合していない） |
| `glyphNotes` | 注記がある場合は必須 | 字体注記のファイルのパス |

- `complete`・`partial`のRevisionの版パッケージは、存在しなければならない（MUST）。存在しなければ違反とする。版パッケージが存在するが読めない場合は、L3では判定せず、版パッケージのL0で扱う。
- `absent`のRevisionに、版パッケージを置いてはならない（MUST NOT）。
- 版パッケージのディレクトリ`versions/<revision_id>/`は、履歴マニフェストのRevisionと一対一に対応しなければならない（MUST）。
- `glyphs: verified`の意味は、字体注記の要件（loj-glyphs）による。

## HIST-09 外部識別子

- Revisionは、版を対象とする外部識別子（e-Govの法令履歴ID等）を`externalIds`に持てる（MAY）。各項目は`issuer`（発行元。例: `egov`）、`kind`（`revision`）、`value`、`evidence`、`verification`を持つ。
- 外部識別子をrevision_idの代わりに使ってはならない（MUST NOT）。
- 版の粒度をそろえているため、e-Govの版とRevisionは原則として一対一に対応する。ただし、LoJが確かめた施行時点・順序とe-Govの収録が異なる場合や、e-Govが基準日の時点から収録している場合は、対応なし・複数の候補を許す。
- Recordを対象とする外部識別子は、登録台帳に置く。

## HIST-10 Revisionの状態

- 各Revisionは`status`（`active`／`retracted`／`merged`／`split`）を持たなければならない（MUST）。
- 撤回・統合・分割したRevisionを、履歴マニフェストから削除してはならない（MUST NOT）。
- `merged`・`split`のRevisionは、`replacedBy`（`revisions`＝統合先・分割先のrevision_idの一覧、`evidence`、`verification`）を持たなければならない（MUST）。`replacedBy`のrevision_idは、同じ履歴マニフェストに存在しなければならない。
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

- `repealed`・`totallyRevised`の`relation`は、同じ履歴マニフェストに存在し、種類が合う関係でなければならない（MUST）。
- 登録台帳の`status`は登録の状態であり、効力の終了で変えない。効力の終了を登録台帳に写してはならない（MUST NOT）。
- 終了の時点より後に施行されるRevisionがあれば、警告とする。施行前に廃止された等の場合があるため、不合格にしない。
