# rpatch v0.2.0 の試用のお願い

rpatch v0.2.0 ができました（コミット `6f3ed42`）。前回のレビュー（`~/project/prod/rpatch/docs/review-v0.1.0.md`）で出た優先順位の 4 項目を、すべて入れています。もう一度試して、感想をもらえると助かります。
バイナリは前回と同じ `/Users/you/project/prod/rpatch/target/release/rpatch` です（`-V` で `0.2.0` と出ます）。詳しい仕様は `rpatch --help` に書いてあります。

## 変わったこと

### 1. 区切り形式の入力（前回の提案 1）

先頭の文字が `{` なら JSON、それ以外なら区切り形式として読みます。

```bash
/Users/you/project/prod/rpatch/target/release/rpatch <<'EOF'
=== app.mjs
<<<<<<< OLD
function help(svc, cmd) {
  const key = `${svc} ${cmd}`;
=======
function help(svc, cmd, sub) {
  const key = sub ? `${svc} ${cmd} ${sub}` : `${svc} ${cmd}`;
>>>>>>> NEW
<<<<<<< OLD count=2
old line
=======
new line
>>>>>>> NEW

=== b.html
<<<<<<< OLD
<p>delete me</p>
=======
>>>>>>> NEW
EOF
```

- ブロックの中身はそのまま書きます。エスケープは要りません。
- `=== file` のあとには、ブロックをいくつでも続けて書けます。
- 行単位で扱います。ブロックの各行に改行が付いているとみなすので、NEW を空にすれば行ごと消せます。
- **照合は行の先頭からに限ります。** インデントを省いて書くと、一致させずに「行の途中にはある」と場所を示して失敗します。行の途中に一致させると、インデントが崩れるためです。
- ファイルが CRLF なら、ブロックの改行も CRLF に合わせます。ファイルの最終行に改行がなくても照合できます。
- `count` はヘッダー行ではなく、ブロックの開始行（`<<<<<<< OLD count=2`）に書きます。提案ではヘッダー行に書く形でしたが、1 つのファイルに複数のブロックを書けるので、ブロックごとに指定できるほうにしました。
- 中身に `=======` だけの行があると区切りと区別できません。そのときは JSON で書いてください。

### 2. アンカー間の範囲置換（前回の提案 2）

```
=== index.html
<<<<<<< FROM
<section id="a">
<<<<<<< TO
<section id="b">
=======
<section id="a">replaced</section>
>>>>>>> NEW
```

- 範囲は、FROM の先頭から、FROM のあとで最初に出てくる TO の手前までです。**FROM は範囲に含めて置き換え、TO は残します。** python のログで一番多かった `s[:a] + new + s[b:]`（`b = s.index(B, a)`）と同じ範囲です。
- FROM がちょうど 1 回マッチしなければ失敗にします。TO は、FROM のあとで最初に出てくるものを使います。
- 成功すると、`edits[0] replaced lines 2-4 (3 lines) with 1 line` のように、置き換えた範囲を必ず出します。
- JSON では `{"file": "...", "from": "...", "to": "...", "new": "..."}` と書きます。JSON の場合は、行の先頭でなくても照合します。

### 3. JSON エラーのヒント（前回の提案 3）

```
rpatch: invalid JSON input: invalid escape at line 1 column 41
  {"edits":[{"file":"a.js","old":"say('it\'s')","new":"x"}]}
                                          ^
  hint: JSON only has the escapes \" \\ \/ \b \f \n \r \t \uXXXX; write ' as is (not \')
  hint: for multi-line text, the block format needs no escaping (see rpatch --help)
```

生の改行のときは、改行がある行の末尾を指します。

### 4. `old == new`（前回の提案 4）

入力の不正（終了コード 2）にしました。

### 見送ったもの

先の編集が失敗したあとの編集は、今回も `not checked` のままです。区切り形式で往復の回数そのものが減ると考えたためです。

## 試してほしいこと

前回のように、まず一時ディレクトリで成功するケースと失敗するケースを試してください。そのあと、普段の置換（特に複数行のブロックと、`s.index` で範囲を切り出していたような置換）を、区切り形式で書いてみてください。

## 聞きたいこと

1. **区切り形式**: python の `'''` と比べて、書きやすさはどうか。区切りの書き間違いをしたとき、エラーの出力で直せたか。
2. **行の先頭からに限る照合**: 安全のためにそうしているが、不便に感じたか。インデントを省いて失敗したとき、出力で直せたか。
3. **範囲置換**: 「FROM は置き換え、TO は残す」「TO は FROM のあとで最初に出てくるもの」は直感に合うか。置き換えた範囲の表示で、意図どおりかを確認できたか。
4. **まだ python に戻りたくなる場面**: あれば、どんな置換か。

気づいたことは、前回と同じように `review.md` にまとめて、`~/project/prod/rpatch/` に置いてください。
