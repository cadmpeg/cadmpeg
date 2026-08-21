// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::{collections::BTreeSet, io::Cursor};

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::ids::BodyId;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::{LossCategory, LossKind, LossTaxonomy};
use cadmpeg_ir::Exactness;

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;
use crate::NxCodec;

use super::*;

#[test]
fn decode_emits_both_intersection_support_pcurves() {
    let stream = two_support_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_discards_serialized_support_uv_lane_that_misses_chart() {
    let stream =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir().model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].pcurve.is_some());
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[1].pcurve.as_ref()
    else {
        panic!("completed second support pcurve");
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(0.0, 10.0)));
    assert!(control_points.iter().all(|point| point.u == 0.0));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_retains_uncharted_intersection_without_inventing_a_range() {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = &result.ir().model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        supports,
        parameterization,
        ..
    } = &procedural.definition
    else {
        panic!("typed tolerant intersection");
    };
    assert_ne!(supports[0], supports[1]);
    assert!(parameterization.is_none());
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .expect("intersection carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Procedural { .. }));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .filter(|edge| edge.curve.as_ref() == Some(&procedural.curve))
        .all(|edge| edge.param_range.is_none()));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn terminal_plane_intersection_without_a_direct_carrier_remains_unresolved() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }
    let second_support_source = two_support_charted_intersection_curve_stream();
    let second_support = second_support_source
        .windows(4)
        .position(|window| window == [0, 50, 0, 13])
        .expect("second plane");
    stream.extend_from_slice(&second_support_source[second_support..second_support + 91]);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural = &result.ir().model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        parameterization: None,
        ..
    } = &procedural.definition
    else {
        panic!("unresolved tolerant intersection");
    };
    let edge = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(&procedural.curve))
        .expect("carrying edge");
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn terminal_cylinder_generator_without_a_direct_carrier_remains_unresolved() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }
    let mut cylinder = record(51, 99);
    put_ref(&mut cylinder, 2, 13);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [0.0, -0.001, 0.0]);
    put_vec3(&mut cylinder, 43, [1.0, 0.0, 0.0]);
    put_f64(&mut cylinder, 67, 0.001);
    put_vec3(&mut cylinder, 75, [0.0, 1.0, 0.0]);
    stream.extend(cylinder);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural = &result.ir().model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        parameterization: None,
        ..
    } = &procedural.definition
    else {
        panic!("unresolved tolerant intersection");
    };
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());

    let second_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 15])
        .expect("second endpoint");
    put_vec3(&mut stream, second_point + 16, [0.01, -0.002, 0.0]);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let cross_branch = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(matches!(
        cross_branch.ir().model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
            parameterization: None,
            ..
        }
    ));
}

