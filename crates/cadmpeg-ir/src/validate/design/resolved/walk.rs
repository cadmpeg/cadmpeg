// SPDX-License-Identifier: Apache-2.0
//! The tree of bases of a document's stored records. Each record with content
//! rests on the first record of its base's id, or on the bottom its chain
//! ends on: nothing, or the extent of a current state. A walk builds each
//! bottom's state once and visits every record below it in place, so each
//! record's state is its base's state with its delta applied.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::super::records::PositionKey;
use super::state::State;
use crate::design::evaluation::{Evaluation, StateBase, StateDelta};
use crate::process::state::{ProcessBase, ProcessState};

/// A record whose content is a delta on a base.
#[derive(Clone, Copy)]
pub(super) enum Stored<'a> {
    Evaluation(&'a Evaluation),
    Process(&'a ProcessState),
}

impl<'a> Stored<'a> {
    pub(super) fn delta(self) -> Option<&'a StateDelta> {
        match self {
            Self::Evaluation(evaluation) => {
                evaluation.content.as_ref().map(|content| &content.delta)
            }
            Self::Process(state) => state.content.as_ref().map(|content| &content.delta),
        }
    }
}

/// The state a chain of bases ends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Bottom<'a> {
    /// Nothing.
    Empty,
    /// The extent of a scope's current state at a position.
    Current(&'a str, PositionKey<'a>),
}

/// What a record rests on.
#[derive(Clone, Copy)]
enum Parent<'a> {
    Bottom(Bottom<'a>),
    Record(usize),
}

/// The first record of each id of one arena, and whether the chain of bases
/// from a record reaches an id.
struct Firsts<'a> {
    by_id: BTreeMap<&'a str, usize>,
    bases: Vec<Option<&'a str>>,
}

impl<'a> Firsts<'a> {
    fn new(ids: impl Iterator<Item = (&'a str, Option<&'a str>)>) -> Self {
        let mut by_id = BTreeMap::new();
        let mut bases = Vec::new();
        for (at, (id, base)) in ids.enumerate() {
            by_id.entry(id).or_insert(at);
            bases.push(base);
        }
        Self { by_id, bases }
    }

    /// The first record of `id` where it has content.
    fn first(&self, id: &str, content: impl Fn(usize) -> bool) -> Option<usize> {
        self.by_id.get(id).copied().filter(|&at| content(at))
    }

    /// Whether the chain of bases from the record `from` reaches `id`.
    fn reaches(&self, from: &str, id: &str) -> bool {
        let mut at = from;
        for _ in 0..=self.by_id.len() {
            if at == id {
                return true;
            }
            let Some(base) = self
                .by_id
                .get(at)
                .and_then(|&record| self.bases.get(record).copied().flatten())
            else {
                return false;
            };
            at = base;
        }
        false
    }

    /// The parent of the record `at`, whose id is `id`, where its base names
    /// the record `base` of this arena: none for a record after the first of
    /// its id whose chain of bases reaches its own id.
    fn parent(
        &self,
        at: usize,
        id: &str,
        base: &str,
        content: impl Fn(usize) -> bool,
    ) -> Option<usize> {
        let first = self.by_id.get(id) == Some(&at);
        if !first && self.reaches(base, id) {
            return None;
        }
        self.first(base, content)
    }
}

/// The records of a walk and what each rests on. Records `0..evaluations`
/// are the evaluations, the rest the process states.
pub(super) struct Forest<'a> {
    records: Vec<Stored<'a>>,
    checked: Vec<bool>,
    parents: Vec<Option<Parent<'a>>>,
}

impl<'a> Forest<'a> {
    /// The evaluations, each checked.
    pub(super) fn of_evaluations(evaluations: &'a [Evaluation]) -> Self {
        let parents = evaluation_parents(evaluations);
        Self {
            records: evaluations.iter().map(Stored::Evaluation).collect(),
            checked: evaluations
                .iter()
                .map(|evaluation| evaluation.content.is_some())
                .collect(),
            parents,
        }
    }

