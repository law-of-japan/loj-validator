# LoJ規範仕様

LoJ（Law of Japan）のデータの規範仕様の入口である。本書は、規範とする文書の範囲、要求レベル、要件IDの付け方、組み合わせて使える版を定める。

## 規範とするもの

次を規範とする。

| ディレクトリ | 対象 | schemaVersion |
|---|---|---|
| [common/v1alpha1](common/v1alpha1/01-overview.md) | 種類にまたがる要件（構成、文字、適合レベル、共通の型、時点値、変更の種類、固定参照、派生ビュー、Record間の検査） | なし（共通の型の版） |
| [v1alpha2](v1alpha2/01-overview.md) | 版パッケージ（`law.yaml`と本文のMarkdown） | `loj/v1alpha2` |
| [loj-history/v1alpha1](loj-history/v1alpha1/01-history.md) | 履歴マニフェスト（`history.yaml`） | `loj-history/v1alpha1` |
| [loj-glyphs/v1alpha1](loj-glyphs/v1alpha1/01-glyphs.md) | 字体注記（`glyphs.yaml`） | `loj-glyphs/v1alpha1` |
| [loj-registry/v1alpha1](loj-registry/v1alpha1/01-registry.md) | 登録台帳のRecord（`records/<Record ID>.yaml`） | `loj-registry/v1alpha1` |
| [loj-rebuild/v1alpha1](loj-rebuild/v1alpha1/01-rebuild.md) | 原典からの再構築と再構築報告（`rebuild.yaml`） | `loj-rebuild/v1alpha1` |

- 各ディレクトリに対応する`schemas/`のJSON Schemaと、適合データ（`conformance/`）も規範とする。
- `docs/`の文書、`examples/`の例、各ディレクトリの付録のうち「参考」と示したものは、参考情報である。
- 規範の間に矛盾があれば、仕様の欠陥として報告する。実装の挙動で規範を上書きしない。

## 要求レベル

- MUST（必須）、MUST NOT（禁止）、SHOULD（理由のある例外を許す推奨）、MAY（任意）で要求レベルを示す。
- 要求レベルを示さない文は、定義または説明である。

## 要件ID

- `## <ID> <名前>`の見出しで、一つの要件に一つのIDを付ける。要件IDは、検査の診断で報告する規則（rule）でもある（[VAL-07](common/v1alpha1/03-validation.md)）。
- 一つの系列（IDの接頭辞）は、一つのディレクトリに属する。系列の名前は、すべてのディレクトリを通じて一意である。
- 番号は、系列の中で読む順に01から振る。一度公開した版で使った番号を、後の版で別の意味に使ってはならない（MUST NOT）。廃止した番号は欠番として残す。
- 要件IDは版ごとに意味を持つ。同じIDでも、版が違えば別の規則である。

| ディレクトリ | 系列 |
|---|---|
| common/v1alpha1 | CORE、CHAR、VAL、SRCM、CONF、TYPE、TIME、CHG、FIXREF、VIEW、XREF |
| v1alpha2 | BASIC、MAN、MD、HEAD、NUM、DIR、DISP、AST、SRC、ID、REF、PARA、TABLE、PRE |
| loj-history/v1alpha1 | HIST、REV |
| loj-glyphs/v1alpha1 | GLYPH |
| loj-registry/v1alpha1 | REG、RID |
| loj-rebuild/v1alpha1 | REB、REBR |

## 組み合わせて使える版

[CORE-05](common/v1alpha1/01-overview.md)の対応表である。検査器は、この表の組だけを処理すればよい。

| 組 | `loj` | `loj-history` | `loj-glyphs` | `loj-rebuild` | `loj-registry` | 共通の型 |
|---|---|---|---|---|---|---|
| 1 | v1alpha2 | v1alpha1 | v1alpha1 | v1alpha1 | v1alpha1 | v1alpha1 |

- 版を上げるときは、行を加える。行を加えることは、既にある版の意味を変えることではない。
- `loj/v1alpha1`の版パッケージは、どの組にも含まない。

## 凍結した版

| 版 | 状態 | 後継 |
|---|---|---|
| [v1alpha1](v1alpha1/01-overview.md)（`loj/v1alpha1`） | 凍結。内容を変えない。検査器と適合データが対応し続けることを求めない | `loj/v1alpha2` |
