// SPDX-License-Identifier: Apache-2.0
//! Constructive-solid primitive validation and native semantic ownership.

use super::geometry::{entity_loss, resolve_transform, Projection};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::{ParameterRecord, TokenValue};
use cadmpeg_ir::ids::CurveId;
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

fn pointer(record: &ParameterRecord, index: usize) -> Option<u32> {
    record.integer(index).and_then(|value| {
        let sequence = u32::try_from(value).ok()?;
        (sequence % 2 == 1).then_some(sequence)
    })
}

fn profile_closed(ir: &CadIr, sequence: u32, tolerance: f64) -> Option<bool> {
    let curve = CurveId(format!("iges:model:curve#D{sequence}"));
    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(&curve))?;
    let point = |vertex: &cadmpeg_ir::ids::VertexId| {
        let point_id = &ir
            .model
            .vertices
            .iter()
            .find(|item| item.id == *vertex)?
            .point;
        ir.model
            .points
            .iter()
            .find(|item| item.id == *point_id)
            .map(|item| item.position)
    };
    let start = point(&edge.start)?;
    let end = point(&edge.end)?;
    Some(super::evaluation::distance(start, end) <= tolerance)
}

#[derive(Clone, Copy)]
enum BooleanTerm {
    Operand(u32),
    Operation,
}

pub(super) fn project(
    ir: &mut CadIr,
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
            _ => None,
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
            _ => false,
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
            _ => {
                losses.push(entity_loss(entry, "primitive solid type is unsupported"));
                continue;
            }
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

    for entry in directory.iter().filter(|entry| {
        (entry.entity_type == 162 && matches!(entry.form, 0 | 1))
            || (entry.entity_type == 164 && entry.form == 0)
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
        let Some(profile) = pointer(record, 1).filter(|sequence| {
            ir.model
                .curves
                .iter()
                .any(|curve| curve.id == CurveId(format!("iges:model:curve#D{sequence}")))
        }) else {
            losses.push(entity_loss(entry, "solid profile curve pointer is invalid"));
            continue;
        };
        let Some(amount) =
            number_or(record, 2, 1.0).filter(|value| value.is_finite() && *value > 0.0)
        else {
            losses.push(entity_loss(entry, "solid sweep amount is invalid"));
            continue;
        };
        if entry.entity_type == 162 && amount > 1.0 {
            losses.push(entity_loss(
                entry,
                "solid revolution fraction is greater than one",
            ));
            continue;
        }
        let (origin, direction_start) = if entry.entity_type == 162 {
            (vector_or(record, 3, Vector3::new(0.0, 0.0, 0.0)), 6)
        } else {
            (Some(Vector3::new(0.0, 0.0, 0.0)), 3)
        };
        let direction = vector_or(record, direction_start, Vector3::new(0.0, 0.0, 1.0));
        if origin.is_none_or(|origin| {
            !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite()
        }) || direction.is_none_or(|direction| !unit(direction))
        {
            losses.push(entity_loss(entry, "solid sweep axis is invalid"));
            continue;
        }
        let Some(closed) = global
            .minimum_resolution_mm()
            .and_then(|tolerance| profile_closed(ir, profile, tolerance))
        else {
            losses.push(entity_loss(
                entry,
                "solid profile endpoints are unavailable",
            ));
            continue;
        };
        if (entry.entity_type == 162 && entry.form == 0 && closed)
            || (entry.entity_type == 164 && !closed)
        {
            losses.push(entity_loss(
                entry,
                "solid sweep form disagrees with profile closure",
            ));
            continue;
        }
        if resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            &mut BTreeSet::new(),
        )
        .is_err()
        {
            losses.push(entity_loss(entry, "solid sweep placement is invalid"));
            continue;
        }
        decoded.insert(entry.sequence);
    }

    let mut boolean_definitions = BTreeMap::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 180 && matches!(entry.form, 0 | 1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 2) else {
            losses.push(entity_loss(
                entry,
                "Boolean postfix length is not greater than two",
            ));
            continue;
        };
        let terms = (0..count)
            .map(|index| {
                let value = record.integer(2 + index)?;
                if value < 0 {
                    let sequence = u32::try_from(value.checked_neg()?).ok()?;
                    (sequence % 2 == 1).then_some(BooleanTerm::Operand(sequence))
                } else if matches!(value, 1..=3) {
                    Some(BooleanTerm::Operation)
                } else {
                    None
                }
            })
            .collect::<Option<Vec<_>>>();
        let Some(terms) = terms else {
            losses.push(entity_loss(entry, "Boolean postfix term is invalid"));
            continue;
        };
        let mut depth = 0_usize;
        let valid_stack = terms.iter().all(|term| match term {
            BooleanTerm::Operand(_) => {
                depth += 1;
                true
            }
            BooleanTerm::Operation if depth >= 2 => {
                depth -= 1;
                true
            }
            BooleanTerm::Operation => false,
        });
        if !valid_stack || depth != 1 {
            losses.push(entity_loss(entry, "Boolean postfix stack is unbalanced"));
            continue;
        }
        boolean_definitions.insert(entry.sequence, terms);
    }
    let mut visited = BTreeSet::new();
    for (sequence, terms) in &boolean_definitions {
        let entry = entries[sequence];
        let operands_valid = terms.iter().all(|term| match term {
            BooleanTerm::Operation => true,
            BooleanTerm::Operand(target) => entries.get(target).is_some_and(|target_entry| {
                matches!(
                    target_entry.entity_type,
                    150 | 152 | 154 | 156 | 158 | 160 | 162 | 164 | 168 | 180 | 430
                ) || (entry.form == 1 && target_entry.entity_type == 186)
            }),
        });
        let has_brep = terms.iter().any(|term| match term {
            BooleanTerm::Operand(target) => entries
                .get(target)
                .is_some_and(|target_entry| target_entry.entity_type == 186),
            BooleanTerm::Operation => false,
        });
        let cyclic = super::directed_cycle(*sequence, &mut visited, |sequence| {
            boolean_definitions
                .get(&sequence)
                .into_iter()
                .flatten()
                .filter_map(|term| match term {
                    BooleanTerm::Operand(target) if boolean_definitions.contains_key(target) => {
                        Some(*target)
                    }
                    BooleanTerm::Operand(_) | BooleanTerm::Operation => None,
                })
                .collect()
        });
        if !operands_valid || (entry.form == 1) != has_brep || cyclic {
            losses.push(entity_loss(
                entry,
                "Boolean operands, form, or reference acyclicity is invalid",
            ));
            continue;
        }
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
            losses.push(entity_loss(entry, "Boolean result placement is invalid"));
            continue;
        }
        decoded.insert(*sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 182 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(tree) = pointer(record, 1).filter(|sequence| {
            decoded.contains(sequence)
                && entries
                    .get(sequence)
                    .is_some_and(|target| target.entity_type == 180)
        }) else {
            losses.push(entity_loss(
                entry,
                "selected-component Boolean tree pointer is invalid",
            ));
            continue;
        };
        let point = (2..=4)
            .map(|index| record.number(index).filter(|value| value.is_finite()))
            .collect::<Option<Vec<_>>>();
        if point.is_none() || entry.status.use_flag != 3 {
            losses.push(entity_loss(
                entry,
                "selected-component point or entity-use flag is invalid",
            ));
            continue;
        }
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
            losses.push(entity_loss(
                entry,
                "selected-component placement is invalid",
            ));
            continue;
        }
        let _ = tree;
        decoded.insert(entry.sequence);
    }

    Projection {
        handled,
        decoded,
        losses,
    }
}
