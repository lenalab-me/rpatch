# 引き継ぎ: rpatch（エージェント向けの一括置換 CLI）

新しいセッションは、この文書から再開する。2026-10-02 に短くした。それまでの全文（始めたときの検討、仕様案、日々の進み具合）は `~/project/prod/rpatch-notes/handoff-history.md` にある。git の履歴にも残っている。

## いまの状態

- v0.12.0。Claude Code（と Codex）が Bash から呼ぶ CLI として、普段の作業で使っている。
- MCP サーバーは Cargo feature `mcp`（既定ではオフ）にある。2026-10-02 に Claude Code と Codex の登録を外し、手元も MCP なしで入れている。
- 仕様と設計は `docs/design.md`（英語版 `docs/design.en.md`）、LLM 向けの入口は `llms.txt`、使い方は `rpatch --help`。
- 試用レビュー、実験の記録、集計の結果、CLI と MCP の比較実験の一式は、リポジトリの外の `~/project/prod/rpatch-notes/` に置く。

## 入れ方と、普段の使われ方

- コードを変えたら `cargo install --locked --path .` で入れ直す（CLI だけ、0.8MB）。MCP を使い直すときは `--features mcp` で入れてから、`claude mcp add --transport stdio --scope user rpatch -- ~/.cargo/bin/rpatch mcp`、`codex mcp add rpatch -- ~/.cargo/bin/rpatch mcp` で登録する。
- エージェントへの案内は `~/.claude/CLAUDE.md` の 2 行だけ（Codex の `~/.codex/AGENTS.md` と Cursor の `~/.cursor/rules/global.mdc` も 2026-10-03 から同じ内容。CLAUDE.md を変えたら両方にコピーする。ただし T3 から delegate した Cursor の子にはルールが届かないので、タスクの文に書く）（2026-10-02 から英語）: 「Replace: `sd` for replacing in a pipe or with a regex. To change a file, use `rpatch`, even for one spot …」と「Multi-line or multi-place replacements and insertions: `rpatch` (pass blocks on stdin; the format is in `rpatch --help`). Chain follow-up commands on the opening line … Don't replace with a python heredoc」。
- エージェントは `rpatch --help | head -40` で読むことが多い。help は、書き方・ブロックの種類・Tips・出力・終了コードを先頭 39 行に収めている。help を変えるときは、この 40 行を崩さない。

## 主な判断（理由は handoff-history.md と git のログ）

- **一致は完全一致、全部か無しか。** 失敗したら何も書かず、行番号と原因を出す。
- **OLD は行全体だけに一致する。** 長い行の先頭だけを書いて部分一致させると、行の残りが黙って残るため。行の一部は `<<<<<<< OLD part` と明示して書く（v0.8.0、#2）。きっかけは、指示なしのセッションで、行の一部の書き換えのために python や `sd -F` に戻っていたこと。
- **書式はゆるく、照合は厳しく。** 意味がひとつにしか取れない書き方の揺れは受け付ける（v0.11.0 から、ブロックの直前のパスだけの行や `--- path` を `=== path` として読む）。止めると、エージェントが heredoc や `cat >` に逃げ、確かめずに書いて壊すほうが危ないため（10/4〜10/6 の 7 回はどれも rpatch に戻らなかった）。照合の側は緩めない。
- **MCP はやめた（2026-10-02）。** MCP のツールは、説明文だけを見て書き方を推測されがちだった（Claude Code は `--- old` / `+++ new` 形式、Codex は引数名 `patch`）。CLI なら CLAUDE.md の案内でまず `--help` を読む。Codex での比較でも、自分から使ったのは CLI 6/6、MCP 2/6。
- **書き込む前の構文チェック（v1、tree-sitter）は見送った（#3）。** 実際に構文を壊した編集は 670 回中 3 回（0.45%）で、どれもエージェント自身の確認ですぐ見つかっていた。`tools/log-analysis/syntax_breaks.py` で測り直せる。
- **CI は手動起動だけ。** private リポジトリでは macOS のランナーが 10 倍、Windows が 2 倍の分数を消費するため。配布用のバイナリが要るときに `gh workflow run build.yml`。Windows はまだ一度も動かしていない。

## 効果の測り方（#4）

1. `python3 tools/log-analysis/extract.py /tmp/rpatch-calls.jsonl` で、全セッションのツール呼び出しを書き出す。セッションの中身（認証情報を含むことがある）が入るので、リポジトリの外に置き、終わったら消す。
2. `python3 tools/log-analysis/metrics.py /tmp/rpatch-calls.jsonl --since … --exclude-proj prod-rpatch --exclude-proj pi-agent` で数値を出す。結果を残すなら `~/project/prod/rpatch-notes/log-analysis/` に置く。
3. 見る数値: python での置換（`python_replace`、`share_python_replace`）、rpatch での編集（`rpatch`、`--help` などは `rpatch_discovery` に分けて数える）、Edit、失敗とそこからの立ち直り（`rework`）。

