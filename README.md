# rpatch

既存のUTF-8ファイルを完全一致でまとめて編集するRust製ツール。複数ファイルの全編集を照合してから書き込み、失敗時には行番号と修正のヒントを返す。CLIとローカルMCPは同じ編集処理を使う。

コーディングエージェント（Claude Code・Codex）が、`sed -i` や python の置換の代わりに使うために、個人で作って毎日使っている。macOS・Linux・Windows でビルドできる。ほかの自作の道具（[devsrv](https://github.com/lenalab-me/devsrv)・[agentlog](https://github.com/lenalab-me/agentlog)）やスキルとまとめて入れる手順は、[harness-engineering の SETUP.md](https://github.com/lenalab-me/harness-engineering/blob/main/SETUP.md) にある。

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

`--dry-run` で確認だけできる。行の一部の置換（`OLD part`）、FROM/TOの範囲置換、BEFORE/AFTERの行挿入、JSONの入力にも対応。入力形式は `rpatch --help`（40行以内の要約）、詳細は `rpatch --help blocks|json|rules|output`、全文は `rpatch --help all`。

入れ方は `cargo install --locked --path .`。MCPサーバーはCargo feature `mcp`（既定ではオフ）に入っていて、使うときだけ `--features mcp` を付ける。

- [設計](docs/design.md)（[English](docs/design.en.md)）: 処理の流れ、照合の規則、診断、書き込み、終了コード
- [llms.txt](llms.txt): LLM 向けの入口（英語）
- [MCPサーバー](docs/mcp.md): Windows／Macのビルド・接続手順
- [edit-guard](docs/edit-guard.md): rpatch の外で確かめずに書き換えるコマンド（python の置換、`sed -i`、追跡しているファイルへの `cat >` など）を止める Claude Code のフックと、それを入れたプラグイン（`plugin/`）

照合中の失敗は全ファイルを変更せずに終了する。書き込みはファイルごとの一時ファイル＋renameで行い、途中で失敗した場合はそれまでの変更を戻す。ロールバック自体が失敗したときは残ったファイルを報告する。プロセス停止や他プロセスの同時編集に対するトランザクションは提供しない。
