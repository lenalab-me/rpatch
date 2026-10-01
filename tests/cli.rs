use std::fs;
use std::path::Path;

use assert_cmd::assert::Assert;
use predicates::prelude::*;
use serde_json::{Value, json};
use tempfile::TempDir;

fn rpatch(dir: &Path, args: &[&str], input: &Value) -> Assert {
    assert_cmd::cargo_bin_cmd!("rpatch")
        .current_dir(dir)
        .args(args)
        .write_stdin(input.to_string())
        .assert()
}

fn setup(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }
    dir
}

fn read(dir: &TempDir, name: &str) -> String {
    fs::read_to_string(dir.path().join(name)).unwrap()
}

// --- Codex apply_patch の 3 つの問題 -----------------------------------------

/// 2 ファイル目で失敗したら、1 ファイル目も書き換えない。
#[test]
fn atomic_across_files_when_second_edit_does_not_match() {
    let dir = setup(&[("a.txt", "hello\n"), ("b.txt", "world\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "hello", "new": "HELLO"},
            {"file": "b.txt", "old": "nope", "new": "NOPE"},
        ]}),
    )
    .code(1)
    .stdout("")
    .stderr(predicate::str::contains("edits[1] b.txt"))
    .stderr(predicate::str::contains("nothing written"));
    assert_eq!(read(&dir, "a.txt"), "hello\n");
    assert_eq!(read(&dir, "b.txt"), "world\n");
}

#[test]
fn atomic_across_files_when_second_file_is_missing() {
    let dir = setup(&[("a.txt", "hello\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "hello", "new": "HELLO"},
            {"file": "missing.txt", "old": "x", "new": "y"},
        ]}),
    )
    .code(1)
    .stderr(predicate::str::contains("missing.txt: file not found"))
    .stderr(predicate::str::contains(format!(
        "cwd: {}",
        dir.path().canonicalize().unwrap().display()
    )));
    assert_eq!(read(&dir, "a.txt"), "hello\n");
}

/// 同じ文字列が 2 箇所にあるとき、黙って 1 つ目だけ置換しない。
#[test]
fn duplicate_match_is_rejected_with_all_line_numbers() {
    let dir = setup(&[("a.js", "x = 1;\ny = 2;\nx = 1;\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.js", "old": "x = 1;", "new": "x = 9;"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains("expected 1 match, found 2"))
    .stderr(predicate::str::contains("lines 1, 3"));
    assert_eq!(read(&dir, "a.js"), "x = 1;\ny = 2;\nx = 1;\n");
}

#[test]
fn duplicate_match_with_matching_count_replaces_all() {
    let dir = setup(&[("a.js", "x = 1;\ny = 2;\nx = 1;\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.js", "old": "x = 1;", "new": "x = 9;", "count": 2}]}),
    )
    .success()
    .stdout("M a.js  edits=1  at_lines=1,3\n");
    assert_eq!(read(&dir, "a.js"), "x = 9;\ny = 2;\nx = 9;\n");
}

/// インデントが違うときに、あいまいに一致させない。代わりに場所を教える。
#[test]
fn indentation_mismatch_is_reported_not_fuzzy_matched() {
    let original = "fn main() {\n    foo();\n    bar();\n}\n";
    let dir = setup(&[("main.rs", original)]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "main.rs", "old": "  foo();\n  bar();", "new": "  baz();"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains("not found"))
    .stderr(predicate::str::contains("different whitespace"))
    .stderr(predicate::str::contains("lines 2-3"))
    .stderr(predicate::str::contains("2 |     foo();"));
    assert_eq!(read(&dir, "main.rs"), original);
}

// --- 成功時の動き -------------------------------------------------------------

#[test]
fn edits_to_same_file_apply_in_order() {
    let dir = setup(&[("a.txt", "one\ntwo\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "one", "new": "ONE"},
            {"file": "a.txt", "old": "ONE\ntwo", "new": "1\n2"},
        ]}),
    )
    .success()
    .stdout("M a.txt  edits=2  at_lines=1-2\n");
    assert_eq!(read(&dir, "a.txt"), "1\n2\n");
}

#[test]
fn reported_lines_refer_to_the_final_file() {
    let dir = setup(&[("a.txt", "l1\nl2\nl3\nl4\nl5\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "l4", "new": "L4"},
            {"file": "a.txt", "old": "l2", "new": "l2a\nl2b"},
        ]}),
    )
    .success()
    .stdout("M a.txt  edits=2  at_lines=2-3,5\n");
}

#[test]
fn multiple_files_one_line_each() {
    let dir = setup(&[("a.txt", "a\n"), ("sub/b.txt", "b\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "a", "new": "A"},
            {"file": "sub/b.txt", "old": "b", "new": "B"},
        ]}),
    )
    .success()
    .stdout("M a.txt  edits=1  at_lines=1\nM sub/b.txt  edits=1  at_lines=1\n");
    assert_eq!(read(&dir, "a.txt"), "A\n");
    assert_eq!(read(&dir, "sub/b.txt"), "B\n");
}

