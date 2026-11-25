// SPDX-License-Identifier: Apache-2.0
//! Part 10 rules of the feature vocabulary (§6.1 to §6.7): sketches, the
//! shared option types, and the operations of each family.
//!
//! The rules are decided by reference lookup, counting, and arithmetic on
//! stated values. A rule about operands does not fire on an `Unresolved` or
//! `Native` operation, a `Native` pick, or an `Unread` selection, and a pick
//! whose geometric kind its target does not state is not refused by a kind
//! rule. A rule about an operation's family reads an `Unresolved` family, and
//! sketch ownership holds an `Unresolved` definition to the sketches it
//! names. A rule that needs a pick resolved in a stored state (a chain's
//! connectivity, an edge's incidence) is not checked here.

mod assembly;
mod body;
mod bounds;
mod dressup;
mod options;
pub(in crate::validate) mod picks;
mod sheet_metal;
mod sketches;
mod surface;
mod swept;
#[cfg(test)]
mod tests;

use crate::report::check::Ruled;
use std::collections::BTreeSet;

use serde_json::Value;

use super::records::{DesignRecords, MemberKey};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::ops::dressup::{EmbossEffect, OffsetResult};
use crate::design::options::Boolean;
use crate::report::check::{Check, Finding};
use crate::report::Severity;

/// Where an operation is stated.
#[derive(Clone, Copy)]
pub(super) struct Site<'a> {
    /// The record findings name: the feature, or the configuration or
    /// arrangement whose override states the definition.
    entity: &'a str,
    /// The feature whose definition holds the operation.
    feature: &'a str,
    /// The scope the feature belongs to, where its containers reach one.
    scope: Option<&'a str>,
    /// Whether the definition that holds the operation states `assembly`.
    assembly: bool,
    /// The design records, which give a bound dimension its cached values.
    index: Option<&'a DesignRecords<'a>>,
    /// The definition that holds the operation.
    definition: Option<&'a FeatureDefinition>,
}

/// Check the sketches and every stated operation of `index` against the
/// rules of §6.1 to §6.7.
pub(super) fn check_operations(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let wire = serde_json::to_value(&index.ir.design).unwrap_or(Value::Null);
    sketches::check_sketches(index, &wire, findings);
    options::check_native_keys(&wire, findings);
    for (site, definition) in definitions(index) {
        check_operation(index, site, &definition.operation, findings);
        if let Some(context) = &definition.assembly {
            assembly::check_assembly_feature(index, site, &definition.operation, context, findings);
        }
    }
    for feature in &index.ir.design.features {
        check_feature(index, feature, findings);
    }
    body::check_generators(index, findings);
    assembly::check_occurrence_creators(index, findings);
    assembly::check_spans(index, findings);
    sheet_metal::check_rules(index, findings);
}

/// The `Boolean` field of an operation's result.
pub(super) enum BooleanField<'a> {
    /// The operation's result has no `Boolean`.
    Absent,
    /// The operation leaves its `Boolean` unstated.
    Unstated,
    /// The stated `Boolean`.
    Stated(&'a Boolean),
}

impl<'a> From<Option<&'a Boolean>> for BooleanField<'a> {
    fn from(boolean: Option<&'a Boolean>) -> Self {
        boolean.map_or(Self::Unstated, Self::Stated)
    }
}

