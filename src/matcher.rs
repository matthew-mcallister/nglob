use std::collections::hash_map::Entry;
use std::fmt::Write;
use std::path::is_separator;

use fnv::{FnvBuildHasher, FnvHashMap};

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

// Extra state data we track outside the state graph. All of this machinery can
// be formalized in terms of pure state graphs, but it is more practical to
// implement using external tracking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StateInfo {
    // true when the previously matched character was a separator. This allows
    // additional redundant separators in the pattern to be skipped.
    // TODO: Move this to the trie?
    have_sep: bool,
    // true when only literals were matched and never ?/*/**
    is_literal: bool,
}

impl Default for StateInfo {
    fn default() -> Self {
        Self {
            have_sep: false,
            is_literal: true,
        }
    }
}

impl StateInfo {
    fn update(self, c: Option<char>, transition: &TransitionRule) -> Self {
        Self {
            have_sep: c.map(is_separator).unwrap_or(self.have_sep),
            is_literal: self.is_literal && transition.is_literal(),
        }
    }
}

type StateSet = FnvHashMap<MatcherState, StateInfo>;

// Merges state data when two automata arrive at the same state
fn merge_state(
    states: &mut StateSet,
    state: MatcherState,
    info: StateInfo,
) -> Option<StateInfo> {
    match states.entry(state) {
        Entry::Occupied(mut entry) => {
            let merged = StateInfo {
                have_sep: entry.get().have_sep || info.have_sep,
                is_literal: entry.get().is_literal || info.is_literal,
            };
            if merged == *entry.get() {
                return None;
            }
            entry.insert(merged);
            Some(merged)
        }
        Entry::Vacant(entry) => {
            entry.insert(info);
            Some(info)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Accepted {
    pub state: MatcherState,
    pub info: StateInfo,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MatchConfig {
    /// Causes the matcher to accept ancestors of a matching path. For example,
    /// if `accept_ancestors` is true, then `foo/bar/baz` will match `foo`,
    /// `foo/bar`, and `foo/bar/baz`.
    pub accept_ancestors: bool,
}

#[derive(Debug)]
struct Matcher<'m, 't, T> {
    machine: &'m StateMachine,
    trie: &'t Trie<T>,
    config: MatchConfig,
    states: StateSet,
    new_states: StateSet, // Reuse memory
    queue: Vec<(MatcherState, StateInfo)>, // Reuse memory
    accepted: Vec<Accepted>,
}

impl<'m, 't, T> Matcher<'m, 't, T> {
    fn new(machine: &'m StateMachine, trie: &'t Trie<T>, config: MatchConfig) -> Self {
        let mut states =
            StateSet::with_capacity_and_hasher(machine.states.len(), FnvBuildHasher::new());
        let initial = MatcherState { nfa: machine.initial(), trie: trie.root() };
        states.insert(initial, StateInfo::default());
        Self {
            machine,
            trie,
            config,
            states,
            new_states: StateSet::with_capacity_and_hasher(
                machine.states.len(),
                FnvBuildHasher::new(),
            ),
            queue: Vec::new(),
            accepted: Vec::new(),
        }
    }

    // Follows epsilon transitions in the NFA until all states are on the
    // frontier.
    fn expand_epsilon(&mut self) {
        let machine = self.machine;
        let states = &mut self.states;
        let queue = &mut self.queue;
        queue.extend(states.drain());

        let mut push_state = |
            queue: &mut Vec<(MatcherState, StateInfo)>,
            s: MatcherState,
            info: StateInfo
        | {
            if let Some(merged) = merge_state(states, s, info) {
                queue.push((s, merged));
            }
        };

        while let Some((s, info)) = queue.pop() {
            if machine[s.nfa].is_epsilon_frontier {
                push_state(queue, s, info);
            }
            for t in machine.transitions(s.nfa) {
                match t {
                    TransitionRule::Epsilon(next) | TransitionRule::WildEpsilon(next) => {
                        push_state(queue, s.with_nfa(*next), info.update(None, t));
                    }
                    // Separator in target can skip multiple separators in pattern
                    TransitionRule::Sep(next) if info.have_sep => {
                        push_state(queue, s.with_nfa(*next), info.update(None, t));
                    }
                    _ => {}
                }
            }
        }
        // Only frontier states can consume input; the rest are dead ends
        self.states.retain(|&s, _| machine[s.nfa].is_epsilon_frontier);

        self.record_accepted();
    }

    // Records states which are at the terminal NFA node (full pattern match)
    // and also at a full node in the trie (full string was inserted).
    //
    // These states are still live but the terminal state has no transitions so
    // they will be cleared out right away.
    fn record_accepted(&mut self) {
        let terminal = self.machine.terminal();
        for (&state, &info) in self.states.iter() {
            let accept_ancestor = self.config.accept_ancestors
                && self.machine[state.nfa].accepts_sep;
            if self.trie.get_value(state.trie).is_some()
                && (state.nfa == terminal || accept_ancestor)
            {
                self.accepted.push(Accepted { state, info });
            }
        }
    }

    // Follows all possible transitions which consume a character in the trie.
    fn expand_matching(&mut self) {
        let new_states = &mut self.new_states;
        for (&s, info) in self.states.iter() {
            for (c, next_trie) in self.trie.children(s.trie) {
                for t in self.machine.transitions(s.nfa) {
                    let next = MatcherState { nfa: t.next(), trie: next_trie };
                    if t.is_epsilon() {
                        test_log!("{c},{next},{t:?}... skipped");
                    } else if t.matches(c) {
                        test_log!("{c},{next},{t:?}... matched");
                        merge_state(new_states, next, info.update(Some(c), t));
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
        for (i, (&s, &info)) in self.states.iter().enumerate() {
            if i > 0 {
                let _ = write!(out, ", ");
            }
            let _ = write!(
                out,
                r#"(t{} "{}", s{} "{}"{}{})"#,
                s.trie,
                self.trie.nodes[s.trie as usize].show_accepts(),
                s.nfa,
                self.machine[s.nfa].show_accepts(),
                if info.have_sep { " sep" } else { "" },
                if info.is_literal { "" } else { " wild" },
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
    }

    fn accepted<'a>(&'a self) -> impl Iterator<Item = Accepted> + 'a {
        self.accepted.iter().copied()
    }
}

fn match_string(machine: &StateMachine, target: &str) -> bool {
    let mut trie = Trie::new();
    trie.insert(target, ());
    let mut matcher = Matcher::new(machine, &trie, MatchConfig::default());
    matcher.run();
    !matcher.accepted.is_empty()
}

fn match_trie<'m, 't, T>(
    machine: &'m StateMachine,
    trie: &'t Trie<T>,
    config: MatchConfig,
) -> Vec<(&'t T, bool)> {
    let mut matcher = Matcher::new(machine, trie, config);
    matcher.run();
    matcher
        .accepted()
        .map(|a| (trie.get_value(a.state.trie).unwrap(), a.info.is_literal))
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
        match_string(&machine(pattern), target)
    }

    fn match_all_is_literal<'t>(
        pattern: &str,
        targets: &[&'t str],
    ) -> Vec<(&'t str, bool)> {
        let trie: Trie<&'t str> = targets.iter().map(|&k| (k, k)).collect();
        let mut matches: Vec<_> = match_trie(&machine(pattern), &trie, Default::default())
            .into_iter()
            .map(|(target, is_literal)| (*target, is_literal))
            .collect();
        matches.sort_unstable();
        matches
    }

    fn match_all_ancestors<'t>(
        pattern: &str,
        targets: &[&'t str],
    ) -> Vec<&'t str> {
        let config = MatchConfig { accept_ancestors: true, ..Default::default() };
        let trie: Trie<&'t str> = targets.iter().map(|&k| (k, k)).collect();
        let mut matches: Vec<_> = match_trie(&machine(pattern), &trie, config)
            .into_iter()
            .map(|(target, _)| *target)
            .collect();
        matches.sort_unstable();
        matches
    }

    fn match_all<'t>(pattern: &str, targets: &[&'t str]) -> Vec<&'t str> {
        match_all_is_literal(pattern, targets)
            .into_iter()
            .map(|(target, _)| target)
            .collect()
    }

    fn is_literal_match(pattern: &str, target: &str) -> bool {
        match_all_is_literal(pattern, &[target]).first().unwrap().1
    }

    #[test]
    fn test_ancestors() {
        assert_eq!(
            match_all_ancestors("foo/bar/baz", &["foo", "foo/bar", "foo/bar/baz", "fo"]),
            ["foo", "foo/bar", "foo/bar/baz"]
        );
        assert_eq!(
            match_all_ancestors("foo/bar", &["foo", "fo"]),
            ["foo"]
        );
        assert_eq!(
            match_all_ancestors("foo", &["foo", "fo"]),
            ["foo"]
        );
        assert_eq!(
            match_all_ancestors("*.rs", &["main.rs", "src", "src/main.rs"]),
            ["main.rs"]
        );
        assert_eq!(
            match_all_ancestors("src/*.rs", &["src", "src/main.rs"]),
            ["src", "src/main.rs"]
        );
    }

    #[test]
    fn test_match_many() {
        assert_eq!(
            match_all("*.rs", &["main.rs", "lib.rs", "src", "a/b.rs"]),
            ["lib.rs", "main.rs"]
        );
        assert_eq!(
            match_all("{a,*.rs}", &["a", "b", "c.rs", "main.rs"]),
            ["a", "c.rs", "main.rs"]
        );
        assert_eq!(
            match_all("**/b", &["b", "x/b", "x/y/b", "a/b/c"]),
            ["x/b", "x/y/b"]
        );
        assert_eq!(
            match_all("a*b", &["ab", "axxb", "a/b"]),
            ["ab", "axxb"]
        );
        assert_eq!(
            match_all(r"\*", &["*", "**"]),
            ["*"]
        );
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

    #[test]
    fn test_match_literal() {
        assert!(is_literal_match("", ""));
        assert!(is_literal_match("abc", "abc"));
        assert!(is_literal_match(r"\*", "*"));
        assert!(is_literal_match(r"a\?c", "a?c"));

        assert!(is_literal_match("a/b", "a/b"));
        assert!(is_literal_match("a//b", "a/b"));
        assert!(is_literal_match("a/b", "a//b"));

        assert!(!is_literal_match("?", "a"));
        assert!(!is_literal_match("a?c", "abc"));
        assert!(is_literal_match("{a,?}", "a"));
        assert!(!is_literal_match("{a,?}", "b"));

        assert!(!is_literal_match("*", "."));
        assert!(!is_literal_match("**", "."));
        assert!(!is_literal_match(".*", "."));
        assert!(!is_literal_match("*", ".."));
        assert!(!is_literal_match("**", ".."));
        assert!(!is_literal_match(".*", ".."));

        assert!(!is_literal_match("*", ""));
        assert!(!is_literal_match("*", "abc"));
        assert!(!is_literal_match("a*", "a"));
        assert!(!is_literal_match("a*", "ab"));
        assert!(!is_literal_match(".*", "."));
        assert!(!is_literal_match("*/", "a/"));
        assert!(!is_literal_match("**", ""));
        assert!(!is_literal_match("**", "a/b"));
        assert!(!is_literal_match("a**b", "ab"));
        assert!(!is_literal_match("**/b", "x/b"));
        assert!(!is_literal_match("a/**/b", "a/b"));

        assert!(is_literal_match("a{*,b}c", "abc"));
        assert!(!is_literal_match("a{*,b}c", "ac"));
        assert!(!is_literal_match("a{*,b}c", "axc"));

        assert_eq!(
            match_all_is_literal("{a,*.rs}", &["a", "b", "c.rs", "main.rs"]),
            [("a", true), ("c.rs", false), ("main.rs", false)]
        );
        assert_eq!(
            match_all_is_literal("a{b,?}c", &["abc", "axc", "abd"]),
            [("abc", true), ("axc", false)]
        );
        assert_eq!(
            match_all_is_literal("**/b", &["b", "x/b", "x/y/b", "a/b/c"]),
            [("x/b", false), ("x/y/b", false)]
        );
        assert_eq!(
            match_all_is_literal("a/b", &["a/b", "a//b", "ab"]),
            [("a//b", true), ("a/b", true)]
        );
    }

    #[test]
    fn test_star_choice() {
        assert!(!matches("a{*,/}c", "ab/c"));
    }

    #[test]
    fn test_multiple_separators_in_target() {
        assert!(matches("a/b", "a//b"));
        assert!(matches("a/b", "a///b"));
        assert!(!matches("a/b", "a/b/c"));
    }

    #[test]
    fn test_multiple_separators_in_pattern() {
        assert!(matches("a//b", "a/b"));
        assert!(matches("a///b", "a/b"));
        assert!(!matches("a//b", "ab"));
        assert!(matches("a/**/b", "a/b"));
        assert!(matches("a{*/,}/b", "asdf/b"));
        assert_eq!(
            match_all("src/**/*.rs", &["src/a/b.rs", "src/main.rs", "src.rs", "src/b.rs"]),
            ["src/a/b.rs", "src/b.rs", "src/main.rs"]
        );
        assert_eq!(
            match_all("a/**/b", &["a/b", "a//b", "a/x/b", "a/b/c"]),
            ["a//b", "a/b", "a/x/b"]
        );
    }
}
