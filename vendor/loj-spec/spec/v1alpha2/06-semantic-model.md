# 意味ASTとソース

## AST-01 意味ASTの形

- 規範のJSON表現は[AST Schema](../../schemas/v1alpha2/ast.schema.json)による。`schemaVersion`、Manifestと同じ`law`、`root`を持つ。
- ノードは`kind`、`id`、`number`、`display`、`content`を持つ。Articleは`status`を、Subitemは`level`を、Paragraphは`numbering`を、Rowは`key`を持つ。Cellは`rowspan`・`colspan`を（[TABLE-03](09-structures.md)）、Paragraphは`caption`を（[PARA-02](09-structures.md)）持てる。ほかのフィールドを持ってはならない（MUST NOT）。
- `number`は正規の番号の文字列とする。番号を持たないLaw、MainProvision、SupplementaryProvision、AppendedMaterial、EnactStatement、Toc、Preamble、Rowは`null`とする。Cellの`number`は行の中での位置（1始まり）とする。
- `display`は次のとおりとする。Lawは`law.title`、MainProvisionは空文字列、SupplementaryProvisionは附則の見出しの原文のラベル（HEAD-03）、AppendedMaterialは宣言した`title`。Table、Row、Cell、EnactStatement、Toc、Preambleは空文字列。
- `content`は、textのオブジェクトまたは子ノードの、順序のある配列でなければならない（MUST）。textは原文のMarkdownであり、HTMLにした結果ではない。
- 親と子は次のとおりとする。

  | 親 | 許す子・回数 |
  |---|---|
  | Law | EnactStatement 0〜1、Toc 0〜1、Preamble 0〜1、MainProvision ちょうど1、SupplementaryProvision 0以上、AppendedMaterial 0以上。この順 |
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
  | EnactStatement、Toc、Preamble | text ちょうど1 |
  | AppendedMaterial | text 1以上、意味の子なし |

- textを許すのは、Paragraph、Item、Subitem、Cell、EnactStatement、Toc、Preamble、AppendedMaterialだけである。
- 子の前後に本文を許し、出現の順を保持しなければならない（MUST）。連続するtextは結合して一つの要素とし、空文字列のtextを作らない。
- 本文の区間は、開始のマーカーの行末の直後から、次の子の開始行の行頭、または終了のマーカーの行頭までとする。子の終了行の行末の直後から、親の本文を再開する。空行を保持し、本文の末尾の改行も保持する。構造の外の区切りの空行は、意味ASTに含めない。
- ASTのフィールドの順は意味を持たず、配列の順は意味を持つ。

## AST-02 意味の検査

- すべてのノードのIDは一意でなければならない（MUST）。
- 親子の制約、IDの導出、元の入力との本文の一致、`display`と`number`、Manifestのメタデータの一致は、schemaだけでは十分に検査できない。意味の検査を必須とする。
- 入力が不合格の場合、部分的なASTを返してよい（MAY）が、有効な完全なASTとして返してはならない（MUST NOT）。
- 番号・`key`の重複は、そこから導いたノードIDの重複を生む。この現象は、番号・`key`の重複を定める規則（分類・条は[HEAD-02](04-headings.md)、項・号・細分・表は[DIR-02](05-directives.md)、行の`key`は[TABLE-02](09-structures.md)）の違反として報告する。本要件の違反として報告するのは、連鎖する違反（[VAL-07](../common/v1alpha1/03-validation.md)）である。番号・`key`の重複を本要件の違反だけで報告する実装は、それらの規則の違反を報告していない。

## SRC-01 ソースの位置

