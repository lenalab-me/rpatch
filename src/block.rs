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
//!
//! Markers are 7 or more characters long. A block opened with N `<` is closed only by
//! markers of length N, so lines with markers of another length are plain content.

use crate::input::{Edit, Target};

const MIN_MARKER: usize = 7;

/// The markers of one block, all of the same length.
struct Markers {
    open: String,
    close_prefix: String,
    to: String,
    sep: String,
    close: String,
}

impl Markers {
    fn new(n: usize) -> Self {
        Markers {
            open: "<".repeat(n),
            close_prefix: ">".repeat(n),
            to: format!("{} TO", "<".repeat(n)),
            sep: "=".repeat(n),
            close: format!("{} NEW", ">".repeat(n)),
        }
    }

    fn is_marker(&self, line: &str) -> bool {
        line == self.sep
            || line
                .strip_prefix(&self.open)
                .or_else(|| line.strip_prefix(&self.close_prefix))
                .is_some_and(|rest| rest.starts_with(' '))
    }

    fn is_opener(&self, line: &str) -> bool {
        line.strip_prefix(&self.open)
            .is_some_and(|rest| rest == " FROM" || rest.starts_with(" OLD"))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Old,
    From,
    To,
    New,
}

impl Part {
    fn name(self) -> &'static str {
        match self {
            Part::Old => "OLD",
            Part::From => "FROM",
            Part::To => "TO",
            Part::New => "NEW",
        }
    }
}

enum Opener {
    Old(usize),
    From,
}

/// Parses `<<<<<<< OLD`, `<<<<<<< OLD count=N` or `<<<<<<< FROM` (7 or more `<`).
/// `None` if the line does not start with 7 `<`.
fn parse_opener(n: usize, line: &str) -> Option<Result<(usize, Opener), String>> {
    let len = line.bytes().take_while(|&b| b == b'<').count();
    if len < MIN_MARKER {
        return None;
    }
    let open = &line[..len];
    let rest = &line[len..];
    let parsed = if rest == " FROM" {
        Some(Opener::From)
    } else if let Some(count) = rest.strip_prefix(" OLD") {
        if count.is_empty() {
            Some(Opener::Old(1))
        } else {
            count
                .strip_prefix(" count=")
                .and_then(|c| c.trim().parse().ok())
                .map(Opener::Old)
        }
    } else {
        None
    };
    Some(parsed.map(|o| (len, o)).ok_or_else(|| {
        format!(
            "line {n}: expected `{open} OLD`, `{open} OLD count=N` or `{open} FROM`, found `{line}`"
        )
    }))
}

struct Block {
    markers: Markers,
    part: Part,
    /// Line of the opener.
    start: usize,
    /// Line where the current part starts.
    part_start: usize,
    count: usize,
    range: bool,
    first: String,
    second: String,
    new: String,
}

impl Block {
    /// Error for a marker line that does not fit the current part.
    fn unexpected(&self, n: usize, line: &str) -> String {
        let m = &self.markers;
        let mut msg = format!(
            "line {n}: unexpected `{line}` inside the {} part that starts at line {}",
            self.part.name(),
            self.part_start
        );
        let missing = match self.part {
            Part::Old | Part::To if line == m.close => Some(m.sep.as_str()),
            Part::Old | Part::To if m.is_opener(line) => Some(m.sep.as_str()),
            Part::New if m.is_opener(line) || line == m.sep => Some(m.close.as_str()),
            _ => None,
        };
        if let Some(missing) = missing {
            msg += &format!("\n  hint: missing `{missing}` before line {n}?");
        }
        let longer = Markers::new(m.open.len() + 2);
        msg += &format!(
            "\n  hint: if the text itself has this line, use longer markers (`{} OLD`, `{}`, `{}`) or JSON",
            longer.open, longer.sep, longer.close
        );
        msg
    }
}

pub fn parse(text: &str) -> Result<Vec<Edit>, String> {
    let mut edits = Vec::new();
    let mut file: Option<String> = None;
    let mut block: Option<Block> = None;

    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let Some(b) = block.as_mut() else {
            if let Some(path) = line.strip_prefix("=== ") {
                let path = path.trim();
                if path.is_empty() {
                    return Err(format!("line {n}: missing file name after `===`"));
                }
                file = Some(path.to_string());
            } else if let Some(opener) = parse_opener(n, line) {
                let (len, opener) = opener?;
                if file.is_none() {
                    return Err(format!(
                        "line {n}: `{line}` comes before any `=== <file>` line"
                    ));
                }
                let (part, count) = match opener {
                    Opener::Old(count) => (Part::Old, count),
                    Opener::From => (Part::From, 1),
                };
                block = Some(Block {
                    markers: Markers::new(len),
                    part,
                    start: n,
                    part_start: n,
                    count,
                    range: part == Part::From,
                    first: String::new(),
                    second: String::new(),
                    new: String::new(),
                });
            } else if !line.trim().is_empty() {
                let mut msg = format!(
                    "line {n}: expected `=== <file>`, `<<<<<<< OLD` or `<<<<<<< FROM`, found `{line}`"
                );
                if line.starts_with(['<', '=', '>']) {
                    msg += "\n  hint: markers need at least 7 characters, e.g. `<<<<<<< OLD`";
                }
                return Err(msg);
            }
            continue;
        };

        let m = &b.markers;
        match b.part {
            Part::Old | Part::To if line == m.sep => {
                b.part = Part::New;
                b.part_start = n;
            }
            Part::From if line == m.to => {
                b.part = Part::To;
                b.part_start = n;
            }
            Part::From if line == m.sep => {
                return Err(format!(
                    "line {n}: a FROM part needs `{}` before `{}` (the block starts at line {})",
                    m.to, m.sep, b.start
                ));
            }
            Part::New if line == m.close => {
                let b = block.take().expect("inside a block");
                let target = if b.range {
                    Target::Range {
                        from: b.first,
                        to: b.second,
                    }
                } else {
                    Target::Text(b.first)
                };
                edits.push(Edit {
                    file: file.clone().unwrap_or_default(),
                    target,
                    new: b.new,
                    count: b.count,
                    lines: true,
                    source_line: Some(b.start),
                });
            }
            _ if m.is_marker(line) => return Err(b.unexpected(n, line)),
            Part::Old | Part::From => push_line(&mut b.first, line),
            Part::To => push_line(&mut b.second, line),
            Part::New => push_line(&mut b.new, line),
        }
    }

    if let Some(b) = block {
        let m = &b.markers;
        let missing = match b.part {
            Part::From => format!("`{}`, `{}` and `{}`", m.to, m.sep, m.close),
            Part::Old | Part::To => format!("`{}` and `{}`", m.sep, m.close),
            Part::New => format!("`{}`", m.close),
        };
        return Err(format!(
            "line {}: block is not closed (missing {missing})",
            b.start
        ));
    }
    Ok(edits)
}

fn push_line(buf: &mut String, line: &str) {
    buf.push_str(line);
    buf.push('\n');
}
