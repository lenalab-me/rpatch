# 引き継ぎ: rpatch（エージェント向けの一括置換 CLI）

新しいセッションは、この文書から再開する。2026-10-02 に短くした。それまでの全文（始めたときの検討、仕様案、日々の進み具合）は `~/project/prod/rpatch-notes/handoff-history.md` にある。git の履歴にも残っている。

## いまの状態

- v0.8.0。Claude Code（と Codex）が Bash から呼ぶ CLI として、普段の作業で使っている。
- MCP サーバーは Cargo feature `mcp`（既定ではオフ）にある。2026-10-02 に Claude Code と Codex の登録を外し、手元も MCP なしで入れている。
- 仕様と設計は `docs/design.md`（英語版 `docs/design.en.md`）、LLM 向けの入口は `llms.txt`、使い方は `rpatch --help`。
- 試用レビュー、実験の記録、集計の結果、CLI と MCP の比較実験の一式は、リポジトリの外の `~/project/prod/rpatch-notes/` に置く。

## 入れ方と、普段の使われ方

- コードを変えたら `cargo install --locked --path .` で入れ直す（CLI だけ、0.8MB）。MCP を使い直すときは `--features mcp` で入れてから、`claude mcp add --transport stdio --scope user rpatch -- ~/.cargo/bin/rpatch mcp`、`codex mcp add rpatch -- ~/.cargo/bin/rpatch mcp` で登録する。
- エージェントへの案内は `~/.claude/CLAUDE.md` の 1 行だけ: 「複数行・複数箇所の置換・挿入: `rpatch`（stdin に区切り形式で渡す。書き方は `rpatch --help`）。python のヒアドキュメントで置換しない」。
- エージェントは `rpatch --help | head -40` で読むことが多い。help は、書き方・ブロックの種類・Tips・出力・終了コードを先頭 37 行に収めている。help を変えるときは、この 40 行を崩さない。

## 主な判断（理由は handoff-history.md と git のログ）

- **一致は完全一致、全部か無しか。** 失敗したら何も書かず、行番号と原因を出す。
- **OLD は行全体だけに一致する。** 長い行の先頭だけを書いて部分一致させると、行の残りが黙って残るため。行の一部は `<<<<<<< OLD part` と明示して書く（v0.8.0、#2）。きっかけは、指示なしのセッションで、行の一部の書き換えのために python や `sd -F` に戻っていたこと。
- **MCP はやめた（2026-10-02）。** MCP のツールは、説明文だけを見て書き方を推測されがちだった（Claude Code は `--- old` / `+++ new` 形式、Codex は引数名 `patch`）。CLI なら CLAUDE.md の案内でまず `--help` を読む。Codex での比較でも、自分から使ったのは CLI 6/6、MCP 2/6。
- **書き込む前の構文チェック（v1、tree-sitter）は見送った（#3）。** 実際に構文を壊した編集は 670 回中 3 回（0.45%）で、どれもエージェント自身の確認ですぐ見つかっていた。`tools/log-analysis/syntax_breaks.py` で測り直せる。
- **CI は手動起動だけ。** private リポジトリでは macOS のランナーが 10 倍、Windows が 2 倍の分数を消費するため。配布用のバイナリが要るときに `gh workflow run build.yml`。Windows はまだ一度も動かしていない。

## 効果の測り方（#4）

1. `python3 tools/log-analysis/extract.py /tmp/rpatch-calls.jsonl` で、全セッションのツール呼び出しを書き出す。セッションの中身（認証情報を含むことがある）が入るので、リポジトリの外に置き、終わったら消す。
2. `python3 tools/log-analysis/metrics.py /tmp/rpatch-calls.jsonl --since … --exclude-proj prod-rpatch --exclude-proj pi-agent` で数値を出す。結果を残すなら `~/project/prod/rpatch-notes/log-analysis/` に置く。
3. 見る数値: python での置換（`python_replace`、`share_python_replace`）、rpatch での編集（`rpatch`、`--help` などは `rpatch_discovery` に分けて数える）、Edit、失敗とそこからの立ち直り（`rework`）。

1 つのセッションをすぐ見たいときは `python3 tools/log-analysis/session.py [プロジェクト名やセッション ID の一部]`。サブエージェントを含めて、編集の仕方の内訳（rpatch・sd・cat>・Edit・python など）と、rpatch の失敗ごとのエラー・hint・立ち直りまでの秒数を出す。引数なしなら直近のセッション（prod-rpatch と pi-agent を除く）、`--list` で直近のセッションの一覧、`-v` で失敗した呼び出しの入力と出力も出す。ブラウザで見るなら `python3 tools/log-analysis/dashboard.py`（http://127.0.0.1:8765/）。開くたびに集計し直し、10 分ごとに再読み込みする。期間を選べるセッション一覧、最近の rpatch の失敗、セッションごとの内訳を出す。

ベースライン（導入前、2026-09-24〜2026-10-01T04:50 UTC、85 セッション）: python での置換 317 回（編集の 75%）、Edit 105 回。

これまでの結果:

- 指示なしの beat-jam のセッション（2026-10-01 11:48〜15:14 UTC）: python での置換 0、rpatch 19、Edit 7、sd 11。複数箇所の編集では rpatch を選んでいた。失敗は 3 回とも「長い 1 行の一部だけを OLD に書く」で、1 分以内に直っていた。
- 同じセッションの終盤で、行の一部の書き換えのために python が 6 回戻った。これを受けて `OLD part` を入れた。新しいセッションでは、最初の成功から `OLD part` を狙いどおりに使っていた。
- まだ見ていないこと: `OLD part` を入れたあと、行の一部の書き換えで python や `sd -F` が減ったか。編集とコミットが同じ呼び出しに入っていないか（下の CLAUDE.md の git の決まり）。別のプロジェクトでも同じ傾向か。

### 比較の区切りにする時刻（CLAUDE.md を変えたとき）

| 時刻（UTC） | 変更 |
|---|---|
| 2026-10-01 05:22 | rpatch の 1 行を足した（導入） |
| 2026-10-01 06:59 | 「置換」を「置換・挿入」に（BEFORE/AFTER） |
| 2026-10-01 11:43 | `sd` の `--`、`gh` の `--body-file` の注意書き（rpatch とは関係ない） |
| 2026-10-01 17:40〜17:42 | 「# git」の節: コミットは編集の結果を見てから別の呼び出しで。同じ呼び出しなら 1 本の `&&` でつなぐ（あとで短くした） |
| 2026-10-02 | v0.8.0（`OLD part`）、help の組み替え、MCP の登録を外した |

## リポジトリと要望の受け方

- GitHub: https://github.com/lenalab-me/rpatch （private）。`main` に直接積んで push する。
- やることと要望は Issue で管理する（`gh issue list --repo lenalab-me/rpatch`）。ほかのセッションで気づいたことは 1 件ずつ起票してもらう。不具合は `bug`、要望は `enhancement`。
- 直したコミットのメッセージに `Fixes #N` を書き、push で Issue を閉じる。
- 開いている Issue: #4（効果測定）、#1（失敗した編集のあとの、同じファイルの編集も照合する。見送ったまま）。

## 次にやること

1. 普段の作業で使う。気になった出力や失敗は Issue にする。
2. 指示なしのセッションがいくつか溜まったら、効果を測る（#4）。上の「まだ見ていないこと」を確かめる。別のプロジェクトでも同じ傾向なら、#4 を閉じる。
