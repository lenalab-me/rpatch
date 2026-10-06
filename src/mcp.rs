use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use tokio::sync::Mutex;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyInput {
    /// The same block-format or JSON input as rpatch CLI stdin. No shell/heredoc wrapper.
    pub input: String,
    /// Include a unified diff alongside the changed-line summary (default: false).
    #[serde(default)]
    pub diff: bool,
    /// Validate and show changes without writing files (default: false).
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Clone)]
struct PatchServer {
    cwd: PathBuf,
    edit_lock: Arc<Mutex<()>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl PatchServer {
    fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            edit_lock: Arc::new(Mutex::new(())),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "rpatch",
        description = "Apply multiple exact text edits across existing UTF-8 files in one batch. All edits are checked before writing; a mismatch writes nothing and returns line-numbered diagnostics. Edits in one file apply in order. Paths are relative to the server's configured working directory, or absolute.\nUse input as block text, for example:\n=== app.js\n<<<<<<< OLD\nconst x = 1;\n=======\nconst x = 2;\n>>>>>>> NEW\nRepeat blocks/files for a batch. OLD matches whole lines, including indentation; count=N requires exactly N matches (default 1). To change part of a line, open the block with <<<<<<< OLD part (matched anywhere, final line break excluded). FROM / TO replaces FROM up to the first TO after it, keeping TO: === file\n<<<<<<< FROM\nstart line\n<<<<<<< TO\nnext section\n=======\nreplacement\n>>>>>>> NEW. Their last line may be a line prefix (a blank last line matches a blank line); no TO lines means to the end of the file. For insertion use BEFORE or AFTER in place of OLD, with an anchor that is any part of one or more lines (count=N allowed). Markers can be longer than 7 characters to contain marker text. Block text follows the file's LF/CRLF endings.\nFor exact substrings anywhere, input can instead be JSON text: {\"edits\":[{\"file\":\"app.js\",\"old\":\"x = 1\",\"new\":\"x = 2\",\"count\":1}]}. JSON also supports from/to instead of old. Empty old, identical old/new and overlapping matches are errors. Returns the CLI summary (file, edit count, changed lines); diff=true adds a unified diff; dry_run checks without writing.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn rpatch(&self, Parameters(args): Parameters<ApplyInput>) -> CallToolResult {
        let guard = self.edit_lock.clone().lock_owned().await;
        let cwd = self.cwd.clone();
        let result = tokio::task::spawn_blocking(move || {
            // Keep serialization even if the client cancels while a write is running.
            let _guard = guard;
            rpatch::apply(
                &args.input,
                rpatch::PatchOptions {
                    diff: args.diff,
                    dry_run: args.dry_run,
                    file: None,
                },
                &cwd,
            )
        })
        .await;
        // Text only: clients that prefer structuredContent would hide the diagnostics.
        match result {
            Ok(Ok(out)) => CallToolResult::success(vec![ContentBlock::text(out)]),
            Ok(Err(err)) => CallToolResult::error(vec![ContentBlock::text(err.message)]),
            Err(err) => CallToolResult::error(vec![ContentBlock::text(format!(
                "rpatch: edit worker failed: {err}\n"
            ))]),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for PatchServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("rpatch", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!("Exact-match batch editing. Relative paths resolve against {}. See the rpatch tool for input syntax.", self.cwd.display()))
    }
}

pub fn serve(cwd: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let server = PatchServer::new(cwd).serve(stdio()).await?;
            server.waiting().await?;
            Ok(())
        })
}
