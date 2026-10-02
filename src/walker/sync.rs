use std::io::Result;
use std::path::Path;
use std::sync::Arc;

use crate::nfa::{Pattern, StateId};
use crate::{GlobResult, test_log};
use crate::walker::{Entry, GlobConfig, Walker, WalkerEntry};

#[derive(Debug)]
struct SyncWalker {
    inner: Walker,
}

impl SyncWalker {
    fn read_dir(
        &mut self,
        cur_dir: &Path,
        recursion_depth: usize,
    ) -> Result<Vec<WalkerEntry>> {
        if recursion_depth > self.inner.config.max_depth {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{}: max recursion depth exceeded", cur_dir.display()),
            ));
        }

        let follow_symlinks = self.inner.config.follow_symlinks;
        let mut dir_entries = Vec::new();
        for entry in std::fs::read_dir(cur_dir)? {
            match entry.and_then(|e| WalkerEntry::from_dir_entry(e, follow_symlinks)) {
                Ok(entry) => dir_entries.push(entry),
                Err(err) => self.inner.out.push(Err(err)),
            }
        }

        Ok(dir_entries)
    }

    fn visit_dir(
        &mut self,
        cur_dir: &Path,
        recursion_depth: usize,
        states: Option<Vec<StateId>>,
    ) -> Result<()> {
        test_log!("visiting {} (depth {recursion_depth})", cur_dir.display());

        let mut entries = self.read_dir(cur_dir, recursion_depth)?;
        let recurse = self.inner.match_entries(cur_dir, states, &mut entries);

        // Descend
        for (i, states) in recurse.into_iter().enumerate() {
            if states.is_empty() { continue; }
            let dir = cur_dir.join(&entries[i].name[..]);
            let res = self.visit_dir(
                &dir,
                recursion_depth + 1,
                Some(states),
            );
            if let Err(e) = res {
                self.inner.out.push(Err(e));
            }
        }

        Ok(())
    }

    fn walk(&mut self) {
        let cur_dir = self.inner.pattern.base_path.to_owned();
        let res = self.visit_dir(Path::new(&cur_dir[..]), 0, None);
        if let Err(e) = res {
            self.inner.out.push(Err(e));
        }
    }
}