#[test]
fn same_file_with_different_spellings_is_one_file() {
    let dir = setup(&[("a.txt", "x\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "x", "new": "y"},
            {"file": "./a.txt", "old": "y", "new": "z"},
        ]}),
    )
    .success()
    .stdout("M a.txt  edits=2  at_lines=1\n");
    assert_eq!(read(&dir, "a.txt"), "z\n");
}

/// sd は `-` で始まる文字列をオプションと解釈して失敗した。stdin 入力ならその心配はない。
#[test]
fn old_starting_with_dashes() {
    let dir = setup(&[("run.sh", "cmd --function-name foo\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "run.sh", "old": "--function-name foo", "new": "--function-name bar"}]}),
    )
    .success();
    assert_eq!(read(&dir, "run.sh"), "cmd --function-name bar\n");
}

#[test]
fn crlf_is_preserved() {
    let dir = setup(&[("a.txt", "a\r\nb\r\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "a", "new": "x"}]}),
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "x\r\nb\r\n");
}

#[test]
fn dry_run_writes_nothing() {
    let dir = setup(&[("a.txt", "hello\n")]);
    rpatch(
        dir.path(),
        &["--dry-run"],
        &json!({"edits": [{"file": "a.txt", "old": "hello", "new": "bye"}]}),
    )
    .success()
    .stdout(predicate::str::contains("M a.txt  edits=1  at_lines=1\n"))
    .stdout(predicate::str::contains("dry run"));
    assert_eq!(read(&dir, "a.txt"), "hello\n");
}

#[test]
fn diff_flag_prints_unified_diff() {
    let dir = setup(&[("a.txt", "hello\nworld\n")]);
    rpatch(
        dir.path(),
        &["--diff"],
        &json!({"edits": [{"file": "a.txt", "old": "world", "new": "there"}]}),
    )
    .success()
    .stdout(predicate::str::contains("--- a/a.txt\n+++ b/a.txt\n"))
    .stdout(predicate::str::contains("-world\n+there\n"));
}

/// 絶対パスでは diff の見出しに a/ b/ を付けない（`a//tmp/x` にしない）。
#[test]
fn diff_header_keeps_absolute_paths_as_is() {
    let dir = setup(&[("a.txt", "hello\n")]);
    let path = dir.path().join("a.txt").display().to_string();
    rpatch(
        dir.path(),
        &["--diff"],
        &json!({"edits": [{"file": path, "old": "hello", "new": "there"}]}),
    )
    .success()
    .stdout(predicate::str::contains(format!(
        "--- {path}\n+++ {path}\n"
    )));
}

/// cwd が削除されていても、入力の誤りは終了コード 2 のまま。
#[cfg(unix)]
#[test]
fn deleted_cwd_still_reports_invalid_input() {
    let dir = setup(&[]);
    let gone = dir.path().join("gone");
    fs::create_dir(&gone).unwrap();
    assert_cmd::Command::new("sh")
        .args(["-c", r#"cd "$1" && rmdir "$1" && exec "$2""#, "sh"])
        .arg(&gone)
        .arg(assert_cmd::cargo::cargo_bin!("rpatch"))
        .write_stdin("not valid input")
        .assert()
        .code(2);
}

#[cfg(unix)]
#[test]
fn permissions_are_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let dir = setup(&[("run.sh", "echo hi\n")]);
    let path = dir.path().join("run.sh");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "run.sh", "old": "hi", "new": "bye"}]}),
    )
    .success();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
}

#[cfg(unix)]
#[test]
fn symlink_is_kept_and_target_is_edited() {
    let dir = setup(&[("target.txt", "hello\n")]);
    std::os::unix::fs::symlink("target.txt", dir.path().join("link.txt")).unwrap();
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "link.txt", "old": "hello", "new": "bye"}]}),
    )
    .success();
    assert!(
        fs::symlink_metadata(dir.path().join("link.txt"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(read(&dir, "target.txt"), "bye\n");
}

// --- 失敗時の診断 -------------------------------------------------------------

#[test]
fn hint_when_an_earlier_edit_changed_the_text() {
    let dir = setup(&[("a.txt", "alpha beta\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "alpha", "new": "ALPHA"},
            {"file": "a.txt", "old": "alpha beta", "new": "x"},
        ]}),
    )
    .code(1)
    .stderr(predicate::str::contains("before edits[0]"));
    assert_eq!(read(&dir, "a.txt"), "alpha beta\n");
}

#[test]
fn hint_shows_where_a_multiline_old_diverges() {
    let original = "function f() {\n  const a = 1;\n  const b = 2;\n  return a + b;\n}\n";
    let dir = setup(&[("f.js", original)]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{
            "file": "f.js",
            "old": "  const a = 1;\n  const b = 3;\n  return a + b;",
            "new": "x",
        }]}),
    )
    .code(1)
    .stderr(predicate::str::contains("differs at line 3"))
    .stderr(predicate::str::contains("old |   const b = 3;"))
    .stderr(predicate::str::contains("3 |   const b = 2;"));
}

