# AGENTS.md

このリポジトリで作業するコーディングエージェント向けの手引き。
利用者向けの説明は [README.md](README.md)、設計と制限は [docs/](docs/) にある。ここには作業に必要な規則だけを書く。

## 概要

日本の法令Markdown（law-of-japan形式）の検査器。Rustのワークスペースで、crateは2つ。

- `crates/law-semantic` — 検査の本体（ライブラリ）。
- `crates/loj-validator` — CLIと、schematterによる構造検査。`main.rs`は引数解析と出力だけを持つ。

検査の経路は3つあり、互いに独立している。変更するときは、どの経路かを先に決める。

| 経路 | 入力と選び方 | 実装 | 設計文書 |
|---|---|---|---|
| 次期仕様（既定） | ディレクトリ、`law.yaml`、登録台帳、git bundle | `law-semantic/src/{common,v1alpha2,repository,registry,xref,rebuild,change,fixref,git,checks}`、`loj-validator/src/next.rs` | [docs/next-spec-implementation.md](docs/next-spec-implementation.md) |
| v1alpha1 | `--profile v1alpha1` | `law-semantic/src/v1alpha1` | [docs/v1alpha1-implementation.md](docs/v1alpha1-implementation.md) |
| legacy | `.md`と`--structure-schema` | `law-semantic/src/{mapping,number,validation,references}.rs`、`loj-validator/src/lib.rs` | [README.md](README.md)の「使い方」以降 |

適合レベル（L0〜L4）の意味は [docs/conformance-levels.md](docs/conformance-levels.md) にある。レベルは互いに独立で、上のレベルの合格は下のレベルの合格を意味しない。

## コマンド

Rust 1.88以上が必要。変更を終える前に次の3つを実行し、すべて通す（CIと同じ）。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

- 常に`--locked`を付ける。依存を変えたときだけ`Cargo.lock`を更新し、その理由をコミットに書く。
- 適合ケースだけを実行する：
  - `cargo test -p law-semantic --test next_conformance --locked`
  - `cargo test -p law-semantic --test v1alpha1_conformance --locked`
- 変更検査と参照解決のテストは、`PATH`上のgit 2.32以上を使う。
- CIはLinux・Windows・macOSのstableと、LinuxのRust 1.88.0で動く。1.88で使えない言語機能や標準ライブラリAPIを使わない。パスや改行をOSに依存させない。

## 触ってはいけないもの

- `vendor/loj-spec`と`vendor/loj-spec-v1alpha1`は、仕様リポジトリ（loj-spec）の固定コピー。手で編集しない。
  - 期待結果（`conformance/**/expected`）をテストに合わせて書き換えない。合わないときは実装を直す。仕様側の誤りだと考えるなら、変更せずに利用者へ報告する。
  - バイト単位で固定している（`.gitattributes`で`-text`）。LF・CRLF・BOM・単独CRの見本を含むので、改行や空白を整形しない。
  - 仕様を更新するときは、コピー・`COMMIT`・`SHA256SUMS.json`・コード中の`SPEC_COMMIT`（`common/mod.rs`、`v1alpha1/mod.rs`）を同じ変更でそろえる。適合テストがこれらの一致を確かめる。
- 依存の固定（`schematter-lib =0.2.0`、`pulldown-cmark =0.13.4`、`yaml-rust2 =0.10.4`など）は、検査結果の再現性のためにある。頼まれない限り変えない。
- `.github/workflows`のActionはコミットSHAで固定している。タグ指定に戻さない。
- v1alpha1とlegacyの公開API・JSON出力・終了コードは互換性を保つ。次期仕様の作業でこれらを変えない。

## 検査結果の規則

この検査器の出力は「合格」の根拠として使われる。誤った合格と誤った不合格の両方を避ける。

- 判定できないものは「未完了」（`incomplete`、終了コード3）にする。読めない入力、知らない`schemaVersion`、実装上限の超過、gitの失敗を、`pass`にも`fail`にもしない。
- 環境やツールの失敗を、データについての所見として報告しない。
- 終了コードは、0 合格、1 違反、2 引数・設定エラー、3 未完了。`--format json`では、エラーのときも標準出力に解析できるJSONを1つだけ出す。
- 診断の`rule`は仕様の要件ID（`MAN-03`、`CHG-02`など）。新しい検査を加えるときは、`vendor/loj-spec/spec`の要件に対応させる。対応する要件がない検査を、仕様違反として報告しない。
- 位置は元ファイルのUTF-8バイトの半開区間。CRLF・BOM・日本語を含めて数え、整形後の文字列から計算しない。
- 未対応の構文を黙って捨てない。診断として残す。
- 公開APIは、不正な入力でパニックせず`Err`か診断を返す。
- 入力は信頼しない。パス、サイズ（1ファイル16 MiB、合計128 MiB）、git bundleの内容について、既存の上限と拒否の条件を弱めない。`git.rs`のgit呼び出しは、利用者の環境変数・設定・フックから切り離してある。

## テスト

- 振る舞いを変えたら、同じ変更でテストを加える。不具合の修正では、修正前に失敗するテストを加える。
- テストの入力は合成例だけにする。実在の法令のXML・HTML・全文をリポジトリに入れない。
- 一時ディレクトリを使うテストは、テストごとに別の名前にする（並列実行で衝突する）。
- 適合ケースの合格は必要条件であり、十分条件ではない。仕様の規則を実装するときは、固定ケースにない境界もテストする。

## 文書

- CLIの引数・既定値・終了コード・出力の項目を変えたら、[README.md](README.md)と`main.rs`・`next.rs`の使い方の文字列を同じ変更で直す。
- 次期仕様の検査や適合ケースの件数が変わったら、[docs/next-spec-implementation.md](docs/next-spec-implementation.md)の表を直す。
- 作業記録、引継ぎ文書、生成した検証レポートをコミットしない。
- README.mdと`docs/conformance-levels.md`は日本語、実装の設計文書とコード中のコメントは英語で書く。

## コミットとプルリクエスト

- `main`へ直接コミットしない。`feat/…`、`fix/…`、`chore/…`の名前でブランチを作る。
- コミットの件名は英語で、`feat:`、`fix:`、`test:`、`docs:`、`refactor:`、`chore:`のいずれかで始める。本文には、何を変えたかと、その理由を書く。
- プルリクエストの題名は日本語で、変更の内容を1文で書く（squash mergeで`main`のコミット件名になる）。
- crates.ioへは公開しない（全crateが`publish = false`）。公開・配布の処理を加えない。
