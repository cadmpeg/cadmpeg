// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container::{self};
use crate::surface::TorusRadius2Encoding;
use crate::test_support::*;
use crate::CreoCodec;

#[test]
fn scan_discovers_typed_surface_rows() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8]);
    payload.extend_from_slice(&[8, 0x24, 4, 0xf6, 0x01, 0]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.rows.len(), 2);
    assert_eq!(scan.surfaces.rows[0].id, 7);
    assert_eq!(scan.surfaces.rows[0].type_byte, 0x22);
    assert_eq!(scan.surfaces.rows[1].id, 8);
    assert_eq!(scan.surfaces.rows[1].type_byte, 0x24);
}

#[test]
fn scan_preserves_linear_extrusion_type_variants() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x2a, 4, 0x01, 0, 8]);
    payload.extend_from_slice(&[8, 0x2c, 4, 0x01, 0, 0]);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.surfaces.rows.len(), 2);
    assert_eq!(
        scan.surfaces.rows[0].kind,
        crate::surface::SurfaceKind::Extrusion
    );
    assert_eq!(scan.surfaces.rows[0].type_byte, 0x2a);
    assert_eq!(
        scan.surfaces.rows[1].kind,
        crate::surface::SurfaceKind::Extrusion
    );
    assert_eq!(scan.surfaces.rows[1].type_byte, 0x2c);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let rows = &result.ir().native.namespace("creo").unwrap().arenas["surface_rows"];
    assert_eq!(rows[0].fields()["surface_variant"], "ruled_surface");
    assert_eq!(rows[1].fields()["surface_variant"], "tabulated_cylinder");
}

#[test]
fn scan_bounds_tabulated_cylinder_cubic_curve_replay() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        9, 0x13, 0xe2, 0x01, 0x00, 0x03, 0x18, 0xe6, 0x0f, 0xe6, 0xf8, 0x04, 0xf7, 32, 0xfb, 0xe2,
        0xf7, 36,
    ]);
    for separator in [
        vec![0x18, 0xf1, 0xf7, 32, 0xe2],
        vec![0x18, 0xe2],
        vec![0x18, 0xe2],
        vec![0x18, 0xf2, 0xf7, 37, 0xf6, 0xe3],
    ] {
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&separator);
    }
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.tabulated_cylinder_replays.len(), 1);
    let replay = &scan.curves.tabulated_cylinder_replays[0];
    assert_eq!(replay.surface_id, 7);
    assert_eq!(replay.curve_id, 9);
    assert_eq!(replay.curve_type, 0x13);
    assert_eq!(replay.degree, 3);
    assert_eq!(replay.parameter_body, [0x18, 0xe6, 0x0f, 0xe6]);
    assert_eq!(replay.control_point_ids, [32, 33, 34, 35]);
    assert_eq!(replay.successor_reference, 36);
    assert_eq!(replay.control_point_bodies[0][0], 0x46);
    assert_eq!(replay.control_point_bodies[3][8], 0x46);
    assert_eq!(replay.control_points, [Some([-3.0, 3.0]); 4]);
    assert_eq!(replay.terminal_reference, 37);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let native = &result.ir().native.namespace("creo").unwrap().arenas
        ["tabulated_cylinder_curve_replays"][0];
    assert_eq!(native.fields()["surface_id"], 7);
    assert_eq!(native.fields()["control_point_ids"][2], 34);
    assert_eq!(native.fields()["control_point_bodies"][3][8], 0x46);
    assert_eq!(native.fields()["control_points"][2][0], -3.0);
    assert_eq!(
        result.source_fidelity().annotations.provenance[native.id()]
            .tag
            .as_deref(),
        Some("tabulated_cylinder_curve_replay")
    );
    assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
}

