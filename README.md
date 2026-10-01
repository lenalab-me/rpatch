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

`--dry-run` で確認だけできる。JSONの部分文字列置換、FROM/TOの範囲置換、BEFORE/AFTERの行挿入にも対応。入力形式の詳細は `rpatch --help`。

```sh
rpatch mcp --cwd /absolute/path/to/project
```

- [MCPツール、Windows／Macのビルド・接続手順](docs/mcp.md)
- [CLI／MCPの自発的な使用を比較する実験](docs/mcp-experiment.md)

照合中の失敗は全ファイルを変更せずに終了する。書き込みはファイルごとの一時ファイル＋renameで行い、途中で失敗した場合はそれまでの変更を戻す。ロールバック自体が失敗したときは残ったファイルを報告する。プロセス停止や他プロセスの同時編集に対するトランザクションは提供しない。
