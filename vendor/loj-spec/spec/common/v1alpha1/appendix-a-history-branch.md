# 付録A（参考） historyブランチの形式

本付録は参考であり、規範ではない。派生ビューの規範は[VIEW-01〜07](09-views.md)だけである。ほかの形式の派生ビューを妨げない。

## 構成

- `history`ブランチに、選択規則で選んだ版を、施行日の順に一つずつcommitとして積む。
- 各commitの内容は、その版の版パッケージを写したものと、ブランチの最上位の索引ファイル（`INDEX.md`）とする。
- 各commitに、revision_idの名前のタグを付ける（[VIEW-04](09-views.md)）。可読ラベルによる別名のタグを付けてもよいが、ラベルの訂正で変わり得る。
- 選択規則で除いた版（[VIEW-02](09-views.md)）は、各commitのtrailer（`LoJ-View-Excluded`）と、索引ファイルの両方で示す。除いた版については、trailer（`LoJ-View-Excluded`）はrevision_idだけを持ち、除いた理由は索引ファイルが持つ。列に含めた版の未確定の施行条件・適用の条件・併存する版・後の段階施行の施行時点（VIEW-02の表）は、索引ファイルだけで示す。
- commitの作者・日時・メッセージは正本から決まった方法で作られ、同じ記録版と同じ生成器の版からは、commitのハッシュまで一致する（[VIEW-03](09-views.md)）。commitの日時は、法的な日付ではない。

## trailer

各commitのメッセージに、次のtrailerを置く。

| trailer | 値 |
|---|---|
| `LoJ-View-Revision` | そのcommitの版のrevision_id |
| `LoJ-View-Effective` | 施行日（`date`の値）。commitの日時には使わない |
| `LoJ-View-Source-Commit` | 生成元の正本のcommit（完全なハッシュ） |
| `LoJ-View-Package` | 版パッケージのパス |
| `LoJ-View-Generator` | 生成器の名前と版 |
| `LoJ-View-Rule` | 選択規則の名前 |
| `LoJ-View-Excluded` | 選択規則で除いた版のrevision_id（カンマと空白で区切る）。生成したすべてのcommitに同じ一覧を書く。除いた版がなければ省略する |

```text
昭和41年法律第86号による改正

LoJ-View-Revision: zwgmz1rt
LoJ-View-Effective: 1966-06-25
LoJ-View-Source-Commit: 0123456789abcdef0123456789abcdef01234567
LoJ-View-Package: versions/zwgmz1rt/law.yaml
LoJ-View-Generator: loj-view 0.1.0
LoJ-View-Rule: confirmed-effective-complete
```

## 索引ファイル

`INDEX.md`には、次を書く。

- 適用した選択規則
- 除いた版と、それぞれの理由（例: 施行時点が`pending`、本文の状態が`partial`）
- 列に含めた版の注記: 施行時点が確定日でない版の原表記、適用の条件がある版の条件の原文、併存する版のrevision_id、後に段階施行の施行時点がある版の後のRevisionのrevision_idと施行の範囲の原文（版ごとに、その版のrevision_idとともに）
- ビューに含まれない情報（字体注記、出典と確認状態、関係、適用条件と収録範囲、除いた版）
- 生成元の正本のcommitと、生成器の版

## 注意

- 正本が一つ進むと、生成元のcommitの記録が変わるため、ビューのすべてのcommitのハッシュが変わる。ビューのcommitへのリンクは安定しない。
- 引用には、正本の固定参照（[FIXREF-02](08-references.md)）を使う。
