# CLI／MCPの自発的な使用を比べる

作業依頼には編集ツール名を出さず、エージェントが自分で何を選ぶかを見る。Codexは `bench/compare-codex.sh`、Claude Codeは `bench/compare.sh` で実行する。同じ初期ファイルを2つにコピーし、同じモデル・課題でCLI条件とMCP条件を比べる。

## Codexで実行

```sh
cargo build --locked --release
LIMIT=1 TIMEOUT_SECONDS=180 bench/compare-codex.sh
```

既定モデルはgpt-6.1-sol。`MODEL` で指定を変えられる。Codexへのログインが必要で、CLIは `--ignore-user-config`、`--ignore-rules`、`--ephemeral`、`--json` をサポートする版を使う。

AGENTS.md、memory、ユーザーの追加指示、他のMCPサーバー、プラグイン、スキルを実験の子セッションだけで無効にする。普段の設定・認証ファイルは変更しない。既存rpatchのあるPATH要素を外し、シェルのsnapshotとlogin shellも無効にして条件を固定する。MCP条件は `required=true` で起動成功を必須にする。

ログはCodexのJSONLを使い、MCP呼び出し、組み込みのapply_patch、シェルでのCLI使用を別々に数える。Codexで得られない費用やturn数はnullとし、トークン使用量を保存する。`MAX_BUDGET_USD` はClaude Code条件だけに適用され、Codexでは時間制限を使う。

## Claude Codeで実行

macOS／LinuxのBash用。Claude Codeへのログイン、Node.js、Python 3、jq、tar、GNU timeoutが必要。WindowsはGit Bash／WSLで実験環境を用意する。ネイティブWindowsのMCP接続自体はPowerShellでよい。

```sh
cargo build --locked --release
LIMIT=1 MAX_BUDGET_USD=0.50 bench/compare.sh
```

最初の課題は、設定名を4ファイルにまたがって変更し、旧形式の互換性を残す作業。2つ目は、長い段落への追記とFAQの挿入。入力の初期状態は [editor-sample](../bench/fixtures/editor-sample)、課題は [compare-prompts](../bench/compare-prompts) に固定してある。

既定は各条件1試行、2課題、1課題あたり300秒・API予算1ドル。`LIMIT=1` は1つ目だけ、`TRIALS=3` は各条件を3回実行する。出力先は `bench/out/compare-<日時>`。既存の出力先を上書きしない。

```sh
TRIALS=3 MAX_BUDGET_USD=1 TIMEOUT_SECONDS=300 \
  bench/compare.sh /tmp/rpatch-comparison
```

## 条件

| 条件 | 追加の編集手段の提示 |
| --- | --- |
| CLI | PATH上のrpatchと、「利用可能なCLI、形式はrpatch --helpで確認」という短い案内 |
| MCP | stdioサーバーと、ツール一覧に含まれるrpatchの説明・引数スキーマ |

Edit／Write／Bashなどの標準ツールは両条件で残す。CLIを使え、MCPを使え、Pythonを使うな、という指示は入れない。

既存rpatchのあるディレクトリは子プロセスのPATHから外し、CLI条件だけ実験用のrpatchを追加する。MCPサーバーは絶対パスで起動する。シェルの初期化ファイルなどがPATHを再設定するとMCP条件でもCLIを見つける可能性があるため、集計では `rpatch_cli` と `rpatch_mcp` を別々に残す。

