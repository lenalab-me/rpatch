# 引き継ぎ: rpatch（エージェント向けの一括置換 CLI）

2026-10-01、`~/project/lab/zatsudan` のセッションから引き継いで始めた。いまは v0.7.0 で、`main` を GitHub に push している（「リポジトリと要望の受け方」を参照）。普段使いは CLI。MCP は Cargo feature `mcp` を付けたビルドだけで使える。以下の「何を作るか」〜「範囲外にしたもの」は始めたときの検討で、その後の変更は「進み具合」に書いている。

## 何を作るか

Claude Code が Bash で毎回書いている「python のヒアドキュメントで置換するスクリプト」の代わりになる、Rust 製の CLI。

```sh
python3 - <<'EOF'
def rep(p, pairs):
    s = open(p).read()
    for old, new in pairs:
        assert old in s, (p, old[:60]); s = s.replace(old, new, 1)
    open(p, 'w').write(s)
rep('a.mjs', [(..., ...), (..., ...)])
EOF
```

これを次のように書けるようにする。

```sh
rpatch <<'EOF'
{"edits":[{"file":"a.mjs","old":"...","new":"..."},{"file":"b.html","old":"...","new":"...","count":2}]}
EOF
```

## なぜ作るか（セッションログを分析した結果）

`~/.claude/projects/**/*.jsonl` を集計した。9 月以降のログが主な対象で、期間は 9/24〜10/01、ツール呼び出しは 3,423 件。7 割は beat-jam のもの。5 月から 9 月前半は Claude Code を使っておらず、その間にハーネスも変わっているので、それより前のログは参考程度。

| 観測 | 回数 |
|---|---|
| python ヒアドキュメントでの置換 | 345（Edit ツールは 100） |
| そのうち `def rep/patch/edit` のヘルパーを毎回書き直したもの | 46 |
| assert を付けておらず、マッチしなくても気づけない置換 | 86 |
| 1 スクリプトで複数ファイルを編集 | 98 |
| Edit ツールで同じファイルを連続編集（まとめられたはずの分） | 65 |
| 編集後の `node --check` | 98 |
| 編集後に HTML の `<script>` を `new Function` で確かめる自作ワンライナー | 67 |

背景:
- python での置換はすべて bypassPermissions モードで起きている。このモードではシステムプロンプトに「Read/Edit の代わりに Bash（ヒアドキュメントや短いスクリプト）を使ってよい」という指示が入る。Edit ツールは 1 回の呼び出しで 1 箇所しか直せないので、たくさん直すときはヒアドキュメントでまとめて書くほうが楽。
- 9/30 22:09 に `~/.claude/CLAUDE.md` の CLI ツールの節を拡充した。そのあと grep はほぼ rg に置き換わり（10/01 は rg 77 回、grep 3 回）、1 行のそのままの置換は sd に移った。**複数行・複数箇所の置換は python のまま。ここが patch で埋めるところ。**
- sd は、置換前の文字列が `-` で始まると（`--function-name …` など）オプションと解釈されて 2 回失敗した。そのため、入力は引数ではなく stdin から受け取る。

## 先行例と、足りない点

- Claude Code の MultiEdit: 2.0.8 で廃止された。編集を順番に当てると位置がずれてファイルが壊れる、という報告もあった。
- Codex の `apply_patch`（Rust）: `/opt/homebrew/bin/codex --codex-run-as-apply-patch '<patch>'` で単体で動く（codex-cli 0.159.2）。試したところ、次の 3 つの問題があった。**この 3 つがそのまま、自作版で必ず通すテストになる。**
  1. 2 ファイルを編集して 2 つ目で失敗すると、**1 つ目は書き換わったまま**になる（アトミックではない）
  2. 同じ行が 2 箇所にあると、**何も言わずに 1 つ目だけ置換**する
  3. インデントが違うと、**あいまいに一致させて置換し、インデントが崩れる**（表示は `Success`）
- FileInteractionMCP や coding-tools-mcp など、MCP サーバーの小規模な実装もある。中身は確認していない。

## 仕様案（v0）

入力は stdin から受け取る JSON。

