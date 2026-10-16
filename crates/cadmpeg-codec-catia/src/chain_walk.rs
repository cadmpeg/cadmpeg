// SPDX-License-Identifier: Apache-2.0
//! Memoised walks along single-successor chains, such as owner chains.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// What one chain node states about a walk that reaches it.
pub(crate) enum ChainStep<N> {
    /// The node answers the walk.
    Stop,
    /// The walk continues at this node.
    Next(N),
    /// The chain ends here without an answer.
    End,
}

/// First stop node on the chain from each start node.
///
/// A walk answers the first node whose step is [`ChainStep::Stop`]. A chain
/// that ends, or that returns to a node it already passed, answers `None`.
/// Every node a walk passes takes the walk's answer, so each node is stepped
/// once across all walks.
pub(crate) struct ChainStops<N, F> {
    step: F,
    answers: HashMap<N, Option<N>>,
}

impl<N: Copy + Eq + Hash, F: FnMut(N) -> ChainStep<N>> ChainStops<N, F> {
    pub(crate) fn new(step: F) -> Self {
        Self {
            step,
            answers: HashMap::new(),
        }
    }

    pub(crate) fn stop(&mut self, start: N) -> Option<N> {
        let mut path = Vec::new();
        let mut on_path = HashSet::new();
        let mut current = start;
        let answer = loop {
            if let Some(answer) = self.answers.get(&current) {
                break *answer;
            }
            if !on_path.insert(current) {
                break None;
            }
            path.push(current);
            match (self.step)(current) {
                ChainStep::Stop => break Some(current),
                ChainStep::Next(next) => current = next,
                ChainStep::End => break None,
            }
        };
        for node in path {
            self.answers.insert(node, answer);
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::{ChainStep, ChainStops};
    use std::cell::Cell;

    #[test]
    fn walks_answer_the_first_stop_and_share_it() {
        let steps = Cell::new(0usize);
        let mut stops = ChainStops::new(|node: u32| {
            steps.set(steps.get() + 1);
            match node {
                0 => ChainStep::Stop,
                9 => ChainStep::End,
                node => ChainStep::Next(node - 1),
            }
        });
        for start in (1..=8).rev() {
            assert_eq!(stops.stop(start), Some(0));
        }
        assert_eq!(steps.get(), 9);
        assert_eq!(stops.stop(9), None);
    }

    #[test]
    fn a_cycle_without_a_stop_answers_none() {
        let mut stops = ChainStops::new(|node: u32| match node {
            0 => ChainStep::Stop,
            3 => ChainStep::Next(1),
            node => ChainStep::Next(node + 1),
        });
        assert_eq!(stops.stop(1), None);
        assert_eq!(stops.stop(2), None);
        assert_eq!(stops.stop(0), Some(0));
    }
}
