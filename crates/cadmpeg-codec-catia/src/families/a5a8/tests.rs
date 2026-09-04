// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `a5a8` family over synthetic byte fixtures.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

use crate::test_support::*;
use crate::variant::Variant;
use crate::CatiaCodec;

#[test]
fn a8_surface_parser_reads_common_form_nurbs() {
    let surfaces = crate::families::a5a8::records::a8_surfaces(&a8_surface_stream());
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].object_id(), Some(0xdeca_fbad));
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree(), surface.v_degree()), (2, 2));
            assert_eq!((surface.u_count(), surface.v_count()), (3, 3));
            assert_eq!(surface.control_points()[8].x, 8.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn selected_nested_a8_surface_frame_decodes_without_a_flat_rescan() {
    let inner = a8_surface_stream();
    let inner_object_id = u32::from_le_bytes(inner[7..11].try_into().unwrap());
    let mut bytes = vec![0xa8, 0x03, 0x62];
    bytes.extend_from_slice(&u32::try_from(inner.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&0x1234_u32.to_le_bytes());
    let inner_start = bytes.len();
    bytes.extend_from_slice(&inner);
    let inner_end = bytes.len();

    assert!(crate::families::a5a8::records::resolved_a8_surfaces(&bytes).is_empty());
    let header = crate::families::a5a8::records::a8_surface_header_from_object_frame(
        &bytes,
        inner_start,
        inner_end,
        inner_object_id,
    )
    .expect("selected nested surface header");
    assert_eq!((header.u_count, header.v_count), (3, 3));
    let surface = crate::families::a5a8::records::resolved_a8_surface_from_object_frame(
        &bytes,
        inner_start,
        inner_end,
        inner_object_id,
    )
    .expect("selected nested surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points()[8].x, 8.0);
    assert!(
        crate::families::a5a8::records::resolved_a8_surface_from_object_frame(
            &bytes,
            inner_start,
            inner_end - 1,
            inner_object_id,
        )
        .is_none()
    );
}

#[test]
fn a8_surface_parser_accepts_frame_bounded_knot_and_pole_counts() {
    let surfaces =
        crate::families::a5a8::records::a8_surfaces(&a8_surface_stream_with_u_count(20_001));
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("NURBS surface");
    };
    assert_eq!((surface.u_count(), surface.v_count()), (20_002, 3));
    assert_eq!(surface.control_points().len(), 60_006);
}

#[test]
fn a8_surface_parser_rejects_unframed_trailing_bytes() {
    let mut bytes = a8_surface_stream();
    bytes.push(0);
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    assert!(crate::families::a5a8::records::a8_surfaces(&bytes).is_empty());
}

#[test]
fn a8_surface_parser_accepts_a_closed_nested_b5_run() {
    let a8 = a8_pcurve_stream();
    let payload = &a8[11..];
    let mut child = vec![0xb5, 0x03, 0x20, u8::try_from(payload.len()).unwrap()];
    child.extend_from_slice(&0x9abcu32.to_le_bytes());
    child.extend_from_slice(payload);

    let mut bytes = a8_surface_stream();
    bytes.extend_from_slice(&child);
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    assert_eq!(crate::families::a5a8::records::a8_surfaces(&bytes).len(), 1);
}

#[test]
fn a8_surface_parser_accepts_a_valid_tail_after_inline_poles() {
    let bytes = a8_inline_tail_surface_stream();
    let [surface] = crate::families::a5a8::records::a8_surfaces(&bytes)
        .try_into()
        .expect("one inline-tail surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points()[8].x, 8.0);
}

#[test]
fn a8_surface_parser_accepts_a_valid_tail_after_inline_weights() {
    let mut bytes = a8_rational_surface_stream();
    bytes.extend_from_slice(&a8_surface_tail());
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [surface] = crate::families::a5a8::records::a8_surfaces(&bytes)
        .try_into()
        .expect("one inline-weight-tail surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.weights(), Some([2.0; 9].as_slice()));
}

#[test]
fn a8_surface_parser_accepts_inline_continuation_tail_variants() {
    let surface_with_tail = |tail: &[u8]| {
        let mut bytes = a8_surface_stream();
        bytes.extend_from_slice(tail);
        let payload_len = u32::try_from(bytes.len() - 11).unwrap();
        bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());
        bytes
    };
    let mut finite_continuation = a8_surface_tail();
    for (index, value) in [2.0, -3.0, 5.0, 7.0, 11.0, 13.0, 17.0, 19.0]
        .into_iter()
        .enumerate()
    {
        let offset = 71 + index * 8;
        finite_continuation[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    let mut alternate_suffix = finite_continuation.clone();
    alternate_suffix[135..141].copy_from_slice(&[0x09, 0x00, 0x09, 0x00, 0x07, 0x07]);
    let mut extrapolated = alternate_suffix.clone();
    extrapolated.resize(142, 0);
    extrapolated[135..142].copy_from_slice(&[0x09, 0x00, 0x09, 0x01, 0x05, 0x07, 0x07]);
    let mut alternate_extrapolated = extrapolated.clone();
    alternate_extrapolated[68..71].copy_from_slice(&[0x05, 0x05, 0x01]);
    alternate_extrapolated[135] = 0x0d;

    for tail in [
        &finite_continuation,
        &alternate_suffix,
        &extrapolated,
        &alternate_extrapolated,
    ] {
        let bytes = surface_with_tail(tail);
        let [surface] = crate::families::a5a8::records::a8_surfaces(&bytes)
            .try_into()
            .expect("one inline-tail surface");
        assert!(matches!(surface.geometry, SurfaceGeometry::Nurbs(_)));
        let [header] = crate::families::a5a8::records::a8_surface_headers(&bytes)
            .try_into()
            .expect("one inline-tail header");
        assert_eq!(
            header.pole_storage,
            crate::families::a5a8::records::PoleStorage::Inline
        );
    }
}

#[test]
fn a8_elided_surface_requires_the_fixed_zero_continuation() {
    let mut bytes = a8_elided_surface_stream();
    let tail_start = 59;
    bytes[tail_start + 71..tail_start + 79].copy_from_slice(&le_f64(2.0));

    let [header] = crate::families::a5a8::records::a8_surface_headers(&bytes)
        .try_into()
        .expect("one parameter lattice");
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
    assert!(crate::families::a5a8::records::resolved_a8_surfaces(&bytes).is_empty());
}

#[test]
fn a8_surface_parser_rejects_a_malformed_tail_after_inline_poles() {
    let mut bytes = a8_inline_tail_surface_stream();
    let tail_start = bytes.len() - 141;
    bytes[tail_start + 68] = 0;
    assert!(crate::families::a5a8::records::a8_surfaces(&bytes).is_empty());
}

#[test]
fn a8_surface_parser_accepts_each_object_frame_flag() {
    for flag in [0x03, 0x13, 0x83] {
        let mut bytes = a8_surface_stream();
        bytes[1] = flag;
        assert_eq!(
            crate::families::a5a8::records::a8_surfaces(&bytes).len(),
            1,
            "flag {flag:#04x}"
        );
        assert_eq!(
            crate::families::a5a8::records::a8_surface_headers(&bytes).len(),
            1,
            "flag {flag:#04x}"
        );
    }

    let mut malformed = a8_surface_stream();
    malformed[1] = 0x23;
    assert!(crate::families::a5a8::records::a8_surfaces(&malformed).is_empty());
}

#[test]
fn a8_surface_header_rejects_nonfinite_and_repeated_distinct_knots() {
    for (start, value, label) in [
        (17, f64::INFINITY, "nonfinite U knot"),
        (25, 0.0, "repeated U knot"),
        (40, f64::INFINITY, "nonfinite V knot"),
        (48, 0.0, "repeated V knot"),
    ] {
        let mut bytes = a8_surface_stream();
        bytes[start..start + 8].copy_from_slice(&le_f64(value));
        assert!(
            crate::families::a5a8::records::a8_surfaces(&bytes).is_empty(),
            "{label} must not produce a resolved surface"
        );
        assert!(
            crate::families::a5a8::records::a8_surface_headers(&bytes).is_empty(),
            "{label} must not produce a surface header"
        );
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
    assert_eq!(
        headers[0].pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
}

#[test]
fn a8_surface_header_identifies_an_elided_pole_grid() {
    let bytes = a8_elided_surface_stream();
    assert!(crate::families::a5a8::records::a8_surfaces(&bytes).is_empty());
    let headers = crate::families::a5a8::records::a8_surface_headers(&bytes);
    assert_eq!(headers.len(), 1);
    assert_eq!(
        headers[0].pole_storage,
        crate::families::a5a8::records::PoleStorage::Elided
    );
}

#[test]
fn a8_surface_header_retains_an_inline_parameter_tail() {
    let headers =
        crate::families::a5a8::records::a8_surface_headers(&a8_inline_tail_surface_stream());
    let [header] = headers.as_slice() else {
        panic!("one inline-tail header");
    };
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
}

#[test]
fn a8_surface_header_rejects_an_incomplete_elided_program() {
    let mut bytes = a8_elided_surface_stream();
    bytes[59 + 44] = 1;
    let [header] = crate::families::a5a8::records::a8_surface_headers(&bytes)
        .try_into()
        .expect("one surface header");
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
    assert!(crate::families::a5a8::records::resolved_a8_surfaces(&bytes).is_empty());
}

#[test]
fn a8_elided_surface_requires_length_closed_nested_children() {
    let mut bytes = a8_elided_surface_stream();
    let payload_len = u32::from_le_bytes(bytes[3..7].try_into().unwrap());
    let a8_end = 11 + usize::try_from(payload_len).unwrap();
    let child = [0xb5, 0x03, 0x5e, 0, 2, 0, 0, 0];
    bytes.splice(a8_end..a8_end, child);
    let new_payload_len = payload_len + u32::try_from(child.len()).unwrap();
    bytes[3..7].copy_from_slice(&new_payload_len.to_le_bytes());

    let [header] = crate::families::a5a8::records::a8_surface_headers(&bytes)
        .try_into()
        .expect("one elided surface header");
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Elided
    );

    bytes[a8_end + 3] = 250;
    let [header] = crate::families::a5a8::records::a8_surface_headers(&bytes)
        .try_into()
        .expect("one surface header");
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
    assert!(crate::families::a5a8::records::resolved_a8_surfaces(&bytes).is_empty());
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
    assert_eq!(surface.control_points().len(), 9);
    assert_eq!(surface.control_points()[8], Point3::new(8.0, 2.0, 2.0));

    let [resolved] = crate::families::a5a8::records::resolved_a8_surfaces(&bytes)
        .try_into()
        .expect("one resolved surface");
    assert_eq!(resolved.object_id(), Some(100));
    let SurfaceGeometry::Nurbs(resolved) = resolved.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(resolved.control_points(), surface.control_points());
}

#[test]
fn a8_elided_surface_uses_the_pcurve_support_reference_to_disambiguate_equal_grids() {
    let first = a8_elided_surface_stream();
    let mut second = a8_elided_surface_stream();
    second[7..11].copy_from_slice(&101_u32.to_le_bytes());
    let pcurve = second
        .windows(3)
        .position(|value| value == [0xb5, 0x03, 0x21])
        .expect("second external pcurve");
    second[pcurve + 10..pcurve + 12].copy_from_slice(&101_u16.to_le_bytes());

    let mut bytes = first;
    bytes.extend(second);
    let headers = crate::families::a5a8::records::a8_surface_headers(&bytes);
    assert_eq!(headers.len(), 2);
    assert_eq!(
        headers
            .iter()
            .map(|header| header.object_id)
            .collect::<Vec<_>>(),
        [100, 101]
    );
    for header in &headers {
        let surface = crate::families::a5a8::records::a8_surface_from_external_grid(&bytes, header)
            .expect("support reference selects one equal-sized grid");
        assert_eq!(surface.object_id(), Some(header.object_id));
    }
}

#[test]
fn a8_elided_surface_accepts_all_child_frame_flags() {
    for a8_flag in [0x03, 0x13, 0x83] {
        for child_flag in [0x03, 0x13, 0x83] {
            let mut bytes = a8_elided_surface_stream();
            bytes[1] = a8_flag;
            let pcurve = bytes
                .windows(3)
                .position(|value| value == [0xb5, 0x03, 0x21])
                .expect("external pcurve");
            bytes[pcurve + 1] = child_flag;
            let successor = bytes
                .windows(3)
                .rposition(|value| value == [0xb5, 0x03, 0x5e])
                .expect("successor frame");
            bytes[successor + 1] = child_flag;

            assert_eq!(
                crate::families::a5a8::records::resolved_a8_surfaces(&bytes).len(),
                1,
                "a8 flag {a8_flag:#04x}, child flag {child_flag:#04x}"
            );
        }
    }
}

#[test]
fn a8_elided_surface_accepts_finite_large_external_poles() {
    let mut bytes = a8_elided_surface_stream();
    let frame = bytes
        .windows(3)
        .position(|value| value == [0xb5, 0x03, 0x21])
        .expect("external pole allocation anchor");
    let pole_start = frame + 8 + usize::from(bytes[frame + 3]);
    bytes[pole_start..pole_start + 8].copy_from_slice(&le_f64(2e12));

    let [resolved] = crate::families::a5a8::records::resolved_a8_surfaces(&bytes)
        .try_into()
        .expect("one resolved surface");
    let SurfaceGeometry::Nurbs(surface) = resolved.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points()[0].x, 2e12);

    bytes[pole_start..pole_start + 8].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::families::a5a8::records::resolved_a8_surfaces(&bytes).is_empty());
}

#[test]
fn a8_elided_surface_requires_a_length_closed_successor_frame() {
    let mut bytes = a8_elided_surface_stream();
    let successor = bytes
        .windows(3)
        .rposition(|value| value == [0xb5, 0x03, 0x5e])
        .expect("successor frame");
    bytes[successor + 3] = 3;
    assert!(crate::families::a5a8::records::resolved_a8_surfaces(&bytes).is_empty());
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
fn a8_pcurve_parser_accepts_frame_bounded_site_count() {
    let pcurves = crate::families::a5a8::records::a8_pcurves(&a8_pcurve_stream_with_count(8193));
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].knots.len(), 8193);
    assert_eq!(pcurves[0].points.len(), 8193);
}

#[test]
fn a8_pcurve_parser_accepts_finite_large_jet_values() {
    let mut bytes = a8_pcurve_stream();
    bytes[40..48].copy_from_slice(&le_f64(2e12));
    let [pcurve] = crate::families::a5a8::records::a8_pcurves(&bytes)
        .try_into()
        .expect("one pcurve");
    assert_eq!(pcurve.points[0][0], 2e12);

    bytes[40..48].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::families::a5a8::records::a8_pcurves(&bytes).is_empty());
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
fn object_stream_pcurve_parser_accepts_each_object_frame_flag() {
    let a8 = a8_pcurve_stream();
    let payload = &a8[11..];
    for flag in [0x03, 0x13, 0x83] {
        let mut stream = vec![
            0xa8,
            flag,
            0x20,
            u8::try_from(payload.len()).unwrap(),
            0,
            0,
            0,
        ];
        stream.extend_from_slice(&0x5678u32.to_le_bytes());
        stream.extend_from_slice(payload);
        let [pcurve] = crate::families::a5a8::records::object_stream_pcurves(&stream)
            .try_into()
            .expect("one pcurve");
        assert_eq!(pcurve.object_id, 0x5678);
    }

    let mut malformed = a8;
    malformed[1] = 0x23;
    assert!(crate::families::a5a8::records::object_stream_pcurves(&malformed).is_empty());
}

#[test]
fn object_stream_pcurve_parser_walks_nested_b5_records_inside_a8() {
    let a8 = a8_pcurve_stream();
    let payload = &a8[11..];
    let mut child = vec![0xb5, 0x03, 0x20, u8::try_from(payload.len()).unwrap()];
    child.extend_from_slice(&0x9abcu32.to_le_bytes());
    child.extend_from_slice(payload);

    let mut wrapper = a8_surface_stream();
    wrapper.extend_from_slice(&child);
    let payload_len = u32::try_from(wrapper.len() - 11).unwrap();
    wrapper[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [pcurve] = crate::families::a5a8::records::object_stream_pcurves(&wrapper)
        .try_into()
        .expect("one nested pcurve");
    assert_eq!(pcurve.object_id, 0x9abc);
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
fn a5_pcurve_parser_accepts_frame_bounded_site_count() {
    let pcurves = crate::families::a5a8::records::a5_pcurves(&a5_pcurve_stream_with_count(4097));
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].knots.len(), 4097);
    assert_eq!(pcurves[0].points.len(), 4097);
}

#[test]
fn a8_surface_parser_reads_rational_weight_grid() {
    let surfaces = crate::families::a5a8::records::a8_surfaces(&a8_rational_surface_stream());
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!(surface.weights(), Some([2.0; 9].as_slice()));
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn surface_parsers_require_finite_nonzero_weights() {
    let mut a5 = a5_rational_surface_stream();
    a5[146..154].copy_from_slice(&le_f64(2e12));
    let [surface] = crate::families::a5a8::records::a5_surfaces(&a5)
        .try_into()
        .expect("one consolidated rational surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.weights().expect("weights")[0], 2e12);
    a5[146..154].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::families::a5a8::records::a5_surfaces(&a5).is_empty());

    let mut a8 = a8_rational_surface_stream();
    a8[275..283].copy_from_slice(&le_f64(2e12));
    let [surface] = crate::families::a5a8::records::a8_surfaces(&a8)
        .try_into()
        .expect("one common-form rational surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.weights().expect("weights")[0], 2e12);
    a8[275..283].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::families::a5a8::records::a8_surfaces(&a8).is_empty());
}

#[test]
fn a5_surface_parser_reads_consolidated_nurbs() {
    use crate::families::a5a8::records::FreeformSurfaceIdentity;

    let surfaces = crate::families::a5a8::records::a5_surfaces(&a5_surface_stream());
    assert_eq!(surfaces.len(), 1);
    assert_eq!(
        surfaces[0].identity,
        FreeformSurfaceIdentity::FrameOffset(surfaces[0].pos)
    );
    assert_eq!(surfaces[0].object_id(), None);
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree(), surface.v_degree()), (1, 1));
            assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
            assert_eq!(surface.control_points()[3].x, 3.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_surface_parser_reads_multispan_cubic_nurbs() {
    let mut bytes = vec![0xa5, 0x03, 0x34];
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0x05);
    for offset in [0.0, 10.0] {
        bytes.extend_from_slice(&[0x0d, 0x0d, 0x0c]);
        bytes.extend(
            [offset, offset + 1.0, offset + 2.0]
                .into_iter()
                .flat_map(le_f64),
        );
    }
    bytes.push(0x01);
    for pole in 0..25 {
        bytes.extend(
            [f64::from(pole), f64::from(pole % 5), f64::from(pole / 5)]
                .into_iter()
                .flat_map(le_f64),
        );
    }
    bytes.extend_from_slice(&a5_surface_tail());
    let payload_len = u32::try_from(bytes.len() - 8).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [surface] = crate::families::a5a8::records::a5_surfaces(&bytes)
        .try_into()
        .expect("one multispan cubic surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (3, 3));
    assert_eq!((surface.u_count(), surface.v_count()), (5, 5));
    assert_eq!(
        surface.u_knots(),
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0, 2.0]
    );
    assert_eq!(surface.control_points().len(), 25);
}

#[test]
fn surface_parsers_accept_finite_large_control_points() {
    let mut a5 = a5_surface_stream();
    a5[47..55].copy_from_slice(&le_f64(2e12));
    let [surface] = crate::families::a5a8::records::a5_surfaces(&a5)
        .try_into()
        .expect("one consolidated surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points()[0].x, 2e12);

    let mut a8 = a8_surface_stream();
    a8[59..67].copy_from_slice(&le_f64(2e12));
    let [surface] = crate::families::a5a8::records::a8_surfaces(&a8)
        .try_into()
        .expect("one common-form surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points()[0].x, 2e12);

    a5[47..55].copy_from_slice(&le_f64(f64::NAN));
    a8[59..67].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::families::a5a8::records::a5_surfaces(&a5).is_empty());
    assert!(crate::families::a5a8::records::a8_surfaces(&a8).is_empty());
}

#[test]
fn a5_surface_parser_rejects_nonfinite_and_repeated_distinct_knots() {
    let mut nonfinite_u = a5_surface_stream();
    nonfinite_u[11..19].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::families::a5a8::records::a5_surfaces(&nonfinite_u).is_empty());

    let mut repeated_u = a5_surface_stream();
    repeated_u[19..27].copy_from_slice(&le_f64(0.0));
    assert!(crate::families::a5a8::records::a5_surfaces(&repeated_u).is_empty());

    let mut nonfinite_v = a5_surface_stream();
    nonfinite_v[30..38].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::families::a5a8::records::a5_surfaces(&nonfinite_v).is_empty());
}

#[test]
fn consolidated_surface_parser_reads_width2_frame() {
    let surfaces = crate::families::a5a8::records::a5_surfaces(&a6_surface_stream());
    assert_eq!(surfaces.len(), 1);
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_surface_parser_reads_rational_weight_program() {
    let surfaces = crate::families::a5a8::records::a5_surfaces(&a5_rational_surface_stream());
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!(surface.weights(), Some([2.0; 4].as_slice()));
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_surface_parser_rejects_zero_tail_codes_without_underflow() {
    for index in [1, 3] {
        let mut malformed = a5_surface_stream();
        let tail = malformed
            .windows(4)
            .position(|window| window == [0x05, 0x05, 0x05, 0x05])
            .expect("surface tail");
        malformed[tail + index] = 0;
        assert!(crate::families::a5a8::records::a5_surfaces(&malformed).is_empty());
    }
}

#[test]
fn a5_surface_parser_rejects_untagged_int_bytes_without_underflow() {
    let bytes = [
        0xa5, 0xa5, 0x03, 0x34, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0xb3, 0xa5, 0xa5, 0xb3, 0xb3, 0xa5,
    ];
    assert!(crate::families::a5a8::records::a5_surfaces(&bytes).is_empty());
}

#[test]
fn a5_surface_parser_accepts_each_structured_tail_variant() {
    for tail in [
        a5_surface_short_tail(),
        a5_surface_tail(),
        a5_surface_extrapolated_short_tail(),
        a5_surface_extrapolated_tail(),
    ] {
        let surfaces =
            crate::families::a5a8::records::a5_surfaces(&a5_surface_stream_with_tail(&tail));
        assert_eq!(surfaces.len(), 1, "tail length {}", tail.len());
    }
}

#[test]
fn a5_surface_parser_rejects_unclosed_or_nonfinite_tail_data() {
    let mut trailing = a5_surface_stream();
    trailing.push(0);
    let payload_len = u32::try_from(trailing.len() - 8).unwrap();
    trailing[3..7].copy_from_slice(&payload_len.to_le_bytes());
    assert!(crate::families::a5a8::records::a5_surfaces(&trailing).is_empty());

    let mut nonfinite = a5_surface_stream();
    let tail = nonfinite
        .windows(4)
        .position(|window| window == [0x05, 0x05, 0x05, 0x05])
        .expect("surface tail");
    nonfinite[tail + 4..tail + 12].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::families::a5a8::records::a5_surfaces(&nonfinite).is_empty());
}

#[test]
fn a5_weight_program_reads_independent_palindromic_rows() {
    let mut bytes = Vec::new();
    for seed in [[1.0, 0.8], [0.9, 0.65]] {
        bytes.extend_from_slice(&[0x01, 0x03, 0x00]);
        bytes.extend(seed.into_iter().flat_map(le_f64));
    }
    bytes.push(0x02);
    bytes.extend_from_slice(&[0x01, 0x03, 0x00]);
    bytes.extend([1.0, 0.8].into_iter().flat_map(le_f64));
    let mut at = 0;
    assert_eq!(
        crate::families::a5a8::records::a5_weights(&bytes, &mut at, 4, 4, bytes.len()),
        Some(vec![
            1.0, 0.8, 0.8, 1.0, 0.9, 0.65, 0.65, 0.9, 0.9, 0.65, 0.65, 0.9, 1.0, 0.8, 0.8, 1.0,
        ])
    );
    assert_eq!(at, bytes.len());
}

#[test]
fn a5_weight_program_reads_zero_prefixed_complete_grid() {
    let expected = [
        1.0, 0.72, 1.31, 0.93, 0.84, 1.19, 0.67, 1.42, 1.27, 0.76, 1.08, 0.88, 0.69, 1.36, 0.81,
        1.14,
    ];
    let mut bytes = vec![0x00];
    bytes.extend(expected.into_iter().flat_map(le_f64));
    let mut at = 0;
    assert_eq!(
        crate::families::a5a8::records::a5_weights(&bytes, &mut at, 4, 4, bytes.len()),
        Some(expected.to_vec())
    );
    assert_eq!(at, bytes.len());
}

#[test]
fn a5_weight_program_does_not_cross_frame_boundary() {
    let mut bytes = vec![0x00];
    bytes.extend([1.0, 2.0, 3.0, 4.0].into_iter().flat_map(le_f64));
    bytes.extend([0u8; 8]);
    let mut at = 0;
    assert!(
        crate::families::a5a8::records::a5_weights(&bytes, &mut at, 2, 2, 1 + 3 * 8,).is_none()
    );
}

#[test]
fn a5_cubic_two_site_knots_are_clamped() {
    assert_eq!(
        crate::families::a5a8::records::a5_knots(&[0.0, 4.0], 3),
        Some((vec![0.0, 0.0, 0.0, 0.0, 4.0, 4.0, 4.0, 4.0], 4))
    );
}

#[test]
fn a5_curve_parser_reads_degree5_rolling_ball_jet() {
    for header_token in [5, 9, 13, 29, 17] {
        let mut bytes = a5_freeform_curve_stream();
        bytes[7] = header_token;
        let curves = crate::families::a5a8::records::a5_freeform_curves(&bytes);
        assert_eq!(curves.len(), 1);
        assert_eq!(curves[0].header_token, u32::from(header_token));
        assert_eq!(curves[0].degree, 5);
        assert_eq!(curves[0].knots, vec![0.0, 1.0]);
        assert_eq!(curves[0].sites[1].radius, 2.0);
    }

    let mut wrong_degree = a5_freeform_curve_stream();
    wrong_degree[9] = 17;
    assert!(crate::families::a5a8::records::a5_freeform_curves(&wrong_degree).is_empty());

    let mut invalid_header_token = a5_freeform_curve_stream();
    invalid_header_token[7] = 18;
    assert!(crate::families::a5a8::records::a5_freeform_curves(&invalid_header_token).is_empty());
}

#[test]
fn a5_curve_parser_accepts_compact_array_marker_values() {
    let mut bytes = a5_freeform_curve_stream();
    bytes[7] = 17;
    bytes[11] = 0x08;
    bytes.insert(12, 0x11);
    let payload_len = u32::try_from(bytes.len() - 8).expect("test frame fits u32");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [curve] = crate::families::a5a8::records::a5_freeform_curves(&bytes)
        .try_into()
        .expect("one compact-marker rolling-ball jet");
    assert_eq!(curve.header_token, 17);
    assert_eq!(curve.sites.len(), 2);

    let mut invalid_marker = bytes;
    invalid_marker[12] = 0x12;
    assert!(crate::families::a5a8::records::a5_freeform_curves(&invalid_marker).is_empty());
}

#[test]
fn a5_curve_parser_accepts_frame_bounded_continuation() {
    let mut bytes = a5_freeform_curve_stream();
    bytes.extend(std::iter::repeat_n(0, 4097));
    let payload_len = u32::try_from(bytes.len() - 8).expect("test frame fits u32");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [curve] = crate::families::a5a8::records::a5_freeform_curves(&bytes)
        .try_into()
        .expect("one rolling-ball jet");
    assert_eq!(curve.knots, [0.0, 1.0]);
    assert_eq!(curve.sites[1].radius, 2.0);
}

#[test]
fn a5_curve_parser_accepts_frame_bounded_site_count() {
    let curves = crate::families::a5a8::records::a5_freeform_curves(
        &a5_freeform_curve_stream_with_count(4097),
    );
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].knots.len(), 4097);
    assert_eq!(curves[0].sites.len(), 4097);
}

#[test]
fn rolling_ball_limit_curves_reproduce_stored_endpoint_sites() {
    let [jet] = crate::families::a5a8::records::a5_freeform_curves(&a5_freeform_curve_stream())
        .try_into()
        .expect("one rolling-ball jet");
    for second_limit in [false, true] {
        let curve = crate::families::a5a8::records::rolling_ball_limit_curve(&jet, second_limit)
            .expect("exact limiting curve");
        let geometry = CurveGeometry::Nurbs(curve);
        let expected = [jet.sites.first().unwrap(), jet.sites.last().unwrap()].map(|site| {
            let point = if second_limit {
                site.limit2
            } else {
                site.limit1
            };
            Point3::new(point[0], point[1], point[2])
        });
        assert_eq!(
            cadmpeg_ir::eval::curve_point(&geometry, jet.knots[0]),
            Some(expected[0])
        );
        assert_eq!(
            cadmpeg_ir::eval::curve_point(&geometry, jet.knots[1]),
            Some(expected[1])
        );
    }
}

#[test]
fn rolling_ball_parsers_accept_finite_nonzero_radii() {
    for radius in [1e-200, 1e200, 1e308] {
        let mut a5 = a5_freeform_curve_stream();
        a5[28..36].copy_from_slice(&le_f64(radius));
        a5[60..68].copy_from_slice(&le_f64(radius));
        let [curve] = crate::families::a5a8::records::a5_freeform_curves(&a5)
            .try_into()
            .expect("one consolidated rolling-ball jet");
        assert_eq!(curve.sites[0].radius, radius);

        let mut a8 = a8_freeform_curve_stream();
        a8[36..44].copy_from_slice(&le_f64(radius));
        a8[68..76].copy_from_slice(&le_f64(radius));
        let [curve] = crate::families::a5a8::records::a8_freeform_curves(&a8)
            .try_into()
            .expect("one common-form rolling-ball jet");
        assert_eq!(curve.sites[0].radius, radius);
    }
}

#[test]
fn rolling_ball_parsers_reject_scale_relative_radius_disagreement() {
    let tiny = 1e-200;
    let mut bytes = a5_freeform_curve_stream();
    bytes[28..36].copy_from_slice(&le_f64(tiny));
    bytes[60..68].copy_from_slice(&le_f64(2.0 * tiny));
    bytes[100..108].copy_from_slice(&le_f64(std::f64::consts::PI));
    assert!(crate::families::a5a8::records::a5_freeform_curves(&bytes).is_empty());
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
fn guide_curve_parser_accepts_frame_bounded_site_count() {
    let curves =
        crate::families::a5a8::records::a5_guide_curves(&a5_guide_curve_stream_with_count(4097));
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].knots.len(), 4097);
    assert_eq!(curves[0].sites.len(), 4097);
}