#[test]
fn scan_bounds_surface_parameter_bodies_and_decodes_scalars() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8, 0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(&[8, 0x24, 4, 0xf6, 6, 0]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 2);
    assert_eq!(scan.surfaces.parameters[0].surface_id, 7);
    assert_eq!(scan.surfaces.parameters[0].body, vec![0x0f, 0xe4]);
    assert_eq!(scan.surfaces.parameters[0].scalar_values, vec![0.0, 1.0]);
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::CompoundClose
    );
    assert_eq!(scan.surfaces.parameters[1].surface_id, 8);
    assert_eq!(scan.surfaces.parameters[1].scalar_values, vec![3.0]);
    assert_eq!(
        scan.surfaces.parameters[1]
            .scalar_tokens
            .iter()
            .map(|token| (token.offset, token.length))
            .collect::<Vec<_>>(),
        [(0, 8)]
    );
    assert_eq!(
        scan.surfaces.parameters[1].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn scan_withholds_type24_carrier_when_eight_slot_forms_collide() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    for value in [2.0, 4.0, 0.0, 0.0, 0.0, 2.0, 2.0, 4.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert!(scan.surfaces.parameters[0]
        .positional_cylinder_frame
        .is_none());
}

#[test]
fn torus_family_does_not_shorten_unframed_negative_world_scalar() {
    let mut payload = visibgeom_payload(1, 0);
    let scalar = [0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66, 0x66, 0x66];
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&scalar);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].body, scalar);
    assert_eq!(scan.surfaces.parameters[0].scalar_tokens[0].length, 8);
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn torus_parameter_trailer_retains_typed_outline_frame() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        0x01, 0x12, 0x50, 0x50, 0x48, 0x68, 0x10, 0x48, 0x14, 0x00, 0x2d, 0x43, 0xff, 0xff, 0xff,
        0xa4, 0x41, 0x99, 0x48, 0x64, 0xf0, 0x48, 0x08, 0x00, 0x2f, 0x4a, 0x40,
    ]);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    let frame = scan.surfaces.parameters[0]
        .torus_outline_frame(0x26)
        .expect("typed torus outline frame");
    assert_eq!(
        frame.values,
        [-192.5, -5.0, -39.999_999_957_278_48, -167.5, -3.0, 52.5]
    );
    assert_eq!(frame.selector, 80);
    assert_eq!(frame.offset, 0);
    assert!(scan.surfaces.parameters[0]
        .torus_outline_frame(0x24)
        .is_none());

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let native = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(native.fields()["torus_outline_frame"]["selector"], 80);
    assert_eq!(native.fields()["torus_outline_frame"]["values"][5], 52.5);
}

#[test]
fn torus_parameter_trailer_retains_tagged_radius_overrides() {
    let cases = [
        (
            vec![
                0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff,
            ],
            0.249_999_999_951_747_04,
            0.249_999_999_951_747_04,
            TorusRadius2Encoding::Direct,
        ),
        (
            vec![
                0x18, 0x0d, 0x2a, 0xe8, 0x00, 0x00, 0x0e, 0x01, 0x29, 0xdf, 0xff,
            ],
            0.250_000_000_000_000_06,
            0.75,
            TorusRadius2Encoding::OuterRingDifference,
        ),
    ];
    for (body, expected_radius2, stored_radial_scalar, expected_encoding) in cases {
        let mut payload = visibgeom_payload(1, 0);
        payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        let data = build_prt("c", &[("VisibGeom", payload)]);
        let scan = container::scan_bytes(data.clone());

        let overrides = scan.surfaces.parameters[0]
            .torus_radius_overrides(0x26)
            .expect("tagged torus radius overrides");
        assert_eq!(overrides.radius1, 0.499_999_999_999_999_94);
        assert_eq!(overrides.radius2, expected_radius2);
        assert_eq!(overrides.radius2_encoding, expected_encoding);
        assert_eq!(overrides.offset, 0);
        assert_eq!(
            scan.surfaces.parameters[0].scalar_values,
            [stored_radial_scalar, 0.499_999_999_999_999_94]
        );
        assert!(scan.surfaces.parameters[0]
            .torus_radius_overrides(0x24)
            .is_none());

        let result = CreoCodec
            .decode(&mut Cursor::new(data), &DecodeOptions::default())
            .expect("decode");
        let native = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
        assert_eq!(
            native.fields()["torus_radius_overrides"]["radius1"],
            0.499_999_999_999_999_94
        );
        assert_eq!(
            native.fields()["torus_radius_overrides"]["radius2"],
            expected_radius2
        );
        assert_eq!(
            native.fields()["torus_radius_overrides"]["radius2_encoding"],
            match expected_encoding {
                TorusRadius2Encoding::Direct => "direct",
                TorusRadius2Encoding::OuterRingDifference => "outer_ring_difference",
            }
        );
        assert_eq!(
            result
                .report()
                .coverage()
                .get("decoded_torus_radius_override_count")
                .copied(),
            Some(1)
        );
        assert_eq!(
            result
                .report()
                .coverage()
                .get("decoded_torus_outline_extent_count")
                .copied(),
            Some(0)
        );
        assert!(result.report().losses.iter().any(|loss| {
            loss.message
                .contains("Retained 1 tagged type-26 radius override(s)")
        }));
    }
}

