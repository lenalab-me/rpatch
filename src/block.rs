//! The block input format, which needs no escaping:
//!
//! ```text
//! === path/to/file
//! <<<<<<< OLD            (or `<<<<<<< OLD count=2`)
//! lines to replace
//! =======
//! replacement lines
//! >>>>>>> NEW
//! <<<<<<< FROM           (range: from these lines up to, not including, the TO lines)
//! first lines of the range
//! <<<<<<< TO
//! lines after the range
//! =======
//! replacement lines
//! >>>>>>> NEW
//! ```

use crate::input::{Edit, Target};

const OLD: &str = "<<<<<<< OLD";
const FROM: &str = "<<<<<<< FROM";
const TO: &str = "<<<<<<< TO";
const SEP: &str = "=======";
const NEW: &str = ">>>>>>> NEW";

#[derive(Clone, Copy, PartialEq)]
enum State {
    Top,
    Old,
    From,
    To,
    New,
}

fn is_marker(line: &str) -> bool {
    line.starts_with("<<<<<<< ") || line.starts_with(">>>>>>> ") || line == SEP
}

/// `""` → 1, `" count=N"` → N.
fn parse_count(rest: &str) -> Option<usize> {
    if rest.is_empty() {
        return Some(1);
    }
    rest.strip_prefix(" count=")?.trim().parse().ok()
}

pub fn parse(text: &str) -> Result<Vec<Edit>, String> {
    let mut edits = Vec::new();
    let mut file: Option<String> = None;
    let mut state = State::Top;
    let (mut first, mut second, mut new) = (String::new(), String::new(), String::new());
    let mut count = 1;
    let mut range = false;
    let mut start = 0;

    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let unexpected = |inside: &str| {
            format!(
                "line {n}: unexpected `{line}` inside the {inside} block that starts at line {start}"
            )
        };
        match state {
            State::Top => {
                if let Some(path) = line.strip_prefix("=== ") {
                    let path = path.trim();
                    if path.is_empty() {
                        return Err(format!("line {n}: missing file name after `===`"));
                    }
                    file = Some(path.to_string());
                    continue;
                }
                let next = if let Some(rest) = line.strip_prefix(OLD) {
                    count = parse_count(rest).ok_or_else(|| {
                        format!("line {n}: expected `{OLD}` or `{OLD} count=N`, found `{line}`")
                    })?;
                    range = false;
                    State::Old
                } else if line == FROM {
                    count = 1;
                    range = true;
                    State::From
                } else if line.trim().is_empty() {
                    continue;
                } else {
                    return Err(format!(
                        "line {n}: expected `=== <file>`, `{OLD}` or `{FROM}`, found `{line}`"
                    ));
                };
                if file.is_none() {
                    return Err(format!(
                        "line {n}: `{line}` comes before any `=== <file>` line"
                    ));
                }
                start = n;
                state = next;
            }
            State::Old => match line {
                SEP => state = State::New,
                _ if is_marker(line) => return Err(unexpected("OLD")),
                _ => push_line(&mut first, line),
            },
            State::From => match line {
                TO => state = State::To,
                SEP => {
                    return Err(format!(
                        "line {n}: a FROM block needs `{TO}` before `{SEP}` (block starts at line {start})"
                    ));
                }
                _ if is_marker(line) => return Err(unexpected("FROM")),
                _ => push_line(&mut first, line),
            },
            State::To => match line {
                SEP => state = State::New,
                _ if is_marker(line) => return Err(unexpected("TO")),
                _ => push_line(&mut second, line),
            },
            State::New => match line {
                NEW => {
                    let target = if range {
                        Target::Range {
                            from: std::mem::take(&mut first),
                            to: std::mem::take(&mut second),
                        }
                    } else {
                        Target::Text(std::mem::take(&mut first))
                    };
                    edits.push(Edit {
                        file: file.clone().unwrap_or_default(),
                        target,
                        new: std::mem::take(&mut new),
                        count,
                        lines: true,
                        source_line: Some(start),
                    });
                    state = State::Top;
                }
                _ if is_marker(line) => return Err(unexpected("NEW")),
                _ => push_line(&mut new, line),
            },
        }
    }

    if state != State::Top {
        return Err(format!(
            "line {start}: block is not closed (missing `{NEW}`)"
        ));
    }
    Ok(edits)
}

fn push_line(buf: &mut String, line: &str) {
    buf.push_str(line);
    buf.push('\n');
}
