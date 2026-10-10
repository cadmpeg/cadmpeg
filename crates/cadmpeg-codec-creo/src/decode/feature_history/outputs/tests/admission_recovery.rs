// SPDX-License-Identifier: Apache-2.0

use super::super::{bodies_containing_edges, evaluated_sweep_body_kind, new_sheet_output_surface_id};
use crate::feature::entity::FeatureEntityTable;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{EdgeId, FaceId, RegionId, ShellId};
use cadmpeg_ir::topology::Shell;
use std::collections::BTreeSet;

#[test]
fn evaluated_sweep_family_recovery_is_free_and_preserves_original_refusal() {
    let ir = CadIr::empty();
    for family in ["", "unknown", "extrusion", "revolution"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || evaluated_sweep_body_kind(&ctx, &ir, family, 40);
        assert_eq!(run().expect("family and absent body need no variable work"), None);
        let original = ctx.charge_work_limit(1, "after sweep family recovery")
            .expect_err("zero Work cap");
        assert_eq!((original.dimension, original.used, original.additional),
            (ResourceDimension::WorkUnits, 0, 1));
        assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn new_sheet_table_recovery_admits_only_present_rows_and_stops_at_duplicate_owner() {
    let table = |feature_id, class_id| FeatureEntityTable::new(
        feature_id, class_id, Vec::new(), &BTreeSet::new(), 0,
    );
    let mut duplicate = vec![table(40, 67), table(40, 67)];
    duplicate.extend((0..4094).map(|_| table(99, 67)));
    for (tables, total) in [(Vec::new(), 0), (vec![table(99, 67)], 1), (duplicate, 2)] {
        // An absent source visits zero rows; the unrelated table visits one.
        // The second owner table proves ambiguity before the unrelated tail.
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &if total == 0 { Vec::new() } else { vec!["creo new sheet entity tables"] }, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || new_sheet_output_surface_id(&ctx, 40, &tables, &[]);
            let result = run();
            if cap == total {
                assert_eq!(result.expect("exact present-row cap"), None);
                let original = ctx.charge_work_limit(1, "after sheet table recovery")
                    .expect_err("exact cap");
                assert_eq!((original.used, original.additional), (total, 1));
            } else {
                let original = ctx.resource_refusal().expect("present row refuses");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, 1, "creo new sheet entity tables"));
            }
            let original = ctx.resource_refusal().expect("original refusal");
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(new_sheet_output_surface_id(&ctx, 40, &[], &[]),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(ctx.resource_refusal(), Some(original));

            if cap < total {
                Err(ctx.resource_refusal().expect("present visit refusal").into())
            } else {
                Ok::<_, CodecError>(())
            }
});
    }
}

#[test]
fn selected_face_shell_without_wire_rows_has_no_exhaustion_charge() {
    let mut ir = CadIr::empty();
    ir.model.shells.push(Shell::with_face(
        ShellId::mint("creo:test:shell#1").expect("shell identity"),
        RegionId::mint("creo:test:region#1").expect("region identity"),
        FaceId::mint("creo:test:face#1").expect("face identity"),
    ));
    let edge = EdgeId::mint("creo:test:edge#1").expect("edge identity");
    let charged_absent_row = std::cell::Cell::new(false);
    crate::test_support::assert_work_boundaries(&["creo selected shell lookup"], |ctx| {
        let result = bodies_containing_edges(ctx, &ir, std::slice::from_ref(&edge));
        if let Some(resource) = ctx.resource_refusal() {
            if resource.operation == "creo selected shell wire edges" {
                charged_absent_row.set(true);
            }
        }
        result.map(|candidates| assert!(candidates.bodies.is_empty()))
    });
    // The shell visit is the last executed variable operation. At its exact
    // cap the shell has no wire row to visit, and no body is selected.
    assert!(!charged_absent_row.get());
}
