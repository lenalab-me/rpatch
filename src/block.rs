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
//! The closing line may repeat the opener's `part` and `count=N` (`>>>>>>> NEW part`).

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

    /// The words after `NEW` on a closing line such as `>>>>>>> NEW part` or
    /// `>>>>>>> NEW count=2`, written to mirror the opener: (`part`, count). `None` if
    /// the line is not a closing line with such words.
    fn closer_words(&self, line: &str) -> Option<(bool, Option<usize>)> {
        let rest = line.strip_prefix(&self.close)?.strip_prefix(' ')?;
        let (mut part, mut count) = (false, None);
        for word in rest.split_whitespace() {
            if word == "part" && !part {
                part = true;
            } else if let Some(c) = word.strip_prefix("count=").and_then(|c| c.parse().ok())
                && count.is_none()
            {
                count = Some(c);
            } else {
                return None;
            }
        }
        Some((part, count))
    }

    fn is_closer(&self, line: &str) -> bool {
        line == self.close || self.closer_words(line).is_some()
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
    let expected = || {
        let mut msg = format!(
            "line {n}: expected `{open} OLD`, `{open} OLD part`, `{open} FROM`, `{open} BEFORE` or `{open} AFTER` (all but FROM may add ` count=N`), found `{line}`"
        );
        // A known word followed by something else: the text of the block was put on the opener line.
        if [" OLD ", " BEFORE ", " AFTER ", " FROM "]
            .iter()
            .any(|w| rest.starts_with(w))
        {
            msg += "\n  hint: the opener line takes only `part` and `count=N`; start the text on the next line (a file goes in `=== <file>` or `rpatch FILE`)";
        }
        msg
    };
    // `None`: not this word. `Some(Err)`: this word followed by something other than a count.
    let with_count = |word: &str| {
        let c = rest.strip_prefix(word)?;
        Some(if c.is_empty() {
            Ok(1)
        } else if let Some(v) = c.strip_prefix(" count=") {
            v.trim().parse().map_err(|_| {
                let mut msg = format!(
                    "line {n}: `count=` needs a whole number such as `count=2`, found `{line}`"
                );
                if v.trim_end().ends_with(" part") {
                    msg +=
                        "\n  hint: write `part` before `count=N`, as in `<<<<<<< OLD part count=2`";
                }
                msg
            })
        } else {
            Err(expected())
        })
    };
    let parsed = if rest == " FROM" {
        Ok((Kind::Range, 1))
    } else if rest.starts_with(" FROM ") {
        Err(format!(
            "line {n}: FROM takes no count, since it has to match once; found `{line}`"
        ))
    } else if let Some(c) = with_count(" OLD part") {
        c.map(|c| (Kind::ReplacePart, c))
    } else if let Some(c) = with_count(" OLD") {
        c.map(|c| (Kind::Replace, c))
    } else if let Some(c) = with_count(" BEFORE") {
        c.map(|c| (Kind::Insert { after: false }, c))
    } else if let Some(c) = with_count(" AFTER") {
        c.map(|c| (Kind::Insert { after: true }, c))
    } else {
        Err(expected())
    };
    Some(parsed.map(|(kind, count)| (len, kind, count)))
}

struct Block {
    markers: Markers,
    /// The opener line, for errors.
    opener: String,
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

    /// Whether the words after `NEW` on a closing line say what the opener says. Words
    /// that say something else have two readings, so they are not taken as the closer.
    /// A FROM opener takes no count, so neither does its closer.
    fn fits(&self, (part, count): (bool, Option<usize>)) -> bool {
        (!part || self.kind == Kind::ReplacePart)
            && match count {
                None => true,
                Some(_) if self.kind == Kind::Range => false,
                Some(c) => c == self.count,
            }
    }

    /// Error for a closing line whose words after `NEW` say something the opener does not.
    fn mismatched_closer(
        &self,
        n: usize,
        line: &str,
        (part, count): (bool, Option<usize>),
    ) -> String {
        let m = &self.markers;
        let mut msg = format!(
            "line {n}: the closing line `{line}` does not match the opener `{}` at line {}",
            self.opener, self.start
        );
        let word = self.kind.first_part();
        if part && !matches!(self.kind, Kind::Replace | Kind::ReplacePart) {
            msg += &format!(
                "\n  hint: `part` is only for `{} OLD part`; close the {word} block with `{}`",
                m.open, m.close
            );
        } else if count.is_some() && self.kind == Kind::Range {
            msg += &format!(
                "\n  hint: FROM takes no count; close the block with `{}`",
                m.close
            );
        } else {
            // The opener decides what the block does: write the fixed opener out.
            let part = if part || self.kind == Kind::ReplacePart {
                " part"
            } else {
                ""
            };
            let count = match count.unwrap_or(self.count) {
                1 => String::new(),
                c => format!(" count={c}"),
            };
            msg += &format!(
                "\n  hint: the opener decides what the block does: open it with `{} {word}{part}{count}` if that is meant, and close it with `{}`",
                m.open, m.close
            );
        }
        msg + &self.longer_hint()
    }

