use serde::Deserialize;

use crate::block;

pub enum Target {
    /// Replace every match of the text.
    Text(String),
    /// Replace from the start of `from` up to (not including) the first `to` after it.
    /// An empty `to` is the end of the file.
    Range { from: String, to: String },
    /// Insert `new` as whole lines before or after each line that contains `anchor`
    /// (which may be any part of the line). Block format only.
    Insert { anchor: String, after: bool },
}

pub struct Edit {
    pub file: String,
    pub target: Target,
    pub new: String,
    pub count: usize,
    /// Block format: texts are whole lines, matched only at line starts, and follow
    /// the file's line endings.
    pub lines: bool,
    /// Block format `OLD part`: the text may be any part of a line (matched anywhere,
    /// like JSON "old"), still following the file's line endings.
    pub part: bool,
    /// Line in the input where the edit starts (block format only).
    pub source_line: Option<usize>,
}

impl Edit {
    pub fn label(&self, i: usize) -> String {
        match self.source_line {
            Some(n) => format!("edits[{i}] {} (input line {n})", self.file),
            None => format!("edits[{i}] {}", self.file),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonInput {
    edits: Vec<JsonEdit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonEdit {
    file: String,
    old: Option<String>,
    from: Option<String>,
    to: Option<String>,
    new: String,
    count: Option<usize>,
}

/// Parses JSON (input starting with `{`) or the block format. `file` is the file for
/// blocks before any `=== <file>` line.
pub fn parse(text: &str, file: Option<&str>) -> Result<Vec<Edit>, String> {
    let edits = if text.trim_start().starts_with('{') {
        if let Some(file) = file {
            return Err(format!(
                "a file argument (`{file}`) works only with the block format; JSON edits name their files in \"file\""
            ));
        }
        parse_json(text)?
    } else {
        block::parse(text, file)?
    };
    validate(&edits)?;
    Ok(edits)
}

fn parse_json(text: &str) -> Result<Vec<Edit>, String> {
    let input: JsonInput = serde_json::from_str(text).map_err(|e| json_error(text, &e))?;
    input
        .edits
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            let target = match (e.old, e.from, e.to) {
                (Some(old), None, None) => Target::Text(old),
                (None, Some(from), Some(to)) => {
                    if e.count.is_some() {
                        return Err(format!(
                            "edits[{i}]: \"count\" cannot be used with \"from\"/\"to\""
                        ));
                    }
                    Target::Range { from, to }
                }
                _ => {
                    return Err(format!(
                        "edits[{i}]: give either \"old\", or both \"from\" and \"to\""
                    ));
                }
            };
            Ok(Edit {
                file: e.file,
                target,
                new: e.new,
                count: e.count.unwrap_or(1),
                lines: false,
                part: false,
                source_line: None,
            })
        })
        .collect()
}

const CONTEXT: usize = 40;

fn json_error(text: &str, e: &serde_json::Error) -> String {
    let mut msg = format!("invalid JSON input: {e}");
    let (line_no, column) = match (e.line(), e.column()) {
        // A raw line break inside a string is reported at column 0 of the next line;
        // point at the end of the line that has the break instead.
        (l, 0) if l > 1 => (l - 1, usize::MAX),
        lc => lc,
    };
    if let Some(line) = line_no.checked_sub(1).and_then(|l| text.lines().nth(l)) {
        // serde_json counts columns in bytes, 1-based.
        let at = floor_boundary(line, column.saturating_sub(1).min(line.len()));
        let start = floor_boundary(line, at.saturating_sub(CONTEXT));
        let end = floor_boundary(line, (at + CONTEXT).min(line.len()));
        let lead = if start > 0 { "…" } else { "" };
        let tail = if end < line.len() { "…" } else { "" };
        msg += &format!("\n  {lead}{}{tail}", &line[start..end]);
        let caret = lead.chars().count() + line[start..at].chars().count();
        msg += &format!("\n  {}^", " ".repeat(caret));
    }
    let what = e.to_string();
    let escape = what.contains("invalid escape");
    let control = what.contains("control character");
    if escape {
        msg += "\n  hint: JSON only has the escapes \\\" \\\\ \\/ \\b \\f \\n \\r \\t \\uXXXX; write ' as is (not \\')";
    }
    if control {
        msg += "\n  hint: line breaks and tabs inside JSON strings must be written as \\n and \\t";
    }
    if escape || control {
        msg +=
            "\n  hint: for multi-line text, the block format needs no escaping (see rpatch --help)";
    }
    msg
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn validate(edits: &[Edit]) -> Result<(), String> {
    if edits.is_empty() {
        return Err("no edits in input (see rpatch --help)".into());
    }
    for (i, e) in edits.iter().enumerate() {
        let label = e.label(i);
        if e.file.is_empty() {
            return Err(format!("{label}: \"file\" is empty"));
        }
        match &e.target {
            Target::Text(old) if old.is_empty() => {
                return Err(format!(
                    "{label}: \"old\" is empty (creating files is not supported)"
                ));
            }
            Target::Text(old) if *old == e.new => {
                return Err(format!(
                    "{label}: \"old\" and \"new\" are identical, so the edit would change nothing"
                ));
            }
            Target::Insert { anchor, after } if anchor.trim().is_empty() => {
                let which = if *after { "AFTER" } else { "BEFORE" };
                return Err(format!("{label}: the {which} text is empty"));
            }
            Target::Insert { .. } if e.new.is_empty() => {
                return Err(format!(
                    "{label}: nothing to insert (the NEW part is empty)"
                ));
            }
            Target::Range { from, .. } if from.is_empty() => {
                return Err(format!("{label}: \"from\" must not be empty"));
            }
            _ => {}
        }
        if e.count == 0 {
            return Err(format!("{label}: \"count\" must be at least 1"));
        }
    }
    Ok(())
}