#[test]
fn terminal_cone_generator_without_a_direct_carrier_remains_unresolved() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);
    for offset in [23, 25, 27] {
        put_ref(&mut stream, intersection + offset, 1);
    }
    let sin_half = 0.5;
    let cos_half = 3.0_f64.sqrt() * 0.5;
    let mut cone = record(52, 115);
    put_ref(&mut cone, 2, 13);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [-0.0005, -0.001 * cos_half, 0.0]);
    put_vec3(&mut cone, 43, [cos_half, -sin_half, 0.0]);
    put_f64(&mut cone, 67, 0.001);
    put_f64(&mut cone, 75, sin_half);
    put_f64(&mut cone, 83, cos_half);
    put_vec3(&mut cone, 91, [sin_half, cos_half, 0.0]);
    stream.extend(cone);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural = &result.ir().model.procedural_curves[0];
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
        parameterization: None,
        ..
    } = &procedural.definition
    else {
        panic!("unresolved tolerant intersection");
    };
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn terminal_sphere_and_torus_meridians_without_a_direct_carrier_remain_unresolved() {
    let terminal_stream = || {
        let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
        let intersection = stream
            .windows(4)
            .position(|window| window == [0, 38, 0, 12])
            .expect("intersection record");
        put_ref(&mut stream, intersection + 21, 13);
        for offset in [23, 25, 27] {
            put_ref(&mut stream, intersection + offset, 1);
        }
        stream
    };
    let radial_height = (0.01_f64.powi(2) - 0.005_f64.powi(2)).sqrt();
    let mut sphere = record(53, 99);
    put_ref(&mut sphere, 2, 13);
    sphere[18] = b'+';
    put_vec3(&mut sphere, 19, [0.005, -radial_height, 0.0]);
    put_f64(&mut sphere, 43, 0.01);
    put_vec3(&mut sphere, 51, [1.0, 0.0, 0.0]);
    put_vec3(&mut sphere, 75, [0.0, 1.0, 0.0]);

    let mut torus = record(54, 107);
    put_ref(&mut torus, 2, 13);
    torus[18] = b'+';
    put_vec3(&mut torus, 19, [0.005, -0.03 - radial_height, 0.0]);
    put_vec3(&mut torus, 43, [1.0, 0.0, 0.0]);
    put_f64(&mut torus, 67, 0.03);
    put_f64(&mut torus, 75, 0.01);
    put_vec3(&mut torus, 83, [0.0, 1.0, 0.0]);

    for (family, record) in [("sphere", sphere), ("torus", torus)] {
        let mut stream = terminal_stream();
        stream.extend(record);
        let result = NxCodec
            .decode(
                &mut Cursor::new(prt_with_partition(&stream)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let procedural = &result.ir().model.procedural_curves[0];
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection {
            parameterization: None,
            ..
        } = &procedural.definition
        else {
            panic!("unresolved {family} meridian");
        };
        let edge = result
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.as_ref() == Some(&procedural.curve))
            .expect("carrying edge");
        assert_eq!(edge.param_range, None);
        assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn decode_emits_inline_descriptor_intersection_witnesses() {
    let stream = inline_descriptor_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir().model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
    ));
    assert!(matches!(
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == result.ir().model.procedural_curves[0].curve)
            .expect("intersection curve")
            .geometry,
        CurveGeometry::Nurbs(_)
    ));
}

#[test]
fn decode_emits_topology_when_record_xmt_uses_extended_encoding() {
    let stream = large_xmt_headers(&topology_partition_stream());
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_maps_parasolid_tolerance_sentinel_to_none() {
    let stream = topology_with_missing_tolerances();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.vertices[0].tolerance, None);
    assert_eq!(result.ir().model.edges[0].tolerance, None);
    assert_eq!(result.ir().model.faces[0].tolerance, None);
}

#[test]
fn decode_dual_writes_inline_entity_metadata_to_annotations() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let ir = result.ir();
    let annotations = &result.source_fidelity().annotations;

    macro_rules! assert_arena_annotations {
        ($arena:expr) => {
            for entity in $arena {
                let provenance = annotations
                    .provenance
                    .get(&entity.id.to_string())
                    .expect("annotation provenance");
                assert!(annotations.streams[provenance.stream as usize].starts_with("nx:"));
                assert!(provenance.tag.is_some());
            }
        };
    }

    assert_arena_annotations!(&ir.model.bodies);
    assert_arena_annotations!(&ir.model.regions);
    assert_arena_annotations!(&ir.model.shells);
    assert_arena_annotations!(&ir.model.faces);
    assert_arena_annotations!(&ir.model.loops);
    assert_arena_annotations!(&ir.model.coedges);
    assert_arena_annotations!(&ir.model.edges);
    assert_arena_annotations!(&ir.model.vertices);
    assert_arena_annotations!(&ir.model.points);
    assert_arena_annotations!(&ir.model.surfaces);
    assert_arena_annotations!(&ir.model.curves);
    let unknowns = ir.native_unknowns("nx").unwrap();
    assert_arena_annotations!(&unknowns);

    let point_note = &annotations.exactness[&ir.model.points[0].id.to_string()];
    assert_eq!(point_note.entity, Exactness::ByteExact);
    assert_eq!(point_note.fields["position"], Exactness::Derived);
    let surface_note = &annotations.exactness[&ir.model.surfaces[0].id.to_string()];
    assert_eq!(surface_note.fields["geometry"], Exactness::Derived);
    let curve_note = &annotations.exactness[&ir.model.curves[0].id.to_string()];
    assert_eq!(curve_note.fields["geometry"], Exactness::Derived);
    for id in [
        ir.model.vertices[0].id.to_string(),
        ir.model.edges[0].id.to_string(),
        ir.model.faces[0].id.to_string(),
    ] {
        assert_eq!(
            annotations.exactness[&id].fields["tolerance"],
            Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_bspline_surface_and_curve() {
    let stream = bspline_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(surface) => Some(surface),
            _ => None,
        })
        .expect("B-spline surface");
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert!((surface.control_points[1].y - 20.0).abs() < 1e-9);
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(curve) => Some(curve),
            _ => None,
        })
        .expect("B-spline curve");
    assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(curve.control_points.len(), 2);
    assert!((curve.control_points[1].x - 20.0).abs() < 1e-9);
}

