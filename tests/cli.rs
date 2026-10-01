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
    .stdout("M a.js  edits=1  lines=1,3\n");
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
    .stdout("M a.txt  edits=2  lines=1\n");
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
    .stdout("M a.txt  edits=2  lines=2,5\n");
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
    .stdout("M a.txt  edits=1  lines=1\nM sub/b.txt  edits=1  lines=1\n");
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
    .stdout("M a.txt  edits=2  lines=1\n");
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
    .stdout(predicate::str::contains("M a.txt  edits=1  lines=1\n"))
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
