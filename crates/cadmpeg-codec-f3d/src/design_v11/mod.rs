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
//! of the scope that encloses it. A stream that lists no item states no container, and a document
//! whose streams state none has no scope.
//!
//! A scope spans its definition and the embedded component definitions below
//! it; where it spans several, each feature names the component the decoder
//! lists it under. The scope holds the parameters (§5.12) and configurations
//! (§5.11) of its document, and its current state, at the end of its first
//! timeline, holds the bodies of the definitions it spans. The user
//! parameters and configuration rows of a referenced design without a scope
//! of its own belong to no scope here, and a loss names each of them.

mod configurations;
mod operation;
mod parameters;
#[cfg(test)]
mod tests;
mod timeline;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::refs::Position;
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ProductDefinition, Storage};
use cadmpeg_ir::CadIr;

use crate::records::entity_header::{DesignFeatureTimeline, SegmentType};

/// The native arena that holds the timeline records.
const TIMELINES: &str = "design_feature_timelines";
/// The native arena that holds the `MetaStream` type tables.
const TYPES: &str = "design_types";

/// Project the design of a decoded Fusion document into `ir.design`, beside
/// its v10 records, and push what it leaves unstated to the losses of
/// `body`. The root definition is already stated. A decode that delivers no
/// design class, as a geometry request that stopped before design intent,
/// states no design record.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    body: &mut DecodeBody,
) -> Result<(), CodecError> {
    if !body.transfer.depth().scope().design {
        return Ok(());
    }
    let losses = &mut body.losses;
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let timelines = namespace.arena_as_for_decode::<DesignFeatureTimeline>(ctx, TIMELINES)?;
    if timelines
        .iter()
        .all(|timeline| timeline.frame().items().is_empty())
    {
        return Ok(());
    }
    let types = namespace.arena_as_for_decode::<SegmentType>(ctx, TYPES)?;
    let current = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect::<BTreeSet<_>>();
    let documents = timeline::project(
        ctx,
        &ir.model.features,
        &timelines,
        &types,
        &current,
        losses,
    )?;
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
    let mut definitions = BTreeMap::new();
    for (prefix, projected) in hosted {
        let enclosing = host(&scopes, &prefix);
        let (definition, into) = scopes
            .entry(enclosing)
            .or_insert_with(|| (crate::tree::file_root_id(), timeline::Projected::default()));
        definitions.insert(prefix, definition.clone());
        into.containers.extend(projected.containers);
        into.features.extend(projected.features);
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
    let mut spans = span(&ir.model.product_definitions, &scopes);
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
        let Some(Container::Ordered { id, members, .. }) = projected.containers.first() else {
            continue;
        };
        let at = members
            .last()
            .cloned()
            .map_or_else(|| Position::Start(id.clone()), Position::After);
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
        });
        ir.design.containers.extend(projected.containers);
        ir.design.features.extend(projected.features);
    }
    ctx.charge_entities(
        u64::try_from(scoped_parameters.len() + scoped_configurations.records.len())
            .map_err(|_| CodecError::malformed("F3D v11 design record count exceeds u64"))?,
        "project F3D v11 parameters and configurations",
    )?;
    ir.design.parameters.extend(scoped_parameters);
    ir.design
        .configurations
        .extend(scoped_configurations.records);
    Ok(())
}

/// The scope that holds the timelines of the document at `prefix`, which has
/// no definition of its own: the scope of the longest document prefix that
/// begins it, else the root scope.
fn host<T>(scopes: &BTreeMap<String, T>, prefix: &str) -> String {
    scopes
        .keys()
        .filter(|scoped| prefix.starts_with(scoped.as_str()))
        .max_by_key(|scoped| scoped.len())
        .cloned()
        .unwrap_or_default()
}

/// The definition whose design the document at `prefix` holds: the root
/// definition for the root document, and the definition that stands for a
/// referenced design otherwise.
fn scope_definition(
    definitions: &[ProductDefinition],
    prefix: &str,
) -> Option<ProductDefinitionId> {
    if prefix.is_empty() {
        return Some(crate::tree::file_root_id());
    }
    let member = crate::ids::archive_product_definition_id();
    let id = format!("f3d:{prefix}{}", member.as_str().strip_prefix("f3d:")?);
    definitions
        .iter()
        .find(|definition| definition.id.as_str() == id)
        .map(|definition| definition.id.clone())
}

/// The definitions a scope spans and the bodies of its current state.
#[derive(Default)]
struct Span {
    definitions: BTreeSet<ProductDefinitionId>,
    bodies: BTreeSet<BodyId>,
}

/// What each scope spans, keyed by document prefix: its own definition and
/// every `Embedded` definition without a scope whose id the scope's document
/// prefix begins, the longest such prefix winning (§5.1). A definition stored
/// as a file of its own, or held outside the document, is spanned by no scope
/// here.
fn span<T>(
    definitions: &[ProductDefinition],
    scopes: &BTreeMap<String, (ProductDefinitionId, T)>,
) -> BTreeMap<String, Span> {
    let mut out = BTreeMap::<String, Span>::new();
    for definition in definitions {
        let own = scopes
            .iter()
            .find(|(_, (scoped, _))| scoped == &definition.id)
            .map(|(prefix, _)| prefix);
        let spanning = || {
            let id = definition.id.as_str();
            let local = id.strip_prefix("f3d:").unwrap_or(id);
            scopes
                .keys()
                .filter(|prefix| local.starts_with(prefix.as_str()))
                .max_by_key(|prefix| prefix.len())
        };
        let prefix = match &definition.storage {
            _ if own.is_some() => own,
            Storage::Embedded => spanning(),
            Storage::File { .. } | Storage::External(_) => None,
        };
        if let Some(prefix) = prefix {
            let span = out.entry(prefix.clone()).or_default();
            span.definitions.insert(definition.id.clone());
            span.bodies.extend(definition.bodies.iter().cloned());
        }
    }
    out
}
