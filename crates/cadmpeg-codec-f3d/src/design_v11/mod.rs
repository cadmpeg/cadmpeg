// SPDX-License-Identifier: Apache-2.0
//! The CADIR v11 design layer of a Fusion design (part 5 of the CADIR v11
//! specification), projected from the decoder's timeline records and its v10
//! records.
//!
//! A Fusion document is one design scope at its root definition, whose
//! timeline holds the features of every component (§5.1). Each timeline record
//! of a Design stream that lists an item is an ordered `Timeline` container,
//! a root of that scope, and each item is one feature of it (§5.2). A design
//! that the document references keeps its own Design streams under an
//! occurrence path and is one scope of its own, at the definition that stands
//! for it; where the decode states no such definition, its timelines are roots
//! of the scope that encloses it. A stream that lists no item states no
//! container.
//!
//! Each planar and spatial sketch is the operation of the feature that owns
//! it (§6.1). A sketch whose owner has no item of a listing timeline is the
//! operation of a member of one unordered `Group` of its document's scope, a
//! root after the timelines; a document whose streams list no item and hold
//! such a sketch, or a construction plane, axis, or point, has a scope whose
//! roots are that group, with its sketches and construction features as
//! members, and whose bodies are unproduced (§5.2, §5.5). A document with
//! neither has no scope.
//!
//! A scope spans its definition and the embedded component definitions below
//! it; where it spans several, each feature names the component the decoder
//! lists it under. The scope holds the parameters (§5.12) and configurations
//! (§5.11) of its document, and its current state, at the rollback marker of
//! its first timeline, holds the bodies of the definitions it spans. The user
//! parameters and configuration rows of a referenced design without a scope
//! of its own belong to no scope here, and a loss names each of them.

mod configurations;
mod datum;
mod operation;
pub(crate) mod parameters;
mod pattern;
mod sheet_metal;
mod sketch;
mod split;
#[cfg(test)]
mod tests;
mod thread;
mod timeline;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, DecodeDepth};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ProductDefinition, Storage};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::records::entity_header::{DesignFeatureTimeline, SegmentType};
use crate::records::feature::scope::DesignScopeTailFlags;

/// The native arena that holds the timeline records.
const TIMELINES: &str = "design_feature_timelines";
/// The native arena that holds the `MetaStream` type tables.
const TYPES: &str = "design_types";
/// The native arena that holds the parameter scopes.
const SCOPES: &str = "design_parameter_scopes";

/// The tail flags of one parameter scope.
#[derive(serde::Deserialize)]
struct ScopeTail {
    id: String,
    record_index: u32,
    tail_flags: Option<DesignScopeTailFlags>,
}