#[test]
fn decode_replaces_partition_bspline_surface_wrapper_from_deltas() {
    let partition = bspline_surface_replacement_partition_stream();
    let deltas = deltas_bspline_surface_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 30.0)
    )));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_bspline_curve_wrapper_from_deltas() {
    let partition = bspline_curve_replacement_partition_stream();
    let deltas = deltas_bspline_curve_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 10.0)
    )));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_uses_partner_fin_vertex_for_edge_endpoint() {
    let mut cur = Cursor::new(prt_with_partition(
        &partnered_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir().model.edges.first().expect("edge");
    assert_ne!(edge.start, edge.end);
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert_eq!(result.ir().model.coedges.len(), 2);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_trimmed_curve_chain() {
    let mut cur = Cursor::new(prt_with_partition(&forward_trimmed_curve_chain_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir().model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir().model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_retains_a_curve_when_its_trim_range_misses_edge_vertices() {
    let mut cur = Cursor::new(prt_with_partition(
        &mismatched_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir().model.edges.first().expect("edge");
    let carrier = edge
        .curve
        .as_ref()
        .and_then(|id| {
            result
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *id)
        })
        .expect("edge carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Line { .. }));
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_omits_overflowing_line_trim_range() {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 69, f64::MAX);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.edges[0].param_range, None);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_resolves_extended_xmt_reference_inside_edge_record() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_curve_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
}

#[test]
fn decode_tracks_extended_face_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_face_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir().model.faces[0].surface,
        result.ir().model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_tracks_extended_edge_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir().model.edges[0].curve.as_ref(),
        Some(&result.ir().model.curves[0].id)
    );
}

#[test]
fn decode_tracks_all_extended_topology_reference_shifts() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_internal_topology_references(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert_eq!(result.ir().model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir().model.points[0].position.x, 10.0);
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_tracks_fully_extended_geometry_header_shift() {
    let stream = topology_with_fully_extended_geometry_headers();
    let graph = crate::topology::Graph::parse(&stream);
    assert!(matches!(
        graph
            .get(50, 6)
            .and_then(crate::topology::Node::surface_geometry),
        Some(SurfaceGeometry::Plane { .. })
    ));
    assert!(matches!(
        graph
            .get(30, 9)
            .and_then(crate::topology::Node::curve_geometry),
        Some(CurveGeometry::Line { .. })
    ));

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir().model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
}

#[test]
fn decode_tracks_geometry_envelope_escape_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_escaped_geometry_envelopes(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir().model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_assembly_reports_external_dependency() {
    let mut cur = Cursor::new(assembly_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(!result.report().geometry_transferred);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|l| l.message.contains("assembly")));
}

#[test]
fn metadata_fallback_does_not_retain_discarded_geometry_unknown_copies() {
    let mut stream = b"PS\0\0 (partition) SCH_TEST_1_9999".to_vec();
    stream.resize(64, b'.');
    let file = prt_with_partition(&stream);
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = (stream.len() * 2) as u64;

    let result = NxCodec
        .decode(&mut Cursor::new(file), &options)
        .expect("live stream and final metadata copy fit the retained budget");

    assert!(!result.report().geometry_transferred);
    assert_eq!(result.ir().native_unknowns("nx").unwrap().len(), 1);
}

#[test]
fn decode_refuses_opaque_container_copy_when_retained_budget_is_exhausted() {
    use cadmpeg_core::decode::ResourceDimension;

    let file = prt_with_named_payloads(&[("/Root/FastLoad/Structure", vec![0x5a; 64])]);
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;

    let error = NxCodec
        .decode(&mut Cursor::new(file), &options)
        .expect_err("opaque payload copy must be budgeted");

    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.context.operation == "retain NX opaque container payload"
    ));
}

#[test]
fn decode_refuses_invalid_preview_copy_when_retained_budget_is_exhausted() {
    use cadmpeg_core::decode::ResourceDimension;

    let file = prt_with_named_payloads(&[("/Root/images/preview", vec![0x5a; 64])]);
    let mut options = DecodeOptions::default();
    options.policy.limits.max_retained_bytes = 1;

    let error = NxCodec
        .decode(&mut Cursor::new(file), &options)
        .expect_err("invalid preview copy must be budgeted");

    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.context.operation == "retain NX invalid JPEG preview"
    ));
}

#[test]
fn decode_retains_every_rmfastload_active_body() {
    let mut cur = Cursor::new(prt_with_two_active_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.faces.len(), 100);
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("rmfastload_active_body_count"))
            .map(String::as_str),
        Some("2")
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn rmfastload_membership_precedes_terminal_lineage_for_any_complete_match() {
    let first = BodyId("nx:s3:body#first".into());
    let second = BodyId("nx:s8:body#second".into());
    let selected = BTreeSet::from([first.clone()]);
    assert!(!super::rmfastload_allows_terminal_lineage(2, &selected));
    assert!(!super::rmfastload_allows_terminal_lineage(
        2,
        &BTreeSet::from([first, second]),
    ));
    assert!(super::rmfastload_allows_terminal_lineage(
        2,
        &BTreeSet::new()
    ));
    assert!(!super::rmfastload_allows_terminal_lineage(1, &selected));
}

#[test]
fn rmfastload_membership_declines_when_a_referenced_topology_entity_is_missing() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 16, 99);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(super::topology_body_node_ids(0, &graph).is_empty());
}

