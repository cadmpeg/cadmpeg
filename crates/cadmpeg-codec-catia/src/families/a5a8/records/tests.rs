// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `a5a8` family over synthetic byte fixtures.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use cadmpeg_test_support::EditableDecodeResult;

use std::io::Cursor;

use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::FiniteVector;

use crate::test_support::test_a5a8::{
    a5_freeform_curve_stream, a5_freeform_curve_stream_with_count, a5_guide_curve_stream,
    a5_guide_curve_stream_with_count, a5_pcurve_stream, a5_pcurve_stream_with_count,
    a5_rational_surface_stream, a5_surface_extrapolated_short_tail, a5_surface_extrapolated_tail,
    a5_surface_short_tail, a5_surface_stream, a5_surface_stream_with_tail, a5_surface_tail,
    a6_freeform_curve_stream, a6_pcurve_stream, a6_surface_stream, a8_catpart,
    a8_elided_surface_stream, a8_freeform_curve_stream, a8_freeform_curve_stream_with_count,
    a8_inline_tail_surface_stream, a8_pcurve_stream, a8_pcurve_stream_with_count,
    a8_rational_surface_stream, a8_surface_stream, a8_surface_stream_with_u_count, a8_surface_tail,
    inner_no_directory_a8_catpart,
};
use crate::test_support::test_b5::a8_elided_surface_stream_with_native_vertex_chain;

use crate::test_support::test_bytes::le_f64;
use crate::test_support::test_container::object_main_catpart;
use crate::variant::Variant;
use crate::CatiaCodec;

#[test]
fn object_stream_frame_scan_resumes_at_parent_end() {
    let mut bytes = vec![0xa8, 0x03, 0x32];
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let following_frame = bytes.len();
    bytes.extend_from_slice(&[0xa8, 0x03, 0x20]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());

    let frames = crate::test_support::with_service_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>(
            super::object_stream_frames(ctx, &bytes)?.collect::<Vec<_>>(),
        )
    })
    .expect("service frame scan budget");

    assert_eq!(
        frames.iter().map(|frame| frame.payload).collect::<Vec<_>>(),
        vec![11, 19, following_frame + 11]
    );
}

fn parsed_a5_freeform_curves(data: &[u8]) -> Vec<crate::families::a5a8::records::A5FreeformCurve> {
    crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_freeform_curves(ctx, data).expect("service decode")
    })
}

fn parsed_a8_freeform_curves(data: &[u8]) -> Vec<crate::families::a5a8::records::A8FreeformCurve> {
    crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_freeform_curves(ctx, data).expect("service decode")
    })
}

fn parsed_a8_pcurves(data: &[u8]) -> Vec<crate::families::a5a8::records::A8Pcurve> {
    crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_pcurves(ctx, data).expect("service decode")
    })
}

fn parsed_object_stream_pcurves(data: &[u8]) -> Vec<crate::families::a5a8::records::A8Pcurve> {
    crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::object_stream_pcurves(ctx, data).expect("service decode")
    })
}

#[test]
fn a8_surface_parser_reads_common_form_nurbs() {
    let surfaces = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &a8_surface_stream(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].object_id(), Some(0xdeca_fbad));
    let surface = &surfaces[0].geometry;
    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 2));
    assert_eq!((surface.u_count(), surface.v_count()), (3, 3));
    assert_eq!(surface.poles().into_iter().nth(8).unwrap().x, 8.0);
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

    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
    let header = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_header_from_object_frame(
            ctx,
            &bytes,
            inner_start,
            inner_end,
            inner_object_id,
        )
        .expect("service decode")
    })
    .expect("selected nested surface header");
    assert_eq!(
        (
            crate::test_support::with_service_context(|ctx| header.u_count(ctx))
                .expect("pole count admission")
                .expect("fixture U lattice has a valid pole count"),
            crate::test_support::with_service_context(|ctx| header.v_count(ctx))
                .expect("pole count admission")
                .expect("fixture V lattice has a valid pole count")
        ),
        (3, 3)
    );
    let surface = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surface_from_object_frame(
            ctx,
            &bytes,
            inner_start,
            inner_end,
            inner_object_id,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .expect("selected nested surface");
    let surface = surface.geometry;
    assert_eq!(surface.poles().into_iter().nth(8).unwrap().x, 8.0);
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surface_from_object_frame(
            ctx,
            &bytes,
            inner_start,
            inner_end - 1,
            inner_object_id,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_none());
}

#[test]
fn a8_surface_parser_accepts_frame_bounded_knot_and_pole_counts() {
    let surfaces = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &a8_surface_stream_with_u_count(20_001),
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert_eq!(surfaces.len(), 1);
    let surface = &surfaces[0].geometry;
    assert_eq!((surface.u_count(), surface.v_count()), (20_002, 3));
    assert_eq!(surface.poles().len(), 60_006);
}

