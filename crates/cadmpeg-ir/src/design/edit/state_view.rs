// SPDX-License-Identifier: Apache-2.0
//! Checked, read-only resolution of current states and evaluation states.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, ResourceLimits};

use super::states::State;
use super::EditError;
use crate::design::evaluation::StateBase;
use crate::design::ids::EvaluationId;
use crate::design::names::NameEntry;
use crate::design::read::Ceiling;
use crate::design::refs::{EntityRef, Position};
use crate::design::selection::OccurrencePath;
use crate::document::Topology;
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::transform::Transform;
use crate::CadIr;

/// A stored state with its base and deltas composed (§5.4, §5.5).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedState(State);

impl ResolvedState {
    /// The topology records of the state.
    #[must_use]
    pub const fn records(&self) -> &Topology {
        &self.0.records
    }

    /// The name entries of the state.
    #[must_use]
    pub const fn names(&self) -> &BTreeMap<EntityRef, NameEntry> {
        &self.0.names
    }

    /// Body ownership entries, with the fallback of §5.4 for absent entries.
    #[must_use]
    pub const fn owners(&self) -> &BTreeMap<BodyId, ProductDefinitionId> {
        &self.0.owners
    }

    /// Occurrence membership entries, with the fallback of §5.4 for absent entries.
    #[must_use]
    pub const fn members(&self) -> &BTreeMap<OccurrenceId, ProductDefinitionId> {
        &self.0.members
    }

    /// The instance-body entries of the state.
    #[must_use]
    pub const fn instance_bodies(&self) -> &BTreeMap<OccurrencePath, Vec<BodyId>> {
        &self.0.instance_bodies
    }

    /// Placements that differ from the resolved configuration placements (§5.4).
    #[must_use]
    pub const fn poses(&self) -> &BTreeMap<OccurrencePath, Transform> {
        &self.0.poses
    }
}

/// Why a stored design state cannot be resolved.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StateResolutionError {
    /// The evaluation id is absent, including an id named by a base.
    #[error("evaluation {0} is absent")]
    MissingEvaluation(EvaluationId),
    /// The evaluation records a status without stored content.
    #[error("evaluation {0} has no stored content")]
    MissingContent(EvaluationId),
    /// The scope has no current state at the position.
    #[error("scope {scope} has no current state at {at:?}")]
    MissingCurrentState {
        /// The scope's definition.
        scope: ProductDefinitionId,
        /// The requested position.
        at: Position,
    },
    /// An evaluation is its own ancestor through its bases.
    #[error("evaluation {0} is its own ancestor")]
    Cycle(EvaluationId),
    /// The chain exceeds the supplied state-chain ceiling.
    #[error("evaluation state chain exceeds the ceiling of {limit}")]
    ChainDepthExceeded {
        /// The supplied ceiling.
        limit: u64,
    },
    /// The state compositor could not read a record.
    #[error(transparent)]
    Composition(#[from] EditError),
}

impl CadIr {
    /// Resolves the extent of the scope's current state at `at` (§5.5).
    ///
    /// # Errors
    ///
    /// Returns [`StateResolutionError`] when the current state is absent or
    /// the compositor cannot read a record. This checks state resolution;
    /// document validation checks the records' semantic invariants.
    pub fn resolved_current_state(
        &self,
        scope: &ProductDefinitionId,
        at: &Position,
    ) -> Result<ResolvedState, StateResolutionError> {
        let current = self.current_state(scope, at).ok_or_else(|| {
            StateResolutionError::MissingCurrentState {
                scope: scope.clone(),
                at: at.clone(),
            }
        })?;
        Ok(ResolvedState(self.extent(scope, current)?))
    }

    /// Resolves an evaluation's stored state under
    /// [`ResourceLimits::max_state_chain_depth`] (§5.4).
    ///
    /// # Errors
    ///
    /// Returns [`StateResolutionError`] for an absent evaluation or current
    /// base, content absent from any link, a cycle, a chain past the ceiling,
    /// or a record the compositor cannot read. Document validation checks
    /// the records' semantic invariants.
    pub fn resolved_evaluation_state(
        &self,
        id: &EvaluationId,
        limits: &ResourceLimits,
    ) -> Result<ResolvedState, StateResolutionError> {
        let limit = Ceiling::StateChain.of(limits);
        let mut seen = BTreeSet::new();
        let mut next = id;
        loop {
            let evaluation = self
                .design
                .evaluations
                .iter()
                .find(|evaluation| &evaluation.id == next)
                .ok_or_else(|| StateResolutionError::MissingEvaluation(next.clone()))?;
            let content = evaluation
                .content
                .as_ref()
                .ok_or_else(|| StateResolutionError::MissingContent(evaluation.id.clone()))?;
            if !seen.insert(&evaluation.id) {
                return Err(StateResolutionError::Cycle(evaluation.id.clone()));
            }
            if u64_from_index(seen.len()) > limit {
                return Err(StateResolutionError::ChainDepthExceeded { limit });
            }
            match &content.base {
                StateBase::Empty => break,
                StateBase::Current(at) => {
                    if self.current_state(&evaluation.scope, at).is_none() {
                        return Err(StateResolutionError::MissingCurrentState {
                            scope: evaluation.scope.clone(),
                            at: at.clone(),
                        });
                    }
                    break;
                }
                StateBase::Evaluation(base) => next = base,
            }
        }
        Ok(ResolvedState(self.resolved(id)?))
    }
}

#[cfg(test)]
mod tests;
