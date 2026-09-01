// SPDX-License-Identifier: Apache-2.0
//! NURBS, offset, blend, and compact-carrier surface decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::LossTaxonomy;

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn faces_decode_nurbs_surface() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(nurbs_surface_carrier(180, 181, 10));
    let bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS surface");
    assert_eq!((nurbs.u_degree, nurbs.v_degree), (1, 1));
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
    assert_eq!(nurbs.control_points.len(), 4);
}

#[test]
fn faces_decode_compact_counted_nurbs_surface_arrays() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(compact_counted_nurbs_surface_carrier(180, 181, 10));
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("compact counted NURBS surface");
    };
    assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
    assert_eq!((surface.u_count, surface.v_count), (2, 2));
    assert_eq!(surface.u_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].z, 500.0);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn conflicting_compact_counted_surface_array_is_rejected() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(compact_counted_nurbs_surface_carrier(180, 181, 10));
    body.extend(compact_f64_array(182, &[1.0; 12]));
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
}

#[test]
fn short_compact_surface_knot_array_is_rejected_without_panicking() {
    let mut bytes = compact_counted_nurbs_surface_carrier(180, 181, 10);
    let multiplicity_attr = 183u16.to_be_bytes();
    let header = bytes
        .windows(4)
        .position(|window| window == [0, 4, multiplicity_attr[0], multiplicity_attr[1]])
        .expect("u multiplicity header");
    bytes[header + 1] = 1;

    assert!(!crate::brep::spline::scan_surface_carriers(&bytes).contains_key(&180));
}

#[test]
fn faces_decode_nested_offset_surface_with_hidden_support() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    body.extend(offset_surface_carrier(180, 181, 0.002));
    body.extend(offset_surface_carrier(181, 100, 0.003));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 2);
    assert_eq!(result.ir().model.surfaces.len(), 3);
    assert!(result.ir().model.procedural_surfaces.iter().any(|surface| {
        matches!(
            surface.definition,
            ProceduralSurfaceDefinition::Offset { distance, .. }
                if (distance - 2.0).abs() < f64::EPSILON
        )
    }));
    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Plane { .. })
            && surface.id.0.contains("hidden-support-surf#100")
    }));

    let face_surface = &result.ir().model.faces[0].surface;
    let point = cadmpeg_ir::eval::model_surface_point_by_id(
        &cadmpeg_ir::index::ModelIndex::new(result.ir()),
        face_surface,
        0.0,
        0.0,
    )
    .expect("nested offset evaluation");
    assert!((point.z - 5.0).abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn blend_emits_typed_and_opaque_hidden_support_surfaces() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&blend_triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(result.ir().model.surfaces.len(), 3);
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &result.ir().model.procedural_surfaces[0].definition
    else {
        panic!("rolling-ball construction");
    };
    let support_surfaces: Vec<_> = supports
        .iter()
        .flatten()
        .map(|support| {
            result
                .ir()
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == support.surface)
                .expect("materialized blend support")
        })
        .collect();
    assert!(matches!(
        support_surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        support_surfaces[1].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    for surface in support_surfaces {
        assert!(surface.id.0.contains("hidden-support-surf#"));
    }
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("1 untyped surface carrier(s) are retained as opaque hidden supports")
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn merged_sites_retain_procedural_surface_constructions() {
    let mut source = outer_header();
    for (type_id, section) in [
        (0x20, "Contents/Config-0-Partition"),
        (0x21, "Contents/Config-1-Partition"),
    ] {
        source.extend(make_block(
            type_id,
            section,
            &parasolid_with_body(
                "partition body",
                "SCH_SW_33103_11000",
                &blend_triangle_body(),
            ),
        ));
    }

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 2);
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .all(|construction| {
            result.ir().model.surfaces.iter().any(|surface| {
                matches!(
                    &surface.geometry,
                    cadmpeg_ir::geometry::SurfaceGeometry::Procedural {
                        construction: candidate,
                    } if candidate == &construction.id
                )
            })
        }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("2 untyped surface carrier(s) are retained as opaque hidden supports")
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn cyclic_offset_surface_graph_remains_unknown() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    body.extend(offset_surface_carrier(180, 181, 0.002));
    body.extend(offset_surface_carrier(181, 180, 0.003));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.procedural_surfaces.is_empty());
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
}

#[test]
fn surface_rejects_nonzero_terminal_multiplicity() {
    let bytes =
        nurbs_surface_carrier_with_v_knot_storage(180, 181, 10, &[2, 2, 1], &[0.0, 1.0, 2.0]);
    assert!(!crate::brep::spline::scan_surface_carriers(&bytes).contains_key(&180));
}

#[test]
fn surface_descriptor_uses_terminal_array_references() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut bytes = nurbs_surface_carrier(180, 181, 10);
    let descriptor = bytes
        .windows(2)
        .position(|window| window == [0x00, 0x7e])
        .expect("surface descriptor");

    // Replace only the five terminal references. The complete fixed fields
    // remain valid, so a parser that uses any earlier descriptor window must
    // fail to recover this carrier.
    for (index, reference) in [190u16, 191, 192, 193, 194].into_iter().enumerate() {
        let at = descriptor + 34 + index * 2;
        bytes[at..at + 2].copy_from_slice(&reference.to_be_bytes());
    }
    bytes.extend(f64_array(
        0x2d,
        190,
        &[
            10.0, 0.0, 0.0, 10.0, 1.0, 0.0, 11.0, 0.0, 0.0, 11.0, 1.0, 0.0,
        ],
    ));
    bytes.extend(u16_array(191, &[2, 2]));
    bytes.extend(u16_array(192, &[2, 2]));
    bytes.extend(f64_array(0x80, 193, &[0.0, 1.0]));
    bytes.extend(f64_array(0x80, 194, &[0.0, 1.0]));

    let carrier = crate::brep::spline::scan_surface_carriers(&bytes)
        .remove(&180)
        .expect("surface carrier");
    let crate::brep::CarrierGeometry::Surface(SurfaceGeometry::Nurbs(surface)) = carrier.geometry
    else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.control_points[0].x, 10_000.0);
    assert_eq!(surface.control_points[3].y, 1_000.0);
}

