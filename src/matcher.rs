//! Finds where an edit applies in a file's current content.

use std::borrow::Cow;

use crate::diagnose;
use crate::input::{Edit, Target};
use crate::text::{LineIndex, gutter, gutter_copyable, line_list};

/// `content[start..end]` is to be replaced with `new`.
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub new: String,
}

pub struct Mismatch {
    pub headline: String,
    pub details: Vec<String>,
}

/// Returns the spans to replace, ascending and non-overlapping.
pub fn find(content: &str, edit: &Edit) -> Result<Vec<Span>, Mismatch> {
    let crlf = edit.lines && content.contains("\r\n");
    let nl = if crlf { "\r\n" } else { "\n" };
    let new = adapt(&edit.new, crlf);
    match &edit.target {
        Target::Text(old) => {
            let old = adapt(old, crlf);
            // OLD part matches anywhere, like JSON; plain OLD only at line starts.
            let whole_lines = edit.lines && !edit.part;
            let mut spans: Vec<Span> = occurrences(content, &old, whole_lines)
                .into_iter()
                .map(|p| Span {
                    start: p,
                    end: p + old.len(),
                    new: new.to_string(),
                })
                .collect();
            if whole_lines {
                spans.extend(last_line_match(content, &old, &new, nl, spans.last()));
            }
            let positions: Vec<usize> = spans.iter().map(|s| s.start).collect();
            let overlap = spans.windows(2).any(|w| w[1].start < w[0].end);
            if spans.len() == edit.count && !overlap {
                return Ok(spans);
            }
            let found = Found {
                positions: &positions,
                overlap,
            };
            let mut err = mismatch("old", content, &old, &found, edit.count, edit.lines);
            if edit.part {
                for d in &mut err.details {
                    *d = d.replace("<<<<<<< OLD count=", "<<<<<<< OLD part count=");
                }
            }
            Err(err)
        }
        Target::Insert { anchor, after } => {
            let field = if *after { "after" } else { "before" };
            let anchor = adapt(anchor, crlf);
            let text = anchor.strip_suffix(nl).unwrap_or(&anchor);
            // Part of a line is enough: inserting at line boundaries rewrites no line.
            let positions = occurrences(content, text, false);
            let found = Found::new(&positions, text.len());
            if positions.len() != edit.count || found.overlap {
                return Err(mismatch(field, content, text, &found, edit.count, true));
            }
            let points: Vec<usize> = positions
                .iter()
                .map(|&p| {
                    if *after {
                        line_end(content, p + text.len())
                    } else {
                        line_start(content, p)
                    }
                })
                .collect();
            if let Some(k) = points.windows(2).position(|w| w[0] == w[1]) {
                let idx = LineIndex::new(content);
                let line = idx.line_of(positions[k]);
                return Err(Mismatch {
                    headline: format!("\"{field}\" matches more than once on the same line"),
                    details: vec![
                        gutter(line, idx.line(content, line)),
                        format!("hint: change \"{field}\" so that it matches once per line"),
                    ],
                });
            }
            Ok(points
                .into_iter()
                .map(|p| {
                    // After a last line without a line break: add the break before the
                    // new lines, and keep the file without a final break.
                    let new =
                        if p == content.len() && !content.is_empty() && !content.ends_with('\n') {
                            format!("{nl}{}", new.strip_suffix(nl).unwrap_or(&new))
                        } else {
                            new.to_string()
                        };
                    Span {
                        start: p,
                        end: p,
                        new,
                    }
                })
                .collect())
        }
        Target::Range { from, to } => {
            let from = adapt(from, crlf);
            let to = adapt(to, crlf);
            // In the block format, FROM and TO match at line starts, and their last line
            // may be only the start of a line: they pick lines, they do not copy them.
            let (from_text, to_text) = if edit.lines {
                (
                    from.strip_suffix(nl).unwrap_or(&from),
                    to.strip_suffix(nl).unwrap_or(&to),
                )
            } else {
                (from.as_ref(), to.as_ref())
            };
            let starts = occurrences(content, from_text, edit.lines);
            let found = Found::new(&starts, from_text.len());
            if starts.len() != 1 {
                return Err(mismatch("from", content, &from, &found, 1, edit.lines));
            }
            let start = starts[0];
            let after = if edit.lines {
                line_end(content, start + from_text.len())
            } else {
                start + from_text.len()
            };
            let Some(&q) = occurrences(&content[after..], to_text, edit.lines).first() else {
                return Err(to_not_found(
                    content, &to, to_text, start, after, edit.lines,
                ));
            };
            let end = after + q;
            let kept = if edit.lines {
                &content[end..line_end(content, end + to_text.len())]
            } else {
                to_text
            };
            if new.ends_with(kept) {
                // An easy slip: TO is kept, so repeating it at the end of NEW doubles it.
                let idx = LineIndex::new(content);
                let line = idx.line_of(end);
                return Err(Mismatch {
                    headline: "\"new\" ends with the \"to\" text, which is kept, so it would appear twice"
                        .into(),
                    details: vec![
                        format!("\"to\" matched at line {line}, right after the range:"),
                        gutter(line, idx.line(content, line)),
                        "hint: remove the \"to\" lines from the end of \"new\"; to really repeat them, use OLD instead of FROM/TO".into(),
                    ],
                });
            }
            Ok(vec![Span {
                start,
                end,
                new: new.into_owned(),
            }])
        }
    }
}