#[test]
fn guide_curve_parser_rejects_nonfinite_jet_channels() {
    for offset in [12, 124, 220] {
        let mut bytes = a5_guide_curve_stream();
        bytes[offset..offset + 8].copy_from_slice(&le_f64(f64::NAN));
        assert!(
            crate::families::a5a8::records::a5_guide_curves(&bytes).is_empty(),
            "offset {offset}"
        );
    }

    let mut repeated_knot = a5_guide_curve_stream();
    repeated_knot[20..28].copy_from_slice(&le_f64(0.0));
    assert!(crate::families::a5a8::records::a5_guide_curves(&repeated_knot).is_empty());
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

#[test]
fn a8_curve_parser_accepts_frame_bounded_site_count() {
    let curves = crate::families::a5a8::records::a8_freeform_curves(
        &a8_freeform_curve_stream_with_count(8193),
    );
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].knots.len(), 8193);
    assert_eq!(curves[0].sites.len(), 8193);
}

#[test]
fn a8_curve_parser_accepts_each_object_frame_flag() {
    for flag in [0x03, 0x13, 0x83] {
        let mut bytes = a8_freeform_curve_stream();
        bytes[1] = flag;
        assert_eq!(
            crate::families::a5a8::records::a8_freeform_curves(&bytes).len(),
            1,
            "flag {flag:#04x}"
        );
    }

    let mut malformed = a8_freeform_curve_stream();
    malformed[1] = 0x23;
    assert!(crate::families::a5a8::records::a8_freeform_curves(&malformed).is_empty());
}

