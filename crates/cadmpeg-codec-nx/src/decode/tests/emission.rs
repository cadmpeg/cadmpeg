// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::{LossCategory, LossKind, LossTaxonomy};
use cadmpeg_ir::Exactness;

use crate::container;
use crate::loss::NxLossCode;
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;
use crate::NxCodec;

use super::*;

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
            cadmpeg_core::CodecError::ResourceLimit(limit)
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
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::Entities
                    && limit.context.operation == "admit NX entities"
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
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let angle = std::f64::consts::FRAC_PI_6;
    let support = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0,
        ratio: 1.0,
        half_angle: angle,
    };
    let expected = 2.0;
    let axial_shift = -expected * angle.sin();
    let offset = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, axial_shift),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0 + expected * angle.cos(),
        ratio: 1.0,
        half_angle: angle,
    };

    let distance = crate::decode::analytic_surface_offset(&support, &offset).expect("offset");
    assert!((distance - expected).abs() <= 1e-12);
    let reverse = crate::decode::analytic_surface_offset(&offset, &support).expect("reverse");
    assert!((reverse + expected).abs() <= 1e-12);

    let mut lateral = offset.clone();
    let SurfaceGeometry::Cone { origin, .. } = &mut lateral else {
        unreachable!()
    };
    origin.x = 0.1;
    assert!(crate::decode::analytic_surface_offset(&support, &lateral).is_none());

    let mut shifted_parameterization = offset.clone();
    let SurfaceGeometry::Cone { origin, .. } = &mut shifted_parameterization else {
        unreachable!()
    };
    origin.z += 0.1;
    assert!(crate::decode::analytic_surface_offset(&support, &shifted_parameterization).is_none());

    let mut elliptical = offset;
    let SurfaceGeometry::Cone { ratio, .. } = &mut elliptical else {
        unreachable!()
    };
    *ratio = 0.5;
    assert!(crate::decode::analytic_surface_offset(&support, &elliptical).is_none());
}

#[test]
fn nx_sphere_offset_lineage_follows_signed_radius_orientation() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let sphere = |radius| SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(4.0), &sphere(6.5)),
        Some(2.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(-4.0), &sphere(-6.5)),
        Some(2.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(-6.5), &sphere(-4.0)),
        Some(-2.5)
    );
    assert!(crate::decode::analytic_surface_offset(&sphere(4.0), &sphere(-6.5)).is_none());
}