#[test]
fn faces_decode_markerless_nurbs_surface_arrays() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(markerless_nurbs_surface_carrier(180, 181, 10));
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS surface");
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
}

#[test]
fn face_on_untyped_surface_keeps_topology() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let f = sldprt_with_body(&untyped_triangle(0.0));
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    let SurfaceGeometry::Unknown {
        record: Some(record),
    } = &result.ir().model.surfaces[0].geometry
    else {
        panic!("opaque surface has no replay record");
    };
    let unknowns = result.ir().native_unknowns("sldprt").unwrap();
    let retained = unknowns
        .iter()
        .find(|unknown| unknown.id == *record)
        .expect("opaque surface record");
    assert!(retained.links.contains(&result.ir().model.surfaces[0].id.0));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|l| l.code.taxonomy() == LossTaxonomy::GeometryNotTransferred));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn strict_rejects_topology_decode_resting_on_untyped_surface() {
    use cadmpeg_ir::report::{LossTaxonomy, StrictConsequence};

    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));
    let fixture = sldprt_with_body(&body);

    let salvaged = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("salvage keeps the topology decode");
    assert_eq!(salvaged.ir().model.faces.len(), 1);
    let census = salvaged
        .report()
        .losses
        .iter()
        .find(|l| l.code.taxonomy() == LossTaxonomy::GeometryNotTransferred)
        .expect("untyped support surface raises a census note");
    assert_eq!(census.strict_consequence(), StrictConsequence::Reject);

    let error = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect_err("strict refuses the untyped-surface census");
    let cadmpeg_ir::codec::DecodeFailure::StrictRejected { loss_code, .. } = &error else {
        panic!("a strict refusal is a policy class, not a container defect: {error:?}");
    };
    assert!(loss_code.starts_with("sldprt/"), "unexpected: {loss_code}");
}

#[test]
fn compact_carrier_shapes_decode() {
    use crate::brep::{parse_carrier, CarrierGeometry};
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    // Cylinder (tag 00 33, 10 f64): origin, axis, radius, refdir.
    let mut cyl = vec![0x00, 0x33];
    be16(&mut cyl, 5);
    be32(&mut cyl, 0);
    for _ in 0..5 {
        be16(&mut cyl, 0);
    }
    cyl.push(0x2b);
    for v in [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.05, 1.0, 0.0, 0.0] {
        bef64(&mut cyl, v);
    }
    match parse_carrier(&cyl, 0).unwrap().geometry {
        CarrierGeometry::Surface(SurfaceGeometry::Cylinder { radius, axis, .. }) => {
            assert_eq!(radius, 50.0); // 0.05 m ×1000
            assert_eq!(axis.z, 1.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }

    // Circle (tag 00 1f, 10 f64): radius is the tenth value.
    let mut circ = vec![0x00, 0x1f];
    be16(&mut circ, 6);
    be32(&mut circ, 0);
    for _ in 0..5 {
        be16(&mut circ, 0);
    }
    circ.push(0x2d);
    for v in [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.003] {
        bef64(&mut circ, v);
    }
    match parse_carrier(&circ, 0).unwrap().geometry {
        CarrierGeometry::Curve(CurveGeometry::Circle { radius, .. }) => assert_eq!(radius, 3.0),
        other => panic!("expected circle, got {other:?}"),
    }

    // A bad marker (not 2b/2d) rejects the candidate.
    let mut bad = cyl.clone();
    bad[2 + 2 + 4 + 10] = 0x00;
    assert!(parse_carrier(&bad, 0).is_none());
}

#[test]
fn compact_carriers_reject_zero_direction_frames() {
    use crate::brep::parse_carrier;

    let line = line_carrier(5, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    assert!(parse_carrier(&line, 0).is_none());

    let cylinder = cylinder_carrier(6, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 1.0);
    assert!(parse_carrier(&cylinder, 0).is_none());
}
