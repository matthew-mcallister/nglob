use std::fmt::Write;

use fnv::FnvHashSet;

use crate::nfa::{StateId, StateMachine, TransitionRule};
use crate::test_log;
use crate::trie::{Trie, TrieId};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
struct MatcherState {
    nfa: StateId,
    trie: TrieId,
}

impl std::fmt::Display for MatcherState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "t{},s{}", self.trie, self.nfa)
    }
}

impl MatcherState {
    fn with_nfa(self, nfa: StateId) -> Self {
        Self { nfa, ..self }
    }
}

#[derive(Debug, Default)]
struct LiveSet {
    states: FnvHashSet<MatcherState>,
}

impl LiveSet {
    fn new(capacity: usize) -> Self {
        let bh = fnv::FnvBuildHasher::new();
        Self {
            states: FnvHashSet::with_capacity_and_hasher(capacity, bh),
        }
    }

    fn clear(&mut self) {
        self.states.clear();
    }

    fn is_live(&self, state: &MatcherState) -> bool {
        self.states.contains(&state)
    }

    fn mark_live(&mut self, state: MatcherState) {
        self.states.insert(state);
    }
}

#[derive(Debug)]
struct Matcher<'m, 't, T> {
    machine: &'m StateMachine,
    trie: &'t Trie<T>,
    states: Vec<MatcherState>,
    new_states: Vec<MatcherState>, // Reuse memory
    live: LiveSet,
    accepted: Vec<TrieId>,
}

impl<'m, 't, T> Matcher<'m, 't, T> {
    fn new(machine: &'m StateMachine, trie: &'t Trie<T>) -> Self {
        Self {
            machine,
            trie,
            states: vec![MatcherState { nfa: 0, trie: 0 }],
            new_states: Vec::new(),
            live: LiveSet::new(machine.states.len()),
            accepted: Vec::new(),
        }
    }

    // Follows epsilon transitions in the NFA until all states are on the
    // frontier.
    fn expand_epsilon(&mut self) {
        self.live.clear();
        let live = &mut self.live;
        let machine= self.machine;

        let mut push_state = |queue: &mut Vec<MatcherState>, s: MatcherState| {
            if !live.is_live(&s) {
                live.mark_live(s);
                queue.push(s);
            }
        };

        let queue = &mut self.new_states;
        self.states.iter().for_each(|&s| push_state(queue, s));
        self.states.clear();
        while let Some(s) = queue.pop() {
            if machine[s.nfa].is_epsilon_frontier {
                self.states.push(s);
            }
            for t in machine.transitions(s.nfa) {
                if let TransitionRule::Epsilon(next) | TransitionRule::WildEpsilon(next) = t {
                    push_state(queue, s.with_nfa(*next));
                }
            }
        }

        self.record_accepted();
    }

    // Records states which are at the terminal NFA node (full pattern match)
    // and also at a full node in the trie (full string was inserted).
    //
    // These states are still live but the terminal state has no transitions so
    // they will be cleared out right away.
    fn record_accepted(&mut self) {
        for state in self.states.iter() {
            if self.trie.get_value(state.trie).is_some()
                && state.nfa == self.machine.terminal()
            {
                self.accepted.push(state.trie);
            }
        }
    }

    // Follows all possible transitions which consume a character in the trie.
    fn expand_matching(&mut self) {
        self.live.clear();
        for s in &self.states {
            for (c, next_trie) in self.trie.children(s.trie) {
                for t in self.machine.transitions(s.nfa) {
                    let next = MatcherState { nfa: t.next(), trie: next_trie };
                    if t.is_epsilon() || self.live.is_live(&next) {
                        test_log!("{c},{next},{t:?}... skipped");
                    } else if t.matches(c) {
                        test_log!("{c},{next},{t:?}... matched");
                        self.new_states.push(next);
                        self.live.mark_live(next);
                    } else {
                        test_log!("{c},{next},{t:?}... failed");
                    }
                }
            }
        }
        std::mem::swap(&mut self.states, &mut self.new_states);
        self.new_states.clear();
    }