```json
{"edits": [{"file": "path", "old": "完全一致する文字列", "new": "置換後", "count": 1}]}
```
- `count` を省略したら 1。マッチ数が `count` とちょうど同じでなければ失敗にする。
- 同じファイルへの編集は、配列の順番どおりにメモリ上で適用する（python の `s.replace` を続けて呼ぶのと同じ動き）。あとの編集は、前の編集を適用したあとの内容に対して照合する。
- 全ファイル・全編集の照合が通るまで、ファイルには書き込まない。書き込みは一時ファイルを作ってから rename する。途中で失敗したら、それまでに書いたファイルを元に戻す。
- 一致は完全一致のみ。空白の違いを許すような、あいまいな一致は**しない**。

失敗したときの出力（ここが一番の売りなので、丁寧に作る）:
- 0 件のとき: 空白を正規化すれば一致する箇所があれば、「42 行目がインデント違いで一致します」と示す。なければ、一番似ている行の行番号と差分を出す。
- 想定より多くマッチしたとき: マッチしたすべての行番号を出す。
- ファイルがないとき: パスと cwd を出す。
- どの場合も、何も書き込まずに終了コード 1 で終わる。

成功したときの出力（トークンを節約するため短く）:
- `M path  edits=3  lines=10,55,80` のように、1 ファイル 1 行にする。
- `--diff` を付けたら unified diff も出す。`--dry-run` を付けたら書き込まない。

終了コードの案: 0 は成功、1 は照合の失敗、2 は入力の不正、3 は I/O エラー。

## v1: 構文チェック

- 触ったファイルを、書き込む前に tree-sitter でパースする。言語は拡張子で判断する（js/mjs/ts、html とその中の script、json、rs、py、css など）。
- **編集前にはなかった ERROR ノードが増えたら、全部取り消す。** もともと壊れていたファイルは止めずに、警告だけ出す。
- 根拠は、編集後の `node --check` が 98 回、`new Function` が 67 回あったこと。

## 決めたこと

- バイナリ名は **`rpatch`**。`patch` は `/usr/bin/patch` とぶつかる。プロジェクトのフォルダも `~/project/prod/rpatch` に変更した。
- 新しいファイルの作成や削除は、v0 では扱わない（`cat > file <<EOF` で足りている）。`old` が空ならエラーにする。
- 2026-10-01、試用レビュー（`docs/review-v0.1.0.md`）を受けて、次のように決めた。
  - **区切り形式の入力を受け付ける**（aider の SEARCH/REPLACE に近い形）。JSON も残し、先頭の文字が `{` かどうかで判別する。根拠は、python での置換 342 回のうち 195 回が `'''` の複数行文字列だったこと。
    - 行単位で扱う。ブロックの各行に改行が付いているとみなし、照合は行の先頭からに限る（行の途中に一致させると、インデントが崩れるため）。例外として、ファイルの最終行に改行がない場合は、末尾の改行を外して照合する。
    - ファイルが CRLF なら、ブロックの改行も CRLF に合わせる。
    - 区切りは 7 文字以上。開始行の `<` の数で長さが決まり、同じ長さの区切りでしか閉じない。中身に 7 文字の区切りの行があるときは、9 文字などの長い区切りで書く（v0.3.0、2 回目のレビューを受けて追加）。
  - **アンカー間の範囲置換を追加する。** 範囲は、`from` の先頭から、`from` のあとで最初に出てくる `to` の手前まで。つまり `from` は範囲に含めて置き換え、`to` は残す。ログで一番多かった `s[:a] + new + s[b:]`（`b = s.index(B, a)`）の書き方に合わせた。`from` はちょうど 1 回マッチしなければ失敗にする。成功したときは、置き換えた行の範囲を必ず示す。
  - **`old == new` は入力の不正**（終了コード 2）にする。
  - JSON の構文エラーでは、該当箇所の前後と、`\'` や生の改行についてのヒントを出す。
  - 先の編集が失敗したあとの編集も照合する件は、見送る。

## 範囲外にしたもの

- 検索の出力を自動で削る仕組み。検索 700 回中 417 回で `| head` を使っていたが、patch とは別のツールにする。
- sleep での待ち（42 回）。待つのはハーネスの Monitor / run_in_background の担当。
- JSON の抜き出し（python 139 回、jq 87 回）。jq で足りる。

## 効果の測り方

