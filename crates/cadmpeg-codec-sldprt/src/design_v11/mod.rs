// SPDX-License-Identifier: Apache-2.0
//! The CADIR v11 design layer of a part (part 5 of the CADIR v11
//! specification), projected from the native Keywords feature history beside
//! the v10 design records.
//!
//! The part is one design scope over its root definition, which owns the
//! bodies of the current geometry. The feature tree is one ordered timeline
//! whose rollback bar the decoder does not read (part 11, entry B.94), so its
//! rollback is unstated and its current state, at `After(Container(timeline))`
//! (§5.3), holds those bodies; the scope holds the
//! part's parameters and configurations. A part whose history holds no
//! feature, or that has no history the decoder reads, states no design.

mod configurations;
mod operation;
mod parameters;
#[cfg(test)]
mod tests;
mod tree;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind};
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};

use crate::records::{FeatureHistory, FeatureInputLane};

/// Project the design of a part document into `ir.design`, beside its v10
/// records, and state the part's root definition.
///
/// `lanes` are the `ResolvedFeatures` lanes whose scalars type the v10
/// parameters. `bodies` are the bodies of the current geometry: those of the
/// active configuration. Returns the number of records added.
pub(crate) fn project(
    ir: &mut CadIr,
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    bodies: Vec<BodyId>,
) -> Result<usize, CodecError> {
    let timeline = tree::timeline(histories)?;
    if timeline.features.is_empty() {
        return Ok(0);
    }
    let definition = root_definition();
    let decoded = parameters::Decoded::new(&ir.model.parameters, lanes);
    let parameters = parameters::parameters(histories, &timeline, &definition, &decoded)?;
    let configurations =
        configurations::configurations(histories, ir.source.as_ref(), &definition)?;
    let id = container_id(cadmpeg_ir::identity_key!("timeline"));
    let members = timeline.members();
    let at = Position::After(Member::Container(id.clone()));
    let current = (!bodies.is_empty())
        .then(|| current_state(at, bodies.clone()))
        .into_iter()
        .collect();
    let container = Container::Ordered {
        id,
        kind: OrderedKind::Timeline,
        members,
        folders: timeline.folders,
        rollback: None,
        placement: None,
    };
    let features = timeline.features;
    let added = 3 + features.len() + parameters.len() + configurations.records.len();
    let mut root = ProductDefinition::root(definition.clone(), ExternalDocument::default());
    root.kind = Some(ProductDefinitionKind::Part);
    root.bodies = bodies;
    ir.model.product_definitions.push(root);
    ir.design.scopes.push(DesignScope {
        definition,
        roots: vec![container.id().clone()],
        active_configuration: configurations.active,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current,
    });
    ir.design.containers.push(container);
    ir.design.features.extend(features);
    ir.design.parameters.extend(parameters);
    ir.design.configurations.extend(configurations.records);
    Ok(added)
}

/// The root definition: the part file itself, which no occurrence places.
fn root_definition() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    )
}

fn container_id(key: impl Into<cadmpeg_ir::ids::IdentityKey>) -> ContainerId {
    ContainerId::from(cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "model", "container"),
        key,
    ))
}

fn current_state(at: Position, bodies: Vec<BodyId>) -> CurrentState {
    CurrentState {
        at,
        bodies,
        poses: BTreeMap::new(),
        status: EvaluationStatus::Current,
        native_ref: None,
    }
}