#[test]
fn a8_surface_parser_rejects_unframed_trailing_bytes() {
    let mut bytes = a8_surface_stream();
    bytes.push(0);
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
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

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a8_surfaces(
                ctx,
                &bytes,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        })
        .len(),
        1
    );
}

#[test]
fn a8_surface_parser_accepts_a_valid_tail_after_inline_poles() {
    let bytes = a8_inline_tail_surface_stream();
    let [surface] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one inline-tail surface");
    let surface = surface.geometry;
    assert_eq!(surface.poles().into_iter().nth(8).unwrap().x, 8.0);
}

#[test]
fn a8_surface_parser_accepts_a_valid_tail_after_inline_weights() {
    let mut bytes = a8_rational_surface_stream();
    bytes.extend_from_slice(&a8_surface_tail());
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [surface] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one inline-weight-tail surface");
    let surface = surface.geometry;
    assert_eq!(
        surface.pole_grid().weights().map(|rows| rows.concat()),
        Some(vec![2.0; 9])
    );
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
        let [_surface] = crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a8_surfaces(
                ctx,
                &bytes,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        })
        .try_into()
        .expect("one inline-tail surface");
        let [header] = crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
                .expect("frame scan admission")
                .collect::<Result<Vec<_>, _>>()
                .expect("service decode")
        })
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

    let [header] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    })
    .try_into()
    .expect("one parameter lattice");
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a8_surface_parser_rejects_a_malformed_tail_after_inline_poles() {
    let mut bytes = a8_inline_tail_surface_stream();
    let tail_start = bytes.len() - 141;
    bytes[tail_start + 68] = 0;
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a8_surface_parser_accepts_each_object_frame_flag() {
    for flag in [0x03, 0x13, 0x83] {
        let mut bytes = a8_surface_stream();
        bytes[1] = flag;
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                crate::families::a5a8::records::a8_surfaces(
                    ctx,
                    &bytes,
                    &mut crate::nurbs::LaneRefusals::new(),
                )
                .expect("service decode")
            })
            .len(),
            1,
            "flag {flag:#04x}"
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
                    .expect("frame scan admission")
                    .collect::<Result<Vec<_>, _>>()
                    .expect("service decode")
            })
            .len(),
            1,
            "flag {flag:#04x}"
        );
    }

    let mut malformed = a8_surface_stream();
    malformed[1] = 0x23;
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &malformed,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
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
            crate::test_support::with_service_context(|ctx| {
                crate::families::a5a8::records::a8_surfaces(
                    ctx,
                    &bytes,
                    &mut crate::nurbs::LaneRefusals::new(),
                )
                .expect("service decode")
            })
            .is_empty(),
            "{label} must not produce a resolved surface"
        );
        assert!(
            crate::test_support::with_service_context(|ctx| {
                crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
                    .expect("frame scan admission")
                    .collect::<Result<Vec<_>, _>>()
                    .expect("service decode")
            })
            .is_empty(),
            "{label} must not produce a surface header"
        );
    }
}

#[test]
fn a8_surface_header_survives_an_opaque_pole_representation() {
    let mut bytes = a8_surface_stream();
    bytes[59..67].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
    let headers = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    });
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].object_id, 0xdeca_fbad);
    assert_eq!((headers[0].u_degree, headers[0].v_degree), (2, 2));
    assert_eq!(
        (
            crate::test_support::with_service_context(|ctx| headers[0].u_count(ctx))
                .expect("pole count admission")
                .expect("fixture U lattice has a valid pole count"),
            crate::test_support::with_service_context(|ctx| headers[0].v_count(ctx))
                .expect("pole count admission")
                .expect("fixture V lattice has a valid pole count")
        ),
        (3, 3)
    );
    assert_eq!(headers[0].u_knots.multiplicities(), [3, 3]);
    assert_eq!(headers[0].v_knots.multiplicities(), [3, 3]);
    assert_eq!(
        headers[0].pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
}

#[test]
fn copied_a8_surface_header_refuses_each_knot_lane_limit() {
    let bytes = a8_surface_stream();
    let [header] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
    })
    .expect("service budget")
    .try_into()
    .expect("one header");
    for (limit, operation) in [
        (1, "catia_a8_copied_distinct_knots"),
        (2, "catia_a8_copied_multiplicities"),
        (5, "catia_a8_copied_distinct_knots"),
        (6, "catia_a8_copied_multiplicities"),
    ] {
        let limited =
            crate::test_support::with_collection_limit(limit, |ctx| header.copy_charged(ctx));
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == operation)
        );
    }
    assert_eq!(
        crate::test_support::with_service_context(|ctx| header.copy_charged(ctx))
            .expect("service budget"),
        header
    );
}