- 手順:
  1. `tools/log-analysis/extract.py <出力パス>` で、全セッションのツール呼び出しを JSONL に書き出す。セッションの中身が入るので、リポジトリの外に置く（`calls.jsonl` は .gitignore 済み）。
  2. `tools/log-analysis/metrics.py <JSONL> --since … --until … --exclude-proj prod-rpatch` で数値を出す。
- 導入する前後で、次の 3 つを比べる。作業量で回数が変わるので、比率でも見る。
  - python での置換の回数（`python_replace`）と、編集全体に占める割合（`share_python_replace`）
  - Edit ツールで同じファイルを連続編集した回数（`edit_same_file_consecutive`）
  - 置換に失敗した回数（`replace_failed_total`）
- 導入するときは、`~/.claude/CLAUDE.md` の「CLI ツール」の節に 1 行追加する。CLAUDE.md への 1 行で grep が rg にほぼ全部置き換わったので、この入れ方で効くはず。

### ベースライン（導入前、2026-10-01 取得）

`tools/log-analysis/baseline-2026-10-01.json`。期間は 2026-09-24〜2026-10-01T04:50（UTC）で、85 セッション、ツール呼び出しは 3,459 件。rpatch 自身の開発セッションは除いた。

| 数値 | 値 |
|---|---|
| python での置換 | 317（編集全体の 75%、ツール呼び出し 1,000 件あたり 91.6） |
| そのうち assert などの確認がないもの | 113 |
| Edit ツール | 105 |
| Edit で同じファイルを連続編集 | 34 |
| 置換に失敗（python の Traceback/AssertionError、Edit の不一致） | 7 |

- 冒頭の表（345、65、86）とは数え方が違う。冒頭の表は zatsudan のセッションでその場で数えたもので、スクリプトが残っていない。前後の比較には、どちらも `metrics.py` の数値を使う。
- 連続編集は、Edit のあいだに Read/Grep/Glob しか挟まず、同じファイルを続けて編集したものを数えている。
- 置換の失敗は少なく見えるが、確認のない置換（113 件）は、マッチしなくてもエラーにならないので数えられていない。導入後は、確認のない置換が減るぶん、失敗として見えるようになる可能性がある。

## 次のセッションで再開すること（2026-10-01、Claude によるレビュー後）

GPT が作った 0.7.0（MCP 対応）を Claude Code（Opus）でレビューした。lib 化は編集処理を変えておらず、テスト 98 件・clippy・fmt は通る。レビューを受けて次の 2 つを済ませた。

- MCP の結果から `structuredContent` を外し、テキストだけを返すようにした（`src/mcp.rs`、`tests/mcp.rs`、`docs/mcp.md`）。理由は 2 つ。同じ出力が `content` にも入っていて量が 2 倍になること。Codex は `structuredContent` があるとそちらをモデルに渡すはずなので（未確認）、diff が JSON の文字列として見えてしまうこと。成否は `isError` で伝わる。
- `cargo install --locked --path .` で修正版を入れ直し、Claude Code に user scope で登録した（`claude mcp add --transport stdio --scope user rpatch -- ~/.cargo/bin/rpatch mcp`）。`--cwd` は付けていない。Claude Code はプロジェクトのディレクトリでサーバーを起動するので、相対パスはそこが基準になる。bench は `--strict-mcp-config` を使うので、この登録の影響を受けない。

残っていること（上ほど優先）:

