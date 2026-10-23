//! The help is one file, `help.txt`; nothing in it is written twice. `--help` prints its summary
//! (up to the blank line after "Exit status:"), so that `head -40` cannot cut it. A reader may cut
//! shorter (`head -30`, `head -3`), so the summary is ordered by what an edit needs first: the line
//! naming the details right under the title, then the usage, then the tips that keep an edit from
//! going wrong, and only then the other blocks, the output and the exit status. A section is the
//! lines from a heading (a line not indented, after a blank line) to the blank line before the next
//! heading; `--help TOPIC` prints the section that `TOPICS` names.

const TEXT: &str = include_str!("help.txt");

/// A name and the start of the heading of the section that answers it. A section no name asks
/// for cannot be read: a test fails.
const TOPICS: &[(&str, &str)] = &[
    ("blocks", "Blocks:"),
    ("json", "JSON"),
    ("rules", "Rules:"),
    ("output", "Output details:"),
    ("probe", "Probe:"),
];

fn names() -> String {
    TOPICS.iter().map(|t| t.0).collect::<Vec<_>>().join(", ")
}

/// The number of lines of the summary: the lines through the "Exit status:" paragraph.
fn summary(lines: &[&str]) -> usize {
    let exit = lines
        .iter()
        .position(|l| l.starts_with("Exit status:"))
        .expect("help.txt says the exit status");
    exit + lines[exit..]
        .iter()
        .position(|l| l.trim().is_empty())
        .unwrap_or(lines.len() - exit)
}

/// The sections after the summary, each without its trailing blank lines.
fn sections<'a>(lines: &'a [&'a str]) -> Vec<&'a [&'a str]> {
    let heading = |i: usize| {
        !lines[i].is_empty() && !lines[i].starts_with(' ') && (i == 0 || lines[i - 1].is_empty())
    };
    let starts: Vec<usize> = (0..lines.len()).filter(|&i| heading(i)).collect();
    starts
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            let mut end = starts.get(k + 1).copied().unwrap_or(lines.len());
            while lines[end - 1].trim().is_empty() {
                end -= 1;
            }
            &lines[s..end]
        })
        .collect()
}

fn join(lines: &[&str]) -> String {
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

/// What `rpatch --help [TOPIC]` prints; an unknown topic is the message.
pub fn render(topic: Option<&str>) -> Result<String, String> {
    let lines: Vec<&str> = TEXT.lines().collect();
    let end = summary(&lines);
    let Some(topic) = topic else {
        return Ok(format!(
            "{}\nDetails: rpatch --help TOPIC ({}); rpatch --help all: everything.\n{}",
            lines[0],
            names(),
            join(&lines[1..end])
        ));
    };
    if topic == "all" {
        return Ok(TEXT.to_string());
    }
    let Some(&(_, heading)) = TOPICS.iter().find(|t| t.0 == topic) else {
        return Err(format!(
            "rpatch: unknown help topic `{topic}`; try: {}, all",
            names()
        ));
    };
    let section = sections(&lines[end + 1..])
        .into_iter()
        .find(|s| s[0].starts_with(heading))
        .unwrap_or_else(|| panic!("help.txt has no section `{heading}`"));
    Ok(join(section))
}
