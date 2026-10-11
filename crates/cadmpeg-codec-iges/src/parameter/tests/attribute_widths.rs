// SPDX-License-Identifier: Apache-2.0

use super::integer_parameter_record;
use crate::parameter::{attribute_table_instance_primary_end, AttributeDefinitionWidths};
use crate::test_support::directory_target;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn attribute_instances_reuse_valid_and_invalid_definition_widths() {
    const ATTRIBUTES: usize = 4_000;
    const INSTANCES: usize = 4_000;
    for overflow in [false, true] {
        for invalid in [false, true] {
            let mut tokens = vec![322, 0, 1, i64::try_from(ATTRIBUTES).unwrap()];
            for _ in 0..ATTRIBUTES {
                tokens.extend([10, 1, 0]);
            }
            if invalid {
                *tokens.last_mut().unwrap() = -1;
            }
            let definition = integer_parameter_record(1, &tokens);
            let definition_entry = directory_target(1, 322);
            let mut entry = directory_target(3, 422);
            entry.structure = -1;
            let record = integer_parameter_record(3, &[422, 9]);
            let directory = BTreeMap::from([(1, &definition_entry)]);
            let records = BTreeMap::from([(1, &definition)]);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            // One descriptor scan plus bounded tree work per instance.
            policy.limits.max_work_units = u64::try_from(ATTRIBUTES + 100 * INSTANCES).unwrap();
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut widths = AttributeDefinitionWidths::new(&ctx).unwrap();
            if overflow {
                let first = integer_parameter_record(7, &[322, 0, 1, 1, 10, 1, 1]);
                assert_eq!(widths.width(&first, &ctx).unwrap(), Some(1));
            }
            for _ in 0..INSTANCES {
                assert_eq!(
                    attribute_table_instance_primary_end(
                        &record,
                        &entry,
                        &directory,
                        &records,
                        &mut widths,
                        &ctx,
                    )
                    .unwrap(),
                    if invalid { 2 } else { 1 }
                );
            }
            let expected = if invalid {
                Err(crate::parameter::AttributeWidthDefect::ValueCount {
                    attribute: ATTRIBUTES - 1,
                })
            } else {
                Ok(0)
            };
            if overflow {
                assert_eq!(
                    widths
                        .first
                        .map(|(sequence, width)| (sequence, width.values_per_row)),
                    Some((7, Ok(1)))
                );
                assert_eq!(widths.values.len(), 1);
                assert_eq!(widths.values[&1].values_per_row, expected);
            } else {
                assert_eq!(
                    widths
                        .first
                        .map(|(sequence, width)| (sequence, width.values_per_row)),
                    Some((1, expected))
                );
                assert!(widths.values.is_empty());
            }
            drop(widths);
            let CodecError::ResourceLimit(limit) = ctx
                .reserve_scoped(u64::MAX, "test released attribute widths")
                .unwrap_err()
            else {
                panic!("expected storage refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.used, 0);
        }
    }
}

#[test]
fn attribute_width_cache_admits_nodes_before_insertion() {
    let record = integer_parameter_record(1, &[322, 0, 1, 1, 10, 1, 1]);
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
    ] {
        let operation = "iges parameter attribute width nodes";
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut widths = AttributeDefinitionWidths::new(&ctx)?;
            let first = integer_parameter_record(3, &[322, 0, 1, 1, 10, 1, 1]);
            assert_eq!(widths.width(&first, &ctx)?, Some(1));
            widths.width(&record, &ctx)
        });
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected cache node refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        let amount = if dimension == ResourceDimension::CollectionItems {
            1
        } else {
            use std::mem::{align_of, size_of};
            u64::try_from(
                11 * (size_of::<u32>() + size_of::<crate::parameter::AttributeDefinitionWidth>())
                    + 16 * size_of::<usize>()
                    + 2 * align_of::<u32>()
                        .max(align_of::<crate::parameter::AttributeDefinitionWidth>())
                        .max(align_of::<usize>()),
            )
            .unwrap()
        };
        assert_eq!(limit.additional, amount);
    }
}

#[test]
fn attribute_width_scan_charges_the_end_probe_after_valid_descriptors() {
    let operation = "iges test attribute width scan";
    let record = integer_parameter_record(1, &[322, 0, 1, 2, 10, 1, 2, 20, 1, 3]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            crate::parameter::attribute_table_definition_width(&record, 0, &ctx, operation)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected width scan end refusal");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 1);
    for cap in [2, 3] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result =
            crate::parameter::attribute_table_definition_width(&record, 2, &ctx, operation);
        if cap == 2 {
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("expected width scan end refusal");
            };
            assert_eq!(limit.operation, operation);
            assert_eq!(limit.used, 2);
            assert_eq!(limit.additional, 1);
        } else {
            assert_eq!(result.unwrap(), Ok(5));
        }
    }
}
