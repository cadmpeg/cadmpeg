// SPDX-License-Identifier: Apache-2.0
//! The resolved state of each evaluation and each process state, visited in
//! the order of its chain of bases, with the records its delta changes.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;

use super::super::records::DesignRecords;
use super::{
    apply, below_process_state, design_base, locate, records, Arena, Chain, Located, ResolvedState,
    Resolver, Stored,
};
use crate::design::evaluation::StateDelta;
use crate::document::Topology;
use crate::index::identities::FastBuildHasher;
use crate::process::state::ProcessState;

/// The key of an id in the index of referrers.
fn key(id: &str) -> u64 {
    FastBuildHasher::default().hash_one(id)
}

/// Each record of the current geometry and of each delta, under the key of
/// each id it references.
struct Referrers<'a>(HashMap<u64, Vec<(&'a str, Located<'a>)>, FastBuildHasher>);

impl<'a> Referrers<'a> {
    fn build(
        ctx: &DecodeContext<'_>,
        topologies: impl IntoIterator<Item = &'a Topology>,
    ) -> Result<Self, CodecError> {
        let mut referrers: HashMap<u64, Vec<(&'a str, Located<'a>)>, FastBuildHasher> =
            HashMap::default();
        for topology in topologies {
            for (arena, id, index) in records(topology) {
                let located = Located {
                    arena,
                    topology,
                    index,
                    holder: None,
                };
                located.visit_references(ctx, &mut |target| {
                    referrers
                        .entry(key(target))
                        .or_default()
                        .push((id, located));
                    Ok(())
                })?;
            }
        }
        Ok(Self(referrers))
    }

    /// The records of `state` that reference the id of `key`.
    fn of<'r>(
        &'r self,
        key: u64,
        state: &'r ResolvedState<'a>,
    ) -> impl Iterator<Item = &'a str> + 'r {
        self.0
            .get(&key)
            .into_iter()
            .flatten()
            .filter(|(id, located)| state.records.get(id).is_some_and(|held| held.is(*located)))
            .map(|(id, _)| *id)
    }
}

impl Located<'_> {
    /// Whether `self` and `other` are the same record of the same `Topology`.
    fn is(self, other: Self) -> bool {
        self.arena == other.arena
            && std::ptr::eq(self.topology, other.topology)
            && self.index == other.index
    }
}

/// A node of the reference graph of a state: a record, or an id that no
/// record of the state holds.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node<'a> {
    Record(&'a str),
    Missing(u64),
}

/// The resolved state of an evaluation or a process state, with the state
/// of its base.
pub(crate) struct StateView<'s, 'a> {
    /// The id of the evaluation or process state.
    pub(crate) holder: &'a str,
    /// The ids of the evaluations and process states that the chain of bases
    /// below `holder` passes, nearest first.
    pub(crate) bases: Vec<&'a str>,
    state: &'s ResolvedState<'a>,
    base: &'s ResolvedState<'a>,
    delta: &'a StateDelta,
    referrers: Option<&'s Referrers<'a>>,
}

impl<'a> StateView<'_, 'a> {
    /// The evaluation or process state whose delta holds the state's record
    /// `id`; `None` for a record of the current geometry.
    pub(crate) fn source(&self, id: &str) -> Option<&'a str> {
        self.state.records.get(id)?.holder
    }

    /// Every record of the state, as a `Topology` of its own.
    pub(crate) fn whole(&self) -> Topology {
        self.state.topology()
    }

    /// The records of the state connected through references, in the state
    /// or from the base's records the delta replaces or removes, with a
    /// record the delta states or removes or with an id of `also`, as a
    /// `Topology` of their own in id order. Every other record of the state
    /// is the base's record, with the same references to it and from it.
    /// Where the references of a record cannot be read for another reason
    /// than a resource limit, every record of the state.
    pub(crate) fn region<'i>(
        &self,
        ctx: &DecodeContext<'_>,
        also: impl IntoIterator<Item = &'i str>,
    ) -> Result<Topology, ResourceLimit> {
        let Some(referrers) = self.referrers else {
            return Ok(self.whole());
        };
        let region = match self.component(ctx, referrers, also) {
            Ok(region) => region,
            Err(CodecError::ResourceLimit(limit)) => return Err(limit),
            Err(_) => return Ok(self.whole()),
        };
        let mut topology = Topology::default();
        for id in region {
            if let Some(located) = self.state.records.get(id) {
                located.copy_into(&mut topology);
            }
        }
        topology.finalize();
        Ok(topology)
    }

    fn component<'i>(
        &self,
        ctx: &DecodeContext<'_>,
        referrers: &Referrers<'a>,
        also: impl IntoIterator<Item = &'i str>,
    ) -> Result<BTreeSet<&'a str>, CodecError> {
        let node = |id: &str| match self.state.records.get_key_value(id) {
            Some((&id, _)) => Node::Record(id),
            None => Node::Missing(key(id)),
        };
        let mut pending: Vec<Node<'a>> = also.into_iter().map(node).collect();
        let stated = records(&self.delta.records)
            .into_iter()
            .map(|(_, id, _)| id);
        let removed = self.delta.removed.iter().filter_map(|removed| {
            let (arena, id) = Arena::of_record(removed);
            self.base.holds(arena, id).then_some(id)
        });
        for id in stated.chain(removed) {
            pending.push(node(id));
            if let Some(located) = self.base.records.get(id) {
                located.visit_references(ctx, &mut |target| {
                    pending.push(node(target));
                    Ok(())
                })?;
            }
        }
        let mut seen: HashSet<Node<'a>, FastBuildHasher> = HashSet::default();
        while let Some(at) = pending.pop() {
            if !seen.insert(at) {
                continue;
            }
            let at_key = match at {
                Node::Record(id) => {
                    if let Some(located) = self.state.records.get(id) {
                        located.visit_references(ctx, &mut |target| {
                            pending.push(node(target));
                            Ok(())
                        })?;
                    }
                    key(id)
                }
                Node::Missing(at_key) => at_key,
            };
            pending.extend(referrers.of(at_key, self.state).map(Node::Record));
        }
        Ok(seen
            .into_iter()
            .filter_map(|node| match node {
                Node::Record(id) => Some(id),
                Node::Missing(_) => None,
            })
            .collect())
    }
}

