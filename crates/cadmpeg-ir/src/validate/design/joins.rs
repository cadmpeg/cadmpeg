// SPDX-License-Identifier: Apache-2.0
//! An `Apply` with `Join` occurs only where the feature's input state, where
//! it is stored, holds a body the feature can join: a first solid feature is
//! `NewBody` (§6).
//!
//! A feature that builds a solid joins a solid body; any other feature joins
//! a body of any kind. The input state is the state entering the feature, in
//! each stored form it has: the current state at that position, or each
//! stored evaluation of it, read through its chain of bases.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::feature::FeatureOperation;
use crate::design::options::{BodyForm, Boolean, BooleanOp, SolidOutput};
use crate::design::selection::Anchor;
use crate::report::check::Finding;
use crate::topology::BodyKind;

use super::index::DesignIndex;
use super::ops::{boolean_field, BooleanField};
use super::selections::{anchored_states, Site, Stored};
use super::states::States;
use super::Holder;

static INPUT: Anchor = Anchor::Input;

/// Checks every feature whose definition, in the configuration of a stored
/// input state, joins.
pub(super) fn check<'a>(
    index: &DesignIndex<'a>,
    states: &'a States<'a>,
    findings: &mut Vec<Finding>,
) {
    let records = &index.records;
    let ir = records.ir;
    let kinds: BTreeMap<&str, &BodyKind> = ir
        .geometry
        .bodies
        .iter()
        .map(|body| (body.id.as_str(), &body.kind))
        .collect();
    for feature in &ir.design.features {
        let id = feature.id.as_str();
        let site = Site {
            holder: Holder::Definition(feature),
            anchor: &INPUT,
            owner: index.owner_definition(id),
        };
        let mut messages = BTreeSet::new();
        for state in anchored_states(index, states, site) {
            let configuration = state.configuration(records);
            let operation = &records.definition_in(feature, configuration).operation;
            let joins = matches!(
                boolean_field(operation),
                BooleanField::Stated(Boolean::Apply {
                    op: BooleanOp::Join,
                    ..
                })
            );
            if !joins {
                continue;
            }
            let Some(bodies) = input_bodies(states, &state, &kinds) else {
                continue;
            };
            let solid = builds_solid(operation);
            let joinable = bodies
                .iter()
                .any(|kind| !solid || matches!(kind, BodyKind::Solid | BodyKind::General));
            if !joinable {
                messages.insert(if solid {
                    "an Apply with Join builds a solid whose stored input state holds no solid body; a first solid feature is NewBody"
                } else {
                    "an Apply with Join enters a stored input state that holds no body"
                });
            }
        }
        for message in messages {
            super::error(
                findings,
                id,
                crate::rule!("design.join-input-holds-no-target-body", "{message}"),
            );
        }
    }
}

/// The kinds of the bodies of a stored state; `None` where a chain does not
/// resolve.
fn input_bodies<'a>(
    states: &States<'a>,
    state: &Stored<'a>,
    kinds: &BTreeMap<&'a str, &'a BodyKind>,
) -> Option<Vec<&'a BodyKind>> {
    match state {
        Stored::Extents(extents) => Some(
            extents
                .iter()
                .flat_map(|extent| &extent.state.bodies)
                .filter_map(|body| kinds.get(body.as_str()).copied())
                .collect(),
        ),
        Stored::Evaluation(evaluation) => {
            let chain = states.chain(evaluation)?;
            let mut bodies: BTreeMap<&str, &BodyKind> = chain
                .bottom
                .iter()
                .flat_map(|extent| &extent.state.bodies)
                .filter_map(|body| Some((body.as_str(), *kinds.get(body.as_str())?)))
                .collect();
            for (records, removed) in chain.layers.iter().rev() {
                bodies.retain(|body, _| !removed.contains(body));
                bodies.extend(
                    records
                        .bodies
                        .iter()
                        .map(|body| (body.id.as_str(), &body.kind)),
                );
            }
            Some(bodies.into_values().collect())
        }
    }
}

/// Whether `operation` builds a solid: a solid or thin-walled result of a
/// swept, primitive, or implicit solid, a rib, a thickening, or a boundary
/// fill.
fn builds_solid(operation: &FeatureOperation) -> bool {
    let solid =
        |output: &SolidOutput| matches!(output.body, Some(BodyForm::Solid | BodyForm::Thin(_)));
    match operation {
        FeatureOperation::Extrude(op) => solid(&op.output),
        FeatureOperation::Revolve(op) => solid(&op.output),
        FeatureOperation::Sweep(op) => solid(&op.output),
        FeatureOperation::HelicalSweep(op) => solid(&op.output),
        FeatureOperation::Primitive(op) => solid(&op.output),
        FeatureOperation::Loft(op) => solid(&op.output),
        FeatureOperation::ImplicitSweep(op) => solid(&op.output),
        FeatureOperation::Rib(_)
        | FeatureOperation::Thicken(_)
        | FeatureOperation::BoundaryFill(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
