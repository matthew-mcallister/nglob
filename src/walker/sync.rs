use std::io::Result;
use std::path::Path;

use crate::matcher::{MatcherState, advance_sep, find_matching_entries, partition_states};
use crate::nfa::{StateId, Pattern, from_pattern};
use crate::pattern::ParsedPattern;
use crate::test_log;
use crate::trie::Trie;
use crate::walker::{Entry, FileType, GlobConfig};

fn from_dir_entry(entry: std::fs::DirEntry) -> Result<Entry> {
    let path = entry.path()
        .file_name()
        .unwrap()
        .to_str()
        .ok_or_else(|| {
            let kind = std::io::ErrorKind::InvalidData;
            let message = format!("filename contains invalid UTF-8: {}", entry.path().to_string_lossy());
            std::io::Error::new(kind, message)
        })?
        .to_owned();
    let file_type = entry.file_type()?.into();
    Ok(Entry { path, file_type })
}

fn walk_dir(
    config: &GlobConfig,
    machine: &Pattern,
    cur_dir: &Path,
    prior_states: &[StateId],
    recursion_depth: usize,
    out: &mut Vec<std::io::Result<Entry>>,
) {
    test_log!("visiting {} (depth {recursion_depth})", cur_dir.display());
    #[cfg(test)]
    let results_start = out.len();

    if recursion_depth > config.max_depth {
        test_log!("maximum recursion depth exceeded");
        out.push(Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "max recursion depth exceeded",
        )));
        return;
    }

    let full_path = |filename: &str| cur_dir.join(filename).into_string().unwrap();

    const LITERALS: &[&str] = &["", ".", ".."];

    let dir_entries = match std::fs::read_dir(cur_dir) {
        Ok(d) => d,
        Err(e) => {
            out.push(Err(e));
            return;
        }
    };
    // TODO: Follow symlinks

    let mut entries: Vec<Entry> = Vec::with_capacity(32);
    for d in LITERALS {
        // Literal-only matches
        entries.push(Entry { path: d.to_string(), file_type: FileType::Directory });
    }
    for e in dir_entries {
        match e.and_then(from_dir_entry) {
            Ok(e) => entries.push(e),
            Err(e) => out.push(Err(e)),
        }
    }

    test_log!("entries: {:?}", entries);

    let trie: Trie<usize> = entries.iter()
        .enumerate()
        .map(|(i, e)| (&e.path, i))
        .collect();

    let get_entry = |state: MatcherState| {
        let idx = *trie.get_value(state.trie).unwrap();
        (idx, &entries[idx])
    };

    // First find all entries that match a path component in the pattern. File
    // type and filters not taken into account.
    let mut accepted = find_matching_entries(
        machine,
        &trie,
        prior_states,
    );
    test_log!(
        "partial matches: {:?}",
        accepted.iter().map(|out| &get_entry(out.state).1.path).collect::<fnv::FnvHashSet<_>>(),
    );
    accepted.retain(|out| {
        // Immediately check if "", ".", ".." were literal matches
        if out.info.is_literal {
            return true;
        }
        let (_, entry) = get_entry(out.state);
        !LITERALS.contains(&entry.path.as_str())
    });

    let (mut full, mut partial) = partition_states(machine, accepted);
    partial.retain(|out| {
        let (_, entry) = get_entry(out.state);
        entry.file_type == FileType::Directory
    });

    // Now we feed a logical '/'. This treats patterns that end in a trailing
    // '/' as a match and may save us from descending into those
    // subdirectories. Patterns ending in `/*` or `/**` are *not* count as a
    // full match, since user intent is ambiguous in those cases.
    let prior = partial.iter().map(|out| out.state);
    let states = advance_sep(machine, &trie, prior);
    let (full2, partial) = partition_states(machine, states);
    full.extend(full2);

    // `partial` now contains all matches that trigger recursion, and `full`
    // contains all full matches that we may yield.

    // Filter, sort + dedupe, add full path, and yield matches
    let mut full: Vec<usize> = full.into_iter()
        .filter_map(|out| {
            let (idx, entry) = get_entry(out.state);
            config.should_match(entry.file_type).then_some(idx)
        })
        .collect();
    full.sort();
    full.dedup();
    for idx in full {
        let entry = &entries[idx];
        out.push(Ok(Entry {
            path: full_path(&entry.path),
            file_type: entry.file_type,
        }));
    }

    test_log!("results: {:?}", &out[results_start..]);

    // Group partial matches by directory and trigger recursion
    let mut kernels: Vec<Vec<StateId>> = vec![Vec::new(); entries.len()];
    for out in partial {
        let (idx, _) = get_entry(out.state);
        kernels[idx].push(out.state.nfa);
    }

    test_log!(
        "descending into: {:?}",
        (0..kernels.len())
            .filter(|&i| !kernels[i].is_empty())
            .map(|i| &entries[i].path)
            .collect::<Vec<_>>(),
    );
    for (i, states) in kernels.into_iter().enumerate() {
        if states.is_empty() { continue; }
        let dir = full_path(&entries[i].path);
        walk_dir(
            config,
            machine,
            Path::new(&dir),
            &states,
            recursion_depth + 1,
            out,
        );
    }
}