/// Project the design of a decoded Fusion document into `ir.design`, beside
/// its v10 records, and push what it leaves unstated to `losses`. The root
/// definition is already stated.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    depth: DecodeDepth,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if depth < DecodeDepth::Design {
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let timelines = namespace.arena_as_for_decode::<DesignFeatureTimeline>(ctx, TIMELINES)?;
    if ir.model.sketches.is_empty()
        && ir.model.spatial_sketches.is_empty()
        && timelines
            .iter()
            .all(|timeline| timeline.frame().items().is_empty())
        && !ir
            .model
            .features
            .iter()
            .any(|feature| sketch::construction(feature.evaluation.definition().operation()))
    {
        return Ok(());
    }
    let types = namespace.arena_as_for_decode::<SegmentType>(ctx, TYPES)?;
    let scope_tails = namespace.arena_as_for_decode::<ScopeTail>(ctx, SCOPES)?;
    let tail_flags: timeline::TailFlags<'_> = scope_tails
        .iter()
        .filter_map(|scope| {
            Some((
                (
                    crate::ids::native_stream(&scope.id)?,
                    u64::from(scope.record_index),
                ),
                scope.tail_flags?,
            ))
        })
        .collect();
    let current = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect::<BTreeSet<_>>();
    let scopes = namespace
        .arena_as_for_decode::<crate::records::feature::scope::DesignParameterScope>(
            ctx,
            "design_parameter_scopes",
        )?;
    let documents = timeline::project(
        ctx,
        &ir.model.features,
        &timelines,
        &types,
        &timeline::ScopeInputs {
            records: &scopes,
            tail_flags: &tail_flags,
        },
        &current,
        losses,
    )?;
    let listing = documents.keys().cloned().collect::<BTreeSet<_>>();
    let mut scopes = BTreeMap::new();
    let mut hosted = Vec::new();
    for (prefix, projected) in documents {
        match scope_definition(&ir.model.product_definitions, &prefix) {
            Some(definition) => {
                scopes.insert(prefix, (definition, projected));
            }
            None => hosted.push((prefix, projected)),
        }
    }
    let hosted_prefixes = hosted
        .iter()
        .map(|(prefix, _)| prefix.clone())
        .collect::<BTreeSet<_>>();
    let placers = Placers::new(&ir.model.product_definitions);
    let mut definitions = BTreeMap::new();
    for (prefix, projected) in hosted {
        let enclosing = host(&scopes, &placers, &prefix);
        let (definition, into) = scopes.entry(enclosing).or_insert_with(|| {
            (
                root_definition(&ir.model.product_definitions),
                timeline::Projected::default(),
            )
        });
        definitions.insert(prefix, definition.clone());
        into.containers.extend(projected.containers);
        into.features.extend(projected.features);
    }
    let restated = scopes
        .values()
        .flat_map(|(_, projected)| &projected.features)
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    for (prefix, owners) in sketch::outside(&ir.model, &restated, &listing, losses)? {
        let enclosing = if scopes.contains_key(&prefix) {
            prefix.clone()
        } else if let Some(definition) = scope_definition(&ir.model.product_definitions, &prefix) {
            scopes.insert(prefix.clone(), (definition, timeline::Projected::default()));
            prefix.clone()
        } else {
            host(&scopes, &placers, &prefix)
        };
        let (_, into) = scopes.entry(enclosing).or_insert_with(|| {
            (
                root_definition(&ir.model.product_definitions),
                timeline::Projected::default(),
            )
        });
        sketch::hold(ctx, &prefix, owners, into)?;
    }
    definitions.extend(
        scopes
            .iter()
            .map(|(prefix, (definition, _))| (prefix.clone(), definition.clone())),
    );
    let scoped_parameters;
    let scoped_configurations;
    {
        let features = scopes
            .values()
            .flat_map(|(_, projected)| &projected.features)
            .map(|feature| feature.id.as_str())
            .collect::<BTreeSet<_>>();
        scoped_parameters = parameters::project(
            &ir.model.parameters,
            &definitions,
            &hosted_prefixes,
            &features,
            losses,
        );
        scoped_configurations = configurations::project(
            &ir.model.configurations,
            &definitions,
            &hosted_prefixes,
            &features,
            &scoped_parameters,
            losses,
        );
    }
    let incidence = sketch::Incidence::new(
        &namespace.arena_as_for_decode::<sketch::CurveRecord>(ctx, "sketch_curve_identities")?,
        &namespace.arena_as_for_decode::<sketch::PointRecord>(ctx, "sketch_points")?,
    );
    let mut spans = span(&ir.model.product_definitions, &placers, &scopes);
    let mut sketches = sketch::Sketches::default();
    let owners = ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((&feature.id, feature.product_definition.as_ref()?)))
        .collect::<HashMap<_, _>>();
    for (prefix, (definition, mut projected)) in scopes {
        let Span {
            definitions: spanned,
            bodies,
        } = spans.remove(&prefix).unwrap_or_default();
        if spanned.len() > 1 {
            for feature in &mut projected.features {
                feature.component = owners
                    .get(&feature.id)
                    .filter(|owner| spanned.contains(**owner))
                    .map(|owner| (*owner).clone());
            }
        }
        let (current, unproduced) = match projected.containers.first() {
            Some(Container::Ordered {
                id,
                members,
                rollback,
                ..
            }) => {
                let at = rollback_position(id, members, rollback.as_ref());
                let current = (!bodies.is_empty())
                    .then(|| CurrentState {
                        at,
                        bodies: bodies.into_iter().collect(),
                        poses: BTreeMap::new(),
                        status: EvaluationStatus::Current,
                        native_ref: None,
                    })
                    .into_iter()
                    .collect();
                (current, BTreeSet::new())
            }
            Some(Container::Unordered { .. }) => (Vec::new(), bodies),
            None => continue,
        };
        let unordered = projected
            .containers
            .iter()
            .filter_map(|container| match container {
                Container::Unordered { members, .. } => Some(members),
                Container::Ordered { .. } => None,
            })
            .flatten()
            .filter_map(|member| match member {
                Member::Feature(feature) => Some(feature.clone()),
                Member::Container(_) => None,
            })
            .collect::<BTreeSet<FeatureId>>();
        let in_scope = scoped_parameters
            .iter()
            .filter(|parameter| parameter.scope == definition)
            .collect::<Vec<_>>();
        sketch::project(
            &sketch::Scope {
                ctx,
                model: &ir.model,
                incidence: &incidence,
                linear: ir.tolerances.linear.get(),
                definition: &definition,
                unordered: &unordered,
                parameters: &in_scope,
            },
            &mut projected.features,
            &mut sketches,
            losses,
        )?;
        ctx.charge_entities(1, "project F3D v11 design scope")?;
        ir.design.scopes.push(DesignScope {
            active_configuration: scoped_configurations.active.get(&definition).cloned(),
            definition,
            roots: projected
                .containers
                .iter()
                .map(|container| container.id().clone())
                .collect(),
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current,
            unproduced,
        });
        ir.design.containers.extend(projected.containers);
        ir.design.features.extend(projected.features);
    }
    ctx.charge_entities(
        u64::try_from(scoped_parameters.len() + scoped_configurations.records.len())
            .map_err(|_| CodecError::malformed("F3D v11 design record count exceeds u64"))?,
        "project F3D v11 parameters and configurations",
    )?;
    ctx.charge_entities(
        u64::try_from(
            sketches.records.len()
                + sketches.entities.len()
                + sketches.constraints.len()
                + sketches.spatial.len()
                + sketches.spatial_entities.len()
                + sketches.spatial_constraints.len()
                + sketches.parameters.len(),
        )
        .map_err(|_| CodecError::malformed("F3D v11 sketch record count exceeds u64"))?,
        "project F3D v11 sketches",
    )?;
    ir.design.sketches.extend(sketches.records);
    ir.design.sketch_entities.extend(sketches.entities);
    ir.design.sketch_constraints.extend(sketches.constraints);
    ir.design.spatial_sketches.extend(sketches.spatial);
    ir.design
        .spatial_sketch_entities
        .extend(sketches.spatial_entities);
    ir.design
        .spatial_sketch_constraints
        .extend(sketches.spatial_constraints);
    ir.design.parameters.extend(scoped_parameters);
    ir.design.parameters.extend(sketches.parameters);
    ir.design
        .configurations
        .extend(scoped_configurations.records);
    sheet_metal::project(ctx, ir, &definitions, losses)?;
    thread::project(ctx, ir, losses)?;
    Ok(())
}

