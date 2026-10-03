//! The block input format, which needs no escaping:
//!
//! ```text
//! === path/to/file
//! <<<<<<< OLD            (or `<<<<<<< OLD count=2`)
//! lines to replace
//! =======
//! replacement lines
//! >>>>>>> NEW
//! <<<<<<< OLD part       (text that may be any part of a line, without its line break)
//! part of a line
//! =======
//! replacement text
//! >>>>>>> NEW
//! <<<<<<< FROM           (range: from these lines up to, not including, the TO lines)
//! first lines of the range
//! <<<<<<< TO
//! lines after the range
//! =======
//! replacement lines
//! >>>>>>> NEW
//! <<<<<<< AFTER          (or BEFORE; insert lines next to each line containing this
//! part of a line         text, which may be any part of the line)
//! =======
//! lines to insert
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
        line.strip_prefix(&self.open).is_some_and(|rest| {
            rest == " FROM"
                || [" OLD", " BEFORE", " AFTER"]
                    .iter()
                    .any(|w| rest.starts_with(w))
        })
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

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Replace,
    /// `OLD part`: the text may be any part of a line.
    ReplacePart,
    Range,
    Insert {
        after: bool,
    },
}

impl Kind {
    /// The name of the block's first part, as written in its opener.
    fn first_part(self) -> &'static str {
        match self {
            Kind::Replace | Kind::ReplacePart => "OLD",
            Kind::Range => "FROM",
            Kind::Insert { after: false } => "BEFORE",
            Kind::Insert { after: true } => "AFTER",
        }
    }
}

