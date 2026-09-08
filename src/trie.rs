use fnv::FnvHashMap;

pub type TrieId = u32;
pub type Char = char;

#[derive(Debug)]
struct TrieNode {
    children: Children,
}

impl Default for TrieNode {
    fn default() -> Self {
        Self { children: Default::default() }
    }
}

// TODO maybe: Squeeze this down from 16 bytes to 8
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
}

#[derive(Debug)]
pub struct Trie<T> {
    nodes: Vec<TrieNode>,
    values: FnvHashMap<TrieId, T>,
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
