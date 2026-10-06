# rpatch

既存のUTF-8ファイルを完全一致でまとめて編集するRust製ツール。複数ファイルの全編集を照合してから書き込み、失敗時には行番号と修正のヒントを返す。CLIとローカルMCPは同じ編集処理を使う。

```sh
cargo build --locked --release
./target/release/rpatch --diff <<'EOF_INPUT'
=== app.js
<<<<<<< OLD
const value = 1;
=======
const value = 2;
>>>>>>> NEW
EOF_INPUT
```

`--dry-run` で確認だけできる。行の一部の置換（`OLD part`）、FROM/TOの範囲置換、BEFORE/AFTERの行挿入、JSONの入力にも対応。入力形式の詳細は `rpatch --help`。

入れ方は `cargo install --locked --path .`。MCPサーバーはCargo feature `mcp`（既定ではオフ）に入っていて、使うときだけ `--features mcp` を付ける。

- [設計](docs/design.md)（[English](docs/design.en.md)）: 処理の流れ、照合の規則、診断、書き込み、終了コード
- [llms.txt](llms.txt): LLM 向けの入口（英語）
- [MCPサーバー](docs/mcp.md): Windows／Macのビルド・接続手順
- [edit-guard](docs/edit-guard.md): rpatch の外で確かめずに書き換えるコマンド（python の置換、`sed -i`、追跡しているファイルへの `cat >` など）を止める Claude Code のフックと、それを入れたプラグイン（`plugin/`）

照合中の失敗は全ファイルを変更せずに終了する。書き込みはファイルごとの一時ファイル＋renameで行い、途中で失敗した場合はそれまでの変更を戻す。ロールバック自体が失敗したときは残ったファイルを報告する。プロセス停止や他プロセスの同時編集に対するトランザクションは提供しない。