#[test]
fn a8_surface_header_identifies_an_elided_pole_grid() {
    let bytes = a8_elided_surface_stream();
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
    let headers = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    });
    assert_eq!(headers.len(), 1);
    assert_eq!(
        headers[0].pole_storage,
        crate::families::a5a8::records::PoleStorage::Elided
    );
}

#[test]
fn a8_surface_header_retains_an_inline_parameter_tail() {
    let headers = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &a8_inline_tail_surface_stream())
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    });
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
    let [header] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    })
    .try_into()
    .expect("one surface header");
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
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

    let [header] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    })
    .try_into()
    .expect("one elided surface header");
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Elided
    );

    bytes[a8_end + 3] = 250;
    let [header] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    })
    .try_into()
    .expect("one surface header");
    assert_eq!(
        header.pole_storage,
        crate::families::a5a8::records::PoleStorage::Inline
    );
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a8_elided_surface_resolves_one_external_pole_grid_gap() {
    let bytes = a8_elided_surface_stream();

    let [header] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    })
    .try_into()
    .expect("one elided header");
    let surface = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_from_external_grid(
            ctx,
            &bytes,
            &header,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .expect("unique external pole allocation");
    let surface = surface.geometry;
    assert_eq!(surface.poles().len(), 9);
    assert_eq!(
        surface.poles().into_iter().nth(8).unwrap(),
        Point3::new(8.0, 2.0, 2.0)
    );

    let [resolved] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one resolved surface");
    assert_eq!(resolved.object_id(), Some(100));
    let resolved = resolved.geometry;
    assert_eq!(resolved.control_grid(), surface.control_grid());
}

#[test]
fn a8_external_grid_range_refuses_collection_limit_before_retention() {
    let bytes = a8_elided_surface_stream();
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        crate::families::a5a8::records::a8_external_grid_ranges(ctx, &bytes)
    });
    assert!(matches!(limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_a8_external_grid_ranges"));
    let ranges = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_external_grid_ranges(ctx, &bytes)
    })
    .expect("service collection budget");
    assert_eq!(ranges.len(), 1);
    assert!(ranges[0].start < ranges[0].end);
    assert!(ranges[0].end <= bytes.len());
}

