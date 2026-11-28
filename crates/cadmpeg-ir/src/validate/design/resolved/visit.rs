// SPDX-License-Identifier: Apache-2.0
//! The resolved state of each evaluation, visited in
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
use crate::CadIr;

/// The resolved state of an evaluation, with what its
/// delta changed.
pub(crate) struct StateView<'s, 'a> {
    /// The id of the evaluation.
    pub(crate) holder: &'a str,
    forest: &'s Forest<'a>,
    at: usize,
    state: &'s State<'a>,
    applied: &'s Applied<'a>,
    holders: &'s HashMap<*const Topology, &'a str>,
}

impl<'a> StateView<'_, 'a> {
    /// The ids of the evaluations that the chain of bases
    /// below `holder` passes, nearest first.
    pub(crate) fn bases(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.forest.bases(self.at).map(Stored::id)
    }

    /// The evaluation whose delta holds the state's record
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

/// Visits the resolved state of each evaluation
/// with content and whose chain of bases resolves, with the same resolution
/// the checks of resolved states use. The first record of each id is
/// visited, each after the records of its chain of bases.
/// Only a resource limit stops the visits with an error. A chain whose
/// bottom does not resolve is not visited; where a delta cannot be applied
/// for another reason, the visits of its walk end. The checks of resolved
/// states report both.
pub(crate) fn visit_resolved_states(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    visit: &mut dyn FnMut(StateView<'_, '_>) -> Result<(), ResourceLimit>,
) -> Result<(), ResourceLimit> {
    let design = DesignRecords::new(ir);
    visit_states(ctx, &design, &ModelIndex::build(ir), visit)
}

fn visit_states<'a>(
    ctx: &DecodeContext<'_>,
    index: &DesignRecords<'a>,
    ids: &ModelIndex<'_>,
    visit: &mut dyn FnMut(StateView<'_, 'a>) -> Result<(), ResourceLimit>,
) -> Result<(), ResourceLimit> {
    let ir = index.ir;
    let evaluations = &ir.design.evaluations;
    let records = || evaluations.iter().map(Stored::Evaluation);
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
    let mut seen = HashSet::new();
    firsts.extend(
        evaluations
            .iter()
            .enumerate()
            .filter_map(|(at, evaluation)| seen.insert(evaluation.id.as_str()).then_some(at)),
    );
    {
        let forest = Forest::of_evaluations(evaluations);
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
                if !firsts.contains(&at) {
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
