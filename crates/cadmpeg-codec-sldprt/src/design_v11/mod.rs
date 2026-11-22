// SPDX-License-Identifier: Apache-2.0
//! The CADIR v11 design layer of a part (part 5 of the CADIR v11
//! specification), projected from the native Keywords feature history beside
//! the v10 design records.
//!
//! The part is one design scope over its root definition, which owns the
//! bodies of the current geometry. The feature tree is one ordered timeline
//! whose current state, at its rollback position (`docs/formats/sldprt.md`
//! §3.4), holds those bodies; the scope holds the part's parameters and
//! configurations, and an evaluation of each other configuration's stored
//! state. The name tables of those states come from the persistent ids of
//! their faces and bodies. A part whose history holds no feature, or that has
//! no history the decoder reads, states no design.

mod configurations;
mod extrude;
mod names;
mod operation;
mod order;
mod parameters;
mod profile_check;
mod sketch;
mod states;
#[cfg(test)]
mod tests;
mod tree;
mod typed;
mod units;

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::NameTable;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::units::{UnitSymbol, UnitSystem};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::SketchEntityId;

use crate::loss::SldprtLossCode;
use crate::records::{FeatureHistory, FeatureInputLane};
use crate::rollback::{Reading, Unread};

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
    /// What the stored marks state of the rollback position.
    pub(crate) rollback: Reading,
    /// The sketch entities read from a curve record of state `0`: curves
    /// outside their sketch's profile.
    pub(crate) outside_profile: &'a HashSet<SketchEntityId>,
}

/// Project the design of a part document into `ir.design`, beside its v10
/// records, and state the part's root definition. A rollback reading that
/// does not fix the position, a configuration whose state cannot be stated, a
/// sketch or extrude that cannot be typed, and a parameter whose literal is a
/// rounded display add their reasons to `losses`. Returns the number of
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
        rollback,
        outside_profile,
    } = part;
    let mut timeline = tree::timeline(histories)?;
    if timeline.features.is_empty() {
        return Ok(0);
    }
    losses.extend(order::regeneration_order(&mut timeline, &ir.model));
    let definition = root_definition();
    let entries = parameters::Entries::new(histories, &timeline)?;
    let typed = typed::project(
        &ir.model,
        histories,
        lanes,
        &mut timeline,
        &entries,
        &typed::Scope {
            definition: &definition,
            outside_profile,
            surfaces: &ir.geometry.surfaces,
        },
    );
    losses.extend(typed.loss);
    losses.extend(typed.assumed);
    losses.extend(typed.state_construction);
    losses.extend(typed.dimension_construction);
    losses.extend(typed.profile_differs);
    let readings = parameters::Readings {
        decoded: parameters::Decoded::new(&ir.model.parameters, lanes),
        length_unit: length,
        bound: typed.bound,
        measured: typed.measured,
        driven: typed.references,
    };
    let (mut parameters, rounded) = entries.parameters(&definition, &readings);
    losses.extend(rounded);
    let sketches = typed.sketches;
    parameters.extend(sketches.driven);
    let configurations =
        configurations::configurations(histories, ir.source.as_ref(), &definition)?;
    let id = container_id(cadmpeg_ir::identity_key!("timeline"));
    let members = timeline.members();
    let (built, unread) =
        stated(&timeline.features, rollback).unwrap_or_else(|unread| (members.len(), Some(unread)));
    if let Some(unread) = unread {
        let through = if built == members.len() {
            "its last member".to_owned()
        } else {
            format!("its first {built} members")
        };
        losses.push(SldprtLossCode::HistoryRollbackUnread.note(format!(
            "The rollback position of the feature tree is not read ({unread}); the timeline is \
             stated built through {through}."
        )));
    }
    let (rollback, at) = built_through(&id, &members, built);
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
        losses,
    )?;
    let container = Container::Ordered {
        id,
        kind: OrderedKind::Timeline,
        members,
        folders: timeline.folders,
        rollback,
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

/// The number of leading timeline features the stored marks state built:
/// through the feature at the current mark where a later feature is unbuilt
/// and unsuppressed, else all of them. A feature between the feature at the
/// current mark and the first such feature that holds no mark record leaves
/// the position unread: the count stands, with that reason.
fn stated(features: &[Feature], reading: Reading) -> Result<(usize, Option<Unread>), Unread> {
    let (holder, later, recorded) = match reading {
        Reading::End { last, recorded } => (last, Vec::new(), recorded),
        Reading::RolledBack {
            after,
            later,
            recorded,
        } => (after, later, recorded),
        Reading::Unread(unread) => return Err(unread),
    };
    let index = |name: String| {
        let found = {
            let mut named = features
                .iter()
                .enumerate()
                .filter(|(_, feature)| feature.name.as_deref() == Some(name.as_str()));
            match (named.next(), named.next()) {
                (Some((index, _)), None) => Some(index),
                _ => None,
            }
        };
        found.ok_or(Unread::FeatureUnresolved { name })
    };
    let at = index(holder)?;
    let mut bar = features.len();
    for name in later {
        let position = index(name.clone())?;
        if position <= at {
            return Err(Unread::FeatureOutOfOrder { name });
        }
        if !matches!(
            features[position].suppressed,
            Some(Dimension::Literal(true))
        ) {
            bar = bar.min(position);
        }
    }
    let built = if bar < features.len() {
        at + 1
    } else {
        features.len()
    };
    let unrecorded = features[at + 1..bar]
        .iter()
        .find(|feature| {
            feature
                .name
                .as_ref()
                .is_none_or(|name| !recorded.contains(name))
        })
        .map(|feature| Unread::UnrecordedFeature {
            name: feature
                .name
                .clone()
                .unwrap_or_else(|| feature.id.to_string()),
        });
    Ok((built, unrecorded))
}

/// The rollback of a timeline `id` whose first `built` members are built, and
/// the position of its current state.
fn built_through(id: &ContainerId, members: &[Member], built: usize) -> (Rollback, Position) {
    let at = built
        .checked_sub(1)
        .and_then(|last| members.get(last))
        .cloned()
        .map_or_else(|| Position::Start(id.clone()), Position::After);
    let rollback = members
        .get(built)
        .cloned()
        .map_or(Rollback::End, Rollback::Before);
    (rollback, at)
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
