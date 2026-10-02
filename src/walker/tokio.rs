use std::io::Result;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::nfa::{Pattern, StateId};
use crate::walker::{Entry, GlobConfig, Walker, WalkerEntry};

#[derive(Debug)]
struct TokioWalker {
    inner: Walker,
    sender: UnboundedSender<Vec<Result<Entry>>>,
}

impl TokioWalker {
    async fn read_dir(
        &mut self,
        cur_dir: &Path,
        recursion_depth: usize,
    ) -> Result<Vec<WalkerEntry>> {
        todo!()
    }

    async fn visit_dir(
        &mut self,
        cur_dir: &Path,
        recursion_depth: usize,
        states: Option<Vec<StateId>>,
    ) -> Result<()> {
        todo!()
    }

    async fn walk(&mut self) {
        todo!()
    }
}

#[derive(Debug)]
pub struct GlobResult {
    results: Vec<Result<Entry>>,
    _private: (),
}

impl GlobResult {
    pub fn results(&self) -> &[Result<Entry>] {
        &self.results
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> + '_ {
        self.results.iter().filter_map(|r| r.as_ref().ok())
    }

    pub fn errors(&self) -> impl Iterator<Item = &std::io::Error> + '_ {
        self.results.iter().filter_map(|r| r.as_ref().err())
    }
}

pub async fn glob(config: GlobConfig, pattern: Pattern) -> GlobResult {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<Vec<Result<Entry>>>();
    let walker = Walker {
        config: Arc::new(config),
        pattern,
        out: Vec::new(),
    };
    let mut walker = TokioWalker {
        inner: walker,
        sender,
    };
    walker.walk().await;

    let mut recv = UnboundedReceiverStream::new(receiver);
    let mut results = Vec::new();
    while let Some(chunk) = recv.next().await {
        results.extend(chunk);
    }

    GlobResult {
        results,
        _private: (),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::create_test_files;

    async fn glob_files(pattern: &str, files: &[&str]) -> Vec<String> {
        todo!()
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
