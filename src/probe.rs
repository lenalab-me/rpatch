//! `rpatch --probe CMD...`: apply the edits, run CMD, put every file back as it was.
//! It answers "does a check fail when this code is broken?" without the caller saving
//! and restoring the files by hand. Whatever happens to CMD, the files are restored and
//! compared byte for byte with what they held.

use std::io::{self, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rpatch::Backups;

const EXIT_NOT_CAUGHT: u8 = 1;
const EXIT_INPUT: u8 = 2;
const EXIT_IO: u8 = 3;

/// Set by SIGINT, SIGTERM and SIGHUP (Ctrl-C on Windows), which would otherwise end rpatch
/// with the files still broken. SIGKILL cannot be caught.
static SIGNALED: AtomicBool = AtomicBool::new(false);

/// Must run before the first file is written.
pub fn catch_signals() -> Result<(), u8> {
    ctrlc::set_handler(|| SIGNALED.store(true, Ordering::SeqCst)).map_err(|err| {
        eprintln!("rpatch: cannot watch for signals, so --probe cannot promise to restore: {err}");
        EXIT_IO
    })
}

enum Outcome {
    Exited(ExitStatus),
    Interrupted,
    Failed(String),
}

/// Holds the backups until they are restored, and restores them if the thread unwinds
/// before that.
struct Armed(Option<Backups>);

impl Armed {
    fn restore(&mut self) -> Result<Vec<String>, Vec<String>> {
        let backups = self.0.take().expect("restored once");
        let files = backups.files().iter().map(|f| f.to_string()).collect();
        backups.restore().map(|()| files)
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        if let Some(backups) = self.0.take()
            && let Err(failed) = backups.restore()
        {
            eprintln!("{}", restore_failed(&failed));
        }
    }
}

fn restore_failed(failed: &[String]) -> String {
    format!(
        "rpatch: RESTORE FAILED: these files are left BROKEN by --probe, fix them by hand: {}",
        failed.join("; ")
    )
}

/// `out` is what a plain run prints (the `M` lines, the diff). Returns the exit status:
/// `Ok` when the command failed with the edits applied (the break is caught).
pub fn run(out: &str, backups: Backups, cmd: &[String]) -> Result<(), u8> {
    let mut armed = Armed(Some(backups));
    if armed.0.as_ref().is_some_and(Backups::is_empty) {
        eprintln!(
            "rpatch: the edits change no file, so there is nothing to probe; the command was not run"
        );
        return Err(EXIT_INPUT);
    }
    // A reader that stopped early (`| head`) is no reason to leave files broken.
    let _ = io::stdout()
        .write_all(out.as_bytes())
        .and_then(|()| io::stdout().flush());
    let outcome = run_command(cmd);
    let restored = armed.restore();
    let shown = cmd_line(cmd);

    let files = match restored {
        Ok(files) => files,
        Err(failed) => {
            eprintln!("{}", restore_failed(&failed));
            return Err(EXIT_IO);
        }
    };
    let restored_line = format!(
        "probe: restored {} (compared byte for byte with before)",
        files.join(", ")
    );
    match outcome {
        Outcome::Interrupted => {
            eprintln!("{restored_line}");
            eprintln!(
                "rpatch: interrupted by a signal; the command was stopped, the files are restored, and the probe has no result"
            );
            Err(EXIT_IO)
        }
        Outcome::Failed(why) => {
            eprintln!("{restored_line}");
            eprintln!("rpatch: {why}; the probe has no result");
            Err(EXIT_IO)
        }
        Outcome::Exited(status) if status.success() => {
            eprintln!("{restored_line}");
            eprintln!(
                "probe: NOT CAUGHT: `{shown}` passed (exit 0) with the edits applied; nothing it runs fails on this break"
            );
            Err(EXIT_NOT_CAUGHT)
        }
        Outcome::Exited(status) => {
            println!("{restored_line}");
            println!(
                "probe: caught: `{shown}` failed ({}) with the edits applied",
                describe(status)
            );
            Ok(())
        }
    }
}

/// Runs the command with its output passed through as it is (the text of the failure is
/// what the caller reports). Its stdin is closed: the edits already used rpatch's.
fn run_command(cmd: &[String]) -> Outcome {
    if SIGNALED.load(Ordering::SeqCst) {
        return Outcome::Interrupted;
    }
    let mut child = match Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return Outcome::Failed(format!("cannot run `{}`: {err}", cmd[0])),
    };
    loop {
        if SIGNALED.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Outcome::Interrupted;
        }
        match child.try_wait() {
            // A signal that arrived as the command ended may have ended it.
            Ok(Some(_)) if SIGNALED.load(Ordering::SeqCst) => return Outcome::Interrupted,
            Ok(Some(status)) => return Outcome::Exited(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Outcome::Failed(format!("cannot wait for `{}`: {err}", cmd[0]));
            }
        }
    }
}

fn describe(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit {code}");
    }
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return format!("killed by signal {signal}");
    }
    "terminated".to_string()
}

/// The command as a shell would take it back, roughly: arguments with spaces are quoted.
fn cmd_line(cmd: &[String]) -> String {
    let line = cmd
        .iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains(|c: char| c.is_whitespace() || "'\"\\$`".contains(c))
            {
                format!("'{}'", arg.replace('\'', "'\\''"))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    // `sh -c '...'` can carry a whole script.
    match line.char_indices().nth(100) {
        Some((cut, _)) => format!("{}...", &line[..cut]),
        None => line,
    }
}