#[test]
fn indexed_a5_record_decoders_match_one_shot_wrappers() {
    let freeform = a5_freeform_curve_stream();
    let records = crate::wire::records::consolidated_records(&freeform);
    let one_shot = crate::families::a5a8::records::a5_freeform_curves(&freeform);
    let indexed =
        crate::families::a5a8::records::a5_freeform_curves_from_records(&freeform, &records);
    assert_eq!(one_shot.len(), indexed.len());
    for (one_shot, indexed) in one_shot.iter().zip(&indexed) {
        assert_eq!(one_shot.pos, indexed.pos);
        assert_eq!(one_shot.header_token, indexed.header_token);
        assert_eq!(one_shot.degree, indexed.degree);
        assert_eq!(one_shot.knots, indexed.knots);
        assert_eq!(one_shot.sites, indexed.sites);
        assert_eq!(one_shot.first_derivatives, indexed.first_derivatives);
        assert_eq!(one_shot.second_derivatives, indexed.second_derivatives);
    }

    let guide = a5_guide_curve_stream();
    let records = crate::wire::records::consolidated_records(&guide);
    let one_shot = crate::families::a5a8::records::a5_guide_curves(&guide);
    let indexed = crate::families::a5a8::records::a5_guide_curves_from_records(&guide, &records);
    assert_eq!(one_shot.len(), indexed.len());
    for (one_shot, indexed) in one_shot.iter().zip(&indexed) {
        assert_eq!(one_shot.pos, indexed.pos);
        assert_eq!(one_shot.header_token, indexed.header_token);
        assert_eq!(one_shot.degree, indexed.degree);
        assert_eq!(one_shot.knots, indexed.knots);
        assert_eq!(one_shot.sites, indexed.sites);
        assert_eq!(one_shot.first_derivatives, indexed.first_derivatives);
        assert_eq!(one_shot.second_derivatives, indexed.second_derivatives);
    }

    let nurbs = a5_nurbs_curve_stream();
    let records = crate::wire::records::consolidated_records(&nurbs);
    assert_eq!(
        crate::families::a5a8::records::a5_nurbs_curves(&nurbs),
        crate::families::a5a8::records::a5_nurbs_curves_from_records(&nurbs, &records)
    );
}