#[test]
fn cone_terminal_half_angle_bounds_the_parameter_body() {
    let half_angle = [0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x05];
    let expected = f64::from_be_bytes([0x3f, 0xe9, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x05]);
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x25, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0xe3, 0x18, 0xe4]);
    payload.extend_from_slice(&half_angle);
    payload.push(0xe3);
    payload.extend_from_slice(&[0xfe; 12]);
    payload.extend_from_slice(&[8, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe3]);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(
        scan.surfaces.parameters[0].body,
        [&[0xe3, 0x18, 0xe4][..], &half_angle[..]].concat()
    );
    assert_eq!(
        scan.surfaces.parameters[0].scalar_values,
        [0.0, 1.0, expected]
    );
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::CompoundClose
    );
    let override_value = scan.surfaces.parameters[0]
        .cone_half_angle_override(0x25)
        .expect("terminal cone half-angle");
    assert_eq!(override_value.radians, expected);
    assert_eq!(override_value.offset, 3);
    assert!(scan.surfaces.parameters[0]
        .cone_half_angle_override(0x26)
        .is_none());

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let native = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(
        native.fields()["cone_half_angle_override"]["radians"],
        expected
    );
    assert_eq!(native.fields()["cone_half_angle_override"]["offset"], 3);
}

#[test]
fn surface_parameter_body_ignores_compound_close_inside_scalar() {
    let mut payload = visibgeom_payload(1, 0);
    let scalar = [0x46, 0x08, 0xe3, 0, 0, 0, 0, 0];
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&scalar);
    payload.push(0xe3);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].body, scalar);
    assert_eq!(
        scan.surfaces.parameters[0].scalar_values,
        [f64::from_be_bytes([0x40, 0x08, 0xe3, 0, 0, 0, 0, 0])]
    );
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::CompoundClose
    );
}

#[test]
fn surface_parameter_body_ignores_invalid_embedded_named_marker() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x2f, 0x43, 0, 0xe0, 0xff, 0x80, 0, 0x0f]);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(
        scan.surfaces.parameters[0].body,
        [0x2f, 0x43, 0, 0xe0, 0xff, 0x80, 0, 0x0f]
    );
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn surface_parameter_body_ignores_valid_looking_header_inside_scalar() {
    let mut payload = visibgeom_payload(1, 0);
    let scalar = [0x71, 0xe0, 0x01, b'x', 0, 0, 0, 0];
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&scalar);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].body, scalar);
    assert_eq!(
        scan.surfaces.parameters[0].scalar_values,
        [f64::from_be_bytes([0x3f, 0xe0, 0x01, b'x', 0, 0, 0, 0])]
    );
    assert_eq!(
        scan.surfaces.parameters[0].boundary,
        crate::surface::SurfaceBodyBoundary::NamedRecord
    );
}

#[test]
fn scan_ignores_surface_header_candidates_inside_a_preceding_header() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0x24]);
    payload.extend_from_slice(&[0x22, 4, 0x01, 0, 0, 0xe3]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].surface_id, 7);
}

