// SPDX-License-Identifier: Apache-2.0
//! The states of a part other than its current state (§5.4).
//!
//! `SolidWorks` stores the model of a configuration it has rebuilt as a
//! partition of its own (`Config-N-Partition`). The active configuration's is
//! the current geometry (§5.5). Every other configuration whose partition the
//! decoder reads has its state at the end of the tree as an evaluation of its
//! own, based on `Empty`: its partition is a kernel index space the current
//! geometry does not hold, so its records leave the current geometry. A
//! configuration whose partition is not stored and that the source flags as
//! needing a rebuild (`swConfigurationNeedsUpdate`) has an evaluation with no
//! content and status `Stale`; a stored partition so flagged is a stale state.
//! A configuration whose partition is not stored charges a loss that names
//! it.
//!
//! The states between features are in [`between`]. A configuration's
//! partition stores them as its Deltas stream, which replays mark by mark
//! along the main line to the node set of every earlier state (§3.1, Stored
//! states). The first word of a feature's `moPMarkRecord_c` is the PMARK id
//! of the mark set after it. A state joined to a feature is the state after
//! it, `of: State(After(feature))`: its records those the partition's
//! builder makes of that node set, its names those of [`Names::entries`],
//! and its base the next later stated state, the last one on the
//! configuration's latest state (§5.4). A stored state that is not stated
//! has no evaluation, which §5.4 reads as not read, and charges a loss of
//! its configuration.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{
    DecodeContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::configuration::DesignConfiguration;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::{ContainerId, EvaluationId};
use cadmpeg_ir::design::ops::body::PatternInstances;
use cadmpeg_ir::design::ops::surface::MeshKind;
use cadmpeg_ir::design::refs::{Evaluated, Member, Position};
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::parameters::DesignConfiguration as ModelConfiguration;
use cadmpeg_ir::features::{ConfigurationId, FeatureId};
use cadmpeg_ir::ids::{BodyId, Identity, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::{EntityKind, EntitySchema};

use super::names::Names;

pub(super) mod between;
use crate::history::project::neutral_configuration_id;
use crate::loss::SldprtLossCode;
use crate::records::FeatureHistory;

/// The scope whose configurations' states are stated.
pub(super) struct Scope<'a> {
    pub(super) definition: &'a ProductDefinitionId,
    pub(super) timeline: &'a ContainerId,
    /// The timeline's features, in member order.
    pub(super) features: &'a [Feature],
    pub(super) configurations: &'a [DesignConfiguration],
    pub(super) active: Option<&'a ConfigurationId>,
    /// The bodies of the current state.
    pub(super) current: &'a [BodyId],
    /// The native ids of the configurations whose partition was given up.
    pub(super) given_up: &'a BTreeSet<String>,
}

/// The states of the configurations that are not active.
#[derive(Default)]
pub(super) struct States {
    /// The evaluation of each configuration whose state the source stores
    /// or flags.
    pub(super) evaluations: Vec<Evaluation>,
    /// The number of records the evaluations hold.
    pub(super) records: usize,
    /// The geometry records that only those states hold, by id, with the
    /// configuration whose state holds each: no current body reaches them,
    /// so they are no part of the current geometry (§5.5).
    pub(super) retired: BTreeMap<String, ConfigurationId>,
}