fn a5_nurbs_curve_stream() -> Vec<u8> {
    let knots = [-2.220_264_955_47_f64, 0.0, 2.220_264_955_47];
    let points = [
        [25.024_609_677_8, 20.779_735_044_5, 13.0],
        [24.316_927_644_1, 21.223_788_035_6, 13.0],
        [23.708_153_935, 21.667_841_026_7, 13.0],
        [23.236_619_670_7, 22.111_894_017_8, 13.0],
        [22.763_380_329_3, 23.0, 13.0],
        [23.236_619_670_7, 23.888_105_982_2, 13.0],
        [23.708_153_935, 24.332_158_973_3, 13.0],
        [24.316_927_644_1, 24.776_211_964_4, 13.0],
        [25.024_609_677_8, 25.220_264_955_5, 13.0],
    ];
    let mut payload = vec![0x15, 0x0d, 0x0c];
    for knot in knots {
        payload.extend_from_slice(&knot.to_le_bytes());
    }
    payload.push(0x01);
    for point in points {
        for coordinate in point {
            payload.extend_from_slice(&f64::to_le_bytes(coordinate));
        }
    }
    payload.extend_from_slice(&[0x05, 0x09]);
    for value in [0.0, knots[2], 1.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x00, 0x07]);
    assert_eq!(payload.len(), 280);
    let mut record = vec![0xa5, 0x13, 0x16];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x0d);
    record.extend(payload);
    record
}

