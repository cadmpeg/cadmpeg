// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{
    DecodeContext, ResourceDimension,
};
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{
    parse_surface_contour_chain, plane_envelopes_for_rows, BoundaryType, SurfaceContourRecord,
    SurfaceKind, SurfaceRow,
};

fn row(kind: SurfaceKind) -> SurfaceRow {
    SurfaceRow {
        id: 7,
        kind,
        feature_id: 4,
        reversed: false,
        boundary_type: BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }
}

fn check_steps<T: PartialEq + std::fmt::Debug>(
    operations: &[&str], expected: T,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    assert_eq!(super::work_output(&run), expected);
    if !operations.is_empty() {
        assert_eq!(crate::test_support::assert_work_boundaries(operations, run), expected);
    }
}

#[test]
fn contour_heads_skip_absent_headers_and_refuse_before_malformed_dispatch() {
    for body in [
        &[][..],
        &[0x82][..],
        &[0xff, 0xff][..],
        &[0x82, 0x10, 4][..],
    ] {
        let operations: &[&str] = if body.len() < 2 { &[] } else { &["creo surface contour chain traversal"] };
        check_steps(operations, None, |ctx| {
            parse_surface_contour_chain(
                ctx,
                body,
                0,
                body.len(),
                &row(SurfaceKind::Plane),
                &ScalarCache::default(),
            )
        });
    }
    check_steps(&[], None, |ctx| {
        parse_surface_contour_chain(
            ctx,
            &[0x82, 0x10],
            2,
            2,
            &row(SurfaceKind::Plane),
            &ScalarCache::default(),
        )
    });
}

#[test]
fn contour_heads_own_only_executed_entries_and_preserve_record_identity() {
    for count in [1_usize, 4, 7, 17] {
        for tail in [0, 257] {
            let mut body = Vec::new();
            let mut expected = Vec::new();
            for index in 0..count {
                let raw = [
                    0x82,
                    0x10,
                    1,
                    0x0f,
                    0xe4,
                    0x0f,
                    0xe4,
                    if index + 1 == count { 0xe1 } else { 0xe3 },
                ];
                let offset = body.len();
                body.extend_from_slice(&raw);
                expected.push(SurfaceContourRecord {
                    surface_id: 7,
                    chain_index: index,
                    curve_header_id: 0x210,
                    trv: 1,
                    parameter_envelope: [Some(0.0), Some(1.0), Some(0.0), Some(1.0)],
                    separator_reference: None,
                    body: raw.to_vec(),
                    offset,
                    envelope_offset: offset + 3,
                    surface_row_offset: 0,
                });
            }
            body.extend(std::iter::repeat_n(0xff, tail));
            check_steps(&["creo surface contour chain traversal", "creo contour chain projection", "creo contour chain body"], Some(expected), |ctx| {
                parse_surface_contour_chain(
                    ctx,
                    &body,
                    0,
                    body.len(),
                    &row(SurfaceKind::Plane),
                    &ScalarCache::default(),
                )
            });
        }
    }
}

#[test]
fn contour_head_scan_does_not_visit_a_missing_terminal_entry() {
    let body = [0x82, 0x10, 1, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3];
    check_steps(
        &["creo surface contour chain traversal"], None,
        |ctx| {
            parse_surface_contour_chain(
                ctx,
                &body,
                0,
                body.len(),
                &row(SurfaceKind::Plane),
                &ScalarCache::default(),
            )
        },
    );
}

#[test]
fn contour_terminal_close_cannot_escape_the_exclusive_row_end() {
    let body = [0x82, 0x10, 1, 0x0f, 0xe4, 0x0f, 0xe4, 0xe1];
    for end in 0..body.len() {
        let operations: &[&str] = if end < 2 { &[] } else { &["creo surface contour chain traversal"] };
        check_steps(operations, None, |ctx| {
            parse_surface_contour_chain(
                ctx,
                &body,
                0,
                end,
                &row(SurfaceKind::Plane),
                &ScalarCache::default(),
            )
        });
    }
}

#[test]
fn contour_intermediate_close_outside_the_row_cannot_allocate_an_entry() {
    let body = [0x82, 0x10, 1, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3];
    for end in 0..body.len() {
        let operations: &[&str] = if end < 2 { &[] } else { &["creo surface contour chain traversal"] };
        check_steps(operations, None, |ctx| {
            parse_surface_contour_chain(
                ctx,
                &body,
                0,
                end,
                &row(SurfaceKind::Plane),
                &ScalarCache::default(),
            )
        });
    }
}

#[test]
fn plane_outline_scans_own_rows_and_present_windows_without_eof_work() {
    for size in [0_usize, 1, 7, 17, 257] {
        let body = vec![0xff; size];
        for kind in [SurfaceKind::Plane, SurfaceKind::Cylinder] {
            let rows = [row(kind)];
            check_steps(&["creo plane envelope row traversal"], 0, |ctx| {
                plane_envelopes_for_rows(ctx, &body, &rows).map(|records| records.len())
            });
        }
    }
    check_steps(&[], 0, |ctx| {
        plane_envelopes_for_rows(ctx, &[], &[]).map(|records| records.len())
    });
}

#[test]
fn plane_outline_search_stops_before_a_named_prototype_and_its_tail() {
    for prefix in [0_usize, 1, 7, 17, 257] {
        for header in [false, true] {
            let mut body = vec![0xff; prefix];
            if header {
                body.extend_from_slice(&[0xe0, 0]);
            }
            body.extend_from_slice(b"srf_prim_ptr(plane)\0outline\0\xf9\x02\x03");
            body.extend(std::iter::repeat_n(0xff, 257));
            check_steps(&["creo plane envelope row traversal"], 0, |ctx| {
                plane_envelopes_for_rows(ctx, &body, &[row(SurfaceKind::Plane)])
                    .map(|records| records.len())
            });
        }
    }
}

#[test]
fn plane_outline_match_admits_visited_windows_before_fixed_slot_storage() {
    for prefix in [0_usize, 1, 7, 17, 257] {
        let mut body = vec![0xff; prefix];
        body.extend_from_slice(b"outline\0\xf9\x02\x03");
        let error = crate::test_support::last_refusal_at(&[], ResourceDimension::CollectionItems,
            "creo surface scalar token slots", |ctx| plane_envelopes_for_rows(ctx, &body, &[row(SurfaceKind::Plane)]));
        assert!(matches!(error, CodecError::ResourceLimit(refusal) if refusal.operation == "creo surface scalar token slots"));

    }
}
