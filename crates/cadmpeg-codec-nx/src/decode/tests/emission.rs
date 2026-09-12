// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

const TOLERANT_INTERSECTION_FIT: f64 = 1.0e-8;
const EPS_TOPOLOGY_TOLERANCE: f64 = 1.0e-8;

use crate::decode::blend::analytic_surface_offset;
use crate::decode::build::{
    ordered_curve_candidates, ordered_point_candidates, ordered_surface_candidates,
};
use crate::decode::pcurves::attach_tolerant_edge_intersections;

use crate::framing::node_kind::NodeKind;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, PcurveNurbs,
    ProceduralCurveDefinition, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::{LossCategory, LossKind, LossTaxonomy};
use cadmpeg_ir::Exactness;

use crate::loss::NxLossCode;
use crate::test_support::*;
use crate::NxCodec;

#[test]
fn decode_refuses_when_max_entities_is_below_known_cardinality() {
    use cadmpeg_core::decode::ResourceDimension;

    let file = prt_with_partition(&topology_partition_stream());
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = NxCodec
        .decode(&mut Cursor::new(file), &options)
        .expect_err("max_entities below stream or IR cardinality must refuse");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
        ),
        "{error:?}"
    );
}

#[test]
fn decode_keeps_stream_and_model_entity_admission_additive() {
    use cadmpeg_core::decode::ResourceDimension;

    let file = prt_with_partition(&topology_partition_stream());
    let decoded = NxCodec
        .decode(&mut Cursor::new(file.clone()), &DecodeOptions::default())
        .expect("decode topology partition");
    let model_entities = decoded.ir().model.entity_count() as u64;
    assert!(model_entities > 1);

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = model_entities;
    let error = NxCodec
        .decode(&mut Cursor::new(file.clone()), &options)
        .expect_err("one stream must remain additive to the model entities");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit NX entities"
        ),
        "{error:?}"
    );

    options.policy.limits.max_entities = model_entities + 1;
    NxCodec
        .decode(&mut Cursor::new(file), &options)
        .expect("the exact additive entity limit must admit the fixture");
}

#[test]
fn nx_circular_cone_offsets_resolve_across_equivalent_axis_origins() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let angle = std::f64::consts::FRAC_PI_6;
    let support = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
            1.0,
            angle,
        )
        .unwrap(),
    ));
    let expected = 2.0;
    let axial_shift = -expected * angle.sin();
    let offset = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::ConeSurface::try_new(
            Point3::new(0.0, 0.0, axial_shift),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0 + expected * angle.cos(),
            1.0,
            angle,
        )
        .unwrap(),
    ));

    let distance = analytic_surface_offset(&support, &offset).expect("offset");
    assert!((distance - expected).abs() <= 1.0e-12);
    let reverse = analytic_surface_offset(&offset, &support).expect("reverse");
    assert!((reverse + expected).abs() <= 1.0e-12);

    let mut lateral = offset.clone();
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) = &mut lateral else {
        unreachable!()
    };
    let origin = cone_surface.origin();
    let axis = cone_surface.axis();
    let ref_direction = cone_surface.ref_direction();
    let radius = cone_surface.radius();
    let ratio = cone_surface.ratio();
    let half_angle = cone_surface.half_angle();
    let mut origin = *origin;
    origin.x = 0.1;
    *cone_surface = cadmpeg_ir::geometry::ConeSurface::try_new(
        origin,
        *axis,
        *ref_direction,
        radius,
        ratio,
        half_angle,
    )
    .unwrap();
    assert!(analytic_surface_offset(&support, &lateral).is_none());

    let mut shifted_parameterization = offset.clone();
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) =
        &mut shifted_parameterization
    else {
        unreachable!()
    };
    let origin = cone_surface.origin();
    let axis = cone_surface.axis();
    let ref_direction = cone_surface.ref_direction();
    let radius = cone_surface.radius();
    let ratio = cone_surface.ratio();
    let half_angle = cone_surface.half_angle();
    let mut origin = *origin;
    origin.z += 0.1;
    *cone_surface = cadmpeg_ir::geometry::ConeSurface::try_new(
        origin,
        *axis,
        *ref_direction,
        radius,
        ratio,
        half_angle,
    )
    .unwrap();
    assert!(analytic_surface_offset(&support, &shifted_parameterization).is_none());

    let mut elliptical = offset;
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) = &mut elliptical else {
        unreachable!()
    };
    let origin = cone_surface.origin();
    let axis = cone_surface.axis();
    let ref_direction = cone_surface.ref_direction();
    let radius = cone_surface.radius();
    let half_angle = cone_surface.half_angle();

    let ratio = 0.5;
    *cone_surface = cadmpeg_ir::geometry::ConeSurface::try_new(
        *origin,
        *axis,
        *ref_direction,
        radius,
        ratio,
        half_angle,
    )
    .unwrap();
    assert!(analytic_surface_offset(&support, &elliptical).is_none());
}

