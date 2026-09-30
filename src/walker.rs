mod sync;

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

#[derive(Debug)]
pub struct Entry {
   pub path: String,
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
}

impl Default for GlobConfig {
   fn default() -> Self {
      Self {
         follow_symlinks: true,
         max_depth: 64,
         match_files: true,
         match_directories: true,
         match_other: true,
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