#[test]
fn scan_decodes_plane_local_system_support_frame() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 1, 0]);
    for value in [0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 1.0, 1.0, 2.0, 1.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    payload.extend_from_slice(&[
        0x18, 0xe5, // stock first in-plane direction [0, 1, 0]
        0xe4, 0x0f, 0x0f, // second in-plane direction
        0x0f, 0x0f, 0x0f, // structural zero row
    ]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 0xe4]);
    payload.push(0xe3);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.planes.local_systems.len(), 1);
    let frame = &scan.planes.local_systems[0];
    assert_eq!(frame.surface_id, 7);
    assert_eq!(frame.slots.len(), 12);
    assert_eq!(frame.origin, Some([3.0, 0.0, 1.0]));
    assert_eq!(frame.u_axis, Some([0.0, 1.0, 0.0]));
    assert_eq!(frame.normal, Some([0.0, 0.0, -1.0]));
    assert_eq!(
        frame.classification,
        crate::surface::LocalSystemClassification::Simple
    );
    assert_eq!(scan.planes.outlines.len(), 1);
    assert_eq!(scan.planes.outlines[0].origin, [0.0, 0.0, 1.0]);
    assert_eq!(scan.planes.outlines[0].normal, [0.0, 0.0, -1.0]);
    assert_eq!(scan.planes.outlines[0].u_axis, [0.0, 1.0, 0.0]);
}

#[test]
fn scan_resolves_section_scalar_cache_in_surface_rows() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0, 0x18, 0x00, 0xe3]);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.surfaces.parameters.len(), 1);
    assert_eq!(scan.surfaces.parameters[0].surface_id, 7);
    assert_eq!(scan.surfaces.parameters[0].scalar_values, vec![3.0]);
}

#[test]
fn scan_decodes_standard_and_compact_plane_envelopes() {
    let mut payload = visibgeom_payload(2, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f, 0xe4]);
    payload.push(0xe3);
    payload.extend_from_slice(&[8, 0x22, 4, 0xf6, 0, 0, 0x0e]);
    payload.extend_from_slice(&[0xe4, 0x0f, 0xe4, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f, 0xe4]);
    payload.push(0xe3);
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.planes.envelopes.len(), 2);
    let crate::surface::PlaneEnvelope::Standard {
        bounds_2d,
        corners_3d,
    } = &scan.planes.envelopes[0].envelope
    else {
        panic!("standard plane envelope");
    };
    assert_eq!(*bounds_2d, [[Some(0.0), Some(1.0)], [Some(1.0), Some(0.0)]]);
    assert_eq!(
        *corners_3d,
        [
            [Some(0.0), Some(0.0), Some(1.0)],
            [Some(1.0), Some(0.0), Some(1.0)]
        ]
    );
    let crate::surface::PlaneEnvelope::Compact { prefix, corners_3d } =
        &scan.planes.envelopes[1].envelope
    else {
        panic!("compact plane envelope");
    };
    assert_eq!(*prefix, [Some(1.0), Some(0.0), Some(1.0)]);
    assert_eq!(
        *corners_3d,
        [
            [Some(0.0), Some(0.0), Some(1.0)],
            [Some(1.0), Some(0.0), Some(1.0)]
        ]
    );
}

#[test]
fn scan_derives_named_surface_plane_from_outline_corners() {
    let mut payload = b"srf_array\0geom_id\0\x05geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\x01next_geom_ptr\0\0\
        outline\0\xf9\x02\x03"
        .to_vec();
    payload.extend_from_slice(&[0xe4, 0x0f, 0x2f, 0, 0, 0x0d, 0x0f, 0x48, 0, 0]);
    let scan = container::scan_bytes(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert_eq!(scan.planes.envelopes.len(), 1);
    assert_eq!(scan.planes.outlines.len(), 1);
    assert_eq!(scan.planes.outlines[0].surface_id, 5);
    assert_eq!(scan.planes.outlines[0].origin, [0.0, 0.0, 0.0]);
    assert_eq!(scan.planes.outlines[0].normal, [0.0, 1.0, 0.0]);
}

#[test]
fn scan_discovers_labeled_surface_namespace_row() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(
        b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\0next_geom_ptr\0\0",
    );
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan
        .surfaces
        .rows
        .iter()
        .any(|row| { row.id == 7 && row.feature_id == 4 && row.next_surface == 0 }));
}

