// SPDX-License-Identifier: Apache-2.0
//! Neutral circular patterns written as IGES circular arrays.

use super::{CadIr, Fields, NativeRecord, Value};
use cadmpeg_ir::features::patterns::PatternTransform;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use std::collections::BTreeSet;

pub(super) fn apply_circular(
    ir: &CadIr,
    record: &NativeRecord,
    fields: &mut Fields<'_>,
) -> Result<Option<String>, String> {
    let mut features = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.native_ref.as_deref() == Some(record.id()));
    let Some(feature) = features.next() else {
        return Ok(None);
    };
    if features.next().is_some() {
        return Err("multiple features name one circular array".into());
    }
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
        feature.evaluation.definition()
    else {
        return Err("current feature is not a circular pattern".into());
    };
    if !seeds.is_empty() || fields.value("transformation").is_some() {
        return Err("circular pattern has unsupported seeds or placement".into());
    }
    let flag = fields
        .value("do_dont_flag")
        .and_then(Value::as_i64)
        .filter(|flag| matches!(flag, 0 | 1))
        .ok_or("circular array has no admitted selection flag")?;
    let source_delta = fields
        .value("delta_angle")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or("circular array has no finite angle increment")?;
    let (center, count, delta, positions) = match pattern.definition() {
        PatternTransform::Circular {
            axis_origin,
            axis_dir,
            angle,
            count,
        } => {
            if axis_dir.get() != cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0) {
                return Err("circular axis has no exact IGES placement".into());
            }
            let locations = ir
                .dimension_value(count)
                .filter(|count| *count > 1)
                .ok_or("circular location count is unresolved")?;
            let spacing = angle
                .resolved_spacing(ir, count)
                .ok_or("circular spacing is unresolved")?
                .get();
            let delta = if source_delta.is_sign_negative() {
                -spacing
            } else {
                spacing
            };
            (axis_origin.get(), locations, delta, Vec::new())
        }
        PatternTransform::CircularAngles {
            axis_origin,
            axis_dir,
            angles,
        } => {
            if *axis_dir.as_raw() != cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0) {
                return Err("circular axis has no exact IGES placement".into());
            }
            let locations = fields
                .value("location_count")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|count| *count > 0)
                .ok_or("circular source grid is unavailable")?;
            if source_delta <= 0.0 && locations > 1 {
                return Err("circular angle increment does not order the current angles".into());
            }
            let mut included = BTreeSet::new();
            for angle in angles {
                let angle = ir
                    .dimension_value(angle)
                    .ok_or("circular angle is unresolved")?
                    .get();
                let index = if locations == 1 && angle == 0.0 {
                    0
                } else {
                    let index = (angle / source_delta).round();
                    if !index.is_finite() || index < 0.0 || index >= f64::from(locations) {
                        return Err("circular angle is outside the source grid".into());
                    }
                    let index = cadmpeg_core::convert::truncate_f64_to_u32(index)
                        .ok_or("circular angle is outside the source grid")?;
                    if source_delta * f64::from(index) != angle {
                        return Err("circular angle has no exact IGES grid position".into());
                    }
                    index
                };
                included.insert(index + 1);
            }
            if included.len() != angles.len() {
                return Err("circular angles repeat an IGES grid position".into());
            }
            let count = usize::try_from(locations).map_err(|_| "circular grid count overflows")?;
            let mut positions = Vec::new();
            if included.len() != count {
                let list_count = if flag == 0 {
                    included.len()
                } else {
                    count - included.len()
                };
                if (flag == 0 && list_count > count / 2)
                    || (flag == 1 && list_count < count.div_ceil(2))
                {
                    return Err(
                        "circular selection mask has no admitted IGES representation".into(),
                    );
                }
                positions
                    .try_reserve_exact(list_count)
                    .map_err(|_| "circular mask allocation failed")?;
                if flag == 0 {
                    positions.extend(included.iter().copied().map(Value::from));
                } else {
                    positions.extend(
                        (1..=locations)
                            .filter(|index| !included.contains(index))
                            .map(Value::from),
                    );
                }
            }
            (axis_origin.get(), locations, source_delta, positions)
        }
        _ => return Err("current feature has no IGES circular pattern".into()),
    };
    fields
        .values
        .insert("location_count".into(), Value::from(count));
    fields.values.insert(
        "center".into(),
        serde_json::json!([center.x, center.y, center.z]),
    );
    fields
        .values
        .insert("delta_angle".into(), serde_json::json!(delta));
    fields
        .values
        .insert("positions".into(), Value::Array(positions));
    Ok(Some(feature.id.to_string()))
}