/// The states of each configuration that is not active. A configuration
/// whose stored bodies share ids with the current bodies has no evaluation,
/// because its records cannot be told from the current state's, and charges
/// a note to `losses`; so does a configuration whose partition is not
/// stored. Only a decode ceiling fails the projection.
pub(super) fn configuration_states(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    scope: &Scope<'_>,
    names: &Names<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<States, CodecError> {
    let current = scope.current.iter().collect::<HashSet<_>>();
    let (at, depended) = end_of_tree(scope);
    let mut evaluations = Vec::new();
    let mut records = 0;
    let mut held = BTreeMap::new();
    let mut index = None;
    for configuration in scope.configurations {
        if Some(&configuration.id) == scope.active {
            continue;
        }
        let native = configuration
            .native_ref
            .as_deref()
            .and_then(|id| model_configuration(ir, id));
        let stale = native
            .is_some_and(|native| crate::decode::configuration_source_needs_update(ir, native));
        let bodies = native.and_then(|native| native.bodies.as_deref());
        let content = match bodies {
            Some(bodies) if bodies.iter().all(|body| !current.contains(body)) => {
                admit_state(ctx, 0)?;
                if index.is_none() {
                    index = Some(Reach::new(ctx, &ir.geometry)?);
                }
                let Some(reach) = &index else {
                    continue;
                };
                let topology = reach.state(ctx, bodies)?;
                let table = names.entries(&topology, bodies, |feature| depended.contains(feature));
                records += topology_len(&topology) + table.len();
                topology_ids(&topology, |id| {
                    held.entry(id.as_str().to_owned())
                        .or_insert_with(|| configuration.id.clone());
                });
                Some(StateContent {
                    base: StateBase::Empty,
                    delta: StateDelta {
                        records: topology,
                        names: table
                            .into_iter()
                            .map(|(entity, entry)| (entity, Some(entry)))
                            .collect(),
                        ..StateDelta::default()
                    },
                })
            }
            Some(bodies) => {
                let shared = bodies
                    .iter()
                    .filter(|body| current.contains(body))
                    .map(BodyId::as_str)
                    .collect::<Vec<_>>();
                losses.push(SldprtLossCode::ConfigStateUnstated.note(format!(
                    "configuration {}: its stored bodies share {} id(s) with the current bodies ({}); its state is not stated.",
                    configuration.id,
                    shared.len(),
                    shared.join(", ")
                )));
                continue;
            }
            None => {
                if let Some(native) = native {
                    let given_up = native
                        .native_ref
                        .as_ref()
                        .is_some_and(|native_ref| scope.given_up.contains(native_ref));
                    losses.push(not_stored(configuration, stale, given_up));
                }
                if !stale {
                    continue;
                }
                None
            }
        };
        evaluations.push(Evaluation {
            id: evaluation_id(&configuration.id),
            scope: scope.definition.clone(),
            configuration: Some(configuration.id.clone()),
            of: Evaluated::State(at.clone()),
            status: if stale {
                EvaluationStatus::Stale
            } else {
                EvaluationStatus::Current
            },
            content,
            native_ref: None,
        });
    }
    if let Some(reach) = index.filter(|_| !held.is_empty()) {
        let current = reach.state_ids(ctx, scope.current)?;
        held.retain(|id, _| !current.contains(id.as_str()));
    }
    Ok(States {
        evaluations,
        records,
        retired: held,
    })
}

fn model_configuration<'a>(ir: &'a CadIr, id: &str) -> Option<&'a ModelConfiguration> {
    ir.model
        .configurations
        .iter()
        .find(|native| native.native_ref.as_deref() == Some(id))
}

/// The geometry of inactive configurations that no current body reaches.
pub(super) fn retired_records(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    histories: &[FeatureHistory],
    current: &[BodyId],
) -> Result<BTreeMap<String, ConfigurationId>, CodecError> {
    let Some((configurations, active)) =
        super::configurations::native_with_active(histories, ir.source.as_ref())
    else {
        return Ok(BTreeMap::new());
    };
    let mut index = None;
    let mut retired = BTreeMap::new();
    let active = &configurations[active].id;
    for configuration in &configurations {
        if &configuration.id == active {
            continue;
        }
        let Some(bodies) = model_configuration(ir, &configuration.id)
            .and_then(|native| native.bodies.as_deref())
            .filter(|bodies| {
                !bodies.is_empty() && bodies.iter().all(|body| !current.contains(body))
            })
        else {
            continue;
        };
        if index.is_none() {
            index = Some(Reach::new(ctx, &ir.geometry)?);
        }
        if let Some(reach) = &index {
            let owner = neutral_configuration_id(&configuration.id)?;
            for id in reach.state_ids(ctx, bodies)? {
                retired
                    .entry(id.to_owned())
                    .or_insert_with(|| owner.clone());
            }
        }
    }
    if let Some(reach) = index {
        let current = reach.state_ids(ctx, current)?;
        retired.retain(|id, _| !current.contains(id.as_str()));
    }
    Ok(retired)
}