#[test]
fn scan_withholds_named_surface_row_without_valid_discriminators() {
    for row in [
        b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04boundary_type\0\0next_geom_ptr\0\0"
            .as_slice(),
        b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\0boundary_type\0\0next_geom_ptr\0\0",
        b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\x02next_geom_ptr\0\0",
    ] {
        let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", row.to_vec())]));
        assert!(scan.surfaces.rows.is_empty());
    }
}

#[test]
fn scan_keeps_depdb_cross_section_surfaces_out_of_model_namespace() {
    let visible = b"srf_array\0\xf8\x01geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\0next_geom_ptr\0\0".to_vec();
    let cross_section = b"Sld_Xsections\0\xe3\xe0\0xsec_geom\0\xe2srf_array\0\xf8\x01geom_id\0\x09geom_type\0\x24feat_id\0\x08orient\0\x01boundary_type\0\x06next_geom_ptr\0\0".to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("VisibGeom", visible), ("Xsections", cross_section)],
    ));

    assert_eq!(scan.surfaces.rows.len(), 1);
    assert_eq!(scan.surfaces.rows[0].id, 7);
    assert_eq!(scan.surfaces.cross_section_rows.len(), 1);
    assert_eq!(scan.surfaces.cross_section_rows[0].id, 9);
    assert_eq!(scan.surfaces.cross_section_rows[0].boundary_type, 0x06);
}

