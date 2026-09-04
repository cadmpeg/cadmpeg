// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

#[test]
fn generated_source_less_face_writes_signed_torus_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Torus {
        center: cadmpeg_ir::math::Point3::new(3.0, -6.0, 9.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.5,
        minor_radius: -6.0,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less torus encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less torus round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_nurbs_surface_carrier() {
    use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![-1.0, -1.0, 2.0, 2.0],
        v_knots: vec![-2.0, -2.0, 3.0, 3.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 2.0),
            cadmpeg_ir::math::Point3::new(20.0, 0.0, 3.0),
            cadmpeg_ir::math::Point3::new(20.0, 10.0, 4.0),
        ],
        weights: None,
        normal_reversed: false,
        u_periodic: true,
        v_periodic: false,
    });
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less NURBS surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less NURBS surface round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_surface_carrier() {
    use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 8.0, 1.0),
            cadmpeg_ir::math::Point3::new(12.0, 0.0, 2.0),
            cadmpeg_ir::math::Point3::new(12.0, 8.0, 3.0),
        ],
        weights: Some(vec![1.0, 0.75, 1.25, 1.0]),
        normal_reversed: false,
        u_periodic: false,
        v_periodic: true,
    });
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rational NURBS surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational NURBS surface round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_edge_curve() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let curve_id = CurveId("generated:nurbs_curve#0".into());
    let expected = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(5.0, 8.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 2.0),
        ],
        weights: Some(vec![1.0, 0.6, 1.0]),
        periodic: true,
    });
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([-1.0, 2.0]);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rational NURBS curve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational NURBS curve round trip");
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected);
    assert_eq!(
        round_trip.ir().model.edges[0].param_range,
        Some([-1.0, 2.0])
    );
}

#[test]
fn generated_source_less_face_writes_inline_nurbs_pcurve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated inline pcurve decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.pcurves[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less inline pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less inline pcurve round trip");
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    assert_eq!(round_trip.ir().model.pcurves[0].geometry, expected.geometry);
    assert_eq!(
        round_trip.ir().model.pcurves[0].wrapper_reversed(),
        expected.wrapper_reversed()
    );
    assert_eq!(
        round_trip.ir().model.pcurves[0].native_tail_flags(),
        expected.native_tail_flags()
    );
    assert_eq!(
        round_trip.ir().model.pcurves[0].parameter_range(),
        expected.parameter_range()
    );
    assert_eq!(
        round_trip.ir().model.pcurves[0].fit_tolerance(),
        expected.fit_tolerance()
    );
    assert_eq!(
        round_trip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    let pcurve_coedge = round_trip
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| !coedge.pcurves.is_empty())
        .expect("generated coedge with pcurve");
    assert!(pcurve_coedge
        .pcurves
        .first()
        .is_some_and(|use_| use_.parameter_range.is_some()));
    assert!(crate::validate::validate_native(round_trip.ir()).is_empty());
}

#[test]
fn generated_source_less_face_lowers_line_pcurve_exactly() {
    use cadmpeg_ir::geometry::PcurveGeometry;
    use cadmpeg_ir::math::Point2;

    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated inline pcurve decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let pcurve = &mut source_less.model.pcurves[0];
    pcurve.geometry = PcurveGeometry::Line {
        origin: Point2::new(2.0, -1.0),
        direction: Point2::new(0.5, 2.0),
    };
    let cadmpeg_ir::geometry::PcurveMetadata::AsmInline(inline) = &mut pcurve.metadata else {
        panic!("decoded fixture uses ASM inline pcurve metadata")
    };
    inline.parameter_range = [-2.0, 3.0];

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less line pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less line pcurve round trip");
    assert_eq!(
        round_trip.ir().model.pcurves[0].parameter_range(),
        Some([-2.0, 3.0])
    );
    assert_eq!(
        round_trip.ir().model.pcurves[0].geometry,
        PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![-2.0, -2.0, 3.0, 3.0],
            control_points: vec![Point2::new(1.0, -5.0), Point2::new(3.5, 5.0)],
            weights: None,
            periodic: false,
        }
    );
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_pcurve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_rational_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated rational pcurve decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.pcurves[0].clone();
    assert!(matches!(
        &expected.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            weights: Some(weights),
            ..
        } if weights == &vec![1.0, 0.5]
    ));

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rational pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational pcurve round trip");
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    let actual = &round_trip.ir().model.pcurves[0];
    assert_eq!(actual.geometry, expected.geometry);
    assert_eq!(actual.wrapper_reversed(), expected.wrapper_reversed());
    assert_eq!(actual.native_tail_flags(), expected.native_tail_flags());
    assert_eq!(actual.parameter_range(), expected.parameter_range());
    assert_eq!(actual.fit_tolerance(), expected.fit_tolerance());
}

