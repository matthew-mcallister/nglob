use std::collections::hash_map::Entry;
use std::fmt::Write;

use fnv::{FnvBuildHasher, FnvHashMap};

use crate::nfa::{State, StateId, StateMachine, Transition, TransitionRule};
use crate::test_log;
use crate::trie::{Trie, TrieId, TrieNode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrieEntry {
    pub index: usize,
    pub is_dir: bool,
}

/// We store live states as StateKey + StateFlags, which is compact and merges
/// states with different is_literal values, but for actual transition
/// calculations we use the richer MatcherState type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct StateKey {
    pub pattern: StateId,
    pub trie: TrieId,
    pub next_component: bool,
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
    pub struct StateFlags: u8 {
        const IS_LITERAL = 1;
    }
}

impl Default for StateFlags {
    fn default() -> Self {
        Self::IS_LITERAL
    }
}

// Merges state data when two automata arrive at the same state. Returns
// `Some(merged_flags)` if state changed.
fn merge_state(states: &mut StateSet, state: StateKey, flags: StateFlags) -> Option<StateFlags> {
    match states.entry(state) {
        Entry::Occupied(mut entry) => {
            let merged = *entry.get() | flags;
            if merged == *entry.get() {
                None
            } else {
                entry.insert(merged);
                Some(merged)
            }
        }
        Entry::Vacant(entry) => {
            entry.insert(flags);
            Some(flags)
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
    pattern: &'a StateMachine,
    pattern_id: StateId,
    trie: &'a Trie<TrieEntry>,
    trie_id: TrieId,
    flags: ExpandedStateFlags
}

impl<'a> ExpandedState<'a> {
    fn raise(
        pattern: &'a StateMachine,
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
        &self.pattern[self.pattern_id]
    }

    fn trie_node(&self) -> &TrieNode {
        self.trie.get(self.trie_id).unwrap()
    }

    fn is_match(&self) -> bool {
        self.trie.get_value(self.trie_id).is_some()
    }

    fn is_dir(&self) -> bool {
        self.trie.get_value(self.trie_id).map_or(false, |n| n.is_dir)
    }

    fn is_terminal(&self) -> bool {
        self.pattern_id == self.pattern.terminal()
            && self.trie.get_value(self.trie_id).is_some()
    }

    fn is_literal(&self) -> bool {
        self.flags.is_literal
    }

    /// State has any transitions which are not epsilons
    fn is_epsilon_frontier(&self) -> bool {
        if self.flags.next_component {
            self.pattern_node()
                .transitions
                .iter()
                .any(|t| !matches!(t.rule, TransitionRule::Epsilon | TransitionRule::NextComponent))
        } else {
            self.pattern_node()
                .transitions
                .iter()
                .any(|t| !matches!(
                    t.rule,
                    TransitionRule::Epsilon
                        | TransitionRule::WildEpsilon
                        | TransitionRule::NextComponent,
                ))
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
            | TransitionRule::NextComponent => None,
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
            // Non-epsilon
            TransitionRule::Char(_) | TransitionRule::Wildcard => None,
        }
    }

    /// Follows all state transitions along matching trie edges.
    fn expand(&self) -> impl Iterator<Item = Self> + '_ {
        self.trie.children(self.trie_id)
            .flat_map(move |x| self.pattern[self.pattern_id].transitions.iter().map(move |t| (x, t)))
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
        self.pattern[self.pattern_id].transitions.iter()
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
pub struct Output {
    pub key: StateKey,
    pub flags: StateFlags,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Retention {
    Partial,
    Recurse,
    Full,
}

#[derive(Debug)]
pub struct Matcher<'a> {
    machine: &'a StateMachine,
    trie: &'a Trie<TrieEntry>,
    states: StateSet,
    old_states: StateSet, // Reuse memory
    queue: Vec<(StateKey, StateFlags)>, // Reuse memory
    // States that matched a path component but aren't full matches
    recurse: Vec<Output>,
    // Full matches
    full: Vec<Output>,
}

impl<'a> Matcher<'a> {
    fn new_inner(
        machine: &'a StateMachine,
        trie: &'a Trie<TrieEntry>,
        states: StateSet,
    ) -> Self {
        Self {
            machine,
            trie,
            states,
            old_states: StateSet::with_capacity_and_hasher(
                machine.states.len(),
                FnvBuildHasher::new(),
            ),
            queue: Vec::new(),
            recurse: Vec::new(),
            full: Vec::new(),
        }
    }

    pub fn new(machine: &'a StateMachine, trie: &'a Trie<TrieEntry>) -> Self {
        let mut states =
            StateSet::with_capacity_and_hasher(machine.states.len(), FnvBuildHasher::new());
        let initial = StateKey {
            pattern: machine.initial(),
            trie: trie.root(),
            next_component: false,
        };
        states.insert(initial, StateFlags::default());
        Self::new_inner(machine, trie, states)
    }

    fn merge_state(&mut self, state: StateKey, flags: StateFlags) -> Option<StateFlags> {
        merge_state(&mut self.states, state, flags)
    }

    /// Follows epsilon transitions (incl. NextComponent) until all states are
    /// at frontier.
    fn expand_epsilon(&mut self) {
        self.queue.extend(self.states.drain());
        while let Some((key, flags)) = self.queue.pop() {
            let state = ExpandedState::raise(&self.machine, &self.trie, key, flags);
            if merge_state(&mut self.states, key, flags).is_none() {
                continue;
            }

            for successor in state.expand_epsilon() {
                self.queue.push(successor.lower());
            }

            if state.is_terminal() {
                self.full.push(Output { key, flags });
            } else if state.is_epsilon_frontier() && state.flags.next_component {
                self.recurse.push(Output { key, flags });
            }
        }

        self.states.retain(|key, flags| {
            ExpandedState::raise(&self.machine, &self.trie, *key, *flags).is_epsilon_frontier()
        });
    }

    fn expand_matching(&mut self) {
        std::mem::swap(&mut self.old_states, &mut self.states);
        self.states.clear();
        for (&src_key, &src_flags) in self.old_states.iter() {
            let s = ExpandedState::raise(&self.machine, &self.trie, src_key, src_flags);
            for successor in s.expand() {
                let (key, flags) = successor.lower();
                let merged = merge_state(&mut self.states, key, flags).is_some();
                if merged && successor.is_terminal() {
                    self.full.push(Output { key, flags });
                }
            }
        }
    }

    // Debug code
    fn show_outputs(&self, outputs: impl IntoIterator<Item = Output>) -> String {
        let mut out = String::new();
        let _ = write!(out, "[");
        for (i, Output { key, flags }) in outputs.into_iter().enumerate() {
            let state = ExpandedState::raise(self.machine, self.trie, key, flags);
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
                if state.is_literal() { " lit" } else { "" },
            );
        }
        let _ = write!(out, "]");
        out
    }

    fn show_states(&self) -> String {
        let outputs = self.states.iter().map(|(&key, &flags)| Output { key, flags });
        self.show_outputs(outputs)
    }

    fn show_full(&self) -> String {
        self.show_outputs(self.full.iter().cloned())
    }

    fn show_recurse(&self) -> String {
        self.show_outputs(self.recurse.iter().cloned())
    }

    fn step(&mut self) {
        self.expand_epsilon();
        test_log!("ε {}", self.show_states());
        self.expand_matching();
        test_log!("o {}", self.show_states());
    }

    // Runs until full trie has been consumed, or no more live states remain.
    pub fn run(&mut self) {
        test_log!("{:?}", self.trie.nodes);
        while !self.states.is_empty() {
            self.step();
        }
    }

    pub fn recurse(&self) -> impl Iterator<Item = Output> + '_ {
        self.recurse.iter().copied()
    }

    pub fn full(&self) -> impl Iterator<Item = Output> + '_ {
        self.full.iter().copied()
    }
}

