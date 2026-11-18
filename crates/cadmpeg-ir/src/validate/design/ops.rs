// SPDX-License-Identifier: Apache-2.0
//! Part 10 rules of the feature vocabulary (§6.1 to §6.7): sketches, the
//! shared option types, and the operations of each family.
//!
//! The rules are decided by reference lookup, counting, and arithmetic on
//! stated values. A rule about operands or names does not fire on an
//! `Unresolved` or `Native` operation, a `Native` pick, or an `Unread`
//! selection, and a pick whose geometric kind its target does not state is
//! not refused by a kind rule. A rule that needs a pick resolved in a stored
//! state (a chain's connectivity, an edge's incidence) is not checked here.

mod assembly;
mod body;
mod dressup;
mod options;
mod picks;
mod sheet_metal;
mod sketches;
mod surface;
mod swept;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use serde_json::Value;

use super::records::{DesignRecords, MemberKey};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
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
    /// Whether the operation is the operation of an `AssemblyFeature`.
    in_assembly_feature: bool,
}

/// Check the sketches and every stated operation of `index` against the
/// rules of §6.1 to §6.7.
pub(super) fn check_operations(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let wire = serde_json::to_value(&index.ir.design).unwrap_or(Value::Null);
    sketches::check_sketches(index, &wire, findings);
    options::check_native_keys(&wire, findings);
    for (site, definition) in definitions(index) {
        check_operation(index, site, &definition.operation, findings);
    }
    for feature in &index.ir.design.features {
        check_feature(index, feature, findings);
    }
    body::check_template_inputs(index, findings);
    assembly::check_occurrence_creators(index, findings);
    sheet_metal::check_rules(index, findings);
}