    /// Error for a marker line that does not fit the current part. `next` is the next
    /// line that is not blank (`None`: the input ends).
    fn unexpected(&self, n: usize, line: &str, next: Option<&str>) -> String {
        let m = &self.markers;
        let mut msg = format!(
            "line {n}: unexpected `{line}` inside the {} part that starts at line {}",
            match self.part {
                Part::Old | Part::From => self.kind.first_part(),
                part => part.name(),
            },
            self.part_start
        );
        msg += &self.length_hints();
        let missing = match self.part {
            Part::Old | Part::To if m.is_closer(line) => Some(m.sep.as_str()),
            Part::Old | Part::To if m.is_opener(line) => Some(m.sep.as_str()),
            // A second separator where the closing line goes: at the end of the input, or
            // right before the closing line. Not accepted, since a NEW text may have a line
            // of `=` (a Markdown heading's underline).
            Part::New if line == m.sep && next.is_none() => {
                msg += &format!(
                    "\n  hint: a block ends with `{}`, not `{}`; write it in place of line {n}?",
                    m.close, m.sep
                );
                None
            }
            Part::New if line == m.sep && next.is_some_and(|l| m.is_closer(l)) => {
                msg += &format!(
                    "\n  hint: the NEW part already started at line {}; remove the second `{}` at line {n}?",
                    self.part_start, m.sep
                );
                None
            }
            Part::New if m.is_opener(line) || line == m.sep => Some(m.close.as_str()),
            _ => None,
        };
        if let Some(missing) = missing {
            msg += &format!("\n  hint: missing `{missing}` before line {n}?");
        }
        msg + &self.longer_hint()
    }

    /// For a marker line meant as text: longer markers make it plain content. Left out
    /// when lines with markers of another length already explain the error.
    fn longer_hint(&self) -> String {
        if !self.odd.is_empty() {
            return String::new();
        }
        let longer = Markers::new(self.markers.open.len() + 2);
        format!(
            "\n  hint: if the text itself has this line, use longer markers (`{} OLD`, `{}`, `{}`) or JSON",
            longer.open, longer.sep, longer.close
        )
    }
}

pub fn parse(text: &str, default_file: Option<&str>) -> Result<Vec<Edit>, String> {
    let mut edits = Vec::new();
    // The files the next block applies to: consecutive `=== <file>` lines make a group, and
    // each block after them is applied to every file in it (otherwise only the last file
    // would be written, with exit status 0).
    let mut files: Vec<String> = default_file.into_iter().map(String::from).collect();
    let mut block: Option<Block> = None;
    // Whether a `=== <file>` line came yet: until then, blocks go to `default_file`.
    let mut file_line = false;
    // Whether a block came after the last `=== <file>` line, which ends its group.
    let mut block_since_file_line = false;
    let mut blocks = 0;

    for (i, raw) in text.lines().enumerate() {
        // Marker lines are recognized without trailing whitespace, which is invisible
        // and easily left by an editor or a model. Content lines are kept as written.
        let line = raw.trim_end();
        let n = i + 1;
        let Some(b) = block.as_mut() else {
            // `=== ` with no name lost its space to trim_end.
            if line == "===" {
                return Err(format!("line {n}: missing file name after `===`"));
            }
            let path = line
                .strip_prefix("=== ")
                .map(str::trim)
                .or_else(|| loose_file_line(text, i, line));
            if let Some(path) = path {
                if let Some(arg) =
                    default_file.filter(|&arg| !file_line && edits.is_empty() && arg != path)
                {
                    return Err(format!(
                        "line {n}: the file argument `{arg}` is not used, since the first block comes after `=== {path}`\n  hint: drop the file argument, or put its blocks before the first `=== ` line"
                    ));
                }
                if !file_line || block_since_file_line {
                    files.clear();
                }
                files.push(path.to_string());
                file_line = true;
                block_since_file_line = false;
            } else if let Some(opener) = parse_opener(n, line) {
                let (len, kind, count) = opener?;
                if files.is_empty() {
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
                    opener: line.to_string(),
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
                } else if text.lines().skip(i + 1).any(|l| {
                    let l = l.trim_end();
                    l.starts_with("=== ") || parse_opener(0, l).is_some()
                }) {
                    // A comment or a sentence written around the blocks.
                    msg += "\n  hint: outside blocks, only `=== <file>` lines and blank lines are allowed; remove this line";
                } else if files.is_empty() {
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
            Part::New if m.is_closer(line) => {
                if let Some(words) = m.closer_words(line).filter(|&w| !b.fits(w)) {
                    return Err(b.mismatched_closer(n, line, words));
                }
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
                for file in &files {
                    edits.push(Edit {
                        file: file.clone(),
                        target: target.clone(),
                        new: b.new.clone(),
                        count: b.count,
                        lines: true,
                        part,
                        source_line: Some(b.start),
                    });
                }
                blocks += 1;
                block_since_file_line = true;
            }
            _ if m.is_marker(line) => {
                let next = text
                    .lines()
                    .skip(i + 1)
                    .map(str::trim_end)
                    .find(|l| !l.is_empty());
                return Err(b.unexpected(n, line, next));
            }
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
        if blocks > 0 {
            let blocks = match blocks {
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

/// A file line written without its `=== `, as a bare path or a diff header (`--- path`),
/// right before a block opener. Its meaning is plain, and refusing it makes agents give up
/// on rpatch, so it is read as `=== path`.
fn loose_file_line<'a>(text: &str, i: usize, line: &'a str) -> Option<&'a str> {
    let path = line.strip_prefix("--- ").unwrap_or(line).trim();
    if path.is_empty()
        || path.contains(char::is_whitespace)
        || path.starts_with(['<', '=', '>', '#'])
    {
        return None;
    }
    let next = text.lines().nth(i + 1)?;
    parse_opener(0, next.trim_end()).map(|_| path)
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
        _ => rest == " NEW" || rest.starts_with(" NEW "),
    };
    (shaped && len >= 3).then_some(len)
}

fn push_line(buf: &mut String, line: &str) {
    buf.push_str(line);
    buf.push('\n');
}