#[test]
fn generated_source_less_two_faces_preserve_shared_radial_edge() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected_surface = SurfaceGeometry::Cylinder {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_line#0".into());
    let expected_curve = CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less shared-edge encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less shared-edge round trip");
    assert_eq!(round_trip.ir().model.faces.len(), 2);
    assert_eq!(round_trip.ir().model.loops.len(), 2);
    assert_eq!(round_trip.ir().model.coedges.len(), 6);
    assert_eq!(round_trip.ir().model.edges.len(), 5);
    assert_eq!(round_trip.ir().model.vertices.len(), 4);
    assert_eq!(round_trip.ir().model.surfaces.len(), 2);
    assert_eq!(round_trip.ir().model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected_curve);
    assert!(round_trip.ir().model.edges[0].curve.is_some());
    let shared = round_trip
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| {
            round_trip
                .ir()
                .model
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == edge.id)
                .count()
                == 2
        })
        .expect("shared radial edge");
    let radial = round_trip
        .ir()
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == shared.id)
        .collect::<Vec<_>>();
    assert_eq!(radial.len(), 2);
    assert_eq!(radial[0].radial_next, radial[1].id);
    assert_eq!(radial[1].radial_next, radial[0].id);
}

#[test]
fn generated_source_less_face_preserves_multiple_loop_chain() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId, PointId, VertexId};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    let loop_id = LoopId("generated:loop#1".into());
    let mut coedge_ids = Vec::new();
    let coordinates = [[2.0, 2.0, 0.0], [4.0, 2.0, 0.0], [2.0, 4.0, 0.0]];
    for (index, [x, y, z]) in coordinates.into_iter().enumerate() {
        let point_id = PointId(format!("generated:inner_point#{index}"));
        source_less.model.points.push(cadmpeg_ir::topology::Point {
            id: point_id.clone(),
            position: cadmpeg_ir::math::Point3::new(x, y, z),
            source_object: None,
        });
        let vertex_id = VertexId(format!("generated:inner_vertex#{index}"));
        source_less
            .model
            .vertices
            .push(cadmpeg_ir::topology::Vertex {
                id: vertex_id,
                point: point_id,
                tolerance: None,
            });
    }
    let inner_vertices = source_less.model.vertices[3..]
        .iter()
        .map(|vertex| vertex.id.clone())
        .collect::<Vec<_>>();
    for index in 0..3 {
        let edge_id = EdgeId(format!("generated:inner_edge#{index}"));
        source_less.model.edges.push(cadmpeg_ir::topology::Edge {
            id: edge_id.clone(),
            curve: None,
            start: inner_vertices[index].clone(),
            end: inner_vertices[(index + 1) % 3].clone(),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        });
        let coedge_id = CoedgeId(format!("generated:inner_coedge#{index}"));
        coedge_ids.push(coedge_id.clone());
        source_less
            .model
            .coedges
            .push(cadmpeg_ir::topology::Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id,
                sense: cadmpeg_ir::topology::Sense::Reversed,
                pcurves: Vec::new(),
                use_curve: None,
            });
    }
    for index in 0..3 {
        let coedge = source_less
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_ids[index])
            .unwrap();
        coedge.next = coedge_ids[(index + 1) % 3].clone();
        coedge.previous = coedge_ids[(index + 2) % 3].clone();
    }
    let face_id = source_less.model.faces[0].id.clone();
    source_less.model.loops.push(cadmpeg_ir::topology::Loop {
        id: loop_id.clone(),
        face: face_id,
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
            coedges: coedge_ids,
            vertex_uses: Vec::new(),
        },
    });
    source_less.model.faces[0].loops.push(loop_id);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multiple-loop encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multiple-loop round trip");
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 2);
    assert_eq!(round_trip.ir().model.faces[0].loops.len(), 2);
    assert_eq!(round_trip.ir().model.coedges.len(), 6);
    assert_eq!(round_trip.ir().model.edges.len(), 6);
}

