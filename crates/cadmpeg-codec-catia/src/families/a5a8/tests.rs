// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `a5a8` family over synthetic byte fixtures.

#![allow(clippy::unwrap_used)]

use crate::tests::{
    a5_freeform_curve_stream, a5_guide_curve_stream, a5_pcurve_stream, a5_rational_surface_stream,
    a5_surface_stream, a6_freeform_curve_stream, a6_pcurve_stream, a6_surface_stream,
    a8_elided_surface_stream, a8_freeform_curve_stream, a8_pcurve_stream,
    a8_rational_surface_stream, a8_surface_stream, le_f64,
};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::Point3;

#[test]
fn a8_surface_parser_reads_common_form_nurbs() {
    let surfaces = crate::families::a5a8::records::a8_surfaces(&a8_surface_stream());
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].object_id, 0xdeca_fbad);
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree, surface.v_degree), (2, 2));
            assert_eq!((surface.u_count, surface.v_count), (3, 3));
            assert_eq!(surface.control_points[8].x, 8.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a8_surface_header_survives_an_opaque_pole_representation() {
    let mut bytes = a8_surface_stream();
    bytes[59..67].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::a5a8::records::a8_surfaces(&bytes).is_empty());
    let headers = crate::families::a5a8::records::a8_surface_headers(&bytes);
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].object_id, 0xdeca_fbad);
    assert_eq!((headers[0].u_degree, headers[0].v_degree), (2, 2));
    assert_eq!((headers[0].u_count, headers[0].v_count), (3, 3));
    assert_eq!(headers[0].u_multiplicities, [3, 3]);
    assert_eq!(headers[0].v_multiplicities, [3, 3]);
    assert!(!headers[0].poles_elided);
}

#[test]
fn a8_surface_header_identifies_an_elided_pole_grid() {
    let mut bytes = a8_surface_stream();
    bytes.truncate(59);
    let mut tail = vec![0; 141];
    tail[..4].copy_from_slice(&[0x05, 0x21, 0x05, 0x05]);
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0, 1, 0, 0, 0]);
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());
    assert!(crate::families::a5a8::records::a8_surfaces(&bytes).is_empty());
    let headers = crate::families::a5a8::records::a8_surface_headers(&bytes);
    assert_eq!(headers.len(), 1);
    assert!(headers[0].poles_elided);
}

#[test]
fn a8_elided_surface_resolves_one_external_pole_grid_gap() {
    let bytes = a8_elided_surface_stream();

    let [header] = crate::families::a5a8::records::a8_surface_headers(&bytes)
        .try_into()
        .expect("one elided header");
    let surface = crate::families::a5a8::records::a8_surface_from_external_grid(&bytes, &header)
        .expect("unique external pole allocation");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points.len(), 9);
    assert_eq!(surface.control_points[8], Point3::new(8.0, 2.0, 2.0));

    let [resolved] = crate::families::a5a8::records::resolved_a8_surfaces(&bytes)
        .try_into()
        .expect("one resolved surface");
    assert_eq!(resolved.object_id, 0xdeca_fbad);
    let SurfaceGeometry::Nurbs(resolved) = resolved.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(resolved.control_points, surface.control_points);
}

