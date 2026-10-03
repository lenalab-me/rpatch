#[cfg(feature = "mcp")]
mod mcp;

use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use rpatch::PatchOptions;

const USAGE: &str = include_str!("help.txt");
const SHORT_USAGE: &str = "usage: rpatch [--diff] [--dry-run] [FILE] <<'EOF' ... EOF   (see `rpatch --help` for the input format)";
#[cfg(feature = "mcp")]
const MCP_USAGE: &str = "usage: rpatch mcp [--cwd <directory>]\nRuns a local MCP server over stdin/stdout. Relative edit paths use --cwd (default: current directory).";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

fn run() -> Result<(), u8> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "mcp") {
        return run_mcp(&args[1..]);
    }
    let Some(opts) = parse_args(&args)? else {
        return Ok(());
    };
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        eprintln!("rpatch: expected edits on stdin\n{SHORT_USAGE}");
        return Err(2);
    }
    let mut text = String::new();
    stdin.read_to_string(&mut text).map_err(|err| {
        eprintln!("rpatch: cannot read stdin: {err}");
        2
    })?;
    // A deleted cwd must not hide input errors; relative paths then fail on their own.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match rpatch::apply(&text, opts, &cwd) {
        Ok(out) => {
            print!("{out}");
            Ok(())
        }
        Err(err) => {
            eprint!("{}", err.message);
            Err(err.code)
        }
    }
}

fn parse_args(args: &[String]) -> Result<Option<PatchOptions>, u8> {
    let mut opts = PatchOptions::default();
    for arg in args {
        match arg.as_str() {
            "--diff" => opts.diff = true,
            "-n" | "--dry-run" => opts.dry_run = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("rpatch {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            _ if arg.starts_with('-') => {
                eprintln!("rpatch: unknown argument: {arg}\n{SHORT_USAGE}");
                return Err(2);
            }
            // `rpatch FILE <<'EOF'`: the file for blocks without a `=== <file>` line.
            _ => {
                if let Some(first) = &opts.file {
                    eprintln!(
                        "rpatch: give at most one file argument (got `{first}` and `{arg}`); for several files, put `=== <file>` lines in the input\n{SHORT_USAGE}"
                    );
                    return Err(2);
                }
                opts.file = Some(arg.clone());
            }
        }
    }
    Ok(Some(opts))
}

#[cfg(not(feature = "mcp"))]
fn run_mcp(_args: &[String]) -> Result<(), u8> {
    eprintln!(
        "rpatch: this binary was built without MCP support; rebuild with `cargo install --locked --path . --features mcp`"
    );
    Err(2)
}

#[cfg(feature = "mcp")]
fn run_mcp(args: &[String]) -> Result<(), u8> {
    let mut cwd = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cwd" => {
                cwd = Some(PathBuf::from(args.next().ok_or_else(|| {
                    eprintln!("rpatch: --cwd needs a directory\n{MCP_USAGE}");
                    2
                })?));
            }
            "-h" | "--help" => {
                println!("{MCP_USAGE}");
                return Ok(());
            }
            _ => {
                eprintln!("rpatch: unknown MCP argument: {arg}\n{MCP_USAGE}");
                return Err(2);
            }
        }
    }
    let cwd = cwd.unwrap_or_else(|| PathBuf::from("."));
    let cwd = std::fs::canonicalize(&cwd)
        .and_then(|path| {
            if path.is_dir() {
                Ok(path)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "not a directory",
                ))
            }
        })
        .map_err(|err| {
            eprintln!("rpatch: cannot use --cwd {}: {err}", cwd.display());
            3
        })?;
    mcp::serve(cwd).map_err(|err| {
        eprintln!("rpatch: MCP server: {err}");
        3
    })
}