#[test]
fn hint_shows_closest_line_for_a_typo() {
    let dir = setup(&[(
        "a.py",
        "import os\nlet_total = compute(a, b)\nprint(total)\n",
    )]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.py", "old": "let_totl = compute(a, b)", "new": "x"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains("closest line"))
    .stderr(predicate::str::contains("2 | let_total = compute(a, b)"));
}

#[test]
fn hint_for_crlf_files() {
    let dir = setup(&[("a.txt", "one\r\ntwo\r\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "one\ntwo", "new": "x"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains("CRLF"));
}

#[test]
fn later_edits_to_a_failed_file_are_reported_as_unchecked() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "zzz", "new": "y"},
            {"file": "a.txt", "old": "a", "new": "b"},
        ]}),
    )
    .code(1)
    .stderr(predicate::str::contains("edits[1]"))
    .stderr(predicate::str::contains("not checked"));
}

#[test]
fn all_failing_files_are_reported_at_once() {
    let dir = setup(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "zzz", "new": "y"},
            {"file": "b.txt", "old": "yyy", "new": "y"},
        ]}),
    )
    .code(1)
    .stderr(predicate::str::contains("edits[0] a.txt"))
    .stderr(predicate::str::contains("edits[1] b.txt"));
}

#[cfg(unix)]
#[test]
fn read_only_file_is_an_io_error_and_nothing_is_written() {
    let dir = setup(&[("a.txt", "a\n"), ("ro.txt", "r\n")]);
    let ro = dir.path().join("ro.txt");
    let mut perms = fs::metadata(&ro).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&ro, perms).unwrap();
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [
            {"file": "a.txt", "old": "a", "new": "A"},
            {"file": "ro.txt", "old": "r", "new": "R"},
        ]}),
    )
    .code(3)
    .stderr(predicate::str::contains("read-only"));
    assert_eq!(read(&dir, "a.txt"), "a\n");
    assert_eq!(read(&dir, "ro.txt"), "r\n");
}

// --- 入力の不正 ---------------------------------------------------------------

#[test]
fn invalid_json_is_exit_2() {
    let dir = setup(&[]);
    assert_cmd::cargo_bin_cmd!("rpatch")
        .current_dir(dir.path())
        .write_stdin("{not json")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid JSON"));
}

#[test]
fn unknown_field_is_exit_2() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "a", "new": "b", "cnt": 2}]}),
    )
    .code(2)
    .stderr(predicate::str::contains("cnt"));
}

#[test]
fn empty_old_is_exit_2() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "", "new": "b"}]}),
    )
    .code(2);
}

#[test]
fn zero_count_is_exit_2() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "a", "new": "b", "count": 0}]}),
    )
    .code(2);
}

#[test]
fn empty_edits_is_exit_2() {
    let dir = setup(&[]);
    rpatch(dir.path(), &[], &json!({"edits": []})).code(2);
}

#[test]
fn unknown_flag_is_exit_2() {
    let dir = setup(&[]);
    rpatch(dir.path(), &["--frobnicate"], &json!({"edits": []})).code(2);
}

#[test]
fn old_equal_to_new_is_exit_2() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "a", "new": "a"}]}),
    )
    .code(2)
    .stderr(predicate::str::contains("identical"));
}

// --- JSON の構文エラーのヒント -------------------------------------------------

