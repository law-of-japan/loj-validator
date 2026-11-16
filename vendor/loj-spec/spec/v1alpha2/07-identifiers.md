# 論理ID

## ID-01 IDの導出

- LawのIDは`law.id`とする。本則のscopeは`{law.id}/main`、附則は`{law.id}/supplement/{key}`、附属資料は`{law.id}/appendix/{key}`とする。
- 制定文は`{law.id}/enact-statement`、目次は`{law.id}/toc`、前文は`{law.id}/preamble`とする。
- 分類は、親のIDに`/{kindの小文字}/{number}`を加える（Subsectionは`subsection`）。条だけは分類を飛ばし、scopeのIDに`/article/{number}`を加えなければならない（MUST）。
- 項・号は親のIDに`/paragraph/{number}`、`/item/{number}`を、細分は`/subitem/{level}/{number}`を加える。枝番号のハイフンは番号の一部である。無番号の項も、属性の`number`から同じ形で導く。
- 表は親のIDに`/table/{number}`を、行は表のIDに`/row/{key}`を、セルは行のIDに`/cell/{c}`（`c`は行の中での位置、1始まり）を加える。
- IDに、ファイル名、見出しのHレベル、バイト位置、出現の順序を含めてはならない（MUST NOT）。例外はセルの行の中での位置だけである。

例: `20990101-act-1/main/article/2-3/paragraph/1/item/2/subitem/1/1`、`19480720-law-178/main/article/2/paragraph/1/table/1/row/b6gr/cell/3`。

## ID-02 IDの安定性

- 条の分類の間の移動、適法なファイルの分割の変更、本文の変更では、IDを保持しなければならない（MUST）。IDの導出（ID-01）はファイル名・見出しのレベル・出現の順序を使わないので、一つの版でID-01に従っていれば満たされる。本項に、ID-01のほかに検査する内容はない。
- 番号の変更、scopeの変更、附則のkeyの変更は、IDの変更になる。分類のノード自身は、親の分類の移動でIDが変わる。
- 表の行の`key`の安定性は、[TABLE-02](09-structures.md)による。行を加えても、ほかの行のIDは変わらない。ただし、行のIDは表のIDを含むので、`key`を保っても、表の番号の変更は行のIDの変更になる。
- ノードIDは版の中の位置を示し、条の歴史的同一性を主張しない。番号の訂正や繰上げで、同じIDが別の規定を指すことがある。改正の時点や法的な同一性を、IDだけで表さない。版の識別は、履歴マニフェストのrevision_idと、記録版の識別は正本のcommitによる。
- 削除条も元の条のIDを保持する。削除した番号の再使用を、履歴上の同じ実体とみなす保証はない。
- 異なる条が同じscopeで同じ番号を持つ場合、旧新の対照表は未対応とする。重複を自動の連番で解消してはならない（MUST NOT）。