#[test]
fn a8_external_grid_poles_refuse_collection_limit_before_materialization() {
    let bytes = a8_elided_surface_stream();
    let [header] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    })
    .try_into()
    .expect("one elided header");
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        crate::families::a5a8::records::a8_surface_from_external_grid(
            ctx,
            &bytes,
            &header,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(matches!(limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_a8_external_poles"));
    let surface = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_from_external_grid(
            ctx,
            &bytes,
            &header,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service collection budget")
    .expect("unique external pole allocation");
    assert_eq!(surface.geometry.poles().len(), 9);
}

#[test]
fn a8_inline_poles_refuse_collection_limit_before_materialization() {
    let bytes = a8_surface_stream();
    let limited = crate::test_support::with_collection_limit(8, |ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(matches!(limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_a8_inline_poles"));
    let surfaces = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service collection budget");
    assert_eq!(surfaces.len(), 1);
}

#[test]
fn a8_header_lanes_refuse_collection_limits_before_materialization() {
    let bytes = a8_surface_stream();
    let object_id = cadmpeg_core::decode::View::u32_le_at(&bytes, 7).expect("fixture object id");
    for (limit, operation) in [
        (0, "catia_a8_distinct_knots"),
        (2, "catia_a8_multiplicities"),
    ] {
        let limited = crate::test_support::with_collection_limit(limit, |ctx| {
            crate::families::a5a8::records::a8_surface_header_from_object_frame(
                ctx,
                &bytes,
                0,
                bytes.len(),
                object_id,
            )
        });
        assert!(matches!(limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == operation));
    }
    let header = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_header_from_object_frame(
            ctx,
            &bytes,
            0,
            bytes.len(),
            object_id,
        )
    })
    .expect("service collection budget")
    .expect("complete surface header");
    assert_eq!(header.object_id, object_id);
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
    let headers = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surface_headers(ctx, &bytes)
            .expect("frame scan admission")
            .collect::<Result<Vec<_>, _>>()
            .expect("service decode")
    });
    assert_eq!(headers.len(), 2);
    assert_eq!(
        headers
            .iter()
            .map(|header| header.object_id)
            .collect::<Vec<_>>(),
        [100, 101]
    );
    for header in &headers {
        let surface = crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a8_surface_from_external_grid(
                ctx,
                &bytes,
                header,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        })
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
                crate::test_support::with_service_context(|ctx| {
                    crate::families::a5a8::records::resolved_a8_surfaces(
                        ctx,
                        &bytes,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                    .expect("service decode")
                })
                .len(),
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

    let [resolved] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one resolved surface");
    let surface = resolved.geometry;
    assert_eq!(surface.poles().into_iter().next().unwrap().x, 2e12);

    bytes[pole_start..pole_start + 8].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a8_elided_surface_requires_a_length_closed_successor_frame() {
    let mut bytes = a8_elided_surface_stream();
    let successor = bytes
        .windows(3)
        .rposition(|value| value == [0xb5, 0x03, 0x5e])
        .expect("successor frame");
    bytes[successor + 3] = 3;
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::resolved_a8_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a8_pcurve_parser_reads_degree5_uv_jet() {
    let pcurves = parsed_a8_pcurves(&a8_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(
        (pcurves[0].object_id, pcurves[0].support_id),
        (0x5678, 0x1234)
    );
    assert_eq!(pcurves[0].points(), vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(
        pcurves[0].range,
        crate::test_support::test_b5::finite_pair([0.0, 1.0])
    );
    assert_eq!(pcurves[0].mode, 0x01);
    let mut wrong_degree = a8_pcurve_stream();
    wrong_degree[15] = 17;
    assert!(parsed_a8_pcurves(&wrong_degree).is_empty());

    let mut repeated_knot = a8_pcurve_stream();
    repeated_knot[28..36].copy_from_slice(&le_f64(0.0));
    assert!(parsed_a8_pcurves(&repeated_knot).is_empty());

    let mut wrong_endpoint_multiplicity = a8_pcurve_stream();
    wrong_endpoint_multiplicity[36] = 21;
    assert!(parsed_a8_pcurves(&wrong_endpoint_multiplicity).is_empty());

    let mut trailing_byte = a8_pcurve_stream();
    trailing_byte.push(0);
    let payload_len = u32::try_from(trailing_byte.len() - 11).unwrap();
    trailing_byte[3..7].copy_from_slice(&payload_len.to_le_bytes());
    assert!(parsed_a8_pcurves(&trailing_byte).is_empty());
}

#[test]
fn a8_pcurve_parser_accepts_frame_bounded_site_count() {
    let pcurves = parsed_a8_pcurves(&a8_pcurve_stream_with_count(8193));
    assert_eq!(pcurves.len(), 1);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| pcurves[0].knots(ctx))
            .expect("service resource budget")
            .len(),
        8193
    );
    assert_eq!(pcurves[0].points().len(), 8193);
}

#[test]
fn object_stream_pcurve_sites_refuse_collection_limit_before_materialization() {
    let bytes = a8_pcurve_stream();
    let result = crate::test_support::with_collection_limit(1, |ctx| {
        crate::families::a5a8::records::object_stream_pcurves(ctx, &bytes)
    });
    assert!(matches!(result,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_object_stream_pcurve_sites"
    ));
    assert_eq!(parsed_object_stream_pcurves(&bytes).len(), 1);
}

#[test]
fn a8_pcurve_collection_refuses_before_retention() {
    let bytes = a8_pcurve_stream();
    let result = crate::test_support::with_collection_limit(2, |ctx| {
        crate::families::a5a8::records::a8_pcurves(ctx, &bytes)
    });
    assert!(matches!(result,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_a8_pcurves"
    ));
    assert_eq!(parsed_a8_pcurves(&bytes).len(), 1);
}

#[test]
fn object_stream_pcurve_collection_refuses_before_retention() {
    let bytes = a8_pcurve_stream();
    let result = crate::test_support::with_collection_limit(2, |ctx| {
        crate::families::a5a8::records::object_stream_pcurves(ctx, &bytes)
    });
    assert!(matches!(result,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_object_stream_pcurves"
    ));
    assert_eq!(parsed_object_stream_pcurves(&bytes).len(), 1);
}

#[test]
fn a8_pcurve_parser_accepts_finite_large_jet_values() {
    let mut bytes = a8_pcurve_stream();
    bytes[40..48].copy_from_slice(&le_f64(2e12));
    let [pcurve] = parsed_a8_pcurves(&bytes).try_into().expect("one pcurve");
    assert_eq!(pcurve.points()[0][0], 2e12);

    bytes[40..48].copy_from_slice(&le_f64(f64::NAN));
    assert!(parsed_a8_pcurves(&bytes).is_empty());
}

#[test]
fn a8_pcurve_parser_retains_mode_five_uv_jet() {
    let mut bytes = a8_pcurve_stream();
    bytes[39] = 0x05;
    let pcurves = parsed_a8_pcurves(&bytes);
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].mode, 0x05);
    assert_eq!(pcurves[0].points(), vec![[0.0, 0.0], [1.0, 1.0]]);
}

#[test]
fn b5_pcurve_parser_reads_degree5_uv_jet() {
    let a8 = a8_pcurve_stream();
    let payload = &a8[11..];
    let mut b5 = vec![0xb5, 0x03, 0x20, u8::try_from(payload.len()).unwrap()];
    b5.extend_from_slice(&0x5678u32.to_le_bytes());
    b5.extend_from_slice(payload);

    let pcurves = parsed_object_stream_pcurves(&b5);

    assert_eq!(pcurves.len(), 1);
    assert_eq!(
        (pcurves[0].object_id, pcurves[0].support_id),
        (0x5678, 0x1234)
    );
    assert_eq!(pcurves[0].points(), vec![[0.0, 0.0], [1.0, 1.0]]);
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
        let [pcurve] = parsed_object_stream_pcurves(&stream)
            .try_into()
            .expect("one pcurve");
        assert_eq!(pcurve.object_id, 0x5678);
    }

    let mut malformed = a8;
    malformed[1] = 0x23;
    assert!(parsed_object_stream_pcurves(&malformed).is_empty());
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

    let [pcurve] = parsed_object_stream_pcurves(&wrapper)
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

    let pcurves = parsed_object_stream_pcurves(&b5);

    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x0012_0034);
}

#[test]
fn a5_pcurve_parser_reads_compact_support_and_uv_jet() {
    let pcurves = crate::families::a5a8::records::a5_pcurves(&a5_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x1234);
    assert_eq!(pcurves[0].extrapolation_sites, 2);
    assert_eq!(
        pcurves[0]
            .points()
            .into_iter()
            .map(FiniteVector::get)
            .collect::<Vec<_>>(),
        vec![[0.0, 0.0], [1.0, 1.0]]
    );
    assert_eq!(pcurves[0].range.endpoints(), [0.0, 1.0]);
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
    assert_eq!(
        pcurves[0]
            .points()
            .into_iter()
            .map(FiniteVector::get)
            .collect::<Vec<_>>(),
        vec![[0.0, 0.0], [1.0, 1.0]]
    );
}

#[test]
fn a5_pcurve_parser_accepts_frame_bounded_site_count() {
    let pcurves = crate::families::a5a8::records::a5_pcurves(&a5_pcurve_stream_with_count(4097));
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].knots().len(), 4097);
    assert_eq!(pcurves[0].points().len(), 4097);
}

