use std::fs::{self, Permissions};
use std::io::{self, Write};
use std::path::Path;

pub struct Pending<'a> {
    pub display: &'a str,
    pub path: &'a Path,
    pub original: &'a str,
    pub content: &'a str,
    /// Written before both `content` and `original`, such as a UTF-8 BOM.
    pub prefix: &'a str,
    pub perms: &'a Permissions,
}

/// Fails if a file is read-only, which rename would replace all the same. Run before a
/// dry run too, so that it fails where the real run would.
pub fn check(files: &[Pending]) -> Result<(), String> {
    for f in files {
        if f.perms.readonly() {
            return Err(format!("{}: file is read-only; nothing written", f.display));
        }
    }
    Ok(())
}

/// Writes every file, or none: if a write fails, files already written are restored.
pub fn commit(files: &[Pending]) -> Result<(), String> {
    check(files)?;
    for (i, f) in files.iter().enumerate() {
        if let Err(e) = write_atomic(f.path, f.prefix, f.content, f.perms) {
            let failed: Vec<&str> = files[..i]
                .iter()
                .filter(|g| write_atomic(g.path, g.prefix, g.original, g.perms).is_err())
                .map(|g| g.display)
                .collect();
            return Err(if failed.is_empty() {
                format!("{}: write failed: {e}; nothing written", f.display)
            } else {
                format!(
                    "{}: write failed: {e}; ROLLBACK FAILED, these files are left modified: {}",
                    f.display,
                    failed.join(", ")
                )
            });
        }
    }
    Ok(())
}

/// Writes to a temporary file next to `path`, then renames it over `path`.
fn write_atomic(path: &Path, prefix: &str, content: &str, perms: &Permissions) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::Builder::new()
        .prefix(".rpatch-")
        .tempfile_in(dir)?;
    tmp.write_all(prefix.as_bytes())?;
    tmp.write_all(content.as_bytes())?;
    tmp.as_file().sync_all()?;
    fs::set_permissions(tmp.path(), perms.clone())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
