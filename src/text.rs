/// Byte offset → 1-based line number lookups.
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(s: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(s.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|&s| s <= offset)
    }

    /// Text of a 1-based line, without the line terminator.
    pub fn line<'a>(&self, s: &'a str, line: usize) -> &'a str {
        let start = self.starts[line - 1];
        let end = self.starts.get(line).map_or(s.len(), |&e| e - 1);
        s[start..end].trim_end_matches('\r')
    }
}

const MAX_SHOWN: usize = 200;

/// A line prefixed with a right-aligned label, e.g. `    42 | text`.
pub fn gutter(label: impl std::fmt::Display, text: &str) -> String {
    let text = match text.char_indices().nth(MAX_SHOWN) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    };
    format!("{label:>6} | {text}")
}

/// "line 3" or "lines 1, 3".
pub fn line_list(lines: &[usize]) -> String {
    let list = lines
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if lines.len() == 1 {
        format!("line {list}")
    } else {
        format!("lines {list}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_lookup() {
        let s = "ab\ncd\r\n\nef";
        let idx = LineIndex::new(s);
        assert_eq!(idx.line_of(0), 1);
        assert_eq!(idx.line_of(2), 1);
        assert_eq!(idx.line_of(3), 2);
        assert_eq!(idx.line_of(s.len()), 4);
        assert_eq!(idx.line(s, 2), "cd");
        assert_eq!(idx.line(s, 3), "");
        assert_eq!(idx.line(s, 4), "ef");
    }
}
