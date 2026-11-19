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
//! for it. A stream that lists no item states no container, and a document
//! whose streams state none has no scope.
//!
//! A scope spans its definition and the embedded component definitions below
//! it. Its current state, at the end of its first timeline, holds the bodies
//! of the definitions it spans.

mod operation;
#[cfg(test)]
mod tests;
mod timeline;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
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
/// its v10 records. The root definition is already stated.
pub(crate) fn project(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
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
    let documents = timeline::project(ctx, &ir.model.features, &timelines, &types)?;
    let scopes = documents
        .into_iter()
        .filter_map(|(prefix, projected)| {
            let definition = scope_definition(&ir.model.product_definitions, &prefix)?;
            Some((prefix, (definition, projected)))
        })
        .collect::<BTreeMap<_, _>>();
    let mut spans = span(&ir.model.product_definitions, &scopes);
    for (prefix, (definition, projected)) in scopes {
        let Span { bodies, .. } = spans.remove(&prefix).unwrap_or_default();
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
            definition,
            roots: projected
                .containers
                .iter()
                .map(|container| container.id().clone())
                .collect(),
            active_configuration: None,
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current,
        });
        ir.design.containers.extend(projected.containers);
        ir.design.features.extend(projected.features);
    }
    Ok(())
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