1. ~~**新しいセッションで、Claude Code から `mcp__rpatch__rpatch` を実際に使えるか確かめる。**~~ 済み（下の「進み具合」を参照）。
2. ~~**`bench/compare_metrics.py:78-80` の失敗判定。**~~ 済み。Codex で照合の失敗を起こして確かめ、`status` だけで判定するようにした（下の「進み具合」を参照）。
3. ~~**コミットを分ける。**~~ 済み（下の「進み具合」を参照）。`main` に直接積んだ。push はまだしていない。
4. **CI（`.github/workflows/build.yml`）はまだ一度も動かしていない。** Windows は手元で検証していない。Windows では `canonicalize` すると `--cwd` が `\\?\C:\...` の形になり、instructions や `cwd:` の表示にそのまま出る。動作は Rust の `PathBuf::push` の正規化でおそらく大丈夫。表示が気になるなら `dunce` を使う。
5. **小さいもの**
   - ~~`docs/mcp.md` の「1つのサーバー内の編集要求は順番に処理する」は、届いた順とも読める。~~ 済み。
   - ~~MCP では diff が既定で有効。~~ 済み。CLI に合わせて既定を無効にした。
   - ~~release バイナリは rmcp と tokio のぶん 4.0MB ある。~~ 済み。`mcp` を Cargo feature（既定ではオフ）にした。featureなしの release は 0.8MB。
   - ~~CLI がファイルを読む前に `current_dir()` を解決するようになった。~~ 済み。解決できなければ `.` で続けるので、入力の誤りは 2 のまま。
   - ~~`src/help.txt` の「See docs/mcp.md」は、バイナリだけ持っている人には辿れない。~~ 済み。参照をやめ、ツールの引数を help に書いた。
   - ~~`docs/mcp.md` の Codex での確認記録を `docs/mcp-experiment.md` に移す。~~ 済み。リンク先の `learn.chatgpt.com/docs/extend/mcp` は実在した（「Model Context Protocol」）。
   - ~~HANDOFF の冒頭が古いままになっている。~~ 済み。
   - ~~絶対パスを編集すると、diff の見出しが `--- a//tmp/x.txt` と二重スラッシュになる。~~ 済み。絶対パスでは `a/` `b/` を付けない。

## 次のセッションで再開すること（2026-10-01、再開後更新）

- 新しいデスクトップアプリのセッションでは `mcp__rpatch__rpatch` が実際のツール一覧に現れた。MCP経由の一時ファイルへのdry-run、本適用、不一致診断も確認済み。設定画面のUnknown表示自体は確認していない。
- ZDOTDIR固定後のCodex比較を実行済み。CLI 0.159.2／gpt-6.1-sol／rpatch 0.7.0、2課題×3試行×2条件の12実行がすべて正常終了・独立検証成功。有効ペア6件、CLI混入0件。
- 自発的なrpatch使用はCLI条件6/6、MCP条件2/6。編集呼び出しはCLI条件rpatch 7回、MCP条件rpatch 2回＋apply_patch 5回。MCPの使用は文章編集の2・3回目で、設定変更は3回ともapply_patchだった。
- 同じ隔離設定の別セッションでも、CLIが見つからないことと、明示指定したMCPのdry-run成功を確認した。これは自発的使用の集計には含めていない。
- 結果は `docs/mcp-experiment.md` と `bench/results/codex-20261001-zdotdir.json`。生ログ・作業コピー・接続確認・要件確認は `bench/out/codex-compare-20261001-zdotdir/`。12課題のテスト成功、12課題コミット、全6コピーのREADME要件・clean状態も確認済み。
- この結果は1モデル・2課題・3試行で、CLIの短い案内とMCPツール説明の提示差を含む。次に比較を広げるなら、提示情報を揃えた条件と別の編集課題を追加する。モデルを変える場合は別の実験として記録する。
- Codex同梱のrgが `--version` でも停止し、検索がタイムアウトした実行があるため、今回の所要時間から速度を比較しない。時間を比較する前に検索ツールの停止を切り分ける。
- 既存のソース・接続手順・実験スクリプトの未コミット変更はそのまま残した。この再開では結果JSONと記録を追加・更新した。比較と接続確認のプロセスは終了済み。

## 進み具合

- 2026-10-01: HANDOFF の「小さいもの」を片付けた。CI（項目 4）は保留。
  - 絶対パスの diff の見出しに `a/` `b/` を付けない（`--- /tmp/x.txt`）。
  - cwd が削除されていても、入力の誤りは終了コード 2 のまま（`current_dir()` が失敗したら `.` で続ける）。
  - MCP の `diff` の既定を `false` にして、CLI と揃えた。ツール説明・help・`docs/mcp.md` も直した。
  - `docs/mcp.md` の並行処理の説明を「同時には処理しない。届いた順とは限らない」に直した。接続確認の記録は `docs/mcp-experiment.md` の「接続確認（2026-10-01）」に移した。
  - CLI テストを 2 件足した（絶対パスの見出し、削除された cwd）。

