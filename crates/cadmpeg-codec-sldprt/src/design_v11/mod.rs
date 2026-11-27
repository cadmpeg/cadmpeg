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
//! their faces and bodies. Each sketch that no feature of the timeline
//! restates is the operation of a feature of one unordered `Group`, a root of
//! the scope after the timeline (§5.2). A part whose history holds no feature,
//! or that has no history the decoder reads, states a scope only where it
//! holds a sketch: that `Group` is its one root, its bodies are unproduced,
//! and no configuration has a stated state.

mod configurations;
mod datum;
mod equations;
mod extrude;
mod names;
mod operation;
mod order;
pub(crate) mod parameters;
mod pattern;
mod profile_check;
mod revolve;
mod sketch;
pub(crate) mod states;
mod swept;
#[cfg(test)]
mod tests;
mod tree;
mod typed;
mod units;

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::NameTable;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::units::{UnitSymbol, UnitSystem};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, ProductDefinitionKind};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::SketchEntityId;

use crate::loss::{NamedCount, SldprtLossCode};
use crate::records::{FeatureHistory, FeatureInputLane};
use crate::resolved_features::assembly::ConfigurationMaps;
use crate::rollback::{Reading, Unread};

pub(crate) use equations::{managers as equation_managers, Managers as EquationManagers};
pub(crate) use names::Persistent;
pub(crate) use parameters::{displayed_length, DisplayedLength};
pub(crate) use units::{length_places, length_unit};

/// What a part's design is projected from.
pub(crate) struct Part<'a> {
    pub(crate) histories: &'a [FeatureHistory],
    /// The histories whose features also hold the parameters the decoder
    /// reads from `ResolvedFeatures` scalars, PMI dimensions, and Move Face
    /// translations; their ordinals give the parameters their ids.
    pub(crate) parameter_histories: &'a [FeatureHistory],
    /// The `ResolvedFeatures` lanes whose scalars type the v10 parameters.
    pub(crate) lanes: &'a [FeatureInputLane],
    /// The `Config-N` class map each lane continues, by lane id.
    pub(crate) configuration_maps: &'a ConfigurationMaps,
    /// The bodies of the current geometry: those of the active
    /// configuration.
    pub(crate) bodies: Vec<BodyId>,
    /// The persistent ids of every decoded face and body.
    pub(crate) persistent: Persistent<'a>,
    /// The part's length unit.
    pub(crate) length: Option<UnitSymbol>,
    /// The equation managers of the configuration streams.
    pub(crate) equations: EquationManagers,
    /// The number of decimal places at which the part shows lengths: the
    /// largest its configurations state.
    pub(crate) length_places: Option<u32>,
    /// What the stored marks state of the rollback position.
    pub(crate) rollback: Reading,
    /// The sketch entities read from a curve record of state `0`: curves
    /// outside their sketch's profile.
    pub(crate) outside_profile: &'a HashSet<SketchEntityId>,
    /// The native ids of the configurations whose partition was given up.
    pub(crate) given_up_configurations: &'a BTreeSet<String>,
}

/// What [`project`] stated.
#[derive(Default)]
pub(crate) struct Projection {
    /// The number of records added.
    pub(crate) added: usize,
    /// The geometry records that the evaluations of the configurations that
    /// are not active hold and no current body reaches, by id, with the
    /// configuration whose state holds each. They are no part of the current
    /// geometry (§5.5).
    pub(crate) retired: BTreeMap<String, ConfigurationId>,
}

