use crate::nfa::{StateId, StateMachine, TransitionRule};
use crate::test_log;

#[derive(Debug, Default)]
struct LiveSet {
    states: Vec<u32>,
    generation: u32,
}

impl LiveSet {
    fn new(size: usize) -> Self {
        Self {
            states: vec![0; size],
            generation: 1,
        }
    }

    fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.states.fill(0);
            self.generation = 1;
        }
    }

    fn is_live(&self, state: StateId) -> bool {
        self.states[state as usize] == self.generation
    }

    fn mark_live(&mut self, state: StateId) {
        self.states[state as usize] = self.generation;
    }
}

#[derive(Debug)]
struct StringMatcher<'a> {
    machine: &'a StateMachine,
    input: &'a str,
    states: Vec<StateId>,
    new_states: Vec<StateId>, // Reuse memory
    live: LiveSet,
}

impl<'a> StringMatcher<'a> {
    fn new(machine: &'a StateMachine, input: &'a str) -> Self {
        Self {
            machine,
            input,
            states: vec![0],
            new_states: Vec::new(),
            live: LiveSet::new(machine.states.len()),
        }
    }

    // Follows epsilon transitions until all states are on the frontier.
    fn expand_epsilon(&mut self) {
        self.live.clear();
        let live = &mut self.live;
        let machine = self.machine;

        let mut push_state = |queue: &mut Vec<StateId>, s: StateId| {
            if !live.is_live(s) {
                live.mark_live(s);
                queue.push(s);
            }
        };

        let queue = &mut self.new_states;
        self.states.iter().for_each(|&s| push_state(queue, s));
        self.states.clear();
        while let Some(s) = queue.pop() {
            if machine.states[s as usize].is_epsilon_frontier {
                self.states.push(s);
            }
            for t in &machine.states[s as usize].transitions {
                if let TransitionRule::Epsilon(next) | TransitionRule::WildEpsilon(next) = t {
                    push_state(queue, *next);
                }
            }
        }
    }

    fn expand_matching(&mut self, c: char) {
        self.live.clear();
        for &s in &self.states {
            for t in &self.machine.states[s as usize].transitions {
                let next = t.next();
                if t.is_epsilon() || self.live.is_live(next) {
                    test_log!("{c},{s},{t:?}... skipped");
                    continue;
                }
                if t.matches(c) {
                    test_log!("{c},{s},{t:?}... matched");
                    self.new_states.push(next);
                    self.live.mark_live(next);
                } else {
                    test_log!("{c},{s},{t:?}... failed");
                }
            }
        }
        std::mem::swap(&mut self.states, &mut self.new_states);
        self.new_states.clear();
    }

    fn step(&mut self, c: char) {
        self.expand_epsilon();
        test_log!("ε {:?}", self.states);
        self.expand_matching(c);
        test_log!("{} {:?}", c, self.states);
    }

    // Runs until full input has been consumed, or no more live states remain.
    fn run(&mut self) -> bool {
        test_log!("{:?}", self.machine.states);
        for c in self.input.chars() {
            self.step(c);
            if self.states.is_empty() {
                return false;
            }
        }
        self.live.clear();
        self.expand_epsilon();
        self.states.contains(&self.machine.terminal())
    }
}

fn match_string(machine: &StateMachine, target: &str) -> bool {
    StringMatcher::new(machine, target).run()
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