- 2026-10-01: 未コミットだった変更をコミットに分け、MCP を Cargo feature にした。`main` に 6 コミット積んだ（最後はこの HANDOFF の更新。push はしていない）。
  - 順に、lib 化（挙動は変えない）→ MCP サーバー（v0.7.0）→ CI → bench と実験記録・README・HANDOFF → `mcp` feature → HANDOFF。各コミットを、その時点のファイルだけを取り出した別ディレクトリで fmt・clippy（`-D warnings`）・テストして確かめた。
  - `mcp` feature は既定でオフ。featureなしで `rpatch mcp` を実行すると、「MCP なしでビルドされた」と 1 行出して終了コード 2 になる（stdin の編集としては処理しない）。`tests/mcp.rs` は feature ありのときだけ動く。CLI のテストを 1 件足して、feature なしのエラーを確かめる（85 件）。feature ありは 84 件＋MCP 8 件。
  - release バイナリは、featureなしで 0.8MB、ありで 4.0MB。CI は feature なし・ありの両方で clippy とテストを回し、配布用の release は `--features mcp` 付きでビルドする（ダウンロードしてそのまま Codex や Claude Code に登録できるように）。
  - `cargo install --locked --path . --features mcp` で入れ直し、`~/.cargo/bin/rpatch` で MCP の initialize と dry-run の tools/call が返ることを確かめた。
  - 作業中に `src/mcp.rs`（未コミットだった）を誤って上書きして失われた。Claude Code のセッション記録（読み出し結果と Sonnet の編集内容）から作り直した。行数（103）と、テスト（MCP 8 件）・clippy が通ることで確かめたが、元のファイルとのバイト単位の照合はできていない。

- 2026-10-01: `bench/compare_metrics.py` の Codex の MCP 失敗判定を直した。
- 2026-10-01: `bench/compare_metrics.py` の Codex の MCP 失敗判定を直した。
  - 修正版のバイナリで Codex（CLI 0.159.2／gpt-6.1-sol、`--ignore-user-config`、rpatch の MCP だけを渡す）に照合の失敗を 1 回起こした。ログは `status: "failed"`、`structured_content: null`、本文は CLI の stderr と同じ診断だった。`isError` だけで `failed` になる。修正前のバイナリの smoke ログでも失敗は `failed` だったので、これまでの集計結果は変わらない。
  - そこで `structured_content.exit_code` を見るのをやめ、`status` と `error` だけで判定する。テストデータも実際の形に合わせた。集計テスト 5 件が通る。
  - 1 回目の試行で、Codex は引数名を `input` ではなく `patch` にして呼んだ（`failed to deserialize parameters: unknown field 'patch'`）。プロンプトで `input` を指定すると正しく呼べた。ツール説明の書き出しで引数名が目立っていない可能性がある。1 回だけなので傾向かどうかは分からない。
  - ログは `bench/out/codex-mcp-fail-20261001/`（`events-1-wrong-arg.jsonl` が 1 回目）。

- 2026-10-01: Claude Code（CLI、Opus）の新しいセッションで `mcp__rpatch__rpatch` を確かめた。`claude mcp get rpatch` は Connected。一時ファイルへの dry-run、本適用、照合の失敗（`"old" not found` と一番似ている行、`rpatch: nothing written`、ツール結果はエラー扱い）の 3 つが期待どおりだった。相対パス（`Cargo.toml`）はプロジェクトのディレクトリを基準に解決された。この HANDOFF の更新も MCP で行った。

- 2026-10-01: Claude Code（Opus）で 0.7.0 をレビューした。MCP の結果から `structuredContent` を外し（Sonnet が実装し、Opus が確認）、修正版を Claude Code に user scope で登録した。テスト 98 件・clippy・fmt が通る。残りは「次のセッションで再開すること（Claude によるレビュー後）」に書いた。

- 2026-10-01: デスクトップアプリの新セッションでrpatch MCPを認識し、CodexのCLI／MCP比較を再開・完了した。
  - MCPのdry-run、本適用、不一致診断に成功。同じ隔離設定での明示指定による接続確認も成功した。
  - 2課題・各条件3回の12実行はすべて検証成功、有効ペア6件。CLIの自発的使用6/6、MCP 2/6。編集失敗・CLI混入は0件。
  - `bench/results/codex-20261001-zdotdir.json` と `docs/mcp-experiment.md` に結果を保存。全6作業コピーのREADME要件、テスト成功、課題コミットも確認した。
  - releaseビルド、集計テスト5件、比較スクリプトのshellcheckが通った。所要時間にはCodex同梱rgの停止の影響があるため、速度の比較には使わない。