fn admit_state(ctx: &DecodeContext<'_>, base_depth: u64) -> Result<(), CodecError> {
    let limit = ctx.policy().limits.max_state_chain_depth;
    if base_depth < limit {
        return Ok(());
    }
    Err(ResourceLimit {
        scope: LimitScope::Session,
        dimension: ResourceDimension::RecursionDepth,
        reason: ResourceFailure::BudgetExceeded,
        limit,
        used: base_depth,
        additional: 1,
        operation: "SolidWorks evaluation-chain depth",
    }
    .into())
}

/// The loss of a configuration that is not active and whose partition the
/// decode does not hold: the file stores none, or `given_up` where the
/// stored partition was given up; `stale` where the file flags it as needing
/// a rebuild.
fn not_stored(configuration: &DesignConfiguration, stale: bool, given_up: bool) -> LossNote {
    let (code, partition, flagged) = if given_up {
        (
            SldprtLossCode::ConfigStateUnstated,
            "its stored partition is given up",
            " and the file flags it as needing a rebuild, so its state is stale",
        )
    } else {
        (
            SldprtLossCode::ConfigStateNotStored,
            "the file stores no partition of it",
            " and flags it as needing a rebuild, so its state is stale",
        )
    };
    let flag = if stale {
        flagged
    } else {
        ", so its state is not stated"
    };
    code.note(format!(
        "configuration {} ({:?}): {partition}{flag}.",
        configuration.id, configuration.name
    ))
}

/// The state at the end of the tree, spelled after its last member that is
/// not stateless (§5.4), and the features that state depends on.
fn end_of_tree<'a>(scope: &Scope<'a>) -> (Position, HashSet<&'a FeatureId>) {
    let last = scope
        .features
        .iter()
        .rposition(|feature| !stateless(feature));
    let Some(last) = last else {
        return (Position::Start(scope.timeline.clone()), HashSet::new());
    };
    let depended = scope.features[..=last]
        .iter()
        .map(|feature| &feature.id)
        .collect();
    (
        Position::After(Member::Feature(scope.features[last].id.clone())),
        depended,
    )
}

/// `CadIr::is_stateless` in a configuration that overrides nothing (§5.4):
/// the feature is suppressed, or its operation cannot change a state; an
/// `Unresolved` operation is judged by its family.
fn stateless(feature: &Feature) -> bool {
    if matches!(feature.suppressed, Some(Dimension::Literal(true))) {
        return true;
    }
    match &feature.definition.operation {
        FeatureOperation::Sketch(_)
        | FeatureOperation::SpatialSketch(_)
        | FeatureOperation::Spreadsheet(_)
        | FeatureOperation::Datum(_)
        | FeatureOperation::TemplateInstance(_)
        | FeatureOperation::Decal(_) => true,
        FeatureOperation::Pattern(pattern) => {
            matches!(pattern.instances, PatternInstances::Members(_))
        }
        FeatureOperation::Mesh(mesh) => matches!(mesh.kind, MeshKind::Section { .. }),
        FeatureOperation::Unresolved { family } => matches!(
            family,
            OperationFamily::Sketch
                | OperationFamily::SpatialSketch
                | OperationFamily::Spreadsheet
                | OperationFamily::Datum
                | OperationFamily::TemplateInstance
                | OperationFamily::Decal
        ),
        _ => false,
    }
}

/// The evaluation id of a configuration's state, in the configuration id's
/// key.
fn evaluation_id(configuration: &ConfigurationId) -> EvaluationId {
    EvaluationId::from(Identity::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "model", "evaluation"),
        Identity::from(configuration.clone()).key(),
    ))
}

/// The arena kind of a record slice.
fn kind_of<T: EntitySchema>(_: &[T]) -> EntityKind {
    T::KIND
}

