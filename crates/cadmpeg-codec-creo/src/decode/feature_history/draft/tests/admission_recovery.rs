// SPDX-License-Identifier: Apache-2.0

use super::super::{admitted_hole_placements, class_942_boundary_surface_entity_graph,
    numbered_feature_name_has_family, thicken_feature_definition};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn empty_hole_placements_keep_zero_work_and_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(admitted_hole_placements(&ctx, [None, None, None])
        .expect("absent placements need no work or storage").is_empty());
    let original = ctx.charge_work_limit(1, "before empty hole placements")
        .expect_err("zero work allowance");
    assert!(matches!(admitted_hole_placements(&ctx, [None, None, None]),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn numbered_feature_fast_recovery_keeps_zero_work_and_original_refusal() {
    for name in ["", "Thicken 3", "Extrude", "Extrude "] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(!numbered_feature_name_has_family(&ctx, name, "Extrude")
            .expect("fixed prefix and absent ordinal are free"));
        let original = ctx.charge_work_limit(1, "before numbered feature recovery")
            .expect_err("zero work allowance");
        assert!(matches!(numbered_feature_name_has_family(&ctx, name, "Extrude"),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn boundary_surface_table_recovery_visits_only_present_prefix_rows() {
    use crate::feature::entity::FeatureEntityTable;
    let surface = crate::surface::SurfaceRow {
        id: 145,
        kind: crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        feature_id: 144,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let table = |feature_id, class| FeatureEntityTable::new(
        feature_id, class, Vec::new(), &std::collections::BTreeSet::new(), 0);
    let mut duplicate_prefix = vec![table(144, 29), table(144, 29)];
    for _ in 0..512 {
        duplicate_prefix.push(table(99, 29));
    }
    // One surface row precedes the table scan. The duplicate slot stops after
    // its two present rows; no trailing row or exhausted iterator is visited.
    for (tables, table_visits) in [(Vec::new(), 0),
        (vec![table(99, 29)], 1), (duplicate_prefix, 2)] {
        let need = 1 + table_visits;
        for cap in 0..=need {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || class_942_boundary_surface_entity_graph(&ctx, 144,
                &tables, std::slice::from_ref(&surface));
            let result = run();
            if cap == need {
                assert!(!result.expect("exact present-row work keeps incomplete graph recovery"));
                let original = ctx.charge_work_limit(1, "after boundary surface recovery")
                    .expect_err("all admitted row work consumed");
                assert_eq!((original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, need, 1));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                let original = ctx.resource_refusal().expect("present row work refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                let operation = if cap == 0 { "creo boundary surface generated rows" }
                    else { "creo boundary surface entity tables" };
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, 1, operation));
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}

#[test]
fn resolved_thicken_source_has_one_present_row_refusal_boundary() {
    let scan = super::thicken_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.faces.push(super::resolved_hole_face());
    let source_refusals = std::cell::Cell::new(0);
    let definition = crate::test_support::assert_work_boundaries(
        &["creo thicken model face lookup"], |ctx| {
            let result = thicken_feature_definition(ctx, &scan, &ir, 17);
            if ctx.resource_refusal().is_some_and(|refusal|
                refusal.operation == "creo thicken source face IDs") {
                source_refusals.set(source_refusals.get() + 1);
            }
            result
        });
    // The boundary walker visits each preceding admission once, then checks
    // the exact model-face allowance. There is one source face, not an end row.
    assert_eq!(source_refusals.get(), 1);
    assert!(matches!(definition, cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Resolved { faces, native }, ..
        }) if faces == vec![super::resolved_hole_face().id]
            && native == "creo:allfeatur:thicken_source_surfaces#17:11"));
}
