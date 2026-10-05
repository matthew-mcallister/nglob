use crate::FileType;

/// Configuration options for directory traversal.
#[derive(Clone, Debug)]
pub struct GlobConfig {
    follow_symlinks: bool,
    max_depth: usize,
    match_files: bool,
    match_directories: bool,
    match_other: bool,
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
    /// Creates a configuration with all options set to their defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Follows symlinks to the file or directory they point to. If false,
    /// symlinks are treated as non-regular files. Default: `true`.
    pub fn with_follow_symlinks(mut self, follow_symlinks: bool) -> Self {
        self.follow_symlinks = follow_symlinks;
        self
    }

    /// Maximum recursion depth. Default: 64.
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Matches regular files. Default: `true`.
    pub fn with_match_files(mut self, match_files: bool) -> Self {
        self.match_files = match_files;
        self
    }

    /// Matches directories. Default: `true`.
    pub fn with_match_directories(mut self, match_directories: bool) -> Self {
        self.match_directories = match_directories;
        self
    }

    /// Matches non-regular files, or symlinks if `follow_symlinks` is not
    /// true. Default: `true`.
    pub fn with_match_other(mut self, match_other: bool) -> Self {
        self.match_other = match_other;
        self
    }

    /// Whether symlinks are followed. Default: `true`.
    pub fn follow_symlinks(&self) -> bool {
        self.follow_symlinks
    }

    /// Maximum recursion depth. Default: 64.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Whether regular files are matched. Default: `true`.
    pub fn match_files(&self) -> bool {
        self.match_files
    }

    /// Whether directories are matched. Default: `true`.
    pub fn match_directories(&self) -> bool {
        self.match_directories
    }

    /// Whether non-regular files are matched. Default: `true`.
    pub fn match_other(&self) -> bool {
        self.match_other
    }

    pub(crate) fn should_match(&self, file_type: FileType) -> bool {
        match file_type {
            FileType::File => self.match_files,
            FileType::Directory => self.match_directories,
            FileType::Symlink | FileType::Other => self.match_other,
        }
    }
}