macro_rules! topology_arenas {
    ($($field:ident),* $(,)?) => {
        /// The records of a `Topology` by id, and the records that reference
        /// each of them.
        struct Reach<'g> {
            geometry: &'g Topology,
            slots: HashMap<&'g str, (EntityKind, usize)>,
            referrers: HashMap<&'g str, Vec<&'g str>>,
        }

        impl<'g> Reach<'g> {
            fn new(ctx: &DecodeContext<'_>, geometry: &'g Topology) -> Result<Self, CodecError> {
                let mut slots = HashMap::new();
                $(for (at, record) in geometry.$field.iter().enumerate() {
                    slots.entry(record.identity()).or_insert((kind_of(&geometry.$field), at));
                })*
                let mut reach = Self {
                    geometry,
                    slots,
                    referrers: HashMap::new(),
                };
                let mut referrers = HashMap::<&'g str, Vec<&'g str>>::new();
                let mut targets = Vec::new();
                for &id in reach.slots.keys() {
                    targets.clear();
                    reach.references(ctx, id, &mut targets)?;
                    for &target in &targets {
                        referrers.entry(target).or_default().push(id);
                    }
                }
                reach.referrers = referrers;
                Ok(reach)
            }

            /// Pushes onto `found` each record of the topology that the record
            /// `id` references.
            fn references(
                &self,
                ctx: &DecodeContext<'_>,
                id: &str,
                found: &mut Vec<&'g str>,
            ) -> Result<(), CodecError> {
                let Some(&(kind, at)) = self.slots.get(id) else {
                    return Ok(());
                };
                let mut visit = |target: &str| {
                    if let Some((target, _)) = self.slots.get_key_value(target) {
                        found.push(*target);
                    }
                    Ok(())
                };
                $(if kind == kind_of(&self.geometry.$field) {
                    return self.geometry.$field[at].visit_references(ctx, &mut visit);
                })*
                Ok(())
            }

            /// The records whose parent pointers lead to a body in `bodies`,
            /// and every record those reference.
            fn state_ids(&self, ctx: &DecodeContext<'_>, bodies: &[BodyId]) -> Result<HashSet<&'g str>, CodecError> {
                let mut owned = HashSet::<&str>::new();
                let mut pending = bodies
                    .iter()
                    .filter_map(|body| self.slots.get_key_value(body.as_str()).map(|(id, _)| *id))
                    .collect::<Vec<_>>();
                while let Some(id) = pending.pop() {
                    if owned.insert(id) {
                        pending.extend(self.referrers.get(id).into_iter().flatten().copied());
                    }
                }
                let mut held = HashSet::<&str>::new();
                let mut pending = owned.into_iter().collect::<Vec<_>>();
                while let Some(id) = pending.pop() {
                    if held.insert(id) {
                        self.references(ctx, id, &mut pending)?;
                    }
                }
                Ok(held)
            }

            /// The records that belong to the state of `bodies`, each arena
            /// sorted by id.
            fn state(&self, ctx: &DecodeContext<'_>, bodies: &[BodyId]) -> Result<Topology, CodecError> {
                let held = self.state_ids(ctx, bodies)?;
                let mut out = Topology::default();
                $(
                    out.$field = self
                        .geometry
                        .$field
                        .iter()
                        .filter(|record| held.contains(record.identity()))
                        .cloned()
                        .collect();
                    out.$field.sort_by(|a, b| a.identity().cmp(b.identity()));
                )*
                Ok(out)
            }
        }

        fn topology_len(topology: &Topology) -> usize {
            0 $(+ topology.$field.len())*
        }

        /// Calls `visit` with the id of every record of `topology`.
        pub(crate) fn topology_ids(topology: &Topology, mut visit: impl FnMut(&Identity)) {
            $(for record in &topology.$field {
                visit(record.id.as_ref());
            })*
        }

        /// Removes from `geometry` every record whose id `ids` holds.
        pub(crate) fn remove_records<V>(geometry: &mut Topology, ids: &BTreeMap<String, V>) {
            $(geometry.$field.retain(|record| !ids.contains_key(record.identity()));)*
        }
    };
}

topology_arenas!(
    bodies,
    regions,
    shells,
    faces,
    loops,
    coedges,
    edges,
    vertices,
    points,
    surfaces,
    curves,
    subds,
    pcurves,
    procedural_surfaces,
    procedural_curves,
);