#[test]
fn generated_source_less_multi_face_writes_nurbs_carriers_and_pcurve() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
    use cadmpeg_ir::ids::{CurveId, PcurveId};

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let pcurve_source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let pcurve = F3dCodec
        .decode(&mut Cursor::new(pcurve_source), &DecodeOptions::default())
        .expect("generated pcurve decode")
        .into_parts()
        .0
        .model
        .pcurves
        .into_iter()
        .next()
        .expect("generated pcurve");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    let expected_surface = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 2.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 3.0),
        ],
        weights: Some(vec![1.0, 0.8, 1.2, 1.0]),
        normal_reversed: false,
        u_periodic: false,
        v_periodic: true,
    });
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_nurbs#0".into());
    let expected_curve = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(5.0, 3.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        weights: Some(vec![1.0, 0.7, 1.0]),
        periodic: false,
    });
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    let pcurve_id = PcurveId("generated:pcurve#0".into());
    let mut pcurve = pcurve;
    pcurve.id = pcurve_id.clone();
    let expected_pcurve = pcurve.geometry.clone();
    source_less.model.pcurves.push(pcurve);
    source_less.model.coedges[0].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: pcurve_id,
        isoparametric: None,
        parameter_range: None,
    }];

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-face NURBS encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face NURBS round trip");
    assert_eq!(round_trip.ir().model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected_curve);
    assert_eq!(round_trip.ir().model.pcurves[0].geometry, expected_pcurve);
    assert_eq!(
        round_trip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
fn generated_source_less_unit_cube_writes_closed_shared_edge_shell() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let tolerant_coedge = source_less.model.coedges[7].id.clone();
    f3d_native_mut(&mut source_less).tolerant_coedge_parameters =
        vec![cadmpeg_asm::brep::records::TolerantCoedgeParameters {
            id: "f3d:asm:tolerant-coedge-parameters#cube".into(),
            coedge: tolerant_coedge,
            record_index: 0,
            parameter_range: [-1.5, 2.25],
            extension: cadmpeg_asm::brep::records::TolerantCoedgeExtension::None,
        }];
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less unit cube encode");
    {
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).unwrap();
        let mut stream = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
            .unwrap()
            .read_to_end(&mut stream)
            .unwrap();
        let records = cadmpeg_asm::sab::frame(&stream, 47, stream.len(), 8).unwrap();
        let tolerant = records
            .iter()
            .find(|record| record.head == "tcoedge")
            .expect("canonical tolerant coedge record");
        assert!(matches!(
            tolerant.chunk(13),
            Some(cadmpeg_asm::sab::Token::Ref(-1))
        ));
        assert!(matches!(
            tolerant.chunk(14),
            Some(cadmpeg_asm::sab::Token::Long(0))
        ));
        assert!(matches!(
            tolerant.chunk(15),
            Some(cadmpeg_asm::sab::Token::Long(0))
        ));
    }
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less unit cube round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].name.as_deref(),
        source_less.model.bodies[0].name.as_deref()
    );
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
    assert_eq!(round_trip.ir().model.regions.len(), 1);
    assert_eq!(round_trip.ir().model.shells.len(), 1);
    assert_eq!(round_trip.ir().model.faces.len(), 6);
    assert_eq!(
        round_trip
            .ir()
            .model
            .faces
            .iter()
            .map(|face| face.name.as_deref())
            .collect::<Vec<_>>(),
        source_less
            .model
            .faces
            .iter()
            .map(|face| face.name.as_deref())
            .collect::<Vec<_>>()
    );
    assert_eq!(round_trip.ir().model.loops.len(), 6);
    assert_eq!(round_trip.ir().model.coedges.len(), 24);
    assert_eq!(round_trip.ir().model.edges.len(), 12);
    assert_eq!(round_trip.ir().model.vertices.len(), 8);
    assert_eq!(round_trip.ir().model.points.len(), 8);
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_coedge_parameters[0].parameter_range,
        [-1.5, 2.25]
    );
    assert!(round_trip.ir().model.edges.iter().all(|edge| {
        round_trip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .count()
            == 2
    }));
    let report = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn generated_source_less_multi_face_writes_torus_and_circle_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected_surface = SurfaceGeometry::Torus {
        center: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 8.0,
        minor_radius: -3.0,
    };
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_circle#0".into());
    let expected_curve = CurveGeometry::Circle {
        center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([0.25, 1.5]);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-face torus encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face torus round trip");
    assert_eq!(round_trip.ir().model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected_curve);
    assert_eq!(
        round_trip.ir().model.edges[0].param_range,
        Some([0.25, 1.5])
    );
}