#[test]
fn nx_sphere_offset_lineage_follows_signed_radius_orientation() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let sphere = |radius| {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::SphereSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .unwrap(),
        ))
    };
    assert_eq!(
        analytic_surface_offset(&sphere(4.0), &sphere(6.5)),
        Some(2.5)
    );
    assert_eq!(
        analytic_surface_offset(&sphere(-4.0), &sphere(-6.5)),
        Some(2.5)
    );
    assert_eq!(
        analytic_surface_offset(&sphere(-6.5), &sphere(-4.0)),
        Some(-2.5)
    );
    assert!(analytic_surface_offset(&sphere(4.0), &sphere(-6.5)).is_none());
}

#[test]
fn nx_torus_offset_lineage_requires_one_ring_orientation() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let torus = |minor_radius| {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::TorusSurface::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                10.0,
                minor_radius,
            )
            .unwrap(),
        ))
    };
    assert_eq!(analytic_surface_offset(&torus(2.0), &torus(3.5)), Some(1.5));
    assert_eq!(
        analytic_surface_offset(&torus(-2.0), &torus(-3.5)),
        Some(1.5)
    );
    assert_eq!(
        analytic_surface_offset(&torus(-3.5), &torus(-2.0)),
        Some(-1.5)
    );
    assert!(analytic_surface_offset(&torus(2.0), &torus(-3.5)).is_none());
    assert!(analytic_surface_offset(&torus(2.0), &torus(10.0)).is_none());
}

#[test]
fn decode_reports_unclassified_bounded_offset_store_controls() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_control(&[1, 2, 3, 4]),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir().source.as_ref().unwrap().attributes;
    assert_eq!(attributes["offset_store_control_count"], "1");
    assert_eq!(attributes["classified_offset_store_control_count"], "0");
    assert_eq!(attributes["unclassified_offset_store_control_count"], "1");
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == LossCategory::Other
            && loss
                .message
                .contains("1 of 1 bounded offset-store control block(s)")
    }));
}

#[test]
fn decode_synthesizes_vertex_for_closed_null_vertex_fin() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 12, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = result.ir().model.edges.first().expect("closed edge");
    assert_eq!(edge.start, edge.end);
    assert!(edge.start.as_str().contains("closed-edge"));
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_aliases_partner_closed_null_vertex_fin_to_edge_start() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 12, 1);
    put_ref(&mut stream, fin + 14, 20);

    let mut partner = record(17, 23);
    put_ref(&mut partner, 2, 20);
    put_ref(&mut partner, 6, 1); // radial partner is not a loop member
    put_ref(&mut partner, 8, 20); // self-forward closed endpoint
    put_ref(&mut partner, 10, 20); // self-backward closed endpoint
    put_ref(&mut partner, 12, 1); // null vertex
    put_ref(&mut partner, 14, 7); // partner fin
    put_ref(&mut partner, 16, 8); // same edge
    put_ref(&mut partner, 18, 9); // same curve
    partner[22] = b'+';
    stream.extend(partner);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = result.ir().model.edges.first().expect("closed edge");
    assert_eq!(edge.start, edge.end);
    assert!(edge.start.as_str().contains("closed-edge"));
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_does_not_alias_unresolved_edge_end_to_start_vertex() {
    let mut stream = topology_partition_stream();
    let first_fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin record");
    put_ref(&mut stream, first_fin + 8, 20);
    put_ref(&mut stream, first_fin + 10, 20);
    put_ref(&mut stream, first_fin + 14, 20);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 20);
    put_ref(&mut second_fin, 6, 5);
    put_ref(&mut second_fin, 8, 7);
    put_ref(&mut second_fin, 10, 7);
    put_ref(&mut second_fin, 12, 21);
    put_ref(&mut second_fin, 14, 7);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 9);
    second_fin[22] = b'+';
    stream.extend(second_fin);

    let mut unresolved_vertex = record(18, 28);
    put_ref(&mut unresolved_vertex, 2, 21);
    put_ref(&mut unresolved_vertex, 16, 99);
    put_f64(&mut unresolved_vertex, 18, 0.000_1);
    stream.extend(unresolved_vertex);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert!(result.ir().model.edges.is_empty());
    assert!(result.ir().model.coedges.is_empty());
    assert!(result.ir().model.loops.is_empty());
}