#[test]
fn a8_surface_parser_reads_rational_weight_grid() {
    let surfaces = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &a8_rational_surface_stream(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert_eq!(
        surfaces[0]
            .geometry
            .pole_grid()
            .weights()
            .map(|rows| rows.concat()),
        Some(vec![2.0; 9])
    );
}

#[test]
fn surface_parsers_require_finite_nonzero_weights() {
    let mut a5 = a5_rational_surface_stream();
    a5[146..154].copy_from_slice(&le_f64(2e12));
    let [surface] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a5,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one consolidated rational surface");
    let surface = surface.geometry;
    assert_eq!(
        surface
            .pole_weights()
            .expect("weights")
            .into_iter()
            .next()
            .unwrap()
            .get(),
        2e12
    );
    a5[146..154].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a5,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());

    let mut a8 = a8_rational_surface_stream();
    a8[275..283].copy_from_slice(&le_f64(2e12));
    let [surface] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &a8,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one common-form rational surface");
    let surface = surface.geometry;
    assert_eq!(
        surface
            .pole_weights()
            .expect("weights")
            .into_iter()
            .next()
            .unwrap()
            .get(),
        2e12
    );
    a8[275..283].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &a8,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a5_surface_parser_reads_consolidated_nurbs() {
    let surfaces = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a5_surface_stream(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].identity, None);
    assert_eq!(surfaces[0].object_id(), None);
    let surface = &surfaces[0].geometry;
    assert_eq!((surface.u_degree(), surface.v_degree()), (1, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
    assert_eq!(surface.poles().into_iter().nth(3).unwrap().x, 3.0);
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

    let [surface] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one multispan cubic surface");
    let surface = surface.geometry;
    assert_eq!((surface.u_degree(), surface.v_degree()), (3, 3));
    assert_eq!((surface.u_count(), surface.v_count()), (5, 5));
    assert_eq!(
        surface.u_knots().as_slice(),
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0, 2.0]
    );
    assert_eq!(surface.poles().len(), 25);
}

#[test]
fn surface_parsers_accept_finite_large_control_points() {
    let mut a5 = a5_surface_stream();
    a5[47..55].copy_from_slice(&le_f64(2e12));
    let [surface] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a5,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one consolidated surface");
    let surface = surface.geometry;
    assert_eq!(surface.poles().into_iter().next().unwrap().x, 2e12);

    let mut a8 = a8_surface_stream();
    a8[59..67].copy_from_slice(&le_f64(2e12));
    let [surface] = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &a8,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .try_into()
    .expect("one common-form surface");
    let surface = surface.geometry;
    assert_eq!(surface.poles().into_iter().next().unwrap().x, 2e12);

    a5[47..55].copy_from_slice(&le_f64(f64::NAN));
    a8[59..67].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a5,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a8_surfaces(
            ctx,
            &a8,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a5_surface_parser_rejects_nonfinite_and_repeated_distinct_knots() {
    let mut nonfinite_u = a5_surface_stream();
    nonfinite_u[11..19].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &nonfinite_u,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());

    let mut repeated_u = a5_surface_stream();
    repeated_u[19..27].copy_from_slice(&le_f64(0.0));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &repeated_u,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());

    let mut nonfinite_v = a5_surface_stream();
    nonfinite_v[30..38].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &nonfinite_v,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn consolidated_surface_parser_reads_width2_frame() {
    let surfaces = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a6_surface_stream(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert_eq!(surfaces.len(), 1);
    assert_eq!(
        (
            surfaces[0].geometry.u_count(),
            surfaces[0].geometry.v_count()
        ),
        (2, 2)
    );
}

#[test]
fn a5_surface_parser_reads_rational_weight_program() {
    let surfaces = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a5_rational_surface_stream(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert_eq!(
        surfaces[0]
            .geometry
            .pole_grid()
            .weights()
            .map(|rows| rows.concat()),
        Some(vec![2.0; 4])
    );
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
        assert!(crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a5_surfaces(
                ctx,
                &malformed,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        })
        .is_empty());
    }
}

#[test]
fn a5_surface_parser_rejects_untagged_int_bytes_without_underflow() {
    let bytes = [
        0xa5, 0xa5, 0x03, 0x34, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0xb3, 0xa5, 0xa5, 0xb3, 0xb3, 0xa5,
    ];
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
}

#[test]
fn a5_surface_parser_accepts_each_structured_tail_variant() {
    for tail in [
        a5_surface_short_tail(),
        a5_surface_tail(),
        a5_surface_extrapolated_short_tail(),
        a5_surface_extrapolated_tail(),
    ] {
        let surfaces = crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a5_surfaces(
                ctx,
                &a5_surface_stream_with_tail(&tail),
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        });
        assert_eq!(surfaces.len(), 1, "tail length {}", tail.len());
    }
}

#[test]
fn a5_surface_parser_rejects_unclosed_or_nonfinite_tail_data() {
    let mut trailing = a5_surface_stream();
    trailing.push(0);
    let payload_len = u32::try_from(trailing.len() - 8).unwrap();
    trailing[3..7].copy_from_slice(&payload_len.to_le_bytes());
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &trailing,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());

    let mut nonfinite = a5_surface_stream();
    let tail = nonfinite
        .windows(4)
        .position(|window| window == [0x05, 0x05, 0x05, 0x05])
        .expect("surface tail");
    nonfinite[tail + 4..tail + 12].copy_from_slice(&le_f64(f64::NAN));
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &nonfinite,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    })
    .is_empty());
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
        crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a5_weights(ctx, &bytes, &mut at, 4, 4, bytes.len())
                .expect("service decode")
        })
        .map(|weights| weights
            .into_iter()
            .map(cadmpeg_ir::scalar::NonZeroReal::get)
            .collect::<Vec<_>>()),
        Some(vec![
            1.0, 0.8, 0.8, 1.0, 0.9, 0.65, 0.65, 0.9, 0.9, 0.65, 0.65, 0.9, 1.0, 0.8, 0.8, 1.0,
        ])
    );
    assert_eq!(at, bytes.len());
}

