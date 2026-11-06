// SPDX-License-Identifier: Apache-2.0
//! Preserve planar arc coordinates when they describe the current curve.

use super::{CadIr, CurveGeometry, CurveId, CurveSpan, Entity, SolvedCurveGeometry, Vector3};

pub(super) fn retain(
    ir: &CadIr,
    id: &CurveId,
    geometry: &CurveGeometry,
    span: Option<&CurveSpan>,
    entity: &mut Entity,
) {
    let Some(body) = planar_arc(ir, id, geometry, span) else {
        return;
    };
    entity.parameter_body = body.into_bytes();
    entity.transform = None;
}

fn planar_arc(
    ir: &CadIr,
    id: &CurveId,
    geometry: &CurveGeometry,
    span: Option<&CurveSpan>,
) -> Option<String> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = geometry else {
        return None;
    };
    let span = span?;
    let source = ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == id)?
        .source_object
        .as_ref()?;
    if source.format != cadmpeg_ir::CodecFormat::Iges {
        return None;
    }
    let source_id = format!(
        "iges:entity:directory#{}",
        source.object_id.as_str().strip_prefix('D')?
    );
    let record = ir
        .native
        .namespace("iges")?
        .arenas()
        .get("entities")?
        .iter()
        .find(|record| record.id() == source_id)?;
    if record.field("entity_type")?.as_i64()? != 100
        || record.field("form")?.as_i64()? != 0
        || record.field("transform")?.as_i64()? != 0
    {
        return None;
    }
    let parameters = record.field("parameters")?;
    let parameters = parameters.as_array()?;
    if parameters.len() != 8 {
        return None;
    }
    let mut values = Vec::new();
    let mut tokens = Vec::new();
    for parameter in &parameters[1..] {
        let value = &parameter["value"];
        match value.get("kind")?.as_str()? {
            "integer" => {
                let n = value.get("value")?.as_i64()?;
                values.push(cadmpeg_core::convert::f64_from_i64(n)?);
                tokens.push(n.to_string());
            }
            "real" => {
                let n = value.get("value")?.as_f64()?;
                values.push(n);
                let mut token = n.to_string();
                if token.len() > 24 {
                    token = format!("{n:e}");
                }
                if !token.contains(['.', 'e', 'E']) {
                    token.push_str(".0");
                }
                tokens.push(token);
            }
            _ => return None,
        }
    }
    let [z, x, y, sx, sy, ex, ey] = values.as_slice() else {
        return None;
    };
    let center = circle.center().get();
    let has_edge = ir.model.edges.iter().any(|edge| edge.curve() == Some(id));
    let delta = Vector3::new(sx - x, sy - y, 0.0);
    let end_delta = Vector3::new(ex - x, ey - y, 0.0);
    let radius = delta.norm();
    let end_radius = end_delta.norm();
    if center.x != *x
        || center.y != *y
        || center.z != *z
        || (has_edge
            && (span.start.x != *sx
                || span.start.y != *sy
                || span.start.z != *z
                || span.end.x != *ex
                || span.end.y != *ey
                || span.end.z != *z))
        || radius != circle.radius().get()
        || end_radius != radius
        || radius == 0.0
        || end_radius == 0.0
        || *circle.frame().axis().as_raw() != Vector3::new(0.0, 0.0, 1.0)
        || *circle.frame().reference().as_raw() != delta.scale(1.0 / radius)
    {
        return None;
    }
    let reference = delta.scale(1.0 / radius);
    let end_direction = end_delta.scale(1.0 / end_radius);
    let mut angle = reference
        .cross(end_direction)
        .z
        .atan2(reference.dot(end_direction))
        .rem_euclid(std::f64::consts::TAU);
    if angle == 0.0 {
        angle = std::f64::consts::TAU;
    }
    if span.range.get() != [0.0, angle] {
        return None;
    }
    Some(format!("{};", tokens.join(",")))
}
