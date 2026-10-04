// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::CodecError;

fn with_work_limit(limit: u64, run: impl FnOnce(&DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit;
    let (context, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    run(&context);
}

#[test]
fn owned_vec_map_option_and_array_admit_their_complete_bounds_once() {
    with_work_limit(7, |context| {
        assert_eq!(
            context
                .admit_iter(vec![1, 2], "owned Vec")
                .expect("admission")
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(
            context
                .admit_iter(BTreeMap::from([(2, "b"), (1, "a")]), "owned BTreeMap")
                .expect("admission")
                .collect::<Vec<_>>(),
            [(1, "a"), (2, "b")]
        );
        assert_eq!(
            context
                .admit_iter(Some(7), "Some")
                .expect("admission")
                .collect::<Vec<_>>(),
            [7]
        );
        assert_eq!(
            context
                .admit_iter(None::<u8>, "None")
                .expect("admission")
                .collect::<Vec<_>>(),
            Vec::<u8>::new()
        );
        assert_eq!(
            context
                .admit_iter([3, 4], "array")
                .expect("admission")
                .collect::<Vec<_>>(),
            [3, 4]
        );

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("seven precharged visits use the complete budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 7);
    });
}

#[test]
fn serde_json_map_supports_owned_and_borrowed_sources() {
    with_work_limit(4, |context| {
        let mut map = serde_json::Map::new();
        map.insert("a".to_owned(), serde_json::Value::from(1));
        map.insert("b".to_owned(), serde_json::Value::from(2));

        let owned = context
            .admit_iter(map.clone(), "owned JSON map")
            .expect("owned map admission")
            .map(|(key, value)| (key, value.as_i64()))
            .collect::<Vec<_>>();
        assert_eq!(owned, [("a".to_owned(), Some(1)), ("b".to_owned(), Some(2))]);

        let borrowed = context
            .admit_iter(&map, "borrowed JSON map")
            .expect("borrowed map admission")
            .map(|(key, value)| (key.as_str(), value.as_i64()))
            .collect::<Vec<_>>();
        assert_eq!(borrowed, [("a", Some(1)), ("b", Some(2))]);

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("both map traversals each admit two entries")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 4);
    });
}

#[test]
fn mutable_vec_and_slice_admission_precedes_mutation() {
    with_work_limit(4, |context| {
        let mut values = vec![1, 2];
        for value in context
            .admit_iter(&mut values, "mutable Vec")
            .expect("admission")
        {
            *value += 10;
        }
        assert_eq!(values, [11, 12]);

        let mut slice = [3, 4];
        for value in context
            .admit_iter(&mut slice[..], "mutable slice")
            .expect("admission")
        {
            *value *= 2;
        }
        assert_eq!(slice, [6, 8]);

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("four precharged visits use the complete budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 4);
    });
}

#[test]
fn mutable_array_admission_and_refusal_precede_direct_array_traversal() {
    with_work_limit(2, |context| {
        let mut values = [1, 2];
        for value in context
            .admit_iter(&mut values, "mutable array")
            .expect("direct array admission")
        {
            *value += 10;
        }
        assert_eq!(values, [11, 12]);

        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("both array visits use the complete budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 2);
    });

    with_work_limit(1, |context| {
        let mut values = [3, 4];
        let refusal = context
            .admit_iter(&mut values, "mutable array")
            .expect_err("the complete array bound exceeds the work limit");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 2);
        assert_eq!(values, [3, 4]);
    });
}

#[test]
fn mutable_source_refusal_precedes_the_first_mutable_item() {
    with_work_limit(1, |context| {
        let mut values = vec![1, 2];
        let refusal = context
            .admit_iter(&mut values, "mutable Vec")
            .expect_err("the complete bound exceeds the work limit");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 2);
        assert_eq!(values, [1, 2]);
    });
}

#[test]
fn mutable_map_values_are_admitted_before_mutation() {
    with_work_limit(2, |context| {
        let mut values = BTreeMap::from([(1, 10), (2, 20)]);
        for (_, value) in context
            .admit_iter(&mut values, "mutable BTreeMap")
            .expect("admission")
        {
            *value += 1;
        }
        assert_eq!(values, BTreeMap::from([(1, 11), (2, 21)]));
        assert!(context.charge_work(1, "probe").is_err());
    });
}

#[test]
fn dialect_layers_admit_the_primary_and_every_extra_layer_upfront() {
    use crate::dialect::{DialectLayers, DialectMatch};

    let layers = DialectLayers::of(DialectMatch::residual(crate::dialect_id!(
        "rhino:archive-80"
    )))
    .with(DialectMatch::residual(crate::dialect_id!("acis:other")))
    .expect("distinct dialect layers");

    with_work_limit(2, |context| {
        let names = context
            .admit_iter(&layers, "dialect layers")
            .expect("both layers fit")
            .map(|layer| layer.dialect().as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["rhino:archive-80", "acis:other"]);
        let CodecError::ResourceLimit(limit) = context
            .charge_work(1, "probe")
            .expect_err("the two layers use the complete budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 2);
    });

    with_work_limit(1, |context| {
        let refusal = context
            .admit_iter(&layers, "dialect layers")
            .expect_err("two layers exceed one unit before the first visit");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 2);
    });
}