#[test]
fn nx_torus_offset_lineage_requires_one_ring_orientation() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let torus = |minor_radius| SurfaceGeometry::Torus {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 10.0,
        minor_radius,
    };
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(2.0), &torus(3.5)),
        Some(1.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(-2.0), &torus(-3.5)),
        Some(1.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(-3.5), &torus(-2.0)),
        Some(-1.5)
    );
    assert!(crate::decode::analytic_surface_offset(&torus(2.0), &torus(-3.5)).is_none());
    assert!(crate::decode::analytic_surface_offset(&torus(2.0), &torus(10.0)).is_none());
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
    assert!(edge.start.0.contains("closed-edge"));
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
    assert!(edge.start.0.contains("closed-edge"));
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
            .get(29, 11)
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
    let points = crate::decode::ordered_point_candidates(&stream, &graph);
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].0, first);
    assert_eq!(points[0].1, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(points[0].2.map(|node| node.xmt), Some(11));
    assert_eq!(points[1].0, stream.len() - 40);
    assert_eq!(points[1].1, cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0));
    assert_eq!(points[1].2.map(|node| node.xmt), Some(77));
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
    let surfaces = crate::decode::ordered_surface_candidates(&stream, &graph);
    assert_eq!(surfaces.len(), 2);
    assert_eq!(surfaces[0].0, first_surface);
    assert_eq!(surfaces[0].2.map(|node| node.xmt), Some(6));
    assert_eq!(surfaces[1].0, second_surface_offset);
    assert_eq!(surfaces[1].2.map(|node| node.xmt), Some(77));

    let curves = crate::decode::ordered_curve_candidates(&stream, &graph);
    assert_eq!(curves.len(), 2);
    assert_eq!(curves[0].0, first_curve);
    assert_eq!(curves[0].2.map(|node| node.xmt), Some(9));
    assert_eq!(curves[1].0, second_curve_offset);
    assert_eq!(curves[1].2.map(|node| node.xmt), Some(78));
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
    assert_eq!(result.ir().model.shells[0].free_vertices.len(), 0);
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
    assert!(matches!(surface.geometry, SurfaceGeometry::Unknown { .. }));
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
        .curve
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
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
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

    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| !matches!(curve.geometry, CurveGeometry::Unknown { .. })));
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
    assert_eq!(edge.curve, None);
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn tolerant_edge_becomes_a_two_support_procedural_intersection() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    let expected_endpoints = [&ir.model.edges[0].start, &ir.model.edges[0].end].map(|vertex_id| {
        let point_id = &ir
            .model
            .vertices
            .iter()
            .find(|vertex| &vertex.id == vertex_id)
            .expect("edge vertex")
            .point;
        ir.model
            .points
            .iter()
            .find(|point| &point.id == point_id)
            .expect("vertex point")
            .position
    });
    ir.model.edges[0].curve = None;
    ir.model.edges[0].param_range = None;
    ir.model.edges[0].tolerance = Some(0.01);
    let mut edges = std::collections::BTreeMap::new();
    edges.insert(8, edge_id.clone());
    let mut incident_coedges = ir
        .model
        .coedges
        .iter_mut()
        .filter(|coedge| coedge.edge == edge_id)
        .collect::<Vec<_>>();
    assert_eq!(incident_coedges.len(), 2);
    incident_coedges[0].id = cadmpeg_ir::ids::CoedgeId("nx:test:fin#7".into());
    incident_coedges[1].id = cadmpeg_ir::ids::CoedgeId("nx:test:fin#22".into());
    let mut stream = partnered_trimmed_topology_partition_stream();
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
    let graph = crate::topology::Graph::parse(&stream);
    let mut off_support_ir = ir.clone();
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");

    crate::decode::attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        "nx:test",
        stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.param_range, None);
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve.as_ref())
        .expect("procedural carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Procedural { .. }));
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == curve.id)
        .expect("intersection construction");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        supports,
        endpoints,
        tolerance,
        parameterization,
    } = &procedural.definition
    else {
        panic!("tolerant intersection definition");
    };
    assert_ne!(supports[0], supports[1]);
    assert_eq!(*endpoints, expected_endpoints);
    assert_eq!(*tolerance, 0.01);
    assert_eq!(*parameterization, None);

    let start = off_support_ir.model.edges[0].start.clone();
    let point_id = off_support_ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == start)
        .expect("edge vertex")
        .point
        .clone();
    let point = off_support_ir
        .model
        .points
        .iter_mut()
        .find(|point| point.id == point_id)
        .expect("vertex point");
    point.position.x += 0.5;
    point.position.y += 0.5;
    point.position.z += 0.5;
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");
    crate::decode::attach_tolerant_edge_intersections(
        &mut off_support_ir,
        &graph,
        &edges,
        "nx:test",
        stream,
        &mut annotations,
    );
    assert_eq!(off_support_ir.model.edges[0].curve, None);
}

#[test]
fn tolerant_edge_does_not_replace_a_serialized_fin_curve() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    ir.model.edges[0].curve = None;
    ir.model.edges[0].param_range = None;
    ir.model.edges[0].tolerance = Some(0.01);
    let edges = std::collections::BTreeMap::from([(8, edge_id.clone())]);
    let mut stream = partnered_trimmed_topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let graph = crate::topology::Graph::parse(&stream);
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");

    crate::decode::attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        "nx:test",
        source_stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.curve, None);
    assert_eq!(edge.param_range, None);
    assert!(ir.model.procedural_curves.is_empty());
}

