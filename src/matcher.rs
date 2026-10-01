//! Finds where an edit applies in a file's current content.

use std::borrow::Cow;

use crate::diagnose;
use crate::input::{Edit, Target};
use crate::text::{LineIndex, gutter, line_list};

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
    let new = adapt(&edit.new, crlf);
    match &edit.target {
        Target::Text(old) => {
            let old = adapt(old, crlf);
            let mut spans: Vec<Span> = occurrences(content, &old, edit.lines)
                .into_iter()
                .map(|p| Span {
                    start: p,
                    end: p + old.len(),
                    new: new.to_string(),
                })
                .collect();
            if edit.lines {
                spans.extend(last_line_match(content, &old, &new, crlf, spans.last()));
            }
            if spans.len() == edit.count {
                return Ok(spans);
            }
            let positions: Vec<usize> = spans.iter().map(|s| s.start).collect();
            Err(mismatch(
                "old", content, &old, &positions, edit.count, edit.lines,
            ))
        }
        Target::Insert { anchor, after } => {
            let field = if *after { "after" } else { "before" };
            let nl = if crlf { "\r\n" } else { "\n" };
            let anchor = adapt(anchor, crlf);
            let text = anchor.strip_suffix(nl).unwrap_or(&anchor);
            // Part of a line is enough: inserting at line boundaries rewrites no line.
            let positions = occurrences(content, text, false);
            if positions.len() != edit.count {
                return Err(mismatch(
                    field, content, text, &positions, edit.count, false,
                ));
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
            let starts = occurrences(content, &from, edit.lines);
            if starts.len() != 1 {
                return Err(mismatch("from", content, &from, &starts, 1, edit.lines));
            }
            let start = starts[0];
            let after = start + from.len();
            let Some(&q) = occurrences(&content[after..], &to, edit.lines).first() else {
                return Err(to_not_found(content, &to, start, after, edit.lines));
            };
            let end = after + q;
            if new.ends_with(to.as_ref()) {
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

fn line_start(content: &str, p: usize) -> usize {
    content[..p].rfind('\n').map_or(0, |i| i + 1)
}

/// The offset just past the line break of the line that contains `p`.
fn line_end(content: &str, p: usize) -> usize {
    content[p..].find('\n').map_or(content.len(), |i| p + i + 1)
}

fn adapt(s: &str, crlf: bool) -> Cow<'_, str> {
    if crlf {
        Cow::Owned(s.replace('\n', "\r\n"))
    } else {
        Cow::Borrowed(s)
    }
}

/// Non-overlapping matches, left to right. With `line_start`, only matches at the
/// start of a line count (and a skipped match does not hide a later overlapping one).
fn occurrences(content: &str, needle: &str, line_start: bool) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = content[from..].find(needle) {
        let p = from + i;
        if !line_start || p == 0 || content.as_bytes()[p - 1] == b'\n' {
            out.push(p);
            from = p + needle.len();
        } else {
            from = p + content[p..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// Block texts end with a line break, so they cannot match a last line that has none.
/// Match that line without the break, and keep the file without a final break.
fn last_line_match(
    content: &str,
    old: &str,
    new: &str,
    crlf: bool,
    previous: Option<&Span>,
) -> Option<Span> {
    if content.ends_with('\n') {
        return None;
    }
    let nl = if crlf { "\r\n" } else { "\n" };
    let old = old.strip_suffix(nl)?;
    if old.is_empty() || !content.ends_with(old) {
        return None;
    }
    let p = content.len() - old.len();
    let at_line_start = p == 0 || content.as_bytes()[p - 1] == b'\n';
    if !at_line_start || previous.is_some_and(|s| s.end > p) {
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
    positions: &[usize],
    expected: usize,
    lines: bool,
) -> Mismatch {
    if !positions.is_empty() {
        let found = positions.len();
        let counted = format!(
            "expected {expected} match{}, found {found}",
            if expected == 1 { "" } else { "es" }
        );
        return Mismatch {
            headline: if field == "old" {
                counted
            } else {
                format!("\"{field}\": {counted}")
            },
            details: diagnose::count_mismatch(content, positions, expected, field),
        };
    }

    let headline = format!("\"{field}\" not found");
    let details = match lines.then(|| partial_line(content, needle, 0)).flatten() {
        Some(details) => details,
        None => diagnose::not_found(content, needle, field, 0),
    };
    Mismatch { headline, details }
}

/// Block texts are whole lines. If the text is in `content[start..]` only as part of a
/// line (leading whitespace left out, or only the start of a line written), say so.
fn partial_line(content: &str, needle: &str, start: usize) -> Option<Vec<String>> {
    let text = needle.strip_suffix('\n')?;
    let text = text.strip_suffix('\r').unwrap_or(text);
    if text.trim().is_empty() {
        return None;
    }
    let loose = occurrences(&content[start..], text, false);
    if loose.is_empty() {
        return None;
    }
    let idx = LineIndex::new(content);
    let mut lines: Vec<usize> = loose.iter().map(|&p| idx.line_of(start + p)).collect();
    lines.dedup();
    let mut out = vec![format!(
        "found only as part of a line (the block format matches whole lines), at {}:",
        line_list(&lines)
    )];
    out.extend(
        lines
            .iter()
            .take(5)
            .map(|&l| gutter(l, idx.line(content, l))),
    );
    out.push(
        "hint: copy the whole lines, including leading whitespace, or use JSON to match part of a line; to only add lines next to it, use BEFORE/AFTER, which accept part of a line"
            .into(),
    );
    Some(out)
}

fn to_not_found(content: &str, to: &str, from_start: usize, after: usize, lines: bool) -> Mismatch {
    let idx = LineIndex::new(content);
    let headline = format!(
        "\"to\" not found after \"from\" (line {})",
        idx.line_of(from_start)
    );
    let before = occurrences(&content[..from_start], to, lines);
    let details = match lines.then(|| partial_line(content, to, after)).flatten() {
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_start_matching_does_not_skip_overlaps() {
        // The match at offset 1 starts mid-line; the overlapping one at offset 3 does not.
        assert_eq!(occurrences("xa\na\na\n", "a\na\n", true), vec![3]);
        assert_eq!(occurrences("xa\na\na\n", "a\na\n", false), vec![1]);
    }
}