#[test]
fn json_hint_for_backslash_quote() {
    let dir = setup(&[]);
    rpatch_raw(
        dir.path(),
        &[],
        r#"{"edits":[{"file":"a.js","old":"say('it\'s')","new":"x"}]}"#,
    )
    .code(2)
    .stderr(predicate::str::contains(r#""old":"say('it\'s')""#))
    .stderr(predicate::str::contains(r"write ' as is"));
}

#[test]
fn json_hint_for_raw_newline() {
    let dir = setup(&[]);
    rpatch_raw(
        dir.path(),
        &[],
        "{\"edits\":[{\"file\":\"a.js\",\"old\":\"a\nb\",\"new\":\"x\"}]}",
    )
    .code(2)
    .stderr(predicate::str::contains(r"\n"))
    .stderr(predicate::str::contains("block format"));
}

// --- 区切り形式 ---------------------------------------------------------------

fn rpatch_raw(dir: &Path, args: &[&str], input: &str) -> Assert {
    assert_cmd::cargo_bin_cmd!("rpatch")
        .current_dir(dir)
        .args(args)
        .write_stdin(input.to_string())
        .assert()
}

#[test]
fn block_replaces_lines_without_escaping() {
    let original = r#"function help(svc, cmd) {
  const key = `${svc} ${cmd}`;
  return KEYS[key] ?? "it's \"none\"";
}
"#;
    let dir = setup(&[("app.mjs", original)]);
    rpatch_raw(
        dir.path(),
        &[],
        r#"=== app.mjs
<<<<<<< OLD
function help(svc, cmd) {
  const key = `${svc} ${cmd}`;
=======
function help(svc, cmd, sub) {
  const key = sub ? `${svc} ${cmd} ${sub}` : `${svc} ${cmd}`;
>>>>>>> NEW
"#,
    )
    .success()
    .stdout("M app.mjs  edits=1  at_lines=1-2\n");
    assert_eq!(
        read(&dir, "app.mjs"),
        r#"function help(svc, cmd, sub) {
  const key = sub ? `${svc} ${cmd} ${sub}` : `${svc} ${cmd}`;
  return KEYS[key] ?? "it's \"none\"";
}
"#
    );
}

#[test]
fn block_multiple_files_and_count() {
    let dir = setup(&[("a.txt", "x\ny\nx\n"), ("b.txt", "1\n2\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD count=2\nx\n=======\nX\n>>>>>>> NEW\n\n\
         === b.txt\n<<<<<<< OLD\n1\n=======\none\n>>>>>>> NEW\n<<<<<<< OLD\n2\n=======\ntwo\n>>>>>>> NEW\n",
    )
    .success()
    .stdout("M a.txt  edits=1  at_lines=1,3\nM b.txt  edits=2  at_lines=1-2\n");
    assert_eq!(read(&dir, "a.txt"), "X\ny\nX\n");
    assert_eq!(read(&dir, "b.txt"), "one\ntwo\n");
}

/// 行の途中に一致させると、残った行頭の空白がインデントを崩す。だから行の先頭からしか照合しない。
#[test]
fn block_matches_only_whole_lines() {
    let original = "fn f() {\n    foo();\n}\n";
    let dir = setup(&[("f.rs", original)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== f.rs\n<<<<<<< OLD\n  foo();\n=======\n  bar();\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("only as part of a line"))
    .stderr(predicate::str::contains("2 |     foo();"));
    assert_eq!(read(&dir, "f.rs"), original);
}

#[test]
fn block_with_empty_new_deletes_lines() {
    let dir = setup(&[("a.txt", "keep\ndrop1\ndrop2\nkeep\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\ndrop1\ndrop2\n=======\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "keep\nkeep\n");
}

#[test]
fn block_matches_last_line_without_newline() {
    let dir = setup(&[("a.txt", "a\nb")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\nb\n=======\nc\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "a\nc");
}

#[test]
fn block_follows_crlf_files() {
    let dir = setup(&[("a.txt", "one\r\ntwo\r\nthree\r\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\none\ntwo\n=======\n1\n2\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "1\r\n2\r\nthree\r\n");
}

#[test]
fn block_unterminated_is_exit_2() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(dir.path(), &[], "=== a.txt\n<<<<<<< OLD\na\n=======\nb\n")
        .code(2)
        .stderr(predicate::str::contains("line 2"))
        .stderr(predicate::str::contains(">>>>>>> NEW"));
}

#[test]
fn block_without_file_header_is_exit_2() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(dir.path(), &[], "<<<<<<< OLD\na\n=======\nb\n>>>>>>> NEW\n")
        .code(2)
        .stderr(predicate::str::contains("=== <file>"));
}

#[test]
fn block_stray_text_is_exit_2() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(dir.path(), &[], "=== a.txt\nhello\n")
        .code(2)
        .stderr(predicate::str::contains("line 2"));
}

#[test]
fn block_old_equal_to_new_is_exit_2() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\na\n=======\na\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("identical"));
}

// --- アンカー間の範囲置換 -----------------------------------------------------

const SECTIONS: &str = "<h1>t</h1>\n<section id=\"a\">\n  <p>a</p>\n</section>\n<section id=\"b\">\n  <p>b</p>\n</section>\n";

#[test]
fn range_replaces_from_anchor_up_to_but_not_including_to() {
    let dir = setup(&[("i.html", SECTIONS)]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{
            "file": "i.html",
            "from": "<section id=\"a\">",
            "to": "<section id=\"b\">",
            "new": "<section id=\"a\">A</section>\n",
        }]}),
    )
    .success()
    .stdout(predicate::str::contains("M i.html  edits=1  at_lines=2\n"))
    .stdout(predicate::str::contains(
        "edits[0] replaced lines 2-4 (3 lines) with 1 line",
    ));
    assert_eq!(
        read(&dir, "i.html"),
        "<h1>t</h1>\n<section id=\"a\">A</section>\n<section id=\"b\">\n  <p>b</p>\n</section>\n"
    );
}

#[test]
fn range_to_is_the_first_match_after_from() {
    let dir = setup(&[("i.html", SECTIONS)]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{
            "file": "i.html",
            "from": "<section id=\"b\">",
            "to": "</section>",
            "new": "<section id=\"b\">B",
        }]}),
    )
    .success();
    assert_eq!(
        read(&dir, "i.html"),
        "<h1>t</h1>\n<section id=\"a\">\n  <p>a</p>\n</section>\n<section id=\"b\">B</section>\n"
    );
}

#[test]
fn range_from_must_be_unique() {
    let dir = setup(&[("i.html", SECTIONS)]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "i.html", "from": "<section", "to": "</section>", "new": "x"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains(
        "\"from\": expected 1 match, found 2",
    ));
}

#[test]
fn range_to_not_found_after_from() {
    let dir = setup(&[("i.html", SECTIONS)]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "i.html", "from": "<section id=\"b\">", "to": "<h1>", "new": "x"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains("\"to\" not found after \"from\" (line 5)"))
    .stderr(predicate::str::contains("only before"));
    assert_eq!(read(&dir, "i.html"), SECTIONS);
}

#[test]
fn range_in_block_format() {
    let dir = setup(&[("i.html", SECTIONS)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== i.html\n<<<<<<< FROM\n<section id=\"a\">\n<<<<<<< TO\n<section id=\"b\">\n=======\n<section id=\"a\">A</section>\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(
        read(&dir, "i.html"),
        "<h1>t</h1>\n<section id=\"a\">A</section>\n<section id=\"b\">\n  <p>b</p>\n</section>\n"
    );
}

#[test]
fn range_rejects_count_and_old() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "from": "a", "to": "b", "new": "x", "count": 2}]}),
    )
    .code(2);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "a", "from": "a", "to": "b", "new": "x"}]}),
    )
    .code(2);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "from": "a", "new": "x"}]}),
    )
    .code(2);
}

