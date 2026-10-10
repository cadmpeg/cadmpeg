// SPDX-License-Identifier: Apache-2.0

use super::super::insert_feature_parameter;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
use std::mem::{align_of, size_of};

const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;
const PARAMETER_KEY: &str = "choice.value";
const PARAMETER_VALUE: &str = "V";

#[test]
fn feature_parameter_key_transfer_keeps_exact_live_storage_under_ambient_parent() {
    // The formatter requests exact text growth. One map entry raises the
    // core tree bound by one node: eleven key/value lanes, sixteen pointer
    // lanes and two alignment widths. The actual key buffer transfers once.
    let alignment = align_of::<String>().max(align_of::<usize>());
    let live_bytes = u64::try_from(
        PARAMETER_KEY.len()
            + PARAMETER_VALUE.len()
            + 11 * (size_of::<String>() + size_of::<String>())
            + 16 * size_of::<usize>()
            + 2 * alignment,
    )
    .expect("parameter storage bound");
    for nested in [false, true] {
        for overflow in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let parent_bytes = if nested { 37 } else { 0 };
            let mut parent = ctx
                .reserve_scoped(parent_bytes, "test live parameter parent")
                .expect("parent storage");
            let available = EMPTY_ROOT_MATERIALIZED_BYTES - live_bytes - parent_bytes;
            let run = || {
                let mut text_storage = ctx.reserve_scoped(0, "creo feature parameter text")?;
                let mut node_storage = ctx.reserve_scoped(0, "creo feature parameter nodes")?;
                let mut parameters = BTreeMap::new();
                insert_feature_parameter(
                    &ctx,
                    &mut text_storage,
                    &mut node_storage,
                    &mut parameters,
                    PARAMETER_KEY,
                    PARAMETER_VALUE,
                )?;
                assert_eq!(parameters.len(), 1);
                assert_eq!(
                    parameters.get(PARAMETER_KEY).map(String::as_str),
                    Some(PARAMETER_VALUE)
                );
                let probe = ctx.reserve_scoped(
                    available + u64::from(overflow),
                    "live feature parameter storage",
                )?;
                drop(probe);
                drop((parameters, node_storage, text_storage));
                let probe = ctx.reserve_scoped(
                    EMPTY_ROOT_MATERIALIZED_BYTES - parent_bytes,
                    "after feature parameter storage",
                )?;
                drop(probe);
                Ok::<(), CodecError>(())
            };
            let result = if nested {
                parent.with_storage(run)
            } else {
                run()
            };
            drop(parent);
            if overflow {
                let original = ctx
                    .resource_refusal()
                    .expect("one byte beyond live storage");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert_eq!(
                    (
                        original.dimension,
                        original.used,
                        original.additional,
                        original.operation
                    ),
                    (
                        ResourceDimension::MaterializedBytes,
                        live_bytes + parent_bytes,
                        available + 1,
                        "live feature parameter storage"
                    )
                );
                assert_eq!(
                    ctx.charge_retained_limit(1, "after refused parameter storage"),
                    Err(original)
                );
            } else {
                result.expect("key transfers within exact live allowance");
                let probe = ctx
                    .reserve_scoped(
                        EMPTY_ROOT_MATERIALIZED_BYTES,
                        "after parameter parent storage",
                    )
                    .expect("all temporary storage ended");
                drop(probe);
                let original = ctx
                    .charge_retained_limit(1, "after temporary parameter storage")
                    .expect_err("parameter staging retained no session storage");
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::RetainedBytes, 0, 1)
                );
            }
        }
    }
}

#[test]
fn returned_feature_parameter_text_transfers_only_surviving_duplicate_keys() {
    let mut scan = crate::test_support::empty_container_scan();
    for value in [3, 4] {
        scan.features
            .choice_fields
            .push(crate::feature::rows::FeatureChoiceField {
                feature_id: 40,
                choice_label: "C".into(),
                name: "value".into(),
                type_byte: 1,
                value: crate::feature::rows::FeatureFieldValue::CompactInt(value),
                offset: 0,
            });
    }
    let first_key = "choice.C.value";
    let second_key = "choice.C.value#2";
    let text_bytes = u64::try_from(first_key.len() + second_key.len() + 2)
        .expect("two surviving keys and one digit per value");
    crate::test_support::assert_refusal_order(
        ResourceDimension::RetainedBytes,
        &["creo feature parameter text"],
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let parts = super::super::feature_parameters(&ctx, &scan, 40)
                .expect("parameter text and nodes are scoped");
            let text_storage = parts.1;
            let node_storage = parts.2;
            let parameters = parts.0;
            assert_eq!(parameters.len(), 2);
            assert_eq!(parameters.get(first_key).map(String::as_str), Some("3"));
            assert_eq!(parameters.get(second_key).map(String::as_str), Some("4"));
            let result = text_storage.commit_value(parameters);
            if cap == text_bytes {
                let parameters = result.expect("exact surviving text bound");
                let original = ctx
                    .charge_retained_limit(1, "after retained parameter map")
                    .expect_err("exact surviving text remains retained");
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::RetainedBytes, text_bytes, 1)
                );
                assert_eq!(parameters.get(first_key).map(String::as_str), Some("3"));
                assert_eq!(parameters.get(second_key).map(String::as_str), Some("4"));
                drop((parameters, node_storage));
            } else {
                let original = ctx.resource_refusal().expect("retained text refusal");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert_eq!(
                    (
                        original.dimension,
                        original.limit,
                        original.used,
                        original.additional,
                        original.operation
                    ),
                    (
                        ResourceDimension::RetainedBytes,
                        cap,
                        0,
                        text_bytes,
                        "creo feature parameter text"
                    )
                );
                drop(node_storage);
                assert!(matches!(super::super::feature_parameters(&ctx, &scan, 40),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
            }

            if cap < text_bytes {
                Err(ctx
                    .resource_refusal()
                    .expect("retained text refusal")
                    .into())
            } else {
                Ok::<_, CodecError>(())
            }
        },
    );
}
