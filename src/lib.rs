mod block;
mod diagnose;
mod input;
mod matcher;
mod text;
mod write;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, Permissions};
use std::io;
use std::path::{Path, PathBuf};

use similar::TextDiff;

use crate::input::{Edit, Target};
use crate::matcher::Span;
use crate::text::LineIndex;

const EXIT_MISMATCH: u8 = 1;
const EXIT_INPUT: u8 = 2;
const EXIT_IO: u8 = 3;

#[derive(Clone, Copy, Default)]
pub struct PatchOptions {
    pub diff: bool,
    pub dry_run: bool,
}

#[derive(Debug)]
pub struct PatchError {
    pub code: u8,
    pub message: String,
}

struct FileState {
    /// The path as first written in the input.
    display: String,
    path: PathBuf,
    original: String,
    content: String,
    perms: Permissions,
    /// The file started with a UTF-8 BOM, which is kept out of `original`/`content`.
    bom: bool,
    /// Number of hard links to the file.
    links: u64,
    applied: Vec<usize>,
    skipped: Vec<usize>,
    /// Byte ranges in `content` that the applied edits changed.
    changes: Vec<(usize, usize)>,
    /// Extra lines printed under the file's summary line.
    notes: Vec<String>,
    failed: bool,
}

impl FileState {
    /// Changed lines as "3", "5-7" and so on, merged and in order.
    fn changed_lines(&self) -> String {
        let c = &self.content;
        let idx = LineIndex::new(c);
        // A deletion at the very end points at the last line, not past it.
        let max = idx.line_of(c.len().saturating_sub(1));
        let mut ranges: Vec<(usize, usize)> = self
            .changes
            .iter()
            .map(|&(s, e)| {
                let first = idx.line_of(s).min(max);
                let last = if e > s { idx.line_of(e - 1) } else { first };
                (first, last.min(max))
            })
            .collect();
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (a, b) in ranges {
            match merged.last_mut() {
                Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        merged
            .iter()
            .map(|&(a, b)| {
                if a == b {
                    a.to_string()
                } else {
                    format!("{a}-{b}")
                }
            })
            .collect::<Vec<_>>()
            .join(",")
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

/// Applies the same input and options as the CLI, resolving paths against `cwd`.
/// Returns CLI output as text; it never writes to stdout or stderr.
pub fn apply(text: &str, opts: PatchOptions, cwd: &Path) -> Result<String, PatchError> {
    let edits = input::parse(text).map_err(|message| PatchError {
        code: EXIT_INPUT,
        message: format!("rpatch: {message}\n"),
    })?;

    let mut report = Report::default();
    let (mut files, file_of) = load_files(&edits, cwd, &mut report);
    apply_edits(&edits, &mut files, &file_of, &mut report);

    if report.failed {
        return Err(PatchError {
            code: if report.io { EXIT_IO } else { EXIT_MISMATCH },
            message: format!("{}\nrpatch: nothing written\n", report.lines.join("\n")),
        });
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
                prefix: if f.bom { "\u{feff}" } else { "" },
                perms: &f.perms,
            })
            .collect();
        if let Err(msg) = write::commit(&pending) {
            return Err(PatchError {
                code: EXIT_IO,
                message: format!("rpatch: {msg}\n"),
            });
        }
    }

    let mut out = String::new();
    for f in &files {
        let _ = writeln!(
            out,
            "M {}  edits={}  at_lines={}",
            f.display,
            f.applied.len(),
            f.changed_lines()
        );
        if f.links > 1 && f.content != f.original {
            let _ = writeln!(
                out,
                "  warning: {} had {} hard links; it is replaced via rename, so the other links keep the old content",
                f.display, f.links
            );
        }
        for note in &f.notes {
            let _ = writeln!(out, "  {note}");
        }
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
    Ok(out)
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
fn load_files(
    edits: &[Edit],
    cwd: &Path,
    report: &mut Report,
) -> (Vec<FileState>, Vec<Option<usize>>) {
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
        let loaded = fs::canonicalize(cwd.join(&e.file))
            .map_err(|err| load_failure(e, cwd, err))
            .and_then(|path| match by_path.get(&path) {
                Some(&k) => Ok(k),
                None => {
                    let (original, perms, bom, links) =
                        read_file(&path).map_err(|err| load_failure(e, cwd, err))?;
                    by_path.insert(path.clone(), files.len());
                    files.push(FileState {
                        display: e.file.clone(),
                        path,
                        content: original.clone(),
                        original,
                        perms,
                        bom,
                        links,
                        applied: Vec::new(),
                        skipped: Vec::new(),
                        changes: Vec::new(),
                        notes: Vec::new(),
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

fn load_failure(e: &Edit, cwd: &Path, err: io::Error) -> LoadFailure {
    let (message, details, io) = if err.kind() == io::ErrorKind::NotFound {
        (
            "file not found".to_string(),
            vec![format!("cwd: {}", cwd.display())],
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

/// Reads a file as UTF-8. Returns the text without a leading BOM, the permissions,
/// whether there was a BOM, and the number of hard links.
fn read_file(path: &Path) -> io::Result<(String, Permissions, bool, u64)> {
    let bytes = fs::read(path)?;
    let meta = fs::metadata(path)?;
    #[cfg(unix)]
    let links = std::os::unix::fs::MetadataExt::nlink(&meta);
    #[cfg(not(unix))]
    let links = 1;
    let mut text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not valid UTF-8 text"))?;
    let bom = text.starts_with('\u{feff}');
    if bom {
        text.drain(..'\u{feff}'.len_utf8());
    }
    Ok((text, meta.permissions(), bom, links))
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
        match matcher::find(&f.content, e) {
            Ok(spans) => {
                if matches!(e.target, Target::Range { .. }) {
                    f.notes.push(range_note(i, &f.content, &spans[0]));
                }
                f.content = splice(&f.content, &spans, &mut f.changes);
                f.applied.push(i);
            }
            Err(mut m) => {
                f.failed = true;
                if !f.applied.is_empty() && matcher::find(&f.original, e).is_ok() {
                    m.details.push(format!(
                        "note: this edit matches the file as it was before {}; an earlier edit in this batch changed that text",
                        edit_list(&f.applied)
                    ));
                }
                report.error(format!("{}: {}", e.label(i), m.headline), m.details);
            }
        }
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

/// "edits[0] replaced lines 2-4 (3 lines) with 1 line", so a range edit's extent is visible.
fn range_note(i: usize, content: &str, span: &Span) -> String {
    let idx = LineIndex::new(content);
    let first = idx.line_of(span.start);
    let last = idx.line_of(span.end.saturating_sub(1).max(span.start));
    let range = if first == last {
        format!("line {first}")
    } else {
        format!("lines {first}-{last}")
    };
    format!(
        "edits[{i}] replaced {range} ({}) with {}",
        plural(last + 1 - first, "line"),
        plural(span.new.lines().count(), "line")
    )
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Replaces each span (non-overlapping, ascending). Existing `changes` are moved to
/// their new offsets. For each span, the part of the new text that differs from the
/// text it replaced (not counting identical leading and trailing lines) is added.
fn splice(content: &str, spans: &[Span], changes: &mut Vec<(usize, usize)>) -> String {
    let mut out = String::with_capacity(content.len());
    let mut new_starts = Vec::with_capacity(spans.len());
    let mut last = 0;
    for s in spans {
        out.push_str(&content[last..s.start]);
        new_starts.push(out.len());
        out.push_str(&s.new);
        last = s.end;
    }
    out.push_str(&content[last..]);

    let remap = |m: usize| {
        let k = spans.partition_point(|s| s.start <= m);
        if k == 0 {
            return m;
        }
        let (s, ns) = (&spans[k - 1], new_starts[k - 1]);
        if m < s.end {
            ns
        } else {
            m - s.end + ns + s.new.len()
        }
    };
    for c in changes.iter_mut() {
        *c = (remap(c.0), remap(c.1));
    }
    for (s, &ns) in spans.iter().zip(&new_starts) {
        let (pre, suf) = common_lines(&content[s.start..s.end], &s.new);
        changes.push((ns + pre, ns + s.new.len() - suf));
    }
    out
}

/// Byte lengths of the whole lines that `old` and `new` share at the start and at the
/// end (the end part never overlaps the start part).
fn common_lines(old: &str, new: &str) -> (usize, usize) {
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let p = a
        .iter()
        .zip(&b)
        .take_while(|(x, y)| x == y && x.ends_with('\n'))
        .count();
    let s = a[p..]
        .iter()
        .rev()
        .zip(b[p..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let pre = b[..p].iter().map(|l| l.len()).sum();
    let suf = b[b.len() - s..].iter().map(|l| l.len()).sum();
    (pre, suf)
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

    fn span(start: usize, end: usize, new: &str) -> Span {
        Span {
            start,
            end,
            new: new.into(),
        }
    }

    #[test]
    fn changes_follow_later_edits() {
        let mut changes = vec![];
        let s = splice("aXbXc", &[span(1, 2, "YY"), span(3, 4, "YY")], &mut changes);
        assert_eq!(s, "aYYbYYc");
        assert_eq!(changes, vec![(1, 3), (4, 6)]);
        // Delete "b" (offset 3): the later range shifts left by one, and an empty
        // range is added where "b" was.
        let s = splice(&s, &[span(3, 4, "")], &mut changes);
        assert_eq!(s, "aYYYYc");
        assert_eq!(changes, vec![(1, 3), (3, 5), (3, 3)]);
    }

    #[test]
    fn change_inside_a_replaced_region_moves_to_its_start() {
        let mut changes = vec![(2, 3)];
        let s = splice("abcdef", &[span(1, 4, "Z")], &mut changes);
        assert_eq!(s, "aZef");
        assert_eq!(changes, vec![(1, 1), (1, 2)]);
    }

    #[test]
    fn identical_context_lines_are_not_changes() {
        // Context line kept, next line deleted: nothing inserted, at the deleted line.
        assert_eq!(common_lines("ctx\ndel\n", "ctx\n"), (4, 0));
        // Only the middle line differs.
        assert_eq!(common_lines("a\nb\nc\n", "a\nB\nc\n"), (2, 2));
        // A partial last line is never a shared start line.
        assert_eq!(common_lines("ab", "ab c"), (0, 0));
    }
}