// --- v0.2.0 の試用レビュー（docs/review-v0.2.0.md）への対応 ---------------------

/// レビューの 3.: TO が見つからないとき、似た箇所は FROM より後ろから探し、ラベルは "to" にする。
#[test]
fn range_to_not_found_looks_after_from() {
    let original = "<main>\n\n\n  <section id=\"a\">\n    <p>a</p>\n  </section>\n\n  <section id=\"b\">\n  </section>\n</main>\n";
    let dir = setup(&[("i.html", original)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== i.html\n<<<<<<< FROM\n  <section id=\"a\">\n<<<<<<< TO\n  <section id=\"c\">\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("\"to\" not found after \"from\" (line 4)"))
    .stderr(predicate::str::contains("to |   <section id=\"c\">"))
    .stderr(predicate::str::contains("8 |   <section id=\"b\">"))
    .stderr(predicate::str::contains("\"old\"").not());
}

/// レビューの 2.: 行の一部だけを書いたときは「空白違い」ではなく「行の一部」と言う。
#[test]
// v0.6.0 (#7) から、FROM / TO の最後の行は行の先頭部分だけでよくなった。
// このケース（`**Why:**` だけを書いた FROM）は、レビュー当時は失敗だったが、今は通る。
fn from_written_as_the_start_of_a_line_now_works() {
    let original = "# memo\n\n**Why:** 仕様は「速さ」ではなく正確さ\n**How:** 試す\n";
    let dir = setup(&[("m.md", original)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== m.md\n<<<<<<< FROM\n**Why:**\n<<<<<<< TO\n**How:**\n=======\nx\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "m.md"), "# memo\n\nx\n**How:** 試す\n");
}