#[test]
fn decode_preselection_retains_skipped_rmfastload_stream_as_unknown() {
    let mut cur = Cursor::new(prt_with_two_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result
        .ir()
        .native_unknowns("nx")
        .unwrap()
        .iter()
        .any(|unknown| unknown.id.0 == "nx:container:parasolid#1"));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_resolves_all_terminal_feature_bodies_without_active_selection() {
    let file = prt_with_two_terminal_bodies();
    assert_eq!(extract_streams(&file).len(), 2);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("active_body_selector"))
            .map(String::as_str),
        Some("terminal_feature_body_lineage")
    );
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("feature_terminal_body_count"))
            .map(String::as_str),
        Some("2")
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_selects_active_shell_when_body_record_is_absent() {
    let mut cur = Cursor::new(prt_with_missing_active_body_record());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir().model.faces.len(), 50);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_keeps_bodies_when_rmfastload_overlap_is_weak() {
    let mut cur = Cursor::new(prt_with_weak_rmfastload_overlap());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir().model.bodies.len(), 2);
    assert!(result
        .ir()
        .source
        .as_ref()
        .is_none_or(|source| !source.attributes.contains_key("active_body_selector")));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("sub-body partition")));
}

#[test]
fn container_only_preserves_streams_without_geometry() {
    let mut cur = Cursor::new(single_part_prt());
    let opts = options_in(DecodeMode::Salvage, true);
    let result = NxCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report().geometry_transferred);
    assert!(result.report().container_only);
    assert_eq!(result.ir().native_unknowns("nx").unwrap().len(), 1);
    assert!(result.ir().model.points.is_empty());
}

#[test]
fn container_only_does_not_decode_bounded_object_model_records() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let opts = options_in(DecodeMode::Salvage, true);
    let result = NxCodec.decode(&mut cur, &opts).unwrap();

    assert_eq!(result.ir().model.entity_count(), 0);
    assert!(result.ir().model.features.is_empty());
    assert!(result.ir().model.sketches.is_empty());
    assert!(result
        .ir()
        .native_unknowns("nx")
        .unwrap()
        .iter()
        .any(|unknown| unknown.id.0.starts_with("nx:om-section-")));
}

#[test]
fn inspect_enumerates_streams_and_names_schema() {
    let mut cur = Cursor::new(single_part_prt());
    let summary = NxCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format, "nx");
    assert_eq!(summary.container_kind, "splmsstr");
    assert!(summary.entries.iter().any(|e| e.role == "parasolid-stream"));
    assert!(summary.notes.iter().any(|n| n.contains("partition")));
}