#[test]
fn opposite_intersection_chart_transfers_adaptively_within_edge_tolerance() {
    let mut ir = cylinder_plane_transfer_fixture(std::f64::consts::TAU, 0.01);

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    let pcurve = context.sides[1].pcurve.as_ref().unwrap();
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve else {
        unreachable!()
    };
    assert!(control_points.len() > 2);
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter).unwrap();
        let point =
            cadmpeg_ir::eval::surface_point(&ir.model.surfaces[1].geometry, uv.u, uv.v).unwrap();
        let angle = std::f64::consts::TAU * parameter;
        assert!((point.x - 10.0 * angle.cos()).abs() < 0.01);
        assert!((point.y - 10.0 * angle.sin()).abs() < 0.01);
        assert!(point.z.abs() < 0.01);
    }
}

#[test]
fn opposite_intersection_chart_transfer_fails_closed_at_sample_budget() {
    const TIGHT_EDGE_TOLERANCE: f64 = 0.0001;

    let mut ir =
        cylinder_plane_transfer_fixture(std::f64::consts::TAU * 10_000.0, TIGHT_EDGE_TOLERANCE);
    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert!(context.sides[1].pcurve.is_none());
}

#[test]
fn opposite_intersection_blend_contact_transfers_many_candidates_within_budget() {
    const CONTACT_FIT_TOLERANCE: f64 = 1.0e-8;
    const CANDIDATE_COUNT: usize = 300;

    let source_pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let mut ir = blend_contact_transfer_fixture(
        CANDIDATE_COUNT,
        &source_pcurve,
        CONTACT_FIT_TOLERANCE,
        true,
    );

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    assert!(ir.model.procedural_curves[1..].iter().all(|procedural| {
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            return false;
        };
        context.sides[1].pcurve.is_some()
    }));
}

#[test]
fn opposite_intersection_blend_contact_keeps_adaptive_fit_certification() {
    const CONTACT_FIT_TOLERANCE: f64 = 1.0e-2;

    let source_pcurve = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(0.2, 0.0),
            Point2::new(1.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let mut ir = blend_contact_transfer_fixture(1, &source_pcurve, CONTACT_FIT_TOLERANCE, true);

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[1].definition
    else {
        unreachable!()
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[1].pcurve.as_ref()
    else {
        panic!("adaptive blend-contact transfer did not produce a pcurve")
    };
    let source_pcurve = context.sides[0].pcurve.as_ref().unwrap();
    let target_pcurve = context.sides[1].pcurve.as_ref().unwrap();
    assert!(control_points.len() > 2);
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let source_uv = cadmpeg_ir::eval::pcurve_uv(source_pcurve, parameter).unwrap();
        let target_uv = cadmpeg_ir::eval::pcurve_uv(target_pcurve, parameter).unwrap();
        assert!((source_uv.u - target_uv.u).abs() <= CONTACT_FIT_TOLERANCE);
        assert_eq!(source_uv.v, target_uv.v);
    }
}

#[test]
fn opposite_intersection_complete_blend_boundary_transfers_many_candidates_without_contact_chart() {
    const BLEND_BOUNDARY_FIT_TOLERANCE: f64 = 1.0e-8;
    const CANDIDATE_COUNT: usize = 300;

    let source_pcurve = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    let mut ir = blend_contact_transfer_fixture(
        CANDIDATE_COUNT,
        &source_pcurve,
        BLEND_BOUNDARY_FIT_TOLERANCE,
        false,
    );

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    assert!(ir.model.procedural_curves[1..].iter().all(|procedural| {
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            return false;
        };
        context.sides[1].pcurve.is_some()
    }));
}

