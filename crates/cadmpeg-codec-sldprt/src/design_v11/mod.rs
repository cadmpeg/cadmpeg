// SPDX-License-Identifier: Apache-2.0
//! The CADIR v11 design layer of a part (part 5 of the CADIR v11
//! specification), projected from the native Keywords feature history beside
//! the v10 design records.
//!
//! The part is one design scope over its root definition, which owns the
//! bodies of the current geometry. The feature tree is one ordered timeline
//! whose current state, at its end, holds those bodies; the scope holds the
//! part's parameters and configurations, and an evaluation of each other
//! configuration's stored state. The name tables of those states come from
//! the persistent ids of their faces and bodies. A part whose history holds
//! no feature, or that has no history the decoder reads, states no design.

mod configurations;
mod extrude;
mod names;
mod operation;
mod parameters;
mod sketch;
mod states;
#[cfg(test)]
mod tests;
mod tree;
mod typed;
mod units;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::NameTable;
use cadmpeg_ir::design::refs::Position;
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::units::{UnitSymbol, UnitSystem};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::loss::LossNote;

use crate::records::{FeatureHistory, FeatureInputLane};

pub(crate) use names::Persistent;
pub(crate) use units::length_unit;

/// What a part's design is projected from.
pub(crate) struct Part<'a> {
    pub(crate) histories: &'a [FeatureHistory],
    /// The `ResolvedFeatures` lanes whose scalars type the v10 parameters.
    pub(crate) lanes: &'a [FeatureInputLane],
    /// The bodies of the current geometry: those of the active
    /// configuration.
    pub(crate) bodies: Vec<BodyId>,
    /// The persistent ids of every decoded face and body.
    pub(crate) persistent: Persistent<'a>,
    /// The part's length unit.
    pub(crate) length: Option<UnitSymbol>,
}

/// Project the design of a part document into `ir.design`, beside its v10
/// records, and state the part's root definition. Returns the number of
/// records added.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    part: Part<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<usize, CodecError> {
    let Part {
        histories,
        lanes,
        bodies,
        persistent,
        length,
    } = part;
    let mut timeline = tree::timeline(histories)?;
    if timeline.features.is_empty() {
        return Ok(0);
    }
    let definition = root_definition();
    let entries = parameters::Entries::new(histories, &timeline)?;
    let typed = typed::project(
        &ir.model,
        histories,
        lanes,
        &mut timeline,
        &entries,
        &definition,
    );
    losses.extend(typed.loss);
    let readings = parameters::Readings {
        decoded: parameters::Decoded::new(&ir.model.parameters, lanes),
        length_unit: length,
        bound: typed.bound,
        measured: typed.measured,
    };
    let mut parameters = entries.parameters(&definition, &readings);
    let sketches = typed.sketches;
    parameters.extend(sketches.driven);
    let configurations =
        configurations::configurations(histories, ir.source.as_ref(), &definition)?;
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
    let names = names::Names::new(persistent, histories, &timeline.features_by_record);
    let table = names.entries(&ir.geometry, &bodies, |_| true);
    let (evaluations, state_records) = states::configuration_states(
        ctx,
        ir,
        &states::Scope {
            definition: &definition,
            timeline: &id,
            features: &timeline.features,
            configurations: &configurations.records,
            active: configurations.active.as_ref(),
            current: &bodies,
        },
        &names,
    )?;
    let container = Container::Ordered {
        id,
        kind: OrderedKind::Timeline,
        members,
        folders: timeline.folders,
        rollback: Rollback::End,
        placement: None,
    };
    let features = timeline.features;
    let added = 3
        + features.len()
        + sketches.records.len()
        + sketches.entities.len()
        + sketches.constraints.len()
        + parameters.len()
        + configurations.records.len()
        + table.len()
        + evaluations.len()
        + state_records;
    let mut root = ProductDefinition::root(definition.clone(), ExternalDocument::default());
    root.kind = Some(ProductDefinitionKind::Part);
    root.units = length.map(|length| UnitSystem {
        length,
        angle: UnitSymbol::Deg,
        mass: None,
        time: None,
    });
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
    ir.design.sketches.extend(sketches.records);
    ir.design.sketch_entities.extend(sketches.entities);
    ir.design.sketch_constraints.extend(sketches.constraints);
    ir.design.parameters.extend(parameters);
    ir.design.configurations.extend(configurations.records);
    ir.design.evaluations.extend(evaluations);
    ir.design.names = NameTable::from(table);
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