[CLAUDE_CODE_DISABLE_CLAUDE_MDS](https://code.claude.com/docs/en/env-vars)を使ってユーザー／プロジェクトのCLAUDE.mdとmemoryを読み込ませず、settingsの自動読込・スキル・他MCPサーバーも無効にする。rpatchを推奨する普段の指示による偏りを避けるための設定。管理者のポリシーやモデルの組み込み指示まで同一化するものではない。

各課題は新しいセッション。ファイルは同じ条件内で次の課題に引き継ぐ。試行の奇数回はCLI→MCP、偶数回はMCP→CLIの順にする。各条件の開始ファイル、課題、モデル、ツールのバージョンは保存する。

この比較は、CLIのヘルプ発見とMCPのツール説明という提示方法の差も含む。「同じ情報量を提示した場合のプロトコルだけの効果」ではない。厳密に比べたい場合は、CLI側にもMCPの説明と同じ内容を渡す条件を別途加える。

## 検証・集計

ログはCodexのJSONLまたはClaude Codeのstream-jsonで保存する。終了コード、実行時間、ツール呼び出し、モデルが返す使用量を残す。モデルが編集できるテストとは別に、[verify-sample.mjs](../bench/verify-sample.mjs)で設定の変更・旧形式の互換性・新形式の優先順位を確認する。

```sh
python3 bench/compare_metrics.py bench/out/compare-YYYYMMDD-HHMMSS
```

`summary.json` は課題ごとの結果と、CLI／MCPの両方が正常終了・検証成功した課題の比較を含む。MCP条件は初期化ログの接続成功も必須。失敗した実行を「ツールを使わなかった」として混ぜない。

主に見る値:

- `runs_with_rpatch_use`: 自発的にrpatchを使った実行数。
- `rpatch_share`: 編集呼び出し全体に占めるrpatchの割合。help/versionの確認とdry-runは編集数から外す。
- `counts`: CLI／MCP、標準編集ツール、Python、sdなどの呼び出し数と失敗数。
- `elapsed_seconds`、`cost_usd`、`num_turns`: 全体の所要時間と負担。

シェルでの編集検出はヒューリスティックで、独自スクリプトや新規ファイル作成を完全には分類できない。数値を読むときは生の `.jsonl` と照合する。少数試行から優劣を結論づけず、まず接続・実行・集計が成立するかを見る。

## Codexの試行（2026-10-01）

Codex CLI 0.159.2／gpt-6.1-solで、ツールを指定した接続テストは成功した。dry-run、本適用、不一致診断の3回とも `mcp__rpatch__rpatch` を呼び出した。

自発的な使用を見る短い試行では、CLI条件はCLIを1回使い、MCP条件もCLIを1回使った。両方とも実験側の検証に通ったが、MCP条件ではシェル設定がPATHを戻しており、CLIが見えていた。MCPだけを提示する条件にはなっていないため、排他的な比較からは除外する。この試行からMCPの使用傾向や優劣は判断しない。

生ログは `bench/out/codex-compare-20261001-fixed/`。集計に `cli_contamination` を追加し、MCP条件でCLI編集が起きた試行は有効ペアに含めないようにした。次回用に空のZDOTDIRも指定し、子シェルでユーザーのzsh設定によるPATH変更を防ぐ。

## ZDOTDIR固定後の再試行（2026-10-01）

Codex CLI 0.159.2／gpt-6.1-sol／rpatch 0.7.0で、ZDOTDIR固定後に2課題を各条件3回実行した。実行順はCLI→MCP、MCP→CLI、CLI→MCP。課題はツールを指定していない。

```sh
TRIALS=3 TIMEOUT_SECONDS=180 bench/compare-codex.sh \
  bench/out/codex-compare-20261001-zdotdir
```

12実行すべてが正常終了し、独立検証に成功した。有効ペアは6件、MCP条件へのCLI編集の混入は0件。

| 条件 | rpatchを使った実行 | rpatch編集呼び出し | 組み込み編集呼び出し | 編集呼び出しに占めるrpatch |
| --- | --- | --- | --- | --- |
| CLI | 6 / 6 | 7 | 0 | 100% |
| MCP | 2 / 6 | 2 | 5 | 28.57% |

| 課題 | CLI条件でrpatchを使った実行 | MCP条件でrpatchを使った実行 |
| --- | --- | --- |
| 設定名の変更・互換性 | 3 / 3 | 0 / 3 |
| 長い段落・FAQの編集 | 3 / 3 | 2 / 3 |

MCP条件では、設定変更は3回とも組み込みのapply_patchを選んだ。文章編集は1回目がapply_patch、2・3回目がrpatch MCP。MCPを使った2回とも、OLDとBEFOREを組み合わせてREADMEの3箇所を1呼び出しで編集した。編集の失敗は0件。MCP条件の4実行ではCLIの `rpatch --help` も探索したが、いずれも見つからず終了コード127だった。

同じモデル・隔離設定で別途接続確認を行い、`command -v rpatch` が終了コード1、明示指定したMCPのdry-runが終了コード0となることを確認した。sample.txtは変更されなかった。この接続確認は自発的な使用の集計に含めない。

独立検証に加え、全6作業コピーのFAQ3問・概要へのローカルJSON保存の追記・設計メモの優先順位を確認した。ログには12課題すべてのテスト成功があり、各コピーに課題ごとの2コミットがある。最終作業ツリーはすべてclean。

この条件ではCLIの案内からrpatchが毎回選ばれ、MCPは文章編集で選ばれた。ただし1モデル・2課題・3試行で、同じ試行内の課題も独立ではない。案内の情報量や提示位置も異なるため、プロトコルの優劣には一般化しない。Codex同梱のrgはこの環境で `--version` でもタイムアウトし、検索が停止した実行がある。所要時間は保存したが、速度の優劣の根拠には使わない。

集計・接続確認・要件確認は、保存した結果JSON（`~/project/prod/rpatch-notes/bench-results/codex-20261001-zdotdir.json`、リポジトリの外）。生ログ、開始ファイル、各課題のコミット、接続確認の `mcp-control/trace.jsonl`、確認用の `audit.mjs`／`audit.json` は `bench/out/codex-compare-20261001-zdotdir/` に保存した。スクリプト・課題・バイナリのSHA-256も記録している。

## 初回の接続試行（2026-10-01）

v0.7.0、Claude Code 2.1.286、claude-sonnet-5-5で、1課題・各条件1回を実行した。MCP条件の初期化ログには `rpatch: connected` と `mcp__rpatch__rpatch` が出た。

両条件とも作業開始前にClaude Codeのセッション上限（HTTP 429）で終了した。ツール呼び出し0回、費用0ドル、有効な比較ペア0件。自発的にどちらを使うかはまだ測れていない。上限解除後に上のコマンドで新しい試行を作る。

ローカルの生ログは `bench/out/compare-20261001-171214/`。このディレクトリはgitignore対象であり、配布物には含めない。

## 接続確認（2026-10-01）

自発的な使用の比較とは別に、ツールを指定して接続を確かめた記録。

- Codex CLI 0.159.2／gpt-6.1-solで新しいセッションを起動し、`mcp__rpatch__rpatch` のdry-run、本適用、不一致診断の3呼び出しを確認した。作業ディレクトリの `sample.txt` は `alpha` → `ALPHA` に変わり、不一致では追加の書き込みはなかった。
- 再開したデスクトップアプリの開発セッションでも `mcp__rpatch__rpatch` が実際のツール一覧に現れた。一時ファイルへのdry-run、本適用、不一致診断が成功し、dry-runと不一致では内容が変わらないことを確認した。MCP設定画面の表示自体はこの確認の対象にしていない。
- Claude Code（CLI、Opus）の新しいセッションで、user scopeに登録したサーバーからdry-run、本適用、不一致診断を確認した。相対パスはプロジェクトのディレクトリが基準になった。
- `structuredContent` を外したあとのバイナリで、Codexに照合の失敗を1回起こした。ログは `status: "failed"`、`structured_content: null` で、本文はCLIのstderrと同じ診断だった。1回目の試行では、Codexが引数名を `input` ではなく `patch` にして呼び、引数エラーになった。ログは `bench/out/codex-mcp-fail-20261001/`。

## 既存の課題を使う

`bench/prompts/00-build.md` で作ったtasklogなど、任意の初期プロジェクトも渡せる。課題ディレクトリには同じ開始状態から順に成立する課題だけ置く。既存の `00-build.md` をそのまま編集課題に混ぜない。

```sh
VERIFY_COMMAND='node test/run.mjs' TRIALS=3 MAX_BUDGET_USD=3 \
  bench/compare.sh /tmp/tasklog-comparison /path/to/tasklog-seed /path/to/edit-prompts
```

検証コマンドは課題終了後の作業ディレクトリで動く。任意のseedを使うときは `VERIFY_COMMAND` を指定する。初期ファイルは `.git` を除いて複製し、実験用リポジトリにする。

生ログと生成物はgitignoreされている `bench/out/` に保存する。モデルが正常終了できなかったときや検証に失敗したときは、その条件の後続課題を止める。
有効な比較ペアが1件も得られなければ、ログと集計を保存したうえでスクリプトは終了コード1を返す。