fn a5_nurbs_curve_stream_with_knot_count(knot_count: usize) -> Vec<u8> {
    assert_eq!(knot_count, 8193);
    let mut payload = vec![0x15, 0x08, 0x01, 0x20, 0x0c];
    for knot in 0..knot_count {
        payload.extend_from_slice(&f64::from(u32::try_from(knot).unwrap()).to_le_bytes());
    }
    payload.push(0x01);
    for _ in 0..(3 * knot_count) {
        for _ in 0..3 {
            payload.extend_from_slice(&0.0f64.to_le_bytes());
        }
    }
    payload.extend_from_slice(&[0x05, 0x09]);
    for value in [
        0.0,
        f64::from(u32::try_from(knot_count - 1).unwrap()),
        1.0,
        0.0,
    ] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x00, 0x07]);
    let mut record = vec![0xa5, 0x13, 0x16];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x0d);
    record.extend(payload);
    record
}

#[test]
fn a5_nurbs_curve_parser_expands_the_degree_five_knot_multiplicities() {
    let curves = crate::families::a5a8::records::a5_nurbs_curves(&a5_nurbs_curve_stream());
    let [curve] = curves.as_slice() else {
        panic!("one degree-five curve");
    };
    assert_eq!(curve.geometry.degree(), 5);
    assert_eq!(curve.geometry.control_points().len(), 9);
    assert_eq!(curve.geometry.knots().len(), 15);
    assert_eq!(curve.geometry.knots()[..6], [-2.220_264_955_47; 6]);
    assert_eq!(curve.geometry.knots()[6..9], [0.0; 3]);
    assert_eq!(curve.geometry.knots()[9..], [2.220_264_955_47; 6]);
    assert!(curve.geometry.weights().is_none());
}