#[test]
fn opposite_intersection_chart_transfer_scopes_to_new_procedural_curves() {
    let mut ir = cylinder_plane_transfer_fixture(std::f64::consts::TAU, 0.01);
    let mut later = ir.model.procedural_curves[0].clone();
    later.id = cadmpeg_ir::ids::ProceduralCurveId("synthetic:later-intersection".into());
    ir.model.procedural_curves.push(later);

    let transfer_budget = cadmpeg_core::decode::WorkBudget::new(
        crate::decode::pcurves::MAX_COMPLETION_TRANSFER_SAMPLES,
    );
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(
        crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );
    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts_with_budget(
        &mut ir,
        1,
        &transfer_budget,
        &geometry_budget,
    );

    let ProceduralCurveDefinition::Intersection { context: first, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert!(first.sides[1].pcurve.is_none());
    let ProceduralCurveDefinition::Intersection { context: later, .. } =
        &ir.model.procedural_curves[1].definition
    else {
        unreachable!()
    };
    assert!(later.sides[1].pcurve.is_some());
}

fn cylinder_plane_transfer_fixture(
    source_pcurve_angle: f64,
    edge_tolerance: f64,
) -> cadmpeg_ir::document::CadIr {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let source = SurfaceId("synthetic:source-cylinder".into());
    let target = SurfaceId("synthetic:target-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 10.0,
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("synthetic:intersection-curve".into());
    let construction = ProceduralCurveId("synthetic:intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(source),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(source_pcurve_angle, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(target.clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:edge".into()),
        curve: Some(curve),
        start: VertexId("synthetic:start".into()),
        end: VertexId("synthetic:end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(edge_tolerance),
    });
    ir
}

fn blend_contact_transfer_fixture(
    candidate_count: usize,
    source_pcurve: &PcurveGeometry,
    tolerance: f64,
    contact_on_source_support: bool,
) -> cadmpeg_ir::document::CadIr {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let support = SurfaceId("synthetic:blend-contact-support".into());
    let other_support = SurfaceId("synthetic:blend-contact-other-support".into());
    let offset = SurfaceId("synthetic:blend-contact-offset".into());
    let target = SurfaceId("synthetic:blend-contact-target".into());
    ir.model.surfaces.extend([
        Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: other_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: offset.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 2.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: ProceduralSurfaceId("synthetic:blend-contact-construction".into()),
            },
            source_object: None,
        },
    ]);

    let spine = CurveId("synthetic:blend-contact-spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 2.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let contact_pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: None,
        periodic: false,
    };
    let contact_surface = if contact_on_source_support {
        offset
    } else {
        other_support.clone()
    };
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("synthetic:blend-contact-spine-construction".into()),
        curve: spine.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(contact_surface),
                        pcurve_parameter_range: None,
                        pcurve: Some(contact_pcurve),
                    },
                    IntcurveSupportSide {
                        surface: Some(other_support.clone()),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("synthetic:blend-contact-construction".into()),
        surface: target.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: support.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: other_support,
                    reversed: false,
                }),
            ],
            spine: Some(spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    for index in 0..candidate_count {
        let curve = CurveId(format!("synthetic:blend-contact-curve-{index}"));
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId(format!("synthetic:blend-contact-intersection-{index}")),
            curve,
            definition: ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: [
                        IntcurveSupportSide {
                            surface: Some(support.clone()),
                            pcurve_parameter_range: None,
                            pcurve: Some(source_pcurve.clone()),
                        },
                        IntcurveSupportSide {
                            surface: Some(target.clone()),
                            pcurve_parameter_range: None,
                            pcurve: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: Some(tolerance),
        });
    }
    ir
}

#[test]
fn blend_boundary_chart_uses_the_solved_curve_when_the_source_blend_is_unevaluable() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let source = SurfaceId("synthetic:unevaluable-source-blend".into());
    let other_support = SurfaceId("synthetic:other-support".into());
    let target = SurfaceId("synthetic:target-blend".into());
    let target_construction = ProceduralSurfaceId("synthetic:target-blend-construction".into());
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        Surface {
            id: other_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: target_construction.clone(),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:target-spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: target_construction,
        surface: target.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: source.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: other_support,
                    reversed: false,
                }),
            ],
            spine: Some(spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let curve = CurveId("synthetic:solved-boundary".into());
    let construction = ProceduralCurveId("synthetic:boundary-intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(source),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(target),
                        pcurve_parameter_range: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:boundary-edge".into()),
        curve: Some(curve),
        start: VertexId("synthetic:boundary-start".into()),
        end: VertexId("synthetic:boundary-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });

    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { control_points, .. } = context.sides[1].pcurve.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(1.0, 0.0)));
}