/// Every feature definition the design states, with where it is stated:
/// each feature's own, and each definition override of a configuration or
/// an arrangement.
fn definitions<'a>(index: &DesignRecords<'a>) -> Vec<(Site<'a>, &'a FeatureDefinition)> {
    let design = &index.ir.design;
    let site = |entity: &'a str, feature: &'a str| Site {
        entity,
        feature,
        scope: index.scope_of(MemberKey::Feature(feature)),
        in_assembly_feature: false,
    };
    let mut out: Vec<(Site<'a>, &'a FeatureDefinition)> = design
        .features
        .iter()
        .map(|feature| {
            (
                site(feature.id.as_str(), feature.id.as_str()),
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
                out.push((site(holder, feature.as_str()), definition));
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
    use FeatureOperation as Op;
    match operation {
        Op::Extrude(extrude) => swept::check_extrude(index, site, extrude, findings),
        Op::Revolve(revolve) => swept::check_revolve(index, site, revolve, findings),
        Op::Sweep(sweep) => swept::check_sweep(index, site, sweep, findings),
        Op::HelicalSweep(sweep) => swept::check_helical_sweep(index, site, sweep, findings),
        Op::Rib(rib) => swept::check_rib(index, site, rib, findings),
        Op::Primitive(primitive) => swept::check_primitive(index, site, primitive, findings),
        Op::Loft(loft) => swept::check_loft(index, site, loft, findings),
        Op::Fillet(fillet) => dressup::check_fillet(site, fillet, findings),
        Op::Chamfer(chamfer) => dressup::check_chamfer(site, chamfer, findings),
        Op::Shell(shell) => dressup::check_shell(site, shell, findings),
        Op::Draft(draft) => dressup::check_draft(site, draft, findings),
        Op::Thicken(thicken) => dressup::check_thicken(index, site, thicken, findings),
        Op::Offset(offset) => dressup::check_offset(index, site, offset, findings),
        Op::Hole(hole) => dressup::check_hole(index, site, hole, findings),
        Op::Thread(thread) => dressup::check_thread(index, site, thread, findings),
        Op::Emboss(emboss) => dressup::check_emboss(index, site, emboss, findings),
        Op::Dome(dome) => dressup::check_dome(site, dome, findings),
        Op::Decal(decal) => dressup::check_decal(site, decal, findings),
        Op::Pattern(pattern) => body::check_pattern(index, site, pattern, findings),
        Op::TemplateInstance(instance) => {
            body::check_template_instance(index, site, instance, findings);
        }
        Op::Combine(combine) => body::check_combine(site, combine, findings),
        Op::Split(split) => body::check_split(site, split, findings),
        Op::TransformBodies(transform) => body::check_transform_bodies(site, transform, findings),
        Op::Flex(flex) => body::check_flex(site, flex, findings),
        Op::MoveFaces(faces) => body::check_move_faces(site, faces, findings),
        Op::DeleteFaces(faces) => body::check_delete_faces(site, faces, findings),
        Op::ReplaceFaces(faces) => body::check_replace_faces(site, faces, findings),
        Op::Graft(graft) => body::check_graft(site, graft, findings),
        Op::Patch(patch) => surface::check_patch(index, site, patch, findings),
        Op::Knit(knit) => surface::check_knit(index, site, knit, findings),
        Op::Unsew(unsew) => surface::check_unsew(site, unsew, findings),
        Op::Extend(extend) => surface::check_extend(index, site, extend, findings),
        Op::ImplicitSweep(sweep) => surface::check_implicit_sweep(index, site, sweep, findings),
        Op::BoundaryFill(fill) => surface::check_boundary_fill(index, site, fill, findings),
        Op::Curve(curve) => surface::check_curve(site, curve, findings),
        Op::Form(form) => surface::check_form(site, form, findings),
        Op::SheetMetal(sheet) => sheet_metal::check_sheet_metal(index, site, sheet, findings),
        Op::Mesh(mesh) => surface::check_mesh(index, site, mesh, findings),
        Op::Place(place) => assembly::check_place(index, site, place, findings),
        Op::PlaceSet(set) => assembly::check_place_set(index, site, set, findings),
        Op::Joint(joint) => assembly::check_joint(site, joint, findings),
        Op::RigidGroup(group) => assembly::check_rigid_group(index, site, group, findings),
        Op::ComponentPattern(pattern) => {
            assembly::check_component_pattern(index, site, pattern, findings);
        }
        Op::MirrorComponents(mirror) => assembly::check_mirror(index, site, mirror, findings),
        Op::AssemblyFeature(feature) => {
            assembly::check_assembly_feature(site, feature, findings);
            let nested = Site {
                in_assembly_feature: true,
                ..site
            };
            check_operation(index, nested, &feature.operation, findings);
        }
        Op::Sketch(_)
        | Op::SpatialSketch(_)
        | Op::Spreadsheet(_)
        | Op::Datum(_)
        | Op::DeleteBodies(_)
        | Op::Extract(_)
        | Op::UnsewFaces(_)
        | Op::MaterialRemoval
        | Op::Stored { .. }
        | Op::Unresolved { .. }
        | Op::Native { .. } => {}
    }
}

/// The rules on a feature's own fields that its operation decides.
fn check_feature(index: &DesignRecords<'_>, feature: &Feature, findings: &mut Vec<Finding>) {
    let id = feature.id.as_str();
    match &feature.definition.operation {
        FeatureOperation::Primitive(_) if feature.placement.is_some() => error(
            findings,
            id,
            format_args!("a Primitive feature states a Feature.placement; its placement is Primitive.placement"),
        ),
        FeatureOperation::Place(_) if feature.placement.is_some() => error(
            findings,
            id,
            format_args!("a Place feature states a Feature.placement; its placement is Place.placement"),
        ),
        _ => {}
    }
    assembly::check_component(index, feature, findings);
    assembly::check_driven_by(index, feature, findings);
}

fn error(findings: &mut Vec<Finding>, entity: &str, message: std::fmt::Arguments<'_>) {
    super::error(findings, entity, message);
}

fn warning(findings: &mut Vec<Finding>, entity: &str, message: std::fmt::Arguments<'_>) {
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
