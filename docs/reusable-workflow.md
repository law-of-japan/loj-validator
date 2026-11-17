# 法令リポジトリの共通CI

`validate-law.yml` は次期仕様の法令リポジトリ向けの再利用ワークフローです。
法令側のコードは実行せず、固定した検査器でルートの `history.yaml` を持つ
リポジトリを検査します。v1alpha1・legacy形式は対象外です。

## 呼び出し

法令側の `.github/workflows/validate.yml` に次の設定を置きます。
`VALIDATOR_COMMIT_SHA` の2箇所を、このワークフローとスクリプトを含む
`law-of-japan/loj-validator` の同じ完全コミットSHA（40桁）へ置き換えてください。
`uses` 内は式で共通化できないため、両方を更新します。

```yaml
name: Validate
on:
  pull_request:
  push:
    branches: [main]
  workflow_dispatch:
permissions:
  contents: read
jobs:
  law:
    uses: law-of-japan/loj-validator/.github/workflows/validate-law.yml@VALIDATOR_COMMIT_SHA
    with:
      validator-ref: VALIDATOR_COMMIT_SHA
```

Secretsの引継ぎは不要です。検査器は公開リポジトリから取得し、Rust 1.88.0で
`cargo build --release --locked` を実行します。PRでは合成マージコミットではなく
PRのhead、pushと手動実行ではイベントのSHAを検査します。
`pull_request_target` からは呼び出さないでください。
`GITHUB_TOKEN` の権限は `contents: read` のみです。fork由来のPRも、baseリポジトリから
イベントのhead SHAを取得します。固定したcheckoutは、全履歴の取得でそのSHAを得られない場合に
SHAを指定して追加取得するため、移動し得る `refs/pull/N/head` へは切り替えません。

## 検査と出力

- L0・L1：履歴に列挙された各版のパッケージ。
- L3：履歴構造と根拠。
- rebuild：再構築報告。
- change：`change-base` を指定した場合だけ、baseから検査対象headまでの変更。

各検査を独立に実行し、先行検査が失敗しても後続の結果を保存します。
終了コード1（違反）、2（設定・実行エラー）、3（未完了）はすべてジョブを失敗させ、
元の終了コードは `summary.json` に残します。JSON出力が壊れている場合も失敗します。
空の結果やオブジェクトでないJSONも合格にはしません。検査前の入力設定の誤りは
`configuration`（終了コード2）、Git・ファイル・ツールの実行障害は `incomplete`（3）として
`summary.json` に理由を保存します。出力先自体に書き込めない場合は標準エラーに理由を表示します。
個別の検査器起動が失敗しても、後続の検査を試み、元の未完了状態をsummaryに残します。
`law-validation` artifactには各JSON、標準エラー、実行コマンド、対象commit・
検査器commit・仕様commit・実行ファイルのSHA-256を保存します。
取得やビルド前に失敗した場合、検査レポートは生成されません。
複数回呼ぶ場合は `artifact-name` に異なる値を指定してください。

ディレクトリ検査の `target.commit` は空のままです。付随する対象SHAの記録は
正本commitへの適合認証を意味しません。L2（原典照合）・L4・台帳検査・固定参照検査は
実行しません。L4は台帳と参照先法令を別途固定・取得するジョブで行ってください。

## 変更検査

`change-base` は検査対象headの祖先である完全SHAを指定します（head自身は不可）。
PRでは必要に応じて `${{ github.event.pull_request.base.sha }}`、mainへのpushでは
`${{ github.event.before }}` を渡せます。ただし、PRのbaseが先に進んだ場合は
headへ取り込むか、適切な共通祖先を別途決めてください。新規ブランチのpushに含まれる
ゼロSHAは使用できません。未指定時は変更検査を実行しません。

変更検査は各コミットを検査します。Squash後のコミットメッセージはPR時点では
検査できないため、mainへのpush後にも検査してください。検査用bundleは一時ブランチ参照を
使って作り、artifactには含めません。再構築導入前の履歴を含む範囲を指定すると、
旧形式の入力で不合格・未完了になることがあります。

法令側での導入と必須チェックへの設定は、このワークフローの追加だけでは行われません。
