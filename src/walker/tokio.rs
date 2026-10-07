//! Tokio-based parallel/async filesystem walker.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{FileType, GlobError, GlobResult};
use crate::nfa::{Pattern, StateId};
use crate::test_log;
use crate::walker::{Entry, GlobConfig, Walker, WalkerEntry};

#[derive(Debug)]
struct TokioWalker {
    inner: Walker,
    sender: UnboundedSender<Vec<Result<Entry, GlobError>>>,
}

async fn walker_entry(entry: tokio::fs::DirEntry, follow_symlinks: bool) -> Result<WalkerEntry, GlobError> {
    let path = entry.path();
    let file_type = entry.file_type().await.map_err(|e| GlobError::new(&path, e))?;
    let file_type = if follow_symlinks && file_type.is_symlink() {
        entry.metadata().await.map_err(|e| GlobError::new(&path, e))?.file_type().into()
    } else {
        FileType::from(file_type)
    };
    WalkerEntry::from_parts(&path, file_type)
}

impl TokioWalker {
    fn fork(&self) -> Self {
        Self {
            inner: self.inner.fork(),
            sender: self.sender.clone(),
        }
    }

    fn flush(&mut self) {
        if self.inner.out.is_empty() {
            return;
        }
        let chunk = std::mem::take(&mut self.inner.out);
        let _ = self.sender.send(chunk);
    }

    async fn read_dir(
        &mut self,
        cur_dir: &Path,
        recursion_depth: usize,
    ) -> Result<Vec<WalkerEntry>, GlobError> {
        if recursion_depth > self.inner.config.max_depth() {
            return Err(GlobError::new(
                cur_dir,
                std::io::Error::other("max recursion depth exceeded"),
            ));
        }

        let follow_symlinks = self.inner.config.follow_symlinks();
        let mut dir_entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(cur_dir)
            .await
            .map_err(|e| GlobError::new(cur_dir, e))?;
        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| GlobError::new(cur_dir, e))?
        {
            match walker_entry(entry, follow_symlinks).await {
                Ok(entry) => dir_entries.push(entry),
                Err(err) => self.inner.out.push(Err(err)),
            }
        }

        Ok(dir_entries)
    }

    fn visit_dir<'a>(
        &'a mut self,
        cur_dir: &'a Path,
        recursion_depth: usize,
        states: Option<Vec<StateId>>,
    ) -> Pin<Box<dyn Future<Output = Result<(), GlobError>> + Send + 'a>> {
        Box::pin(async move {
            test_log!("visiting {} (depth {recursion_depth})", cur_dir.display());

            let mut entries = self.read_dir(cur_dir, recursion_depth).await?;
            let recurse = self.inner.match_entries(cur_dir, states, &mut entries);
            self.flush();

            let mut join_set = tokio::task::JoinSet::new();
            for (i, states) in recurse.into_iter().enumerate() {
                if states.is_empty() { continue; }
                let dir = cur_dir.join(&entries[i].name[..]);
                let mut walker = self.fork();
                join_set.spawn(async move {
                    let res = walker.visit_dir(&dir, recursion_depth + 1, Some(states)).await;
                    if let Err(e) = res {
                        walker.inner.out.push(Err(e));
                    }
                    walker.flush();
                });
            }

            while let Some(res) = join_set.join_next().await {
                if let Err(e) = res {
                    let error = GlobError::new(cur_dir, std::io::Error::other(e));
                    self.inner.out.push(Err(error));
                }
            }

            Ok(())
        })
    }

    async fn walk(&mut self) {
        let cur_dir = self.inner.pattern.base_path.to_owned();
        let res = self.visit_dir(Path::new(&cur_dir[..]), 0, None).await;
        if let Err(e) = res {
            self.inner.out.push(Err(e));
        }
        self.flush();
    }
}

/// Asynchronous file glob routine using Tokio.
///
/// This routine gathers all matches/errors into a single `GlobResult` rather
/// than streaming results.
pub async fn glob(config: &GlobConfig, pattern: &Pattern) -> GlobResult {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<Vec<Result<Entry, GlobError>>>();
    let walker = Walker {
        config: Arc::new(config.clone()),
        pattern: pattern.clone(),
        out: Vec::new(),
    };
    let mut walker = TokioWalker {
        inner: walker,
        sender,
    };
    walker.walk().await;
    drop(walker);

    let mut recv = UnboundedReceiverStream::new(receiver);
    let mut results = Vec::new();
    while let Some(chunk) = recv.next().await {
        results.extend(chunk);
    }

    GlobResult {
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::create_test_files;

    async fn glob_files(pattern: &str, files: &[&str]) -> Vec<String> {
        let dir = create_test_files(files);
        let full = format!("{}/{}", dir.path().display(), pattern);
        let result = glob(&Default::default(), &Pattern::compile(&full).unwrap()).await;
        let prefix = format!("{}/", dir.path().display());
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

    #[tokio::test]
    async fn starstar() {
        assert_eq!(
            glob_files(
                "**/*.rs",
                &["main.rs", "src/lib.rs", "src/sub/mod.rs", "README.md"]
            ).await,
            ["main.rs", "src/lib.rs", "src/sub/mod.rs"],
        );
        assert_eq!(
            glob_files(
                "asdf/**",
                &["asdf/blorb.txt"]
            ).await,
            ["asdf/", "asdf/blorb.txt"],
        );
        assert_eq!(
            glob_files(
                "asdf",
                &["asdf/blorb.txt"]
            ).await,
            ["asdf/"],
        );
    }
}
