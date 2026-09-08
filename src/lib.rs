mod pattern;
mod nfa;
mod matcher;
mod trie;

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