    // Debug code
    fn show_states(&self) -> String {
        let mut out = String::new();
        let _ = write!(out, "[");
        for (i, &s) in self.states.iter().enumerate() {
            if i > 0 {
                let _ = write!(out, ", ");
            }
            let _ = write!(
                out,
                r#"({} "{}", {} "{}")"#,
                s.trie,
                self.trie.nodes[s.trie as usize].show_accepts(),
                s.nfa,
                self.machine[s.nfa].show_accepts(),
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
    fn run(&mut self) {
        test_log!("{:?}", self.trie.nodes);
        test_log!("{:?}", self.machine.states);
        while !self.states.is_empty() {
            self.step();
        }
        self.live.clear();
        self.expand_epsilon();
    }

    fn accepted<'a>(&'a self) -> impl Iterator<Item = &'t T> + 'a {
        self.accepted.iter().map(|&id| self.trie.get_value(id).unwrap())
    }
}

fn match_string(machine: &StateMachine, target: &str) -> bool {
    let mut trie = Trie::new();
    trie.insert(target, ());
    let mut matcher = Matcher::new(machine, &trie);
    matcher.run();
    !matcher.accepted.is_empty()
}

fn match_trie<'m, 't, T>(
    machine: &'m StateMachine,
    trie: &'t Trie<T>,
) -> Vec<&'t T> {
    let mut matcher = Matcher::new(machine, trie);
    matcher.run();
    matcher.accepted().collect()
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
        match_string(&machine(pattern), target)
    }

    #[test]
    fn test_match() {
        assert!(matches("", ""));
        assert!(!matches("", "a"));
        assert!(matches("abc", "abc"));
        assert!(!matches("abc", "ab"));
        assert!(!matches("abc", "abcd"));
        assert!(matches("héllo", "héllo"));

        assert!(matches("a/b", "a/b"));
        assert!(!matches("a/b", "ab"));

        assert!(matches("?", "a"));
        assert!(matches("?", "é"));
        assert!(!matches("?", "/"));
        assert!(!matches("?", ""));
        assert!(!matches("?", "ab"));
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "a/c"));

        assert!(matches("*", ""));
        assert!(matches("*", "abc"));
        assert!(!matches("*", "/"));
        assert!(matches("a*b", "ab"));
        assert!(matches("a*b", "axxb"));
        assert!(!matches("a*b", "a/b"));

        assert!(matches("**", ""));
        assert!(matches("**", "a/b/c"));
        assert!(matches("a**b", "a/xb"));
        assert!(!matches("a**b", "a/b/c"));
        assert!(matches("a/**", "a/b/c"));
        assert!(!matches("a/**", "a"));
        assert!(matches("***", "a/b"));

        assert!(matches("{a,b}", "a"));
        assert!(matches("{a,b}", "b"));
        assert!(!matches("{a,b}", "c"));
        assert!(matches("{ab,cd}", "cd"));
        assert!(matches("{,a}", ""));
        assert!(matches("{a,{b,c}d}", "bd"));
        assert!(!matches("{a,{b,c}d}", "bcd"));
        assert!(matches("{a,*.rs}", "a"));
        assert!(matches("{a,*.rs}", "main.rs"));
        assert!(!matches("{a,*.rs}", "b"));

        assert!(matches(r"\*", "*"));
        assert!(!matches(r"\*", "**"));
        assert!(matches(r"\?", "?"));
        assert!(matches(r"\{a\}", "{a}"));
        assert!(matches(r"a\,b", "a,b"));
        assert!(matches(r"a\*b", "a*b"));

        assert!(matches(r"\\", "\\"));
        #[cfg(not(windows))]
        assert!(!matches(r"\\", "/"));
        #[cfg(windows)]
        assert!(matches(r"\\", "/"));

        assert!(matches("?*", "ab"));
        assert!(!matches("?*", ""));
        assert!(matches("*/*", "a/b"));
        assert!(!matches("*/*", "a/b/c"));
        assert!(matches("**/b", "x/y/b"));
        assert!(matches("{a,b}?c", "axc"));
        assert!(matches("./*", "./a"));
        assert!(matches("*/.", "a/."));
        assert!(matches("src/**/*.rs", "src/a/b.rs"));
        assert!(!matches("src/**/*.rs", "src.rs"));
    }
}
