// SPDX-License-Identifier: Apache-2.0
//! Part 10 rules for appearances, appearance bindings, and source attributes
//! (§8) that the reference checks of `topology` leave out: the targets that
//! name a shell, a loop, a region, a carrier, a design record, a product,
//! PMI, drawing, or presentation record, or a neutral record by its arena;
//! and the `native_ref` of each record.

use std::collections::BTreeSet;

use crate::appearance::AppearanceTarget;
use crate::attributes::AttributeTarget;
use crate::document::design_records::DesignRecords;
use crate::document::CadIr;
use crate::index::{ModelIndex, StandardIndex};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::schema::ArenaRecord;

pub(super) fn check_appearance_attributes(
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    check_native_refs(ir, ids, findings);
    if ir.model.appearance_bindings.is_empty() && ir.model.attributes.is_empty() {
        return;
    }
    let records = DesignRecords::of(ir);
    let model = ModelRecords::of(ir);
    for binding in &ir.model.appearance_bindings {
        let owner = binding.id.as_str();
        match &binding.target.entity {
            AppearanceTarget::Occurrence(id) if !model.occurrences.contains(id.as_str()) => {
                missing(findings, owner, "occurrence", id.as_str());
            }
            AppearanceTarget::Record { arena, id } => record(ir, findings, owner, arena, id),
            _ => {}
        }
    }
    for attribute in &ir.model.attributes {
        let owner = attribute.id.as_str();
        if let AttributeTarget::Record { arena, id } = &attribute.target.entity {
            record(ir, findings, owner, arena, id);
        } else if let Some((kind, id)) =
            unresolved_target(&attribute.target.entity, ids, &records, &model)
        {
            missing(findings, owner, kind, id);
        }
    }
}

/// The ids of the model arenas that an attribute or a binding names and
/// that no [`ModelIndex`] lookup indexes.
struct ModelRecords<'a> {
    product_definitions: BTreeSet<&'a str>,
    occurrences: BTreeSet<&'a str>,
    pmi: BTreeSet<&'a str>,
    semantic_annotations: BTreeSet<&'a str>,
    drawings: BTreeSet<&'a str>,
    view_presentations: BTreeSet<&'a str>,
}

impl<'a> ModelRecords<'a> {
    fn of(ir: &'a CadIr) -> Self {
        fn ids<T: ArenaRecord>(records: &[T]) -> BTreeSet<&str> {
            records.iter().map(ArenaRecord::record_identity).collect()
        }
        let model = &ir.model;
        Self {
            product_definitions: ids(&model.product_definitions),
            occurrences: ids(&model.occurrences),
            pmi: ids(&model.pmi),
            semantic_annotations: ids(&model.semantic_annotations),
            drawings: ids(&model.drawings),
            view_presentations: ids(&model.view_presentations),
        }
    }
}

/// The kind and id of an attribute target that names no record of its kind.
/// The document, body, face, coedge, edge, vertex, and configuration arms
/// are the reference checks' own.
fn unresolved_target<'t>(
    target: &'t AttributeTarget,
    ids: &ModelIndex<'_>,
    records: &DesignRecords<'_>,
    model: &ModelRecords<'_>,
) -> Option<(&'static str, &'t str)> {
    let (kind, id, found) = match target {
        AttributeTarget::Shell(id) => (
            "shell",
            id.as_str(),
            ids.shells(id.as_str(), StandardIndex).is_some(),
        ),
        AttributeTarget::Loop(id) => (
            "loop",
            id.as_str(),
            ids.loops(id.as_str(), StandardIndex).is_some(),
        ),
        AttributeTarget::Region(id) => (
            "region",
            id.as_str(),
            ids.regions(id.as_str(), StandardIndex).is_some(),
        ),
        AttributeTarget::Surface(id) => (
            "surface",
            id.as_str(),
            ids.surfaces(id.as_str(), StandardIndex).is_some(),
        ),
        AttributeTarget::Curve(id) => (
            "curve",
            id.as_str(),
            ids.curves(id.as_str(), StandardIndex).is_some(),
        ),
        AttributeTarget::Point(id) => (
            "point",
            id.as_str(),
            ids.points(id.as_str(), StandardIndex).is_some(),
        ),
        AttributeTarget::Tessellation(id) => (
            "tessellation",
            id.as_str(),
            ids.tessellations(id, StandardIndex).is_some(),
        ),
        AttributeTarget::Feature(id) => ("feature", id.as_str(), records.holds_feature(id)),
        AttributeTarget::Sketch(id) => ("sketch", id.as_str(), records.holds_sketch(id)),
        AttributeTarget::Parameter(id) => ("parameter", id.as_str(), records.holds_parameter(id)),
        AttributeTarget::ProductDefinition(id) => (
            "product definition",
            id.as_str(),
            model.product_definitions.contains(id.as_str()),
        ),
        AttributeTarget::Occurrence(id) => (
            "occurrence",
            id.as_str(),
            model.occurrences.contains(id.as_str()),
        ),
        AttributeTarget::Pmi(id) => (
            "PMI annotation",
            id.as_str(),
            model.pmi.contains(id.as_str()),
        ),
        AttributeTarget::SemanticAnnotation(id) => (
            "semantic annotation",
            id.as_str(),
            model.semantic_annotations.contains(id.as_str()),
        ),
        AttributeTarget::Drawing(id) => {
            ("drawing", id.as_str(), model.drawings.contains(id.as_str()))
        }
        AttributeTarget::ViewPresentation(id) => (
            "view presentation",
            id.as_str(),
            model.view_presentations.contains(id.as_str()),
        ),
        AttributeTarget::Document
        | AttributeTarget::Body(_)
        | AttributeTarget::Face(_)
        | AttributeTarget::Coedge(_)
        | AttributeTarget::Edge(_)
        | AttributeTarget::Vertex(_)
        | AttributeTarget::Configuration(_)
        | AttributeTarget::Record { .. } => return None,
    };
    (!found).then_some((kind, id))
}

