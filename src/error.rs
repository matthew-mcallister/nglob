use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// An error encountered while searching for files.
#[derive(Debug)]
pub struct GlobError {
    path: PathBuf,
    error: io::Error,
}

impl GlobError {
    pub(crate) fn new(path: impl Into<PathBuf>, error: io::Error) -> Self {
        Self {
            path: path.into(),
            error,
        }
    }

    /// The file path that produced the error.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn escape_path(path: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if path.chars().any(|c| matches!(c, '"' | '\\' | ' ')) {
        write!(f, "\"")?;
        for c in path.chars() {
            if matches!(c, '"' | '\\' | ' ') {
                write!(f, "\\")?;
            }
            write!(f, "{c}")?;
        }
        write!(f, "\"")
    } else {
        write!(f, "{path}")
    }
}

impl fmt::Display for GlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        escape_path(&self.path.to_string_lossy(), f)?;
        write!(f, ": {}", self.error)
    }
}

impl Error for GlobError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(path: &str) -> GlobError {
        GlobError::new(path, io::Error::new(io::ErrorKind::NotFound, "not found"))
    }

    #[test]
    fn display() {
        assert_eq!(err("/foo/bar").to_string(), "/foo/bar: not found");
        assert_eq!(err("/foo bar").to_string(), "\"/foo\\ bar\": not found");
        assert_eq!(err("/foo\"bar").to_string(), "\"/foo\\\"bar\": not found");
        assert_eq!(err("/foo\\bar").to_string(), "\"/foo\\\\bar\": not found");
    }
}
