// SPDX-License-Identifier: Apache-2.0
use super::super::{
    agreed_generated_cylinder_extent, feature_outline_plane, feature_outline_planes,
    feature_plane_equations, generated_arc_cylinder_extent, generated_cap_plane_extent,
    unique_available_positional_cylinder_frame_records,
};
use super::{plane_outline, plane_row};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
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
        7, Some(7), [0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], 0,
    ).expect("section transform")
}

#[test]
fn empty_plane_queries_are_free_and_preserve_original_refusal() {
    let scan = crate::test_support::empty_container_scan();
    let ir = CadIr::empty();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let parameters = crate::surface::SurfaceParameters::from_rows(Vec::new());
    let transform = transform();
    let mut definition = crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: None, owner_feature_id: None,
        },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(),
        variables: None, segments: None, trim_entities: None, trim_vertices: None,
        order_table: None, section_3d: None, dimensions: None, relations: None,
        saved_section: None, offset: 0,
    };
    for owner in [None, Some(7)] {
        definition.identity = crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: None, owner_feature_id: owner,
        };
        let arena = DecodeArena::new();
        let policy = work_policy(0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let queries = || [
            feature_outline_plane(&ctx, &scan, 7, 31).map(|value| value.is_none()),
            feature_outline_planes(&ctx, &scan, 7).map(|value| value.expect("empty planes").0.is_empty()),
            feature_plane_equations(&ctx, &scan, &ir, &carriers, 7)
                .map(|value| value.expect("empty equations").0.is_empty()),
            generated_arc_cylinder_extent(&ctx, &scan, &ir, &carriers, &definition, &transform)
                .map(|value| value.is_none()),
            generated_cap_plane_extent(&ctx, &scan, &ir, &carriers, 7).map(|value| value.is_none()),
            unique_available_positional_cylinder_frame_records(&ctx, &BTreeSet::new(), &parameters)
                .map(|value| value.expect("empty frames").is_empty()),
            agreed_generated_cylinder_extent(&ctx, &transform, [].iter()).map(|value| value.is_none()),
        ];
        for result in queries() { assert!(result.expect("empty query")); }
        let original = ctx.charge_work_limit(1, "seed plane query refusal").expect_err("zero cap");
        assert_eq!((original.used, original.additional), (0, 1));
        for result in queries() {
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
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
        scan.planes.outlines = outlines.into_iter().map(|id| plane_outline(id, 2.0)).collect();
        scan.planes.positional_frames = positional.into_iter().map(|id| plane_outline(id, 2.0)).collect();
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = feature_outline_plane(&ctx, &scan, 917, 31);
            if cap == visits {
                let expected = present.then_some((31, [0.0, 0.0, 2.0], [0.0, 0.0, 1.0]));
                assert_eq!(result.expect("source visits admitted"), expected);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("visit limit"); };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!((original.used, original.additional), (cap, 1));
                assert!(matches!(feature_outline_plane(&ctx, &scan, 917, 31), Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!(ctx.resource_refusal(), Some(original));
            }
        }
    }
}

#[test]
fn cylinder_frame_agreement_counts_present_frames_and_stops_at_first_disagreement() {
    let transform = transform();
    let frame = |length| crate::surface::PositionalCylinderFrame::new(
        [0.0; 3], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 0.75, length,
    ).expect("cylinder frame");
    let valid = frame(Some(8.0));
    let mut missing_length = vec![frame(None)];
    missing_length.extend(std::iter::repeat_n(valid, 128));
    let mut mismatch = vec![valid, frame(Some(7.0))];
    mismatch.extend(std::iter::repeat_n(valid, 128));
    for (frames, visits, present) in [
        (Vec::new(), 0, false), (vec![valid], 1, true), (vec![valid, valid], 2, true),
        (missing_length, 1, false), (mismatch, 2, false),
    ] {
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = agreed_generated_cylinder_extent(&ctx, &transform, frames.iter());
            if cap == visits {
                let expected = present.then_some((cadmpeg_ir::features::ExtrudeExtent::OneSided {
                    side: cadmpeg_ir::features::ExtrudeSide {
                        termination: cadmpeg_ir::features::LinearTermination::Blind {
                            length: cadmpeg_ir::scalar::NonZeroLength::new(8.0).expect("nonzero length"),
                        }, draft: None,
                    },
                }, [0.0, 1.0, 0.0]));
                assert_eq!(result.expect("source visits admitted"), expected);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("visit limit"); };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!((original.used, original.additional), (cap, 1));
                assert!(matches!(agreed_generated_cylinder_extent(&ctx, &transform, frames.iter()), Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!(ctx.resource_refusal(), Some(original));
            }
        }
    }
}
