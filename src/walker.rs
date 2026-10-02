use std::fs::DirEntry;
use std::io::Result;
use std::path::Path;
use std::sync::Arc;

use crate::matcher::{Matcher, TrieEntry};
use crate::nfa::{Pattern, StateId};
use crate::trie::Trie;
use crate::{Entry, FileType, GlobConfig, SmallString, test_log};

pub mod sync;
#[cfg(feature = "tokio")]
pub mod tokio;

#[derive(Debug)]
pub(crate) struct WalkerEntry {
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
   fn from_parts(path: &Path, file_type: FileType) -> Result<Self> {
      let name: SmallString = path
         .file_name()
         .unwrap()
         .to_str()
         .ok_or_else(|| {
            let kind = std::io::ErrorKind::InvalidData;
            let message = format!("filename contains invalid UTF-8: {}", path.to_string_lossy());
            std::io::Error::new(kind, message)
         })?
         .into();
      Ok(Self { name, file_type })
   }

   fn from_dir_entry(entry: DirEntry, follow_symlinks: bool) -> Result<Self> {
      let file_type = if follow_symlinks {
         get_file_type(&entry)?
      } else {
         entry.file_type()?.into()
      };
      Self::from_parts(&entry.path(), file_type)
   }
}

#[derive(Debug)]
struct WalkerDir {
   entries: Vec<WalkerEntry>,
   trie: Trie<TrieEntry>,
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

#[derive(Debug)]
struct Walker {
   config: Arc<GlobConfig>,
   pattern: Pattern,
   out: Vec<Result<Entry>>,
}

impl Walker {
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
}