#[test]
fn scan_decodes_named_surface_prototype_parameter_wrappers() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"srf_prim_ptr(cylinder)\0");
    payload.extend_from_slice(b"\xe0\x02local_sys\0\xf9\x04\x03");
    payload.extend([0xe4; 12]);
    payload.extend_from_slice(b"\xe0\x01radius\0\xe4");
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x02\x07\x08");
    payload.extend_from_slice(b"\xe0\x00i_pnts\0\xf8\x03\xf7\x80\x80\xfb");
    payload.extend_from_slice(b"\xe0\x01id\0\x0f");
    payload.extend_from_slice(b"\xe0\x01degree\0\x03");
    payload.extend_from_slice(b"\xe0\x02params\0\xf8\x04\x00\x00\x01\x01");
    payload.extend_from_slice(b"\xe0\x01flip\0\xf1\x01");
    payload.extend_from_slice(b"\xe0\x02dum_array\0\xf8\x03\x01\x02\x03\x04");
    payload.extend_from_slice(b"\xe0\x00frst_cntr_crv_hdr_ptr\0\x2f");
    payload.extend_from_slice(b"\xe0\x01trv\0\x00");
    payload.extend_from_slice(b"\xe0\x00frst_cntr_ptr\0\x30");
    payload.extend_from_slice(b"\xe0\x00envlp\0\xf7\x03");
    payload.extend_from_slice(b"\xe0\x00outline\0\xf7\x04");
    payload.extend_from_slice(b"\xe0\x00next_cntr_ptr\0\x31");
    payload.extend_from_slice(b"\xe0\x00srf_flip_dat\0\xf7\x05");
    payload.extend_from_slice(b"\xe0\x01tan_spline\0");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.surfaces.prototype_records.len(), 1);
    let prototype = &scan.surfaces.prototype_records[0];
    assert_eq!(prototype.declared_family, "cylinder");
    assert_eq!(
        prototype.family,
        crate::surface::SurfacePrototypeFamily::Cylinder
    );
    assert_eq!(
        prototype.field("local_sys").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::ScalarArray {
            dimensions: 4,
            count: 3,
            values: vec![Some(1.0); 12],
            tokens: Vec::new(),
        })
    );
    assert_eq!(
        prototype.field("radius").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::ScalarSequence(vec![
            1.0
        ]))
    );
    assert_eq!(
        prototype.field("parent_feats").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactIntArray(vec![
            7, 8
        ]))
    );
    assert_eq!(
        prototype.field("i_pnts").map(|field| &field.value),
        Some(
            &crate::surface::SurfaceNamedValue::ContiguousEntityReferences {
                start_id: 128,
                entity_ids: vec![128, 129, 130],
            }
        )
    );
    assert_eq!(
        prototype.field("id").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(15))
    );
    assert_eq!(
        prototype.field("degree").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(3))
    );
    assert_eq!(
        prototype.field("params").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactIntArray(vec![
            0, 0, 1, 1
        ]))
    );
    assert_eq!(
        prototype.field("flip").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(1))
    );
    assert_eq!(
        prototype.field("dum_array").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Opaque(vec![
            0xf8, 0x03, 0x01, 0x02, 0x03, 0x04
        ]))
    );
    assert_eq!(
        prototype.field("tan_spline").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Empty)
    );
    assert_eq!(
        prototype
            .field("frst_cntr_crv_hdr_ptr")
            .map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(47))
    );
    assert_eq!(
        prototype.field("trv").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(0))
    );
    assert_eq!(
        prototype.field("frst_cntr_ptr").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(48))
    );
    assert_eq!(
        prototype.field("envlp").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Opaque(vec![0xf7, 0x03]))
    );
    assert_eq!(
        prototype.field("outline").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Opaque(vec![0xf7, 0x04]))
    );
    assert_eq!(
        prototype.field("next_cntr_ptr").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::CompactInt(49))
    );
    assert_eq!(
        prototype.field("srf_flip_dat").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Opaque(vec![0xf7, 0x05]))
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let native = &result.ir().native.namespace("creo").unwrap().arenas["surface_prototypes"][0];
    assert_eq!(native.fields()["declared_family"], "cylinder");
    assert_eq!(native.fields()["family"], "cylinder");
    assert_eq!(native.fields()["parameters"][0]["name"], "local_sys");
    assert_eq!(
        native.fields()["parameters"][0]["value_kind"],
        "scalar_array"
    );
    assert_eq!(native.fields()["parameters"][0]["scalar_dimensions"], 4);
    assert_eq!(native.fields()["parameters"][0]["scalar_values"][0], 1.0);
    assert_eq!(native.fields()["parameters"][1]["name"], "radius");
    assert_eq!(native.fields()["parameters"][1]["body"][0], 0xe4);
    assert_eq!(native.fields()["parameters"][2]["compact_values"][0], 7);
    assert_eq!(native.fields()["parameters"][2]["compact_values"][1], 8);
    assert_eq!(native.fields()["parameters"][3]["compact_values"][0], 128);
    assert_eq!(native.fields()["parameters"][3]["compact_values"][1], 129);
    assert_eq!(native.fields()["parameters"][3]["compact_values"][2], 130);
    assert_eq!(native.fields()["parameters"][4]["name"], "id");
    assert_eq!(native.fields()["parameters"][4]["compact_values"][0], 15);
    assert_eq!(native.fields()["parameters"][5]["name"], "degree");
    assert_eq!(native.fields()["parameters"][5]["compact_values"][0], 3);
    assert_eq!(native.fields()["parameters"][6]["name"], "params");
    assert_eq!(native.fields()["parameters"][6]["compact_values"][2], 1);
    assert_eq!(native.fields()["parameters"][7]["name"], "flip");
    assert_eq!(
        native.fields()["parameters"][7]["value_kind"],
        "compact_int"
    );
    assert_eq!(native.fields()["parameters"][7]["compact_values"][0], 1);
    assert_eq!(native.fields()["parameters"][7]["body"][0], 0xf1);
    assert_eq!(native.fields()["parameters"][8]["name"], "dum_array");
    assert_eq!(native.fields()["parameters"][8]["value_kind"], "opaque");
    assert_eq!(
        native.fields()["parameters"][9]["name"],
        "frst_cntr_crv_hdr_ptr"
    );
    assert_eq!(native.fields()["parameters"][9]["compact_values"][0], 47);
    assert_eq!(native.fields()["parameters"][10]["name"], "trv");
    assert_eq!(native.fields()["parameters"][10]["compact_values"][0], 0);
    assert_eq!(native.fields()["parameters"][11]["name"], "frst_cntr_ptr");
    assert_eq!(native.fields()["parameters"][11]["compact_values"][0], 48);
    assert_eq!(native.fields()["parameters"][12]["name"], "envlp");
    assert_eq!(native.fields()["parameters"][12]["value_kind"], "opaque");
    assert_eq!(native.fields()["parameters"][13]["name"], "outline");
    assert_eq!(native.fields()["parameters"][13]["value_kind"], "opaque");
    assert_eq!(native.fields()["parameters"][14]["name"], "next_cntr_ptr");
    assert_eq!(native.fields()["parameters"][14]["compact_values"][0], 49);
    assert_eq!(native.fields()["parameters"][15]["name"], "srf_flip_dat");
    assert_eq!(native.fields()["parameters"][15]["value_kind"], "opaque");
    assert_eq!(native.fields()["parameters"][16]["name"], "tan_spline");
    assert_eq!(native.fields()["parameters"][16]["value_kind"], "empty");
    assert_eq!(
        native.fields()["parameters"][16]["body"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        result.source_fidelity().annotations.provenance[native.id()]
            .tag
            .as_deref(),
        Some("surface_prototype_record")
    );
}

