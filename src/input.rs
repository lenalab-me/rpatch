use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    edits: Vec<Edit>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub file: String,
    pub old: String,
    pub new: String,
    #[serde(default = "default_count")]
    pub count: usize,
}

fn default_count() -> usize {
    1
}

pub fn parse(text: &str) -> Result<Vec<Edit>, String> {
    let input: Input =
        serde_json::from_str(text).map_err(|e| format!("invalid JSON input: {e}"))?;
    if input.edits.is_empty() {
        return Err("\"edits\" is empty".into());
    }
    for (i, e) in input.edits.iter().enumerate() {
        if e.file.is_empty() {
            return Err(format!("edits[{i}]: \"file\" is empty"));
        }
        if e.old.is_empty() {
            return Err(format!(
                "edits[{i}]: \"old\" is empty (creating files is not supported)"
            ));
        }
        if e.count == 0 {
            return Err(format!("edits[{i}]: \"count\" must be at least 1"));
        }
    }
    Ok(input.edits)
}
