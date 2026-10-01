//! Explanations for edits whose text did not match the expected number of times.
//!
//! `field` names the text in messages ("old", "from" or "to"). `start` limits the
//! search for similar text to `content[start..]`; line numbers are always for the
//! whole file.

use crate::text::{LineIndex, gutter, line_list};

const MAX_LOCATIONS: usize = 10;
const MAX_BLOCK_LINES: usize = 8;

/// `field` ("old" or "from") matched, but not `expected` times.
pub fn count_mismatch(
    content: &str,
    positions: &[usize],
    expected: usize,
    field: &str,
) -> Vec<String> {
    let idx = LineIndex::new(content);
    let mut lines: Vec<usize> = positions.iter().map(|&p| idx.line_of(p)).collect();
    lines.dedup();
    let mut out = vec![format!("matches at {}", line_list(&lines))];
    for &l in lines.iter().take(MAX_LOCATIONS) {
        out.push(gutter(l, idx.line(content, l)));
    }
    if lines.len() > MAX_LOCATIONS {
        out.push(format!("… and {} more", lines.len() - MAX_LOCATIONS));
    }
    let found = positions.len();
    out.push(if field != "old" {
        format!("hint: add surrounding text to \"{field}\" so that it matches once")
    } else if found > expected {
        format!(
            "hint: add surrounding text to \"old\" to pick one, or set \"count\": {found} to replace all"
        )
    } else {
        format!("hint: set \"count\": {found} if that is intended")
    });
    out
}

/// `needle` did not match at all.
pub fn not_found(content: &str, needle: &str, field: &str, start: usize) -> Vec<String> {
    crlf_hint(content, needle, field, start)
        .or_else(|| whitespace_hint(content, needle, field, start))
        .or_else(|| divergence_hint(content, needle, field, start))
        .or_else(|| closest_line_hint(content, needle, field, start))
        .unwrap_or_else(|| vec!["no similar text found".into()])
}

fn crlf_hint(content: &str, needle: &str, field: &str, start: usize) -> Option<Vec<String>> {
    if !needle.contains('\n')
        || needle.contains('\r')
        || !content[start..].contains(&needle.replace('\n', "\r\n"))
    {
        return None;
    }
    Some(vec![format!(
        "the file uses CRLF line endings; write line breaks in \"{field}\" as \\r\\n"
    )])
}

/// Collapses each whitespace run to a single space. The second value maps each byte
/// of the result to the byte offset it came from.
fn collapse_whitespace(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len());
    let mut in_ws = false;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
                map.push(i);
            }
            in_ws = true;
        } else {
            in_ws = false;
            let before = out.len();
            out.push(c);
            map.resize(map.len() + (out.len() - before), i);
        }
    }
    (out, map)
}

fn whitespace_hint(content: &str, needle: &str, field: &str, start: usize) -> Option<Vec<String>> {
    let (needle, _) = collapse_whitespace(needle.trim());
    if needle.is_empty() {
        return None;
    }
    let (hay, map) = collapse_whitespace(&content[start..]);
    let ranges: Vec<(usize, usize)> = {
        let idx = LineIndex::new(content);
        hay.match_indices(&needle)
            .map(|(i, _)| {
                (
                    idx.line_of(start + map[i]),
                    idx.line_of(start + map[i + needle.len() - 1]),
                )
            })
            .collect()
    };
    if ranges.is_empty() {
        return None;
    }

    let idx = LineIndex::new(content);
    let mut out = vec![format!(
        "found with different whitespace (indentation, spacing or line breaks) at {}:",
        ranges
            .iter()
            .map(|&(a, b)| if a == b {
                format!("line {a}")
            } else {
                format!("lines {a}-{b}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    )];
    let (first, last) = ranges[0];
    for l in first..=last.min(first + MAX_BLOCK_LINES - 1) {
        out.push(gutter(l, idx.line(content, l)));
    }
    if last >= first + MAX_BLOCK_LINES {
        out.push(format!(
            "… {} more lines",
            last + 1 - first - MAX_BLOCK_LINES
        ));
    }
    out.push(format!(
        "hint: copy \"{field}\" exactly from the file; rpatch never matches loosely"
    ));
    Some(out)
}

/// Finds the longest prefix of `old` that occurs in the file and shows where it stops matching.
fn divergence_hint(content: &str, old: &str, field: &str, start: usize) -> Option<Vec<String>> {
    let hay = &content[start..];
    let bounds: Vec<usize> = old
        .char_indices()
        .map(|(i, _)| i)
        .chain([old.len()])
        .collect();
    // Invariant: old[..bounds[lo]] occurs in content, old[..bounds[hi]] does not.
    let (mut lo, mut hi) = (0, bounds.len() - 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if hay.contains(&old[..bounds[mid]]) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let prefix = &old[..bounds[lo]];
    if prefix.trim().chars().count() < 8 {
        return None;
    }

    let pos = start + hay.find(prefix)?;
    let fidx = LineIndex::new(content);
    let oidx = LineIndex::new(old);
    let fline = fidx.line_of(pos + prefix.len());
    let oline = oidx.line_of(prefix.len());
    let file_text = if pos + prefix.len() == content.len() {
        "(end of file)"
    } else {
        fidx.line(content, fline)
    };
    let mut out = vec![format!(
        "\"{field}\" matches the file up to a point, then differs at line {fline}:"
    )];
    out.push(gutter(field, oidx.line(old, oline)));
    out.push(gutter(fline, file_text));
    let occurrences = hay.matches(prefix).count();
    if occurrences > 1 {
        out.push(format!(
            "(the matching part occurs {occurrences} times; showing the first)"
        ));
    }
    Some(out)
}

/// Compares the first non-blank line of `old` against every line of the file from `start`.
fn closest_line_hint(content: &str, old: &str, field: &str, start: usize) -> Option<Vec<String>> {
    let target = old.lines().map(str::trim).find(|l| !l.is_empty())?;
    let target_len = target.chars().count();
    let idx = LineIndex::new(content);
    let first = idx.line_of(start);
    let mut best: Option<(f64, usize)> = None;
    for (i, line) in content.lines().enumerate().skip(first - 1) {
        let t = line.trim();
        let len = t.chars().count();
        if len.abs_diff(target_len) * 2 > len.max(target_len) {
            continue;
        }
        let score = strsim::normalized_levenshtein(target, t);
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, i + 1));
        }
    }
    let (score, line) = best.filter(|&(s, _)| s >= 0.5)?;
    Some(vec![
        format!(
            "closest line to the first line of \"{field}\" ({:.0}% similar):",
            score * 100.0
        ),
        gutter(field, target),
        gutter(line, idx.line(content, line)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_maps_back_to_source_offsets() {
        let (s, map) = collapse_whitespace("a  \n\tbé c");
        assert_eq!(s, "a bé c");
        assert_eq!(map, vec![0, 1, 5, 6, 6, 8, 9]);
    }
}
