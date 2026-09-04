// SPDX-License-Identifier: Apache-2.0
//! Source-less IR builders and encode/decode helpers for crate tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::SldprtCodec;

/// Translate every model-space carrier along x so a forced modification stays
/// geometrically consistent: vertices remain on their edge curves and surfaces.
pub(crate) fn translate_model_x(ir: &mut cadmpeg_ir::document::CadIr, dx: f64) {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    fn translate_curve_x(curve: &mut CurveGeometry, dx: f64) {
        match curve {
            CurveGeometry::Line { origin, .. } => origin.x += dx,
            CurveGeometry::Circle { center, .. }
            | CurveGeometry::Ellipse { center, .. }
            | CurveGeometry::Hyperbola { center, .. } => center.x += dx,
            CurveGeometry::Parabola { vertex, .. } => vertex.x += dx,
            CurveGeometry::Degenerate { point } => point.x += dx,
            CurveGeometry::Nurbs(nurbs) => {
                for pole in nurbs.control_points_mut() {
                    pole.x += dx;
                }
            }
            CurveGeometry::Polyline(polyline) => {
                for point in polyline.points_mut() {
                    point.x += dx;
                }
            }
            CurveGeometry::Transformed { transform, .. } => transform.rows[0][3] += dx,
            CurveGeometry::Composite { .. } => {}
            CurveGeometry::Procedural { .. } => {}
            CurveGeometry::Unknown { .. } => {}
        }
    }
    for point in &mut ir.model.points {
        point.position.x += dx;
    }
    for curve in &mut ir.model.curves {
        translate_curve_x(&mut curve.geometry, dx);
    }
    for surface in &mut ir.model.surfaces {
        match &mut surface.geometry {
            SurfaceGeometry::Plane { origin, .. }
            | SurfaceGeometry::Cylinder { origin, .. }
            | SurfaceGeometry::Cone { origin, .. } => origin.x += dx,
            SurfaceGeometry::Sphere { center, .. } | SurfaceGeometry::Torus { center, .. } => {
                center.x += dx;
            }
            SurfaceGeometry::Nurbs(nurbs) => {
                for pole in nurbs.control_points_mut() {
                    pole.x += dx;
                }
            }
            SurfaceGeometry::Polygonal(surface) => {
                for vertex in surface.vertices_mut() {
                    vertex.x += dx;
                }
            }
            SurfaceGeometry::Transformed { transform, .. } => transform.rows[0][3] += dx,
            SurfaceGeometry::Procedural { .. } => {}
            SurfaceGeometry::Unknown { .. } => {}
        }
    }
}

pub(crate) fn strict_options() -> DecodeOptions {
    use cadmpeg_core::decode::{DecodeMode, DecodePolicy};
    DecodeOptions {
        container_only: false,
        policy: DecodePolicy {
            mode: DecodeMode::Strict,
            ..DecodePolicy::desktop()
        },
    }
}

/// Translate every positional carrier in the model by `t`. Directions and
/// normals are invariant under translation, so a pure translation is a rigid
/// motion of the whole body.
pub(crate) fn translate_model(ir: &mut cadmpeg_ir::CadIr, t: [f64; 3]) {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;
    let shift = |p: &Point3| Point3::new(p.x + t[0], p.y + t[1], p.z + t[2]);
    for point in &mut ir.model.points {
        point.position = shift(&point.position);
    }
    for curve in &mut ir.model.curves {
        if let CurveGeometry::Line { origin, .. } = &mut curve.geometry {
            *origin = shift(origin);
        }
    }
    for surface in &mut ir.model.surfaces {
        if let SurfaceGeometry::Plane { origin, .. } = &mut surface.geometry {
            *origin = shift(origin);
        }
    }
}

pub(crate) fn source_less_cube() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    ir
}

pub(crate) fn encode_decode_result(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::codec::DecodeResult {
    let mut encoded = Vec::new();
    SldprtCodec
        .plan(
            cadmpeg_ir::codec::write::EncodeInput { ir, fidelity: None },
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap()
}

pub(crate) fn encode_decode(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::CadIr {
    encode_decode_result(ir).into_parts().0
}

pub(crate) fn sorted_point_positions(ir: &cadmpeg_ir::CadIr) -> Vec<[f64; 3]> {
    let mut positions: Vec<[f64; 3]> = ir
        .model
        .points
        .iter()
        .map(|point| [point.position.x, point.position.y, point.position.z])
        .collect();
    positions.sort_by(|a, b| a.partial_cmp(b).unwrap());
    positions
}