#[test]
fn generated_source_less_multi_face_writes_cone_sphere_and_ellipse_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 8.0,
        ratio: 1.0,
        half_angle: 0.35,
    };
    let sphere = SurfaceGeometry::Sphere {
        center: Point3::new(-1.0, 4.0, 2.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -12.0,
    };
    source_less.model.surfaces[0].geometry = cone.clone();
    source_less.model.surfaces[1].geometry = sphere.clone();
    let curve_id = CurveId("generated:shared_ellipse#0".into());
    let ellipse = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 9.0,
        minor_radius: 4.0,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: ellipse.clone(),
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-face analytic encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face analytic round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, cone);
    assert_eq!(round_trip.ir().model.surfaces[1].geometry, sphere);
    assert_eq!(round_trip.ir().model.curves[0].geometry, ellipse);
}

#[test]
fn generated_source_less_writes_translational_extrusion_definition() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_surfaces[0].clone();
    let directrix_id = match expected.definition() {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { directrix, .. } => {
            directrix.clone()
        }
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == directrix_id)
        .expect("extrusion directrix")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(5.0, 10.0, -5.0),
        direction: cadmpeg_ir::math::Vector3::new(2.0, -4.0, 1.0),
    };

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less extrusion round trip");
    assert_eq!(round_trip.ir().model.procedural_surfaces.len(), 1);
    let actual = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(actual.definition(), expected.definition());
    assert_eq!(actual.cache_fit_tolerance(), expected.cache_fit_tolerance());
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
        directrix,
        direction,
        parameter_interval,
        native_position,
        revision_form: None,
    } = actual.definition()
    else {
        panic!("expected extrusion definition")
    };
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *directrix));
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [0.25, 0.25, 0.75, 0.75]
                && curve.control_points == [
                    cadmpeg_ir::math::Point3::new(5.5, 9.0, -4.75),
                    cadmpeg_ir::math::Point3::new(6.5, 7.0, -4.25),
                ]
    ));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
}

