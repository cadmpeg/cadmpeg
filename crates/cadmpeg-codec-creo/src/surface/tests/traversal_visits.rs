// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
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
    steps: &[u64],
    expected: T,
    limits: (usize, usize, usize),
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let work: u64 = steps.iter().sum();
    for cap in 0..=work + 1 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = u64_from_index(limits.0);
        policy.limits.max_collection_items = u64_from_index(limits.1);
        policy.limits.max_materialized_bytes = u64_from_index(limits.2);
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = if cap < work {
            let CodecError::ResourceLimit(refusal) =
                run(&ctx).expect_err("actual traversal or copy")
            else {
                panic!("resource refusal");
            };
            let mut used = 0;
            let additional = steps
                .iter()
                .copied()
                .find(|additional| {
                    if used + additional > cap {
                        true
                    } else {
                        used += additional;
                        false
                    }
                })
                .expect("source-derived step exceeds cap");
            assert_eq!((refusal.used, refusal.additional), (used, additional));
            refusal
        } else {
            assert_eq!(
                run(&ctx).expect("exact traversal and storage bounds"),
                expected
            );
            let refusal = ctx
                .charge_work_limit(cap - work + 1, "after surface traversal")
                .expect_err("exact remaining Work");
            assert_eq!((refusal.used, refusal.additional), (work, cap - work + 1));
            refusal
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
        );
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
        let steps = if body.len() < 2 { Vec::new() } else { vec![1] };
        check_steps(&steps, None, (0, 0, 0), |ctx| {
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
    check_steps(&[], None, (0, 0, 0), |ctx| {
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
            let mut steps = Vec::new();
            let mut capacity = 0;
            let mut overlap = 0;
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
                steps.push(1); // A complete encoded contour head.
                if index == capacity {
                    if capacity != 0 {
                        overlap = capacity * std::mem::size_of::<SurfaceContourRecord>();
                        steps.push(u64_from_index(overlap));
                    }
                    capacity = (2 * capacity).max(4);
                }
                steps.push(8); // Exact retained entry bytes, without a separator.
            }
            body.extend(std::iter::repeat_n(0xff, tail));
            let retained = capacity * std::mem::size_of::<SurfaceContourRecord>() + 8 * count;
            check_steps(&steps, Some(expected), (retained, count, overlap), |ctx| {
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
        &[1, 8],
        None,
        (4 * std::mem::size_of::<SurfaceContourRecord>() + 8, 1, 0),
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
        let steps = if end < 2 { &[][..] } else { &[1][..] };
        check_steps(steps, None, (0, 0, 0), |ctx| {
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
        let steps = if end < 2 { &[][..] } else { &[1][..] };
        check_steps(steps, None, (0, 0, 0), |ctx| {
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
            let mut steps = vec![u64_from_index(size), 1, 1]; // Cache and two row walks.
            if kind == SurfaceKind::Plane {
                steps.extend(std::iter::repeat_n(
                    1,
                    size.saturating_sub(b"srf_prim_ptr(".len() - 1),
                ));
                steps.extend(std::iter::repeat_n(
                    1,
                    size.saturating_sub(b"outline\0\xf9\x02\x03".len() - 1),
                ));
            }
            check_steps(&steps, 0, (0, 0, 0), |ctx| {
                plane_envelopes_for_rows(ctx, &body, &rows).map(|records| records.len())
            });
        }
    }
    check_steps(&[], 0, (0, 0, 0), |ctx| {
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
            let mut steps = vec![u64_from_index(body.len()), 1, 1];
            steps.extend(std::iter::repeat_n(1, prefix + usize::from(header) * 2 + 1));
            steps.extend(std::iter::repeat_n(
                1,
                prefix.saturating_sub(b"outline\0\xf9\x02\x03".len() - 1),
            ));
            check_steps(&steps, 0, (0, 0, 0), |ctx| {
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
        let mut steps = vec![u64_from_index(body.len()), 1, 1];
        steps.extend(std::iter::repeat_n(
            1,
            body.len().saturating_sub(b"srf_prim_ptr(".len() - 1),
        ));
        steps.extend(std::iter::repeat_n(1, prefix + 1));
        let work: u64 = steps.iter().sum();
        for cap in 0..=work + 1 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || plane_envelopes_for_rows(&ctx, &body, &[row(SurfaceKind::Plane)]);
            let CodecError::ResourceLimit(original) =
                run().expect_err("visit or fixed slot storage")
            else {
                panic!("resource refusal");
            };
            if cap < work {
                let mut used = 0;
                let additional = steps
                    .iter()
                    .copied()
                    .find(|additional| {
                        if used + additional > cap {
                            true
                        } else {
                            used += additional;
                            false
                        }
                    })
                    .expect("source-derived step exceeds cap");
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, used, additional)
                );
            } else {
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::CollectionItems, 0, 6)
                );
                assert_eq!(original.operation, "creo surface scalar token slots");
            }
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        }
    }
}