/// レビューの 3. の改善案: 記号を長くすれば、7 個の記号の行を中身として書ける。
#[test]
fn longer_markers_allow_marker_lines_as_content() {
    let dir = setup(&[("doc.md", "# doc\nexample here\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== doc.md\n<<<<<<<<< OLD\nexample here\n=========\n<<<<<<< OLD\nold\n=======\nnew\n>>>>>>> NEW\n>>>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(
        read(&dir, "doc.md"),
        "# doc\n<<<<<<< OLD\nold\n=======\nnew\n>>>>>>> NEW\n"
    );
}

#[test]
fn longer_markers_work_for_ranges() {
    let dir = setup(&[("a.txt", "a\nb\nc\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<<< FROM\na\n<<<<<<<< TO\nc\n========\n=======\n>>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "=======\nc\n");
}

#[test]
fn marker_line_in_content_suggests_longer_markers() {
    let dir = setup(&[("doc.md", "x\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== doc.md\n<<<<<<< OLD\nx\n=======\nsee <<<<<<< FROM below\n<<<<<<< FROM\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("<<<<<<<<< OLD"))
    .stderr(predicate::str::contains("JSON"));
}

/// レビューの 1. の細かい点: NEW の部分の開始行は `=======` の行。
#[test]
fn block_error_names_the_part_that_starts_at_the_separator() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\na\n=======\nb\n<<<<<<< OLD\n",
    )
    .code(2)
    .stderr(predicate::str::contains("NEW part that starts at line 4"))
    .stderr(predicate::str::contains(
        "missing `>>>>>>> NEW` before line 6?",
    ));
}

#[test]
fn block_hint_for_missing_separator() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\na\nb\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("missing `=======` before line 5?"));
}

#[test]
fn block_hint_for_short_markers() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<< OLD\na\n=======\nb\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("at least 7"));
}

// --- 3 回目のコメント: 区切りの長さの食い違い -------------------------------------

#[test]
fn unclosed_block_points_at_markers_of_another_length() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<<<< OLD\na\n=========\nb\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("not closed"))
    .stderr(predicate::str::contains(
        "line 6 has `>>>>>>> NEW` (7 characters), but this block's markers are 9 characters long",
    ));
}

#[test]
fn mistyped_separator_length_is_named_instead_of_longer_markers() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<<<< OLD\na\n========\nb\n>>>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "line 4 has `========` (8 characters), but this block's markers are 9 characters long",
    ))
    .stderr(predicate::str::contains("<<<<<<<<<<< OLD").not());
}

// --- 4 回目のコメント: 出力の at_lines と、範囲置換で TO を重ねて書いたとき ----------