/// Project the design of a part document into `ir.design`, beside its v10
/// records, and state the part's root definition. Each configuration that is
/// not active has its stored state as an evaluation of its own. A rollback
/// reading that does not fix the position, a configuration whose state cannot
/// be stated, a sketch, extrude or revolve that cannot be typed, a parameter
/// whose literal is a rounded display, and a dimension whose bare-number text
/// has no value add their reasons to `losses`. A part whose timeline is empty
/// states its sketches and its configurations in a scope whose bodies are
/// unproduced (§5.5); a part that states no scope has its equation-manager
/// lines go to `losses`. Interim: a body the geometry of such a part holds
/// outside the active configuration's bodies is one more unproduced body of
/// its scope, and a loss names it. A feature node under a tree node that is
/// no timeline member states no design feature, and a loss names it, unless
/// it owns a sketch that no timeline member restates: it is then a member of
/// the sketches' `Group`. A reference-dimension parameter that no dimension
/// binds is a `Driven` parameter of its owner where that owner is a design
/// feature.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    part: Part<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Projection, CodecError> {
    let Part {
        histories,
        parameter_histories,
        lanes,
        configuration_maps,
        mut bodies,
        persistent,
        length,
        equations: managers,
        length_places,
        rollback,
        outside_profile,
        given_up_configurations,
    } = part;
    let active = equations::active_slot(histories, ir.source.as_ref());
    let mut timeline = tree::timeline(histories)?;
    timeline.grouped = sketch::owners_outside(&ir.model, &timeline.features);
    timeline
        .skipped
        .retain(|(_, feature)| !timeline.grouped.contains(feature));
    let mut skipped = NamedCount::default();
    for (_, feature) in &timeline.skipped {
        skipped.push(feature.as_str());
    }
    if !skipped.is_empty() {
        losses.push(SldprtLossCode::DesignFeatureOutsideTimeline.note(format!(
            "{} feature nodes under a feature-tree node that is no timeline member (an \
             annotations, body or equations folder, or another node with no design meaning) \
             state no design feature: {skipped}",
            skipped.count()
        )));
    }
    let listed = !timeline.features.is_empty();
    let definition = root_definition();
    let mut configurations =
        configurations::configurations(histories, ir.source.as_ref(), &definition, losses)?;
    if listed {
        losses.extend(order::regeneration_order(&mut timeline, &ir.model));
    } else if ir.model.sketches.is_empty()
        && ir.model.spatial_sketches.is_empty()
        && configurations.records.is_empty()
    {
        losses.extend(equations::without_design(&managers, active));
        return Ok(Projection::default());
    }
    let entries = parameters::Entries::new(histories, parameter_histories, &timeline)?;
    losses.extend(entries.outside(&ir.model.parameters));
    let typed = typed::project(
        &ir.model,
        histories,
        lanes,
        &mut timeline,
        &entries,
        &typed::Scope {
            ctx,
            definition: &definition,
            outside_profile,
            surfaces: &ir.geometry.surfaces,
            angular: ir.tolerances.angular.get(),
        },
    )?;
    losses.extend(typed.loss);
    losses.extend(typed.assumed);
    losses.extend(typed.state_construction);
    losses.extend(typed.dimension_circles);
    losses.extend(typed.profile_differs);
    losses.extend(typed.sketch_empty);
    losses.extend(typed.profile_unchecked);
    losses.extend(typed.revolve_axis_lines);
    losses.extend(typed.revolve_axis_derived);
    losses.extend(typed.revolve_boolean_unstated);
    losses.extend(typed.revolve_profile_differs);
    losses.extend(typed.revolve_profile_unchecked);
    losses.extend(typed.revolve_sense_assumed);
    losses.extend(typed.sketch_notes);
    losses.extend(typed.datums);
    let mut sketches = typed.sketches;
    let group = group(std::mem::take(&mut sketches.outside));
    if !listed && group.is_none() && configurations.records.is_empty() {
        losses.extend(equations::without_design(&managers, active));
        return Ok(Projection::default());
    }
    let mut readings = parameters::Readings {
        decoded: parameters::Decoded::new(&ir.model.parameters, lanes, configuration_maps),
        length_unit: length,
        length_places,
        bound: typed.bound,
        measured: typed.measured,
        driven: typed.references,
        manager: equations::stored_values(
            &managers,
            active,
            &timeline.features,
            |feature, name| entries.owned(feature, name).cloned(),
        ),
        confirmed: HashSet::new(),
    };
    let linking = equations::Linking {
        managers: &managers,
        scope: &definition,
        active,
        features: &timeline.features,
        length_unit: length,
        tolerances: &ir.tolerances,
    };
    let (mut parameters, mut notes, mut bare) = entries.parameters(&definition, &readings);
    let entered = parameters.len();
    parameters.extend(std::mem::take(&mut sketches.driven));
    readings.confirmed = equations::results(&parameters, &readings.manager, &linking);
    if !readings.confirmed.is_empty() {
        let driven = parameters.split_off(entered);
        (parameters, notes, bare) = entries.parameters(&definition, &readings);
        parameters.extend(driven);
    }
    losses.extend(notes);
    losses.extend(equations::bare_references(
        &mut parameters,
        &bare,
        &timeline.features,
        length,
        &ir.tolerances,
    ));
    losses.extend(equations::link(&mut parameters, &linking));
    let design_features = timeline
        .features
        .iter()
        .chain(group.iter().flat_map(|(_, members)| members))
        .map(|feature| &feature.id)
        .collect::<HashSet<_>>();
    losses.extend(parameters::references_unbound(
        &ir.model.parameters,
        &mut parameters,
        &design_features,
        &definition,
    ));
    configurations::parameter_overrides(
        &mut configurations,
        &ir.model.configurations,
        &parameters,
        length,
    );
    losses.extend(configurations::unread_states(
        &configurations,
        &ir.model.configurations,
        &timeline.features,
    ));
    let mut containers = Vec::new();
    let mut current = Vec::new();
    let mut unproduced = BTreeSet::new();
    let mut table = BTreeMap::new();
    let mut states = states::States::default();
    if listed {
        let id = container_id(cadmpeg_ir::identity_key!("timeline"));
        let members = timeline.members();
        let (rollback, at) = match stated(&timeline.features, rollback) {
            Ok((built, None)) => {
                let (rollback, at) = built_through(&id, &members, built);
                (Some(rollback), at)
            }
            Ok((_, Some(unread))) | Err(unread) => {
                losses.push(SldprtLossCode::HistoryRollbackUnread.note(format!(
                    "The rollback position of the feature tree is not read ({unread}); the \
                     timeline's rollback is not stated."
                )));
                (None, Position::After(Member::Container(id.clone())))
            }
        };
        if !bodies.is_empty() {
            current.push(current_state(at, bodies.clone()));
        }
        let designed = sketches
            .entities
            .iter()
            .map(|entity| &entity.id)
            .collect::<HashSet<_>>();
        let generators = crate::resolved_features::side_generators::SideGenerators::derive(
            &ir.model, histories, lanes,
        )
        .into_stated(|id| designed.contains(id));
        let names = names::Names::new(persistent, histories, &timeline.features_by_record)
            .with_generators(generators);
        table = names.entries(&ir.geometry, &bodies, |_| true);
        states = states::configuration_states(
            ctx,
            ir,
            &states::Scope {
                definition: &definition,
                timeline: &id,
                features: &timeline.features,
                configurations: &configurations.records,
                active: configurations.active.as_ref(),
                current: &bodies,
                given_up: given_up_configurations,
            },
            &names,
            losses,
        )?;
        containers.push(Container::Ordered {
            id,
            kind: OrderedKind::Timeline,
            members,
            folders: timeline.folders,
            rollback,
            placement: None,
        });
    } else {
        if let Some(note) = configurations::unplaced(&ir.geometry.bodies, &bodies) {
            losses.push(note);
            bodies = ir
                .geometry
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect();
        }
        unproduced = bodies.iter().cloned().collect();
        for configuration in &configurations.records {
            if Some(&configuration.id) != configurations.active.as_ref() {
                losses.push(SldprtLossCode::ConfigStateUnstated.note(format!(
                    "configuration {}: the part stores no feature history, so its state is \
                     not stated.",
                    configuration.id
                )));
            }
        }
    }
    let mut features = timeline.features;
    if let Some((group, members)) = group {
        containers.push(group);
        features.extend(members);
    }
    let roots = containers
        .iter()
        .map(|container| container.id().clone())
        .collect::<Vec<_>>();
    let spatial = sketches
        .spatial
        .iter()
        .map(|restated| 1 + restated.entities.len() + restated.constraints.len())
        .sum::<usize>();
    let added = 2
        + containers.len()
        + features.len()
        + sketches.records.len()
        + sketches.entities.len()
        + sketches.constraints.len()
        + spatial
        + parameters.len()
        + configurations.records.len()
        + table.len()
        + states.evaluations.len()
        + states.records;
    ir.model
        .product_definitions
        .push(root(definition.clone(), length, bodies));
    ir.design.scopes.push(DesignScope {
        definition,
        roots,
        active_configuration: configurations.active,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current,
        unproduced,
    });
    ir.design.containers.extend(containers);
    ir.design.features.extend(features);
    ir.design.sketches.extend(sketches.records);
    ir.design.sketch_entities.extend(sketches.entities);
    ir.design.sketch_constraints.extend(sketches.constraints);
    for restated in sketches.spatial {
        ir.design.spatial_sketches.push(restated.sketch);
        ir.design.spatial_sketch_entities.extend(restated.entities);
        ir.design
            .spatial_sketch_constraints
            .extend(restated.constraints);
    }
    ir.design.parameters.extend(parameters);
    ir.design.configurations.extend(configurations.records);
    ir.design.evaluations.extend(states.evaluations);
    ir.design.names = NameTable::from(table);
    Ok(Projection {
        added,
        retired: states.retired,
    })
}

