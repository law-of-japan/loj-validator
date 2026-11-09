# 意味ASTとソース

## AST-01

規範JSON表現は[AST Schema](../../schemas/v1alpha1/ast.schema.json)による。schemaVersion、Manifestと同じlaw、rootを持つ。ノードはkind/id/number/display/contentを持ち、Articleにはstatus、Subitemにはlevelを追加する。他のフィールドは不可。numberは正規番号文字列、番号を持たないLaw/MainProvision/SupplementaryProvision/AppendedMaterialはnull。Lawのdisplayはlaw.title、MainProvisionは空文字列、附則は`附則`、附属資料は宣言title。contentはtextオブジェクト又は子ノードの順序付き配列（MUST）。textは原文MarkdownでありHTMLレンダリング結果ではない。

| 親 | 許可する子・回数 |
|---|---|
| Law | MainProvisionちょうど1、SupplementaryProvision 0以上、AppendedMaterial 0以上。この順序 |
| MainProvision | rank 1〜5の分類又はArticle、計1以上 |
| Part/Chapter/Section/Subsection/Division | 自身より大きいrankの分類又はArticle、計1以上 |
| Article(active) | Paragraph 1以上 |
| Article(deleted) | 子なし、本文なし |
| SupplementaryProvision | 分類/Article群又はParagraph群のどちらか、計1以上 |
| Paragraph | Item 0以上、text 1以上 |
| Item | level=1のSubitem 0以上、text 1以上 |
| Subitem | 次levelのSubitem 0以上、text 1以上（level10には子なし） |
| AppendedMaterial | text 1以上、意味子なし |

textを許すのはParagraph/Item/Subitem/AppendedMaterialだけ。子の前後に本文を許し出現順を保持する（MUST）。連続textは結合して1要素とし、空文字列のtextを作らない。本文区間は開始マーカー行末の直後から次の子の開始行頭又は終了マーカー行頭まで。子終了行の行末の直後から親本文を再開する。空行も保持し、本文末の改行も保持する。構造外の区切り空行は意味ASTには含めない。ASTのフィールド順は意味を持たず、配列順は意味を持つ。

## AST-02

全ノードのIDは一意でなければならない（MUST）。親子制約、ID導出、元入力との本文一致、displayとnumber、Manifestメタデータ一致はSchemaだけで十分に検証できない。意味検査を必須とする。入力不合格の場合、部分ASTを返すことはMAYだが、有効な完全ASTとして返してはならない。

## SRC-01

実装は意味ASTと別に元入力全バイト及びソース対応を保持する（MUST）。意味JSONだけはlosslessなファイル保存形式ではない。完全復元のため、マーカー、見出し、区切り空白、最終改行を含む元law.yaml/Markdownを保管する。元入力の再構成に正規番号やdisplayだけを使用してはならない（MUST NOT）。

位置は`{path,start,end}`、元ファイルのUTF-8バイト数による半開区間[start,end)。CRLFは2バイト、日本語は実際のUTF-8長。境界はUnicode文字を分断しない。合成Law/MainProvisionには空の位置列。分類・条は見出し行（行末込み）、明示コンテナは開始行頭から終了行末込み、附則・附属資料scopeは最初の見出し行を位置とする。本文片は文字列と一致する位置を持ち、複数ファイルでは位置列を持つ。行/列表示は任意でありバイト位置に代わらない。

未閉鎖診断は開始マーカー行、不一致終了は当該終了行、属性不正は当該行、重複は後の定義行、見出し違反は当該行、表示不一致は本文の当該行を指す（MUST）。Manifest意味違反はpath=`law.yaml`の文書全体を指してよい。期待位置ファイルはこの契約の選択したノード位置を固定している。