pub fn glob(config: GlobConfig, pattern: Pattern) -> GlobResult {
    let walker = Walker {
        config: Arc::new(config),
        pattern,
        out: Vec::new(),
    };
    let mut walker = SyncWalker {
        inner: walker,
    };
    walker.walk();
    GlobResult {
        results: walker.inner.out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::create_test_files;

    fn glob_with_dir(config: GlobConfig, pattern: &str, dir: &Path) -> Vec<String> {
        let full = format!("{}/{}", dir.display(), pattern);
        let result = glob(config, Pattern::compile(&full).unwrap());
        let prefix = format!("{}/", dir.display());
        if let Some(e) = result.errors().next() {
            panic!("{}", e);
        }
        let mut paths: Vec<String> = result
            .entries()
            .map(|e| e.path.strip_prefix(&prefix).unwrap().to_owned())
            .collect();
        paths.sort();
        paths
    }

    fn glob_with_config(config: GlobConfig, pattern: &str, files: &[&str]) -> Vec<String> {
        let dir = create_test_files(files);
        glob_with_dir(config, pattern, dir.path())
    }

    fn glob_files(pattern: &str, files: &[&str]) -> Vec<String> {
        glob_with_config(Default::default(), pattern, files)
    }

    #[test]
    fn literal_name() {
        assert_eq!(
            glob_files("foo.txt", &["foo.txt", "bar.txt"]),
            ["foo.txt"],
        );
    }

    #[test]
    fn question() {
        assert_eq!(
            glob_files("?.txt", &["a.txt", "ab.txt"]),
            ["a.txt"],
        );
    }

    #[test]
    fn alternatives() {
        assert_eq!(
            glob_files("{cat,dog}.txt", &["cat.txt", "dog.txt", "bird.txt"]),
            ["cat.txt", "dog.txt"],
        );
    }

    #[test]
    fn star() {
        assert_eq!(
            glob_files("*.txt", &["foo.txt", "bar.txt", "baz.rs"]),
            ["bar.txt", "foo.txt"],
        );
        assert_eq!(
            glob_files(
                "src/*.rs",
                &["src/main.rs", "src/lib.rs", "src/sub/mod.rs"]
            ),
            ["src/lib.rs", "src/main.rs"],
        );
        // Escaped star handled correctly
        assert_eq!(
            glob_files(r"a\*b.txt", &["a*b.txt", "aXb.txt"]),
            ["a*b.txt"],
        );
    }

    #[test]
    fn starstar() {
        assert_eq!(
            glob_files(
                "**/*.rs",
                &["main.rs", "src/lib.rs", "src/sub/mod.rs", "README.md"]
            ),
            ["main.rs", "src/lib.rs", "src/sub/mod.rs"],
        );
        assert_eq!(
            glob_files(
                "asdf/**",
                &["asdf/blorb.txt"]
            ),
            ["asdf/", "asdf/blorb.txt"],
        );
        assert_eq!(
            glob_files(
                "asdf",
                &["asdf/blorb.txt"]
            ),
            ["asdf/"],
        );
    }

    #[test]
    fn match_filter() {
        assert_eq!(
            glob_with_config(
                GlobConfig {
                    match_files: false,
                    match_directories: true,
                    ..Default::default()
                },
                "**",
                &["main.rs", "src/lib.rs", "src/sub/mod.rs", "README.md"]
            ),
            ["", "src/", "src/sub/"],
        );
        assert_eq!(
            glob_with_config(
                GlobConfig {
                    match_files: true,
                    match_directories: false,
                    ..Default::default()
                },
                "**",
                &["main.rs", "src/lib.rs", "src/sub/mod.rs", "README.md"]
            ),
            ["README.md", "main.rs", "src/lib.rs", "src/sub/mod.rs"],
        );
    }

    #[test]
    #[cfg(unix)]
    fn match_other() {
        let dir = create_test_files(&["main.rs"]);
        let _socket = std::os::unix::net::UnixListener::bind(dir.path().join("sock")).unwrap();
        assert_eq!(
            glob_with_dir(
                GlobConfig {
                    match_files: false,
                    match_other: true,
                    ..Default::default()
                },
                "*",
                dir.path(),
            ),
            ["sock"],
        );
        assert_eq!(
            glob_with_dir(
                GlobConfig {
                    match_files: true,
                    match_other: false,
                    ..Default::default()
                },
                "*",
                dir.path(),
            ),
            ["main.rs"],
        );
    }

    #[test]
    #[cfg(unix)]
    fn symlinks() {
        let dir = create_test_files(&["file.txt", "sub/real.txt"]);
        std::os::unix::fs::symlink(dir.path().join("file.txt"), dir.path().join("link_file")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("sub"), dir.path().join("link_dir")).unwrap();

        assert_eq!(
            glob_with_dir(
                GlobConfig {
                    match_files: false,
                    match_directories: false,
                    match_other: true,
                    follow_symlinks: false,
                    ..Default::default()
                },
                "**",
                dir.path(),
            ),
            ["link_dir", "link_file"],
        );
        assert_eq!(
            glob_with_dir(
                GlobConfig {
                    match_other: false,
                    follow_symlinks: false,
                    ..Default::default()
                },
                "**",
                dir.path(),
            ),
            ["", "file.txt", "sub/", "sub/real.txt"],
        );

        assert_eq!(
            glob_with_dir(Default::default(), "**", dir.path()),
            ["", "file.txt", "link_dir/", "link_dir/real.txt", "link_file", "sub/", "sub/real.txt"],
        );
    }
}
