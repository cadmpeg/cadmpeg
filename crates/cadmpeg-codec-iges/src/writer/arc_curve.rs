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
    let Some(body) = planar_arc(ir, id, geometry, span)
        .or_else(|| planar_ellipse(ir, id, geometry, span, entity.form))
        .or_else(|| planar_parabola(ir, id, geometry, span, entity.form))
    else {
        return;
    };
    entity.parameter_body = body.into_bytes();
    entity.transform = None;
}

fn planar_parabola(
    ir: &CadIr,
    id: &CurveId,
    geometry: &CurveGeometry,
    span: Option<&CurveSpan>,
    form: i64,
) -> Option<String> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola)) = geometry else {
        return None;
    };
    let span = span?;
    let (values, tokens) = source_parameters(ir, id, 104, form, 12)?;
    let [coeff_a, coeff_b, coeff_c, coeff_d, coeff_e, coeff_f, plane_z, start_x, start_y, end_x, end_y] =
        values.as_slice()
    else {
        return None;
    };
    if *coeff_a == 0.0
        || *coeff_e == 0.0
        || *coeff_b != 0.0
        || *coeff_c != 0.0
        || *coeff_d != 0.0
        || *coeff_f != 0.0
    {
        return None;
    }
    let focal =
        cadmpeg_ir::math::product_quotient([*coeff_e, 1.0, 1.0, 1.0], [4.0, *coeff_a, 1.0])?.abs();
    let direction = if -coeff_a / coeff_e < 0.0 { -1.0 } else { 1.0 };
    let parameter = |x: f64| {
        cadmpeg_ir::math::multiply_divide(
            cadmpeg_ir::scalar::FiniteReal::new(-direction * x)?,
            cadmpeg_ir::scalar::FiniteReal::HALF,
            focal,
        )
        .map(cadmpeg_ir::scalar::FiniteReal::get)
    };
    let mut start = parameter(*start_x)?;
    let mut end = parameter(*end_x)?;
    let axis = if end < start {
        start = -start;
        end = -end;
        Vector3::new(0.0, 0.0, -1.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let vertex = parabola.vertex().get();
    if vertex.x != 0.0
        || vertex.y != 0.0
        || vertex.z != *plane_z
        || parabola.focal_distance().get() != focal.get()
        || *parabola.frame().axis().as_raw() != axis
        || *parabola.frame().reference().as_raw() != Vector3::new(0.0, direction, 0.0)
        || span.start.x != *start_x
        || span.start.y != *start_y
        || span.start.z != *plane_z
        || span.end.x != *end_x
        || span.end.y != *end_y
        || span.end.z != *plane_z
        || span.range.get() != [start, end]
    {
        return None;
    }
    Some(format!("{};", tokens.join(",")))
}

fn planar_ellipse(
    ir: &CadIr,
    id: &CurveId,
    geometry: &CurveGeometry,
    span: Option<&CurveSpan>,
    form: i64,
) -> Option<String> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse)) = geometry else {
        return None;
    };
    let span = span?;
    let (values, tokens) = source_parameters(ir, id, 104, form, 12)?;
    let [coeff_a, coeff_b, coeff_c, coeff_d, coeff_e, coeff_f, plane_z, start_x, start_y, end_x, end_y] =
        values.as_slice()
    else {
        return None;
    };
    if *coeff_b != 0.0
        || *coeff_d != 0.0
        || *coeff_e != 0.0
        || *coeff_a == 0.0
        || *coeff_c == 0.0
        || coeff_a.is_sign_positive() != coeff_c.is_sign_positive()
        || coeff_f.is_sign_positive() == coeff_a.is_sign_positive()
    {
        return None;
    }
    let rx = coeff_f.abs().sqrt() / coeff_a.abs().sqrt();
    let ry = coeff_f.abs().sqrt() / coeff_c.abs().sqrt();
    let (major, minor, reference) = if rx < ry {
        (ry, rx, Vector3::new(0.0, 1.0, 0.0))
    } else {
        (rx, ry, Vector3::new(1.0, 0.0, 0.0))
    };
    let center = ellipse.center().get();
    if center.x != 0.0
        || center.y != 0.0
        || center.z != *plane_z
        || ellipse.major_radius().get() != major
        || ellipse.minor_radius().get() != minor
        || *ellipse.frame().axis().as_raw() != Vector3::new(0.0, 0.0, 1.0)
        || *ellipse.frame().reference().as_raw() != reference
        || span.start.x != *start_x
        || span.start.y != *start_y
        || span.start.z != *plane_z
        || span.end.x != *end_x
        || span.end.y != *end_y
        || span.end.z != *plane_z
    {
        return None;
    }
    let parameter = |x: f64, y: f64| {
        let (u, v) = if rx < ry {
            (y / major, -x / minor)
        } else {
            (x / major, y / minor)
        };
        v.atan2(u).rem_euclid(std::f64::consts::TAU)
    };
    let start = parameter(*start_x, *start_y);
    let mut sweep = (parameter(*end_x, *end_y) - start).rem_euclid(std::f64::consts::TAU);
    if sweep == 0.0 {
        sweep = std::f64::consts::TAU;
    }
    if span.range.get() != [start, start + sweep] {
        return None;
    }
    Some(format!("{};", tokens.join(",")))
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
    let (values, tokens) = source_parameters(ir, id, 100, 0, 8)?;
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

