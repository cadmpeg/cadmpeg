// SPDX-License-Identifier: Apache-2.0
use super::super::{
    agreed_generated_cylinder_extent, feature_outline_plane, generated_cap_plane_extent,
};
use super::{plane_outline, plane_row};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::document::CadIr;
use std::collections::BTreeSet;

fn work_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn transform() -> crate::placement::FeatureSectionTransform {
    crate::placement::FeatureSectionTransform::new(
        7,
        Some(7),
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        0,
    )
    .expect("section transform")
}

#[test]
fn outline_plane_queries_visit_present_carriers_and_stop_at_second_match() {
    let mut duplicate = vec![31, 31];
    duplicate.extend(std::iter::repeat_n(99, 128));
    let mut late_duplicate = vec![99, 31, 31];
    late_duplicate.extend(std::iter::repeat_n(99, 128));
    for (outlines, positional, visits, present) in [
        (Vec::new(), Vec::new(), 0, false),
        (vec![31, 98, 99], Vec::new(), 3, true),
        (Vec::new(), vec![31, 98, 99], 3, true),
        (vec![31, 99], vec![98, 31, 99], 5, true),
        (vec![98, 99], vec![96, 97, 98], 5, false),
        (duplicate.clone(), vec![31; 128], 2, false),
        (late_duplicate, vec![31; 128], 3, false),
        (vec![31, 98, 99], duplicate, 5, false),
    ] {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.push(plane_row(31));
        scan.planes.outlines = outlines
            .into_iter()
            .map(|id| plane_outline(id, 2.0))
            .collect();
        scan.planes.positional_frames = positional
            .into_iter()
            .map(|id| plane_outline(id, 2.0))
            .collect();
        let mut operations = Vec::new();
        if !scan.planes.outlines.is_empty() {
            operations.push("creo feature outline plane search");
        }
        if visits > u64::try_from(scan.planes.outlines.len()).expect("fixture count") {
            operations.push("creo feature positional plane search");
        }
        crate::test_support::assert_work_boundaries(
            if visits == 0 { &[] } else { &operations },
            |ctx| feature_outline_plane(ctx, &scan, 917, 31),
        );
        let arena = DecodeArena::new();
        let policy = work_policy(visits);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = feature_outline_plane(&ctx, &scan, 917, 31);
        let expected = present.then_some((31, [0.0, 0.0, 2.0], [0.0, 0.0, 1.0]));
        assert_eq!(result.expect("source visits admitted"), expected);
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn cylinder_frame_agreement_counts_present_frames_and_stops_at_first_disagreement() {
    let transform = transform();
    let frame = |length| {
        crate::surface::PositionalCylinderFrame::new(
            [0.0; 3],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            0.75,
            length,
        )
        .expect("cylinder frame")
    };
    let valid = frame(Some(8.0));
    let mut missing_length = vec![frame(None)];
    missing_length.extend(std::iter::repeat_n(valid, 128));
    let mut mismatch = vec![valid, frame(Some(7.0))];
    mismatch.extend(std::iter::repeat_n(valid, 128));
    for (frames, visits, present) in [
        (Vec::new(), 0, false),
        (vec![valid], 1, true),
        (vec![valid, valid], 2, true),
        (missing_length, 1, false),
        (mismatch, 2, false),
    ] {
        crate::test_support::assert_work_boundaries(
            match visits {
                0 => &[],
                1 => &["creo generated cylinder frame scan"],
                _ => &[
                    "creo generated cylinder frame scan",
                    "creo generated cylinder frame agreement",
                ],
            },
            |ctx| agreed_generated_cylinder_extent(ctx, &transform, frames.iter()),
        );
        let arena = DecodeArena::new();
        let policy = work_policy(visits);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = agreed_generated_cylinder_extent(&ctx, &transform, frames.iter());
        let expected = present.then_some((
            cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(8.0)
                            .expect("nonzero length"),
                    },
                    draft: None,
                },
            },
            [0.0, 1.0, 0.0],
        ));
        assert_eq!(result.expect("source visits admitted"), expected);
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn invalid_generated_cap_rows_stop_before_plane_preparation() {
    use crate::feature::entity::{FeatureEntityTable, FeatureEntityTableEntry};
    // One unique table plus its three present entries is all reached
    // input work. The two indexed cap-row checks are fixed work.
    const VISITS: u64 = 1 + 3;

    let entry = |entity_id, class_id, source_entity_id| FeatureEntityTableEntry {
        entity_id,
        payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let table = FeatureEntityTable::new(
        917,
        29,
        vec![
            entry(31, 204, None),
            entry(32, 203, None),
            entry(33, 200, Some(11)),
        ],
        &BTreeSet::new(),
        0,
    )
    .with_surface_ids([31, 32, 33]);
    for invalid_id in [31, 32] {
        for invalid_kind in 0..3 {
            let mut scan = crate::test_support::empty_container_scan();
            scan.features.entity_tables.push(table.clone());
            for id in [31, 32] {
                let mut row = plane_row(id);
                if id == invalid_id {
                    match invalid_kind {
                        0 => continue,
                        1 => row.feature_id = 918,
                        _ => row.kind = crate::surface::SurfaceKind::Cylinder,
                    }
                }
                scan.surfaces.rows.push(row);
            }
            // Plane preparation would allocate nodes for these unrelated
            // outlines. Neither cap row pair can support an extent.
            scan.planes.outlines.push(plane_outline(99, 2.0));
            let mut ir = CadIr::empty();
            ir.model.surfaces.push(super::plane_surface(32, 8.0));
            crate::test_support::assert_work_boundaries(
                &[
                    "creo generated cap table search",
                    "creo generated cap entry scan",
                ],
                |ctx| {
                    let carriers =
                        crate::decode::source_carriers::SourceUnitCarriers::for_decode(ctx, None);
                    generated_cap_plane_extent(ctx, &scan, &ir, &carriers, 917)
                },
            );
            let arena = DecodeArena::new();
            let policy = work_policy(VISITS);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let carriers =
                crate::decode::source_carriers::SourceUnitCarriers::for_decode(&ctx, None);
            assert!(generated_cap_plane_extent(&ctx, &scan, &ir, &carriers, 917)
                .expect("absent cap rows need no plane preparation")
                .is_none());
            assert_eq!(ctx.resource_refusal(), None);
            let original = ctx
                .charge_work_limit(1, "after absent generated cap rows")
                .expect_err("exact reached work");
            assert_eq!(
                (original.dimension, original.used, original.additional),
                (ResourceDimension::WorkUnits, VISITS, 1)
            );
        }
    }
}