- 2026-10-01: Codex側の登録と接続確認を追加した。
  - `cargo install --locked --path .` で手元のrpatchを0.7.0に更新し、`codex mcp add rpatch -- /Users/you/.cargo/bin/rpatch mcp` で登録済み。cwdは固定しない。
  - 新しいCodexセッション（CLI 0.159.2／gpt-6.1-sol）から `mcp__rpatch__rpatch` のdry-run、本適用、不一致診断の3回を確認した。実際のsample.txtの変更も確認済み。既存のアプリセッションはMCP設定の再読込が必要。
  - `bench/compare-codex.sh` とCodex JSONL集計を追加した。集計テスト5件とshellcheckが通った。
  - 自発的な使用の試行は両条件ともCLIを使用し、独立検証に成功。ただしMCP条件にもCLIが見えていたので、排他的な比較から除外。集計の `cli_contamination` で検出し、次回用にZDOTDIRを固定した。生ログは `bench/out/codex-compare-20261001-fixed/`。

- 2026-10-01: ローカルMCPを追加してv0.7.0にした。
  - `rpatch mcp [--cwd <directory>]` でstdioサーバーを起動する。ツールは `rpatch` の1つで、引数は `input`、`diff`（既定true）、`dry_run`（既定false）。
  - 編集処理を `src/lib.rs` に移し、CLIとMCPで共有した。差分・診断はCLIと同じ文面。MCPの結果には `isError` と `structuredContent` の終了コード・stdout・stderrを付ける。
  - サーバー内の編集要求を直列化し、キャンセル後も実行中の編集が終わるまで次を待たせる。cwdは明示的に渡し、プロセス全体の作業ディレクトリは変えない。
  - Rustのテスト98件（既存90＋MCP接続8）、集計テスト3件、fmt／clippy／shellcheckが通った。Apple Siliconでreleaseビルド済み。Windows x64／Intel Mac／Apple Siliconのテストとビルドを `.github/workflows/build.yml` に追加したが、CI自体は未実行。
  - ビルド・接続は `docs/mcp.md`。比較実験は `bench/compare.sh` と `docs/mcp-experiment.md`。既存の普段使いの指示を除き、同じseed・課題で自由にツールを選ばせる。
  - 短い比較試行ではClaude CodeからMCPへの接続成功を確認。ただし両条件ともセッション上限（429）で作業前に終了したので、有効な比較結果はまだない。生ログは `bench/out/compare-20261001-171214/`（gitignore対象）。

- 2026-10-01: `git init` と `cargo init` を済ませ、v0 を実装した。テストは `tests/cli.rs`（28 件）と単体テスト 4 件で、Codex の 3 つの問題を含む。
  - 失敗時のヒントは、次の順で最初に当てはまったものを 1 つ出す（`src/diagnose.rs`）。CRLF の違い → 空白の違い（該当行を表示）→ どこまで一致してどこで食い違うか → 一番似ている行。
  - 読み取り専用のファイルが含まれていたら、何も書かずに終了コード 3 で終わる（rename だと書き込めてしまうので、事前に確かめている）。
  - symlink はリンク先を書き換える。パーミッションは引き継ぐ。
- 2026-10-01: clippy と rustfmt を入れた。clippy の指摘はなく、rustfmt で整形した。導入前のベースラインを取った（「効果の測り方」を参照）。
- 2026-10-01: zatsudan のセッションで試用レビューを受けた（`docs/review-v0.1.0.md`）。それを受けて v0.2.0 を実装した（中身は「決めたこと」を参照）。テストは結合テスト 47 件と単体テスト 5 件。
  - 区切り形式の入力（`src/block.rs`）。照合は `src/matcher.rs` にまとめた。
  - アンカー間の範囲置換（JSON の `from`/`to`、区切り形式の `<<<<<<< FROM` / `<<<<<<< TO`）。成功したときは `edits[0] replaced lines 2-4 (3 lines) with 1 line` のように範囲を示す。
  - JSON の構文エラーでは、該当箇所の前後とキャレット、`\'` や生の改行についてのヒントを出す。
  - `old == new` は終了コード 2 にした。
  - このセッションの後半の編集は rpatch 自身（区切り形式）で行った。4 ファイル・7 箇所（範囲置換 1 つを含む）を 1 回で当てられた。