/// Where a text matched, and whether any of the matches overlap.
struct Found<'a> {
    positions: &'a [usize],
    overlap: bool,
}

impl<'a> Found<'a> {
    fn new(positions: &'a [usize], len: usize) -> Self {
        Found {
            positions,
            overlap: positions.windows(2).any(|w| w[1] < w[0] + len),
        }
    }
}

fn line_start(content: &str, p: usize) -> usize {
    content[..p].rfind('\n').map_or(0, |i| i + 1)
}

/// The offset just past the line break of the line that contains `p`.
fn line_end(content: &str, p: usize) -> usize {
    content[p..].find('\n').map_or(content.len(), |i| p + i + 1)
}

fn at_line_start(content: &str, p: usize) -> bool {
    p == 0 || content.as_bytes()[p - 1] == b'\n'
}

fn adapt(s: &str, crlf: bool) -> Cow<'_, str> {
    if crlf {
        Cow::Owned(s.replace('\n', "\r\n"))
    } else {
        Cow::Borrowed(s)
    }
}

/// Every match, left to right, overlapping ones included: uniqueness has to count
/// them, or an ambiguous text would silently replace its first match. With
/// `line_start`, only matches at the start of a line count.
fn occurrences(content: &str, needle: &str, line_start: bool) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = content[from..].find(needle) {
        let p = from + i;
        if !line_start || at_line_start(content, p) {
            out.push(p);
        }
        from = p + content[p..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// Block texts end with a line break, so they cannot match a last line that has none.
/// Match that line without the break, and keep the file without a final break.
fn last_line_match(
    content: &str,
    old: &str,
    new: &str,
    nl: &str,
    previous: Option<&Span>,
) -> Option<Span> {
    if content.ends_with('\n') {
        return None;
    }
    let old = old.strip_suffix(nl)?;
    if old.is_empty() || !content.ends_with(old) {
        return None;
    }
    let p = content.len() - old.len();
    if !at_line_start(content, p) || previous.is_some_and(|s| s.end > p) {
        return None;
    }
    Some(Span {
        start: p,
        end: content.len(),
        new: new.strip_suffix(nl).unwrap_or(new).to_string(),
    })
}

fn mismatch(
    field: &str,
    content: &str,
    needle: &str,
    found: &Found,
    expected: usize,
    block: bool,
) -> Mismatch {
    let positions = found.positions;
    if found.overlap {
        // Overlap fails whatever the count, so it leads; "expected 2, found 2" read as a contradiction.
        let mut details = diagnose::count_mismatch(content, positions, expected, field, block);
        details.remove(0);
        details.pop();
        let within_line = !needle.trim_end_matches(['\n', '\r']).contains('\n');
        details.push(if within_line {
            format!(
                "hint: the matches overlap within a line, so they can be neither told apart nor all replaced; add the characters before or after it to \"{field}\" so that it matches once"
            )
        } else {
            format!(
                "hint: the matches overlap (such as repeated identical lines), so they can be neither told apart nor all replaced; add a nearby line to \"{field}\" that makes it unique"
            )
        });
        let counted = format!(
            "{} matches found, but they overlap ({})",
            positions.len(),
            overlap_places(content, positions, needle.len(), within_line)
        );
        return Mismatch {
            headline: if field == "old" {
                counted
            } else {
                format!("\"{field}\": {counted}")
            },
            details,
        };
    }
    if !positions.is_empty() {
        let n = positions.len();
        let counted = format!(
            "expected {expected} match{}, found {n}",
            if expected == 1 { "" } else { "es" },
        );
        let details = diagnose::count_mismatch(content, positions, expected, field, block);
        return Mismatch {
            headline: if field == "old" {
                counted
            } else {
                format!("\"{field}\": {counted}")
            },
            details,
        };
    }

    let headline = format!("\"{field}\" not found");
    let details = match block
        .then(|| partial_line(content, needle, 0, field))
        .flatten()
    {
        Some(details) => details,
        None => diagnose::not_found(content, needle, field, 0),
    };
    Mismatch { headline, details }
}

/// Where overlapping matches are: "lines 1-2 and 2-3", or for text within a line
/// "line 1, columns 1-2 and 2-3".
fn overlap_places(content: &str, positions: &[usize], len: usize, within_line: bool) -> String {
    const MAX_PLACES: usize = 10;
    let idx = LineIndex::new(content);
    let mut groups: Vec<(usize, Vec<String>)> = Vec::new();
    for &p in positions.iter().take(MAX_PLACES) {
        let end = (p + len).min(content.len());
        let (line, place) = if within_line {
            let line = idx.line_of(p);
            let col = content[line_start(content, p)..p].chars().count() + 1;
            let last = col + content[p..end].chars().count() - 1;
            (line, format!("{col}-{last}"))
        } else {
            let first = idx.line_of(p);
            let last = idx.line_of(end.saturating_sub(1).max(p));
            (0, format!("{first}-{last}"))
        };
        match groups.last_mut() {
            Some((l, places)) if *l == line => places.push(place),
            _ => groups.push((line, vec![place])),
        }
    }
    let more = if positions.len() > MAX_PLACES {
        ", …"
    } else {
        ""
    };
    let and_list = |items: &[String]| match items {
        [a, b] => format!("{a} and {b}"),
        _ => items.join(", "),
    };
    let text = if within_line {
        groups
            .iter()
            .map(|(line, places)| format!("line {line}, columns {}", and_list(places)))
            .collect::<Vec<_>>()
            .join("; ")
    } else {
        format!("lines {}", and_list(&groups[0].1))
    };
    format!("{text}{more}")
}

/// Block texts match at line starts. If the text is in `content[start..]` only as part of
/// a line, say which of its lines is cut short: the first one (leading text such as
/// indentation left out) or, for OLD, the last one (only the start of a line written).
fn partial_line(content: &str, needle: &str, start: usize, field: &str) -> Option<Vec<String>> {
    let text = needle.strip_suffix('\n').unwrap_or(needle);
    let text = text.strip_suffix('\r').unwrap_or(text);
    if text.trim().is_empty() {
        return None;
    }
    let loose: Vec<usize> = occurrences(&content[start..], text, false)
        .into_iter()
        .map(|p| start + p)
        .collect();
    let &p = loose.first()?;
    let end = p + text.len();
    let cut_start = !at_line_start(content, p);
    // FROM and TO may stop early; OLD has to reach the end of its last line.
    let cut_end =
        field == "old" && !content[end..].starts_with(['\n', '\r']) && end < content.len();
    if !cut_start && !cut_end {
        return None;
    }

    let idx = LineIndex::new(content);
    let name = field.to_uppercase();
    // Text before and after it on the same line: in the middle, neither "the end" nor "the start".
    let first_end = p + text.find('\n').unwrap_or(text.len());
    let middle =
        cut_start && first_end < content.len() && !content[first_end..].starts_with(['\n', '\r']);
    let mut out =
        vec!["found only as part of a line (the block format matches whole lines):".to_string()];
    if cut_start {
        let l = idx.line_of(p);
        out.push(if middle {
            format!(
                "{name} line 1 is in the middle of line {l} (text before and after it on that line is missing):"
            )
        } else {
            format!(
                "{name} line 1 is only the end of line {l} (text at the start of the line, such as indentation, is missing):"
            )
        });
        // The whole line, untruncated: the usual fix is to copy it into OLD.
        out.push(gutter_copyable(l, idx.line(content, l)));
    }
    // A one-line OLD in the middle of a line was named once above.
    if cut_end && !(middle && !text.contains('\n')) {
        let l = idx.line_of(end);
        out.push(format!(
            "{name} line {} is only the start of line {l}:",
            text.lines().count()
        ));
        out.push(gutter_copyable(l, idx.line(content, l)));
    }
    if loose.len() > 1 {
        let mut others: Vec<usize> = loose[1..].iter().map(|&q| idx.line_of(q)).collect();
        others.dedup();
        out.push(format!("(also found as part of {})", line_list(&others)));
    }
    out.push(if field == "old" {
        "hint: copy the whole lines, including leading whitespace; to change only part of a line, open the block with `<<<<<<< OLD part`; to only add lines next to it, use BEFORE/AFTER, which accept part of a line".into()
    } else {
        format!("hint: {name} has to start at the start of a line, including leading whitespace (its last line may stop early)")
    });
    Some(out)
}

fn to_not_found(
    content: &str,
    to: &str,
    to_text: &str,
    from_start: usize,
    after: usize,
    block: bool,
) -> Mismatch {
    let idx = LineIndex::new(content);
    let headline = format!(
        "\"to\" not found after \"from\" (line {})",
        idx.line_of(from_start)
    );
    let before = occurrences(&content[..from_start], to_text, block);
    let details = match block
        .then(|| partial_line(content, to, after, "to"))
        .flatten()
    {
        Some(details) => details,
        None if !before.is_empty() => {
            let mut at: Vec<usize> = before.iter().map(|&p| idx.line_of(p)).collect();
            at.dedup();
            vec![format!(
                "\"to\" occurs only before \"from\", at {}",
                line_list(&at)
            )]
        }
        None => diagnose::not_found(content, to, "to", after),
    };
    Mismatch { headline, details }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occurrences_include_overlapping_matches() {
        assert_eq!(occurrences("xa\na\na\n", "a\na\n", true), vec![3]);
        assert_eq!(occurrences("xa\na\na\n", "a\na\n", false), vec![1, 3]);
        assert_eq!(occurrences("aaa", "aa", false), vec![0, 1]);
    }
}
