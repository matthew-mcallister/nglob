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