#[test]
fn tolerant_nurbs_boundary_establishes_both_intersection_charts() {
    use cadmpeg_ir::geometry::{Curve, NurbsSurface, ProceduralCurve, Surface};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let nurbs = SurfaceId("synthetic:nurbs-boundary".into());
    let plane = SurfaceId("synthetic:boundary-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_knots: vec![0.0, 0.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count: 2,
                v_count: 2,
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 5.0, 0.0),
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(10.0, 5.0, 0.0),
                ],
                weights: None,
                u_periodic: false,
                v_periodic: false,
            }),
            source_object: None,
        },
        Surface {
            id: plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("synthetic:boundary-curve".into());
    let construction = ProceduralCurveId("synthetic:boundary-intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(10.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::TolerantIntersection {
            supports: [nurbs, plane],
            endpoints: [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            tolerance: 1.0e-8,
            parameterization: None,
        },
        cache_fit_tolerance: None,
    });
    let point_ids = [
        PointId("synthetic:p0".into()),
        PointId("synthetic:p1".into()),
    ];
    let vertex_ids = [
        VertexId("synthetic:v0".into()),
        VertexId("synthetic:v1".into()),
    ];
    ir.model.points.extend([
        Point {
            id: point_ids[0].clone(),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: point_ids[1].clone(),
            position: Point3::new(10.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: vertex_ids[0].clone(),
            point: point_ids[0].clone(),
            tolerance: Some(1.0e-8),
        },
        Vertex {
            id: vertex_ids[1].clone(),
            point: point_ids[1].clone(),
            tolerance: Some(1.0e-8),
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:boundary-edge".into()),
        curve: Some(curve),
        start: vertex_ids[0].clone(),
        end: vertex_ids[1].clone(),
        param_range: None,
        tolerance: Some(1.0e-8),
    });

    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::pcurves::complete_exact_boundary_intersection_pcurves(&mut ir, &mut annotations);

    let ProceduralCurveDefinition::TolerantIntersection {
        supports,
        parameterization: Some(parameterization),
        ..
    } = &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert_eq!(
        ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(1.0e-8)
    );
    assert_eq!(parameterization.parameter_range, [0.0, 1.0]);
    assert_eq!(ir.model.edges[0].param_range, Some([0.0, 1.0]));
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let evaluated = cadmpeg_ir::eval::model_curve_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &ir.model.procedural_curves[0].curve,
            parameter,
        )
        .expect("charted tolerant intersection evaluates");
        let inverted = cadmpeg_ir::eval::model_curve_parameter_near_point(
            &ir,
            &ir.model.procedural_curves[0].curve,
            evaluated,
            parameter,
        )
        .expect("charted tolerant intersection inverts");
        assert!((inverted - parameter).abs() < 1.0e-8);
        let points: [Point3; 2] = std::array::from_fn(|side| {
            let uv =
                cadmpeg_ir::eval::pcurve_uv(&parameterization.pcurves[side], parameter).unwrap();
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == supports[side])
                .unwrap();
            cadmpeg_ir::eval::surface_point(&surface.geometry, uv.u, uv.v).unwrap()
        });
        assert!((points[0].x - 10.0 * parameter).abs() < 1.0e-8);
        assert_eq!(evaluated, points[0]);
        assert!(
            (points[0].x - points[1].x)
                .hypot(points[0].y - points[1].y)
                .hypot(points[0].z - points[1].z)
                < 1.0e-8
        );
    }
}