macro_rules! record_arena_holds {
    (@arena $ir:ident, $arena:ident, $id:ident, $named:ident, $found:ident, $field:ident, process) => {};
    (@arena $ir:ident, $arena:ident, $id:ident, $named:ident, $found:ident, $field:ident, $home:ident) => {
        if $arena == stringify!($field) {
            $named = true;
            $found = $found
                || $ir
                    .$home
                    .$field
                    .iter()
                    .any(|record| ArenaRecord::record_identity(record) == $id);
        }
    };
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Whether `arena` is the field name of an arena of the topology,
        /// the model, or the design, and if so, whether a record of one such
        /// arena has `id`. A field name the design shares with a model arena
        /// names both.
        fn record_arena_holds(ir: &CadIr, arena: &str, id: &str) -> Option<bool> {
            let mut named = false;
            let mut found = false;
            $(record_arena_holds!(@arena ir, arena, id, named, found, $field, $home);)*
            named.then_some(found)
        }
    };
}
crate::document::every_arena_registry!(record_arena_holds);

/// A `Record` target names an arena of the model, the design, or the
/// topology, and an id of that arena.
fn record(
    ir: &CadIr,
    findings: &mut Vec<Finding>,
    owner: &str,
    arena: &cadmpeg_core::text::NonBlankString,
    id: &str,
) {
    match record_arena_holds(ir, arena.as_str(), id) {
        None => error(
            findings,
            owner,
            format_args!("record target names `{arena}`, which is no arena of the model, the design, or the topology"),
        ),
        Some(false) => error(
            findings,
            owner,
            format_args!("record target `{id}` is no record of arena `{arena}`"),
        ),
        Some(true) => {}
    }
}

/// Every `native_ref` of an appearance, a binding, and an attribute resolves.
fn check_native_refs(ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    let mut check = |owner: &str, native_ref: Option<&String>| {
        let Some(target) = native_ref else { return };
        let resolves = super::annotations_native::native_link_resolves(
            ir,
            |target| ids.native_record_owns(target),
            target,
        );
        if !resolves {
            error(
                findings,
                owner,
                format_args!("native_ref `{target}` does not resolve"),
            );
        }
    };
    for appearance in &ir.model.appearances {
        check(appearance.id.as_str(), appearance.native_ref.as_ref());
    }
    for binding in &ir.model.appearance_bindings {
        check(binding.id.as_str(), binding.native_ref.as_ref());
    }
    for attribute in &ir.model.attributes {
        check(attribute.id.as_str(), attribute.native_ref.as_ref());
    }
}

fn missing(findings: &mut Vec<Finding>, owner: &str, kind: &str, id: &str) {
    error(
        findings,
        owner,
        format_args!("references missing {kind} `{id}`"),
    );
}

fn error(findings: &mut Vec<Finding>, entity: &str, message: std::fmt::Arguments<'_>) {
    super::record_finding(
        findings,
        Check::Design,
        Severity::Error,
        Some(entity),
        message,
    );
}

#[cfg(test)]
mod tests;