/// The revision-gated `cyl_spl_sur` layout carries the shared surface tail, so
/// the tail's enum, discontinuity arrays, and closing boolean reach the IR and
/// come back byte-identical through source-less generation. The compact layout
/// has no tail and keeps writing the compact record.
#[test]
fn generated_source_less_writes_revision_gated_extrusion_definition() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_versioned_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("revision-gated extrusion decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_surfaces[0].clone();
    let ProceduralSurfaceDefinition::Extrusion {
        revision_form: Some(form),
        ..
    } = expected.definition()
    else {
        panic!("expected a revision-gated extrusion")
    };
    assert_eq!(form.revision, 23100);
    assert_eq!(form.flags, [true]);
    assert_eq!(form.cache.selector(), 0);
    assert_eq!(form.cache.parameterization(), None);
    assert_eq!(
        form.discontinuities,
        expected_revision_surface_tail_discontinuities()
    );
    assert!(!form.tail_flag);
    assert_eq!(expected.cache_fit_tolerance(), Some(0.02));

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("revision-gated extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("revision-gated extrusion round trip");
    let actual = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(actual.definition(), expected.definition());
    assert_eq!(actual.cache_fit_tolerance(), expected.cache_fit_tolerance());

    // The directrix sense Boolean is stored, not assumed: the opposite value
    // survives the same round trip.
    source_less.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Extrusion {
            revision_form: Some(form),
            ..
        } = definition
        else {
            unreachable!("revision-gated extrusion")
        };
        form.flags = vec![false];
    });
    let expected = source_less.model.procedural_surfaces[0].clone();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("reversed-directrix extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("reversed-directrix extrusion round trip");
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
        expected.definition()
    );
}

/// A form-`2` extrusion stores its parameterization in place of a solved cache.
/// It regenerates from that parameterization, with no cache to draw on.
#[test]
fn generated_source_less_writes_parameterized_extrusion_definition() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_versioned_cyl_spl_sur_with_tail_smbh(2),
            )),
            &DecodeOptions::default(),
        )
        .expect("parameterized extrusion decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_surfaces[0].clone();
    assert_eq!(expected.cache_fit_tolerance(), None);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("parameterized extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("parameterized extrusion round trip");
    let actual = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(actual.cache_fit_tolerance(), None);
    let ProceduralSurfaceDefinition::Extrusion {
        revision_form: Some(form),
        ..
    } = actual.definition()
    else {
        panic!("expected a parameterized revision-gated extrusion")
    };
    assert_eq!(form.cache.selector(), 2);
    assert_eq!(
        form.cache.parameterization(),
        Some(&expected_revision_surface_tail_parameterization())
    );
    assert_eq!(actual.definition(), expected.definition());
}

#[test]
fn generated_cacheless_translational_extrusion_retains_exact_construction() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cacheless_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated cache-less extrusion decode");

    assert_eq!(decoded.ir().model.procedural_surfaces.len(), 1);
    let procedural = &decoded.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let ProceduralSurfaceDefinition::Extrusion {
        directrix,
        direction,
        parameter_interval,
        native_position,
        revision_form: None,
    } = procedural.definition()
    else {
        panic!("expected extrusion definition")
    };
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
    let directrix_geometry = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .map(|curve| &curve.geometry);
    assert!(
        matches!(directrix_geometry, Some(CurveGeometry::Nurbs(_))),
        "unexpected extrusion directrix: {directrix_geometry:?}"
    );
    let u = 0.5;
    let v = 0.25;
    let directrix_point =
        cadmpeg_ir::eval::curve_point(directrix_geometry.expect("typed extrusion directrix"), u)
            .expect("directrix evaluation");
    let surface_geometry = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| {
            decoded.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
        })
        .map(|surface| &surface.geometry)
        .expect("extrusion surface carrier");
    let surface_point = cadmpeg_ir::eval::model_surface_point(decoded.ir(), surface_geometry, u, v)
        .expect("procedural extrusion evaluation");
    assert_eq!(surface_point.x, directrix_point.x + v * direction.x);
    assert_eq!(surface_point.y, directrix_point.y + v * direction.y);
    assert_eq!(surface_point.z, directrix_point.z + v * direction.z);
    assert!(matches!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| {
                decoded.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
            })
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Procedural { construction, .. }) if *construction == procedural.id
    ));

    let expected_definition = procedural.definition().clone();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-less extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-less extrusion round trip");
    assert_eq!(round_trip.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
        &expected_definition
    );
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].cache_fit_tolerance(),
        None
    );
    assert!(matches!(
        round_trip
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| {
                round_trip.ir().model.procedural_surface_owner(
                    &round_trip.ir().model.procedural_surfaces[0].id,
                ) == Some(&surface.id)
            })
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Procedural { construction, .. })
            if *construction == round_trip.ir().model.procedural_surfaces[0].id
    ));

    source_less.model.procedural_surfaces[0]
        .set_cache_fit_tolerance(Some(0.01))
        .unwrap();
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("cache-less extrusion tolerance must be rejected");
    assert!(error
        .to_string()
        .contains("cache-less F3D extrusion cannot carry a cache-fit tolerance"));
}

