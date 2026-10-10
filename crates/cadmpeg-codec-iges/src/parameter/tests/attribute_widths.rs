// SPDX-License-Identifier: Apache-2.0

use super::integer_parameter_record;
use crate::parameter::{AttributeDefinitionWidths, attribute_table_instance_primary_end};
use crate::test_support::directory_target;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn attribute_instances_reuse_valid_and_invalid_definition_widths() {
    const ATTRIBUTES: usize = 4_000;
    const INSTANCES: usize = 4_000;
    for invalid in [false, true] {
        let mut tokens = vec![322, 0, 1, i64::try_from(ATTRIBUTES).unwrap()];
        for _ in 0..ATTRIBUTES { tokens.extend([10, 1, 0]); }
        if invalid { *tokens.last_mut().unwrap() = -1; }
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
        for _ in 0..INSTANCES {
            assert_eq!(attribute_table_instance_primary_end(
                &record, &entry, &directory, &records, &mut widths, &ctx,
            ).unwrap(), if invalid { 2 } else { 1 });
        }
        assert_eq!(widths.values.len(), 1);
        assert_eq!(widths.values[&1], if invalid { None } else { Some(0) });
        drop(widths);
        let CodecError::ResourceLimit(limit) = ctx.reserve_scoped(u64::MAX, "test released attribute widths").unwrap_err() else {
            panic!("expected storage refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.used, 0);
    }
}

#[test]
fn attribute_width_cache_admits_nodes_before_insertion() {
    let record = integer_parameter_record(1, &[322, 0, 1, 1, 10, 1, 1]);
    for dimension in [ResourceDimension::CollectionItems, ResourceDimension::MaterializedBytes] {
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
            widths.width(&record, &ctx)
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("expected cache node refusal"); };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        let amount = if dimension == ResourceDimension::CollectionItems {
            1
        } else {
            use std::mem::{align_of, size_of};
            u64::try_from(11 * (size_of::<u32>() + size_of::<Option<usize>>())
                + 16 * size_of::<usize>()
                + 2 * align_of::<u32>().max(align_of::<Option<usize>>()).max(align_of::<usize>())).unwrap()
        };
        assert_eq!(limit.additional, amount);
    }
}
