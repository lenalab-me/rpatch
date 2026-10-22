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

// --- v0.2.0 の試用レビューへの対応 ---------------------

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

// --- 区切りの長さの食い違い -------------------------------------

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
/// TO の行を書く前に入力が終わったら、そう伝え、前の完成したブロックも書かれていないと示す。
fn input_ending_right_after_to_says_so_and_that_nothing_was_written() {
    let dir = setup(&[("a.txt", "a\nb\nc\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n<<<<<<< FROM\nb\n<<<<<<< TO\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "the input ends right after the TO marker at line 9",
    ))
    .stderr(predicate::str::contains("the block before line 7 parsed"))
    .stderr(predicate::str::contains("rpatch: nothing written"));
    assert_eq!(read(&dir, "a.txt"), "a\nb\nc\n");
}

/// `rpatch <<'A' && cat > f <<'B'` で B の本文を先に書くと、rpatch はファイルの中身を読む。
#[test]
fn file_content_as_input_hints_at_heredoc_order() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(dir.path(), &[], "// a comment\nfn f() {}\n")
        .code(2)
        .stderr(predicate::str::contains(
            "the body for rpatch must come first",
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

// --- 閉じ行の書き間違い -----------------------------------------

/// 開きに合わせて閉じにも `part` や `count=N` を書いても、閉じとして読む。
#[test]
fn closer_may_repeat_the_openers_part_and_count() {
    let dir = setup(&[("a.md", "pic が 0 と出る。\nx\nx\nkeep y\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.md\n<<<<<<< OLD part\n0\n=======\n2\n>>>>>>> NEW part\n<<<<<<< OLD count=2\nx\n=======\nX\n>>>>>>> NEW count=2\n<<<<<<< OLD part count=1\ny\n=======\nY\n>>>>>>> NEW part\n",
    )
    .success()
    .stdout(predicate::str::contains("edits=3"));
    assert_eq!(read(&dir, "a.md"), "pic が 2 と出る。\nX\nX\nkeep Y\n");
}

/// 長い区切りでも同じ。中の 7 個の `>>>>>>> NEW part` はただの本文のまま。
#[test]
fn longer_closer_may_repeat_the_openers_part() {
    let dir = setup(&[("a.md", "see here\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.md\n<<<<<<<<< OLD part\nhere\n=========\nthis:\n>>>>>>> NEW part\n>>>>>>>>> NEW part\n",
    )
    .success();
    assert_eq!(read(&dir, "a.md"), "see this:\n>>>>>>> NEW part\n");
}

/// 開きと食い違う語は、開きと閉じのどちらを信じるかで当てる箇所が変わるので、受け付けない。
#[test]
fn closer_with_a_count_the_opener_lacks_is_refused_with_the_fixed_opener() {
    let dir = setup(&[("a.txt", "x\nx\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\nx\n=======\n>>>>>>> NEW count=2\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "line 5: the closing line `>>>>>>> NEW count=2` does not match the opener `<<<<<<< OLD` at line 2",
    ))
    .stderr(predicate::str::contains(
        "open it with `<<<<<<< OLD count=2` if that is meant, and close it with `>>>>>>> NEW`",
    ))
    .stderr(predicate::str::contains("nothing written"));
    assert_eq!(read(&dir, "a.txt"), "x\nx\n");
}

#[test]
fn closer_with_part_the_opener_lacks_is_refused_with_the_fixed_opener() {
    let dir = setup(&[("a.txt", "a b\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD count=2\nb\n=======\nB\n>>>>>>> NEW part\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "open it with `<<<<<<< OLD part count=2` if that is meant",
    ));
    assert_eq!(read(&dir, "a.txt"), "a b\n");
}

#[test]
fn closer_with_part_after_an_insert_says_part_is_only_for_old() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< AFTER\na\n=======\nb\n>>>>>>> NEW part\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "`part` is only for `<<<<<<< OLD part`; close the AFTER block with `>>>>>>> NEW`",
    ));
}

/// NEW の後ろのほかの語（Git の衝突の印 `>>>>>>> main` など）は、今までどおり閉じとは読まない。
#[test]
fn closer_with_other_words_is_still_refused() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\na\n=======\nb\n>>>>>>> NEW done\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "unexpected `>>>>>>> NEW done` inside the NEW part",
    ));
    assert_eq!(read(&dir, "a.txt"), "a\n");
}

/// 9 個で開いて 7 個の `>>>>>>> NEW part` で閉じたら、長さの違いを指す。
#[test]
fn closer_with_words_of_another_length_is_pointed_at() {
    let dir = setup(&[("a.txt", "a b\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<<<< OLD part\nb\n=========\nB\n>>>>>>> NEW part\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "line 6 has `>>>>>>> NEW part` (7 characters), but this block's markers are 9 characters long",
    ));
}

/// 閉じの代わりに `=======` を書いて入力が終わった。NEW の本文に `=======`
/// （Markdown の見出しの下線）があり得るので受け付けず、閉じの書き方を示す。
#[test]
fn separator_in_place_of_the_closer_at_the_end_says_so() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\na\n=======\nb\n=======\n\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "a block ends with `>>>>>>> NEW`, not `=======`; write it in place of line 6?",
    ))
    .stderr(predicate::str::contains("missing `>>>>>>> NEW`").not());
}

#[test]
fn second_separator_right_before_the_closer_says_to_remove_it() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\na\n=======\nb\n=======\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "the NEW part already started at line 4; remove the second `=======` at line 6?",
    ));
}

