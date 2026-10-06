//! # nglob
//!
//! nglob is a Rust library for file globbing. It supports synchronous and
//! parallel async directory search, string matching, filetype filters, and
//! both Windows and Unix-like filesystems.
//!
//! ## Configuration
//!
//! The [`GlobConfig`] struct contains a list of options you can pass to the
//! search.
//!
//! - `follow_symlinks` (default: `true`): Follows symlinks to the file or
//!   directory they point to. When `false`, symlinks will be treated as
//!   non-regular files.
//! - `max_depth` (default: 64): Maximum recursion depth. This prevents symlink
//!   cycles from causing infinite recursion.
//! - `match_files` (default: `true`): When `true`, matches regular files.
//! - `match_directories` (default: `true`): When `true`, matches directories.
//! - `match_other` (default: `true`): When `true`, matches non-regular files,
//!   such as Unix sockets, or symlinks (when `follow_symlinks` is false).
//!
//! #### Example
//!
//! This config will match only regular files.
//! ```rust
//! use nglob::GlobConfig;
//!
//! let config = GlobConfig::new()
//!     .with_match_files(true)
//!     .with_match_directories(false)
//!     .with_match_other(false);
//! ```
//!
//! ## Choosing a walker
//!
//! nglob works by walking your filesystem starting from the topmost directory
//! that is part of your pattern and descending into subdirectories. Which
//! walker you should use depends on your use case:
//!
//! - [`walker::sync::glob`]: Synchronous, single-threaded directory search.
//! - [`walker::tokio::glob`]: Requires the `tokio` feature. Parallel,
//!   asynchronous search based on tokio.
//! - [`walker::string::glob`]: Matches against a list of file path strings
//!   without interacting with the filesystem. This is intended for searching
//!   filepaths inside applications that deal with archives, HTTP directories,
//!   sandboxes, and the like.
//!
//! #### Synchronous matching
//!
//! Here's an example of synchronous directory matching.
//!
//! ```rust
//! use nglob::{GlobConfig, Pattern};
//! use nglob::walker::sync::glob;
//!
//! let config = GlobConfig::default();
//! let pattern = Pattern::compile("src/**.rs").unwrap();
//! let output = glob(config, pattern);
//! for result in output.results() {
//!     match result {
//!         Ok(e) => println!("{}", e.path),
//!         Err(e) => println!("error: {}", e),
//!     }
//! }
//! ```
//!
//! #### Asynchronous matching
//!
//! The Tokio-based async matcher [`walker::tokio::glob`] uses the same
//! interface as
//!
//! ```rust
//! # #[cfg(feature = "tokio")]
//! # fn main() {
//! # async {
//! use nglob::{GlobConfig, Pattern};
//! use nglob::walker::tokio::glob;
//!
//! let config = GlobConfig::default();
//! let pattern = Pattern::compile("src/**.rs").unwrap();
//! let output = glob(config, pattern).await;
//! for result in output.results() {
//!     match result {
//!         Ok(e) => println!("{}", e.path),
//!         Err(e) => println!("error: {}", e),
//!     }
//! }
//! # };}
//! # #[cfg(not(feature = "tokio"))]
//! # fn main() {}
//! ```
//!
//! #### String matching
//!
//! nglob can do matching against bare file paths without touching the
//! filesystem. Currently, paths are treated according to the same semantics as
//! the host operating system, so be aware there might be minute differences
//! depending on the machine. Every path is treated as a file; don't include
//! any directories in your list of file paths.
//!
//! Unlike file matching `glob` methods, string matching takes no configuration
//! and returns strings instead of a `GlobResult`.
//!
//! ```rust
//! use nglob::Pattern;
//! use nglob::walker::string::glob;
//!
//! let pattern = Pattern::compile("src/**.rs").unwrap();
//! let matches = glob(pattern, &["README.md", "src/main.rs", "src/unix/mod.rs"]);
//! for string in matches {
//!     println!("{}", string);
//! }
//! ```
//!
//! ## Error handling
//!
//! File matching glob methods return a `GlobResult` struct which contains both
//! matches and errors in the order they were encountered. Results from a
//! single directory are always grouped together even when matching in
//! parallel.
//!
//! ## Compiling a pattern
//!
//! A pattern string must be compiled into a [`Pattern`] object using the
//! [`Pattern::compile`] method before it can be used. If parsing the pattern
//! fails, a [`ParseError`] is returned. Patterns are reference counted and
//! cheap to clone.
//!
//! ```rust
//! # use nglob::Pattern;
//! Pattern::compile("{a,b,c}"); // OK
//! Pattern::compile("{a,b");    // Error: unmatched brace
//! ```
//!
//! ## Path separators
//!
//! The host operating system determines which characters are treated as a path
//! separator in a pattern. Note that, on Windows, the path separator `\` must
//! be escaped as `\\`. Windows does not allow `\` in filenames, so `\` is
//! always treated as a path separator.
//!
//! Repeated path separators will be treated as redundant in both patterns and
//! file paths. For example, `foo///*` will match `foo/bar`, and `foo/*` will
//! match `foo///bar`.
//!
//! ## Special characters
//!
//! #### Wilcard (`?`)
//!
//! The wildcard character matches any character in a file or directory name.
//! It does not match a path separator. E.g. `foo.?s` will match `foo.js`,
//! `foo.ts`, and `foo.rs`, but not `foo/rs`.
//!
//! #### Star (`*`)
//!
//! The star character matches zero or more characters in a file or directory
//! name. For example, `foo.*` will match `foo.js`, `foo.min.js`, and so on.
//!
//! Star never matches a path separator. So `log*.txt` will match `log0.txt`
//! but not `log/0.txt`.
//!
//! Star also never matches a totally empty filename. For example, `foo/*` will
//! match `foo/bar` but not `foo/`, and `foo/*/baz` will match `foo/bar/baz`
//! but not `foo//baz` (which is equivalent to `foo/baz`).
//!
//! #### Star-star (`**`)
//!
//! Two stars in a row will match zero or more characters, *including* path
//! separators. For example, `**.rs` will match `main.rs`, `io/mod.rs`,
//! `io/unix/mod.rs`, and so on.
//!
//! More precisely, `**` will match zero or more *path components*. This means
//! that `**/*.rs` can match `foo.rs` as well as `src/foo.rs`, and `src/**`
//! will match `src/` as well as `src/tests/`.
//!
//! #### Alternatives (`{,}`)
//!
//! Curly braces can be used to enclose a comma-separated list of subpatterns,
//! and any one of the subpatterns may be matched.
//!
//! All special syntax will work as expected when nested under `{}`. Examples:
//! - `{a,{b,c}}` matches `ab` and `ac`.
//! - `a{/,?}b` matches `a/b` and `a.b`.
//! - `a{/,*}b` matches `ab` and `a/b`.
//! - `a{}b` matches `ab`.
//!
//! #### Escape sequences (`\`)
//!
//! Every special character in a pattern may be escaped using the escape
//! character (`\`).
//!
//! List of valid escape sequences: `\\`, `\?`, `\*`, `\{`, `\}`, `\,`.
//!
//! ## Special directories (`.` and `..`)
//!
//! The special directories `.` and `..` can be matched by patterns, but only
//! by literal `.` characters. The wildcard matching patterns `?`, `*`, and
//! `**` will never match these directories, even in a pattern like `.?` or
//! `.*`. If you *want* to match `..`, try an alternative pattern like `.{.,?}`
//! instead.
//!
//! ## Absolute paths
//!
//! If a pattern begins with a literal prefix that matches an absolute file
//! path (e.g. `/home`, `C:\\Windows`, `//host/share`), that will be used as
//! the base directory for searches. Otherwise, the pattern is treated as a
//! relative path and the current directory is used as the search base.
//!
//! Every search must start from a unique base directory. If a pattern begins
//! with a non-literal prefix, such as `{C,D}:\\` or `//host/*`, it will be
//! treated as a relative file path rather than searching from multiple bases.