1 つのセッションをすぐ見たいときは `python3 tools/log-analysis/session.py [プロジェクト名やセッション ID の一部]`。サブエージェントを含めて、編集の仕方の内訳（rpatch・sd・cat>・Edit・python など）と、rpatch の失敗ごとのエラー・hint・立ち直りまでの秒数を出す。引数なしなら直近のセッション（prod-rpatch と pi-agent を除く）、`--list` で直近のセッションの一覧、`-v` で失敗した呼び出しの入力と出力も出す。ブラウザで見るなら `python3 tools/log-analysis/dashboard.py`（http://127.0.0.1:8765/）。開くたびに集計し直し、10 分ごとに再読み込みする。期間を選べるセッション一覧、失敗の内訳（種類ごとの件数、rpatch で立ち直った割合と時間、そのあと何で書き換えたか）、最近の rpatch の失敗、セッションごとの内訳を出す。

ベースライン（導入前、2026-09-24〜2026-10-01T04:50 UTC、85 セッション）: python での置換 317 回（編集の 75%）、Edit 105 回。

これまでの結果:

- 指示なしの beat-jam のセッション（2026-10-01 11:48〜15:14 UTC）: python での置換 0、rpatch 19、Edit 7、sd 11。複数箇所の編集では rpatch を選んでいた。失敗は 3 回とも「長い 1 行の一部だけを OLD に書く」で、1 分以内に直っていた。
- 同じセッションの終盤で、行の一部の書き換えのために python が 6 回戻った。これを受けて `OLD part` を入れた。新しいセッションでは、最初の成功から `OLD part` を狙いどおりに使っていた。
- 指示なしの試用（2026-10-06 14:37〜14:49 UTC、v0.11.0、Claude Code の Opus 5.5）: `~/project/lab/tasklog` を複製した `~/project/lab/tasklog-trial-1006` で、期限とタグの追加、文書、テストを頼んだ。rpatch のことは依頼に書いていない。始めにみずから `rpatch --help | head -60` を 1 回読み、既存ファイルの編集は rpatch 22 回（Edit・python の置換・`cat >` は 0）。成功した 19 回で 98 ブロック（1 回に最大 14、11 ファイル）。OLD 87・BEFORE 5・FROM 3・OLD part 2・AFTER 1。失敗 3 回はどれも次の rpatch で直った（6・38・35 秒）。後続のコマンドは開始行に `&&` でつなぎ（`git diff --stat` 13 回、`npm test` 8 回）、コミットは編集と別の呼び出しだった。`sd` は正規表現の置換に 2 回。途中で T3 Code が落ち、「続きを」とだけ送って再開した。失敗のうち 1 回は、切れた行の前に行を足す編集で、`OLD part` のプレビューが切れた行だけを見せて元と同じに見えたもの。これを受けて v0.11.1 で、置き換えたあとの行を 8 行まで見せるようにした。
- ベンチ（`tools/bench/run.sh`、11 課題、Sonnet 5.5）を 2026-10-06 に v0.11.1 で回し直した（出力は `tools/bench/out/20261006-235844`、作業ディレクトリは `~/project/lab/tasklog-bench-1006`）。2026-10-01 の初回と比べて、rpatch の失敗は 12/31（39%）から 6/29（21%）、行の一部の失敗は 10 から 3 に減った。`OLD part` は 0 から 37 ブロック、`sd` は 4 から 0。python の置換を 2 回試みたが、どちらもフックが止めて Write と rpatch に移った。時間と費用はほぼ同じ（約 10 分・$2.6〜2.8）。この間に CLAUDE.md の書き方とフックも変わっているので、rpatch だけの効果ではない。
- まだ見ていないこと: `OLD part` を入れたあと、行の一部の書き換えで python や `sd -F` が減ったか。編集とコミットが同じ呼び出しに入っていないか（下の CLAUDE.md の git の決まり）。別のプロジェクトでも同じ傾向か。

### 比較の区切りにする時刻（CLAUDE.md を変えたとき）