// --- 出力の at_lines と、範囲置換で TO を重ねて書いたとき ----------

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

/// TO の行が空なら、FROM からファイルの最後までを置き換える（末尾を消すのに `head -n` へ戻っていた）。
#[test]
fn empty_to_runs_to_the_end_of_the_file() {
    let dir = setup(&[("a.go", "package a\n\nfunc a() {}\n\n// old\nfunc b() {}\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.go\n<<<<<<< FROM\n\n// old\n<<<<<<< TO\n=======\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains(
        "replaced lines 4-6 (3 lines) with 0 lines",
    ));
    assert_eq!(read(&dir, "a.go"), "package a\n\nfunc a() {}\n");
}

/// 最後の行に改行がないファイルは、置き換えたあとも改行なしで終わる。
#[test]
fn empty_to_keeps_a_file_without_a_final_line_break() {
    let dir = setup(&[("a.txt", "a\nb\nc")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< FROM\nb\n<<<<<<< TO\n=======\nB\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "a\nB");
}

/// JSON の "to" が空でも、ファイルの最後まで。
#[test]
fn empty_json_to_runs_to_the_end_of_the_file() {
    let dir = setup(&[("a.txt", "a\nb\nc\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "from": "b\n", "to": "", "new": ""}]}),
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "a\n");
}

/// FROM のあとに TO を書かずに `=======` が来たら、OLD を勧め、FROM の順番を示す。
#[test]
fn separator_in_from_suggests_old_and_shows_the_order() {
    let dir = setup(&[("a.txt", "a\nb\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< FROM\na\n=======\nA\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "open the block with `<<<<<<< OLD` instead",
    ))
    .stderr(predicate::str::contains(
        "TO lines (none: to the end of the file)",
    ));
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

// --- v0.4.1 の試用メモ: BEFORE / AFTER など ---------------

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

#[test]
fn file_argument_names_the_file_for_blocks_before_any_header() {
    let dir = setup(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n=== b.txt\n<<<<<<< OLD\nb\n=======\nB\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("M a.txt"));
    assert_eq!(read(&dir, "a.txt"), "A\n");
    assert_eq!(read(&dir, "b.txt"), "B\n");
}

#[test]
fn file_argument_takes_one_file_and_only_the_block_format() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(dir.path(), &["a.txt", "b.txt"], "")
        .code(2)
        .stderr(predicate::str::contains("at most one file"));
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        r#"{"edits": [{"file": "a.txt", "old": "a", "new": "A"}]}"#,
    )
    .code(2)
    .stderr(predicate::str::contains("only with the block format"));
    assert_eq!(read(&dir, "a.txt"), "a\n");
}

/// `=== ` を付け忘れたパスだけの行や、diff 風の `--- path` も、すぐ次がブロックの開始行ならファイル行として読む。
/// ヒントを出して止めると、エージェントは rpatch に戻らず別の手段に移る。
#[test]
fn file_line_without_its_prefix_is_read_as_one() {
    // Seen from Codex and Claude Code: `config.js` or `--- config.js` instead of `=== config.js`.
    let dir = setup(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "a.txt\n<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n--- b.txt\n<<<<<<< OLD\nb\n=======\nB\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("M a.txt"))
    .stdout(predicate::str::contains("M b.txt"));
    assert_eq!(read(&dir, "a.txt"), "A\n");
    assert_eq!(read(&dir, "b.txt"), "B\n");
    // 次の行が開始行でなければ、これまでどおり誤り。
    rpatch_raw(
        dir.path(),
        &[],
        "a.txt\nA\n<<<<<<< OLD\nA\n=======\na\n>>>>>>> NEW\n",
    )
    .code(2);
}

// --- v0.5.0 の確認メモ -------------------------------------

#[test]
fn insert_anchor_may_span_lines() {
    // BEFORE goes before the line where the match starts, AFTER after the line where it ends.
    let dir = setup(&[("a.txt", "f() {\n  one\n}\nf() {\n  two\n}\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< AFTER\nf() {\n  two\n=======\n  added\n>>>>>>> NEW\n<<<<<<< BEFORE\n  one\n}\n=======\n  // first\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(
        read(&dir, "a.txt"),
        "f() {\n  // first\n  one\n}\nf() {\n  two\n  added\n}\n"
    );
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
    .stderr(predicate::str::contains(
        "2 matches found, but they overlap (lines 1-2 and 2-3)",
    ))
    .stderr(predicate::str::contains("add a nearby line"));
    assert_eq!(read(&dir, "ov.txt"), "x\nx\nx\n");
}

/// 最後の行に改行がなくても、その行と重なる一致を数える。前は黙って 1 つ目に当たっていた。
#[test]
fn overlap_with_a_last_line_without_newline_is_ambiguous() {
    let dir = setup(&[("ov.txt", "x\nx\nx")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== ov.txt\n<<<<<<< OLD\nx\nx\n=======\ny\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "2 matches found, but they overlap (lines 1-2 and 2-3)",
    ));
    assert_eq!(read(&dir, "ov.txt"), "x\nx\nx");
}

/// #15: count が一致数と同じでも「expected 2, found 2」とは言わない。重なりを見出しにする。
#[test]
fn overlap_with_matching_count_names_the_overlap() {
    let dir = setup(&[("ov.txt", "x\nx\nx\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== ov.txt\n<<<<<<< OLD count=2\nx\nx\n=======\ny\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "2 matches found, but they overlap",
    ))
    .stderr(predicate::str::contains("expected").not());
}

/// #15: 行の中の重なりは列で示し、ヒントは「前後の文字を足す」。
#[test]
fn overlap_within_a_line_shows_columns() {
    let dir = setup(&[("a.txt", "aaa\n")]);
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "aa", "new": "b"}]}),
    )
    .code(1)
    .stderr(predicate::str::contains(
        "2 matches found, but they overlap (line 1, columns 1-2 and 2-3)",
    ))
    .stderr(predicate::str::contains(
        "add the characters before or after it",
    ))
    .stderr(predicate::str::contains("nearby line").not());
}

/// #14: 行の途中にしかない OLD は「行の途中」と 1 回だけ言う。
#[test]
fn old_in_the_middle_of_a_line_is_named_once() {
    let dir = setup(&[("mid.txt", "aa Approval Gate bb\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== mid.txt\n<<<<<<< OLD\nApproval Gate\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "OLD line 1 is in the middle of line 1 (text before and after it on that line is missing)",
    ))
    .stderr(predicate::str::contains("only the end").not())
    .stderr(predicate::str::contains("only the start").not());
}

/// #14: FROM でも、前にも後ろにも続きがあれば「行の途中」と言う。
#[test]
fn from_in_the_middle_of_a_line_is_not_called_the_end() {
    let dir = setup(&[("mid.txt", "aa Approval Gate bb\nend\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== mid.txt\n<<<<<<< FROM\nApproval Gate\n<<<<<<< TO\nend\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "FROM line 1 is in the middle of line 1",
    ))
    .stderr(predicate::str::contains("only the end").not());
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

// --- #2: OLD part（行の一部を書き換える） -------------------------------------

/// 行の途中の文字列だけを書き換え、行の残りは残す。
#[test]
fn old_part_replaces_text_inside_a_line() {
    let dir = setup(&[(
        "a.sh",
        "echo 練習のときだけ: JAM_MOCK_PROFILE=prod && run\nnext\n",
    )]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.sh\n<<<<<<< OLD part\n練習のときだけ: JAM_MOCK_PROFILE=prod\n=======\n練習のときだけ: JAM_MOCK_LANG=ja JAM_MOCK_PROFILE=prod\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("at_lines=1"));
    assert_eq!(
        read(&dir, "a.sh"),
        "echo 練習のときだけ: JAM_MOCK_LANG=ja JAM_MOCK_PROFILE=prod && run\nnext\n"
    );
}

/// 複数行にまたがる部分（1 行目の末尾から 2 行目の先頭まで）も書ける。
#[test]
fn old_part_spans_lines() {
    let dir = setup(&[(
        "a.js",
        "  }).join(\"\") + `\n    ${C ? `<div id=\"panel\">` : \"\"}\n",
    )]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.js\n<<<<<<< OLD part\n.join(\"\") + `\n    ${C ? `<div id=\"panel\">\n=======\n.join(\"\") + `</div>\n    ${C ? `<div id=\"panel\" class=\"d\">\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(
        read(&dir, "a.js"),
        "  }).join(\"\") + `</div>\n    ${C ? `<div id=\"panel\" class=\"d\">` : \"\"}\n"
    );
}

/// CRLF のファイルでも、OLD part の改行はファイルに合わせる。
#[test]
fn old_part_follows_crlf() {
    let dir = setup(&[("a.txt", "one two\r\nthree four\r\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD part\ntwo\nthree\n=======\n2\n3\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "one 2\r\n3 four\r\n");
}

/// OLD part も、ちょうど count 回でなければ失敗にする。ヒントは OLD part の書き方で出す。
#[test]
fn old_part_count_hint_uses_old_part() {
    let dir = setup(&[("a.txt", "x = 1; y = 1;\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD part\n= 1\n=======\n= 2\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("expected 1 match, found 2"))
    .stderr(predicate::str::contains("<<<<<<< OLD part count=2"));
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD part count=2\n= 1\n=======\n= 2\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "x = 2; y = 2;\n");
}

/// 行の一部としてだけ見つかったときのヒントで、OLD part を案内する。
#[test]
fn partial_line_hint_suggests_old_part() {
    let dir = setup(&[("a.txt", "aa Approval Gate bb\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\nApproval Gate\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("<<<<<<< OLD part"));
}

/// OLD part でも、空や NEW と同じものは入力の誤り。
#[test]
fn old_part_rejects_empty_and_identical() {
    let dir = setup(&[("a.txt", "abc\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD part\n=======\nx\n>>>>>>> NEW\n",
    )
    .code(2);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD part\nb\n=======\nb\n>>>>>>> NEW\n",
    )
    .code(2);
}

/// 自己流の diff 形式で書かれたら、diff ではないと伝え、正しい書き方の例を示す。
#[test]
fn guessed_diff_format_gets_the_real_format() {
    let dir = setup(&[("a.html", "let x = 1;\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.html\n--- old\nlet x = 1;\n+++ new\nlet x = 2;\n",
    )
    .code(2)
    .stderr(predicate::str::contains("`OLD part`"))
    .stderr(predicate::str::contains(
        "this is not a diff or patch format",
    ))
    .stderr(predicate::str::contains(
        "    <<<<<<< OLD\n    exact lines to replace\n    =======\n",
    ));
    assert_eq!(read(&dir, "a.html"), "let x = 1;\n");
}

// --- 空行で終わる FROM/TO/アンカー、改行の混在、表面の細部 ----

/// TO が空行 1 行なら、次の空行の手前まで。以前は「TO の行なし」と同じになり、後ろの行を黙って消していた。
#[test]
fn blank_to_stops_at_the_next_blank_line() {
    let dir = setup(&[("a.md", "head\nstart\nbody1\nbody2\n\nkeep1\nkeep2\n")]);
    rpatch_raw(
        dir.path(),
        &["a.md"],
        "<<<<<<< FROM\nstart\n<<<<<<< TO\n\n=======\nNEW\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("replaced lines 2-4 (3 lines)"));
    assert_eq!(read(&dir, "a.md"), "head\nNEW\n\nkeep1\nkeep2\n");
}

/// 空行だけの FROM は空行に一致する（以前は panic で終了コード 101）。
#[test]
fn blank_from_matches_a_blank_line() {
    let dir = setup(&[("a.txt", "a\n\nb\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< FROM\n\n<<<<<<< TO\nb\n=======\nX\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "a\nX\nb\n");
}

/// FROM の最後の空行は、本当にある空行にだけ一致する。直後の行を飲み込まない。
#[test]
fn from_ending_in_a_blank_line_needs_that_blank_line() {
    let dir = setup(&[("a.txt", "start\nend\nx\nend\nz\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< FROM\nstart\n\n<<<<<<< TO\nend\n=======\nX\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "found without the blank line at its end, at line 1",
    ));
    assert_eq!(read(&dir, "a.txt"), "start\nend\nx\nend\nz\n");

    let dir = setup(&[("a.txt", "start\n\nbody\nend\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< FROM\nstart\n\n<<<<<<< TO\nend\n=======\nX\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "X\nend\n");
}

/// AFTER のアンカーの最後の空行も同じ。ないなら挿入せず、あるならその空行の後ろに入れる。
#[test]
fn anchor_ending_in_a_blank_line_needs_that_blank_line() {
    let dir = setup(&[("a.txt", "foo\nbar\nbaz\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< AFTER\nfoo\n\n=======\nINS\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "a blank last line in AFTER matches only a blank line",
    ));
    assert_eq!(read(&dir, "a.txt"), "foo\nbar\nbaz\n");

    let dir = setup(&[("a.txt", "foo\n\nbar\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< AFTER\nfoo\n\n=======\nINS\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "foo\n\nINS\nbar\n");
}

/// NEW の末尾と TO の重なりは行で比べる。TO `}` と NEW の最後の `    }` は別の行。
#[test]
fn range_new_ending_in_an_indented_to_line_is_fine() {
    let dir = setup(&[(
        "a.rs",
        "fn f() {\n    if x {\n        y();\n    }\n}\nrest\n",
    )]);
    rpatch_raw(
        dir.path(),
        &["a.rs"],
        "<<<<<<< FROM\nfn f() {\n<<<<<<< TO\n}\n=======\nfn f() {\n    if z {\n        w();\n    }\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(
        read(&dir, "a.rs"),
        "fn f() {\n    if z {\n        w();\n    }\n}\nrest\n"
    );
}

/// LF と CRLF が混ざったファイルは、区切り形式では専用のエラーにする。
#[test]
fn mixed_line_endings_are_refused_with_their_own_error() {
    let original = "a\nb\r\nc\n";
    let dir = setup(&[("a.txt", original)]);
    for block in [
        "<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n",
        "<<<<<<< AFTER\na\n=======\nINS\n>>>>>>> NEW\n",
    ] {
        rpatch_raw(dir.path(), &["a.txt"], block)
            .code(1)
            .stderr(predicate::str::contains(
                "the file mixes LF and CRLF line endings",
            ))
            .stderr(predicate::str::contains(
                "line 1 ends with LF, line 2 with CRLF",
            ));
    }
    assert_eq!(read(&dir, "a.txt"), original);
    // JSON は書いたとおりに照合する。
    rpatch(
        dir.path(),
        &[],
        &json!({"edits": [{"file": "a.txt", "old": "a\n", "new": "A\n"}]}),
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "A\nb\r\nc\n");
}

/// count より一致が少ないとき、「1 つに絞れ」とは言わない。
#[test]
fn count_hint_when_fewer_matches_than_count() {
    let dir = setup(&[("a.txt", "x\nx\ny\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< OLD count=3\nx\n=======\nz\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "open the block with `<<<<<<< OLD count=2` if that is intended",
    ))
    .stderr(predicate::str::contains("to pick one").not());
}

/// stdin の先頭の BOM（Windows の PowerShell）は外して読む。
#[test]
fn bom_on_stdin_is_ignored() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "\u{feff}=== a.txt\n<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "a.txt"), "A\n");
}

/// ブロックの外のコメントや説明文には、ファイル行や heredoc の見当違いのヒントを出さない。
#[test]
fn text_outside_blocks_gets_its_own_hint() {
    let dir = setup(&[("a.txt", "a\n")]);
    for input in [
        "# replace a\n<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n",
        "Here you go:\n=== a.txt\n<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n",
    ] {
        rpatch_raw(dir.path(), &["a.txt"], input)
            .code(2)
            .stderr(predicate::str::contains(
                "outside blocks, only `=== <file>` lines and blank lines are allowed",
            ))
            .stderr(predicate::str::contains("write `===").not())
            .stderr(predicate::str::contains("heredocs").not());
    }
}

#[test]
fn malformed_counts_and_empty_file_lines_are_named() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD count=x\na\n=======\nA\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("`count=` needs a whole number"));
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< FROM count=2\na\n<<<<<<< TO\n=======\nA\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("FROM takes no count"));
    rpatch_raw(
        dir.path(),
        &[],
        "=== \n<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("missing file name after `===`"));
}

/// ブロック形式のエラーは、JSON のキー名ではなくブロックの名前で言う。
#[test]
fn block_errors_name_block_parts() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< OLD\na\n=======\na\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("OLD and NEW are identical"));
}

/// 引数のファイルが使われないなら、黙って無視せず入力の誤りにする。
#[test]
fn unused_file_argument_is_an_error() {
    let dir = setup(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "=== b.txt\n<<<<<<< OLD\nb\n=======\nB\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains(
        "the file argument `a.txt` is not used",
    ));
    assert_eq!(read(&dir, "b.txt"), "b\n");
    // 同じファイルを両方に書くのは構わない。
    rpatch_raw(
        dir.path(),
        &["b.txt"],
        "=== b.txt\n<<<<<<< OLD\nb\n=======\nB\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "b.txt"), "B\n");
}

/// dry run でも読み取り専用を確かめる。本番で失敗するものを成功と言わない。
#[cfg(unix)]
#[test]
fn dry_run_fails_on_a_read_only_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = setup(&[("a.txt", "a\n")]);
    fs::set_permissions(dir.path().join("a.txt"), fs::Permissions::from_mode(0o444)).unwrap();
    rpatch_raw(
        dir.path(),
        &["-n", "a.txt"],
        "<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n",
    )
    .code(3)
    .stderr(predicate::str::contains("file is read-only"));
}

/// 編集が打ち消し合ったファイルは `M` と言わず、書かない。
#[test]
fn edits_that_cancel_out_are_reported_as_unchanged() {
    let dir = setup(&[("a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &["a.txt"],
        "<<<<<<< OLD\na\n=======\nZ\n>>>>>>> NEW\n<<<<<<< OLD\nZ\n=======\na\n>>>>>>> NEW\n",
    )
    .success()
    .stdout("= a.txt  edits=2  unchanged\n");
}

/// 書き込んだあとで読み手が先に終わっても（`| head`）、失敗にしない。以前は panic で 101。
#[test]
fn closed_stdout_after_writing_is_not_a_failure() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = setup(&[("a.txt", "a\n")]);
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("rpatch"))
        .current_dir(dir.path())
        .args(["--diff", "a.txt"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n")
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert_eq!(read(&dir, "a.txt"), "A\n");
}

#[test]
fn double_dash_allows_a_file_starting_with_a_dash() {
    let dir = setup(&[("-a.txt", "a\n")]);
    rpatch_raw(
        dir.path(),
        &["--", "-a.txt"],
        "<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n",
    )
    .success();
    assert_eq!(read(&dir, "-a.txt"), "A\n");
}

const HELP_TXT: &str = include_str!("../src/help.txt");

/// `rpatch --help` の標準出力と終了コード。
fn help(args: &[&str]) -> (Option<i32>, String, String) {
    let o = assert_cmd::cargo_bin_cmd!("rpatch")
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

/// help.txt の見出し: 要約のあとの、字下げのない行。
fn help_headings() -> Vec<&'static str> {
    let lines: Vec<&str> = HELP_TXT.lines().collect();
    let exit = lines
        .iter()
        .position(|l| l.starts_with("Exit status:"))
        .unwrap();
    let blank = exit + lines[exit..].iter().position(|l| l.is_empty()).unwrap();
    let mut out = Vec::new();
    for i in blank + 1..lines.len() {
        if !lines[i].is_empty() && !lines[i].starts_with(' ') {
            assert!(
                lines[i - 1].is_empty(),
                "help.txt: `{}` is not indented but follows a line; a heading needs a blank line before it",
                lines[i]
            );
            out.push(lines[i]);
        }
    }
    out
}

/// エージェントは `rpatch --help | head -40` で読む。`--help` は書き方から終了コードまでの要約と
/// 詳細の引き方で、40 行に収まる。
#[test]
fn the_summary_of_the_help_fits_in_forty_lines() {
    for args in [&["--help"][..], &["-h"], &["--bogus", "--help"]] {
        let (code, out, _) = help(args);
        assert_eq!(code, Some(0));
        let n = out.lines().count();
        assert!(
            n <= 40,
            "`rpatch {args:?}` prints {n} lines; `head -40` cuts off:\n{out}"
        );
        assert!(
            out.contains("Exit status:")
                && out.contains("3 I/O error.")
                && out.contains("<<<<<<< OLD part")
                && out.contains("rpatch --help all"),
            "{out}"
        );
        assert!(!out.contains("\nBlocks:\n"), "{out}");
    }
}

/// 読む側は `head -40` より短く切ることもある（`head -30`・`head -25`・`head -3`）。どこで切られても
/// 使えるように、編集に要るものを上から並べる: 2 行目に詳細の引き方、20 行目までに書き方と、編集を
/// 間違えないための注意（`&&` は開きの行に、EOF の後ろの行は失敗しても走る、全部合うまで書かない）。
#[test]
fn the_first_lines_of_the_help_carry_what_an_edit_needs() {
    let (_, out, _) = help(&["--help"]);
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines[1].starts_with("Details: rpatch --help TOPIC"),
        "line 2 should say where the details are:\n{out}"
    );
    let top = lines[..20].join("\n");
    for needed in [
        "<<'EOF' && git diff --stat",
        "Quote the heredoc delimiter",
        "after the closing EOF runs even when the edit failed",
        "Nothing is written unless every edit matches",
    ] {
        assert!(
            top.contains(needed),
            "`{needed}` is not in the first 20 lines of `rpatch --help`:\n{out}"
        );
    }
}

#[test]
fn help_for_a_topic_has_its_section_and_not_the_others() {
    let (code, out, _) = help(&["--help", "rules"]);
    assert_eq!(code, Some(0));
    assert!(out.starts_with("Rules:\n"), "{out}");
    assert!(!out.contains("Blocks follow"), "{out}");
    assert_eq!(help(&["-h", "rules"]), (Some(0), out, String::new()));
}

#[test]
fn help_all_is_the_whole_of_help_txt() {
    let (code, out, _) = help(&["--help", "all"]);
    assert_eq!(code, Some(0));
    assert_eq!(out, HELP_TXT);
}

#[test]
fn help_for_an_unknown_topic_says_so_and_lists_the_names() {
    let (code, out, err) = help(&["--help", "foo"]);
    assert_eq!(code, Some(2));
    assert!(out.is_empty());
    let first = err.lines().next().unwrap();
    assert!(
        first.contains("unknown") && first.contains("`foo`") && first.contains("blocks"),
        "{err}"
    );
}

/// A section of help.txt that no name asks for would be written but never read.
#[test]
fn every_section_of_help_txt_is_asked_for_by_some_name() {
    let (_, _, err) = help(&["--help", "foo"]);
    let list = err.lines().next().unwrap().split("try: ").nth(1).unwrap();
    let names: Vec<&str> = list.split(", ").filter(|n| *n != "all").collect();
    assert!(names.len() >= 4, "{names:?}");
    let mut shown = String::new();
    for name in names {
        let (code, out, _) = help(&["--help", name]);
        assert_eq!(code, Some(0), "{name}");
        shown.push_str(&out);
    }
    let headings = help_headings();
    assert!(headings.len() >= 4, "{headings:?}");
    for h in headings {
        assert!(
            shown.lines().any(|l| l == h),
            "no topic of `rpatch --help` prints the section `{h}`"
        );
    }
}

#[test]
fn version() {
    let dir = setup(&[]);
    rpatch_raw(dir.path(), &["-V"], "")
        .success()
        .stdout(predicate::str::starts_with("rpatch "));
}

#[test]
fn invalid_utf8_stdin_says_nothing_written() {
    let dir = setup(&[("a.txt", "a\n")]);
    assert_cmd::cargo_bin_cmd!("rpatch")
        .current_dir(dir.path())
        .arg("a.txt")
        .write_stdin(vec![0xff, b'\n'])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("rpatch: nothing written"));
}

#[cfg(unix)]
#[test]
fn invalid_utf8_argument_is_an_input_error() {
    use std::os::unix::ffi::OsStrExt;
    let dir = setup(&[]);
    assert_cmd::cargo_bin_cmd!("rpatch")
        .current_dir(dir.path())
        .arg(std::ffi::OsStr::from_bytes(b"\xff.txt"))
        .write_stdin("")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not valid UTF-8"));
}

#[test]
fn unreadable_files_are_io_errors() {
    let dir = setup(&[("sub/x", "")]);
    fs::write(dir.path().join("bin.txt"), [0xff, 0xfe, b'\n']).unwrap();
    for file in ["bin.txt", "sub"] {
        rpatch_raw(
            dir.path(),
            &[file],
            "<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n",
        )
        .code(3)
        .stderr(predicate::str::contains("cannot read"));
    }
}

/// 2 ファイル目の書き込みに失敗したら、1 ファイル目を元に戻す。
#[cfg(unix)]
#[test]
fn failed_write_restores_files_already_written() {
    use std::os::unix::fs::PermissionsExt;
    let dir = setup(&[("a.txt", "a\n"), ("locked/b.txt", "b\n")]);
    let locked = dir.path().join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let assert = rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n=== locked/b.txt\n<<<<<<< OLD\nb\n=======\nB\n>>>>>>> NEW\n",
    );
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert
        .code(3)
        .stderr(predicate::str::contains("locked/b.txt: write failed"))
        .stderr(predicate::str::contains("nothing written"));
    assert_eq!(read(&dir, "a.txt"), "a\n");
    assert_eq!(read(&dir, "locked/b.txt"), "b\n");
}

/// 長い行の先頭だけを OLD に書いたとき、`OLD part` にしたら行がどうなるかを見せる。
/// NEW に行全体を書いていると行の残りが二重になるので、名前を挙げるだけでなく結果で選ばせる。
#[test]
fn partial_old_shows_the_line_that_old_part_would_give() {
    let original = "#\nlong line with a lot of text after it\nend\n";
    let dir = setup(&[("a.md", original)]);
    rpatch_raw(
        dir.path(),
        &["a.md"],
        "<<<<<<< OLD\nlong line\n=======\nshort line\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "with `<<<<<<< OLD part` and the same OLD and NEW, the other text on the line is kept, and the lines read:\n       2 | short line with a lot of text after it\n",
    ))
    .stderr(predicate::str::contains(
        "change only the opener to `<<<<<<< OLD part`",
    ));
    rpatch_raw(
        dir.path(),
        &["a.md"],
        "<<<<<<< OLD\nlong line\n=======\nlong line, all new\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "       2 | long line, all new with a lot of text after it\n",
    ));
    assert_eq!(read(&dir, "a.md"), original);
    // 切れた行の前に行を足すときは、足した行も見せる（切れた行だけだと元と同じに見えた）。
    rpatch_raw(
        dir.path(),
        &["a.md"],
        "<<<<<<< OLD\n#\nlong line\n=======\n#\nadded\nlong line\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains(
        "       1 | #\n       2 | added\n       3 | long line with a lot of text after it\n",
    ));
}

// --- === を続けて並べたとき、~/ のパス -------------------------------------------

/// 続けて並べた `===` 行は 1 つの組になり、後ろのブロックは組のファイル全部に当たる。
/// 以前は最後のファイルにだけ当たり、終了コード 0 で前のファイルが黙って変わらなかった。
#[test]
fn consecutive_file_lines_share_the_blocks() {
    let dir = setup(&[
        ("a.txt", "x = 1\n"),
        ("b.txt", "x = 1\n"),
        ("c.txt", "x = 1\ny\n"),
    ]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n=== b.txt\n\n=== c.txt\n<<<<<<< OLD\nx = 1\n=======\nx = 2\n>>>>>>> NEW\n=== c.txt\n<<<<<<< AFTER\ny\n=======\nz\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("M a.txt  edits=1"))
    .stdout(predicate::str::contains("M b.txt  edits=1"))
    .stdout(predicate::str::contains("M c.txt  edits=2"));
    assert_eq!(read(&dir, "a.txt"), "x = 2\n");
    assert_eq!(read(&dir, "b.txt"), "x = 2\n");
    // The `=== c.txt` after a block starts a new group: AFTER applies to c.txt only.
    assert_eq!(read(&dir, "c.txt"), "x = 2\ny\nz\n");
}

/// 組の中で同じファイルを 2 度挙げても（同じ名前、書き方違い、symlink とリンク先）、ブロックは
/// そのファイルに 1 回だけ当たる。前は 2 回当たり、AFTER が黙って 2 行入っていた。
#[test]
fn a_group_naming_one_file_twice_applies_each_block_once() {
    let dir = setup(&[("a.txt", "anchor\n"), ("b.txt", "x\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n=== ./a.txt\n=== a.txt\n<<<<<<< AFTER\nanchor\n=======\nnew\n>>>>>>> NEW\n=== b.txt\n=== ./b.txt\n<<<<<<< OLD\nx\n=======\nx\ny\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("M a.txt  edits=1  at_lines=2\n"))
    .stdout(predicate::str::contains(
        "note: `./a.txt` is the same file as `a.txt`; each block is applied to it once",
    ))
    .stdout(predicate::str::contains(
        "note: `a.txt` is named twice in one `===` group",
    ))
    .stdout(predicate::str::contains("M b.txt  edits=1"));
    assert_eq!(read(&dir, "a.txt"), "anchor\nnew\n");
    assert_eq!(read(&dir, "b.txt"), "x\ny\n");
    // Separate groups for one file are separate edits, applied in order.
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n<<<<<<< AFTER\nanchor\n=======\none\n>>>>>>> NEW\n=== ./a.txt\n<<<<<<< AFTER\nanchor\n=======\ntwo\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("M a.txt  edits=2"));
    assert_eq!(read(&dir, "a.txt"), "anchor\ntwo\none\nnew\n");
}

#[cfg(unix)]
#[test]
fn a_group_naming_a_symlink_and_its_target_applies_each_block_once() {
    let dir = setup(&[("a.txt", "anchor\n")]);
    std::os::unix::fs::symlink("a.txt", dir.path().join("link")).unwrap();
    rpatch_raw(
        dir.path(),
        &[],
        "=== link\n=== a.txt\n<<<<<<< AFTER\nanchor\n=======\nnew\n>>>>>>> NEW\n",
    )
    .success()
    .stdout(predicate::str::contains("M link  edits=1"));
    assert_eq!(read(&dir, "a.txt"), "anchor\nnew\n");
}

/// 組の中の 1 つのファイルで当たらなければ、どのファイルにも書かない。
#[test]
fn a_group_is_all_or_nothing() {
    let dir = setup(&[("a.txt", "x = 1\n"), ("b.txt", "other\n")]);
    rpatch_raw(
        dir.path(),
        &[],
        "=== a.txt\n=== b.txt\n<<<<<<< OLD\nx = 1\n=======\nx = 2\n>>>>>>> NEW\n",
    )
    .code(1)
    .stderr(predicate::str::contains("b.txt"))
    .stderr(predicate::str::contains("nothing written"));
    assert_eq!(read(&dir, "a.txt"), "x = 1\n");
}

/// heredoc の中ではシェルが `~` を展開しないので、`=== ~/path` はホームとして読む。
#[test]
fn home_in_a_file_line_is_expanded() {
    let home = setup(&[(".config/app.txt", "a\n")]);
    let work = setup(&[]);
    assert_cmd::cargo_bin_cmd!("rpatch")
        .current_dir(work.path())
        .env("HOME", home.path())
        .write_stdin("=== ~/.config/app.txt\n<<<<<<< OLD\na\n=======\nA\n>>>>>>> NEW\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("M ~/.config/app.txt  edits=1"));
    assert_eq!(read(&home, ".config/app.txt"), "A\n");
}

/// 開始行にブロックの本文やファイル名を書いた誤りには、本文は次の行から書くと案内する。
/// `OLD count=3 part` の並びの誤りには、`part` を先に書くと案内する。
#[test]
fn text_on_the_opener_line_gets_a_hint() {
    let dir = setup(&[("a.txt", "x\n")]);
    for opener in [
        "<<<<<<< OLD a.txt",
        "<<<<<<< AFTER x = 1",
        "<<<<<<< BEFORE - item",
    ] {
        rpatch_raw(
            dir.path(),
            &[],
            &format!("{opener}\nx\n=======\ny\n>>>>>>> NEW\n"),
        )
        .code(2)
        .stderr(predicate::str::contains("start the text on the next line"))
        .stderr(predicate::str::contains("nothing written"));
    }
    rpatch_raw(
        dir.path(),
        &[],
        "<<<<<<< OLD count=3 part\nx\n=======\ny\n>>>>>>> NEW\n",
    )
    .code(2)
    .stderr(predicate::str::contains("write `part` before `count=N`"));
    assert_eq!(read(&dir, "a.txt"), "x\n");
}