#[test]
fn a8_pcurve_parser_reads_degree5_uv_jet() {
    let pcurves = crate::families::a5a8::records::a8_pcurves(&a8_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(
        (pcurves[0].object_id, pcurves[0].support_id),
        (0x5678, 0x1234)
    );
    assert_eq!(pcurves[0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(pcurves[0].range, [0.0, 1.0]);
    assert_eq!(pcurves[0].mode, 0x01);
    let mut wrong_degree = a8_pcurve_stream();
    wrong_degree[15] = 17;
    assert!(crate::families::a5a8::records::a8_pcurves(&wrong_degree).is_empty());

    let mut repeated_knot = a8_pcurve_stream();
    repeated_knot[28..36].copy_from_slice(&le_f64(0.0));
    assert!(crate::families::a5a8::records::a8_pcurves(&repeated_knot).is_empty());

    let mut wrong_endpoint_multiplicity = a8_pcurve_stream();
    wrong_endpoint_multiplicity[36] = 21;
    assert!(crate::families::a5a8::records::a8_pcurves(&wrong_endpoint_multiplicity).is_empty());

    let mut trailing_byte = a8_pcurve_stream();
    trailing_byte.push(0);
    let payload_len = u32::try_from(trailing_byte.len() - 11).unwrap();
    trailing_byte[3..7].copy_from_slice(&payload_len.to_le_bytes());
    assert!(crate::families::a5a8::records::a8_pcurves(&trailing_byte).is_empty());
}

#[test]
fn a8_pcurve_parser_retains_mode_five_uv_jet() {
    let mut bytes = a8_pcurve_stream();
    bytes[39] = 0x05;
    let pcurves = crate::families::a5a8::records::a8_pcurves(&bytes);
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].mode, 0x05);
    assert_eq!(pcurves[0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
}

#[test]
fn b5_pcurve_parser_reads_degree5_uv_jet() {
    let a8 = a8_pcurve_stream();
    let payload = &a8[11..];
    let mut b5 = vec![0xb5, 0x03, 0x20, u8::try_from(payload.len()).unwrap()];
    b5.extend_from_slice(&0x5678u32.to_le_bytes());
    b5.extend_from_slice(payload);

    let pcurves = crate::families::a5a8::records::object_stream_pcurves(&b5);

    assert_eq!(pcurves.len(), 1);
    assert_eq!(
        (pcurves[0].object_id, pcurves[0].support_id),
        (0x5678, 0x1234)
    );
    assert_eq!(pcurves[0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
}

#[test]
fn b5_pcurve_parser_accepts_split_24_bit_support_reference() {
    let a8 = a8_pcurve_stream();
    let mut payload = a8[11..].to_vec();
    payload.splice(1..4, [0x28, 0x34, 0x12]);
    let mut b5 = vec![0xb5, 0x03, 0x20, u8::try_from(payload.len()).unwrap()];
    b5.extend_from_slice(&0x5678u32.to_le_bytes());
    b5.extend_from_slice(&payload);

    let pcurves = crate::families::a5a8::records::object_stream_pcurves(&b5);

    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x0012_0034);
}

#[test]
fn a5_pcurve_parser_reads_compact_support_and_uv_jet() {
    let pcurves = crate::families::a5a8::records::a5_pcurves(&a5_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x1234);
    assert_eq!(pcurves[0].extrapolation_sites, 2);
    assert_eq!(pcurves[0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(pcurves[0].range, [0.0, 1.0]);
    assert_eq!(pcurves[0].tail, [0x07]);

    let mut padded = a5_pcurve_stream();
    padded.push(0);
    let payload_len = u32::try_from(padded.len() - 8).unwrap();
    padded[3..7].copy_from_slice(&payload_len.to_le_bytes());
    assert_eq!(
        crate::families::a5a8::records::a5_pcurves(&padded)[0].tail,
        [0x07, 0]
    );

    let mut trailing = padded;
    trailing.push(1);
    let payload_len = u32::try_from(trailing.len() - 8).unwrap();
    trailing[3..7].copy_from_slice(&payload_len.to_le_bytes());
    assert!(crate::families::a5a8::records::a5_pcurves(&trailing).is_empty());
}

#[test]
fn consolidated_pcurve_parser_reads_width2_frame() {
    let pcurves = crate::families::a5a8::records::a5_pcurves(&a6_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x1234);
    assert_eq!(pcurves[0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
}

#[test]
fn a8_surface_parser_reads_rational_weight_grid() {
    let surfaces = crate::families::a5a8::records::a8_surfaces(&a8_rational_surface_stream());
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => assert_eq!(surface.weights, Some(vec![2.0; 9])),
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_surface_parser_reads_consolidated_nurbs() {
    let surfaces = crate::families::a5a8::records::a5_surfaces(&a5_surface_stream());
    assert_eq!(surfaces.len(), 1);
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
            assert_eq!((surface.u_count, surface.v_count), (2, 2));
            assert_eq!(surface.control_points[3].x, 3.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn consolidated_surface_parser_reads_width2_frame() {
    let surfaces = crate::families::a5a8::records::a5_surfaces(&a6_surface_stream());
    assert_eq!(surfaces.len(), 1);
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => assert_eq!((surface.u_count, surface.v_count), (2, 2)),
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_surface_parser_reads_rational_weight_program() {
    let surfaces = crate::families::a5a8::records::a5_surfaces(&a5_rational_surface_stream());
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => assert_eq!(surface.weights, Some(vec![2.0; 4])),
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_curve_parser_reads_degree5_rolling_ball_jet() {
    let curves = crate::families::a5a8::records::a5_freeform_curves(&a5_freeform_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].knots, vec![0.0, 1.0]);
    assert_eq!(curves[0].sites[1].radius, 2.0);

    let mut wrong_degree = a5_freeform_curve_stream();
    wrong_degree[9] = 17;
    assert!(crate::families::a5a8::records::a5_freeform_curves(&wrong_degree).is_empty());
}

#[test]
fn consolidated_curve_parser_reads_width2_frame() {
    let curves = crate::families::a5a8::records::a5_freeform_curves(&a6_freeform_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].sites[1].radius, 2.0);
}

#[test]
fn guide_curve_parser_reads_position_and_unit_direction_jet() {
    let curves = crate::families::a5a8::records::a5_guide_curves(&a5_guide_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].sites[0].point, [0.0, 0.0, 0.0]);
    assert_eq!(curves[0].sites[0].direction, [1.0, 0.0, 0.0]);
    assert_eq!(curves[0].sites[1].direction, [0.0, 1.0, 0.0]);
    let points = curves[0]
        .sites
        .iter()
        .map(|site| site.point)
        .collect::<Vec<_>>();
    let derivatives = vec![[0.0; 3]; 2];
    let (knots, controls) = crate::nurbs::quintic_jet_bspline3(
        curves[0].degree,
        &curves[0].knots,
        &points,
        &derivatives,
        &derivatives,
    )
    .expect("exact 3D quintic jet");
    assert_eq!(knots, [vec![0.0; 6], vec![1.0; 6]].concat());
    assert_eq!(controls.first(), Some(&[0.0, 0.0, 0.0]));
    assert_eq!(controls.last(), Some(&[2.0, 3.0, 4.0]));
}

#[test]
fn a8_curve_parser_reads_common_form_rolling_ball_jet() {
    let curves = crate::families::a5a8::records::a8_freeform_curves(&a8_freeform_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].object_id, 0x1234_5678);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].multiplicities, vec![6, 6]);
    assert_eq!(curves[0].sites[1].radius, 2.0);
    assert_eq!(curves[0].tail_len, 59);

    let mut repeated_knot = a8_freeform_curve_stream();
    repeated_knot[26..34].copy_from_slice(&le_f64(0.0));
    assert!(crate::families::a5a8::records::a8_freeform_curves(&repeated_knot).is_empty());

    let mut invalid_endpoint_multiplicity = a8_freeform_curve_stream();
    invalid_endpoint_multiplicity[34] = 21;
    assert!(
        crate::families::a5a8::records::a8_freeform_curves(&invalid_endpoint_multiplicity)
            .is_empty()
    );
}