#[test]
fn inspect_classifies_named_container_streams() {
    let file = prt_with_named_payloads(&[
        ("/Root/FastLoad/RMFastLoad", vec![1]),
        ("/Root/FastLoad/Structure", vec![2]),
        ("/Root/FastLoad/JT", vec![3]),
        ("/Root/UG_PART/DisplayJT", vec![4]),
        ("/Root/UG_PART/ExternalReferences", vec![5]),
        ("/Root/UG_PART/LastSavedToggleInfoStream", vec![6]),
        ("/Root/images/preview", vec![7]),
        ("/Root/materialsTif/Steel", vec![8]),
        ("/Root/part/arrangements", vec![9]),
        ("/Root/part/attrs", vec![10]),
        ("/Root/qafmetadata", vec![11]),
        ("/Root/vendor/private", vec![12]),
    ]);
    let summary = NxCodec
        .inspect(&mut Cursor::new(file), &InspectOptions::default())
        .unwrap();
    let roles = summary
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.role.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(roles["/Root/FastLoad/RMFastLoad"], "active-body-index");
    assert_eq!(roles["/Root/FastLoad/Structure"], "fast-load-structure");
    assert_eq!(roles["/Root/FastLoad/JT"], "fast-load-jt");
    assert_eq!(roles["/Root/UG_PART/DisplayJT"], "display-jt");
    assert_eq!(
        roles["/Root/UG_PART/ExternalReferences"],
        "external-references"
    );
    assert_eq!(
        roles["/Root/UG_PART/LastSavedToggleInfoStream"],
        "save-toggle-info"
    );
    assert_eq!(roles["/Root/images/preview"], "preview-image");
    assert_eq!(roles["/Root/materialsTif/Steel"], "material-texture");
    assert_eq!(roles["/Root/part/arrangements"], "arrangements");
    assert_eq!(roles["/Root/part/attrs"], "part-attributes");
    assert_eq!(roles["/Root/qafmetadata"], "asset-catalog");
    assert_eq!(roles["/Root/vendor/private"], "named-opaque-stream");
}

#[test]
fn decode_retains_unsupported_named_stream_payloads() {
    let structure = b"opaque fast-load structure".to_vec();
    let fast_load_jt = b"opaque fast-load JT".to_vec();
    let toggle = b"opaque save-toggle state".to_vec();
    let vendor = b"opaque vendor stream".to_vec();
    let file = prt_with_named_payloads(&[
        ("/Root/FastLoad/Structure", structure.clone()),
        ("/Root/FastLoad/JT", fast_load_jt.clone()),
        ("/Root/UG_PART/LastSavedToggleInfoStream", toggle.clone()),
        ("/Root/vendor/private", vendor.clone()),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let unknowns = result.ir().native_unknowns("nx").unwrap();
    assert_eq!(unknowns.len(), 4);
    assert_eq!(
        result
            .source_fidelity()
            .retained_records
            .iter()
            .map(|record| record.byte_len)
            .collect::<Vec<_>>(),
        vec![
            structure.len() as u64,
            fast_load_jt.len() as u64,
            toggle.len() as u64,
            vendor.len() as u64
        ]
    );
    assert!(unknowns
        .iter()
        .all(|unknown| unknown.id.0.starts_with("nx:container-entry:opaque#")));
    for name in [
        "/Root/FastLoad/Structure",
        "/Root/FastLoad/JT",
        "/Root/UG_PART/LastSavedToggleInfoStream",
        "/Root/vendor/private",
    ] {
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(name)));
    }
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_typed_saved_toggle_stream_is_not_retained_as_opaque() {
    let member = b"0123456789abcdef0123456789abcdef:Off";
    let mut toggle = vec![1];
    toggle.extend_from_slice(&1_u32.to_le_bytes());
    toggle.extend_from_slice(&(member.len() as u16).to_le_bytes());
    toggle.extend_from_slice(member);
    toggle.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let file = prt_with_named_payloads(&[("/Root/UG_PART/LastSavedToggleInfoStream", toggle)]);

    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let namespace = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX native namespace");
    assert_eq!(namespace.arenas["saved_toggle_streams"].len(), 1);
    assert_eq!(namespace.arenas["saved_toggle_entries"].len(), 1);
    assert!(result.ir().native_unknowns("nx").unwrap().is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != crate::loss::NxLossCode::ContainerStreamOpaque.kind()));
    assert!(cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new()).is_ok());
}