#[test]
fn a5_surface_poles_refuse_collection_limit_before_materialization() {
    let bytes = a5_surface_stream();
    assert_a5_surface_collection_refusal(&bytes, 16, "catia_a5_surface_poles");
}

#[test]
fn a5_surface_rows_refuse_collection_limit_before_materialization() {
    let bytes = a5_surface_stream();
    assert_a5_surface_collection_refusal(&bytes, 20, "catia_a5_surface_pole_rows");
}

#[test]
fn a5_mirrored_weights_refuse_collection_limit_before_materialization() {
    let bytes = a5_rational_surface_stream();
    assert_a5_surface_collection_refusal(&bytes, 20, "catia_a5_mirrored_weights");
}

fn assert_a5_surface_collection_refusal(bytes: &[u8], first_cap: u64, operation: &str) {
    let mut cap = first_cap;
    for _ in 0..128 {
        let result = crate::test_support::with_collection_limit(cap, |ctx| {
            crate::families::a5a8::records::a5_surfaces(
                ctx,
                bytes,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        });
        match result {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation => {
                return
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                let next = limit
                    .used
                    .checked_add(limit.additional)
                    .expect("bounded test cap");
                assert!(next > cap, "cap sweep did not progress at {cap}: {limit:?}");
                cap = next;
            }
            other => panic!("surface boundary {operation} not reached at cap {cap}: {other:?}"),
        }
    }
    panic!("surface boundary {operation} not reached within 128 refusals");
}

#[test]
fn a5_surface_record_scan_refuses_caller_collection_limit() {
    let bytes = a5_surface_stream();
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &bytes,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_record_source_ranges")
    );
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
        crate::test_support::with_service_context(|ctx| {
            crate::families::a5a8::records::a5_weights(ctx, &bytes, &mut at, 4, 4, bytes.len())
                .expect("service decode")
        })
        .map(|weights| weights
            .into_iter()
            .map(cadmpeg_ir::scalar::NonZeroReal::get)
            .collect::<Vec<_>>()),
        Some(expected.to_vec())
    );
    assert_eq!(at, bytes.len());
}

