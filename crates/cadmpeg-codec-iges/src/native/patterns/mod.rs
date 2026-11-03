// SPDX-License-Identifier: Apache-2.0
//! Neutral typed patterns from admitted IGES rectangular and circular arrays.
//!
//! The array base entity has no neutral body form, so every pattern carries
//! empty seeds and records the repetition rule. The rectangular grid origin,
//! the circular start angle and the circular radius stay in the native array
//! record, which the repetition rule alone cannot locate.

use super::{NativeCircularArray, NativeRectangularArray};
use crate::directory::DirectoryEntry;
use crate::entities::geometry::resolve_transform;
use crate::global::RealPrecision;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn directory_label(entry: &DirectoryEntry) -> Option<String> {
    std::str::from_utf8(&entry.label)
        .ok()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn scale_length(value: f64, factor: f64) -> Option<cadmpeg_ir::scalar::Length> {
    cadmpeg_ir::scalar::Length::new(value * factor)
}

fn entry_rotation(
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: Option<f64>,
    precision: RealPrecision,
    sequence: u32,
    native_id: &str,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Transform>, CodecError> {
    let Some(entry) = entries.get(&sequence) else {
        return Ok(None);
    };
    if entry.transform == 0 {
        return Ok(None);
    }
    let Some(length_factor) = length_factor else {
        return Ok(None);
    };
    match resolve_transform(
        entry.transform,
        entries,
        records,
        length_factor,
        precision,
        &mut BTreeSet::new(),
        ctx,
    ) {
        Ok(transform) => Ok(Some(transform)),
        Err(_) => {
            losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
                "{native_id}: array placement pointer is not a resolvable transformation"
            )));
            Ok(None)
        }
    }
}

fn push_feature(
    ir: &mut CadIr,
    sequence: u32,
    label: Option<String>,
    native_ref: String,
    definition: cadmpeg_ir::features::FeatureDefinition,
    ordinal: &mut u64,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    ctx.charge_entities(1, "iges_array_features")?;
    ir.model.features.push(cadmpeg_ir::features::Feature {
        product_definition: None,
        id: crate::ids::feature(&crate::ids::Stem::directory(sequence)),
        ordinal: *ordinal,
        name: label,
        suppressed: None,
        needs_recompute: false,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref),
    });
    *ordinal = ordinal
        .checked_add(1)
        .ok_or_else(|| CodecError::malformed("IGES array feature ordinal overflows"))?;
    Ok(())
}

fn rectangular_positions(
    columns: u32,
    rows: u32,
    flag: Option<i64>,
    positions: &[Option<i64>],
) -> Option<Vec<[u32; 2]>> {
    let total = (columns as u64).checked_mul(rows as u64)?;
    let mut listed = BTreeSet::new();
    for position in positions.iter().flatten() {
        let position = u32::try_from(*position).ok()?;
        if position == 0 || u64::from(position) > total {
            continue;
        }
        listed.insert(position);
    }
    if positions.iter().any(Option::is_none) && matches!(flag, Some(1 | 2)) {
        return None;
    }
    let included: BTreeSet<u32> = if positions.is_empty() {
        (1..=(total as u32)).collect()
    } else {
        match flag {
            Some(0) => listed,
            Some(1) => (1..=(total as u32))
                .filter(|n| !listed.contains(n))
                .collect(),
            _ => (1..=(total as u32)).collect(),
        }
    };
    let mut omitted = Vec::new();
    for column in 0..columns {
        for row in 0..rows {
            let position = column * rows + row + 1;
            if !included.contains(&position) {
                omitted.push([column, row]);
            }
        }
    }
    Some(omitted)
}

