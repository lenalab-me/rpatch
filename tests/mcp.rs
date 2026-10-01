use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

struct Session {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<Result<Value, String>>,
    next_id: u64,
}

impl Session {
    fn start(cwd: &Path) -> Self {
        let mut child = Command::new(assert_cmd::cargo::cargo_bin!("rpatch"))
            .args(["mcp", "--cwd"])
            .arg(cwd)
            // Deliberately launch outside the edit directory to exercise --cwd.
            .current_dir(cwd.parent().unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, responses) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value = line.map_err(|err| err.to_string()).and_then(|line| {
                    serde_json::from_str(&line)
                        .map_err(|err| format!("non-protocol stdout: {line}: {err}"))
                });
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let mut session = Self {
            child,
            stdin,
            responses,
            next_id: 1,
        };
        let response = session.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "rpatch-tests", "version": "1"},
            }),
        );
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(response["result"]["serverInfo"]["name"], "rpatch");
        session.send(json!({"jsonrpc":"2.0", "method":"notifications/initialized"}));
        session
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}));
        loop {
            let response = self
                .responses
                .recv_timeout(Duration::from_secs(15))
                .expect("MCP server timed out or closed stdout")
                .expect("valid JSON-RPC stdout");
            if response.get("id") == Some(&json!(id)) {
                return response;
            }
        }
    }

    fn apply(&mut self, arguments: Value) -> Value {
        let response = self.request(
            "tools/call",
            json!({"name":"rpatch", "arguments":arguments}),
        );
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().unwrap()
}

#[test]
fn advertises_one_editing_tool_with_defaults_and_annotations() {
    let dir = TempDir::new().unwrap();
    let mut session = Session::start(dir.path());
    let response = session.request("tools/list", json!({}));
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    let tool = &tools[0];
    assert_eq!(tool["name"], "rpatch");
    assert_eq!(tool["inputSchema"]["required"], json!(["input"]));
    assert_eq!(tool["inputSchema"]["properties"]["diff"]["default"], true);
    assert_eq!(tool["annotations"]["readOnlyHint"], false);
    assert_eq!(tool["annotations"]["destructiveHint"], true);
}

#[test]
fn block_batch_writes_sequentially_and_returns_the_cli_diff() {
    let dir = TempDir::new().unwrap();
    let cli_dir = TempDir::new().unwrap();
    for root in [dir.path(), cli_dir.path()] {
        fs::write(root.join("a.txt"), "hello\n").unwrap();
        fs::write(root.join("b.txt"), "world\n").unwrap();
    }
    let input = "=== a.txt\n<<<<<<< OLD\nhello\n=======\nHELLO\n>>>>>>> NEW\n<<<<<<< AFTER\nHELLO\n=======\n日本語 `${x}`\n>>>>>>> NEW\n=== b.txt\n<<<<<<< OLD\nworld\n=======\nWORLD\n>>>>>>> NEW\n";
    let result = Session::start(dir.path()).apply(json!({"input": input}));
    assert_eq!(result["isError"], false);
    assert!(result.get("structuredContent").is_none());
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "HELLO\n日本語 `${x}`\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "WORLD\n"
    );
    let cli = assert_cmd::cargo_bin_cmd!("rpatch")
        .current_dir(cli_dir.path())
        .arg("--diff")
        .write_stdin(input)
        .output()
        .unwrap();
    assert!(cli.status.success());
    assert_eq!(text(&result), String::from_utf8(cli.stdout).unwrap());
}

#[test]
fn mismatch_writes_nothing_and_diagnostics_equal_cli_then_server_recovers() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("b.txt"), "world\n").unwrap();
    let input = json!({"edits":[
        {"file":"a.txt", "old":"hello", "new":"HELLO"},
        {"file":"b.txt", "old":"missing", "new":"NEW"},
    ]})
    .to_string();
    let mut session = Session::start(dir.path());
    let result = session.apply(json!({"input": input}));
    assert_eq!(result["isError"], true);
    assert!(text(&result).ends_with("rpatch: nothing written\n"));
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "world\n"
    );
    let cli = assert_cmd::cargo_bin_cmd!("rpatch")
        .current_dir(dir.path())
        .write_stdin(input)
        .output()
        .unwrap();
    assert_eq!(text(&result), String::from_utf8(cli.stderr).unwrap());
    let fixed = json!({"edits":[{"file":"a.txt", "old":"hello", "new":"HELLO"}]}).to_string();
    let result = session.apply(json!({"input": fixed, "diff": false}));
    assert_eq!(result["isError"], false);
    assert!(!text(&result).contains("--- a/"));
}