#[derive(Debug)]
pub struct GlobResult {
    results: Vec<std::io::Result<Entry>>,
    _private: (),
}

impl GlobResult {
    pub fn results(&self) -> &[std::io::Result<Entry>] {
        &self.results
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> + '_ {
        self.results.iter().filter_map(|r| r.as_ref().ok())
    }

    pub fn errors(&self) -> impl Iterator<Item = &std::io::Error> + '_ {
        self.results.iter().filter_map(|r| r.as_ref().err())
    }
}

pub fn glob(config: &GlobConfig, pattern: &ParsedPattern) -> GlobResult {
    let machine = from_pattern(&pattern.root);
    let mut results = Vec::new();
    walk_dir(
        config,
        &machine,
        Path::new(&pattern.base),
        &[machine.initial()],
        0,
        &mut results,
    );
    GlobResult {
        results,
        _private: (),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pattern::parse;
    use crate::testing::create_test_files;

    fn glob_files(pattern: &str, files: &[&str]) -> Vec<String> {
        let dir = create_test_files(files);
        let full = format!("{}/{}", dir.path().display(), pattern);
        let result = glob(&GlobConfig::default(), &parse(&full).unwrap());
        let prefix = format!("{}/", dir.path().display());
        let mut paths: Vec<String> = result
            .entries()
            .map(|e| e.path.strip_prefix(&prefix).unwrap().to_owned())
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn literal_name() {
        assert_eq!(
            glob_files("foo.txt", &["foo.txt", "bar.txt"]),
            ["foo.txt"]
        );
    }

    #[test]
    fn star_matches_within_component() {
        assert_eq!(
            glob_files("*.txt", &["foo.txt", "bar.txt", "baz.rs"]),
            ["bar.txt", "foo.txt"]
        );
    }

    #[test]
    fn question_matches_one_char() {
        assert_eq!(
            glob_files("?.txt", &["a.txt", "ab.txt"]),
            ["a.txt"]
        );
    }

    #[test]
    fn alternatives() {
        assert_eq!(
            glob_files("{cat,dog}.txt", &["cat.txt", "dog.txt", "bird.txt"]),
            ["cat.txt", "dog.txt"]
        );
    }

    #[test]
    fn starstar_recurses() {
        assert_eq!(
            glob_files(
                "**/*.rs",
                &["main.rs", "src/lib.rs", "src/sub/mod.rs", "README.md"]
            ),
            ["main.rs", "src/lib.rs", "src/sub/mod.rs"]
        );
    }

    #[test]
    fn star_does_not_cross_separator() {
        assert_eq!(
            glob_files(
                "src/*.rs",
                &["src/main.rs", "src/lib.rs", "src/sub/mod.rs"]
            ),
            ["src/lib.rs", "src/main.rs"]
        );
    }

    #[test]
    fn escaped_star_is_literal() {
        assert_eq!(
            glob_files(r"a\*b.txt", &["a*b.txt", "aXb.txt"]),
            ["a*b.txt"]
        );
    }
}