fn match_trie<'m, 't>(
    machine: &'m StateMachine,
    trie: &'t Trie<TrieEntry>,
) -> Vec<(usize, bool)> {
    let mut matcher = Matcher::new(machine, trie);
    matcher.run();
    matcher.full()
        .map(|m| (trie.get_value(m.key.trie).unwrap().index, m.flags.contains(StateFlags::IS_LITERAL)))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::nfa::from_pattern;
    use crate::pattern::parse_ast;

    use super::*;

    fn machine(s: &str) -> StateMachine {
        from_pattern(&parse_ast(s).unwrap())
    }

    fn matches(pattern: &str, target: &str) -> bool {
        let mut trie = Trie::new();
        trie.insert(target, TrieEntry { index: 0, is_dir: false });
        !match_trie(&machine(pattern), &trie).is_empty()
    }

    fn match_all<'t>(pattern: &str, targets: &[&'t str]) -> Vec<&'t str> {
        let mut trie = Trie::new();
        for (index, target) in targets.iter().enumerate() {
            trie.insert(target, TrieEntry { index, is_dir: false });
        }
        let matches = match_trie(&machine(pattern), &trie);
        matches.into_iter().map(|(i, _)| targets[i]).collect()
    }

    #[test]
    fn test_matches() {
        assert!(matches("", ""));
        assert!(matches("{}", ""));
        assert!(matches("{,a}", ""));
        assert!(matches("asdf", "asdf"));
        assert!(!matches("asdf", "fdsa"));
        assert!(!matches("asdf", ""));

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
}