#[test]
fn dry_run_preserves_crlf_bom_and_range_endpoint() {
    let dir = TempDir::new().unwrap();
    let original = "\u{feff}start\r\nold\r\nend\r\n";
    fs::write(dir.path().join("a.txt"), original).unwrap();
    let input = "=== a.txt\n<<<<<<< FROM\nstart\n<<<<<<< TO\nend\n=======\nnew\n>>>>>>> NEW\n";
    let mut session = Session::start(dir.path());
    let result = session.apply(json!({"input": input, "dry_run": true}));
    assert_eq!(result["isError"], false);
    assert!(text(&result).contains("replaced lines 1-2"));
    assert!(text(&result).contains("dry run: nothing written"));
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        original
    );
    assert_eq!(session.apply(json!({"input": input}))["isError"], false);
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "\u{feff}new\r\nend\r\n"
    );
}

#[test]
fn invalid_input_and_missing_file_return_cli_diagnostics() {
    let dir = TempDir::new().unwrap();
    let mut session = Session::start(dir.path());
    let bad = session.apply(json!({"input": "{broken"}));
    assert_eq!(bad["isError"], true);
    // Input errors come from the parser, before any file is read.
    assert!(text(&bad).starts_with("rpatch: "));
    assert!(text(&bad).contains("invalid JSON input"));
    let missing = session
        .apply(json!({"input": "=== absent.txt\n<<<<<<< OLD\na\n=======\nb\n>>>>>>> NEW\n"}));
    assert_eq!(missing["isError"], true);
    assert!(text(&missing).starts_with("error: "));
    assert!(text(&missing).ends_with("rpatch: nothing written\n"));
    assert!(text(&missing).contains(&format!(
        "cwd: {}",
        dir.path().canonicalize().unwrap().display()
    )));
    let invalid_args = session.request(
        "tools/call",
        json!({"name":"rpatch", "arguments":{"input":"x", "unknown":true}}),
    );
    // Argument schema failures belong to the SDK's protocol handling.
    assert!(invalid_args.get("error").is_some() || invalid_args["result"]["isError"] == true);
    let unknown_tool = session.request("tools/call", json!({"name":"missing", "arguments":{}}));
    assert!(unknown_tool.get("error").is_some());
    assert!(session.request("ping", json!({})).get("result").is_some());
}

#[test]
fn read_only_file_returns_io_error_without_writes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "old\n").unwrap();
    let original_perms = fs::metadata(&path).unwrap().permissions();
    let mut readonly = original_perms.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&path, readonly).unwrap();
    let result = Session::start(dir.path())
        .apply(json!({"input":"=== a.txt\n<<<<<<< OLD\nold\n=======\nnew\n>>>>>>> NEW\n"}));
    // Restore before assertions so Windows can remove the temporary directory.
    fs::set_permissions(&path, original_perms).unwrap();
    assert_eq!(result["isError"], true);
    assert!(text(&result).contains("read-only"));
    assert_eq!(fs::read_to_string(path).unwrap(), "old\n");
}

#[test]
fn mcp_startup_errors_are_stderr_only_and_help_needs_no_handshake() {
    let dir = TempDir::new().unwrap();
    assert_cmd::cargo_bin_cmd!("rpatch")
        .args(["mcp", "--cwd"])
        .arg(dir.path().join("missing"))
        .assert()
        .code(3)
        .stdout("");
    assert_cmd::cargo_bin_cmd!("rpatch")
        .args(["mcp", "--cwd"])
        .assert()
        .code(2)
        .stdout("");
    assert_cmd::cargo_bin_cmd!("rpatch")
        .args(["mcp", "--diff"])
        .assert()
        .code(2)
        .stdout("");
    assert_cmd::cargo_bin_cmd!("rpatch")
        .args(["mcp", "--help"])
        .assert()
        .success();
}

#[test]
fn concurrent_requests_do_not_both_commit_against_the_same_original() {
    let dir = TempDir::new().unwrap();
    let original = format!("{}old\n", "context\n".repeat(16_000));
    fs::write(dir.path().join("a.txt"), &original).unwrap();
    let mut session = Session::start(dir.path());
    let input = json!({"edits":[{"file":"a.txt", "old":"old", "new":"new"}]}).to_string();
    for id in 2..10 {
        session.send(
            json!({"jsonrpc":"2.0", "id":id, "method":"tools/call", "params":{
                "name":"rpatch", "arguments":{"input":input,"diff":false},
            }}),
        );
    }
    let mut successes = 0;
    let mut failures = 0;
    let mut ids = std::collections::HashSet::new();
    while ids.len() < 8 {
        let response = session
            .responses
            .recv_timeout(Duration::from_secs(15))
            .unwrap()
            .unwrap();
        let Some(id) = response.get("id").and_then(Value::as_u64) else {
            continue;
        };
        assert!(ids.insert(id));
        assert!(response.get("error").is_none(), "{response}");
        let result = &response["result"];
        if result["isError"] == true {
            assert!(
                text(result).ends_with("rpatch: nothing written\n"),
                "{result}"
            );
            failures += 1;
        } else {
            successes += 1;
        }
    }
    assert_eq!((successes, failures), (1, 7));
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        original.replace("old", "new")
    );
}
