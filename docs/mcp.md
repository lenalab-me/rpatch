# ローカルMCP

`rpatch mcp [--cwd <directory>]` でstdioのMCPサーバーを起動する。HTTP、認証、常駐サービスの設定は不要。クライアントが起動・終了させる。

MCPはCargo feature `mcp`（既定ではオフ）に入っている。CLIだけ使うなら不要で、rmcpとtokioを含まないぶんバイナリが小さい。featureなしのビルドで `rpatch mcp` を実行すると、その旨のエラーを出して終了コード2で終わる。以下のビルド・インストールのコマンドには `--features mcp` を付けている。

編集ツールは `rpatch` の1つ。既存CLIと同じ入力、照合、書き込み、差分・診断を使う。stdoutはMCP通信専用。起動エラーはstderrに出る。

## ビルド

[Rust](https://www.rust-lang.org/tools/install)のstableツールチェーンを入れる。WindowsはRustのインストーラーが案内するVisual StudioのC++ビルドツール／Windows SDKも必要。

Mac:

```sh
cargo build --locked --release --features mcp
./target/release/rpatch --version
./target/release/rpatch mcp --help
```

Windows（PowerShell）:

```powershell
cargo build --locked --release --features mcp
.\target\release\rpatch.exe --version
.\target\release\rpatch.exe mcp --help
```

ソースからPATHに入れる場合は、どちらも `cargo install --locked --path . --features mcp`。接続設定のcommandには、PATHへの依存を避けるため実行ファイルの絶対パスを使う。

[Build and test](../.github/workflows/build.yml)はApple Silicon、Intel Mac、Windows x64のホストで、featureなし・ありの両方のclippyとテストを実行する。releaseビルドは `--features mcp` 付きで、バイナリをActionsのartifactとして保存する。署名・notarizationやGitHub Releaseへの公開は含まない。

## Codexから接続

```sh
cargo install --locked --path . --features mcp
codex mcp add rpatch -- /Users/you/.cargo/bin/rpatch mcp
codex mcp get rpatch --json
```

Windows（PowerShell）:

```powershell
cargo install --locked --path . --features mcp
codex mcp add rpatch -- 'C:\Users\you\.cargo\bin\rpatch.exe' mcp
codex mcp get rpatch --json
```

MCPの登録はCodex CLI・デスクトップアプリ・IDEで共有される。登録後は新しいセッションで確認する。デスクトップアプリではMCP設定画面のRestartから再読込する。[公式OpenAIドキュメント](https://learn.chatgpt.com/docs/extend/mcp)に登録と再起動の手順がある。

作業ディレクトリを固定しない登録なら、各セッションの作業ディレクトリを基準にする。クライアントで起動位置が異なる場合はCodexの `mcp_servers.rpatch.cwd` を設定するか、rpatchの `--cwd` に絶対パスを渡す。

## Claude Codeから接続

編集するプロジェクト内で登録する。`--scope local` はそのプロジェクト用の設定。

Mac:

```sh
claude mcp add --transport stdio --scope local rpatch -- \
  /Users/you/tools/rpatch mcp --cwd /Users/you/project/example
claude mcp get rpatch
```

Windows（PowerShell）:

```powershell
claude mcp add --transport stdio --scope local rpatch -- `
  'C:\Users\you\tools\rpatch.exe' mcp --cwd 'C:\Users\you\project\example'
claude mcp get rpatch
```

バイナリを直接起動するので `cmd /c` のラッパーは不要。Claude Codeを起動し、`/mcp` で接続状態を確認する。登録方法は[Claude Codeのstdio接続手順](https://code.claude.com/docs/en/mcp#option-3-add-a-local-stdio-server)に従っている。

## JSON設定を使うクライアント

stdioの `mcpServers` を受け付けるクライアントの設定例。クライアントにより設定ファイルの場所は異なる。

Mac:

```json
{
  "mcpServers": {
    "rpatch": {
      "type": "stdio",
      "command": "/Users/you/tools/rpatch",
      "args": ["mcp", "--cwd", "/Users/you/project/example"]
    }
  }
}
```

Windows:

```json
{
  "mcpServers": {
    "rpatch": {
      "type": "stdio",
      "command": "C:\\Users\\you\\tools\\rpatch.exe",
      "args": ["mcp", "--cwd", "C:\\Users\\you\\project\\example"]
    }
  }
}
```

`--cwd` を省略するとサーバーを起動したプロセスの作業ディレクトリを使う。クライアントによってはプロジェクトと異なる場所で起動するので、明示するのが確実。`--cwd` はパスの基準であり、アクセス制限ではない。CLIと同じく絶対パス、`..`、symlinkのリンク先も編集できる。

## ツールの引数と結果

| 引数 | 型 | 既定値 | 意味 |
| --- | --- | --- | --- |
| `input` | string | 必須 | CLIのstdinと同じ区切り形式またはJSON文字列 |
| `diff` | boolean | `false` | unified diffを付ける |
| `dry_run` | boolean | `false` | 照合・差分表示だけで、書き込まない |

CLIと同じく、diffは既定で無効。成功時の結果は、ファイルごとの編集数と変わった行番号（`at_lines=`）だけになる。

```json
{
  "input": "=== app.js\n<<<<<<< OLD\nconst value = 1;\n=======\nconst value = 2;\n>>>>>>> NEW\n",
  "dry_run": true
}
```

シェルの `rpatch <<'EOF'` は付けず、入力本体だけ渡す。複数ファイル・複数箇所は同じ文字列にまとめる。JSON形式を使う場合も `input` は文字列。

成功時の `content[0].text` はCLIのstdoutと同じ変更行表示・差分。失敗時はCLIのstderrと同じ診断で `isError: true` を返す。失敗の種類は照合失敗、ファイルなし、入力不正、I/Oエラーで、本文の診断で区別する。`structuredContent` は返さない。未知のツールや引数の型違いはSDKのプロトコルエラー処理に従う。

1つのサーバー内の編集要求は同時には処理せず、1件ずつ処理する。届いた順に処理されるとは限らない。同じ箇所への編集を同時に送ると、後から送ったほうが先に当たることがある（先に当たらなかったほうは照合に失敗し、ファイルは壊れない）。実行中の編集がキャンセルされても、その編集処理が終わるまで次の編集を待たせる。他のサーバーやCLIからの同時書き込みは調整しない。

実装には[公式Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)のrmcpを使用。通常のinitialize方式に加え、SDKが対応するプロトコルの交渉・通信処理を利用する。
