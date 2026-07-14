// SPDX-License-Identifier: Apache-2.0
//! Constructive-solid primitive validation and native semantic ownership.

use super::geometry::{entity_loss, resolve_transform, Projection};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::{ParameterRecord, TokenValue};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn number_or(record: &ParameterRecord, index: usize, default: f64) -> Option<f64> {
    match record.tokens.get(index).map(|token| &token.value) {
        None | Some(TokenValue::Omitted) => Some(default),
        Some(TokenValue::Integer(_) | TokenValue::Real(_)) => record.number(index),
        Some(TokenValue::String(_)) => None,
    }
}

fn vector_or(record: &ParameterRecord, start: usize, default: Vector3) -> Option<Vector3> {
    Some(Vector3::new(
        number_or(record, start, default.x)?,
        number_or(record, start + 1, default.y)?,
        number_or(record, start + 2, default.z)?,
    ))
}

fn unit(vector: Vector3) -> bool {
    vector.norm().is_finite() && (vector.norm() - 1.0).abs() <= 1.0e-10
}

fn orthogonal(left: Vector3, right: Vector3) -> bool {
    (left.x * right.x + left.y * right.y + left.z * right.z).abs() <= 1.0e-10
}

pub(super) fn project(
    _ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> Projection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory.iter().filter(|entry| {
        matches!(entry.entity_type, 150 | 152 | 154 | 156 | 158 | 160 | 168) && entry.form == 0
    }) {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        if resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            &mut BTreeSet::new(),
        )
        .is_err()
        {
            losses.push(entity_loss(entry, "primitive placement is invalid"));
            continue;
        }
        let dimensions = match entry.entity_type {
            150 | 168 => (1..=3)
                .map(|index| record.number(index))
                .collect::<Option<Vec<_>>>(),
            152 => (1..=4)
                .map(|index| record.number(index))
                .collect::<Option<Vec<_>>>(),
            154 => (1..=2)
                .map(|index| record.number(index))
                .collect::<Option<Vec<_>>>(),
            156 => [
                record.number(1),
                record.number(2),
                number_or(record, 3, 0.0),
            ]
            .into_iter()
            .collect::<Option<Vec<_>>>(),
            158 => record.number(1).map(|value| vec![value]),
            160 => (1..=2)
                .map(|index| record.number(index))
                .collect::<Option<Vec<_>>>(),
            _ => unreachable!("filtered primitive type"),
        };
        let Some(dimensions) = dimensions else {
            losses.push(entity_loss(entry, "primitive dimensions are not numeric"));
            continue;
        };
        let dimensions_valid = match entry.entity_type {
            150 => dimensions
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
            152 => {
                dimensions[..3]
                    .iter()
                    .all(|value| value.is_finite() && *value > 0.0)
                    && dimensions[3].is_finite()
                    && dimensions[3] >= 0.0
                    && dimensions[3] < dimensions[0]
            }
            154 | 158 => dimensions
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
            156 => {
                dimensions[0] > 0.0
                    && dimensions[1] > dimensions[2]
                    && dimensions[2] >= 0.0
                    && dimensions.iter().all(|value| value.is_finite())
            }
            160 => {
                dimensions[0] > dimensions[1]
                    && dimensions[1] > 0.0
                    && dimensions.iter().all(|value| value.is_finite())
            }
            168 => {
                dimensions[0] >= dimensions[1]
                    && dimensions[1] >= dimensions[2]
                    && dimensions[2] > 0.0
                    && dimensions.iter().all(|value| value.is_finite())
            }
            _ => unreachable!("filtered primitive type"),
        };
        if !dimensions_valid {
            losses.push(entity_loss(
                entry,
                "primitive dimension invariant is violated",
            ));
            continue;
        }
        let (origin_start, x_axis_start, z_axis_start) = match entry.entity_type {
            150 => (4, Some(7), Some(10)),
            152 => (5, Some(8), Some(11)),
            154 => (3, None, Some(6)),
            156 => (4, None, Some(7)),
            158 => (2, None, None),
            160 => (3, None, Some(6)),
            168 => (4, Some(7), Some(10)),
            _ => unreachable!("filtered primitive type"),
        };
        let Some(origin) = vector_or(record, origin_start, Vector3::new(0.0, 0.0, 0.0)) else {
            losses.push(entity_loss(entry, "primitive origin is invalid"));
            continue;
        };
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
            losses.push(entity_loss(entry, "primitive origin is non-finite"));
            continue;
        }
        let x_axis =
            x_axis_start.and_then(|start| vector_or(record, start, Vector3::new(1.0, 0.0, 0.0)));
        let z_axis =
            z_axis_start.and_then(|start| vector_or(record, start, Vector3::new(0.0, 0.0, 1.0)));
        if x_axis_start.is_some() != x_axis.is_some()
            || z_axis_start.is_some() != z_axis.is_some()
            || x_axis.is_some_and(|axis| !unit(axis))
            || z_axis.is_some_and(|axis| !unit(axis))
            || x_axis
                .zip(z_axis)
                .is_some_and(|(x_axis, z_axis)| !orthogonal(x_axis, z_axis))
        {
            losses.push(entity_loss(entry, "primitive axes are not orthonormal"));
            continue;
        }
        decoded.insert(entry.sequence);
    }

    Projection {
        handled,
        decoded,
        losses,
    }
}
