# 意味ASTとソース

## AST-01 意味ASTの形

- 規範のJSON表現は[AST Schema](../../schemas/v1alpha2/ast.schema.json)による。`schemaVersion`、Manifestと同じ`law`、`root`を持つ。
- ノードは`kind`、`id`、`number`、`display`、`content`を持つ。Articleは`status`を、Subitemは`level`を、Paragraphは`numbering`を、Rowは`key`を持つ。Cellは`rowspan`・`colspan`を持てる（[TABLE-03](09-structures.md)）。ほかのフィールドを持ってはならない（MUST NOT）。
- `number`は正規の番号の文字列とする。番号を持たないLaw、MainProvision、SupplementaryProvision、AppendedMaterial、EnactStatement、Preamble、Rowは`null`とする。Cellの`number`は行の中での位置（1始まり）とする。
- `display`は次のとおりとする。Lawは`law.title`、MainProvisionは空文字列、SupplementaryProvisionは附則の見出しの原文のラベル（HEAD-03）、AppendedMaterialは宣言した`title`。Table、Row、Cell、EnactStatement、Preambleは空文字列。
- `content`は、textのオブジェクトまたは子ノードの、順序のある配列でなければならない（MUST）。textは原文のMarkdownであり、HTMLにした結果ではない。
- 親と子は次のとおりとする。

  | 親 | 許す子・回数 |
  |---|---|
  | Law | EnactStatement 0〜1、Preamble 0〜1、MainProvision ちょうど1、SupplementaryProvision 0以上、AppendedMaterial 0以上。この順 |
  | MainProvision | rank 1〜5の分類またはArticle、計1以上 |
  | Part/Chapter/Section/Subsection/Division | 自身より大きいrankの分類またはArticle、計1以上 |
  | Article（active） | Paragraph 1以上 |
  | Article（deleted） | 子なし、本文なし |
  | SupplementaryProvision | 分類・Articleの群、またはParagraphの群のどちらか、計1以上 |
  | Paragraph | Item 0以上、Table 0以上、text 1以上 |
  | Item | level=1のSubitem 0以上、Table 0以上、text 1以上 |
  | Subitem | 次のlevelのSubitem 0以上、Table 0以上、text 1以上（level 10は子のSubitemを持たない） |
  | Table | Row 1以上 |
  | Row | Cell 1以上 |
  | Cell | text ちょうど1 |
  | EnactStatement、Preamble | text ちょうど1 |
  | AppendedMaterial | text 1以上、意味の子なし |

- textを許すのは、Paragraph、Item、Subitem、Cell、EnactStatement、Preamble、AppendedMaterialだけである。
- 子の前後に本文を許し、出現の順を保持しなければならない（MUST）。連続するtextは結合して一つの要素とし、空文字列のtextを作らない。
- 本文の区間は、開始のマーカーの行末の直後から、次の子の開始行の行頭、または終了のマーカーの行頭までとする。子の終了行の行末の直後から、親の本文を再開する。空行を保持し、本文の末尾の改行も保持する。構造の外の区切りの空行は、意味ASTに含めない。
- ASTのフィールドの順は意味を持たず、配列の順は意味を持つ。

## AST-02 意味の検査

- すべてのノードのIDは一意でなければならない（MUST）。
- 親子の制約、IDの導出、元の入力との本文の一致、`display`と`number`、Manifestのメタデータの一致は、schemaだけでは十分に検査できない。意味の検査を必須とする。
- 入力が不合格の場合、部分的なASTを返してよい（MAY）が、有効な完全なASTとして返してはならない（MUST NOT）。

## SRC-01 ソースの保持と位置

- 実装は、意味ASTとは別に、元の入力の全バイトとソースの対応を保持しなければならない（MUST）。意味のJSONだけでは、損失のないファイルの保存形式にならない。完全に復元するため、マーカー、見出し、区切りの空白、最終の改行を含む元の`law.yaml`とMarkdownを保管する。
- 元の入力の再構成に、正規の番号や`display`だけを使ってはならない（MUST NOT）。
- 位置は`{path, start, end}`で、元のファイルのUTF-8のバイト数による半開区間[start, end)とする。CRLFは2バイト、日本語は実際のUTF-8の長さとする。境界でUnicodeの文字を分断しない。
- 合成したLaw・MainProvisionは、空の位置の列を持つ。分類・条は見出しの行（行末を含む）を、明示のコンテナ（項・号・細分・表・行・セル・前文・制定文）は開始行の行頭から終了行の行末までを、附則・附属資料のscopeは最初の見出しの行を位置とする。本文の片は、文字列と一致する位置を持ち、複数のファイルにわたる場合は位置の列を持つ。
- 行・列の表示は任意であり、バイト位置の代わりにならない。
- 診断の位置は次のとおりとしなければならない（MUST）。未閉鎖は開始のマーカーの行、対応しない終了はその終了の行、属性の不正はその行、重複は後の定義の行、見出しの違反はその行、表示の不一致は本文のその行。Manifestの意味の違反は、`path: law.yaml`の文書全体を指してよい（MAY）。
