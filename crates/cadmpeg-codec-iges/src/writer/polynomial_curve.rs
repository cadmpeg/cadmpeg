// SPDX-License-Identifier: Apache-2.0
//! Reconstruct a polynomial spline when its coefficients match the current curve.

use super::{CadIr, CurveGeometry, CurveId, Entity, SolvedCurveGeometry};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;
use serde_json::Value;

pub(super) fn retain(ir: &CadIr, id: &CurveId, geometry: &CurveGeometry, entity: &mut Entity) {
    let Some(SolvedCurveGeometry::Nurbs(current)) = geometry.solved() else {
        return;
    };
    let Some(sequence) = ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == id)
        .and_then(|curve| curve.source_object.as_ref())
        .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)
        .and_then(|source| source.object_id.as_str().strip_prefix('D'))
    else {
        return;
    };
    let Some(record) = ir
        .native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("entities"))
        .and_then(|records| {
            records
                .iter()
                .find(|record| record.id() == format!("iges:entity:directory#{sequence}"))
        })
    else {
        return;
    };
    if record.field("entity_type").and_then(|value| value.as_i64()) != Some(112)
        || record.field("form").and_then(|value| value.as_i64()) != Some(0)
        || record.field("transform").and_then(|value| value.as_i64()) != Some(0)
    {
        return;
    }
    let Some(parameters) = record.field("parameters") else {
        return;
    };
    let Some(parameters) = parameters.as_array() else {
        return;
    };
    let Some(values) = parameters
        .iter()
        .map(|token| {
            let value = token.get("value")?;
            match value.get("kind")?.as_str()? {
                "integer" => value
                    .get("value")?
                    .as_i64()
                    .and_then(cadmpeg_core::convert::f64_from_i64),
                "real" => value.get("value")?.as_f64(),
                _ => None,
            }
        })
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let Some(count) = parameters
        .get(4)
        .and_then(|token| token.get("value"))
        .and_then(|value| value.get("value"))
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| *count > 0)
    else {
        return;
    };
    let Some(expected_len) = count
        .checked_mul(12)
        .and_then(|n| n.checked_add(count))
        .and_then(|n| n.checked_add(18))
    else {
        return;
    };
    if values.len() != expected_len {
        return;
    }
    let breaks = &values[5..6 + count];
    if breaks.windows(2).any(|pair| pair[0] >= pair[1]) {
        return;
    }
    let coefficients = &values[6 + count..6 + count + count * 12];
    let mut knots = vec![breaks[0]; 4];
    for &value in &breaks[1..count] {
        knots.extend([value; 3]);
    }
    knots.extend([breaks[count]; 4]);
    let mut points = Vec::new();
    for (segment, values) in coefficients.chunks_exact(12).enumerate() {
        let width = breaks[segment + 1] - breaks[segment];
        let lane = |offset: usize| {
            let [a, b, c, d] = [
                values[offset],
                values[offset + 1],
                values[offset + 2],
                values[offset + 3],
            ];
            [
                a,
                a + b * width / 3.0,
                a + 2.0 * b * width / 3.0 + c * width * width / 3.0,
                a + b * width + c * width * width + d * width * width * width,
            ]
        };
        let (x, y, z) = (lane(0), lane(4), lane(8));
        for index in usize::from(segment != 0)..4 {
            points.push(Point3::new(x[index], y[index], z[index]));
        }
    }
    let Ok(candidate) = NurbsCurve::from_lanes(3, knots, points, None, false) else {
        return;
    };
    if &candidate != current {
        return;
    }
    let mut tokens = Vec::new();
    for token in &parameters[1..] {
        let value = &token["value"];
        let text = match value["kind"].as_str() {
            Some("integer") => value["value"].as_i64().map(|value| value.to_string()),
            Some("real") => value["value"].as_f64().map(|value| {
                let mut text = value.to_string();
                if text.len() > 24 {
                    text = format!("{value:e}");
                }
                if !text.contains(['.', 'e', 'E']) {
                    text.push_str(".0");
                }
                text
            }),
            _ => None,
        };
        let Some(text) = text else {
            return;
        };
        tokens.push(text);
    }
    entity.type_code = 112;
    entity.form = 0;
    entity.parameter_body = format!("{};", tokens.join(",")).into_bytes();
}
