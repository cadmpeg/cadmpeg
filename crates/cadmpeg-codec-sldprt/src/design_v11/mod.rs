// SPDX-License-Identifier: Apache-2.0
//! The CADIR v11 design layer of a part (part 5 of the CADIR v11
//! specification), projected from the native Keywords feature history beside
//! the v10 design records.
//!
//! The part is one design scope over its root definition, which owns the
//! bodies of the current geometry. The feature tree is one ordered timeline
//! whose current state, at its end, holds those bodies; the scope holds the
//! part's parameters and configurations. A part whose history
//! holds no feature, or that has no history the decoder reads, follows the
//! no-timeline rule: one unordered group holding one stored base feature per
//! body.

mod configurations;
mod operation;
mod parameters;
#[cfg(test)]
mod tests;
mod tree;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, StoredOrigin};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};

use crate::history::project::EncodedNativeKey;
use crate::records::FeatureHistory;

/// Project the design of a part document into `ir.design`, beside its v10
/// records, and state the part's root definition.
///
/// `bodies` are the bodies of the current geometry: those of the active
/// configuration. Returns the number of records added.
pub(crate) fn project(
    ir: &mut CadIr,
    histories: &[FeatureHistory],
    bodies: Vec<BodyId>,
) -> Result<usize, CodecError> {
    let timeline = tree::timeline(histories)?;
    if timeline.features.is_empty() && bodies.is_empty() {
        return Ok(0);
    }
    let definition = root_definition();
    let parameters = parameters::parameters(histories, &timeline, &definition)?;
    let configurations =
        configurations::configurations(histories, ir.source.as_ref(), &definition)?;
    let (container, features, current) = if timeline.features.is_empty() {
        no_timeline(&bodies)?
    } else {
        let id = container_id(cadmpeg_ir::identity_key!("timeline"));
        let members = timeline.members();
        let at = members
            .last()
            .cloned()
            .map_or_else(|| Position::Start(id.clone()), Position::After);
        let current = (!bodies.is_empty())
            .then(|| current_state(at, bodies.clone()))
            .into_iter()
            .collect();
        let container = Container::Ordered {
            id,
            kind: OrderedKind::Timeline,
            members,
            folders: timeline.folders,
            rollback: Rollback::End,
            placement: None,
        };
        (container, timeline.features, current)
    };
    let added = 3 + features.len() + parameters.len() + configurations.records.len();
    ir.model.product_definitions.push(ProductDefinition {
        id: definition.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies,
        sketches: Vec::new(),
        tessellations: Vec::new(),
        native_ref: None,
    });
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

/// The definition of the part file itself, which no occurrence places.
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

/// One root group of one stored base feature per body, each body the current
/// state after its feature.
fn no_timeline(
    bodies: &[BodyId],
) -> Result<(Container, Vec<Feature>, Vec<CurrentState>), CodecError> {
    let mut members = BTreeSet::new();
    let mut features = Vec::new();
    let mut current = Vec::new();
    for body in bodies {
        let key = body
            .as_str()
            .rsplit_once('#')
            .map_or(body.as_str(), |(_, key)| key);
        let id = FeatureId::mint(format!(
            "sldprt:model:feature#stored:{}",
            EncodedNativeKey(key)
        ))
        .map_err(CodecError::malformed)?;
        members.insert(Member::Feature(id.clone()));
        current.push(current_state(
            Position::After(Member::Feature(id.clone())),
            vec![body.clone()],
        ));
        features.push(Feature {
            id,
            name: None,
            suppressed: None,
            definition: FeatureDefinition {
                operation: FeatureOperation::Stored {
                    bodies: Some(vec![body.clone()]),
                    tessellations: Vec::new(),
                    origin: StoredOrigin::Base {},
                },
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        });
    }
    let container = Container::Unordered {
        id: container_id(cadmpeg_ir::identity_key!("group")),
        kind: UnorderedKind::Group,
        members,
    };
    Ok((container, features, current))
}