/// 前後の文脈の行を OLD と NEW の両方に書いても、実際に変わった行だけを示す。
#[test]
fn at_lines_skips_identical_context_lines() {
    let dir = setup(&[("a.txt", "1\n2\n3\n4\n5\n6\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\n2\n3\n4\n=======\n2\nTHREE\n4\n>>>>>>> NEW\n<<<<<<< OLD\n5\n6\n=======\n5\n>>>>>>> NEW\n",
    )
    .success()
    .stdout("M a.txt  edits=2  at_lines=3,5\n");
    assert_eq!(read(&dir, "a.txt"), "1\n2\nTHREE\n4\n5\n");
}

#[test]
fn at_lines_for_appending_at_the_end() {
    let dir = setup(&[("a.txt", "1\n2\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\n2\n=======\n2\n3\n4\n>>>>>>> NEW\n",
    )
    .success()
    .stdout("M a.txt  edits=1  at_lines=3-4\n");
}

/// TO は残るので、NEW の末尾にも TO を書くと 2 回出てくる。よくある書き間違いなので失敗にする。
#[test]
fn range_rejects_new_ending_with_the_kept_to() {
    let original = "mod t {\n    fn a() {}\n}\n";
    let dir = setup(&[("a.rs", original)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.rs\n<<<<<<< FROM\n    fn a() {}\n<<<<<<< TO\n}\n=======\n    fn b() {}\n}\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("would appear twice"))
    .stderr(predicate::str::contains("3 | }"));
    assert_eq!(read(&dir, "a.rs"), original);
}

// --- v0.4.1 の試用メモ（docs/review-v0.4.1.md）: BEFORE / AFTER など ---------------

const PARA: &str =
    "# plan\nThis is a very long paragraph that goes on and on, written as one line.\nnext\n";

#[test]
fn after_inserts_below_the_line_containing_the_anchor() {
    let dir = setup(&[("p.md", PARA)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== p.md\n<<<<<<< AFTER\nThis is a very long\n=======\nadded 1\nadded 2\n>>>>>>> NEW\n",
    )
    .success()
    .stdout("M p.md  edits=1  at_lines=3-4\n");
    assert_eq!(
        read(&dir, "p.md"),
        "# plan\nThis is a very long paragraph that goes on and on, written as one line.\nadded 1\nadded 2\nnext\n"
    );
}

#[test]
fn before_inserts_above_the_line_containing_the_anchor() {
    let dir = setup(&[("p.md", PARA)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== p.md\n<<<<<<< BEFORE\ngoes on and on\n=======\nadded\n>>>>>>> NEW\n",
    )
    .success()
    .stdout("M p.md  edits=1  at_lines=2\n");
    assert_eq!(
        read(&dir, "p.md"),
        "# plan\nadded\nThis is a very long paragraph that goes on and on, written as one line.\nnext\n"
    );
}

#[test]
fn insert_anchor_must_match_count_times() {
    let dir = setup(&[("a.sh", "esac\nfoo\nesac\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.sh\n<<<<<<< AFTER\nesac\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "\"after\": expected 1 match, found 2",
    ));
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.sh\n<<<<<<< AFTER count=2\nesac\n=======\nx\n>>>>>>> NEW\n",
    )
    .success()
    .stdout("M a.sh  edits=1  at_lines=2,5\n");
    assert_eq!(read(&dir, "a.sh"), "esac\nx\nfoo\nesac\nx\n");
}

#[test]
fn insert_anchor_twice_on_one_line_is_rejected() {
    let dir = setup(&[("a.txt", "a, a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< BEFORE count=2\na\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("on the same line"));
}

#[test]
fn after_last_line_without_newline() {
    let dir = setup(&[("a.txt", "a\nb")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< AFTER\nb\n=======\nc\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "a\nb\nc");
}

#[test]
fn insert_follows_crlf() {
    let dir = setup(&[("a.txt", "a\r\nb\r\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< AFTER\na\n=======\nx\ny\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "a\r\nx\r\ny\r\nb\r\n");
}

#[test]
fn insert_anchor_not_found_shows_closest_line() {
    let dir = setup(&[("p.md", PARA)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== p.md\n<<<<<<< AFTER\nThis is a very lng paragraph\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("\"after\" not found"))
    .stderr(predicate::str::contains(
        "2 | This is a very long paragraph",
    ));
}

#[test]
fn partial_line_hint_mentions_before_and_after() {
    let dir = setup(&[("p.md", PARA)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== p.md\n<<<<<<< OLD\nThis is a very long\n=======\nadded\nThis is a very long\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("only as part of a line"))
    .stderr(predicate::str::contains("BEFORE"));
}

#[test]
fn unknown_flag_prints_a_short_usage() {
    let dir = setup(&[]);
    rpatch_raw(dir.path(), &["-F"], "")
        .code(2)
        .stderr(predicate::str::contains("rpatch --help"))
        .stderr(predicate::str::contains("Block format").not());
}

// --- v0.5.0 の確認メモ（docs/review-v0.5.0.md） -------------------------------------

#[test]
fn insert_anchor_must_be_a_single_line() {
    let dir = setup(&[("a.txt", "a\nb long\nc\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< AFTER\nb long\nc\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("must be a single line"));
}

#[test]
fn insert_count_mismatch_suggests_count() {
    let dir = setup(&[("a.md", "## A\n\n## B\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.md\n<<<<<<< AFTER\n## \n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("`<<<<<<< AFTER count=2`"));
}

// --- Issue #5-#13 ------------------------------------------------------------

/// #8: 重なり合う一致も数える。曖昧なら、count に関係なく失敗にする。
#[test]
fn overlapping_matches_are_ambiguous_block() {
    let dir = setup(&[("ov.txt", "x\nx\nx\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== ov.txt\n<<<<<<< OLD\nx\nx\n=======\ny\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("found 2"))
    .stderr(predicate::str::contains("overlap"));
    assert_eq!(read(&dir, "ov.txt"), "x\nx\nx\n");
}

#[test]
fn overlapping_matches_are_ambiguous_json() {
    let dir = setup(&[("a.txt", "aaa\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "aa", "new": "b"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains("overlap"));
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "aa", "new": "b", "count": 2}]}),
    )
    .code(1)
    .stderr(predicate::str::contains("overlap"));
}

#[test]
fn overlapping_from_is_ambiguous() {
    let dir = setup(&[("a.txt", "x\nx\nx\nend\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< FROM\nx\nx\n<<<<<<< TO\nend\n=======\ny\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("overlap"));
}

/// #6: 複数行の OLD の最後の行だけが行の一部なら、その行を指す。
#[test]
fn partial_last_line_of_multiline_old_is_named() {
    let original = "function render() {\n  if (!data) return;\n  // Do not redraw under someone typing a verdict (the next poll)\n  draw();\n}\n";
    let dir = setup(&[("r.js", original)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== r.js\n<<<<<<< OLD\nfunction render() {\n  if (!data) return;\n  // Do not redraw under someone typing a verdict\n=======\nX\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("OLD line 3 is only the start of line 3"))
    .stderr(predicate::str::contains(
        "3 |   // Do not redraw under someone typing a verdict (the next poll)",
    ));
}

/// #16: 行の一部としてだけ見つかったら、長い行でも切らずに出す（そのまま OLD にコピーできるように）。
#[test]
fn partial_line_shows_long_lines_whole() {
    let long = format!("- {} end of the line", "長い説明が続く。".repeat(60));
    let dir = setup(&[("a.md", &format!("{long}\nnext\n"))]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.md\n<<<<<<< OLD\n- 長い説明が続く。\n=======\nX\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "OLD line 1 is only the start of line 1",
    ))
    .stderr(predicate::str::contains(format!("1 | {long}\n")));
    assert_eq!(read(&dir, "a.md"), format!("{long}\nnext\n"));
}

/// 極端に長い行（4000 文字超）だけは切る。
#[test]
fn partial_line_cuts_only_extreme_lines() {
    let long = format!("start {}", "x".repeat(5000));
    let dir = setup(&[("a.js", &format!("{long}\n"))]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.js\n<<<<<<< OLD\nstart\n=======\nX\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(format!(
        "1 | {}…\n",
        &long[..4000]
    )));
}

#[test]
fn partial_first_line_names_missing_leading_text() {
    let dir = setup(&[("f.rs", "fn f() {\n    foo();\n}\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== f.rs\n<<<<<<< OLD\n  foo();\n=======\n  bar();\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "OLD line 1 is only the end of line 2",
    ));
}

/// #9: 区切りの行の末尾の空白は無視する。
#[test]
fn markers_with_trailing_whitespace_are_recognized() {
    let dir = setup(&[("a.mjs", "  const a = 1;\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.mjs\n<<<<<<< OLD \n  const a = 1;\n======= \n  const a = 2;\n>>>>>>> NEW\t\n",
    )
    .success();
    assert_eq!(read(&dir, "a.mjs"), "  const a = 2;\n");
}

/// #10: 一番似ている行の表示で、入力側の行も空白付きのまま出す。
#[test]
fn closest_line_shows_input_with_its_indentation() {
    let dir = setup(&[("a.mjs", "function f(a, b) {\n  return a + b;\n}\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.mjs\n<<<<<<< OLD\n  retrun a + b;\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("old |   retrun a + b;"));
}

/// #11: BOM は読むときに外し、書くときに付け直す。
#[test]
fn utf8_bom_is_kept_and_does_not_block_the_first_line() {
    let dir = setup(&[("bom.txt", "\u{feff}bom line\nnext\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== bom.txt\n<<<<<<< OLD\nbom line\n=======\nBOM LINE\n>>>>>>> NEW\n",
    )
    .success()
    .stdout("M bom.txt  edits=1  at_lines=1\n");
    assert_eq!(read(&dir, "bom.txt"), "\u{feff}BOM LINE\nnext\n");
}

/// #7: FROM / TO の最後の行は、行の先頭部分だけでもよい。照合は行の先頭から。
#[test]
fn from_and_to_may_be_the_start_of_a_line() {
    let original = "# runbook\n- 1問は「準備 → 作戦 → 実行」で進む。とても長い行がここに続く。\nold detail\n## 次の節 とても長い見出し\nkeep\n";
    let dir = setup(&[("d.md", original)]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== d.md\n<<<<<<< FROM\n- 1問は「準備\n<<<<<<< TO\n## 次の節\n=======\n- new text\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("replaced lines 2-3 (2 lines) with 1 line"));
    assert_eq!(
        read(&dir, "d.md"),
        "# runbook\n- new text\n## 次の節 とても長い見出し\nkeep\n"
    );
}

#[test]
fn from_must_still_start_at_a_line_start() {
    let dir = setup(&[("f.rs", "fn f() {\n    foo();\n}\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== f.rs\n<<<<<<< FROM\nfoo();\n<<<<<<< TO\n}\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("\"from\" not found"));
}

#[test]
fn new_ending_with_the_kept_to_line_is_rejected_for_prefix_to() {
    let dir = setup(&[("d.md", "## A\nold\n## B title\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== d.md\n<<<<<<< FROM\n## A\n<<<<<<< TO\n## B\n=======\n## A\nnew\n## B title\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("would appear twice"));
}

/// #5: 区切り形式では、count のヒントも区切り形式の書き方にする。
#[test]
fn block_count_hint_uses_block_syntax() {
    let dir = setup(&[("a.txt", "x\ny\nx\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\nx\n=======\nz\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("`<<<<<<< OLD count=2`"));
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "x", "new": "z"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains("\"count\": 2"));
}

/// #13: ハードリンクは切れるので、警告する。
#[cfg(unix)]
#[test]
fn hard_link_warning() {
    let dir = setup(&[("app.mjs", "const a = 1;\n")]);
    fs::hard_link(dir.path().join("app.mjs"), dir.path().join("hard.mjs")).unwrap();
    rpatch_raw(
        dir.path(),
        &[],
        "=== app.mjs\n<<<<<<< OLD\nconst a = 1;\n=======\nconst a = 10;\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains(
        "warning: app.mjs had 2 hard links",
    ));
}

/// mcp feature なしのビルドでは `rpatch mcp` は使えない。stdin の編集として処理せず、終了コード 2 で知らせる。
#[cfg(not(feature = "mcp"))]
#[test]
fn mcp_subcommand_without_the_feature_is_a_usage_error() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &["mcp"],
        "=== a.txt\n<<<<<<< OLD\na\n=======\nb\n>>>>>>> NEW\n",
    )
    .code(2)
    .stdout("")
    .stderr(predicate::str::contains("built without MCP support"))
    .stderr(predicate::str::contains("--features mcp"));
    assert_eq!(read(&dir, "a.txt"), "a\n");
}
