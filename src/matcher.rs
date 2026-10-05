use std::collections::hash_map::Entry;

use fnv::{FnvBuildHasher, FnvHashMap};

use crate::nfa::{State, StateId, Pattern, Transition, TransitionRule};
use crate::test_log;
use crate::trie::{Trie, TrieId};

#[cfg(test)] use std::fmt::Write;
#[cfg(test)] use crate::trie::TrieNode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrieEntry {
    pub(crate) index: u32,
    pub(crate) is_dir: bool,
    /// If true, only literal patterns match. Used by ""/"."/".."
    pub(crate) is_literal: bool,
}

/// We store live states as StateKey + StateFlags, which is compact and merges
/// states with different is_literal values, but for actual transition
/// calculations we use the richer MatcherState type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct StateKey {
    pub(crate) pattern: StateId,
    pub(crate) trie: TrieId,
    pub(crate) next_component: bool,
}

impl std::fmt::Display for StateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "t{},p{}", self.trie, self.pattern)?;
        if self.next_component {
            write!(f, ",/")?;
        }
        Ok(())
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct StateFlags: u8 {
        const IS_LITERAL = 1;
    }
}

impl Default for StateFlags {
    fn default() -> Self {
        Self::IS_LITERAL
    }
}

// Merges state data when two automata arrive at the same state. Returns
// true if the state changed.
fn merge_state(states: &mut StateSet, state: StateKey, flags: StateFlags) -> bool {
    match states.entry(state) {
        Entry::Occupied(mut entry) => {
            let merged = *entry.get() | flags;
            if merged == *entry.get() {
                false
            } else {
                entry.insert(merged);
                true
            }
        }
        Entry::Vacant(entry) => {
            entry.insert(flags);
            true
        }
    }
}

type StateSet = FnvHashMap<StateKey, StateFlags>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExpandedStateFlags {
    /// True if the state is reachable while only matching literal chars and
    /// never ?/*/**.
    is_literal: bool,
    /// If trie ID points at a matched directory, can follow NextComponent
    /// transitions which set this flag.
    next_component: bool,
}

impl Default for ExpandedStateFlags {
    fn default() -> Self {
        Self { is_literal: true, next_component: false }
    }
}

impl ExpandedStateFlags {
    fn with_literal(self, value: bool) -> Self {
        Self { is_literal: value, ..self }
    }

    fn with_next_component(self, value: bool) -> Self {
        Self { next_component: value, ..self }
    }
}

#[derive(Clone, Copy, Debug)]
struct ExpandedState<'a> {
    pattern: &'a Pattern,
    pattern_id: StateId,
    trie: &'a Trie<TrieEntry>,
    trie_id: TrieId,
    flags: ExpandedStateFlags
}

impl<'a> ExpandedState<'a> {
    fn raise(
        pattern: &'a Pattern,
        trie: &'a Trie<TrieEntry>,
        key: StateKey,
        flags: StateFlags,
    ) -> Self {
        let flags = ExpandedStateFlags {
            next_component: key.next_component,
            is_literal: flags.contains(StateFlags::IS_LITERAL),
        };
        Self {
            pattern: pattern,
            pattern_id: key.pattern,
            trie,
            trie_id: key.trie,
            flags,
        }
    }

    fn lower(&self) -> (StateKey, StateFlags) {
        let key = StateKey {
            pattern: self.pattern_id,
            trie: self.trie_id,
            next_component: self.flags.next_component,
        };
        let mut flags = StateFlags::default();
        flags.set(StateFlags::IS_LITERAL, self.flags.is_literal);
        (key, flags)
    }

    fn pattern_node(&self) -> &State {
        self.pattern.get(self.pattern_id)
    }

    #[cfg(test)]
    fn trie_node(&self) -> &TrieNode {
        self.trie.get(self.trie_id).unwrap()
    }

    fn trie_entry(&self) -> Option<&TrieEntry> {
        self.trie.get_value(self.trie_id)
    }

    fn is_dir(&self) -> bool {
        self.trie_entry().map_or(false, |n| n.is_dir)
    }

    fn is_match(&self) -> bool {
        if self.pattern_id == self.pattern.terminal()
            && let Some(entry) = self.trie_entry()
            && (!entry.is_literal || self.is_literal())
        {
            true
        } else {
            false
        }
    }

    fn is_recurse(&self) -> bool {
        if self.flags.next_component
            && self.is_epsilon_frontier()
            && let Some(entry) = self.trie_entry()
            && entry.is_dir
            && (!entry.is_literal || self.is_literal())
        {
            true
        } else {
            false
        }
    }

    fn is_literal(&self) -> bool {
        self.flags.is_literal
    }

    /// State has any transitions which are not epsilons
    fn is_epsilon_frontier(&self) -> bool {
        let node = self.pattern_node();
        if self.flags.next_component {
            node.is_component_frontier
        } else {
            node.is_epsilon_frontier
        }
    }