#[test]
fn a5_nurbs_curve_parser_accepts_frame_bounded_knot_count() {
    let curves = crate::families::a5a8::records::a5_nurbs_curves(
        &a5_nurbs_curve_stream_with_knot_count(8193),
    );
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].geometry.control_points().len(), 24_579);
    assert_eq!(curves[0].geometry.knots().len(), 24_585);
}

#[test]
fn a5_nurbs_curve_parser_rejects_nonfinite_knots_and_control_points() {
    let mut nonfinite_knot = a5_nurbs_curve_stream();
    nonfinite_knot[11..19].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::a5a8::records::a5_nurbs_curves(&nonfinite_knot).is_empty());

    let mut nonfinite_control_point = a5_nurbs_curve_stream();
    nonfinite_control_point[36..44].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::a5a8::records::a5_nurbs_curves(&nonfinite_control_point).is_empty());
}

#[test]
fn a5_nurbs_curve_parser_rejects_broken_frame_invariants() {
    let valid = a5_nurbs_curve_stream();
    for offset in [8, 9, 10, 27, 35, 252, 253, 254, 262, 270, 278, 286, 287] {
        let mut broken = valid.clone();
        broken[offset] ^= 1;
        assert!(
            crate::families::a5a8::records::a5_nurbs_curves(&broken).is_empty(),
            "offset {offset}"
        );
    }
}

