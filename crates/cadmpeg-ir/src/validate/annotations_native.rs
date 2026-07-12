// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for annotations native.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::subd::SubdSurface;

pub(super) fn check_native_ids(ir: &CadIr, findings: &mut Vec<Finding>) {
    let mut ids = HashSet::new();
    let mut check = |id: &str| {
        if id.is_empty() || !ids.insert(id.to_owned()) {
            findings.push(Finding {
                check: Check::Identity,
                severity: Severity::Error,
                message: "native record id is empty or duplicated".into(),
                entity: Some(id.to_owned()),
            });
        }
    };
    if let Some(native) = &ir.native.f3d {
        macro_rules! check_f3d_arenas {
            ($( $field:ident: $ty:ty; )*) => {
                $(
                    for record in &native.$field {
                        check(&record.id);
                    }
                )*
            };
        }
        crate::native::f3d::f3d_arenas!(check_f3d_arenas);
        // History container ids are covered by the arena sweep above; only the
        // nested per-state records remain.
        for history in &native.asm_histories {
            for state in &history.states {
                check(&state.id);
                for board in &state.bulletin_boards {
                    check(&board.id);
                    for change in &board.changes {
                        check(&change.id);
                    }
                }
                for record in &state.records {
                    check(&record.id);
                }
            }
        }
    }
    if let Some(native) = &ir.native.sldprt {
        for history in &native.feature_histories {
            check(&history.id);
            for configuration in &history.configurations {
                check(&configuration.id);
            }
            for feature in &history.features {
                check(&feature.id);
            }
        }
        for lane in &native.feature_input_lanes {
            check(&lane.id);
            for entity in &lane.sketch_entities {
                check(&entity.id);
            }
        }
    }
}

pub(super) fn check_design_records(ir: &CadIr, findings: &mut Vec<Finding>) {
    let Some(ir) = ir.native.f3d.as_ref() else {
        return;
    };
    let record_indices = ir
        .design_record_headers
        .iter()
        .map(|record| record.record_index)
        .collect::<HashSet<_>>();
    for header in &ir.design_entity_headers {
        if let Some(declared) = header.declared_reference_count {
            if declared as usize != header.reference_indices.len() {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: "sketch container reference count does not match its reference run"
                        .into(),
                    entity: Some(header.entity_id.clone()),
                });
            }
        }
        if header
            .reference_indices
            .iter()
            .any(|index| !record_indices.contains(index))
        {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "sketch container references an absent Design record".into(),
                entity: Some(header.entity_id.clone()),
            });
        }
    }
    let sketch_owners = ir
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(crate::design::DesignObjectKind::Sketch))
        .map(|header| header.entity_suffix as u32)
        .collect::<HashSet<_>>();
    for relation in &ir.sketch_relations {
        const CONSTRAINT_MASK: u32 = 0x3000_3ff7;
        let recognized_count = (relation.state & CONSTRAINT_MASK).count_ones() as usize;
        if !sketch_owners.contains(&relation.owner_reference)
            || relation.raw_bytes.len() != 101
            || relation.unknown_constraint_bits != relation.state & !CONSTRAINT_MASK
            || relation.constraint_kinds.len() != recognized_count
        {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "sketch relation references an absent owner or has an invalid byte frame"
                    .into(),
                entity: Some(relation.record_index.to_string()),
            });
        }
    }
    for point in &ir.sketch_points {
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "sketch point contains a non-finite coordinate".into(),
                entity: Some(point.record_index.to_string()),
            });
        }
    }
    for curve in &ir.sketch_curve_identities {
        let valid = match &curve.geometry {
            None => true,
            Some(crate::design::SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            }) => {
                [start.x, start.y, start.z, end.x, end.y, end.z]
                    .into_iter()
                    .all(f64::is_finite)
                    && (direction.norm() - 1.0).abs() <= 1.0e-9
                    && (normal.norm() - 1.0).abs() <= 1.0e-9
                    && ((end.x - start.x).powi(2)
                        + (end.y - start.y).powi(2)
                        + (end.z - start.z).powi(2))
                    .sqrt()
                        > 0.0
            }
            Some(crate::design::SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            }) => {
                [
                    center.x,
                    center.y,
                    center.z,
                    *radius,
                    *start_angle,
                    *end_angle,
                ]
                .into_iter()
                .all(f64::is_finite)
                    && *radius > 0.0
                    && (normal.norm() - 1.0).abs() <= 1.0e-9
                    && (reference_direction.norm() - 1.0).abs() <= 1.0e-9
            }
            Some(crate::design::SketchCurveGeometry::Nurbs {
                degree,
                fit_tolerance,
                knots,
                weights,
                control_points,
                ..
            }) => {
                fit_tolerance.is_finite()
                    && knots.len() == control_points.len() + *degree as usize + 1
                    && (weights.is_empty() || weights.len() == control_points.len())
                    && knots.windows(2).all(|pair| pair[0] <= pair[1])
                    && weights
                        .iter()
                        .all(|weight| weight.is_finite() && *weight > 0.0)
            }
        };
        if !valid {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "sketch curve contains an invalid exact geometry frame".into(),
                entity: Some(curve.record_index.to_string()),
            });
        }
    }
}