fn source_parameters(
    ir: &CadIr,
    id: &CurveId,
    entity_type: i64,
    form: i64,
    count: usize,
) -> Option<(Vec<f64>, Vec<String>)> {
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
    if record.field("entity_type")?.as_i64()? != entity_type
        || record.field("form")?.as_i64()? != form
        || record.field("transform")?.as_i64()? != 0
    {
        return None;
    }
    let parameters = record.field("parameters")?;
    let parameters = parameters.as_array()?;
    if parameters.len() != count {
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
    Some((values, tokens))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::{
        analytic::{EllipseCurve, ParabolaCurve},
        CurveGeometry, SolvedCurveGeometry,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    use crate::test_support::test_curves_and_surfaces::conic_arc_file;
    use crate::IgesCodec;

    #[test]
    fn changed_ellipse_does_not_reuse_source_parameters() {
        let decoded = IgesCodec
            .decode(
                &mut Cursor::new(conic_arc_file(0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;")),
                &DecodeOptions::default(),
            )
            .expect("ellipse decodes");
        let ir = decoded.ir();
        let curve = &ir.model.curves[0];
        let span = super::super::curve_reference_span(ir, &curve.id, &curve.geometry)
            .expect("ellipse has a span");
        assert!(super::planar_ellipse(ir, &curve.id, &curve.geometry, Some(&span), 0).is_some());
        let Some(SolvedCurveGeometry::Ellipse(ellipse)) = curve.geometry.solved() else {
            panic!("fixture holds an ellipse");
        };
        let edited = EllipseCurve::try_from_parts(
            ellipse.center(),
            *ellipse.frame(),
            PositiveLength::new(3.0).expect("positive radius"),
            ellipse.minor_radius(),
        )
        .expect("ordered radii");
        let edited = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(edited));
        assert!(super::planar_ellipse(ir, &curve.id, &edited, Some(&span), 0).is_none());
    }

    #[test]
    fn changed_parabola_does_not_reuse_source_parameters() {
        let decoded = IgesCodec
            .decode(
                &mut Cursor::new(conic_arc_file(3, b"104,1,0,0,0,-4,0,0,2,1,-2,1;")),
                &DecodeOptions::default(),
            )
            .expect("parabola decodes");
        let ir = decoded.ir();
        let curve = &ir.model.curves[0];
        let span = super::super::curve_reference_span(ir, &curve.id, &curve.geometry)
            .expect("parabola has a span");
        assert!(super::planar_parabola(ir, &curve.id, &curve.geometry, Some(&span), 3).is_some());
        let Some(SolvedCurveGeometry::Parabola(parabola)) = curve.geometry.solved() else {
            panic!("fixture holds a parabola");
        };
        let edited = ParabolaCurve::new(
            parabola.vertex(),
            *parabola.frame(),
            PositiveLength::new(2.0).expect("positive focal distance"),
        );
        let edited = CurveGeometry::Solved(SolvedCurveGeometry::Parabola(edited));
        assert!(super::planar_parabola(ir, &curve.id, &edited, Some(&span), 3).is_none());
    }
}
