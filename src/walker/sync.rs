use std::io::Result;
use std::path::{Path, PathBuf};

use crate::matcher::find_matching_entries;
use crate::nfa::{StateId, StateMachine};
use crate::pattern::Pattern;
use crate::trie::Trie;
use crate::walker::{Entry, FileType};

#[derive(Debug)]
struct Walker {
    cur_dir: PathBuf,
    pattern: Pattern,
    machine: StateMachine,
}

fn from_dir_entry(entry: std::fs::DirEntry) -> Result<Entry> {
    let path = entry.path().into_os_string().into_string()
        .map_err(|_| {
            let kind = std::io::ErrorKind::InvalidData;
            let message = format!("filename contains invalid UTF-8: {}", entry.path().to_string_lossy());
            std::io::Error::new(kind, message)
        })?;
    Ok(Entry {
        path,
        file_type: entry.file_type()?.into(),
    })
}

fn walk_dir(
    machine: &StateMachine,
    dir: &Path,
    prior_states: &[StateId],
    out: &mut Vec<Result<Entry>>,
) -> Result<()> {
    let dir_entries = std::fs::read_dir(dir)?;

    let mut entries: Vec<Entry> = Vec::with_capacity(32);
    for d in &["", ".", ".."] {
        // Literal-only matches
        entries.push(Entry {
            path: dir.join(d).into_string().unwrap(),
            file_type: FileType::Directory,
        });
    }
    for e in dir_entries {
        match e.and_then(from_dir_entry) {
            Ok(e) => entries.push(e),
            Err(err) => out.push(Err(err)),
        }
    }

    let trie: Trie<usize> = entries.iter()
        .enumerate()
        .map(|(i, e)| (&e.path, i))
        .collect();

    let matches = find_matching_entries(
        machine,
        &trie,
        prior_states,
    );

    // We want to prune subdirectories before descending. We already know that
    // some directories are full matches and not partial matches because they
    // matched the terminal state.
    //
    // However, some patterns end in a trailing `/`, and we want to treat those
    // as full matches without descending. We also want to avoid matching a
    // pattern that ends in `/*` as it may be unintended for `src/*` to match
    // `src/`, yet `/{,*}` *should* be matched because it is explicit/literal
    // in matching a trailing `/`.
    //
    // This is solved by running a pruning step on directories which follows
    // all Sep transitions and literal epsilons. States that reach terminal are
    // full matches and get pruned; all remaining states trigger recursion.

    todo!()
}