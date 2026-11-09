# HTMLコメントマーカー

## DIR-01

[EBNF](../../grammar/v1alpha1/directives.ebnf)の開始・終了マーカーを単独行で用いる（MUST）。小文字のparagraph/item/subitemだけを認める。`<!--`直後、`-->`直前はASCII空白1個、属性間は1個以上。タブ、改行、引用符、属性の周囲の余分な空白、末尾空白は禁止。属性順は任意。paragraph/itemにはnumberだけが必須、subitemにはlevelとnumberが必須。未知属性、重複、欠落、未知マーカーは違反。終了マーカーに属性はない。

```markdown
<!-- loj:paragraph number=1 -->
本文
<!-- loj:item number=1 -->
一　号の本文
<!-- loj:subitem level=1 number=1 -->
1　細分の本文
<!-- loj:end-subitem -->
<!-- loj:end-item -->
<!-- loj:end-paragraph -->
```

## DIR-02

すべての開始には同一ファイル内で明示的に対応する終了が必要（MUST）。最内側を同種の終了で閉じる。省略、交差、余分な終了、見出し又はEOFを越える未閉鎖は違反。paragraphの親はArticle又は直接項形式のSupplementaryProvision、itemの親はParagraph、level=1のsubitemの親はItem、level=nの親はlevel=n-1のSubitemでなければならない。levelは1〜10、飛越し不可。兄弟の同種・同levelのnumberは一意。各コンテナには、子を除く自身の本文に空白以外の文字が必要（MUST）。コメントだけでも非空原文となるが法的内容の有無は判定しない。

## DISP-01

表示番号は本文の第1非空行の先頭token（ASCII空白又は全角空白で終端）で検査する。paragraphは十進又は全角十進、itemはNUM-01の漢数字又は十進、subitemはASCII十進を使う。枝番号は本文では`の`区切り（成分の字種は各種の許容範囲）。正規化値がnumberと等しくなければならない（MUST）。paragraph number=1だけは番号省略可。それ以外には表示番号が必要。番号に続く同じ行に非空本文を置く。番号を含む全文をtextに残し、検出したtokenをdisplayへ保存する。無表示の第1項のdisplayは空文字列。

第1項でも認識可能な数字tokenで始まれば検査する。番号を意図しない数字が紛らわしい場合は原典確認を要し、自動修正しない。いろは・括弧・ローマ数字等の細分表示は未対応であり、勝手に十進へ転記せずunsupportedFeaturesに申告する。本版の明示的制限とする。