| 時刻（UTC） | 変更 |
|---|---|
| 2026-10-01 05:22 | rpatch の 1 行を足した（導入） |
| 2026-10-01 06:59 | 「置換」を「置換・挿入」に（BEFORE/AFTER） |
| 2026-10-01 11:43 | `sd` の `--`、`gh` の `--body-file` の注意書き（rpatch とは関係ない） |
| 2026-10-01 17:40〜17:42 | 「# git」の節: コミットは編集の結果を見てから別の呼び出しで。同じ呼び出しなら 1 本の `&&` でつなぐ（あとで短くした） |
| 2026-10-02 | v0.8.0（`OLD part`）、help の組み替え、MCP の登録を外した |
| 2026-10-02 02:25 | 「置換」の行を変えた: ファイルの書き換えは 1 か所でも rpatch、sd はパイプの中と正規表現だけ。直前 3 日の sd 174 件のうち約 155 件が 1 ファイルへの `sd -F` だった |
| 2026-10-02 02:27 | CLAUDE.md 全体を英語にした（意味は変えていない）。02:25 の変更と近いので、前後の比較では 1 つの区切りとして扱う |
| 2026-10-02 02:31 | rpatch の行に「後続コマンドは開始行に `&&` でつなぐ。閉じた EOF の後のコマンドは失敗を隠す」を足した。直前 24 時間の rpatch の失敗 45 件のうち 27 件で、後続のコマンドが終了コードを 0 にしていた（`dashboard.py` の「終了コードが隠れた失敗」で追える） |
| 2026-10-02 08:02 | v0.9.0: 空の TO はファイルの最後まで。FROM を OLD のように使ったときの hint。その前の 805df2f（未リリース）で、書式エラーの hint（TO の直後で入力が終わった、heredoc の順番）と `nothing written` |
| 2026-10-02 17:30 | フックを入れた: `~/.claude/settings.json` の PreToolUse（Bash）で `~/.claude/hooks/no-python-replace.py` を呼び、python の置換で書き込むコマンド（`python3 -c`・`python3 - <<`・`python3 <<` のうち `.replace(`/`re.sub(` と書き込みを含むもの）を拒否して rpatch を案内する。サブエージェントにも効く。過去のログ 6,748 回の Bash に当てると、metrics.py の `python_replace` 349 回のうち 345 回を止め、それ以外は止めなかった。以後 `python_replace` は 0 になるはずなので、拒否されたあとに何を使ったかを見る |
| 2026-10-03 10:55 | v0.10.0: `rpatch FILE <<'EOF'` を受け付ける（`=== ` 行より前のブロックの対象）。10/2 からの Claude Code で `unknown argument` が 7 回あった。BEFORE/AFTER の文字列を 2 行以上にできる（Codex が 2 行以上を書いて 3 回失敗していた）。以後、この 2 つの失敗は 0 になるはず |
| 2026-10-03 11:00 | Codex の `~/.codex/AGENTS.md` を `~/.claude/CLAUDE.md` と同じ内容にした（それまでは 10/1 の日本語版で、置換は `sd -F`、rpatch は複数行・複数箇所だけ、`--diff` を付ける、だった。元のファイルは `~/.codex/backups/AGENTS.md.20261003-before-claude-sync`）。以後、Codex で 1 か所の置換に rpatch を使うか、apply_patch がどれだけ残るかを見る |
| 2026-10-06 14:27 | v0.11.0 を入れた。コードの見直しと 1 週間のログから: 空行で終わる FROM/TO/アンカーは空行に一致（空行 1 行の TO が後ろの行を黙って消していた）、TO の重複判定は行単位、改行の混在は専用のエラー、パスだけの行と `--- path` をファイル行として読む、OLD が行の一部としてだけ見つかったら `OLD part` にした結果の行を見せる、打ち消し合った編集は `= <file> … unchanged`。直前 7 日（716 回中 125 回失敗）の内訳: 入力の書式 33（うちファイル行の書き間違い 7）、行の一部（後半欠け）32、NEW と TO の重複 9。以後、ファイル行の書き間違いは 0 に、行の一部の失敗は次の 1 回で直る割合（いま 27/32、中央値 9 秒）と、`sd` や `cat>` に移る件数を見る |
| 2026-10-06 14:54 | v0.11.1: `OLD part` のプレビューで、切れた行だけでなく置き換えたあとの行を 8 行まで見せる（上の試用で、行を足す編集が元と同じ行に見えた）。以後、行の一部の失敗で、次の 1 回が `OLD part` になる割合を見る |
| 2026-10-06 15:14 | v0.12.0: 読み込んだあとにファイルが変わっていたら、何も書かずに終了コード 3（書き込みの前と、ファイルごとの rename の直前に、バイト単位で確かめる）。同じ作業ツリーのほかのエージェントやフォーマッタの変更を黙って消さないため。集計では、フックに止められた python の置換を `python_replace_blocked`（session.py とダッシュボードは `py阻止`）として別に数える |

## リポジトリと要望の受け方

- GitHub: https://github.com/lenalab-me/rpatch （private）。`main` に直接積んで push する。
- やることと要望は Issue で管理する（`gh issue list --repo lenalab-me/rpatch`）。ほかのセッションで気づいたことは 1 件ずつ起票してもらう。不具合は `bug`、要望は `enhancement`。
- 直したコミットのメッセージに `Fixes #N` を書き、push で Issue を閉じる。
- 開いている Issue: #4（効果測定）、#1（失敗した編集のあとの、同じファイルの編集も照合する。見送ったまま）。

## 次にやること

1. 普段の作業で使う。気になった出力や失敗は Issue にする。
2. 指示なしのセッションがいくつか溜まったら、効果を測る（#4）。上の「まだ見ていないこと」を確かめる。別のプロジェクトでも同じ傾向なら、#4 を閉じる。
