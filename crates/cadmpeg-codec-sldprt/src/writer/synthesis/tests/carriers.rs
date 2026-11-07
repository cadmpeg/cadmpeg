// SPDX-License-Identifier: Apache-2.0
//! Synthesized periodic and sampled carriers.

use super::*;

#[test]
fn periodic_nurbs_surface_is_written_and_redecoded() {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::topology::Sense;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let face = ir.model.faces[0].clone();
    let loop_id = face.loops.iter().next().unwrap();
    let boundary = ir.model.loops.iter().find(|lp| &lp.id == loop_id).unwrap();
    let corners = boundary
        .coedges()
        .iter()
        .map(|id| {
            let coedge = ir
                .model
                .coedges
                .iter()
                .find(|coedge| &coedge.id == id)
                .unwrap();
            let edge = ir
                .model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .unwrap();
            let vertex_id = if coedge.sense == Sense::Forward {
                &edge.start
            } else {
                &edge.end
            };
            let vertex = ir
                .model
                .vertices
                .iter()
                .find(|vertex| &vertex.id == vertex_id)
                .unwrap();
            ir.model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .unwrap()
                .position()
        })
        .collect::<Vec<_>>();
    assert_eq!(corners.len(), 4);
    let surface = NurbsSurface::new(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], true),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![corners[0], corners[3]],
                vec![corners[1], corners[2]],
                vec![corners[0], corners[3]],
            ],
        },
        false,
    )
    .unwrap();
    let expected = surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == face.surface)
        .unwrap()
        .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.surfaces.iter().any(|surface| matches!(&surface.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) if nurbs.u_periodic() && !nurbs.v_periodic()
            && nurbs.u_knots() == expected.u_knots() && nurbs.v_knots() == expected.v_knots()
            && nurbs.control_grid() == expected.control_grid()
    )));
}

#[test]
fn periodic_nurbs_curve_is_written_and_redecoded() {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let edge = ir.model.edges[0].clone();
    let point = |vertex_id: &cadmpeg_ir::ids::VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|v| &v.id == vertex_id)
            .unwrap();
        ir.model
            .points
            .iter()
            .find(|p| p.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    let start = point(&edge.start);
    let end = point(&edge.end);
    let poles = NurbsPoles3::from_lanes(vec![start, end, start], None).unwrap();
    let curve = NurbsCurve::new(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], poles, true).unwrap();
    let mut carrier = Vec::new();
    crate::writer::write_nurbs_curve(&mut carrier, 10, &curve, &mut 20, 0.001).unwrap();
    let descriptor = carrier.windows(2).position(|tag| tag == [0, 0x88]).unwrap() + 2;
    assert_eq!(
        cadmpeg_core::decode::View::u16_be_at(
            &carrier,
            descriptor + crate::layout::bspline_curve_descriptor::PERIODIC
        ),
        Some(0x0100)
    );
    ir.model
        .curves
        .iter_mut()
        .find(|curve| Some(&curve.id) == edge.curve())
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
    ir.model.edges[0].set_param_range(Some(
        cadmpeg_ir::topology::ParameterInterval::new([0.0, 0.5]).unwrap(),
    ));
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.curves.iter().any(|curve| matches!(&curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) if nurbs.periodic()
            && nurbs.control_points().iter().map(|p| p.get()).collect::<Vec<_>>() == [start, end, start]
    )), "{:#?}", output.ir().model.curves);
}

#[test]
fn polyline_carrier_is_written_as_a_degree_one_nurbs() {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let edge = ir.model.edges[0].clone();
    let point = |vertex_id: &cadmpeg_ir::ids::VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|v| &v.id == vertex_id)
            .unwrap();
        ir.model
            .points
            .iter()
            .find(|p| p.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    let start = point(&edge.start);
    let end = point(&edge.end);
    let middle = Point3::new(
        f64::midpoint(start.x, end.x),
        f64::midpoint(start.y, end.y),
        f64::midpoint(start.z, end.z),
    );
    let polyline = PolylineCurve::new(
        PolylineSamples::Unparameterized {
            points: vec![start, middle, end].try_into().unwrap(),
        },
        0.0,
    )
    .unwrap();
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| Some(&curve.id) == edge.curve())
        .unwrap();
    let curve_id = curve.id.clone();
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline));
    curve.parameter_range = None;
    ir.model.edges[0].set_param_range(None);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.polyline-converted"
            && loss.message.contains(curve_id.as_str())));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert!(output.ir().model.curves.iter().any(|curve| matches!(&curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
            if nurbs.degree() == 1 && nurbs.knots().as_slice() == [0.0, 0.0, 1.0, 2.0, 2.0]
                && nurbs.control_points().iter().map(|point| point.get()).collect::<Vec<_>>() == [start, middle, end]
    )));
}
