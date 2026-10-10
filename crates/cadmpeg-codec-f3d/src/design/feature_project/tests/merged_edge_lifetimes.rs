// SPDX-License-Identifier: Apache-2.0
//! Edge merge visits, comparison prefixes, and rejected owned candidates.

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::EdgeSelection;
use cadmpeg_ir::ids::{EdgeId, FeatureInputTopologyId, HistoricalEdgeId};

const OUTPUT_ALLOWANCE: u64 = 8192;
const SCRATCH_ALLOWANCE: u64 = 4096;

fn scope() -> DesignParameterScope {
    DesignParameterScope::empty("f3d:test:scope#100", DesignFeatureKind::Thread, 100)
}

fn historical(state: &str, edges: &[&str]) -> EdgeSelection {
    EdgeSelection::historical(
        FeatureInputTopologyId::mint(state).unwrap(),
        edges
            .iter()
            .map(|id| HistoricalEdgeId::mint(*id).unwrap())
            .collect(),
        scope().id,
        &cadmpeg_test_support::service_decode_context(),
    )
    .unwrap()
    .unwrap()
}

fn assert_only_native_retained(selections: &[EdgeSelection]) {
    let scope = scope();
    let padding = "x".repeat(usize::try_from(OUTPUT_ALLOWANCE).unwrap() - scope.id.len());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = OUTPUT_ALLOWANCE;
    policy.limits.max_materialized_bytes = SCRATCH_ALLOWANCE;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            let mut output_storage = ctx
                .provisional_retained("test actual merge output lifetime")
                .unwrap();
            let selected = output_storage
                .with_storage(|| super::super::merge_edge_selections(ctx, &scope, selections))
                .unwrap();
            assert_eq!(selected, EdgeSelection::Native(scope.id.clone()));
            let mut padding_storage = ctx
                .provisional_retained("test actual remaining retained allowance")
                .unwrap();
            let copied = padding_storage
                .with_storage(|| ctx.copy_retained_text(&padding, "copy remaining merge allowance"))
                .unwrap();
            assert_eq!(copied.len(), padding.len());
            assert_eq!(selected, EdgeSelection::Native(scope.id.clone()));
            drop(
                ctx.reserve_scoped(SCRATCH_ALLOWANCE, "reuse merge temporary backing")
                    .unwrap(),
            );
            drop(copied);
            drop(padding_storage);
            drop(selected);
            drop(output_storage);
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn merged_edge_lifetime_direct_duplicate_discards_all_candidate_storage() {
    let long = format!("f3d:brep:edge#{}", "a".repeat(2048));
    let first = EdgeId::mint(&long).unwrap();
    let second = EdgeId::mint("f3d:brep:edge#second").unwrap();
    assert_only_native_retained(&[
        EdgeSelection::Edges(vec![first.clone(), second]),
        EdgeSelection::Edges(vec![first]),
    ]);
}

#[test]
fn merged_edge_lifetime_historical_duplicate_discards_state_and_edges() {
    let long = format!("f3d:history:edge#{}", "a".repeat(2048));
    let first = historical("f3d:history:state#7", &[&long, "f3d:history:edge#2"]);
    let duplicate = historical("f3d:history:state#7", &[&long]);
    assert_only_native_retained(&[first, duplicate]);
}

#[test]
fn merged_edge_lifetime_historical_survivor_keeps_order_and_releases_uniqueness_scratch() {
    let first = historical("f3d:history:state#7", &["f3d:history:edge#2"]);
    let second = historical("f3d:history:state#7", &["f3d:history:edge#1"]);
    let expected = historical(
        "f3d:history:state#7",
        &["f3d:history:edge#2", "f3d:history:edge#1"],
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_ALLOWANCE;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let selected =
            super::super::merge_edge_selections(ctx, &scope(), &[first, second]).unwrap();
        assert_eq!(selected, expected);
        drop(
            ctx.reserve_scoped(SCRATCH_ALLOWANCE, "reuse historical uniqueness scratch")
                .unwrap(),
        );
        assert_eq!(selected, expected);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn merged_edge_lifetime_classifier_stops_before_unvisited_suffix() {
    let selections: Vec<_> = (0..64).map(|_| EdgeSelection::Unresolved).collect();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            super::super::merge_edge_selections(ctx, &scope(), &selections)
        else {
            panic!("native copy refuses after sole visited classifier");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "f3d merged edge native id");
        assert_eq!(
            (original.used, original.additional),
            (1, u64::try_from(scope().id.len()).unwrap())
        );
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(super::super::merge_edge_selections(ctx, &scope(), &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn merged_edge_lifetime_direct_identity_refuses_before_comparing_first_byte() {
    let first = EdgeId::mint("f3d:brep:edge#1").unwrap();
    let second = EdgeId::mint("f3d:brep:edge#2").unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "f3d merged direct edge identity comparison",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) = super::super::merge_edge_selections(
            ctx,
            &scope(),
            &[EdgeSelection::Edges(vec![first, second])],
        ) else {
            panic!("actual duplicate comparison refuses before byte");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(
            original.operation,
            "f3d merged direct edge identity comparison"
        );
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(super::super::merge_edge_selections(ctx, &scope(), &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn merged_edge_lifetime_historical_state_refuses_before_comparing_first_byte() {
    let selected = historical("f3d:history:state#7", &["f3d:history:edge#1"]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "f3d merged historical state identity comparison",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) =
            super::super::merge_edge_selections(ctx, &scope(), &[selected])
        else {
            panic!("actual state comparison refuses before byte");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(
            original.operation,
            "f3d merged historical state identity comparison"
        );
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(super::super::merge_edge_selections(ctx, &scope(), &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn merged_edge_lifetime_historical_edge_refuses_before_comparing_first_byte() {
    let selected = historical(
        "f3d:history:state#7",
        &["f3d:history:edge#1", "f3d:history:edge#2"],
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "f3d merged historical edge identity comparison",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) =
            super::super::merge_edge_selections(ctx, &scope(), &[selected])
        else {
            panic!("actual duplicate comparison refuses before byte");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(
            original.operation,
            "f3d merged historical edge identity comparison"
        );
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(super::super::merge_edge_selections(ctx, &scope(), &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn merged_edge_lifetime_historical_state_mismatch_does_not_visit_suffix() {
    let first_state = "f3d:history:state#7";
    let first = historical(first_state, &["f3d:history:edge#1"]);
    let mismatch = historical("g3d:history:state#7", &["f3d:history:edge#2"]);
    let mut selections = vec![first, mismatch];
    selections.extend((0..64).map(|_| EdgeSelection::Unresolved));
    let scope = scope();
    let mut policy = DecodePolicy::service();
    // One direct classifier visit, two historical visits, the first state's
    // equal bytes, one mismatch byte, and the final native text copy.
    let work = u64::try_from(first_state.len() + scope.id.len()).unwrap() + 4;
    policy.limits.max_work_units = work;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            super::super::merge_edge_selections(ctx, &scope, &selections).unwrap(),
            EdgeSelection::Native(scope.id.clone())
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units = work - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            super::super::merge_edge_selections(ctx, &scope, &selections)
        else {
            panic!("one below final native copy");
        };
        assert_eq!(original.operation, "f3d merged edge native id");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(
            (original.used, original.additional),
            (
                u64::try_from(first_state.len()).unwrap() + 4,
                u64::try_from(scope.id.len()).unwrap()
            )
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn merged_edge_lifetime_direct_first_mismatch_does_not_compare_text_suffix() {
    let tail = "a".repeat(1024);
    let first_text = format!("f3d:brep:edge#{tail}");
    let second_text = format!("g3d:brep:edge#{tail}");
    let first = EdgeId::mint(&first_text).unwrap();
    let second = EdgeId::mint(&second_text).unwrap();
    let selections = [EdgeSelection::Edges(vec![first.clone(), second.clone()])];
    let scope = scope();
    let expected = EdgeSelection::Resolved {
        edges: vec![first, second],
        native: scope.id.clone(),
    };
    let mut policy = DecodePolicy::service();
    // One classifier visit, one duplicate candidate visit, one differing byte;
    // the two identity copies and final native copy remain paid in full.
    // Two pushes share the first four-slot vector allocation and move no bytes.
    let before_native = u64::try_from(first_text.len() + second_text.len()).unwrap() + 3;
    let work = before_native + u64::try_from(scope.id.len()).unwrap();
    policy.limits.max_work_units = work;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            super::super::merge_edge_selections(ctx, &scope, &selections).unwrap(),
            expected
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units = work - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            super::super::merge_edge_selections(ctx, &scope, &selections)
        else {
            panic!("one below final native copy");
        };
        assert_eq!(original.operation, "f3d merged direct native id");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(
            (original.used, original.additional),
            (before_native, u64::try_from(scope.id.len()).unwrap())
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}
