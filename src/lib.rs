mod pattern;
mod nfa;
mod trie;

macro_rules! try_nested {
    ($expr:expr) => {
        match $expr {
            Ok(Some(x)) => x,
            Ok(None) => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    }
}