- 2026-10-01: 2 回目の試用レビュー（`docs/review-v0.2.0.md`）を受けて v0.3.0 にした。レビューの結論は「このまま普段の作業で使い始めていい」。テストは結合テスト 55 件と単体テスト 5 件。
  - TO が見つからないときは、似た箇所を FROM より後ろだけから探す。ラベルも `"to"` にした（それまでは `"old"` で、FROM の行を指していた）。
  - ブロックの中身が行の一部としてだけ見つかるときは、「空白違い」ではなく `found only as part of a line` と出す。インデントの省略も、行の先頭だけ書いた場合（`**Why:**` など）も、これにまとめた。
  - 長い区切り（上の「決めたこと」を参照）。rpatch 自身の `--help` の文面を、9 文字の区切りで編集できた。
  - 区切りの書き間違いには、`hint: missing >>>>>>> NEW before line 6?` のような 1 行のヒントを出す。エラーには、その部分の開始行（NEW なら `=======` の行）を出す。
  - レビューは `docs/review-v0.1.0.md` と `docs/review-v0.2.0.md` に置いた。
- 2026-10-01: 3 回目のコメント（残った問題は区切りの長さの食い違いだけ）を受けて v0.3.1 にした。ブロックの中で長さだけが違う区切りらしい行を覚えておき、ブロックが閉じないときや予期しない区切りがあるときに `hint: line 4 has ======== (8 characters), but this block's markers are 9 characters long` と示す。そういう行があるときは、長い区切りを勧めるヒントは出さない。
- 2026-10-01: 4 回目のコメント（導入後の初使用）を受けて v0.4.0 にした。
  - 成功時の `lines=` は「NEW の先頭の行番号」だったが、ファイル全体の行数と読み違えられた（末尾への追記で、たまたま一致した）。`at_lines=3,10-12` に改め、実際に変わった行の範囲を出す。OLD と NEW で先頭・末尾が同じ行（文脈として書いた行）は数えない。削除は、消した位置の行番号を出す。`--help` に出力の読み方を書いた。
  - **範囲置換で、NEW の末尾が TO と同じなら失敗にする**（終了コード 1）。TO は残るので、NEW の末尾にも書くと 2 回出てくる。v0.4.0 を作る途中で、私（Claude）がこの間違いを 3 回した。範囲の表示（`replaced lines 500-518 …`）は正しかったが、見落とした。警告だけでは足りないと判断した。TO を本当に 2 回並べたいときは、OLD で書く。
  - 同じ作業中に、FROM に行の一部だけを書く間違いも 2 回した。どちらも v0.3.0 で直した診断（`found only as part of a line`）で、すぐ原因が分かった。
- 2026-10-01: beat-jam での実際の作業で 9 回使ったメモ（`docs/review-v0.4.1.md`）を受けて v0.5.0 にした。
  - 一番効いたのは FROM/TO（関数の中身を、両端の 1 行ずつで書き換えられる）。一番多い失敗は「長い 1 行の先頭だけを OLD に書く」（同じ日に 2 回）。9 回のうち 4 回は「既存の行の前後に足すだけ」だった。
  - **`<<<<<<< BEFORE` / `<<<<<<< AFTER` を追加した。** アンカーは行の一部でよい（行の境目に挿入するだけで、既存の行は書き換えないので安全）。アンカーはちょうど `count` 回（既定 1）マッチしなければ失敗にする。同じ行で 2 回マッチしたら失敗にする。区切り形式だけで、JSON にはない（JSON は `old`/`new` で書ける）。
  - 「行の一部としてだけ見つかった」の診断で、BEFORE/AFTER を案内する。
  - 未知のオプションでは、ヘルプ全文ではなく 1 行の usage を出す。`--help` に Tips（`<<'EOF'` で引用符を付ける、続けるコマンドは `&&` でつなぐ）を足した。
  - 見送り: `OLD partial`（行の一部の書き換え）。挿入は BEFORE/AFTER で済み、書き換えは JSON で書ける。
  - メモ: Claude Code の Edit ツールで編集するファイルを rpatch でも書き換えると、Edit の前に読み直しが要る（ハーネス側の挙動）。1 つのファイルにはどちらか片方を使うほうが手間が少ない。