    fn follow_char(&self, c: char, transition: &Transition) -> Option<ExpandedStateFlags> {
        if self.flags.next_component { return None; }
        match transition.rule {
            TransitionRule::Char(ch) if c == ch => Some(self.flags),
            TransitionRule::Char(_) => None,
            TransitionRule::Wildcard => Some(self.flags.with_literal(false)),
            // Epsilon
            TransitionRule::Epsilon
            | TransitionRule::WildEpsilon
            | TransitionRule::NextComponent
            | TransitionRule::WildNextComponent => None,
        }
    }

    fn follow_epsilon(&self, transition: &Transition) -> Option<ExpandedStateFlags> {
        match transition.rule {
            TransitionRule::Epsilon => Some(self.flags),
            // This prevents xyz/* from matching bare xyz/
            TransitionRule::WildEpsilon if self.flags.next_component => None,
            TransitionRule::WildEpsilon => Some(self.flags.with_literal(false)),
            TransitionRule::NextComponent if self.is_dir() => Some(self.flags.with_next_component(true)),
            TransitionRule::NextComponent => None,
            // This prevents ** from matching zero components recursively
            TransitionRule::WildNextComponent if self.is_dir() => Some(self.flags.with_next_component(true).with_literal(false)),
            TransitionRule::WildNextComponent => None,
            // Non-epsilon
            TransitionRule::Char(_) | TransitionRule::Wildcard => None,
        }
    }

