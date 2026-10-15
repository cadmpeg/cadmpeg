// SPDX-License-Identifier: Apache-2.0
//! Body selection preserves native identity and owns rejected candidate backing.

use crate::records::bodies::{DesignBodyBinding, DesignBodyBindingWire};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::BodySelection;
use cadmpeg_ir::ids::BodyId;

const OUTPUT_ALLOWANCE: u64 = 8192;
const SCRATCH_ALLOWANCE: u64 = 4096;

fn scope() -> DesignParameterScope {
    DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::BaseFeature,
        10,
    )
}

fn binding(suffix: u64, body: &str) -> DesignBodyBinding {
    DesignBodyBinding::try_from(DesignBodyBindingWire::<String> {
        id: "f3d:Design/BulkStream.dat:design-body-binding#10".to_owned(),
        stream: "Design/BulkStream.dat".to_owned(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 7,
        asm_body_key_offset: 10,
        entity_suffix: suffix,
        entity_suffix_offset: 18,
        blob_name: "BREP.body".to_owned(),
        blob_name_offset: 30,
        body: Some(BodyId::try_from(body).unwrap()),
    })
    .unwrap()
}

fn assert_native_only(suffixes: &[u64], bindings: &[DesignBodyBinding]) {
    let scope = scope();
    let padding = "x".repeat(usize::try_from(OUTPUT_ALLOWANCE).unwrap() - scope.id.len());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = OUTPUT_ALLOWANCE;
    policy.limits.max_materialized_bytes = SCRATCH_ALLOWANCE;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            let mut output_storage = ctx
                .provisional_retained("test actual body selection output lifetime")
                .unwrap();
            let selected = output_storage
                .with_storage(|| {
                    super::super::design_body_selection(
                        ctx,
                        &scope,
                        suffixes.iter().copied(),
                        bindings,
                    )
                })
                .unwrap();
            assert_eq!(selected, BodySelection::Native(scope.id.clone()));
            let mut padding_storage = ctx
                .provisional_retained("test remaining body selection retained allowance")
                .unwrap();
            let copied = padding_storage
                .with_storage(|| {
                    ctx.copy_retained_text(&padding, "copy remaining body selection allowance")
                })
                .unwrap();
            assert_eq!(copied.len(), padding.len());
            drop(
                ctx.reserve_scoped(SCRATCH_ALLOWANCE, "reuse body selection scratch")
                    .unwrap(),
            );
            assert_eq!(selected, BodySelection::Native(scope.id.clone()));
            drop(copied);
            drop(padding_storage);
            drop(selected);
            drop(output_storage);
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn body_selection_lifetime_missing_later_binding_refunds_owned_prefix() {
    let long = format!("f3d:model:body#{}", "a".repeat(2048));
    assert_native_only(&[20, 21], &[binding(20, &long)]);
}

#[test]
fn body_selection_lifetime_conflicting_later_binding_refunds_owned_prefix() {
    let long = format!("f3d:model:body#{}", "a".repeat(2048));
    assert_native_only(
        &[20, 21],
        &[
            binding(20, &long),
            binding(21, "f3d:model:body#1"),
            binding(21, "f3d:model:body#2"),
        ],
    );
}

#[test]
fn body_selection_lifetime_repeated_body_refunds_clones_and_distinct_index() {
    let long = format!("f3d:model:body#{}", "a".repeat(2048));
    assert_native_only(&[20, 21], &[binding(20, &long), binding(21, &long)]);
}

#[test]
fn body_selection_lifetime_survivor_keeps_source_order_and_only_live_backing() {
    let scope = scope();
    let bindings = [
        binding(20, "f3d:model:body#1"),
        binding(21, "f3d:model:body#2"),
    ];
    let body_bytes: usize = bindings
        .iter()
        .map(|binding| binding.body.as_ref().unwrap().as_str().len())
        .sum();
    let retained = body_bytes + scope.id.len() + 4 * std::mem::size_of::<BodyId>();
    let padding = "x".repeat(usize::try_from(OUTPUT_ALLOWANCE).unwrap() - retained);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = OUTPUT_ALLOWANCE;
    policy.limits.max_materialized_bytes = SCRATCH_ALLOWANCE;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            let mut output_storage = ctx
                .provisional_retained("test actual resolved body selection lifetime")
                .unwrap();
            let selected = output_storage
                .with_storage(|| {
                    super::super::design_body_selection(
                        ctx,
                        &scope,
                        [21, 20].into_iter(),
                        &bindings,
                    )
                })
                .unwrap();
            let BodySelection::Resolved { bodies, native } = &selected else {
                panic!("complete distinct bodies resolve");
            };
            assert_eq!(native, &scope.id);
            assert_eq!(
                bodies.as_slice(),
                [
                    bindings[1].body.as_ref().unwrap().clone(),
                    bindings[0].body.as_ref().unwrap().clone()
                ]
            );
            drop(
                ctx.reserve_scoped(
                    SCRATCH_ALLOWANCE,
                    "reuse body uniqueness scratch while output lives",
                )
                .unwrap(),
            );
            let mut padding_storage = ctx
                .provisional_retained("test remaining resolved body allowance")
                .unwrap();
            let copied = padding_storage
                .with_storage(|| {
                    ctx.copy_retained_text(&padding, "copy remaining resolved body allowance")
                })
                .unwrap();
            assert_eq!(copied.len(), padding.len());
            assert_eq!(native, &scope.id);
            drop(copied);
            drop(padding_storage);
            drop(selected);
            drop(output_storage);
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

fn assert_original_work_refusal(operation: &'static str) {
    let scope = scope();
    let bindings = [
        binding(20, "f3d:model:body#1"),
        binding(20, "f3d:model:body#2"),
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let Err(CodecError::ResourceLimit(original)) =
            super::super::design_body_selection(ctx, &scope, [20].into_iter(), &bindings)
        else {
            panic!("actual visited operation must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, operation);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(super::super::design_body_selection(ctx, &scope, [].into_iter(), &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn body_selection_lifetime_scope_stream_preserves_original_refusal() {
    assert_original_work_refusal("resolve F3D body selection stream");
}

#[test]
fn body_selection_lifetime_suffix_preserves_original_refusal() {
    assert_original_work_refusal("visit F3D body selection suffix");
}

#[test]
fn body_selection_lifetime_binding_visit_preserves_original_refusal() {
    assert_original_work_refusal("visit F3D body selection binding");
}

#[test]
fn body_selection_lifetime_binding_stream_preserves_original_refusal() {
    assert_original_work_refusal("resolve F3D body binding stream");
}

#[test]
fn body_selection_lifetime_stream_comparison_preserves_original_refusal() {
    assert_original_work_refusal("compare F3D body binding stream");
}

#[test]
fn body_selection_lifetime_body_comparison_preserves_original_refusal() {
    assert_original_work_refusal("compare F3D body selection identity");
}

#[test]
fn body_selection_lifetime_first_mismatch_skips_text_and_binding_suffix() {
    let scope = scope();
    let tail = "a".repeat(1024);
    let first = format!("f3d:model:body#{tail}");
    let second = format!("g3d:model:body#{tail}");
    let mut bindings = vec![binding(20, &first), binding(20, &second)];
    bindings.extend((0..64).map(|_| binding(20, &first)));
    // The scope's reverse separator search; one suffix step; two binding
    // steps, each with a reverse separator search and equal stream bytes;
    // one differing body byte. No next binding/end probe is executed.
    let stream = crate::ids::native_stream(&scope.id).unwrap();
    let binding_stream = crate::ids::native_stream(bindings[0].id()).unwrap();
    let before_native = u64::try_from(
        scope.id.len() - stream.len()
            + 1
            + 2 * (1 + bindings[0].id().len() - binding_stream.len() + stream.len())
            + 1,
    )
    .unwrap();
    let native_bytes = u64::try_from(scope.id.len()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = before_native + native_bytes;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            super::super::design_body_selection(ctx, &scope, [20].into_iter(), &bindings).unwrap(),
            BodySelection::Native(scope.id.clone())
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            super::super::design_body_selection(ctx, &scope, [20].into_iter(), &bindings)
        else {
            panic!("one below native copy must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "f3d body selection native id");
        assert_eq!(
            (original.used, original.additional),
            (before_native, native_bytes)
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(super::super::design_body_selection(ctx, &scope, [].into_iter(), &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}
