use std::fs;

use tempfile::TempDir;

pub(crate) fn create_test_files(paths: &[&str]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for path in paths {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::File::create(&full).unwrap();
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_files_and_implicit_directories() {
        let dir = create_test_files(&["a.txt", "sub/b.txt", "sub/deep/c.txt"]);
        assert!(dir.path().join("a.txt").is_file());
        assert!(dir.path().join("sub/b.txt").is_file());
        assert!(dir.path().join("sub/deep/c.txt").is_file());
    }
}