- 2026-10-01: v0.5.0 の確認メモ（`docs/review-v0.5.0.md`）を受けて v0.5.1 にした。要望の 4 つはどれも期待どおりだった。
  - BEFORE/AFTER のアンカーは 1 行だけにした（2 行以上は終了コード 2）。それまでは 1 行目が行の一部だと、2 行をつないだ文字列が見つからず、`no similar text found` と出て戸惑わせていた。
  - アンカーが複数に当たったときのヒントで、`<<<<<<< AFTER count=N`（全部に入れる）も示す。
  - `--help` の JSON の節に「BEFORE/AFTER は区切り形式だけ」と書いた。
- 2026-10-01: 他のセッションから届いた Issue #5〜#13 をまとめて片付けて v0.6.0 にした。
  - #8（bug、重要）: 重なり合う一致も数える。重なっていたら、count に関係なく失敗にする（`found 2 (overlapping)`）。それまでは、`x\nx\nx` に OLD `x\nx` が黙って 1-2 行目を置換していた。
  - #7: 区切り形式の FROM/TO の最後の行は、行の先頭部分だけでよい（長い段落の書き出しで範囲の端を指せる）。照合は今までどおり行の先頭から。「行のどこでも」にしなかったのは、TO によく使う `}` がインデントされた `    }` にも当たってしまうため。
  - #6: 行の一部としてしか見つからないとき、どの行が切れているかを示す（`OLD line 3 is only the start of line 3`、`OLD line 1 is only the end of line 2`）。
  - #9: 区切りの行の末尾の空白を無視する。#10: 一番似ている行の表示で、入力側も空白付きのまま出す。#11: UTF-8 の BOM は、読むときに外して書くときに付け直す。#5: 区切り形式では count のヒントも `<<<<<<< OLD count=N` の形にする。
  - #12、#13: `--help` に書いた（同じアンカーへの AFTER は逆順になる。rename でハードリンクが切れる）。ハードリンクが切れるときは警告も出す。
- 2026-10-01: 効果測定の途中経過（#4 にコメント）。導入から約 1.5 時間、2 セッション（beat-jam、zatsudan）で、python での置換は 317（75%）→ **0**、rpatch は 51 回（dry run 16）、本番の失敗は 5 回。ただし、どちらも rpatch を試すのが主な目的のセッションだったので、普段の作業を 1〜2 日したあとで測り直す。`metrics.py` は dry run を別に数えるようにした（失敗するケースをわざと試すのに使われるため）。

## 導入（2026-10-01 14:22 JST = 05:22 UTC）

- `cargo install --path .` で `~/.cargo/bin/rpatch`（v0.3.1）に入れた。コードを変えたら、もう一度入れ直す。0.7.0 からは MCP が Cargo feature になったので、`cargo install --locked --path . --features mcp` で入れる（Claude Code と Codex に MCP として登録しているため）。featureなしだと `rpatch mcp` が使えなくなる。
- `~/.claude/CLAUDE.md` の「CLI ツール」の節に、次の 1 行を足した（`sd` の行の次）。
  - 「複数行・複数箇所の置換: `rpatch`（stdin に区切り形式で渡す。書き方は `rpatch --help`）。python のヒアドキュメントで置換しない」
  - 2026-10-01 15:59 JST（06:59 UTC）に「置換」を「置換・挿入」に変えた（BEFORE/AFTER を入れたため）。導入後の集計で、この前後を分けて見るときの区切りにする。
- これより前に zatsudan のセッションで試用した分（絶対パスで呼んだもの）は、導入前として扱う。

## リポジトリと要望の受け方（2026-10-01 から）

- GitHub: https://github.com/lenalab-me/rpatch （private）。`main` を push している。
- **やることと要望は Issue で管理する。** 開いている項目は `gh issue list --repo lenalab-me/rpatch` で見る。
- 他のセッションで使ってもらうときは、気づいたことを 1 件ずつ `gh issue create --repo lenalab-me/rpatch` で起票してもらう。不具合は `bug`、要望は `enhancement` のラベルを付ける。
- 直したコミットのメッセージには `Fixes #N` を書き、push で Issue を閉じる。
- `docs/review-*.md` は、Issue にする前の経緯の記録として残す。

## 次のセッションでやること

1. 普段の作業で使う（bypass モードを含む）。気になった出力や失敗は Issue にする
2. 1〜2 日使ったら、効果を測る（#4）
3. v1（tree-sitter での構文チェック）。比べた結果を見てから始める
