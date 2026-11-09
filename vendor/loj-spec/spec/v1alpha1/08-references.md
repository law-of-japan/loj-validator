# 論理参照（将来L2の設計）

## REF-01

L0/L1では通常Markdownリンクをそのまま本文として保持し、解決成功を合格条件にしてはならない（MUST NOT）。`[第二条](loj:main/article/2)`も不正な`loj:`リンクもL1では本文である。リンクを捨てたり自動修正してはならない。GitHubの通常表示では独自schemeへのクリック可能性を保証しない。

以下はL2の予約設計であり、正式な適合要件ではない。リンク先をCommonMarkで復号した後、`loj:`以降を同法令のscope相対IDとし、law.idと`/`を前置して検索する。main、supplement/key、appendix/key及びその子が対象。percent encoding、query、fragment、相対`..`、他法令IDを許さない案とする。構文不正・未解決・曖昧参照は将来L2で不合格。自動リンクと参照形式リンクの扱いも次版で確定する。

conformanceのinformative/L2ケースはこの案の機械可読な設計例にすぎず、L0/L1の合格数には入れない。外部法令、施行時点、前条・同項等の自然言語参照、改正履歴は未対応。
