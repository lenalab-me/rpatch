mod help;
#[cfg(feature = "mcp")]
mod mcp;
mod probe;

use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use rpatch::PatchOptions;

const SHORT_USAGE: &str = "usage: rpatch [--diff] [--dry-run] [FILE] [--probe CMD...] <<'EOF' ... EOF   (see `rpatch --help` for the input format)";
#[cfg(feature = "mcp")]
const MCP_USAGE: &str = "usage: rpatch mcp [--cwd <directory>]\nRuns a local MCP server over stdin/stdout. Relative edit paths use --cwd (default: current directory).";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

fn run() -> Result<(), u8> {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string().map_err(|arg| {
                eprintln!(
                    "rpatch: argument is not valid UTF-8: {}",
                    arg.to_string_lossy()
                );
                2
            })
        })
        .collect::<Result<_, _>>()?;
    if args.first().is_some_and(|arg| arg == "mcp") {
        return run_mcp(&args[1..]);
    }
    let Some((opts, probe_cmd)) = parse_args(&args)? else {
        return Ok(());
    };
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        eprintln!("rpatch: expected edits on stdin\n{SHORT_USAGE}");
        return Err(2);
    }
    let mut text = String::new();
    stdin.read_to_string(&mut text).map_err(|err| {
        eprintln!("rpatch: cannot read stdin: {err}\nrpatch: nothing written");
        2
    })?;
    // A deleted cwd must not hide input errors; relative paths then fail on their own.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(cmd) = probe_cmd {
        // Before the first write: a signal that comes later still ends in a restore.
        probe::catch_signals()?;
        return match rpatch::apply_with_backups(&text, opts, &cwd) {
            Ok((out, backups)) => probe::run(&out, backups, &cmd),
            Err(err) => {
                eprint!("{}", err.message);
                // 1 is "not caught" in a probe; an edit that did not match broke nothing.
                if err.code == 1 {
                    eprintln!("rpatch: --probe broke nothing, so the command was not run");
                    return Err(2);
                }
                Err(err.code)
            }
        };
    }
    match rpatch::apply(&text, opts, &cwd) {
        Ok(out) => print_out(&out).map_err(|err| {
            eprintln!("rpatch: the edits were written, but printing the result failed: {err}");
            3
        }),
        Err(err) => {
            eprint!("{}", err.message);
            Err(err.code)
        }
    }
}

/// Prints to stdout. A reader that stopped early (`| head`) is not a failure.
fn print_out(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

/// The options, and the command after `--probe` if there is one.
type Parsed = (PatchOptions, Option<Vec<String>>);

fn parse_args(args: &[String]) -> Result<Option<Parsed>, u8> {
    // `--probe` takes everything after it as the command, so the command's own `--help` or
    // `--diff` are the command's. A `--probe` after `--` is a file name.
    let end = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    let (args, probe) = match args[..end].iter().position(|arg| arg == "--probe") {
        Some(k) => {
            let cmd = &args[k + 1..];
            // `--probe -- CMD`: the `--` is only a divider, as in `git commit --`.
            let cmd = cmd.strip_prefix(&["--".to_string()]).unwrap_or(cmd);
            (&args[..k], Some(cmd.to_vec()))
        }
        None => (args, None),
    };
    // `--` ends the options, so a file name may start with `-`.
    let options = match args.iter().position(|arg| arg == "--") {
        Some(k) => &args[..k],
        None => args,
    };
    // --help and --version win wherever they are, as an unknown argument next to them
    // is no reason to withhold the help.
    let print = |text: &str| {
        print_out(text).map_err(|err| {
            eprintln!("rpatch: {err}");
            3
        })
    };
    if let Some(k) = options
        .iter()
        .position(|arg| arg == "-h" || arg == "--help")
    {
        // `--help TOPIC`: the argument right after it, unless that is another option.
        let topic = options.get(k + 1).filter(|arg| !arg.starts_with('-'));
        let text = help::render(topic.map(String::as_str)).map_err(|msg| {
            eprintln!("{msg}");
            2
        })?;
        print(&text)?;
        return Ok(None);
    }
    if options.iter().any(|arg| arg == "-V" || arg == "--version") {
        print(&format!("rpatch {}\n", env!("CARGO_PKG_VERSION")))?;
        return Ok(None);
    }
    let mut opts = PatchOptions::default();
    for (i, arg) in args.iter().enumerate() {
        let option = i < options.len();
        match arg.as_str() {
            "--" if i == options.len() => {}
            "--diff" if option => opts.diff = true,
            "-n" | "--dry-run" if option => opts.dry_run = true,
            _ if option && arg.starts_with('-') => {
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
    if let Some(cmd) = &probe {
        if cmd.is_empty() {
            eprintln!("rpatch: --probe needs a command after it\n{SHORT_USAGE}");
            return Err(2);
        }
        if opts.dry_run {
            eprintln!(
                "rpatch: --probe cannot be combined with --dry-run: a dry run breaks nothing, so the command would run on the code as it is\n{SHORT_USAGE}"
            );
            return Err(2);
        }
    }
    Ok(Some((opts, probe)))
}

#[cfg(not(feature = "mcp"))]
fn run_mcp(_args: &[String]) -> Result<(), u8> {
    eprintln!(
        "rpatch: this binary was built without MCP support; rebuild with `cargo install --locked --path . --features mcp`\n  (for a file named `mcp`, write `./mcp`)"
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
