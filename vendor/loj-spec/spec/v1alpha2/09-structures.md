# 新しい構造

## PARA-01 無番号の項

- 原典に項番号がない項は、開始のマーカーに`numbering=implicit`を書く。省略時は`numbering=explicit`（番号がある）とする。

  ```markdown
  <!-- loj:paragraph number=2 numbering=implicit -->
  昭和二年勅令第二十五号は、これを廃止する。
  <!-- loj:end-paragraph -->
  ```

- `implicit`の項でも、`number`は必須とし、入力者が明示する。値は、同じ親の項の中での原典上の順序（1から始まる連番）でなければならない（MUST）。番号のない項も、法令の引用では順序で数えるためである。
- 同じ親の項は、すべて`explicit`か、すべて`implicit`でなければならない（MUST）。混在は違反とする。
- 項が一つだけの親では`explicit`とし、`implicit`を書いてはならない（MUST NOT）。第1項は番号を省略できる（DISP-01）ため、どちらでも同じ原文になり、表現が二通りになるのを避ける。
- `implicit`の項には表示番号を置かない。DISP-01の表示番号の検査を行わず、`display`は空文字列とする。本文の先頭が数字で始まっても、表示番号とみなさない。
- 意味ASTのParagraphは、`numbering`（`explicit`／`implicit`）を必ず持つ。

## TABLE-01 表の構造

- 表は、項・号・細分の本文の中に置く`Table`ノードとする。本文の中での位置（出現の順）を保持する。附属資料の中の表は、不透明な本文とする。
- 表は、`table`・`row`・`cell`のマーカーで書く。行とセルは出現の順に並べる。

  ```markdown
  <!-- loj:table number=1 -->
  <!-- loj:row key=b6gr -->
  <!-- loj:cell -->
  元日
  <!-- loj:end-cell -->
  <!-- loj:cell -->
  一月一日
  <!-- loj:end-cell -->
  <!-- loj:cell -->
  年のはじめを祝う。
  <!-- loj:end-cell -->
  <!-- loj:end-row -->
  <!-- loj:end-table -->
  ```

- 表の`number`は、同じ親の中の表の順序（1から始まる連番）で、入力者が明示しなければならない（MUST）。
- 罫線、文字の配置、縦書き・横書き、列の幅は記録しない。見出しの行の区別は、本版では型付けしない。
- 項・号・細分の本文の中のGFMの表は、違反とする。法令の表はマーカーで書かなければならない（MUST）。
- 法令の表を、箇条書き等の本文で書いてはならない（MUST NOT。BASIC-03）。これは機械では検査できず、原典照合（L3）で確かめる。

## TABLE-02 行のkey

- 行には、入力者が属性`key`を付けなければならない（MUST）。値はCrockford Base32の小文字4文字（`0-9a-hjkmnp-tv-z`）で、同じ表の中で一意とし、意味を持たない。
- 行のIDは`{表のID}/row/{key}`とする（ID-01）。
- 改正・復元・記録訂正で、既存の行の`key`を変えてはならない（MUST NOT）。削った行の`key`を、同じRecordの同じ表で再使用してはならない（MUST NOT）。
- 改正後の版の行は、基礎の版の同じ行と同じ`key`を持たなければならない（MUST）。行を加える改正・復元では、新しい`key`を発行する。
- `key`を行の内容（祝日の名前等）から作ってはならない（MUST NOT）。新しい`key`はランダムに選び、同じ表の既存の`key`（過去の版で削った行のものを含む）と衝突しないことを確かめる。

## TABLE-03 セル

- セルの内容は本文（text）だけとし、項・号・表を置いてはならない（MUST NOT）。
- セルのIDは`{行のID}/cell/{c}`とし、`c`は行の中での位置（1始まり）とする。セルの`number`も同じ値とする。
- セルの結合は、属性`rowspan`・`colspan`（2以上の整数）で書く。既定は1とし、値1を書いてはならない（MUST NOT）。表現が二通りになるのを避けるためである。
- 意味ASTのCellは、`rowspan`・`colspan`を、マーカーに書いた場合にだけ持つ。
- 結合したセルが占める位置は、後の行のセルの位置の数え方に影響しない。`c`は、その行に書いたセルの順序である。

## PRE-01 前文

- 前文は、`preamble`のマーカーで書き、意味ASTでは`Preamble`ノードとする。
- 本則のscopeの最初のファイルで、最初の構造の見出しより前に置かなければならない（MUST）。制定文がある場合は、制定文の後に置く。
- 前文の内容は本文だけとする。前文の中の段落は、本文の段落として保持し、項として型付けしない。
- 意味ASTでは、Lawの子として、MainProvisionの前に置く。IDは`{law.id}/preamble`とする。

```markdown
<!-- loj:enact-statement -->
内閣は、……この政令を制定する。
<!-- loj:end-enact-statement -->
<!-- loj:preamble -->
日本国民は、……
<!-- loj:end-preamble -->
```

## PRE-02 制定文

- 制定文は、`enact-statement`のマーカーで書き、意味ASTでは`EnactStatement`ノードとする。
- 本則のscopeの最初のファイルで、最初の構造の見出しと前文より前に置かなければならない（MUST）。
- 制定文の内容は本文だけとする。
- 意味ASTでは、Lawの最初の子とする。IDは`{law.id}/enact-statement`とする。
