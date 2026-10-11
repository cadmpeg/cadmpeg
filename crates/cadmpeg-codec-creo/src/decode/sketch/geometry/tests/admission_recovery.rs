// SPDX-License-Identifier: Apache-2.0

use super::super::saved_profile_chains;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometry, SketchGeometryDefinition, SketchId};

use std::mem::size_of;

const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;

#[test]
fn saved_profile_endpoint_candidates_admit_only_present_rows() {
    let sketch = SketchId::mint("creo:model:sketch#917").expect("sketch identity");
    for count in 0_u64..=2 {
        let geometries: Vec<_> = (0..count)
            .map(|index| {
                let position = f64::from(u32::try_from(index).expect("two rows")) * 4.0;
                (
                    u32::try_from(index).expect("two rows"),
                    SketchGeometry::try_from(SketchGeometryDefinition::Line {
                        start: cadmpeg_ir::math::Point2::new(position, 0.0),
                        end: cadmpeg_ir::math::Point2::new(position + 1.0, 0.0),
                    })
                    .expect("isolated line"),
                )
            })
            .collect();
        let observed = std::cell::RefCell::new(Vec::new());
        let profiles =
            crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = saved_profile_chains(&ctx, &sketch, &geometries);
                if let Err(CodecError::ResourceLimit(original)) = &result {
                    assert_eq!(ctx.resource_refusal(), Some(*original));
                    if original.operation == "creo saved profile endpoint candidates" {
                        assert_eq!(
                            (original.dimension, original.used, original.additional),
                            (ResourceDimension::WorkUnits, cap, 1)
                        );
                        observed.borrow_mut().push(cap);
                    }
                    assert!(matches!(saved_profile_chains(&ctx, &sketch, &geometries),
                    Err(CodecError::ResourceLimit(actual)) if actual == *original));
                }
                result
            });
        // Each row visits every candidate for both endpoints.
        assert_eq!(
            observed.borrow().len(),
            usize::try_from(2 * count * count).expect("candidate visits")
        );
        assert!(profiles.is_empty());
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            saved_profile_chains(ctx, &sketch, &geometries)
        })
        .expect("service isolated rows")
        .is_empty());
    }
}

fn profile_backing_bytes() -> u64 {
    // push_vec admits four amortized slots for the single circular use.
    // The four-element line-use Vec and outer profiles Vec also retain four
    // slots each. Five identities retain their exact formatted text lengths.
    let text_bytes: usize = [30, 10, 11, 12, 13]
        .into_iter()
        .map(|id| format!("creo:featdefs:sketch_entity#917:{id}").len())
        .sum();
    u64::try_from(
        text_bytes + 8 * size_of::<SketchEntityUse>() + 4 * size_of::<Vec<SketchEntityUse>>(),
    )
    .expect("profile backing bound")
}

fn assert_profile_identity(profiles: &[Vec<SketchEntityUse>]) {
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 1);
    assert_eq!(
        profiles[0][0].entity.as_str(),
        "creo:featdefs:sketch_entity#917:30"
    );
    assert!(!profiles[0][0].reversed);
    assert_eq!(profiles[1].len(), 4);
    for (usage, id) in profiles[1].iter().zip([10, 11, 12, 13]) {
        assert_eq!(
            usage.entity.as_str(),
            format!("creo:featdefs:sketch_entity#917:{id}")
        );
        assert!(!usage.reversed);
    }
}

#[test]
fn saved_profile_chain_transfer_keeps_only_surviving_use_backing() {
    let (sketch, geometries) = super::saved_profile_fixture();
    let bytes = profile_backing_bytes();
    crate::test_support::assert_refusal_order(ResourceDimension::RetainedBytes, &[], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = saved_profile_chains(&ctx, &sketch, &geometries);
        let refused = result.is_err();
        if refused {
            let original = ctx.resource_refusal().expect("smaller retained cap");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
            assert!(matches!(saved_profile_chains(&ctx, &sketch, &geometries),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            assert_profile_identity(&result.expect("exact surviving backing"));
            let original = ctx
                .charge_retained_limit(1, "after retained profile")
                .expect_err("all retained backing consumed");
            assert_eq!((original.used, original.additional), (bytes, 1));
        }
        if refused {
            Err(ctx.resource_refusal().expect("smaller retained cap").into())
        } else {
            Ok(())
        }
    });
}

#[test]
fn saved_profile_chain_transfer_preserves_actual_parent_overlap_and_cleanup() {
    let (sketch, geometries) = super::saved_profile_fixture();
    let bytes = profile_backing_bytes();
    for overflow in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut parent = ctx
            .reserve_scoped(37, "saved profile parent")
            .expect("parent");
        let profiles = parent
            .with_storage(|| saved_profile_chains(&ctx, &sketch, &geometries))
            .expect("profiles remain under actual parent");
        assert_profile_identity(&profiles);
        let available = EMPTY_ROOT_MATERIALIZED_BYTES - bytes - 37;
        let probe = ctx.reserve_scoped_limit(
            available + u64::from(overflow),
            "saved profile live backing",
        );
        if overflow {
            let original = probe.expect_err("one byte above actual surviving backing");
            assert_eq!(
                (
                    original.dimension,
                    original.used,
                    original.additional,
                    original.operation
                ),
                (
                    ResourceDimension::MaterializedBytes,
                    bytes + 37,
                    available + 1,
                    "saved profile live backing"
                )
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
        } else {
            drop(probe.expect("exact live backing"));
        }
        drop(profiles);
        drop(parent);
        if !overflow {
            drop(
                ctx.reserve_scoped(EMPTY_ROOT_MATERIALIZED_BYTES, "saved profile cleanup")
                    .expect("all scratch and parent backing released"),
            );
            let original = ctx
                .charge_retained_limit(1, "saved profile retained cleanup")
                .expect_err("retained zero policy");
            assert_eq!((original.used, original.additional), (0, 1));
        }
    }
}
