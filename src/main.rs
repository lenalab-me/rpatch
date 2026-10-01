use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

use rpatch::PatchOptions;

const USAGE: &str = include_str!("help.txt");
const SHORT_USAGE: &str = "usage: rpatch [--diff] [--dry-run] <<'EOF' ... EOF   (see `rpatch --help` for the input format)";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

fn run() -> Result<(), u8> {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
    let cwd = std::env::current_dir().map_err(|err| {
        eprintln!("rpatch: cannot resolve current directory: {err}");
        3
    })?;
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
            _ => {
                eprintln!("rpatch: unknown argument: {arg}\n{SHORT_USAGE}");
                return Err(2);
            }
        }
    }
    Ok(Some(opts))
}