#[test]
fn surface_prototype_field_rejects_duplicate_names() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"srf_prim_ptr(cylinder)\0");
    payload.extend_from_slice(b"\xe0\x01radius\0\xe4");
    payload.extend_from_slice(b"\xe0\x01radius\0\xe4");
    payload.extend_from_slice(b"\xe0\x00tan_spline\0");

    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));
    let prototype = &scan.surfaces.prototype_records[0];

    assert_eq!(
        prototype
            .parameters
            .iter()
            .filter(|field| field.name == "radius")
            .count(),
        2
    );
    assert!(prototype.field("radius").is_none());
}

#[test]
fn scan_decodes_cone_half_angle_in_its_positive_dict_lane() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"srf_prim_ptr(cone)\0");
    payload.extend_from_slice(b"\xe0\x01half_angle\0\x74\x21\xfb\x54\x44\x2d\x23");
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x01\x04");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    let prototype = scan
        .surfaces
        .prototype_records
        .iter()
        .find(|record| record.family == crate::surface::SurfacePrototypeFamily::Cone)
        .expect("cone prototype");
    assert_eq!(
        prototype.field("half_angle").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::ScalarSequence(vec![
            f64::from_be_bytes([0x3f, 0xe9, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23]),
        ]))
    );
}

#[test]
fn scan_keeps_out_of_range_cone_half_angle_opaque() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"srf_prim_ptr(cone)\0");
    payload.extend_from_slice(b"\xe0\x01half_angle\0\x8b\0\0\0\0\0\0");
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x01\x04");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    let prototype = scan
        .surfaces
        .prototype_records
        .iter()
        .find(|record| record.family == crate::surface::SurfacePrototypeFamily::Cone)
        .expect("cone prototype");
    assert_eq!(
        prototype.field("half_angle").map(|field| &field.value),
        Some(&crate::surface::SurfaceNamedValue::Opaque(vec![
            0x8b, 0, 0, 0, 0, 0, 0,
        ]))
    );
}

