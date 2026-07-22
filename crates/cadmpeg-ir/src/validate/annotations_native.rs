// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for annotations native.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::drawings::Drawing;
use crate::features::{DesignConfiguration, DesignParameter, FeatureInputTopology};
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Component, Occurrence};
use crate::semantic_annotations::SemanticAnnotation;
use crate::sketches::{
    Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchConstraint,
    SpatialSketchEntity,
};
use crate::spreadsheets::Spreadsheet;
use crate::subd::SubdSurface;

macro_rules! define_model_entity_json {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] => $key:expr; )*) => {
        fn model_entity_json(ir: &CadIr, id: &str) -> Option<serde_json::Value> {
            $(
                let key: fn(&$element) -> String = $key;
                if let Some(entity) = ir.model.$field.iter().find(|entity| key(entity) == id) {
                    return serde_json::to_value(entity).ok();
                }
            )*
            None
        }
    };
}
crate::document::arena_registry!(define_model_entity_json);

/// Serialize the single entity `id` names. Covers the same id universe as the
/// identity checks: model arenas, unknowns, and native records including
/// nested history and feature entities.
fn entity_json(ir: &CadIr, id: &str) -> Option<serde_json::Value> {
    if let Some(value) = model_entity_json(ir, id) {
        return Some(value);
    }
    ir.native
        .0
        .values()
        .flat_map(|namespace| namespace.arenas.values())
        .flatten()
        .find(|record| record.id == id)
        .and_then(|record| serde_json::to_value(record).ok())
}

pub(super) fn check_annotations(
    ir: &CadIr,
    annotations: &crate::Annotations,
    all_ids: &HashSet<String>,
    findings: &mut Vec<Finding>,
) {
    for (id, provenance) in &annotations.provenance {
        if !all_ids.contains(id) {
            annotation_finding(
                findings,
                Severity::Error,
                id,
                "provenance key does not resolve to an entity",
            );
        }
        if provenance.stream as usize >= annotations.streams.len() {
            annotation_finding(
                findings,
                Severity::Error,
                id,
                "provenance stream index is out of range",
            );
        }
    }
    for (id, note) in &annotations.exactness {
        if !all_ids.contains(id) {
            annotation_finding(
                findings,
                Severity::Error,
                id,
                "exactness key does not resolve to an entity",
            );
            continue;
        }
        if note.fields.is_empty() {
            continue;
        }
        let Some(entity) = entity_json(ir, id) else {
            annotation_finding(
                findings,
                Severity::Warning,
                id,
                "entity could not be serialized to validate its exactness field paths",
            );
            continue;
        };
        for path in note.fields.keys() {
            if path.is_empty() || !field_path_resolves(&entity, path) {
                annotation_finding(
                    findings,
                    Severity::Warning,
                    id,
                    &format!("exactness field path `{path}` does not resolve"),
                );
            }
        }
    }
}

fn annotation_finding(findings: &mut Vec<Finding>, severity: Severity, id: &str, message: &str) {
    findings.push(Finding {
        check: Check::Annotations,
        severity,
        message: message.into(),
        entity: Some(id.into()),
    });
}

fn field_path_resolves(mut value: &serde_json::Value, path: &str) -> bool {
    for component in path.split('.') {
        match value {
            serde_json::Value::Object(object) => {
                let Some(next) = object.get(component) else {
                    return false;
                };
                value = next;
            }
            serde_json::Value::Array(array) => {
                let Ok(index) = component.parse::<usize>() else {
                    return false;
                };
                let Some(next) = array.get(index) else {
                    return false;
                };
                value = next;
            }
            _ => return false,
        }
    }
    true
}

