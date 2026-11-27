// SPDX-License-Identifier: Apache-2.0
//! The resolved state of each evaluation and each process state, visited in
//! the order of its chain of bases, with the records its delta changes.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::CodecError;

use super::super::records::DesignRecords;
use super::state::{Applied, Closure, State};
use super::walk::{Forest, Stored};
use super::{locate, topology_ids, Resolver};
use crate::document::Topology;
use crate::index::ModelIndex;
use crate::process::state::ProcessState;

/// The resolved state of an evaluation or a process state, with what its
/// delta changed.
pub(crate) struct StateView<'s, 'a> {
    /// The id of the evaluation or process state.
    pub(crate) holder: &'a str,
    forest: &'s Forest<'a>,
    at: usize,
    state: &'s State<'a>,
    applied: &'s Applied<'a>,
    holders: &'s HashMap<*const Topology, &'a str>,
}

impl<'a> StateView<'_, 'a> {
    /// The ids of the evaluations and process states that the chain of bases
    /// below `holder` passes, nearest first.
    pub(crate) fn bases(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.forest.bases(self.at).map(Stored::id)
    }

    /// The evaluation or process state whose delta holds the state's record
    /// `id`; `None` for a record of the current geometry.
    pub(crate) fn source(&self, id: &str) -> Option<&'a str> {
        let located = self.state.records.get(id)?;
        self.holders
            .get(&std::ptr::from_ref(located.topology))
            .copied()
    }

    /// Every record of the state, as a `Topology` of its own.
    pub(crate) fn whole(&self) -> Topology {
        self.state.topology()
    }

    /// The records of the state connected through references, in the state
    /// or from the base's records the delta replaces or removes, with a
    /// record the delta states or removes, as a `Topology` of their own in id
    /// order. Every other record of the state is the base's record, with the
    /// same references to it and from it. Where the references of a record
    /// cannot be read for another reason than a resource limit, every record
    /// of the state.
    pub(crate) fn region(&self, ctx: &DecodeContext<'_>) -> Result<Topology, ResourceLimit> {
        match self.state.region_topology(ctx, self.applied) {
            Ok(region) => Ok(region),
            Err(CodecError::ResourceLimit(limit)) => Err(limit),
            Err(_) => Ok(self.whole()),
        }
    }
}

/// Visits the resolved state of each evaluation, then of each process state,
/// with content and whose chain of bases resolves, with the same resolution
/// the checks of resolved states use. The first record of each id is
/// visited, each after the records of its chain of bases. `scope` gives the
/// design scope of a process state's setup, which a `Current` base reads.
/// Only a resource limit stops the visits with an error. A chain whose
/// bottom does not resolve is not visited; where a delta cannot be applied
/// for another reason, the visits of its walk end. The checks of resolved
/// states report both.
pub(in crate::validate) fn visit_states<'a>(
    ctx: &DecodeContext<'_>,
    index: &DesignRecords<'a>,
    ids: &ModelIndex<'_>,
    scope: impl Fn(&ProcessState) -> Option<&'a str>,
    visit: &mut dyn FnMut(StateView<'_, 'a>) -> Result<(), ResourceLimit>,
) -> Result<(), ResourceLimit> {
    let ir = index.ir;
    let evaluations = &ir.design.evaluations;
    let states = &ir.process.states;
    let records = || {
        evaluations
            .iter()
            .map(Stored::Evaluation)
            .chain(states.iter().map(Stored::Process))
    };
    let holders: HashMap<*const Topology, &'a str> = records()
        .filter_map(|record| Some((std::ptr::from_ref(&record.delta()?.records), record.id())))
        .collect();
    if holders.is_empty() {
        return Ok(());
    }
    let resolver = Resolver {
        ctx,
        index,
        geometry: locate(&ir.geometry),
        children: ir.geometry.child_lists(),
    };
    let topology = topology_ids(&resolver.geometry, records().filter_map(Stored::delta));
    let closure = Closure {
        ctx,
        topology: &topology,
        ids,
    };
    let mut firsts: HashSet<usize> = HashSet::new();
    for (offset, ids) in [
        (
            0,
            evaluations
                .iter()
                .map(|evaluation| evaluation.id.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            evaluations.len(),
            states.iter().map(|state| state.id.as_str()).collect(),
        ),
    ] {
        let mut seen = HashSet::new();
        firsts.extend(
            ids.into_iter()
                .enumerate()
                .filter(|&(_, id)| seen.insert(id))
                .map(|(at, _)| offset + at),
        );
    }
    for (forest, process) in [
        (Forest::of_evaluations(evaluations), false),
        (Forest::of_process_states(evaluations, states, &scope), true),
    ] {
        let walked = forest.walk(
            ctx,
            |bottom| match resolver.bottom(&closure, bottom) {
                Err(CodecError::ResourceLimit(limit)) => Err(limit.into()),
                Err(_) => Ok(None),
                state => state,
            },
            |at, record, state| {
                let Some(delta) = record.delta() else {
                    return Ok(());
                };
                let applied = state.apply(&closure, delta, None)?;
                let holder = record.id();
                if process != matches!(record, Stored::Process(_)) || !firsts.contains(&at) {
                    return Ok(());
                }
                visit(StateView {
                    holder,
                    forest: &forest,
                    at,
                    state,
                    applied: &applied,
                    holders: &holders,
                })
                .map_err(CodecError::from)
            },
        );
        if let Err(CodecError::ResourceLimit(limit)) = walked {
            return Err(limit);
        }
    }
    Ok(())
}