    /// Follows all state transitions along matching trie edges.
    fn expand(&self) -> impl Iterator<Item = Self> + '_ {
        self.trie.children(self.trie_id)
            .flat_map(move |x|
                self.pattern.get(self.pattern_id)
                    .transitions.iter()
                    .map(move |t| (x, t))
            )
            .filter_map(move |((c, tr_id), t)| {
                #[cfg(test)]
                let (key, _) = self.lower();
                if let Some(flags) = self.follow_char(c, t) {
                    test_log!("{key},{c},{t:?}... match");
                    Some(Self {
                        pattern: self.pattern,
                        pattern_id: t.next,
                        trie: self.trie,
                        trie_id: tr_id,
                        flags,
                    })
                } else {
                    test_log!("{key},{c},{t:?}... no match");
                    None
                }
            })
    }

    fn expand_epsilon(&self) -> impl Iterator<Item = Self> + '_ {
        self.pattern.get(self.pattern_id).transitions.iter()
            .filter_map(move |t| {
                #[cfg(test)]
                let (key, _) = self.lower();
                if let Some(flags) = self.follow_epsilon(t) {
                    test_log!("{key},{t:?}... match");
                    Some(Self {
                        pattern: self.pattern,
                        pattern_id: t.next,
                        trie: self.trie,
                        trie_id: self.trie_id,
                        flags,
                    })
                } else {
                    test_log!("{key},{t:?}... no match");
                    None
                }
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Output {
    pub(crate) state: StateId,
    pub(crate) index: usize,
}

#[derive(Debug)]
pub(crate) struct Matcher<'a> {
    pattern: &'a Pattern,
    trie: &'a Trie<TrieEntry>,
    states: StateSet,
    old_states: StateSet, // Reuse memory
    queue: Vec<(StateKey, StateFlags)>, // Reuse memory
    // States that matched a path component but aren't full matches
    pub(crate) recurse: Vec<Output>,
    // Full matches
    pub(crate) full: Vec<Output>,
}

impl<'a> Matcher<'a> {
    pub(crate) fn new(
        pattern: &'a Pattern,
        trie: &'a Trie<TrieEntry>,
        states: Option<Vec<StateId>>,
    ) -> Self {
        let mut s =
            StateSet::with_capacity_and_hasher(pattern.states.len(), FnvBuildHasher::new());
        if let Some(states) = states {
            for state in states {
                let key = StateKey {
                    pattern: state,
                    trie: trie.root(),
                    next_component: false,
                };
                s.insert(key, Default::default());
            }
        } else {
            let initial = StateKey {
                pattern: pattern.initial(),
                trie: trie.root(),
                next_component: false,
            };
            s.insert(initial, Default::default());
        }
        Self {
            pattern,
            trie,
            states: s,
            old_states: StateSet::with_capacity_and_hasher(
                pattern.states.len(),
                FnvBuildHasher::new(),
            ),
            queue: Vec::new(),
            recurse: Vec::new(),
            full: Vec::new(),
        }
    }

    fn record_matches(&mut self) {
        for (&key, &flags) in self.states.iter() {
            if let Some(e) = self.trie.get_value(key.trie) {
                let state = ExpandedState::raise(&self.pattern, &self.trie, key, flags);
                let output = Output {
                    state: key.pattern,
                    index: e.index as usize,
                };
                if state.is_match() {
                    self.full.push(output);
                } else if state.is_recurse() {
                    self.recurse.push(output);
                }
            }
        }
    }

    /// Follows epsilon transitions (incl. NextComponent) until all states are
    /// at frontier.
    fn expand_epsilon(&mut self) {
        self.queue.extend(self.states.iter().map(|(&k, &v)| (k, v)));
        while let Some((key, flags)) = self.queue.pop() {
            let state = ExpandedState::raise(&self.pattern, &self.trie, key, flags);
            for successor in state.expand_epsilon() {
                let (key, flags) = successor.lower();
                if merge_state(&mut self.states, key, flags) {
                    self.queue.push(successor.lower());
                }
            }
        }

        self.record_matches();

        // Filter out useless states
        self.states.retain(|key, flags| {
            ExpandedState::raise(&self.pattern, &self.trie, *key, *flags).is_epsilon_frontier()
        });
    }

    fn expand_matching(&mut self) {
        std::mem::swap(&mut self.old_states, &mut self.states);
        self.states.clear();
        for (&src_key, &src_flags) in self.old_states.iter() {
            let s = ExpandedState::raise(&self.pattern, &self.trie, src_key, src_flags);
            for successor in s.expand() {
                let (key, flags) = successor.lower();
                merge_state(&mut self.states, key, flags);
            }
        }
    }

    #[cfg(test)]
    fn show_states(&self) -> String {
        let mut out = String::new();
        let _ = write!(out, "[");
        for (i, (key, flags)) in self.states.iter().enumerate() {
            let state = ExpandedState::raise(self.pattern, self.trie, *key, *flags);
            if i > 0 {
                let _ = write!(out, ", ");
            }
            let _ = write!(
                out,
                r#"(t{} "{}", s{} "{}"{})"#,
                state.trie_id,
                state.trie_node().show_accepts(),
                state.pattern_id,
                state.pattern_node().show_accepts(),
                if state.flags.is_literal { " lit" } else { "" },
            );
        }
        let _ = write!(out, "]");
        out
    }

    fn step(&mut self) {
        self.expand_epsilon();
        test_log!("ε {}", self.show_states());
        self.expand_matching();
        test_log!("o {}", self.show_states());
    }

    // Runs until full trie has been consumed, or no more live states remain.
    pub(crate) fn run(&mut self) {
        test_log!("{:?}", self.pattern.states);
        test_log!("{:?}", self.trie.nodes);
        while !self.states.is_empty() {
            self.step();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn match_trie<'m, 't>(
        pattern: &'m Pattern,
        trie: &'t Trie<TrieEntry>,
    ) -> Vec<Output> {
        let mut matcher = Matcher::new(pattern, trie, None);
        matcher.run();
        matcher.full
    }

    fn matches(pattern: &str, target: &str) -> bool {
        let mut trie = Trie::new();
        trie.insert(target, TrieEntry { index: 0, is_dir: false, is_literal: false });
        let pattern = Pattern::compile_without_base(pattern).unwrap();
        !match_trie(&pattern, &trie).is_empty()
    }

    fn match_all<'t>(pattern: &str, targets: &[&'t str]) -> Vec<&'t str> {
        let mut trie = Trie::new();
        for (index, target) in targets.iter().enumerate() {
            trie.insert(target, TrieEntry { index: index as u32, is_dir: false, is_literal: false });
        }
        let pattern = Pattern::compile_without_base(pattern).unwrap();
        let matches = match_trie(&pattern, &trie);
        matches.into_iter().map(|Output { index, .. }| targets[index]).collect()
    }

    fn matches_literal(pattern: &str, target: &str) -> bool {
        let mut trie = Trie::new();
        trie.insert(target, TrieEntry { index: 0, is_dir: false, is_literal: true });
        let pattern = Pattern::compile_without_base(pattern).unwrap();
        !match_trie(&pattern, &trie).is_empty()
    }

    #[test]
    fn test_matches() {
        assert!(matches("", ""));
        assert!(matches("{}", ""));
        assert!(matches("{,a}", ""));
        assert!(matches("asdf", "asdf"));
        assert!(matches("{a}{b}", "ab"));
        assert!(!matches("asdf", "fdsa"));
        assert!(!matches("asdf", ""));
        assert!(!matches("asd", "asdf"));

        assert!(matches("{a,bc}", "a"));
        assert!(matches("{a,bc}", "bc"));
        assert!(!matches("{a,bc}", "b"));
        assert!(matches("{a,*}", "1"));
        assert!(matches("{a,*}", "b"));

        assert!(matches("*", "asdf"));
        assert!(matches("*", ""));
        assert!(matches("*", "a"));
        assert!(matches("**", ""));
        assert!(matches("**", "a"));

        assert!(matches("a*f", "asdf"));
        assert!(matches("a*f", "af"));
        assert!(!matches("a*f", "asd"));
        assert!(!matches("a*f", "asdc"));
    }

    #[test]
    fn test_match_all() {
        assert_eq!(match_all("ab?b", &["abab", "aaab"]), vec!["abab"]);
        assert_eq!(match_all("a?ab", &["abab", "aaab"]), vec!["abab", "aaab"]);
    }

    #[test]
    fn test_match_literal() {
        assert!(matches_literal("", ""));
        assert!(matches_literal("{}", ""));
        assert!(matches_literal("..", ".."));
        assert!(matches_literal("{*,..}", ".."));
        assert!(matches("*", "") && !matches_literal("*", ""));
        assert!(matches("*", "..") && !matches_literal("*", ".."));
        assert!(matches("**", "") && !matches_literal("**", ""));
        assert!(matches("**", "..") && !matches_literal("**", ".."));
    }
}