#[test]
fn a5_explicit_weights_refuse_collection_limit_before_materialization() {
    let mut bytes = vec![0x00];
    bytes.extend([1.0, 2.0, 3.0, 4.0].into_iter().flat_map(le_f64));
    let limited = crate::test_support::with_collection_limit(3, |ctx| {
        let mut at = 0;
        crate::families::a5a8::records::a5_weights(ctx, &bytes, &mut at, 2, 2, bytes.len())
    });
    assert!(matches!(limited,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_a5_explicit_weights"));
}

#[test]
fn a5_weight_program_does_not_cross_frame_boundary() {
    let mut bytes = vec![0x00];
    bytes.extend([1.0, 2.0, 3.0, 4.0].into_iter().flat_map(le_f64));
    bytes.extend([0u8; 8]);
    let mut at = 0;
    assert!(crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_weights(ctx, &bytes, &mut at, 2, 2, 1 + 3 * 8)
            .expect("service decode")
    })
    .is_none());
}

#[test]
fn a5_cubic_two_site_knots_are_clamped() {
    assert_eq!(
        crate::test_support::with_service_context(|ctx| crate::families::a5a8::records::a5_knots(
            ctx,
            &[0.0, 4.0],
            3
        )
        .expect("service decode")),
        Some((vec![0.0, 0.0, 0.0, 0.0, 4.0, 4.0, 4.0, 4.0], 4))
    );
}

#[test]
fn a5_curve_parser_reads_degree5_rolling_ball_jet() {
    for header_token in [5, 9, 13, 29, 17] {
        let mut bytes = a5_freeform_curve_stream();
        bytes[7] = header_token;
        let curves = parsed_a5_freeform_curves(&bytes);
        assert_eq!(curves.len(), 1);
        assert_eq!(curves[0].header_token, u32::from(header_token));
        assert_eq!(
            crate::test_support::with_service_context(|ctx| curves[0].knots(ctx))
                .expect("service resource budget"),
            vec![0.0, 1.0]
        );
        assert_eq!(curves[0].sites[1].site.radius(), 2.0);
    }

    let mut wrong_degree = a5_freeform_curve_stream();
    wrong_degree[9] = 17;
    assert!(parsed_a5_freeform_curves(&wrong_degree).is_empty());

    let mut invalid_header_token = a5_freeform_curve_stream();
    invalid_header_token[7] = 18;
    assert!(parsed_a5_freeform_curves(&invalid_header_token).is_empty());
}

#[test]
fn a5_freeform_sites_refuse_collection_limit_before_materialization() {
    assert_a5_freeform_collection_refusal(1, "catia_a5_freeform_sites");
}

#[test]
fn a5_freeform_curves_refuse_collection_limit_before_retention() {
    assert_a5_freeform_collection_refusal(2, "catia_a5_freeform_curves");
}

fn assert_a5_freeform_collection_refusal(limit: u64, operation: &'static str) {
    let bytes = a5_freeform_curve_stream();
    let result = crate::test_support::with_collection_limit(limit, |ctx| {
        crate::families::a5a8::records::a5_freeform_curves(ctx, &bytes)
    });
    assert!(matches!(result,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == operation
    ));
    assert_eq!(parsed_a5_freeform_curves(&bytes).len(), 1);
}

#[test]
fn a5_curve_parser_accepts_compact_array_marker_values() {
    let mut bytes = a5_freeform_curve_stream();
    bytes[7] = 17;
    bytes[11] = 0x08;
    bytes.insert(12, 0x11);
    let payload_len = u32::try_from(bytes.len() - 8).expect("test frame fits u32");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [curve] = parsed_a5_freeform_curves(&bytes)
        .try_into()
        .expect("one compact-marker rolling-ball jet");
    assert_eq!(curve.header_token, 17);
    assert_eq!(curve.sites.len(), 2);

    let mut invalid_marker = bytes;
    invalid_marker[12] = 0x12;
    assert!(parsed_a5_freeform_curves(&invalid_marker).is_empty());
}