#![cfg_attr(docsrs, doc(auto_cfg(hide(loom))))]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

mod config;
mod nfa;
mod pattern;
mod matcher;
mod trie;
pub mod walker;

#[cfg(test)]
mod testing;

pub use crate::config::GlobConfig;
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

/// File type, used for filtering and returned as part of the glob result set.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum FileType {
   /// A regular file.
   File,
   /// A directory.
   Directory,
   /// A symlink. Only returned when `follow_symlinks` is false.
   Symlink,
   /// A non-regular file.
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
#[non_exhaustive]
pub struct Entry {
   /// Path to the discovered file or directory. Directories will have a
   /// trailing '/' appended ('\' on Windows).
   pub path: String,
   /// Type of the matched file or directory.
   pub file_type: FileType,
}

/// Results of a glob file search, containing both matches and errors.
#[derive(Debug)]
pub struct GlobResult {
   pub(crate) results: Vec<std::io::Result<Entry>>,
}

impl GlobResult {
   /// All results and errors encounted by the matcher.
   pub fn results(&self) -> &[std::io::Result<Entry>] {
      &self.results
   }

   /// Iterates over matched entries, skipping errors.
   pub fn entries(&self) -> impl Iterator<Item = &Entry> + '_ {
      self.results.iter().filter_map(|r| r.as_ref().ok())
   }

   /// Iterates over any encountered errors, skipping matches.
   pub fn errors(&self) -> impl Iterator<Item = &std::io::Error> + '_ {
      self.results.iter().filter_map(|r| r.as_ref().err())
   }
}