#[test]
fn container_only_retains_typed_saved_toggle_payload() {
    let member = b"0123456789abcdef0123456789abcdef:On";
    let mut toggle = vec![1];
    toggle.extend_from_slice(&1_u32.to_le_bytes());
    toggle.extend_from_slice(&(member.len() as u16).to_le_bytes());
    toggle.extend_from_slice(member);
    toggle.extend_from_slice(&[1, 2, 3, 4]);
    let toggle_len = toggle.len() as u64;
    let file = prt_with_named_payloads(&[("/Root/UG_PART/LastSavedToggleInfoStream", toggle)]);

    let result = NxCodec
        .decode(
            &mut Cursor::new(file),
            &options_in(DecodeMode::Salvage, true),
        )
        .unwrap();
    assert_eq!(result.ir().native_unknowns("nx").unwrap().len(), 1);
    assert_eq!(
        result.source_fidelity().retained_records[0].byte_len,
        toggle_len
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::NxLossCode::ContainerStreamOpaque.kind()));
}

#[test]
fn design_intent_losses_distinguish_native_and_sketch_gaps() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{
        BooleanOp, ConfigurationBodies, ConfigurationId, DesignConfiguration, Feature,
        FeatureDefinition, FeatureId,
    };

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    for (ordinal, kind) in ["DELETE", "DELETE"].into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("test:feature#{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: kind.to_string(),
                parameters: Default::default(),
                properties: Default::default(),
            },
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 3,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Unresolved,
            sketch: None,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-delete".into()),
        ordinal: 10,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        },
        native_ref: None,
    });
    for (ordinal, definition) in [
        FeatureDefinition::DatumPlaneUnresolved,
        FeatureDefinition::DatumCoordinateSystemUnresolved,
        FeatureDefinition::LoftUnresolved,
        FeatureDefinition::FreeformSurfaceUnresolved,
        FeatureDefinition::LoftUnresolved,
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId(format!("test:feature#unresolved-{ordinal}")),
            ordinal: ordinal as u64 + 4,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-block".into()),
        ordinal: 9,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-sweep".into()),
        ordinal: 11,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(None),
            sections: Vec::new(),
            path: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            mode: cadmpeg_ir::features::SweepMode::Unresolved,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });
    ir.model.configurations.extend([
        DesignConfiguration {
            id: ConfigurationId("test:configuration#0".into()),
            ordinal: 0,
            active: true.into(),
            source_index: Some(0),
            name: "Model".into(),
            material: None,
            properties: Default::default(),
            parameter_overrides: Default::default(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: Default::default(),
            feature_states: Default::default(),
            native_ref: None,
        },
        DesignConfiguration {
            id: ConfigurationId("test:configuration#1".into()),
            ordinal: 1,
            active: false.into(),
            source_index: Some(1),
            name: "Arrangement".into(),
            material: None,
            properties: Default::default(),
            parameter_overrides: Default::default(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Unresolved,
            parameter_values: Default::default(),
            feature_states: Default::default(),
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 7);
    assert_eq!(losses[0].code.category(), LossCategory::DesignIntent);
    assert!(losses[0]
        .message
        .contains("10 NX feature history operation"));
    assert_eq!(losses[1].code.category(), LossCategory::DesignIntent);
    assert!(losses[1].message.contains("2 NX design configuration"));
    assert_eq!(losses[2].code.category(), LossCategory::DesignIntent);
    assert!(losses[2].message.contains("DELETE (2)"));
    assert_eq!(losses[3].code.category(), LossCategory::DesignIntent);
    assert!(losses[3].message.contains("datum coordinate system (1)"));
    assert!(losses[3].message.contains("datum plane (1)"));
    assert!(losses[3].message.contains("freeform surface (1)"));
    assert!(losses[3].message.contains("loft (2)"));
    assert_eq!(losses[4].code.category(), LossCategory::DesignIntent);
    assert!(losses[4].message.contains("block (1)"));
    assert!(losses[4].message.contains("sweep (1)"));
    assert_eq!(losses[5].code.category(), LossCategory::DesignIntent);
    assert!(losses[5].message.contains("delete body (1)"));
    assert!(losses[5].message.contains("sketch (1)"));
    assert_eq!(losses[6].code.category(), LossCategory::DesignIntent);
    assert!(losses[6].message.contains("1 NX sketch history feature"));
    assert!(losses[6].message.contains("1 have no neutral sketch graph"));

    let sketch_id = cadmpeg_ir::sketches::SketchId("test:sketch#0".into());
    ir.model.sketches.push(cadmpeg_ir::sketches::Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.features[2].definition = FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch_id),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 6);
    assert!(losses[4].message.contains("block (1)"));
    assert!(!losses[5].message.contains("sketch"));
}

#[test]
fn design_intent_losses_ignore_unresolved_suppression_outside_active_closure() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.extend([
        Feature {
            id: FeatureId("test:feature#active".into()),
            ordinal: 0,
            name: Some("active".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![body],
            definition: FeatureDefinition::DatumPoint {
                position: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                construction: None,
            },
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#inactive".into()),
            ordinal: 1,
            name: Some("inactive".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DatumPoint {
                position: cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
                construction: None,
            },
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#inactive-native".into()),
            ordinal: 2,
            name: Some("inactive-native".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "DELETE".into(),
                parameters: Default::default(),
                properties: Default::default(),
            },
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#inactive-datum-csys".into()),
            ordinal: 3,
            name: Some("inactive-datum-csys".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DatumCoordinateSystemUnresolved,
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#inactive-sketch".into()),
            ordinal: 4,
            name: Some("inactive-sketch".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Unresolved,
                sketch: None,
            },
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn design_intent_losses_do_not_scope_to_retained_base_feature_alone() {
    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.features.extend([
        Feature {
            id: FeatureId("test:feature#retained-input".into()),
            ordinal: 0,
            name: Some("Retained history input".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![body.clone()],
            definition: FeatureDefinition::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: vec![body],
                    native: "nx:segment-body-bindings".into(),
                },
            },
            native_ref: None,
        },
        Feature {
            id: FeatureId("test:feature#unresolved".into()),
            ordinal: 1,
            name: Some("unresolved".into()),
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "DELETE".into(),
                parameters: Default::default(),
                properties: Default::default(),
            },
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 2);
    assert!(losses[0]
        .message
        .contains("Suppression state remains unresolved for 1 NX feature history operation"));
    assert!(losses[1].message.contains("DELETE (1)"));
}

#[test]
fn design_intent_losses_accept_output_free_local_body_operations() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PatternKind};

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut source_properties = std::collections::BTreeMap::new();
    source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#local-pattern".into()),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties,
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved { form: None },
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("incomplete neutral construction fields"));
    assert!(losses[0].message.contains("pattern (1)"));
}

#[test]
fn design_intent_losses_accept_pattern_construction_without_body_reference() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PatternKind};

    let feature = Feature {
        id: FeatureId("test:feature#pattern-construction".into()),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved { form: None },
        },
        native_ref: None,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(feature);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("incomplete neutral construction fields"));
    assert!(losses[0].message.contains("pattern (1)"));

    ir.model.features[0]
        .source_properties
        .insert("body_reference.0".into(), "42".into());
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("output lineage is missing, duplicated"));
}

#[test]
fn design_intent_losses_accept_unbound_trim_surface_construction() {
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, PathRef, TrimRegion,
    };

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#construction-trim".into()),
        ordinal: 0,
        name: Some("TRIMMED_SH".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("TRIMMED_SH".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::TrimSurface {
            faces: FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("face".into())]),
            tool: PathRef::Edges(vec![cadmpeg_ir::ids::EdgeId("edge".into())]),
            keep: TrimRegion::Inside,
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0]
        .source_properties
        .insert("body_reference.0".into(), "42".into());
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("output lineage is missing, duplicated"));
}

#[test]
fn output_free_local_body_construction_requires_unbound_primary_body() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, PatternKind};

    let mut source_properties = std::collections::BTreeMap::new();
    source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    let mut feature = Feature {
        id: FeatureId("test:feature#local-pattern".into()),
        ordinal: 0,
        name: Some("Pattern Geometry".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties,
        source_tag: Some("Pattern Geometry".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved { form: None },
        },
        native_ref: None,
    };

    assert!(crate::decode::output_free_local_body_construction(&feature));

    feature.source_properties.remove("primary_body_reference");
    feature
        .source_properties
        .insert("body_reference.0".to_string(), "42".to_string());
    assert!(!crate::decode::output_free_local_body_construction(
        &feature
    ));

    feature.source_properties.insert(
        "primary_body_reference".to_string(),
        "reference".to_string(),
    );
    feature.source_properties.insert(
        "primary_body_segment_use".to_string(),
        "segment-use".to_string(),
    );
    assert!(!crate::decode::output_free_local_body_construction(
        &feature
    ));
}