- 本要件は、意味ASTと診断の位置を定める。位置はASTと診断の一部であり、実装が元の入力を保持するか（[SRC-02](#src-02-元の入力の保持)）によらない。
- 位置は`{path, start, end}`で、元のファイルのUTF-8のバイト数による半開区間[start, end)とする。CRLFは2バイト、日本語は実際のUTF-8の長さとする。境界でUnicodeの文字を分断しない。UTF-16のコード単位の数を位置にしてはならない（MUST NOT）。`path`は、`law.yaml`のあるディレクトリからの相対パスで、`documents[].path`と同じ表記とする（`law.yaml`自身は`law.yaml`）。
- 合成したLaw・MainProvisionは、空の位置の列を持つ。分類・条は見出しの行（行末を含む）を、明示のコンテナ（項・号・細分・表・行・セル・前文・制定文・目次）は開始行の行頭から終了行の行末までを、附則・附属資料のscopeは最初の見出しの行を位置とする。本文の片は、文字列と一致する位置を持ち、複数のファイルにわたる場合は位置の列を持つ。
- 行・列の表示は任意であり、バイト位置の代わりにならない。
- 次の違反の診断の位置は、次のとおりとしなければならない（MUST）。
  - 未閉鎖は、開始のマーカーの行。対応しない終了は、その終了のマーカーの行。属性の不正（属性の重複を含む）は、そのマーカーの行。
  - 重複は、後の定義の行。後か先かは、読み込みの順（[MAN-03](02-manifest.md)の`documents`の順と、ファイルの中の出現の順）で決め、ファイルの名前の順にはよらない。定義の行は、分類・条では見出しの行、明示のコンテナ（行の`key`の重複を含む）では開始のマーカーの行とする。
  - 見出しの違反（[HEAD-01](04-headings.md)・[NUM-01](04-headings.md)の見出しの形、[HEAD-03](04-headings.md)の附則・附属資料の最初の見出し。附属資料の最初の見出しと`appendices[].title`の不一致を含む）は、その見出しとして検査した行。
  - 表示の不一致は、表示番号を検査した本文の行（[DISP-01](05-directives.md)）。
  - 項の見出しの違反（見出しの行の形、その後の本文の行の欠落）は、見出しの行として検査した本文の行（[PARA-02](09-structures.md)）。
  - 附属資料の最後でないファイル（scopeの最初のファイルを含む）のブロックの閉鎖の不足は、そのブロックの開始行。空白だけの後続のファイルは、そのファイルの全体（[MAN-03](02-manifest.md)）。
- Manifestの意味の違反の位置は、定めない。`path: law.yaml`の文書全体を指してよい（MAY）。
  - 定義: Manifestの意味の違反は、Markdownの本文によらず、`law.yaml`と`documents`のファイルで判定するManifestの違反（MAN-01・MAN-02の違反、MAN-03の読み込みの順序の違反）をいう。
  - 含むもの: `law.yaml`の中の重複（YAMLの重複キー、パスの重複、keyの重複）。これらは上の一覧の重複には当たらない。
  - 含まないもの: 附属資料の`title`と最初の見出しの不一致。これは上の一覧の見出しの違反とする。
- 上の一覧にない診断の位置は、定めない（例: 削除条の形の違反、[HEAD-04](04-headings.md)）。
- 一覧の位置は、違反の元になった行で決まり、違反を見つけた時点や回復の方法にはよらない。例えば、未閉鎖は、終了がないと分かるのがEOFや後の見出しであっても、開始のマーカーの行を指す。回復の後に続く診断の位置は、定めない。
- 診断の位置の行は、行頭から行末の改行（LFは1バイト、CRLFは2バイト）までとする。最後の行に改行がなければ、ファイルの末尾までとする。分類・条の見出しの行の位置と同じ数え方である。

## SRC-02 元の入力の保持

- 本要件は、版パッケージのファイルを損失なく書き出す機能を提供する実装に適用する（[要件の適用対象](../README.md#要件の適用対象)）。損失なく書き出すとは、利用者が変えなかった部分を、元の入力のバイト列のまま出力することをいう（例: 元のファイルの復元、編集した結果の保存、意味ASTからのファイルの再出力）。表現を意図して変える出力（整形等）は、変えた部分について本要件の対象ではない。検査の結果（診断、AST、結果の値）だけを返す実装には適用しない。
- その実装は、意味ASTとは別に、元の入力の全バイトとソースの対応を保持しなければならない（MUST）。意味のJSONだけでは、損失のないファイルの保存形式にならない。マーカー、見出し、区切りの空白、最終の改行を含む元の`law.yaml`とMarkdownを、書き出す結果に使えるように保つ。
- 元の入力の再構成に、正規の番号や`display`だけを使ってはならない（MUST NOT）。
- 保持の方法と期間は、書き出す結果に影響しない限り、実装に委ねる。