/// The `Boolean` field of `operation`'s result.
pub(super) fn boolean_field(operation: &FeatureOperation) -> BooleanField<'_> {
    match operation {
        FeatureOperation::Extrude(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::Revolve(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::Sweep(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::HelicalSweep(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::Primitive(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::Loft(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::ImplicitSweep(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::Rib(op) => op.boolean.as_ref().into(),
        FeatureOperation::Thicken(op) => op.boolean.as_ref().into(),
        FeatureOperation::BoundaryFill(op) => op.boolean.as_ref().into(),
        FeatureOperation::Patch(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::Knit(op) => op.output.boolean.as_ref().into(),
        FeatureOperation::Extend(op) => op
            .output
            .as_ref()
            .and_then(|output| output.boolean.as_ref())
            .into(),
        FeatureOperation::Offset(op) => match &op.result {
            OffsetResult::Copy(output) => output.boolean.as_ref().into(),
            OffsetResult::Move {} => BooleanField::Absent,
        },
        _ => BooleanField::Absent,
    }
}

/// Whether `operation` builds a tool body that it combines with the state
/// it enters (§5.4): one that states `Boolean::Apply`, a `Hole`, or an
/// `Emboss` whose effect is not `Scribe`. `None` where the operation is
/// `Unresolved` or `Native`, or states no Boolean or effect.
pub(super) fn builds_tool_body(operation: &FeatureOperation) -> Option<bool> {
    match operation {
        FeatureOperation::Unresolved { .. } | FeatureOperation::Native { .. } => None,
        FeatureOperation::Hole(_) => Some(true),
        FeatureOperation::Emboss(op) => match op.effect {
            EmbossEffect::Emboss { .. } | EmbossEffect::Engrave { .. } => Some(true),
            EmbossEffect::Scribe => Some(false),
            EmbossEffect::Native(_) => None,
        },
        _ => match boolean_field(operation) {
            BooleanField::Absent => Some(false),
            BooleanField::Unstated => None,
            BooleanField::Stated(boolean) => Some(matches!(boolean, Boolean::Apply { .. })),
        },
    }
}

/// Every feature definition the design states, with where it is stated:
/// each feature's own, and each definition override of a configuration or
/// an arrangement.
fn definitions<'a>(index: &'a DesignRecords<'a>) -> Vec<(Site<'a>, &'a FeatureDefinition)> {
    let design = &index.ir.design;
    let site = |entity: &'a str, feature: &'a str, definition: &'a FeatureDefinition| Site {
        entity,
        feature,
        scope: index.scope_of(MemberKey::Feature(feature)),
        assembly: definition.assembly.is_some(),
        index: Some(index),
        definition: Some(definition),
    };
    let mut out: Vec<(Site<'a>, &'a FeatureDefinition)> = design
        .features
        .iter()
        .map(|feature| {
            (
                site(
                    feature.id.as_str(),
                    feature.id.as_str(),
                    &feature.definition,
                ),
                &feature.definition,
            )
        })
        .collect();
    let overrides = design
        .configurations
        .iter()
        .map(|configuration| (configuration.id.as_str(), &configuration.overrides))
        .chain(
            design
                .arrangements
                .iter()
                .map(|arrangement| (arrangement.id.as_str(), &arrangement.overrides)),
        );
    for (holder, overrides) in overrides {
        for (feature, state) in &overrides.features {
            if let Some(definition) = &state.definition {
                out.push((site(holder, feature.as_str(), definition), definition));
            }
        }
    }
    out
}

fn check_operation(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    operation: &FeatureOperation,
    findings: &mut Vec<Finding>,
) {
    match operation {
        FeatureOperation::Extrude(extrude) => swept::check_extrude(index, site, extrude, findings),
        FeatureOperation::Revolve(revolve) => swept::check_revolve(index, site, revolve, findings),
        FeatureOperation::Sweep(sweep) => swept::check_sweep(index, site, sweep, findings),
        FeatureOperation::HelicalSweep(sweep) => {
            swept::check_helical_sweep(index, site, sweep, findings);
        }
        FeatureOperation::Rib(rib) => swept::check_rib(index, site, rib, findings),
        FeatureOperation::Primitive(primitive) => {
            swept::check_primitive(index, site, primitive, findings);
        }
        FeatureOperation::Loft(loft) => swept::check_loft(index, site, loft, findings),
        FeatureOperation::Fillet(fillet) => dressup::check_fillet(site, fillet, findings),
        FeatureOperation::Chamfer(chamfer) => dressup::check_chamfer(site, chamfer, findings),
        FeatureOperation::Shell(shell) => dressup::check_shell(site, shell, findings),
        FeatureOperation::Draft(draft) => dressup::check_draft(site, draft, findings),
        FeatureOperation::Thicken(thicken) => {
            dressup::check_thicken(index, site, thicken, findings);
        }
        FeatureOperation::Offset(offset) => dressup::check_offset(index, site, offset, findings),
        FeatureOperation::Hole(hole) => dressup::check_hole(index, site, hole, findings),
        FeatureOperation::Thread(thread) => dressup::check_thread(index, site, thread, findings),
        FeatureOperation::Emboss(emboss) => dressup::check_emboss(index, site, emboss, findings),
        FeatureOperation::Dome(dome) => dressup::check_dome(site, dome, findings),
        FeatureOperation::Decal(decal) => dressup::check_decal(site, decal, findings),
        FeatureOperation::Pattern(pattern) => body::check_pattern(index, site, pattern, findings),
        FeatureOperation::TemplateInstance(instance) => {
            body::check_template_instance(index, site, instance, findings);
        }
        FeatureOperation::Combine(combine) => body::check_combine(site, combine, findings),
        FeatureOperation::Split(split) => body::check_split(site, split, findings),
        FeatureOperation::TransformBodies(transform) => {
            body::check_transform_bodies(site, transform, findings);
        }
        FeatureOperation::Flex(flex) => body::check_flex(site, flex, findings),
        FeatureOperation::MoveFaces(faces) => body::check_move_faces(site, faces, findings),
        FeatureOperation::DeleteFaces(faces) => body::check_delete_faces(site, faces, findings),
        FeatureOperation::ReplaceFaces(faces) => body::check_replace_faces(site, faces, findings),
        FeatureOperation::Graft(graft) => body::check_graft(site, graft, findings),
        FeatureOperation::Patch(patch) => surface::check_patch(index, site, patch, findings),
        FeatureOperation::Knit(knit) => surface::check_knit(index, site, knit, findings),
        FeatureOperation::Unsew(unsew) => surface::check_unsew(site, unsew, findings),
        FeatureOperation::Extend(extend) => surface::check_extend(index, site, extend, findings),
        FeatureOperation::ImplicitSweep(sweep) => {
            surface::check_implicit_sweep(index, site, sweep, findings);
        }
        FeatureOperation::BoundaryFill(fill) => {
            surface::check_boundary_fill(index, site, fill, findings);
        }
        FeatureOperation::Curve(curve) => surface::check_curve(index, site, curve, findings),
        FeatureOperation::Form(form) => surface::check_form(site, form, findings),
        FeatureOperation::SheetMetal(sheet) => {
            sheet_metal::check_sheet_metal(index, site, sheet, findings);
        }
        FeatureOperation::Mesh(mesh) => surface::check_mesh(index, site, mesh, findings),
        FeatureOperation::Place(place) => assembly::check_place(index, site, place, findings),
        FeatureOperation::PlaceSet(set) => assembly::check_place_set(index, site, set, findings),
        FeatureOperation::Joint(joint) => assembly::check_joint(index, site, joint, findings),
        FeatureOperation::RigidGroup(group) => {
            assembly::check_rigid_group(index, site, group, findings);
        }
        FeatureOperation::ComponentPattern(pattern) => {
            assembly::check_component_pattern(index, site, pattern, findings);
        }
        FeatureOperation::MirrorComponents(mirror) => {
            assembly::check_mirror(index, site, mirror, findings);
        }
        FeatureOperation::Sketch(_)
        | FeatureOperation::SpatialSketch(_)
        | FeatureOperation::Spreadsheet(_)
        | FeatureOperation::Datum(_)
        | FeatureOperation::DeleteBodies(_)
        | FeatureOperation::Extract(_)
        | FeatureOperation::UnsewFaces(_)
        | FeatureOperation::MaterialRemoval
        | FeatureOperation::Stored { .. }
        | FeatureOperation::Unresolved { .. }
        | FeatureOperation::Native { .. } => {}
    }
}

/// The rules on a feature's own fields that its operation decides.
fn check_feature(index: &DesignRecords<'_>, feature: &Feature, findings: &mut Vec<Finding>) {
    let id = feature.id.as_str();
    match &feature.definition.operation {
        FeatureOperation::Primitive(_) if feature.placement.is_some() => error(
            findings,
            id,
            crate::rule!("design.primitive-feature-states-feature-placement-placement", "a Primitive feature states a Feature.placement; its placement is Primitive.placement"),
        ),
        FeatureOperation::Place(_) if feature.placement.is_some() => error(
            findings,
            id,
            crate::rule!("design.place-feature-states-feature-placement-placement", "a Place feature states a Feature.placement; its placement is Place.placement"),
        ),
        _ => {}
    }
    assembly::check_component(index, feature, findings);
    assembly::check_driven_by(index, feature, findings);
    assembly::check_driven_definition(index, feature, findings);
}

fn error(findings: &mut Vec<Finding>, entity: &str, message: Ruled<'_>) {
    super::error(findings, entity, message);
}

fn warning(findings: &mut Vec<Finding>, entity: &str, message: Ruled<'_>) {
    super::super::record_finding(
        findings,
        Check::Design,
        Severity::Warning,
        Some(entity),
        message,
    );
}

/// Every object of `value` that is an adjacently tagged arm with tag `kind`.
fn tagged<'v>(value: &'v Value, kind: &str) -> Vec<&'v Value> {
    let mut found = Vec::new();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(map) => {
                if map.get("kind").and_then(Value::as_str) == Some(kind) {
                    found.push(value);
                }
                pending.extend(map.values());
            }
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    found
}

/// Every string `value` holds, at any depth.
fn strings(value: &Value) -> BTreeSet<&str> {
    let mut found = BTreeSet::new();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::String(text) => {
                found.insert(text.as_str());
            }
            Value::Object(map) => pending.extend(map.values()),
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    found
}

/// The strings of a value's wire form.
fn wire_strings<T: serde::Serialize>(value: &T) -> BTreeSet<String> {
    serde_json::to_value(value)
        .map(|value| strings(&value).into_iter().map(str::to_owned).collect())
        .unwrap_or_default()
}