#[test]
fn exact_boundary_completion_preserves_existing_cache_fit_tolerance() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first_support = SurfaceId("nx:test:boundary-plane-a".into());
    let second_support = SurfaceId("nx:test:boundary-plane-b".into());
    ir.model.surfaces.extend([
        Surface {
            id: first_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: second_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("nx:test:boundary-line".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(10.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let points = [
        (
            PointId("nx:test:boundary-point-0".into()),
            Point3::new(0.0, 0.0, 0.0),
        ),
        (
            PointId("nx:test:boundary-point-1".into()),
            Point3::new(10.0, 0.0, 0.0),
        ),
    ];
    ir.model
        .points
        .extend(points.iter().map(|(id, position)| Point {
            id: id.clone(),
            position: *position,
            source_object: None,
        }));
    let vertices = [
        VertexId("nx:test:boundary-vertex-0".into()),
        VertexId("nx:test:boundary-vertex-1".into()),
    ];
    ir.model.vertices.extend([
        Vertex {
            id: vertices[0].clone(),
            point: points[0].0.clone(),
            tolerance: None,
        },
        Vertex {
            id: vertices[1].clone(),
            point: points[1].0.clone(),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("nx:test:boundary-edge".into()),
        curve: Some(curve.clone()),
        start: vertices[0].clone(),
        end: vertices[1].clone(),
        param_range: None,
        tolerance: Some(1.0e-8),
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("nx:test:serialized-boundary".into()),
        curve,
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first_support.clone()),
                        pcurve: None,
                        pcurve_parameter_range: None,
                    },
                    IntcurveSupportSide {
                        surface: Some(second_support.clone()),
                        pcurve: None,
                        pcurve_parameter_range: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: Some(0.25),
    });

    crate::decode::pcurves::complete_exact_boundary_intersection_pcurves(
        &mut ir,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
    );

    let procedural = ir
        .model
        .procedural_curves
        .last()
        .expect("boundary construction");
    assert_eq!(procedural.cache_fit_tolerance, Some(0.25));
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        panic!("intersection construction");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
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
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &result.ir().model.pcurves[0].geometry
    else {
        panic!("expected NURBS pcurve");
    };
    assert_eq!(*degree, 1);
    assert_eq!(knots, &[0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        control_points,
        &[Point2::new(10.0, 20.0), Point2::new(10.0, 20.0)]
    );
    assert!(weights.is_none());
    assert!(!periodic);
    assert_eq!(result.ir().model.pcurves[0].fit_tolerance, Some(0.01));
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

    assert_eq!(result.ir().model.pcurves[0].parameter_range, None);
    assert_eq!(
        result.ir().model.coedges[0].pcurves[0].parameter_range,
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

    assert_eq!(result.ir().model.pcurves[0].fit_tolerance, None);
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
fn inspect_reports_bounded_nx_object_model_entities() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let summary = NxCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert!(summary.notes.iter().any(|note| {
        note == "NX object model: 1 indexed section(s), 2 bounded entity record(s)"
    }));
}

#[test]
fn decode_projects_part_attributes_to_document_attributes() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" utf8title="Material"
    utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/attrs", xml.to_vec()),
    ]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.attributes.len(), 1);
    let attribute = &result.ir().model.attributes[0];
    assert_eq!(attribute.name, "Material");
    assert_eq!(
        attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Document
    );
    assert_eq!(
        attribute.values,
        vec![cadmpeg_ir::attributes::AttributeValue::String(
            "Steel".to_string()
        )]
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_exposes_strict_nx_jpeg_preview_metadata() {
    let preview = [
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xb9,
        0x00, 0xf7, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
    ];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", preview.to_vec()),
    ]);
    let container_only_file = file.clone();
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir().source.as_ref().unwrap().attributes;
    assert_eq!(attributes["jpeg_preview_count"], "1");
    assert_eq!(attributes["jpeg_preview_0_width"], "247");
    assert_eq!(attributes["jpeg_preview_0_height"], "185");
    assert_eq!(attributes["jpeg_preview_0_precision"], "8");
    assert_eq!(attributes["jpeg_preview_0_components"], "3");
    assert_eq!(
        attributes["jpeg_preview_0_byte_len"],
        preview.len().to_string()
    );
    assert_eq!(result.ir().model.assets.len(), 1);
    let asset = &result.ir().model.assets[0];
    assert_eq!(asset.name.as_deref(), Some("preview.jpg"));
    assert_eq!(asset.media_type.as_deref(), Some("image/jpeg"));
    assert_eq!(
        asset.native_ref.as_deref(),
        Some("nx:container:jpeg-preview#0")
    );
    assert!(matches!(
        &asset.content,
        cadmpeg_ir::assets::AssetContent::Embedded { data } if data == &preview
    ));
    let container_only_result = NxCodec
        .decode(
            &mut Cursor::new(container_only_file),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        container_only_result.ir().model.assets,
        result.ir().model.assets
    );

    let mut malformed = preview;
    malformed[10..12].copy_from_slice(&16u16.to_be_bytes());
    assert!(crate::decode::jpeg_dimensions(&malformed).is_none());
    let malformed_file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", malformed.to_vec()),
    ]);
    let malformed_result = NxCodec
        .decode(&mut Cursor::new(malformed_file), &DecodeOptions::default())
        .unwrap();
    assert!(malformed_result.ir().model.assets.is_empty());
    let malformed_unknowns = malformed_result.ir().native_unknowns("nx").unwrap();
    assert!(malformed_unknowns
        .iter()
        .any(|unknown| unknown.id.0 == "nx:container:jpeg-preview#0"));
}

#[test]
fn decode_rejects_repeated_nx_arrangement_terminators_atomically() {
    let mut arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    arrangements.extend_from_slice(&[0, 0]);
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.configurations.is_empty());
}