    /// The process states, each checked, on the evaluations. `scope` gives
    /// the design scope of a state's setup, which a `Current` base reads.
    pub(super) fn of_process_states(
        evaluations: &'a [Evaluation],
        states: &'a [ProcessState],
        scope: impl Fn(&ProcessState) -> Option<&'a str>,
    ) -> Self {
        let mut parents = evaluation_parents(evaluations);
        let offset = evaluations.len();
        let firsts = Firsts::new(states.iter().map(|state| {
            let base = match &state.content.as_ref().map(|content| &content.base) {
                Some(ProcessBase::State(base)) => Some(base.as_str()),
                _ => None,
            };
            (state.id.as_str(), base)
        }));
        let evaluation_firsts = Firsts::new(
            evaluations
                .iter()
                .map(|evaluation| (evaluation.id.as_str(), None)),
        );
        for (at, state) in states.iter().enumerate() {
            let parent = state
                .content
                .as_ref()
                .and_then(|content| match &content.base {
                    ProcessBase::State(base) => firsts
                        .parent(at, state.id.as_str(), base.as_str(), |base| {
                            states[base].content.is_some()
                        })
                        .map(|base| Parent::Record(offset + base)),
                    ProcessBase::Design(StateBase::Empty) => Some(Parent::Bottom(Bottom::Empty)),
                    ProcessBase::Design(StateBase::Current(position)) => {
                        scope(state).map(|scope| {
                            Parent::Bottom(Bottom::Current(scope, PositionKey::of(position)))
                        })
                    }
                    ProcessBase::Design(StateBase::Evaluation(base)) => evaluation_firsts
                        .first(base.as_str(), |base| evaluations[base].content.is_some())
                        .map(Parent::Record),
                });
            parents.push(parent);
        }
        let mut checked = vec![false; evaluations.len()];
        checked.extend(states.iter().map(|state| state.content.is_some()));
        Self {
            records: evaluations
                .iter()
                .map(Stored::Evaluation)
                .chain(states.iter().map(Stored::Process))
                .collect(),
            checked,
            parents,
        }
    }

    fn is_checked(&self, at: usize) -> bool {
        matches!(self.checked.as_slice().get(at), Some(true))
    }

    /// Visits every checked record whose chain of bases resolves, and every
    /// record on such a chain, each after its base. `bottom` gives the state
    /// a chain ends on, `None` where it does not resolve. `visit` applies the
    /// record's delta to the state and checks it; the walk undoes the delta
    /// once every record resting on it is visited.
    pub(super) fn walk(
        &self,
        ctx: &DecodeContext<'_>,
        mut bottom: impl FnMut(Bottom<'a>) -> Result<Option<State<'a>>, CodecError>,
        mut visit: impl FnMut(usize, Stored<'a>, &mut State<'a>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        enum Step {
            Enter(usize),
            Leave(usize),
        }
        let mut needed = self.checked.clone();
        for at in 0..self.records.len() {
            if !self.is_checked(at) {
                continue;
            }
            let mut parent = self.parents[at];
            for _ in 0..self.records.len() {
                let Some(Parent::Record(base)) = parent else {
                    break;
                };
                if needed[base] {
                    break;
                }
                needed[base] = true;
                parent = self.parents[base];
            }
        }
        let mut bottoms: BTreeMap<Bottom<'a>, Vec<usize>> = BTreeMap::new();
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); self.records.len()];
        for (at, parent) in self.parents.iter().enumerate() {
            if !needed[at] {
                continue;
            }
            match parent {
                Some(Parent::Bottom(below)) => bottoms.entry(*below).or_default().push(at),
                Some(Parent::Record(base)) => children[*base].push(at),
                None => {}
            }
        }
        for (below, roots) in bottoms {
            let Some(mut state) = bottom(below)? else {
                continue;
            };
            let mut steps: Vec<Step> = roots.into_iter().rev().map(Step::Enter).collect();
            while let Some(step) = steps.pop() {
                match step {
                    Step::Enter(at) => {
                        steps.push(Step::Leave(state.mark()));
                        visit(at, self.records[at], &mut state)?;
                        steps.extend(children[at].iter().rev().map(|&child| Step::Enter(child)));
                    }
                    Step::Leave(mark) => state.undo(ctx, mark)?,
                }
            }
        }
        Ok(())
    }
}

fn evaluation_parents(evaluations: &[Evaluation]) -> Vec<Option<Parent<'_>>> {
    let firsts = Firsts::new(evaluations.iter().map(|evaluation| {
        let base = match evaluation.content.as_ref().map(|content| &content.base) {
            Some(StateBase::Evaluation(base)) => Some(base.as_str()),
            _ => None,
        };
        (evaluation.id.as_str(), base)
    }));
    evaluations
        .iter()
        .enumerate()
        .map(
            |(at, evaluation)| match &evaluation.content.as_ref()?.base {
                StateBase::Empty => Some(Parent::Bottom(Bottom::Empty)),
                StateBase::Current(position) => Some(Parent::Bottom(Bottom::Current(
                    evaluation.scope.as_str(),
                    PositionKey::of(position),
                ))),
                StateBase::Evaluation(base) => firsts
                    .parent(at, evaluation.id.as_str(), base.as_str(), |base| {
                        evaluations[base].content.is_some()
                    })
                    .map(Parent::Record),
            },
        )
        .collect()
}
