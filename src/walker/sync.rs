use std::fs::DirEntry;
use std::io::Result;
use std::path::Path;

use crate::matcher::{Matcher, TrieEntry};
use crate::nfa::{Pattern, StateId};
use crate::{SmallString, test_log};
use crate::trie::Trie;
use crate::walker::{Entry, FileType, GlobConfig};

#[derive(Debug)]
struct WalkerEntry {
    file_type: FileType,
    name: SmallString,
}

fn get_file_type(entry: &DirEntry) -> Result<FileType> {
    let file_type = entry.file_type()?;
    if file_type.is_symlink() {
        Ok(entry.path().metadata()?.file_type().into())
    } else {
        Ok(file_type.into())
    }
}

impl WalkerEntry {
    fn from_dir_entry(entry: DirEntry, follow_symlinks: bool) -> Result<Self> {
        let name: SmallString = entry.path()
            .file_name()
            .unwrap()
            .to_str()
            .ok_or_else(|| {
                let kind = std::io::ErrorKind::InvalidData;
                let message = format!("filename contains invalid UTF-8: {}", entry.path().to_string_lossy());
                std::io::Error::new(kind, message)
            })?
            .into();
        let file_type = if follow_symlinks {
            get_file_type(&entry)?
        } else {
            entry.file_type()?.into()
        };
        Ok(Self { name, file_type })
    }
}

#[derive(Debug)]
struct WalkerDir {
    entries: Vec<WalkerEntry>,
    trie: Trie<TrieEntry>,
}

#[derive(Debug)]
struct Walker {
    config: GlobConfig,
    pattern: Pattern,
    out: Vec<Result<Entry>>,
}

const LITERALS: &[&str] = &["", ".", ".."];

fn build_trie(entries: &[WalkerEntry]) -> Trie<TrieEntry> {
    entries.iter()
        .enumerate()
        .map(|(i, e)| (&e.name, TrieEntry {
            index: i as u32,
            is_dir: e.file_type == FileType::Directory,
            is_literal: false,
        }))
        .collect()
}

fn add_special_entries(
    entries: &mut Vec<WalkerEntry>,
    trie: &mut Trie<TrieEntry>,
) {
    for &d in LITERALS {
        // Virtual directories
        let index = entries.len() as u32;
        entries.push(WalkerEntry {
            name: d.into(),
            file_type: FileType::Directory,
        });
        trie.insert(d, TrieEntry {
            index,
            is_dir: true,
            is_literal: true,
        });
    }
}

impl Walker {
    fn read_dir(
        &mut self,
        cur_dir: &Path,
        recursion_depth: usize,
    ) -> Result<Vec<WalkerEntry>> {
        if recursion_depth > self.config.max_depth {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{}: max recursion depth exceeded", cur_dir.display()),
            ));
        }

        let follow_symlinks = self.config.follow_symlinks;
        let mut dir_entries = Vec::new();
        for entry in std::fs::read_dir(cur_dir)? {
            match entry.and_then(|e| WalkerEntry::from_dir_entry(e, follow_symlinks)) {
                Ok(entry) => dir_entries.push(entry),
                Err(err) => self.out.push(Err(err)),
            }
        }

        Ok(dir_entries)
    }

    /// Records matched files/directories and returns recursive states.
    fn match_entries(
        &mut self,
        cur_dir: &Path,
        states: Option<Vec<StateId>>,
        entries: &mut Vec<WalkerEntry>,
    ) -> Vec<Vec<StateId>> {
        test_log!("entries: {:?}", entries);
        let mut trie = build_trie(&entries);
        add_special_entries(entries, &mut trie);

        let mut matcher = Matcher::new(&self.pattern, &trie, states);
        matcher.run();

        test_log!("matches: {:?}", matcher.full.iter().map(|e| (e.state, e.index)).collect::<Vec<_>>());
        for m in matcher.full {
            let entry = &entries[m.index as usize];
            if !self.config.should_match(entry.file_type) {
                continue;
            }
            let mut full_path = cur_dir.join(&entry.name[..]);
            if entry.file_type == FileType::Directory {
                full_path = full_path.join("");
            }
            self.out.push(Ok(Entry {
                path: full_path.to_str().unwrap().to_owned(),
                file_type: entry.file_type,
            }))
        }

        test_log!("recurse: {:?}", matcher.recurse.iter().map(|e| (e.state, e.index)).collect::<Vec<_>>());
        let mut recurse: Vec<Vec<StateId>> = vec![Vec::new(); entries.len()];
        for m in matcher.recurse {
            recurse[m.index as usize].push(m.state);
        }

        recurse
    }

    fn visit_dir(
        &mut self,
        cur_dir: &Path,
        recursion_depth: usize,
        states: Option<Vec<StateId>>,
    ) -> Result<()> {
        test_log!("visiting {} (depth {recursion_depth})", cur_dir.display());

        let mut entries = self.read_dir(cur_dir, recursion_depth)?;
        let recurse = self.match_entries(cur_dir, states, &mut entries);

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
                self.out.push(Err(e));
            }
        }

        Ok(())
    }

    fn walk(&mut self) {
        let cur_dir = self.pattern.base_path.to_owned();
        let res = self.visit_dir(Path::new(&cur_dir[..]), 0, None);
        if let Err(e) = res {
            self.out.push(Err(e));
        }
    }
}

#[derive(Debug)]
pub struct GlobResult {
    results: Vec<Result<Entry>>,
    _private: (),
}

impl GlobResult {
    pub fn results(&self) -> &[Result<Entry>] {
        &self.results
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> + '_ {
        self.results.iter().filter_map(|r| r.as_ref().ok())
    }

    pub fn errors(&self) -> impl Iterator<Item = &std::io::Error> + '_ {
        self.results.iter().filter_map(|r| r.as_ref().err())
    }
}

pub fn glob(config: GlobConfig, pattern: Pattern) -> GlobResult {
    let mut walker = Walker {
        config,
        pattern,
        out: Vec::new(),
    };
    walker.walk();
    GlobResult {
        results: walker.out,
        _private: (),
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