#[test]
fn decode_transfers_point_plane_cylinder_line() {
    let mut cur = Cursor::new(single_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    // Point coordinate is scaled metres → millimetres, byte-exact.
    let p = &result.ir().model.points[0].position;
    assert!((p.x - 62.5).abs() < 1e-6 && (p.z - 12.7).abs() < 1e-6);

    // One plane, one cylinder decoded.
    let planes = result
        .ir()
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let cyls: Vec<_> = result
        .ir()
        .model
        .surfaces
        .iter()
        .filter_map(|s| match &s.geometry {
            SurfaceGeometry::Cylinder { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect();
    assert_eq!(planes, 1);
    assert_eq!(cyls.len(), 1);
    assert!((cyls[0] - 4.05).abs() < 1e-6);
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane {
            u_axis: axis,
            ..
        } if axis == Vector3::new(1.0, 0.0, 0.0)
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder {
            ref_direction: direction,
            ..
        } if direction == Vector3::new(1.0, 0.0, 0.0)
    )));

    // One line decoded, with a unit direction.
    let lines: Vec<_> = result
        .ir()
        .model
        .curves
        .iter()
        .filter(|c| matches!(c.geometry, CurveGeometry::Line { .. }))
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
        result.source_fidelity().retained_records[0].sha256.len(),
        64
    );
    assert_eq!(
        unknowns[0].links,
        ["nx:s0:surf#0", "nx:s0:surf#1", "nx:s0:crv#0",]
    );
    assert_eq!(
        result.source_fidelity().annotations.exactness[&unknowns[0].id.to_string()].fields["links"],
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
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
    assert_eq!(result.ir().model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir().model.edges[0].tolerance, Some(0.3));
    assert_eq!(result.ir().model.faces[0].tolerance, Some(0.2));
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

#[test]
fn retained_material_library_assets_do_not_imply_an_assignment_loss() {
    let file = prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&topology_partition_stream()),
        ),
        (
            "/Root/materialsTif/Steel",
            vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0],
        ),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.assets.len(), 1);
    let asset = &result.ir().model.assets[0];
    assert_eq!(asset.name.as_deref(), Some("Steel"));
    assert_eq!(asset.media_type.as_deref(), Some("image/tiff"));
    assert!(matches!(
        &asset.content,
        cadmpeg_ir::assets::AssetContent::Embedded { data }
            if data == &[b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0]
    ));
    assert_eq!(
        asset.native_ref.as_deref(),
        Some("nx:container:material-texture#0")
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != LossKind::shared(LossTaxonomy::MaterialNotTransferred)));
}