/// The position of the rollback marker of the timeline `id`: after its last
/// built member, or at its start when it builds none; `After(Container(id))`
/// where the rollback is unstated.
fn rollback_position(
    id: &ContainerId,
    members: &[Member],
    rollback: Option<&Rollback>,
) -> Position {
    let built = match rollback {
        None => return Position::After(Member::Container(id.clone())),
        Some(Rollback::End) => members.len(),
        Some(Rollback::Before(marker)) => members
            .iter()
            .position(|member| member == marker)
            .unwrap_or(members.len()),
    };
    members
        .get(..built)
        .and_then(<[_]>::last)
        .cloned()
        .map_or_else(|| Position::Start(id.clone()), Position::After)
}

/// The document prefix of the archive member that places each archive
/// member, keyed by the member's document prefix; empty for a member that the
/// root document places.
struct Placers(BTreeMap<String, String>);

impl Placers {
    fn new(definitions: &[ProductDefinition]) -> Self {
        Self(
            definitions
                .iter()
                .filter_map(|definition| {
                    let id = definition.id.as_str();
                    let key = crate::ids::member_definition_key(id)?;
                    let placer = crate::ids::member_key(id)
                        .map_or_else(String::new, |key| format!("{key}@"));
                    Some((format!("{key}@"), placer))
                })
                .collect(),
        )
    }

