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
/// Also fails if a file changed on disk after it was read: writing would silently drop
/// that change (another agent, a formatter or an editor working on the same tree).
pub fn check(files: &[Pending]) -> Result<(), String> {
    for f in files {
        if f.perms.readonly() {
            return Err(format!("{}: file is read-only; nothing written", f.display));
        }
    }
    for f in files {
        if !unchanged(f) {
            return Err(format!(
                "{}: {CHANGED}; nothing written{CHANGED_HINT}",
                f.display
            ));
        }
    }
    Ok(())
}

const CHANGED: &str = "the file changed on disk after rpatch read it";
const CHANGED_HINT: &str = "\n  hint: another process or agent is writing it; read the file again and rerun the edit (rpatch never overwrites changes it has not seen)";

/// Whether the file still holds what was read, so that replacing it loses nothing.
fn unchanged(f: &Pending) -> bool {
    fs::read(f.path).is_ok_and(|bytes| {
        bytes.len() == f.prefix.len() + f.original.len()
            && bytes.starts_with(f.prefix.as_bytes())
            && bytes[f.prefix.len()..] == *f.original.as_bytes()
    })
}

/// Writes every file, or none: if a write fails, files already written are restored.
pub fn commit(files: &[Pending]) -> Result<(), String> {
    check(files)?;
    for (i, f) in files.iter().enumerate() {
        // Checked again right before each rename, to keep the window small.
        let written = if unchanged(f) {
            write_atomic(f.path, f.prefix, f.content, f.perms)
                .map_err(|e| format!("write failed: {e}"))
        } else {
            Err(CHANGED.to_string())
        };
        if let Err(why) = written {
            let hint = if why == CHANGED { CHANGED_HINT } else { "" };
            let failed: Vec<&str> = files[..i]
                .iter()
                .filter(|g| write_atomic(g.path, g.prefix, g.original, g.perms).is_err())
                .map(|g| g.display)
                .collect();
            return Err(if failed.is_empty() {
                format!("{}: {why}; nothing written{hint}", f.display)
            } else {
                format!(
                    "{}: {why}; ROLLBACK FAILED, these files are left modified: {}{hint}",
                    f.display,
                    failed.join(", ")
                )
            });
        }
    }
    Ok(())
}

/// What a file held before an edit wrote it, to put back: `--probe` breaks files on purpose
/// and must leave them as they were.
pub struct Backup {
    pub display: String,
    path: std::path::PathBuf,
    prefix: String,
    original: String,
    perms: Permissions,
}

impl Backup {
    pub fn of(f: &Pending) -> Backup {
        Backup {
            display: f.display.to_string(),
            path: f.path.to_path_buf(),
            prefix: f.prefix.to_string(),
            original: f.original.to_string(),
            perms: f.perms.clone(),
        }
    }

    /// Writes the old bytes back, then reads the file again and compares it with them.
    pub fn restore(&self) -> Result<(), String> {
        write_atomic(&self.path, &self.prefix, &self.original, &self.perms)
            .map_err(|e| format!("write failed: {e}"))?;
        let now = fs::read(&self.path).map_err(|e| format!("cannot read it back: {e}"))?;
        let mut want = self.prefix.clone().into_bytes();
        want.extend_from_slice(self.original.as_bytes());
        if now == want {
            Ok(())
        } else {
            Err("it differs from the old bytes after the restore".to_string())
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pending<'a>(
        path: &'a Path,
        original: &'a str,
        content: &'a str,
        perms: &'a Permissions,
    ) -> Pending<'a> {
        Pending {
            display: "f",
            path,
            original,
            content,
            prefix: "",
            perms,
        }
    }

    #[test]
    fn a_file_changed_after_reading_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, "a\n").unwrap();
        // Read as "b\n", then changed by someone else before the write.
        fs::write(&b, "theirs\n").unwrap();
        let perms = fs::metadata(&a).unwrap().permissions();
        let files = [
            pending(&a, "a\n", "A\n", &perms),
            pending(&b, "b\n", "B\n", &perms),
        ];
        let err = commit(&files).unwrap_err();
        assert!(
            err.contains("changed on disk after rpatch read it"),
            "{err}"
        );
        assert!(err.contains("nothing written"), "{err}");
        assert_eq!(fs::read_to_string(&a).unwrap(), "a\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "theirs\n");
        // A dry run checks the same.
        assert!(check(&files).is_err());
    }

    #[test]
    fn an_unchanged_file_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        fs::write(&a, "\u{feff}a\n").unwrap();
        let perms = fs::metadata(&a).unwrap().permissions();
        let mut f = pending(&a, "a\n", "A\n", &perms);
        f.prefix = "\u{feff}";
        commit(&[f]).unwrap();
        assert_eq!(fs::read_to_string(&a).unwrap(), "\u{feff}A\n");
    }
}