/// `Ok(None)` for an error other than a resource limit, which stops nothing
/// but the work it ends.
fn ceiling<T>(result: Result<T, CodecError>) -> Result<Option<T>, ResourceLimit> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(CodecError::ResourceLimit(limit)) => Err(limit),
        Err(_) => Ok(None),
    }
}

/// Visits the resolved state of each evaluation, then of each process state,
/// with content and whose chain of bases resolves, with the same resolution
/// the checks of resolved states use. The first record of each id is
/// visited, each after the records of its chain of bases. `scope` gives the
/// design scope of a process state's setup, which a `Current` base reads.
/// Only a resource limit stops the visits; a state whose base does not
/// resolve for another reason is not visited, and the checks of resolved
/// states report it.
pub(in crate::validate) fn visit_states<'a>(
    ctx: &DecodeContext<'_>,
    index: &DesignRecords<'a>,
    scope: impl Fn(&ProcessState) -> Option<&'a str>,
    visit: &mut dyn FnMut(StateView<'_, 'a>) -> Result<(), ResourceLimit>,
) -> Result<(), ResourceLimit> {
    let ir = index.ir;
    let evaluations = &ir.design.evaluations;
    let states = &ir.process.states;
    if evaluations
        .iter()
        .all(|evaluation| evaluation.content.is_none())
        && states.iter().all(|state| state.content.is_none())
    {
        return Ok(());
    }
    let deltas = evaluations
        .iter()
        .filter_map(Stored::delta)
        .chain(states.iter().filter_map(Stored::delta))
        .map(|delta| &delta.records);
    let referrers = ceiling(Referrers::build(
        ctx,
        std::iter::once(&ir.geometry).chain(deltas),
    ))?;
    let resolver = Resolver {
        ctx,
        index,
        geometry: locate(&ir.geometry),
        children: ir.geometry.child_lists(),
    };
    let mut chain = Chain::new(evaluations, evaluations.iter().filter_map(Stored::base));
    for evaluation in chain.bases_first(evaluations) {
        let base = ceiling(chain.base_state(evaluation, |below| resolver.below_evaluation(below)))?;
        let (Some(Some(base)), Some(delta)) = (base, evaluation.delta()) else {
            continue;
        };
        let holder = evaluation.id.as_str();
        let mut state = (*base).clone();
        apply(&mut state, delta, holder, None);
        visit(StateView {
            holder,
            bases: chain.below(evaluation).0,
            state: &state,
            base: &base,
            delta,
            referrers: referrers.as_ref(),
        })?;
        chain.keep(evaluation, state);
    }
    let mut evaluations = Chain::new(evaluations, states.iter().filter_map(design_base));
    let mut chain = Chain::new(states, states.iter().filter_map(Stored::base));
    for state in chain.bases_first(states) {
        let base = ceiling(chain.base_state(state, |below| {
            below_process_state(&resolver, &mut evaluations, &scope, below)
        }))?;
        if let Some(evaluation) = design_base(state) {
            evaluations.release(evaluation);
        }
        let (Some(Some(base)), Some(delta)) = (base, state.delta()) else {
            continue;
        };
        let holder = state.id.as_str();
        let mut resolved = (*base).clone();
        apply(&mut resolved, delta, holder, None);
        let (mut bases, last) = chain.below(state);
        if let Some(&evaluation) = design_base(last).and_then(|id| evaluations.by_id.get(id)) {
            bases.push(evaluation.id.as_str());
            bases.extend(evaluations.below(evaluation).0);
        }
        visit(StateView {
            holder,
            bases,
            state: &resolved,
            base: &base,
            delta,
            referrers: referrers.as_ref(),
        })?;
        chain.keep(state, resolved);
    }
    Ok(())
}
