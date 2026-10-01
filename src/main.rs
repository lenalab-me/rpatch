mod diagnose;
mod input;
mod text;
mod write;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, Permissions};
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use similar::TextDiff;

use crate::input::Edit;
use crate::text::LineIndex;

const USAGE: &str = r#"rpatch - apply exact-match text replacements to files, all or nothing

Usage: rpatch [--diff] [--dry-run] <<'EOF'
{"edits": [{"file": "path", "old": "exact text", "new": "replacement", "count": 1}]}
EOF

  - "old" must match exactly; whitespace is never matched loosely.
  - The number of matches must equal "count" (default 1).
  - Edits to the same file apply in order; each one sees the result of the previous.
  - Nothing is written unless every edit matches. Each file is replaced via temp file + rename.

Options:
  --diff         also print a unified diff of the changes
  -n, --dry-run  check and report, but write nothing
  -h, --help     print this help
  -V, --version  print the version

Exit status: 0 ok, 1 an edit did not match or a file is missing, 2 invalid input, 3 I/O error
"#;

const EXIT_MISMATCH: u8 = 1;
const EXIT_INPUT: u8 = 2;
const EXIT_IO: u8 = 3;

#[derive(Default)]
struct Opts {
    diff: bool,
    dry_run: bool,
}

struct FileState {
    /// The path as first written in the input.
    display: String,
    path: PathBuf,
    original: String,
    content: String,
    perms: Permissions,
    applied: Vec<usize>,
    skipped: Vec<usize>,
    /// Byte offsets in `content` where replacement text starts.
    marks: Vec<usize>,
    failed: bool,
}

impl FileState {
    fn changed_lines(&self) -> Vec<usize> {
        let idx = LineIndex::new(&self.content);
        let mut lines: Vec<usize> = self.marks.iter().map(|&m| idx.line_of(m)).collect();
        lines.sort_unstable();
        lines.dedup();
        lines
    }
}

#[derive(Default)]
struct Report {
    lines: Vec<String>,
    failed: bool,
    io: bool,
}

impl Report {
    fn error(&mut self, headline: String, details: Vec<String>) {
        self.failed = true;
        self.lines.push(format!("error: {headline}"));
        self.lines
            .extend(details.into_iter().map(|d| format!("  {d}")));
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

fn run() -> Result<(), u8> {
    let Some(opts) = parse_args()? else {
        return Ok(());
    };
    let edits = read_edits()?;

    let mut report = Report::default();
    let (mut files, file_of) = load_files(&edits, &mut report);
    apply_edits(&edits, &mut files, &file_of, &mut report);

    if report.failed {
        for line in &report.lines {
            eprintln!("{line}");
        }
        eprintln!("rpatch: nothing written");
        return Err(if report.io { EXIT_IO } else { EXIT_MISMATCH });
    }

    if !opts.dry_run {
        let pending: Vec<write::Pending> = files
            .iter()
            .filter(|f| f.content != f.original)
            .map(|f| write::Pending {
                display: &f.display,
                path: &f.path,
                original: &f.original,
                content: &f.content,
                perms: &f.perms,
            })
            .collect();
        if let Err(msg) = write::commit(&pending) {
            eprintln!("rpatch: {msg}");
            return Err(EXIT_IO);
        }
    }

    let mut out = String::new();
    for f in &files {
        let lines: Vec<String> = f.changed_lines().iter().map(|l| l.to_string()).collect();
        let _ = writeln!(
            out,
            "M {}  edits={}  lines={}",
            f.display,
            f.applied.len(),
            lines.join(",")
        );
    }
    if opts.diff {
        for f in &files {
            let diff = TextDiff::from_lines(&f.original, &f.content);
            let _ = write!(
                out,
                "{}",
                diff.unified_diff()
                    .header(&format!("a/{}", f.display), &format!("b/{}", f.display))
            );
        }
    }
    if opts.dry_run {
        out.push_str("(dry run: nothing written)\n");
    }
    print!("{out}");
    Ok(())
}

/// Returns `None` when the invocation only asked for help or the version.
fn parse_args() -> Result<Option<Opts>, u8> {
    let mut opts = Opts::default();
    for arg in std::env::args().skip(1) {
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
                eprint!("rpatch: unknown argument: {arg}\n\n{USAGE}");
                return Err(EXIT_INPUT);
            }
        }
    }
    Ok(Some(opts))
}

fn read_edits() -> Result<Vec<Edit>, u8> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        eprint!("rpatch: expected JSON on stdin\n\n{USAGE}");
        return Err(EXIT_INPUT);
    }
    let mut text = String::new();
    if let Err(e) = stdin.read_to_string(&mut text) {
        eprintln!("rpatch: cannot read stdin: {e}");
        return Err(EXIT_INPUT);
    }
    input::parse(&text).map_err(|msg| {
        eprintln!("rpatch: {msg}");
        EXIT_INPUT
    })
}

struct LoadFailure {
    file: String,
    message: String,
    details: Vec<String>,
    io: bool,
    edits: Vec<usize>,
}