#[test]
fn direct_round_radii_cover_homogeneous_and_mixed_carrier_sets() {
    let direct_quarter = [
        0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff, 0xe3,
    ];
    let round = |second_trailer: &[u8]| {
        let mut geometry = visibgeom_payload(2, 0);
        geometry.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 8]);
        geometry.extend_from_slice(&direct_quarter);
        geometry.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
        geometry.extend_from_slice(second_trailer);
        let allfeatur = vec![
            4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
        ];
        build_prt(
            "c",
            &[
                ("VisibGeom", geometry),
                ("AllFeatur", allfeatur),
                ("MdlStatus", b"Round id 4\0".to_vec()),
            ],
        )
    };
    let data = round(&direct_quarter);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(radius),
            }, ..
        }] if (radius - 0.249_999_999_951_747_04).abs() < 1.0e-12)
    ));

    let cylinder_panel = [
        0x15, 0x2d, 0x2b, 0x4d, 0xd8, 0x2f, 0xd7, 0x5e, 0x1f, 0x18, 0x2d, 0x2c, 0x1a, 0xa4, 0xfc,
        0xa4, 0x2a, 0xec, 0x2f, 0x00, 0x00, 0x2d, 0x36, 0x59, 0x99, 0x99, 0x99, 0x99, 0x9a, 0x42,
        0xf7, 0x33, 0x2e, 0x03, 0x33, 0x2e, 0x37, 0xcc, 0x29, 0xf7, 0x33,
    ];
    let mut mixed_geometry = visibgeom_payload(2, 0);
    mixed_geometry.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 8]);
    mixed_geometry.extend_from_slice(&cylinder_panel);
    mixed_geometry.extend_from_slice(&[0xe3, 8, 0x26, 4, 0x01, 0, 0]);
    mixed_geometry.extend_from_slice(&direct_quarter);
    let mixed = build_prt(
        "c",
        &[
            ("VisibGeom", mixed_geometry),
            (
                "AllFeatur",
                vec![
                    4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
                ],
            ),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(mixed), &DecodeOptions::default())
        .expect("decode");
    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::UnresolvedVariable, ..
        }])
    ));

    let mut partial_geometry = visibgeom_payload(3, 0);
    partial_geometry.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 8]);
    partial_geometry.extend_from_slice(&cylinder_panel);
    partial_geometry.extend_from_slice(&[0xe3, 8, 0x26, 4, 0x01, 0, 9]);
    partial_geometry.extend_from_slice(&direct_quarter);
    partial_geometry.extend_from_slice(&[9, 0x29, 4, 0x01, 0, 0, 0xe3]);
    let partial = build_prt(
        "c",
        &[
            ("VisibGeom", partial_geometry),
            (
                "AllFeatur",
                vec![
                    4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
                ],
            ),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(partial), &DecodeOptions::default())
        .expect("decode");
    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::UnresolvedVariable, ..
        }])
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_VARIABLE_RADIUS_FILLET_FEATURE_COUNT),
        1
    );

    let conflicting = round(&[
        0x18, 0x0d, 0x29, 0xdf, 0xff, 0x7b, 0x0e, 0x29, 0xdf, 0xff, 0xe3,
    ]);
    let result = CreoCodec
        .decode(&mut Cursor::new(conflicting), &DecodeOptions::default())
        .expect("decode");
    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [group] if group.radius.is_unresolved())
    ));
}

#[test]
fn prototype_minor_radius_replays_define_a_constant_round_radius() {
    let replay = [0x18, 0x0c, 0x29, 0xc9, 0x99];
    let mut geometry = visibgeom_payload(2, 0);
    geometry.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 8]);
    geometry.extend_from_slice(&replay);
    geometry.push(0xe3);
    geometry.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
    geometry.extend_from_slice(&replay);
    geometry.push(0xe3);
    geometry.extend_from_slice(b"srf_prim_ptr(torus)\0\xe0\x02local_sys\0\xf9\x04\x03");
    for value in [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, -2.0] {
        push_generated_scalar(&mut geometry, value);
    }
    geometry.extend_from_slice(b"\xe0\x01radius1\0\xe4\xe0\x01radius2\0\x29\xc9\x99");
    geometry.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let data = build_prt(
        "c",
        &[
            ("ND:0:VisibGeom:0", geometry),
            (
                "AllFeatur",
                vec![
                    4, 0xeb, 0x04, 0, 0x10, 1, 0x80, 0x80, 0, 0xe4, 0xe3, 0xf6, 0x83, 0x91, 0xe1,
                ],
            ),
            ("MdlStatus", b"Round id 4\0".to_vec()),
        ],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_TYPE26_REPLAYED_MINOR_RADIUS_COUNT),
        2
    );
    assert!(matches!(
        result.ir().model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(radius),
            }, ..
        }] if radius.to_bits() == 0.199_999_999_999_999_98_f64.to_bits())
    ));
}