#[test]
fn decode_geometry_fallback_transfers_an_external_a8_pole_grid() {
    let file = object_main_catpart(&a8_elided_surface_stream());
    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points().len(), 9);
    assert_eq!(surface.control_points()[8], Point3::new(8.0, 2.0, 2.0));
}

#[test]
fn decode_float_packed_stream_transfers_an_elided_a8_surface_with_native_topology() {
    let stream = a8_elided_surface_stream_with_native_vertex_chain();
    let graph = crate::families::b5::graph::parse(&stream).expect("generated A8 topology");
    assert!(graph.complete);
    assert_eq!(graph.faces.len(), 1);
    assert_eq!(graph.loops.len(), 1);
    assert_eq!(graph.pcurves.len(), 3);
    assert_eq!(graph.edges.len(), 3);
    assert_eq!(graph.logical_vertex_refs, [600, 601, 602]);
    assert_eq!(
        graph.logical_vertex_points,
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode elided A8 surface topology");
    assert_eq!(result.ir().model.surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points()[8], Point3::new(1.0, 1.0, 0.0));
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 3);
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_object_stream_does_not_promote_unbound_a8_pcurve() {
    let file = object_main_catpart(&a8_pcurve_stream());
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode unbound object-stream pcurve");
    assert!(decoded.ir().model.pcurves.is_empty());
    assert!(!decoded.ir().native_unknowns("catia").unwrap().is_empty());
}

#[test]
fn decode_object_stream_transfers_a8_rolling_ball_jet() {
    let file = object_main_catpart(&a8_freeform_curve_stream());
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::FloatPackedInnerNoFbb
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode rolling-ball object stream");
    let [procedural] = decoded.ir().model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
        degree,
        knots,
        multiplicities,
        sites,
    } = procedural.definition()
    else {
        panic!("rolling-ball jet");
    };
    assert_eq!(*degree, 5);
    assert_eq!(knots, &[0.0, 1.0]);
    assert_eq!(multiplicities, &[6, 6]);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[1].first_limit, Point3::new(2.0, 0.0, 0.0));
    assert_eq!(sites[1].angle, std::f64::consts::FRAC_PI_2);
    let provenance = &decoded.source_fidelity().annotations.provenance[procedural.id.as_str()];
    assert_eq!(provenance.stream(), "catia:object_stream_a8_03_32");
    let tag = provenance
        .tag
        .as_deref()
        .expect("rolling-ball provenance tag");
    assert!(tag.contains("object_id:12345678"));
    assert!(tag.contains("multiplicities:[6, 6]"));
    assert_eq!(
        decoded.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:12345678"))
    );
}

#[test]
fn decode_float_packed_stream_transfers_a8_nurbs() {
    assert_eq!(
        crate::container::scan_bytes(a8_catpart()).variant,
        Variant::FloatPackedInnerNoFbb
    );
    let mut cur = Cursor::new(a8_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Nurbs(_)
    ));
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
}

#[test]
fn decode_inner_no_directory_transfers_a8_nurbs() {
    assert_eq!(
        crate::container::scan_bytes(inner_no_directory_a8_catpart()).variant,
        Variant::InnerNoDirectory
    );
    let mut cur = Cursor::new(inner_no_directory_a8_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Nurbs(_)
    ));
    assert_eq!(
        result.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
}