#[test]
fn decode_retains_topology_owned_point_at_origin() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [0.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::points(&stream).len(), 1);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(NodeKind::Point, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert_eq!(result.ir().model.bodies[0].transform, None);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(
        result.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn decode_orders_graph_only_origin_before_later_nonzero_point() {
    let mut stream = topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, first + 16, [0.0, 0.0, 0.0]);
    let mut second = record(29, 40);
    put_ref(&mut second, 2, 77);
    put_vec3(&mut second, 16, [0.04, 0.05, 0.06]);
    stream.extend(second);

    let graph = crate::topology::Graph::parse(&stream);
    let points = ordered_point_candidates(&stream, &graph);
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].1.pos, first);
    assert_eq!(points[0].0, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(points[0].1.xmt, 11);
    assert_eq!(points[1].1.pos, stream.len() - 40);
    assert_eq!(points[1].0, cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0));
    assert_eq!(points[1].1.xmt, 77);
}

#[test]
fn decode_orders_graph_only_escaped_analytics_before_later_records() {
    let mut stream = topology_with_escaped_geometry_envelopes();
    let first_surface = stream
        .windows(3)
        .position(|window| window == [0, 50, 0xff])
        .expect("escaped plane record");
    let first_curve = stream
        .windows(3)
        .position(|window| window == [0, 30, 0xff])
        .expect("escaped line record");

    let second_surface_offset = stream.len();
    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 77);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    stream.extend(plane);

    let second_curve_offset = stream.len();
    let mut line = record(30, 67);
    put_ref(&mut line, 2, 78);
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.04, 0.05, 0.06]);
    put_vec3(&mut line, 43, [0.0, 1.0, 0.0]);
    stream.extend(line);

    let graph = crate::topology::Graph::parse(&stream);
    let surfaces = ordered_surface_candidates(&stream, &graph);
    assert_eq!(surfaces.len(), 2);
    assert_eq!(surfaces[0].1.pos, first_surface);
    assert_eq!(surfaces[0].1.xmt, 6);
    assert_eq!(surfaces[1].1.pos, second_surface_offset);
    assert_eq!(surfaces[1].1.xmt, 77);

    let curves = ordered_curve_candidates(&stream, &graph);
    assert_eq!(curves.len(), 2);
    assert_eq!(curves[0].1.pos, first_curve);
    assert_eq!(curves[0].1.xmt, 9);
    assert_eq!(curves[1].1.pos, second_curve_offset);
    assert_eq!(curves[1].1.xmt, 78);
}

#[test]
fn decode_rejects_scanner_geometry_with_an_ambiguous_record_identity() {
    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 77);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    let mut stream = plane.clone();
    stream.extend(plane);

    assert_eq!(crate::geometry::surfaces(&stream).len(), 2);
    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(NodeKind::Plane, 77).is_none());
    assert!(ordered_surface_candidates(&stream, &graph).is_empty());
}