/// Reads every file named in `edits` once. Returns the files and, per edit, the index
/// of its file (`None` when the file could not be loaded).
fn load_files(edits: &[Edit], report: &mut Report) -> (Vec<FileState>, Vec<Option<usize>>) {
    let mut files: Vec<FileState> = Vec::new();
    let mut by_path: HashMap<PathBuf, usize> = HashMap::new();
    let mut failures: Vec<LoadFailure> = Vec::new();
    let mut file_of = Vec::with_capacity(edits.len());

    for (i, e) in edits.iter().enumerate() {
        if let Some(f) = failures.iter_mut().find(|f| f.file == e.file) {
            f.edits.push(i);
            file_of.push(None);
            continue;
        }
        let loaded = fs::canonicalize(&e.file)
            .map_err(|err| load_failure(e, err))
            .and_then(|path| match by_path.get(&path) {
                Some(&k) => Ok(k),
                None => {
                    let (original, perms) = read_file(&path).map_err(|err| load_failure(e, err))?;
                    by_path.insert(path.clone(), files.len());
                    files.push(FileState {
                        display: e.file.clone(),
                        path,
                        content: original.clone(),
                        original,
                        perms,
                        applied: Vec::new(),
                        skipped: Vec::new(),
                        marks: Vec::new(),
                        failed: false,
                    });
                    Ok(files.len() - 1)
                }
            });
        match loaded {
            Ok(k) => file_of.push(Some(k)),
            Err(mut f) => {
                f.edits.push(i);
                failures.push(f);
                file_of.push(None);
            }
        }
    }

    for f in failures {
        report.io |= f.io;
        report.error(
            format!("{}: {} ({})", f.file, f.message, edit_list(&f.edits)),
            f.details,
        );
    }
    (files, file_of)
}

fn load_failure(e: &Edit, err: io::Error) -> LoadFailure {
    let (message, details, io) = if err.kind() == io::ErrorKind::NotFound {
        let cwd = std::env::current_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|_| "(unknown)".into());
        (
            "file not found".to_string(),
            vec![format!("cwd: {cwd}")],
            false,
        )
    } else {
        (format!("cannot read: {err}"), Vec::new(), true)
    };
    LoadFailure {
        file: e.file.clone(),
        message,
        details,
        io,
        edits: Vec::new(),
    }
}

fn read_file(path: &Path) -> io::Result<(String, Permissions)> {
    let bytes = fs::read(path)?;
    let perms = fs::metadata(path)?.permissions();
    let text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not valid UTF-8 text"))?;
    Ok((text, perms))
}

fn apply_edits(
    edits: &[Edit],
    files: &mut [FileState],
    file_of: &[Option<usize>],
    report: &mut Report,
) {
    for (i, e) in edits.iter().enumerate() {
        let Some(k) = file_of[i] else { continue };
        let f = &mut files[k];
        if f.failed {
            f.skipped.push(i);
            continue;
        }
        let positions: Vec<usize> = f
            .content
            .match_indices(e.old.as_str())
            .map(|(p, _)| p)
            .collect();
        if positions.len() == e.count {
            f.content = replace_at(&f.content, &positions, &e.old, &e.new, &mut f.marks);
            f.applied.push(i);
            continue;
        }

        f.failed = true;
        let (headline, mut details) = if positions.is_empty() {
            (
                "\"old\" not found".to_string(),
                diagnose::not_found(&f.content, &e.old),
            )
        } else {
            (
                format!(
                    "expected {} match{}, found {}",
                    e.count,
                    if e.count == 1 { "" } else { "es" },
                    positions.len()
                ),
                diagnose::count_mismatch(&f.content, &positions, e.count),
            )
        };
        if !f.applied.is_empty() && f.original.matches(e.old.as_str()).count() == e.count {
            details.push(format!(
                "note: \"old\" matches the file as it was before {}; an earlier edit in this batch changed that text",
                edit_list(&f.applied)
            ));
        }
        report.error(format!("edits[{i}] {}: {headline}", e.file), details);
    }

    for f in files.iter() {
        if !f.skipped.is_empty() {
            report.lines.push(format!(
                "note: {} ({}) not checked because an earlier edit to the same file failed",
                edit_list(&f.skipped),
                f.display
            ));
        }
    }
}

/// Replaces `old` at each of `positions` (non-overlapping, ascending) with `new`.
/// Existing `marks` are moved to their new offsets, and the start of each inserted
/// `new` is added as a mark.
fn replace_at(
    content: &str,
    positions: &[usize],
    old: &str,
    new: &str,
    marks: &mut Vec<usize>,
) -> String {
    let mut out = String::with_capacity(content.len() + positions.len() * new.len());
    let mut new_positions = Vec::with_capacity(positions.len());
    let mut last = 0;
    for &p in positions {
        out.push_str(&content[last..p]);
        new_positions.push(out.len());
        out.push_str(new);
        last = p + old.len();
    }
    out.push_str(&content[last..]);

    for m in marks.iter_mut() {
        let k = positions.partition_point(|&p| p <= *m);
        if k == 0 {
            continue;
        }
        let (p, np) = (positions[k - 1], new_positions[k - 1]);
        *m = if *m < p + old.len() {
            np
        } else {
            *m - (p + old.len()) + np + new.len()
        };
    }
    marks.extend(new_positions);
    out
}

fn edit_list(ids: &[usize]) -> String {
    ids.iter()
        .map(|i| format!("edits[{i}]"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_follow_later_edits() {
        let mut marks = vec![];
        let s = replace_at("aXbXc", &[1, 3], "X", "YY", &mut marks);
        assert_eq!(s, "aYYbYYc");
        assert_eq!(marks, vec![1, 4]);
        // Replace "b" (offset 3) with nothing: the mark after it shifts left by one,
        // and a mark is added where "b" was.
        let s = replace_at(&s, &[3], "b", "", &mut marks);
        assert_eq!(s, "aYYYYc");
        assert_eq!(marks, vec![1, 3, 3]);
    }

    #[test]
    fn mark_inside_a_replaced_region_moves_to_its_start() {
        let mut marks = vec![2];
        let s = replace_at("abcdef", &[1], "bcd", "Z", &mut marks);
        assert_eq!(s, "aZef");
        assert_eq!(marks, vec![1, 1]);
    }
}