pub(super) fn check_feature_input_lanes(ir: &CadIr, findings: &mut Vec<Finding>) {
    const MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];

    let Some(ir) = ir.native.sldprt.as_ref() else {
        return;
    };
    for lane in &ir.feature_input_lanes {
        for entity in &lane.sketch_entities {
            let Ok(offset) = usize::try_from(entity.offset) else {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "feature-input entity offset exceeds address space".into(),
                    entity: Some(lane.id.clone()),
                });
                continue;
            };
            let marker_matches = offset
                .checked_add(MARKER.len())
                .and_then(|end| lane.native_payload.get(offset..end))
                == Some(MARKER);
            let field_in_bounds = offset
                .checked_add(21)
                .is_some_and(|end| end <= lane.native_payload.len());
            if !marker_matches || !field_in_bounds {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "feature-input entity is outside its native payload".into(),
                    entity: Some(lane.id.clone()),
                });
            }
        }
    }
}

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

macro_rules! define_f3d_entity_json {
    ($( $field:ident: $ty:ty; )*) => {
        fn f3d_entity_json(
            native: &crate::native::f3d::F3dNative,
            id: &str,
        ) -> Option<serde_json::Value> {
            $(
                if let Some(record) = native.$field.iter().find(|record| record.id == id) {
                    return serde_json::to_value(record).ok();
                }
            )*
            None
        }
    };
}
crate::native::f3d::f3d_arenas!(define_f3d_entity_json);

/// Serialize the single entity `id` names. Covers the same id universe as the
/// identity checks: model arenas, unknowns, and native records including
/// nested history and feature entities.
fn entity_json(ir: &CadIr, id: &str) -> Option<serde_json::Value> {
    if let Some(value) = model_entity_json(ir, id) {
        return Some(value);
    }
    if let Some(record) = ir.unknowns.iter().find(|record| record.id.0 == id) {
        return serde_json::to_value(record).ok();
    }
    if let Some(native) = &ir.native.f3d {
        if let Some(value) = f3d_entity_json(native, id) {
            return Some(value);
        }
        for state in native.asm_histories.iter().flat_map(|h| &h.states) {
            if state.id == id {
                return serde_json::to_value(state).ok();
            }
            for board in &state.bulletin_boards {
                if board.id == id {
                    return serde_json::to_value(board).ok();
                }
                if let Some(change) = board.changes.iter().find(|change| change.id == id) {
                    return serde_json::to_value(change).ok();
                }
            }
            if let Some(record) = state.records.iter().find(|record| record.id == id) {
                return serde_json::to_value(record).ok();
            }
        }
    }
    if let Some(native) = &ir.native.sldprt {
        for history in &native.feature_histories {
            if history.id == id {
                return serde_json::to_value(history).ok();
            }
            if let Some(entry) = history.configurations.iter().find(|c| c.id == id) {
                return serde_json::to_value(entry).ok();
            }
            if let Some(entry) = history.features.iter().find(|f| f.id == id) {
                return serde_json::to_value(entry).ok();
            }
        }
        for lane in &native.feature_input_lanes {
            if lane.id == id {
                return serde_json::to_value(lane).ok();
            }
            if let Some(entry) = lane.sketch_entities.iter().find(|e| e.id == id) {
                return serde_json::to_value(entry).ok();
            }
        }
    }
    None
}

pub(super) fn check_annotations(
    ir: &CadIr,
    all_ids: &HashSet<String>,
    findings: &mut Vec<Finding>,
) {
    for (id, provenance) in &ir.annotations.provenance {
        if !all_ids.contains(id) {
            annotation_finding(
                findings,
                Severity::Error,
                id,
                "provenance key does not resolve to an entity",
            );
        }
        if provenance.stream as usize >= ir.annotations.streams.len() {
            annotation_finding(
                findings,
                Severity::Error,
                id,
                "provenance stream index is out of range",
            );
        }
    }
    for (id, note) in &ir.annotations.exactness {
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
    let mut native_ids = Vec::new();
    collect_native_ids(ir, &mut native_ids);
    let native_ids = native_ids
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
    }

    for record in &ir.unknowns {
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
}
