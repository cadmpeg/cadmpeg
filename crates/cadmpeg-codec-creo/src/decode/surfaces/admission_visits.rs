// SPDX-License-Identifier: Apache-2.0

use super::{
    fc05_cap_pair_model_frame, matches_native_surface_id, native_surface_namespace,
    transfer_part_product, Fc05CapPairFrame,
};
use crate::curve::{Fc05CapEdge, Fc05CylinderCapPair, ParameterSense};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn pair(count: usize) -> Fc05CylinderCapPair {
    Fc05CylinderCapPair {
        surface_id: 2,
        cap_edges: (0..count)
            .map(|index| Fc05CapEdge {
                curve_id: u32::try_from(index).expect("fixture index fits u32") + 1,
                cap_plane_id: u32::try_from(index).expect("fixture index fits u32") + 1,
                cap_ordinate_row_frame: f64::from(
                    u32::try_from(index).expect("fixture index fits u32"),
                ),
            })
            .collect(),
        center_row_frame: [0.0, 0.0],
        radius_mm: 1.0,
        reference_direction_row_frame: [1.0, 0.0],
        parameter_sense: ParameterSense::Increasing,
        cap_ordinates_row_frame: (0..count)
            .map(|index| f64::from(u32::try_from(index).expect("fixture index fits u32")))
            .collect(),
        offset: 0,
    }
}

#[test]
fn cap_pair_frames_admit_only_present_edges_and_stop_at_missing_outline() {
    for count in [2, 3, 8] {
        let mut scan = crate::test_support::empty_container_scan();
        for index in 0..count {
            scan.planes.outlines.push(crate::surface::OutlinePlane {
                surface_id: u32::try_from(index).expect("fixture index fits u32") + 1,
                origin: [
                    0.0,
                    f64::from(u32::try_from(index).expect("fixture index fits u32")),
                    0.0,
                ],
                normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
                u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
                offset: 0,
            });
        }
        let indexed_arena = DecodeArena::new();
        let indexed_policy = DecodePolicy::service();
        let (indexed_ctx, _) = DecodeContext::from_root_bytes(&[], &indexed_arena, &indexed_policy)
            .expect("index root");
        let outlines = super::native_ids::UniqueRows::new(
            &indexed_ctx,
            &scan.planes.outlines,
            |plane| Some(plane.surface_id),
            "test cap outline index",
        )
        .expect("fixture index");
        for missing in [false, true] {
            let mut pair = pair(count);
            if missing {
                pair.cap_edges[0].cap_plane_id = u32::MAX;
            }
            // This owner visits one present edge before a missing first outline,
            // or all n edges for a complete frame. The terminal None is free.
            let total = if missing {
                1
            } else {
                u64::try_from(count).expect("fixture count fits u64")
            };
            let run = |ctx: &DecodeContext<'_>| {
                let frame = Fc05CapPairFrame::from_outlines(ctx, &pair, &outlines)?;
                assert_eq!(frame.is_none(), missing);
                if let Some(frame) = frame {
                    assert_eq!(frame.origin, [0.0; 3]);
                    assert_eq!(frame.unit_vector(), [0.0, 1.0, 0.0]);
                    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
                }
                Ok(())
            };
            let policy_at = |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                policy
            };
            crate::test_support::assert_refusal_order(
                ResourceDimension::WorkUnits,
                &vec![
                    "creo cap pair placed edge traversal";
                    usize::try_from(total).expect("edge count")
                ],
                |cap| {
                    let arena = DecodeArena::new();
                    let policy = policy_at(cap);
                    let (ctx, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    let result = run(&ctx);
                    if let Err(CodecError::ResourceLimit(original)) = &result {
                        assert_eq!((original.limit, original.used), (cap, cap));
                        assert_eq!(original.additional, 1);
                        assert_eq!(ctx.resource_refusal().as_ref(), Some(original));
                        assert!(
                            matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == *original)
                        );
                    }
                    result
                },
            );
            let arena = DecodeArena::new();
            let policy = policy_at(total);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            run(&ctx).expect("exact present-edge cap");
            let original = ctx
                .charge_work_limit(1, "after cap pair visits")
                .expect_err("exact cap");
            assert_eq!(
                (original.dimension, original.used, original.additional),
                (ResourceDimension::WorkUnits, total, 1)
            );
            assert!(
                matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
        }
    }
}

#[test]
fn surface_fixed_returns_are_free_and_preserve_original_refusal() {
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let outlines = super::native_ids::UniqueRows::new(
        &ctx,
        &scan.planes.outlines,
        |plane| Some(plane.surface_id),
        "test empty outlines",
    )
    .expect("empty index");
    let check = |refused| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let mut results =
            vec![
                transfer_part_product(&ctx, &scan, &mut ir, &mut annotations, &carriers)
                    .map(|present| !present),
            ];
        for count in [0, 1] {
            let pair = pair(count);
            results.push(
                Fc05CapPairFrame::from_outlines(&ctx, &pair, &outlines)
                    .map(|frame| frame.is_none()),
            );
            results
                .push(fc05_cap_pair_model_frame(&ctx, &scan, &pair).map(|frame| frame.is_none()));
        }
        for nonvisible in [false, true] {
            let mut selected = crate::test_support::empty_container_scan();
            let row = crate::surface::SurfaceRow {
                id: 7,
                kind: crate::surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: 0,
            };
            let (rows, namespace, prefix) = if nonvisible {
                (
                    &mut selected.surfaces.nonvisible_rows,
                    crate::identity::NOVISGEOM_SURFACE,
                    "creo:novisgeom:surface#",
                )
            } else {
                (
                    &mut selected.surfaces.rows,
                    crate::identity::VISIBGEOM_SURFACE,
                    "creo:visibgeom:surface#",
                )
            };
            rows.push(row);
            let candidate = cadmpeg_ir::ids::SurfaceId::compose(&namespace, 7);
            results.push(
                native_surface_namespace(&ctx, &selected, 7)
                    .map(|actual| actual == (namespace, prefix)),
            );
            results.push(matches_native_surface_id(&ctx, &selected, 7, &candidate));
        }
        results.push(native_surface_namespace(&ctx, &scan, 7).map(|actual| {
            actual
                == (
                    crate::identity::VISIBGEOM_SURFACE,
                    "creo:visibgeom:surface#",
                )
        }));
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                assert!(result.expect("fixed route"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx
        .charge_work_limit(1, "after surface fixed returns")
        .expect_err("zero cap");
    assert_eq!(
        (original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1)
    );
    check(true);
    assert_eq!(ctx.resource_refusal(), Some(original));
}
