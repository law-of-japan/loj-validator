# 論理ID

## ID-01

LawのIDはlaw.id。本則scopeは`{law.id}/main`、附則は`{law.id}/supplement/{key}`、附属資料は`{law.id}/appendix/{key}`。分類は親IDに`/{kind小文字}/{number}`を追加する（Subsectionはsubsection）。条だけは分類を飛ばしscopeに`/article/{number}`を追加する（MUST）。項・号は親IDに`/paragraph/{number}`、`/item/{number}`、細分は`/subitem/{level}/{number}`。枝番号のハイフンは番号の一部である。IDにファイル名、見出しのHレベル、バイト位置、出現順序を含めてはならない（MUST NOT）。

例: `20990101-act-1/main/article/2-3/paragraph/1/item/2/subitem/1/1`。

## ID-02

条の分類間移動、適法なファイル分割変更、本文変更ではIDを保持する（MUST）。番号変更、scope変更、附則key変更はID変更となる。分類ノード自身は親分類の移動でIDが変わる。改正時点や法的同一性をIDだけで表現しない。時点識別には別途Git commit等のスナップショットが必要。削除条も元の条IDを保持する。削除番号の再使用を履歴上の同一実体とみなす保証はない。無番号の条・項、異なる条が同一scopeで同一番号を持つケース、旧新対照表は未対応。重複を自動serialで解消してはならない。