#[test]
fn a5_curve_parser_accepts_frame_bounded_continuation() {
    let mut bytes = a5_freeform_curve_stream();
    bytes.extend(std::iter::repeat_n(0, 4097));
    let payload_len = u32::try_from(bytes.len() - 8).expect("test frame fits u32");
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let [curve] = parsed_a5_freeform_curves(&bytes)
        .try_into()
        .expect("one rolling-ball jet");
    assert_eq!(
        crate::test_support::with_service_context(|ctx| curve.knots(ctx))
            .expect("service resource budget"),
        [0.0, 1.0]
    );
    assert_eq!(curve.sites[1].site.radius(), 2.0);
}

#[test]
fn a5_curve_parser_accepts_frame_bounded_site_count() {
    let curves = parsed_a5_freeform_curves(&a5_freeform_curve_stream_with_count(4097));
    assert_eq!(curves.len(), 1);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| curves[0].knots(ctx))
            .expect("service resource budget")
            .len(),
        4097
    );
    assert_eq!(curves[0].sites.len(), 4097);
}

#[test]
fn rolling_ball_limit_curves_reproduce_stored_endpoint_sites() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits input limit");
    let [jet] = parsed_a5_freeform_curves(&a5_freeform_curve_stream())
        .try_into()
        .expect("one rolling-ball jet");
    for second_limit in [false, true] {
        let curve = crate::families::a5a8::records::rolling_ball_limit_curve(
            &ctx,
            &jet,
            second_limit,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service resource budget")
        .expect("exact limiting curve");
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
        let expected = [jet.sites.first().unwrap(), jet.sites.last().unwrap()].map(|sample| {
            let point = if second_limit {
                sample.site.limit2
            } else {
                sample.site.limit1
            };
            point.get()
        });
        let knots = jet.knots(&ctx).expect("service resource budget");
        assert_eq!(
            cadmpeg_ir::eval::decode::curve_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &geometry,
                knots[0]
            )
            .map(cadmpeg_ir::features::FinitePoint3::get),
            Ok(expected[0])
        );
        assert_eq!(
            cadmpeg_ir::eval::decode::curve_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &geometry,
                knots[1]
            )
            .map(cadmpeg_ir::features::FinitePoint3::get),
            Ok(expected[1])
        );
    }
}

#[test]
fn a5_rolling_ball_limit_refuses_jet_and_pole_allocations() {
    let [jet] = parsed_a5_freeform_curves(&a5_freeform_curve_stream())
        .try_into()
        .expect("one rolling-ball jet");
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::families::a5a8::records::rolling_ball_limit_curve(
            ctx,
            &jet,
            false,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("service resource budget")
        .is_some());
    for (cap, operation) in [
        (0, "catia A5 rolling ball positions"),
        (2, "catia A5 rolling ball first jets"),
        (4, "catia A5 rolling ball second jets"),
        (6, "catia A5 rolling ball knots"),
    ] {
        assert!(matches!(
            crate::test_support::with_collection_limit(cap, run),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
    }
}

#[test]
fn a8_pcurve_bspline_refuses_nested_jet_allocations() {
    let [jet] = parsed_a8_pcurves(&a8_pcurve_stream())
        .try_into()
        .expect("one A8 pcurve jet");
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| jet.bspline(ctx);
    assert!(crate::test_support::with_service_context(run)
        .expect("service resource budget")
        .is_some());
    for (cap, operation) in [
        (0, "catia A8 pcurve jet knots"),
        (2, "catia A8 pcurve jet points"),
        (4, "catia A8 pcurve first jets"),
        (6, "catia A8 pcurve second jets"),
    ] {
        assert!(matches!(
            crate::test_support::with_collection_limit(cap, run),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
    }
}

mod decode_transfer;

mod curve_and_guide_records;

#[test]
fn a5_surface_strict_knot_refusal_stays_in_the_outer_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let bytes = a5_surface_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let record = records.first().unwrap();
    let frame = crate::wire::records::ConsolidatedFrame {
        pos: record.byte_offset(),
        payload: record.payload().unwrap().start,
        end: record.range().unwrap().end,
        header_token: record.header_token(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) =
        super::a5_surface(&ctx, &bytes, frame, &mut crate::nurbs::LaneRefusals::new())
    else {
        panic!("strict knot refusal must not disappear as a missing surface");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    // Distinct materialization admits work before the strict knot scan.
    assert_eq!(original.operation, "catia_a5_distinct_materialization");
    assert_eq!(original.used, 0);
    // The fixture stores the two distinct u-knots 0 and 1.
    assert_eq!(original.additional, 2);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
    );
}

mod work_admission;