    /// The nearest document with a scope that holds the records of the
    /// document at `prefix`: that document, else the nearest member that
    /// places it, else the root document.
    fn nearest<T>(&self, prefix: &str, scopes: &BTreeMap<String, T>) -> Option<String> {
        std::iter::successors(Some(prefix), |at| {
            (!at.is_empty()).then(|| self.0.get(*at).map_or("", String::as_str))
        })
        .take(self.0.len() + 2)
        .find(|at| scopes.contains_key(*at))
        .map(str::to_owned)
    }
}

/// The scope that holds the timelines of the document at `prefix`, which has
/// no definition of its own: the scope of the nearest member that places it,
/// else the root scope.
fn host<T>(scopes: &BTreeMap<String, T>, placers: &Placers, prefix: &str) -> String {
    placers.nearest(prefix, scopes).unwrap_or_default()
}

/// The definition whose design the document at `prefix` holds: the root
/// definition for the root document, and the definition that stands for a
/// referenced design otherwise.
fn scope_definition(
    definitions: &[ProductDefinition],
    prefix: &str,
) -> Option<ProductDefinitionId> {
    if prefix.is_empty() {
        return Some(root_definition(definitions));
    }
    let key = prefix.strip_suffix('@')?;
    definitions
        .iter()
        .find(|definition| crate::ids::member_definition_key(definition.id.as_str()) == Some(key))
        .map(|definition| definition.id.clone())
}

/// The file's root definition: the root component where its record stands for
/// the root, else the definition the decoder states in its stead.
fn root_definition(definitions: &[ProductDefinition]) -> ProductDefinitionId {
    definitions
        .iter()
        .find(|definition| definition.is_root())
        .map_or_else(crate::tree::file_root_id, |definition| {
            definition.id.clone()
        })
}

/// The definitions a scope spans and the bodies of its current state.
#[derive(Default)]
struct Span {
    definitions: BTreeSet<ProductDefinitionId>,
    bodies: BTreeSet<BodyId>,
}

/// What each scope spans, keyed by document prefix: its own definition and
/// every `Embedded` definition without a scope that its document stores, or
/// the nearest scope that holds the records of that document (§5.1). A
/// definition stored as a file of its own, or held outside the document, is
/// spanned by no scope here.
fn span<T>(
    definitions: &[ProductDefinition],
    placers: &Placers,
    scopes: &BTreeMap<String, (ProductDefinitionId, T)>,
) -> BTreeMap<String, Span> {
    let mut out = BTreeMap::<String, Span>::new();
    for definition in definitions {
        let own = scopes
            .iter()
            .find(|(_, (scoped, _))| scoped == &definition.id)
            .map(|(prefix, _)| prefix.clone());
        let prefix = match &definition.storage {
            _ if own.is_some() => own,
            Storage::Embedded => {
                placers.nearest(timeline::document_prefix(definition.id.as_str()), scopes)
            }
            Storage::File { .. } | Storage::External(_) => None,
        };
        if let Some(prefix) = prefix {
            let span = out.entry(prefix).or_default();
            span.definitions.insert(definition.id.clone());
            span.bodies.extend(definition.bodies.iter().cloned());
        }
    }
    out
}
