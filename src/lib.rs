mod nfa;
mod pattern;
mod matcher;
mod trie;
pub mod walker;

#[cfg(test)]
mod testing;

pub use crate::nfa::Pattern;
pub use crate::pattern::ParseError;

#[cfg(test)]
macro_rules! test_log {
    ($($tok:tt)*) => {
        println!($($tok)*);
    }
}

#[cfg(not(test))]
macro_rules! test_log {
    ($($tok:tt)*) => {}
}

pub(crate) use test_log;

pub(crate) type SmallString = smallstr::SmallString<[u8; 23]>;

/// Inferred file type for filtering purposes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum FileType {
   File,
   Directory,
   /// Only returned when `follow_symlinks` is false.
   Symlink,
   Other,
}

impl From<std::fs::FileType> for FileType {
    fn from(value: std::fs::FileType) -> Self {
         match (value.is_file(), value.is_dir(), value.is_symlink()) {
            (true, false, false) => Self::File,
            (false, true, false) => Self::Directory,
            (false, false, true) => Self::Symlink,
            _ => Self::Other,
         }
    }
}

/// A file or discovery matched by a glob pattern.
#[derive(Debug)]
pub struct Entry {
   /// Path to the discovered file or directory. Directories will have a
   /// trailing '/' appended ('\' on Windows).
   pub path: String,
   /// Type of the matched file or directory.
   pub file_type: FileType,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct GlobConfig {
   /// If true, follows symlinks to the file or directory they point to. If
   /// false, symlinks are treated as irregular files. Default: `true`.
   pub follow_symlinks: bool,
   /// Maximum recursion depth. Default: 64.
   pub max_depth: usize,
   /// Matches regular files. Default: `true`.
   pub match_files: bool,
   /// Matches directories. Default: `true`.
   pub match_directories: bool,
   /// Matches non-regular files, or symlinks if `follow_symlinks` is not true.
   /// Default: `true`.
   pub match_other: bool,
   _private: (),
}

impl Default for GlobConfig {
   fn default() -> Self {
      Self {
         follow_symlinks: true,
         max_depth: 64,
         match_files: true,
         match_directories: true,
         match_other: true,
         _private: (),
      }
   }
}

impl GlobConfig {
   pub(crate) fn should_match(&self, file_type: FileType) -> bool {
      match file_type {
         FileType::File => self.match_files,
         FileType::Directory => self.match_directories,
         FileType::Symlink | FileType::Other => self.match_other,
      }
   }
}

#[derive(Debug)]
pub struct GlobResult {
    pub(crate) results: Vec<std::io::Result<Entry>>,
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
