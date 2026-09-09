use std::collections::hash_map;
use std::iter::{FusedIterator, Once};

use fnv::FnvHashMap;

pub type TrieId = u32;
pub type Char = char;

#[derive(Debug)]
pub struct TrieNode {
    children: Children,
}

impl Default for TrieNode {
    fn default() -> Self {
        Self { children: Default::default() }
    }
}

impl TrieNode {
    // Debug code
    pub fn show_accepts(&self) -> String {
        fn write_escaped(w: &mut impl std::fmt::Write, c: char) {
            if c == '"' {
                let _ = write!(w, "\\\"");
            } else {
                let _ = write!(w, "{}", c);
            }
        }
        let mut out = String::new();
        for (c, _) in self.children.iter() {
            write_escaped(&mut out, c);
        }
        out
    }
}

// TODO maybe: Squeeze this down to 8 bytes from 16
#[derive(Debug)]
enum Children {
    Empty,
    Singleton(Char, TrieId),
    Full(Box<FnvHashMap<Char, TrieId>>),
}

impl Default for Children {
    fn default() -> Self {
        Self::Empty
    }
}

impl Children {
    fn insert(&mut self, c: Char, next: TrieId) {
        match self {
            Self::Empty => {
                *self = Self::Singleton(c, next);
            }
            &mut Self::Singleton(old_c, old_next) => {
                let mut map = FnvHashMap::default();
                map.insert(old_c, old_next);
                map.insert(c, next);
                *self = Self::Full(Box::new(map));
            }
            Self::Full(map) => {
                map.insert(c, next);
            }
        };
    }

    fn get(&self, c: Char) -> Option<TrieId> {
        match self {
            Self::Empty => None,
            &Self::Singleton(x, next) if x == c => Some(next),
            Self::Singleton(_, _) => None,
            Self::Full(map) => map.get(&c).copied(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (Char, TrieId)> + '_ {
        #[derive(Debug)]
        enum Iter<'a> {
            Empty,
            Singleton(Once<(Char, TrieId)>),
            Full(hash_map::Iter<'a, Char, TrieId>),
        }

        impl<'a> Iterator for Iter<'a> {
            type Item = (Char, TrieId);

            fn next(&mut self) -> Option<Self::Item> {
                match self {
                    Iter::Empty => None,
                    Iter::Singleton(inner) => inner.next(),
                    Iter::Full(inner) => inner.next().map(|(k, v)| (*k, *v)),
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                match self {
                    Iter::Empty => (0, Some(0)),
                    Iter::Singleton(inner) => inner.size_hint(),
                    Iter::Full(inner) => inner.size_hint(),
                }
            }
        }

        impl<'a> FusedIterator for Iter<'a> {}
        impl<'a> ExactSizeIterator for Iter<'a> {}

        match self {
            Children::Empty => Iter::Empty,
            &Children::Singleton(c, id) => Iter::Singleton(std::iter::once((c, id))),
            Children::Full(hash_map) => Iter::Full(hash_map.iter()),
        }
    }
}

#[derive(Debug)]
pub struct Trie<T> {
    pub nodes: Vec<TrieNode>,
    pub values: FnvHashMap<TrieId, T>,
}

impl<T> Default for Trie<T> {
    fn default() -> Self {
        Self { nodes: Default::default(), values: Default::default() }
    }
}

impl<T> Trie<T> {
    pub fn new() -> Self {
        Self {
            nodes: vec![
                TrieNode {
                    children: Children::Empty,
                }
            ],
            values: Default::default(),
        }
    }

    pub fn root(&self) -> TrieId {
        0
    }

    pub fn insert(&mut self, key: &str, value: T) {
        let mut cur: usize = 0;
        for ch in key.chars() {
            if let Some(next) = self.nodes[cur].children.get(ch) {
                cur = next as usize;
            } else {
                let next = self.nodes.len();
                self.nodes[cur].children.insert(ch, next as TrieId);
                self.nodes.push(TrieNode::default());
                cur = next;
            }
        }
        self.values.insert(cur as TrieId, value);
    }

    // Useful for testing
    pub fn get(&self, key: &str) -> Option<&T> {
        let mut cur: usize = 0;
        for ch in key.chars() {
            let next = self.nodes[cur].children.get(ch)?;
            cur = next as usize;
        }
        self.values.get(&(cur as TrieId))
    }

    pub fn get_value(&self, id: TrieId) -> Option<&T> {
        self.values.get(&id)
    }

    pub fn children(&self, id: TrieId) -> impl Iterator<Item = (Char, TrieId)> + '_ {
        self.nodes[id as usize].children.iter()
    }
}

impl<S, T> FromIterator<(S, T)> for Trie<T>
    where S: AsRef<str>
{
    fn from_iter<I: IntoIterator<Item = (S, T)>>(iter: I) -> Self {
        let mut trie = Trie::new();
        trie.extend(iter);
        trie
    }
}

impl<S, T> Extend<(S, T)> for Trie<T>
    where S: AsRef<str>
{
    fn extend<I: IntoIterator<Item = (S, T)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k.as_ref(), v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_and_insert() {
        let mut trie = Trie::new();
        assert_eq!(trie.get("foo"), None);

        trie.insert("foo", 1);
        trie.insert("bar", 2);
        trie.insert("fo", 3);

        assert_eq!(trie.get("foo"), Some(&1));
        assert_eq!(trie.get("bar"), Some(&2));
        assert_eq!(trie.get("fo"), Some(&3));
        assert_eq!(trie.get("f"), None);
        assert_eq!(trie.get("foos"), None);
        assert_eq!(trie.get("baz"), None);

        trie.insert("foo", 4);
        assert_eq!(trie.get("foo"), Some(&4));
        assert_eq!(trie.get("fo"), Some(&3));
    }
}
