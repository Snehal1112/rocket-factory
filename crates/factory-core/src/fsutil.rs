use crate::error::CoreError;
use std::io::Write;
use std::path::Path;

/// Writes the file by creating a sibling temp file and renaming it over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{}.{}.tmp", name, std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(CoreError::io(path, e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_parents_replaces_content_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a").join("b.txt");
        write_atomic(&path, b"one").expect("first write");
        write_atomic(&path, b"two").expect("second write");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "two");
        let names: Vec<_> = std::fs::read_dir(path.parent().expect("parent"))
            .expect("list")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["b.txt".to_string()]);
    }
}
