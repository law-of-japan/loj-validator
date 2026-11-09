# 適合データ契約

## CONF-01

[manifest.json](../../conformance/v1alpha1/manifest.json)を入口とする。各ケースはid、title、requirements、level、normative、input、expected、successを持つ（MUST）。inputはlaw.yamlへのリポジトリ相対パス。正例と負例はvalid/invalidへ分ける。期待値は[期待結果Schema](../../schemas/v1alpha1/expected.schema.json)、索引は[ケースSchema](../../schemas/v1alpha1/conformance.schema.json)に従う。

success=trueでは完全な意味ASTを固定JSONとして保持し、実装の結果をschemaVersion/law/rootへ投影して完全一致で比較する（MUST）。ソース・診断・実装独自キャッシュは比較対象外だが、別のpositions指定があれば数値位置を比較する。textの改行・空白も一致させる。success=falseは期待diagnosticsのruleをすべて含む診断集合を要求する。部分ASTは正解データとして使わない。expectedのpositionsはnodeIdとpath/start/endを持つ選択検査である。

expectedは規範から人が記述した固定データであり、パーサー出力のコピーやsnapshot更新で正解としてはならない（MUST NOT）。共通データの組立ては許すが、その入力は手定義の意味木とし、Markdownパーサーを期待値生成に使用しない。ケース追加時に規範の該当箇所をレビューする。

## CONF-02

各レベルの適合主張は、そのレベルに指定された正式ケースを満たす必要がある（MUST）。L0はL0の正式ケース、L1はL1の正式ケースを対象とし、他レベルの成功を代用してはならない。複数レベルを実行するランナーは結果をレベル別に集計する。数字の大小による適合の包含関係はない。

固定ケース成功は必要条件であり、未知入力を含む全規範への適合証明ではない。各レベルの定義は[VAL-01](09-validation.md)による。L2/L3/L4は本版では将来対応であり、正式合格を宣言できない。informativeケースの成否をL0/L1に混ぜてはならない。入力法令への適合レベル判定と実装全体の適合宣言も区別する。

[実装ガイド](../../docs/implementation-guide.md)に実装手順、[検証記録](../../docs/validation.md)に実際の検査範囲を記載する。