/// The unordered `Group` whose members are `features`, the owners of the
/// sketches outside the timeline, with those features; none where there are
/// none.
fn group(features: Vec<Feature>) -> Option<(Container, Vec<Feature>)> {
    if features.is_empty() {
        return None;
    }
    let container = Container::Unordered {
        id: container_id(cadmpeg_ir::identity_key!("sketches")),
        kind: UnorderedKind::Group,
        members: features
            .iter()
            .map(|feature| Member::Feature(feature.id.clone()))
            .collect(),
    };
    Some((container, features))
}

/// The root definition `definition`: a part in `length` units that holds
/// `bodies`.
fn root(
    definition: ProductDefinitionId,
    length: Option<UnitSymbol>,
    bodies: Vec<BodyId>,
) -> ProductDefinition {
    let mut root = ProductDefinition::root(definition, ExternalDocument::default());
    root.kind = Some(ProductDefinitionKind::Part);
    root.units = length.map(|length| UnitSystem {
        length,
        angle: UnitSymbol::Deg,
        mass: None,
        time: None,
    });
    root.bodies = bodies;
    root
}

/// The number of leading timeline features the stored marks state built:
/// through the feature at the current mark where a later feature is unbuilt
/// and unsuppressed, else all of them. A feature between the feature at the
/// current mark and the first such feature that holds no mark record leaves
/// the position unread: the count is then a lower bound, returned with that
/// reason, and the timeline states no rollback.
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
