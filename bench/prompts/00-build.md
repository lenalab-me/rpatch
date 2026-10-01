Node.js（ESM、外部依存なし）で、小さなタスク管理ツール「tasklog」を作ってください。

- `tasklog.mjs`: CLI。`add <title>`、`list`、`done <id>`、`stats` のサブコマンド。データは `data/tasks.json` に保存する。
- `lib/store.mjs`（読み書き）、`lib/format.mjs`（一覧の表示）、`lib/stats.mjs`（集計: 総数、完了数、完了率）に分ける。
- `web/index.html`: `data/tasks.json` を fetch して、一覧と集計を表示するダッシュボード。JS は `<script type="module">` で HTML の中に書く。スタイルは `web/style.css`。
- `docs/README.md`: 概要、インストール、使い方（コマンドの表）、データ形式、設計メモの節。各段落はある程度長めに書く。
- `config.json`: 表示の設定（一覧の最大件数など）。`lib/format.mjs` から読む。
- `test/run.mjs`: node:assert で lib の関数を確かめるテスト。`node test/run.mjs` で走る。
- `package.json` に version 0.1.0 と test スクリプト。

できたらテストを流し、git init してコミットしてください。
