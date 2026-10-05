use std::path::{Path, PathBuf};
use std::sync::Arc;

use fnv::FnvHashMap;

use crate::{FileType, test_log};
use crate::nfa::{Pattern, StateId};
use crate::walker::{Walker, WalkerEntry};

#[derive(Debug)]
pub struct VirtualDir {
    entries: Vec<WalkerEntry>,
}

#[derive(Debug)]
pub struct VirtualFs {
    dirs: FnvHashMap<PathBuf, VirtualDir>,
}

impl VirtualFs {
    fn add(&mut self, path: &str) {
        let mut path = Path::new(path);
        let mut file_type = FileType::File;
        while let Some(parent) = path.parent() {
            let name = path.file_name().unwrap().to_str().unwrap();
            if let Some(dir) = self.dirs.get_mut(parent) {
                dir.entries.push(WalkerEntry { name: name.into(), file_type });
                break;
            }
            self.dirs.insert(parent.to_owned(), VirtualDir {
                entries: vec![WalkerEntry { name: name.into(), file_type }],
            });
            file_type = FileType::Directory;
            path = parent;
        }
    }

    fn read_dir(&self, path: &Path) -> &[WalkerEntry] {
        &self.dirs[path].entries[..]
    }
}

#[derive(Debug)]
struct StringWalker {
    inner: Walker,
    fs: VirtualFs,
}

impl StringWalker {
    fn visit_dir(
        &mut self,
        cur_dir: &str,
        recursion_depth: usize,
        states: Option<Vec<StateId>>,
    ) {
        test_log!("visiting {} (depth {recursion_depth})", cur_dir);

        if recursion_depth > self.inner.config.max_depth() {
            return;
        }

        let mut entries = self.fs.read_dir(Path::new(cur_dir)).to_vec();
        let recurse = self.inner.match_entries(Path::new(cur_dir), states, &mut entries);

        for (i, states) in recurse.into_iter().enumerate() {
            if states.is_empty() { continue; }
            let dir = Path::new(cur_dir).join(&entries[i].name[..]);
            self.visit_dir(dir.to_str().unwrap(), recursion_depth + 1, Some(states));
        }
    }

    fn walk(&mut self) {
        let cur_dir = self.inner.pattern.base_path.to_owned();
        self.visit_dir(&cur_dir, 0, None);
    }
}

/// Returns filepaths which match the given pattern. All paths are treated as
/// files, not directories.
///
/// Host system file path semantics are used when matching, including path
/// separators. The pattern may contain '.' and '..' and they will work as
/// expected.
pub fn glob(pattern: Pattern, paths: &[&str]) -> Vec<String> {
    let walker = Walker {
        config: Arc::new(Default::default()),
        pattern,
        out: Vec::new(),
    };
    let mut walker = StringWalker {
        inner: walker,
        fs: VirtualFs { dirs: FnvHashMap::default() },
    };
    for path in paths {
        walker.fs.add(path);
    }
    walker.walk();
    walker.inner.out.into_iter()
        .filter_map(|r| r.ok())
        .map(|e| e.path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob_paths(pattern: &str, paths: &[&str]) -> Vec<String> {
        let pattern = Pattern::compile(pattern).unwrap();
        let mut matches = glob(pattern, paths);
        matches.sort();
        matches
    }

    #[test]
    fn starstar() {
        assert_eq!(
            glob_paths(
                "**/*.rs",
                &["main.rs", "src/lib.rs", "src/sub/mod.rs", "README.md"]
            ),
            ["main.rs", "src/lib.rs", "src/sub/mod.rs"],
        );
        assert_eq!(
            glob_paths(
                "asdf/**",
                &["asdf/blorb.txt"]
            ),
            ["asdf/", "asdf/blorb.txt"],
        );
        assert_eq!(
            glob_paths(
                "asdf",
                &["asdf/blorb.txt"]
            ),
            ["asdf/"],
        );
    }
}