fn project_rectangular(
    ir: &mut CadIr,
    record: &NativeRectangularArray,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: Option<f64>,
    precision: RealPrecision,
    ordinal: &mut u64,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
    admitted: &BTreeSet<u32>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::patterns::{
        LinearPatternDirection, PatternDistance, PatternKind, PatternSeed, PatternTransform,
    };
    use cadmpeg_ir::features::{Dimension, FeatureDefinition, FeatureDirection3, FeatureOperation};

    let sequence = super::products::source_sequence(&record.source_entity)?;
    if !admitted.contains(&sequence) {
        return Ok(());
    }
    let unavailable = |losses: &mut Vec<LossNote>| {
        losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
            "{}: rectangular array dimensions are invalid",
            record.id
        )));
    };
    let (Some(columns), Some(rows)) = (
        record.columns.and_then(|v| u32::try_from(v).ok()),
        record.rows.and_then(|v| u32::try_from(v).ok()),
    ) else {
        unavailable(losses);
        return Ok(());
    };
    if columns == 0 || rows == 0 {
        unavailable(losses);
        return Ok(());
    }
    let Some(length_factor) = length_factor else {
        unavailable(losses);
        return Ok(());
    };
    let (Some(column_spacing), Some(row_spacing)) = (record.column_spacing, record.row_spacing)
    else {
        unavailable(losses);
        return Ok(());
    };
    let (Some(column_mm), Some(row_mm)) = (
        scale_length(column_spacing, length_factor),
        scale_length(row_spacing, length_factor),
    ) else {
        unavailable(losses);
        return Ok(());
    };
    let rotation = record.rotation.unwrap_or(0.0);
    if !rotation.is_finite() {
        unavailable(losses);
        return Ok(());
    }
    let placement = entry_rotation(
        entries,
        records,
        Some(length_factor),
        precision,
        sequence,
        &record.id,
        losses,
        ctx,
    )?;
    let rotate = |x: f64, y: f64| {
        let (sin, cos) = rotation.sin_cos();
        let vector = cadmpeg_ir::math::Vector3::new(x * cos - y * sin, x * sin + y * cos, 0.0);
        match placement {
            Some(placement) => placement.apply_vector(vector).and_then(|direction| {
                cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(direction.get())
            }),
            None => cadmpeg_ir::units::UnitVector3::new(vector),
        }
    };
    let (Some(first), Some(second)) = (rotate(1.0, 0.0), rotate(0.0, 1.0)) else {
        unavailable(losses);
        return Ok(());
    };
    let Some(omitted) =
        rectangular_positions(columns, rows, record.do_dont_flag, &record.positions)
    else {
        unavailable(losses);
        return Ok(());
    };
    let transform = PatternTransform::Linear {
        direction: Some(FeatureDirection3::from(first)),
        distance: PatternDistance::Spacing {
            distance: Dimension::Literal(column_mm),
        },
        count: Dimension::Literal(columns),
        second: Some(LinearPatternDirection {
            direction: FeatureDirection3::from(second),
            distance: PatternDistance::Spacing {
                distance: Dimension::Literal(row_mm),
            },
            count: Dimension::Literal(rows),
        }),
        omitted_instances: omitted,
    };
    let pattern = match PatternKind::new(transform) {
        Ok(pattern) => pattern,
        Err(reason) => {
            losses.push(
                IgesLossCode::NeutralProjectionUnavailable.note(format!("{}: {reason}", record.id)),
            );
            return Ok(());
        }
    };
    push_feature(
        ir,
        sequence,
        entries
            .get(&sequence)
            .and_then(|entry| directory_label(entry)),
        record.id.clone(),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: Vec::<PatternSeed>::new(),
            pattern,
        }),
        ordinal,
        ctx,
    )
}