pub(super) fn check_native_links(
    ir: &CadIr,
    all_ids: &HashSet<String>,
    findings: &mut Vec<Finding>,
) {
    let native_ids = collect_native_ids(ir)
        .into_iter()
        .map(|(_, id)| id)
        .collect::<HashSet<_>>();
    for feature in &ir.model.features {
        if let Some(target) = &feature.native_ref {
            if !native_ids.contains(target.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("native_ref `{target}` does not resolve"),
                    entity: Some(feature.id.0.clone()),
                });
            }
        }
        if let crate::features::FeatureDefinition::HelixNativeAxis {
            axis_native_ref: target,
            ..
        } = &feature.definition
        {
            if !native_ids.contains(target.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("helix axis native_ref `{target}` does not resolve"),
                    entity: Some(feature.id.0.clone()),
                });
            }
        }
    }
    for parameter in &ir.model.parameters {
        if let Some(target) = &parameter.native_ref {
            if !native_ids.contains(target.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("native_ref `{target}` does not resolve"),
                    entity: Some(parameter.id.0.clone()),
                });
            }
        }
        if let Some(semantic) = &parameter.pmi {
            if !native_ids.contains(semantic.native_ref.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("PMI native_ref `{}` does not resolve", semantic.native_ref),
                    entity: Some(parameter.id.0.clone()),
                });
            }
        }
    }
    for configuration in &ir.model.configurations {
        if let Some(target) = &configuration.native_ref {
            if !native_ids.contains(target.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("native_ref `{target}` does not resolve"),
                    entity: Some(configuration.id.0.clone()),
                });
            }
        }
    }
    for sketch in &ir.model.sketches {
        if let Some(target) = &sketch.native_ref {
            if !native_ids.contains(target.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("native_ref `{target}` does not resolve"),
                    entity: Some(sketch.id.0.clone()),
                });
            }
        }
    }
    for sketch in &ir.model.spatial_sketches {
        if let Some(target) = &sketch.native_ref {
            if !native_ids.contains(target.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("native_ref `{target}` does not resolve"),
                    entity: Some(sketch.id.0.clone()),
                });
            }
        }
    }
    for constraint in &ir.model.sketch_constraints {
        if let Some(target) = &constraint.native_ref {
            if !native_ids.contains(target.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("native_ref `{target}` does not resolve"),
                    entity: Some(constraint.id.0.clone()),
                });
            }
        }
        if let crate::sketches::SketchConstraintDefinition::Native { operands, .. } =
            &constraint.definition
        {
            for operand in operands {
                if let Some(target) = &operand.native_ref {
                    if !native_ids.contains(target.as_str()) {
                        findings.push(Finding {
                            check: Check::NativeLinks,
                            severity: Severity::Error,
                            message: format!("operand native_ref `{target}` does not resolve"),
                            entity: Some(constraint.id.0.clone()),
                        });
                    }
                }
            }
        }
    }
    for constraint in &ir.model.spatial_sketch_constraints {
        if let Some(target) = &constraint.native_ref {
            if !native_ids.contains(target.as_str()) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("native_ref `{target}` does not resolve"),
                    entity: Some(constraint.id.0.clone()),
                });
            }
        }
        if let crate::sketches::SpatialSketchConstraintDefinition::Native { operands, .. } =
            &constraint.definition
        {
            for operand in operands {
                if let Some(target) = &operand.native_ref {
                    if !native_ids.contains(target.as_str()) {
                        findings.push(Finding {
                            check: Check::NativeLinks,
                            severity: Severity::Error,
                            message: format!("operand native_ref `{target}` does not resolve"),
                            entity: Some(constraint.id.0.clone()),
                        });
                    }
                }
            }
        }
    }

    let native_unknowns = ir.all_native_unknowns().unwrap_or_default();
    for record in &native_unknowns {
        for target in &record.links {
            if !all_ids.contains(target) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("unknown-record link `{target}` does not resolve"),
                    entity: Some(record.id.0.clone()),
                });
            }
        }
    }
    for namespace in ir.native.0.values() {
        for records in namespace.arenas.values() {
            for record in records {
                let Some(serde_json::Value::Array(links)) = record.fields.get("links") else {
                    continue;
                };
                for target in links.iter().filter_map(serde_json::Value::as_str) {
                    if !all_ids.contains(target) {
                        findings.push(Finding {
                            check: Check::NativeLinks,
                            severity: Severity::Error,
                            message: format!("native-record link `{target}` does not resolve"),
                            entity: Some(record.id.clone()),
                        });
                    }
                }
            }
        }
    }
}