#[test]
fn decode_does_not_attach_unreferenced_point_to_solid_topology() {
    let mut stream = topology_partition_stream();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 77);
    put_vec3(&mut point, 16, [0.04, 0.05, 0.06]);
    stream.extend_from_slice(&point);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert_eq!(result.ir().model.shells[0].free_vertices().len(), 0);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_retains_connected_topology_with_unknown_surface_carrier() {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut stream, face + 26, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == result.ir().model.faces[0].surface)
        .expect("unknown face carrier");
    assert!(matches!(
        surface.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
    ));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_retains_unknown_non_null_edge_curve_carrier() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let curve = result.ir().model.edges[0]
        .curve()
        .as_ref()
        .and_then(|id| {
            result
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| &curve.id == id)
        })
        .expect("unknown edge carrier");
    assert!(matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_drops_unknown_carrier_outside_emitted_topology() {
    let mut stream = topology_partition_stream();
    let mut orphan = record(16, 32);
    put_ref(&mut orphan, 2, 88);
    put_f64(&mut orphan, 10, 0.000_3);
    put_ref(&mut orphan, 18, 1);
    put_ref(&mut orphan, 24, 99);
    stream.extend(orphan);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert!(result.ir().model.curves.iter().all(|curve| !matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
    )));
    assert_eq!(result.ir().model.edges.len(), 1);
}