#[test]
fn generated_cacheless_circle_extrusion_decodes_as_analytic_cylinder() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cacheless_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated cache-less extrusion decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let directrix = source_less.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval,
            direction,
            ..
        } = definition
        else {
            panic!("expected extrusion definition")
        };
        *parameter_interval = Some([0.0, std::f64::consts::TAU]);
        *direction = Vector3::new(0.0, 0.0, -20.0);
        directrix.clone()
    });
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == directrix)
        .expect("extrusion directrix")
        .geometry = CurveGeometry::Circle {
        center: Point3::new(2.0, 3.0, 4.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less circle extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less circle extrusion round trip");
    let surface = round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| {
            round_trip
                .ir()
                .model
                .procedural_surface_owner(&round_trip.ir().model.procedural_surfaces[0].id)
                == Some(&surface.id)
        })
        .expect("extrusion carrier");
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = surface
        .geometry
        .solved_cache()
        .expect("extrusion solved cache")
    else {
        panic!("unexpected extrusion carrier: {:?}", surface.geometry)
    };
    assert!((origin.x - 2.0).abs() < 1.0e-12);
    assert!((origin.y - 3.0).abs() < 1.0e-12);
    assert!((origin.z - 4.0).abs() < 1.0e-12);
    assert_eq!(*axis, Vector3::new(0.0, 0.0, -1.0));
    assert!((ref_direction.x - 1.0).abs() < 1.0e-12);
    assert!(ref_direction.y.abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
    assert!((radius - 5.0).abs() < 1.0e-12);
}

#[test]
fn generated_source_less_writes_rolling_ball_blend_definition() {
    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let supports = match source_less.model.procedural_surfaces[0].definition() {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend { supports, .. } => {
            supports.each_ref().map(|support| {
                support
                    .as_ref()
                    .expect("rolling-ball support")
                    .surface
                    .clone()
            })
        }
        _ => panic!("expected rolling-ball definition"),
    };
    let spine = match source_less.model.procedural_surfaces[0].definition() {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend { spine, .. } => {
            spine.clone().expect("rolling-ball spine")
        }
        _ => unreachable!(),
    };
    let support_geometries = [
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
            center: cadmpeg_ir::math::Point3::new(10.0, -5.0, 2.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius: 7.5,
        },
    ];
    for (support, geometry) in supports.iter().zip(&support_geometries) {
        source_less
            .model
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == *support)
            .expect("rolling-ball support carrier")
            .geometry = geometry.clone();
    }
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .expect("rolling-ball spine carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(-2.0, 4.0, 1.0),
        direction: cadmpeg_ir::math::Vector3::new(3.0, -1.0, 2.0),
    };
    let expected = source_less.model.procedural_surfaces[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rolling-ball encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rolling-ball round trip");
    assert_eq!(round_trip.ir().model.procedural_surfaces.len(), 1);
    let actual = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(actual.definition(), expected.definition());
    assert_eq!(actual.cache_fit_tolerance(), expected.cache_fit_tolerance());
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend {
        supports, spine, ..
    } = actual.definition()
    else {
        unreachable!()
    };
    for (support, expected) in supports.iter().zip(support_geometries) {
        let support = support.as_ref().expect("round-trip rolling-ball support");
        let actual = round_trip
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == support.surface)
            .expect("round-trip rolling-ball support carrier");
        assert_eq!(actual.geometry, expected);
    }
    let spine = spine.as_ref().expect("round-trip rolling-ball spine");
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *spine)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [0.0, 0.0, 1.0, 1.0]
                && curve.control_points == [
                    cadmpeg_ir::math::Point3::new(-2.0, 4.0, 1.0),
                    cadmpeg_ir::math::Point3::new(1.0, 3.0, 3.0),
                ]
    ));
}