fn project_circular(
    ir: &mut CadIr,
    record: &NativeCircularArray,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: Option<f64>,
    precision: RealPrecision,
    ordinal: &mut u64,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
    admitted: &BTreeSet<u32>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::patterns::{
        CircularPatternDistribution, PatternAngle, PatternKind, PatternSeed, PatternTransform,
    };
    use cadmpeg_ir::features::{Dimension, FeatureDefinition, FeatureDirection3, FeatureOperation};

    let sequence = super::products::source_sequence(&record.source_entity)?;
    if !admitted.contains(&sequence) {
        return Ok(());
    }
    let unavailable = |losses: &mut Vec<LossNote>| {
        losses.push(IgesLossCode::NeutralProjectionUnavailable.note(format!(
            "{}: circular array dimensions are invalid",
            record.id
        )));
    };
    let Some(count) = record.location_count.and_then(|v| u32::try_from(v).ok()) else {
        unavailable(losses);
        return Ok(());
    };
    if count == 0 {
        unavailable(losses);
        return Ok(());
    }
    let Some(length_factor) = length_factor else {
        unavailable(losses);
        return Ok(());
    };
    let center = match record.center {
        [Some(x), Some(y), Some(z)] => [x * length_factor, y * length_factor, z * length_factor],
        _ => {
            unavailable(losses);
            return Ok(());
        }
    };
    let Some(center) = cadmpeg_ir::scalar::FiniteReal::new(center[0])
        .zip(cadmpeg_ir::scalar::FiniteReal::new(center[1]))
        .zip(cadmpeg_ir::scalar::FiniteReal::new(center[2]))
        .map(|((x, y), z)| cadmpeg_ir::features::FinitePoint3::from_coordinates(x, y, z))
    else {
        unavailable(losses);
        return Ok(());
    };
    let (Some(start), Some(delta)) = (record.start_angle, record.delta_angle) else {
        unavailable(losses);
        return Ok(());
    };
    if !start.is_finite() || !delta.is_finite() {
        unavailable(losses);
        return Ok(());
    }
    // discarded-value: the circular start angle stays in the native array record only.
    let _ = start;
    let placement = entry_rotation(
        entries,
        records,
        Some(length_factor),
        precision,
        sequence,
        &record.id,
        losses,
        ctx,
    )?;
    let mut axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    if let Some(placement) = placement {
        let Some(rotated) = placement.apply_vector(axis).and_then(|direction| {
            cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(direction.get())
                .map(|unit| *unit.as_raw())
        }) else {
            unavailable(losses);
            return Ok(());
        };
        axis = rotated;
    }
    let Some(axis) = cadmpeg_ir::units::UnitVector3::new(axis) else {
        unavailable(losses);
        return Ok(());
    };
    let total = u64::from(count);
    let listed: BTreeSet<u32> = record
        .positions
        .iter()
        .flatten()
        .filter_map(|position| u32::try_from(*position).ok())
        .filter(|position| *position >= 1 && u64::from(*position) <= total)
        .collect();
    if record.positions.iter().any(Option::is_none) && matches!(record.do_dont_flag, Some(1 | 2)) {
        unavailable(losses);
        return Ok(());
    }
    let included: BTreeSet<u32> = if record.positions.is_empty() {
        (1..=count).collect()
    } else {
        match record.do_dont_flag {
            Some(0) => listed,
            Some(1) => (1..=count).filter(|n| !listed.contains(n)).collect(),
            _ => (1..=count).collect(),
        }
    };
    let transform = if included.len() as u64 == total {
        if count == 1 {
            let Some(zero) = cadmpeg_ir::scalar::Angle::new(0.0) else {
                unavailable(losses);
                return Ok(());
            };
            PatternTransform::CircularAngles {
                axis_origin: center,
                axis_dir: axis,
                angles: vec![cadmpeg_ir::features::Dimension::Literal(zero)],
            }
        } else {
            let Some(span) =
                cadmpeg_ir::scalar::FiniteReal::new((delta * f64::from(count - 1)).abs())
                    .and_then(cadmpeg_ir::scalar::PositiveAngle::from_assigned_real)
            else {
                unavailable(losses);
                return Ok(());
            };
            PatternTransform::Circular {
                axis_origin: center,
                axis_dir: FeatureDirection3::from(axis),
                angle: PatternAngle::Span {
                    angle: Dimension::Literal(span),
                    distribution: CircularPatternDistribution::Inclusive,
                },
                count: Dimension::Literal(count),
            }
        }
    } else {
        let mut angles = Vec::new();
        for position in included {
            let Some(angle) = cadmpeg_ir::scalar::Angle::new(delta * f64::from(position - 1))
            else {
                unavailable(losses);
                return Ok(());
            };
            angles.push(Dimension::Literal(angle));
        }
        PatternTransform::CircularAngles {
            axis_origin: center,
            axis_dir: axis,
            angles,
        }
    };
    let pattern = match PatternKind::new(transform) {
        Ok(pattern) => pattern,
        Err(reason) => {
            losses.push(
                IgesLossCode::NeutralProjectionUnavailable.note(format!("{}: {reason}", record.id)),
            );
            return Ok(());
        }
    };
    push_feature(
        ir,
        sequence,
        entries
            .get(&sequence)
            .and_then(|entry| directory_label(entry)),
        record.id.clone(),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: Vec::<PatternSeed>::new(),
            pattern,
        }),
        ordinal,
        ctx,
    )
}

pub(super) fn project(
    ir: &mut CadIr,
    rectangular: &[NativeRectangularArray],
    circular: &[NativeCircularArray],
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    length_factor: Option<f64>,
    precision: RealPrecision,
    ordinal: &mut u64,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
    admitted: &BTreeSet<u32>,
) -> Result<(), CodecError> {
    for record in rectangular {
        project_rectangular(
            ir,
            record,
            entries,
            records,
            length_factor,
            precision,
            ordinal,
            losses,
            ctx,
            admitted,
        )?;
    }
    for record in circular {
        project_circular(
            ir,
            record,
            entries,
            records,
            length_factor,
            precision,
            ordinal,
            losses,
            ctx,
            admitted,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