#[test]
fn decode_retains_native_carrierless_edge() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let fin = stream
        .windows(2)
        .position(|window| window == [0, 17])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = &result.ir().model.edges[0];
    assert_eq!(edge.curve().as_ref(), None);
    assert_eq!(edge.param_range(), None);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_attaches_dimension_two_bcurve_through_surface_curve() {
    let stream = pcurve_topology_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result.ir().model.coedges[0]
            .pcurves
            .first()
            .map(|pcurve| &pcurve.pcurve),
        Some(&result.ir().model.pcurves[0].id)
    );
    let PcurveGeometry::Nurbs { nurbs } = &result.ir().model.pcurves[0].geometry else {
        panic!("expected NURBS pcurve");
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        nurbs.control_points(),
        [Point2::new(10.0, 20.0), Point2::new(10.0, 20.0)]
    );
    assert!(nurbs.weights().is_none());
    assert!(!nurbs.periodic());
    assert_eq!(result.ir().model.pcurves[0].fit_tolerance(), Some(0.01));
    assert_eq!(
        result.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 0.0)
    );
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_assigns_descending_pcurve_trim_to_the_coedge_use() {
    let mut stream = pcurve_topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 26);
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 26);
    trim[18] = b'+';
    put_ref(&mut trim, 19, 25);
    put_f64(&mut trim, 69, 1.0);
    put_f64(&mut trim, 77, 0.0);
    stream.extend(trim);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.pcurves[0].parameter_range(), None);
    assert_eq!(
        result.ir().model.coedges[0].pcurves[0]
            .parameter_range
            .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints),
        Some([0.0, 1.0])
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_omits_surface_curve_missing_tolerance_sentinel() {
    let mut stream = pcurve_topology_partition_stream();
    let surface_curve = stream
        .windows(2)
        .position(|window| window == [0, 137])
        .expect("surface curve");
    put_f64(
        &mut stream,
        surface_curve + 25,
        crate::decode::MISSING_TOLERANCE,
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.pcurves[0].fit_tolerance(), None);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_rejects_overflowing_pcurve_parameter_conversion() {
    let mut stream = pcurve_topology_partition_stream();
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 22])
        .expect("pcurve payload");
    put_f64(&mut stream, payload + 15, f64::MAX);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.pcurves.is_empty());
    assert!(result.ir().model.coedges[0].pcurves.is_empty());
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_preserves_multiple_shells_in_one_region() {
    let stream = shared_region_shells_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.shells.len(), 2);
    assert_eq!(result.ir().model.regions[0].shells.len(), 2);
    assert_eq!(result.ir().model.bodies[0].regions.len(), 1);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_transfers_point_plane_cylinder_line() {
    let mut cur = Cursor::new(single_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    // Point coordinate is scaled metres → millimetres, byte-exact.
    let p = &result.ir().model.points[0].position;
    assert!((p.x - 62.5).abs() < 1.0e-6 && (p.z - 12.7).abs() < 1.0e-6);

    // One plane, one cylinder decoded.
    let planes = result
        .ir()
        .model
        .surfaces
        .iter()
        .filter(|s| {
            matches!(
                s.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
            )
        })
        .count();
    let cyls: Vec<_> = result
        .ir()
        .model
        .surfaces
        .iter()
        .filter_map(|s| match &s.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                let radius = cylinder_surface.radius();
                Some(radius)
            }
            _ => None,
        })
        .collect();
    assert_eq!(planes, 1);
    assert_eq!(cyls.len(), 1);
    assert!((cyls[0] - 4.05).abs() < 1.0e-6);
    assert!(result.ir().model.surfaces.iter().any(
        |surface| matches!(surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))
        if {
            let axis = plane_surface.u_axis();
            *axis == Vector3::new(1.0, 0.0, 0.0)
        })
    ));
    assert!(result.ir().model.surfaces.iter().any(
        |surface| matches!(surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
        if {
            let direction = cylinder_surface.ref_direction();
            *direction == Vector3::new(1.0, 0.0, 0.0)
        })
    ));

    // One line decoded, with a unit direction.
    let lines: Vec<_> = result
        .ir()
        .model
        .curves
        .iter()
        .filter(|c| {
            matches!(
                c.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
            )
        })
        .collect();
    assert_eq!(lines.len(), 1);

    assert!(result.ir().model.faces.is_empty() && result.ir().model.edges.is_empty());
    assert!(result.report().losses.iter().any(|l| l.code.category()
        == cadmpeg_ir::report::LossCategory::Topology
        && l.severity == cadmpeg_ir::report::Severity::Blocking));

    // The Parasolid stream is preserved verbatim.
    let unknowns = result.ir().native_unknowns("nx").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        result
            .source_fidelity()
            .retained_records
            .values()
            .next()
            .expect("retained record")
            .sha256()
            .len(),
        64
    );
    assert_eq!(
        unknowns[0].links,
        ["nx:s0:surf#0", "nx:s0:surf#1", "nx:s0:crv#0",]
    );
    assert_eq!(
        {
            let note =
                &result.source_fidelity().annotations.exactness()[&unknowns[0].id.to_string()];
            note.fields().get("links").copied().unwrap_or(note.entity())
        },
        Exactness::Derived
    );

    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_emits_connected_primitive_brep() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(
        result.ir().model.faces[0].loops,
        vec![result.ir().model.loops[0].id.clone()]
    );
    assert_eq!(
        result.ir().model.edges[0].curve().as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
    assert_eq!(
        result.ir().model.vertices[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.1)
    );
    assert_eq!(
        result.ir().model.edges[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.3)
    );
    assert_eq!(
        result.ir().model.faces[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.2)
    );
    assert_eq!(
        result.ir().model.coedges[0].radial_next,
        result.ir().model.coedges[0].id
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.category() != cadmpeg_ir::report::LossCategory::Topology));
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != LossKind::shared(LossTaxonomy::MaterialNotTransferred)));
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != NxLossCode::AttributeValueUnresolved.kind()));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == LossKind::shared(LossTaxonomy::AssemblyPlacementsNotTransferred)
            && loss.message.contains("Assembly occurrence placements")
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_does_not_report_assembly_placements_for_inline_external_metadata() {
    let file = prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&topology_partition_stream()),
        ),
        (
            "/Root/UG_PART/ExternalReferences",
            external_reference_stream(),
        ),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();

    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == LossKind::shared(LossTaxonomy::AssemblyPlacementsNotTransferred)));
}

#[test]
fn decode_reports_external_assembly_boundary_without_inline_geometry() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/ExternalReferences",
        external_reference_stream(),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();

    assert!(result.report().losses.iter().any(|loss| {
        loss.code == NxLossCode::AssemblyComponentsExternal.kind()
            && loss.message.contains("No inline Parasolid geometry")
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == LossKind::shared(LossTaxonomy::AssemblyPlacementsNotTransferred)));
}

mod document_metadata;

mod intersection_charts;