/// Parses an opener such as `<<<<<<< OLD`, `<<<<<<< AFTER count=2` or `<<<<<<< FROM`
/// (7 or more `<`) into (marker length, kind, count). `None` if the line does not
/// start with 7 `<`.
fn parse_opener(n: usize, line: &str) -> Option<Result<(usize, Kind, usize), String>> {
    let len = line.bytes().take_while(|&b| b == b'<').count();
    if len < MIN_MARKER {
        return None;
    }
    let open = &line[..len];
    let rest = &line[len..];
    // `None`: not this word. `Some(None)`: this word with a malformed count.
    let with_count = |word: &str| {
        let c = rest.strip_prefix(word)?;
        Some(if c.is_empty() {
            Some(1)
        } else {
            c.strip_prefix(" count=")
                .and_then(|c| c.trim().parse().ok())
        })
    };
    let parsed = if rest == " FROM" {
        Some((Kind::Range, 1))
    } else if let Some(c) = with_count(" OLD part") {
        c.map(|c| (Kind::ReplacePart, c))
    } else if let Some(c) = with_count(" OLD") {
        c.map(|c| (Kind::Replace, c))
    } else if let Some(c) = with_count(" BEFORE") {
        c.map(|c| (Kind::Insert { after: false }, c))
    } else if let Some(c) = with_count(" AFTER") {
        c.map(|c| (Kind::Insert { after: true }, c))
    } else {
        None
    };
    Some(parsed.map(|(kind, count)| (len, kind, count)).ok_or_else(|| {
        format!(
            "line {n}: expected `{open} OLD`, `{open} OLD part`, `{open} FROM`, `{open} BEFORE` or `{open} AFTER` (all but FROM may add ` count=N`), found `{line}`"
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
    kind: Kind,
    first: String,
    second: String,
    new: String,
    /// Lines that look like markers of another length: (line, text, length).
    odd: Vec<(usize, String, usize)>,
}

impl Block {
    /// Points at marker-like lines whose length differs from this block's markers,
    /// the likely cause when a block does not close where expected.
    fn length_hints(&self) -> String {
        self.odd
            .iter()
            .take(3)
            .map(|(n, line, len)| {
                format!(
                    "\n  hint: line {n} has `{line}` ({len} characters), but this block's markers are {} characters long",
                    self.markers.open.len()
                )
            })
            .collect()
    }

    /// Error for a marker line that does not fit the current part.
    fn unexpected(&self, n: usize, line: &str) -> String {
        let m = &self.markers;
        let mut msg = format!(
            "line {n}: unexpected `{line}` inside the {} part that starts at line {}",
            match self.part {
                Part::Old | Part::From => self.kind.first_part(),
                part => part.name(),
            },
            self.part_start
        );
        if !self.odd.is_empty() {
            return msg + &self.length_hints();
        }
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

pub fn parse(text: &str, default_file: Option<&str>) -> Result<Vec<Edit>, String> {
    let mut edits = Vec::new();
    let mut file: Option<String> = default_file.map(String::from);
    let mut block: Option<Block> = None;

    for (i, raw) in text.lines().enumerate() {
        // Marker lines are recognized without trailing whitespace, which is invisible
        // and easily left by an editor or a model. Content lines are kept as written.
        let line = raw.trim_end();
        let n = i + 1;
        let Some(b) = block.as_mut() else {
            if let Some(path) = line.strip_prefix("=== ") {
                let path = path.trim();
                if path.is_empty() {
                    return Err(format!("line {n}: missing file name after `===`"));
                }
                file = Some(path.to_string());
            } else if let Some(opener) = parse_opener(n, line) {
                let (len, kind, count) = opener?;
                if file.is_none() {
                    return Err(format!(
                        "line {n}: `{line}` comes before any `=== <file>` line"
                    ));
                }
                let part = if kind == Kind::Range {
                    Part::From
                } else {
                    Part::Old
                };
                block = Some(Block {
                    markers: Markers::new(len),
                    part,
                    start: n,
                    part_start: n,
                    count,
                    kind,
                    first: String::new(),
                    second: String::new(),
                    new: String::new(),
                    odd: Vec::new(),
                });
            } else if !line.trim().is_empty() {
                let mut msg = format!(
                    "line {n}: expected `=== <file>` or a block opener (`<<<<<<< OLD`, `OLD part`, `FROM`, `BEFORE` or `AFTER`), found `{line}`"
                );
                if line.starts_with(['<', '=', '>']) {
                    msg += "\n  hint: markers need at least 7 characters, e.g. `<<<<<<< OLD`";
                } else if ["--- ", "+++ ", "@@ ", "*** "]
                    .iter()
                    .any(|p| line.starts_with(p))
                {
                    msg += "\n  hint: this is not a diff or patch format; write the old and the new text as they are";
                } else if file.is_none() {
                    // Seen with `rpatch <<'A' && cat > f <<'B'` and the bodies written in the
                    // other order: rpatch then reads the file's content.
                    msg += "\n  hint: with several heredocs on one command line, the shell hands out the bodies in the order the `<<` appear on the line, so the body for rpatch must come first";
                }
                // Guessed formats are the usual cause, so show the real one.
                msg += "\n  the format is:\n    === path/to/file\n    <<<<<<< OLD\n    exact lines to replace\n    =======\n    replacement lines\n    >>>>>>> NEW\n  (`<<<<<<< OLD part` for part of a line; see `rpatch --help`)";
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
                // Seen when FROM is used like OLD, or its parts are written out of order.
                return Err(format!(
                    "line {n}: a FROM part needs `{}` before `{}` (the block starts at line {})\n  hint: to replace exactly the lines above, open the block with `{} OLD` instead\n  the FROM order is: FROM lines, `{}`, TO lines (none: to the end of the file), `{}`, new lines, `{}`",
                    m.to, m.sep, b.start, m.open, m.to, m.sep, m.close
                ));
            }
            Part::New if line == m.close => {
                let mut b = block.take().expect("inside a block");
                let part = b.kind == Kind::ReplacePart;
                if part {
                    // Part of a line: the line break after the last line is not part of it.
                    for text in [&mut b.first, &mut b.new] {
                        text.pop();
                    }
                }
                let target = match b.kind {
                    Kind::Replace | Kind::ReplacePart => Target::Text(b.first),
                    Kind::Range => Target::Range {
                        from: b.first,
                        to: b.second,
                    },
                    Kind::Insert { after } => Target::Insert {
                        anchor: b.first,
                        after,
                    },
                };
                edits.push(Edit {
                    file: file.clone().unwrap_or_default(),
                    target,
                    new: b.new,
                    count: b.count,
                    lines: true,
                    part,
                    source_line: Some(b.start),
                });
            }
            _ if m.is_marker(line) => return Err(b.unexpected(n, line)),
            part => {
                if let Some(len) = marker_like_len(line).filter(|&len| len != m.open.len()) {
                    b.odd.push((n, line.to_string(), len));
                }
                let buf = match part {
                    Part::Old | Part::From => &mut b.first,
                    Part::To => &mut b.second,
                    Part::New => &mut b.new,
                };
                push_line(buf, raw);
            }
        }
    }

    if let Some(b) = block {
        let m = &b.markers;
        let missing = match b.part {
            Part::From => format!("`{}`, `{}` and `{}`", m.to, m.sep, m.close),
            Part::Old | Part::To => format!("`{}` and `{}`", m.sep, m.close),
            Part::New => format!("`{}`", m.close),
        };
        let mut msg = format!(
            "line {}: block is not closed (missing {missing}){}",
            b.start,
            b.length_hints()
        );
        // The usual case: the input stops right after a part's marker, often a TO whose
        // lines were not read yet. The blocks before it were fine but are not applied.
        let empty = match b.part {
            Part::Old | Part::From => b.first.is_empty(),
            Part::To => b.second.is_empty(),
            Part::New => false,
        };
        if empty {
            let part = match b.part {
                Part::To => "TO",
                _ => b.kind.first_part(),
            };
            msg += &format!(
                "\n  hint: the input ends right after the {part} marker at line {}; write its lines (read the file first if they are not known yet)",
                b.part_start
            );
        }
        if !edits.is_empty() {
            let blocks = match edits.len() {
                1 => "the block".to_string(),
                n => format!("the {n} blocks"),
            };
            msg += &format!(
                "\n  note: {blocks} before line {} parsed, but nothing is applied until the whole input does; send them again with this one fixed",
                b.start
            );
        }
        return Err(msg);
    }
    Ok(edits)
}

/// The length of a line that looks like a marker of any length (`<<<<< OLD`, `=====`,
/// `>>>>> NEW`, ...).
fn marker_like_len(line: &str) -> Option<usize> {
    let c = *line.as_bytes().first()?;
    if !matches!(c, b'<' | b'=' | b'>') {
        return None;
    }
    let len = line.bytes().take_while(|&b| b == c).count();
    let rest = &line[len..];
    let shaped = match c {
        b'<' => {
            rest == " FROM"
                || rest == " TO"
                || [" OLD", " BEFORE", " AFTER"]
                    .iter()
                    .any(|w| rest.starts_with(w))
        }
        b'=' => rest.is_empty(),
        _ => rest == " NEW",
    };
    (shaped && len >= 3).then_some(len)
}

fn push_line(buf: &mut String, line: &str) {
    buf.push_str(line);
    buf.push('\n');
}
