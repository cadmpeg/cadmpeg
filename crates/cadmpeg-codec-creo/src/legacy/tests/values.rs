// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn type_2_reals_decode_compact_bits_runs_and_child_rows() {
    let data = b"@scalar 1 2\n0 1 3FF\n\
            @scale 2 2\n0 2 40396R\n\
            @matrix 3 2\n0 3 [2][2]\n$3FF,2*0,\n$3FF\n\
            @single 4 2\n0 4 [1]\n1 4 400\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.real_values.rows.len(), 4);
    assert_eq!(persistence.real_values.unresolved_count, 0);
    assert_eq!(
        persistence.real_values.rows[0].payload,
        RealPayload::Scalar {
            value: Real(1.0f64.to_bits())
        }
    );
    assert_eq!(
        persistence.real_values.rows[1].payload,
        RealPayload::Scalar {
            value: Real(25.4f64.to_bits())
        }
    );
    assert_eq!(
        persistence.real_values.rows[2].payload,
        crate::decode::with_test_decode_ctx(|ctx| RealPayload::array(
            ctx,
            vec![2, 2],
            vec![
                RealRun {
                    count: 1,
                    value: Real(1.0f64.to_bits()),
                },
                RealRun {
                    count: 2,
                    value: Real(0.0f64.to_bits()),
                },
                RealRun {
                    count: 1,
                    value: Real(1.0f64.to_bits()),
                },
            ]
        ))
        .expect("numeric array work admission")
        .expect("complete numeric array")
    );
    assert_eq!(persistence.real_values.rows[2].payload.element_count(), 4);
    assert_eq!(
        persistence.real_values.rows[3].payload,
        crate::decode::with_test_decode_ctx(|ctx| RealPayload::array(
            ctx,
            vec![1],
            vec![RealRun {
                count: 1,
                value: Real(2.0f64.to_bits()),
            }]
        ))
        .expect("numeric array work admission")
        .expect("complete numeric array")
    );
}

#[test]
fn type_2_reals_withhold_incomplete_or_nonfinite_values() {
    let data = b"@short 1 2\n0 1 [3]\n$2*0\n\
            @lower 2 2\n0 2 3ff\n\
            @infinite 3 2\n0 3 7FF\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert!(persistence.real_values.rows.is_empty());
    assert_eq!(persistence.real_values.unresolved_count, 3);
}

#[test]
fn type_1_integers_decode_signed_scalars_runs_and_child_rows() {
    let data = b"@minimum 1 1\n0 1 -2147483648\n\
            @array 2 1\n0 2 [4]\n$1,2*-1,0\n\
            @single 3 1\n0 3 [1]\n1 3 42\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.integer_values.rows.len(), 3);
    assert_eq!(persistence.integer_values.unresolved_count, 0);
    assert_eq!(
        persistence.integer_values.rows[0].payload,
        IntegerPayload::Scalar { value: i32::MIN }
    );
    assert_eq!(
        persistence.integer_values.rows[1].payload,
        crate::decode::with_test_decode_ctx(|ctx| IntegerPayload::array(
            ctx,
            vec![4],
            vec![
                IntegerRun { count: 1, value: 1 },
                IntegerRun {
                    count: 2,
                    value: -1,
                },
                IntegerRun { count: 1, value: 0 },
            ]
        ))
        .expect("numeric array work admission")
        .expect("complete numeric array")
    );
    assert_eq!(
        persistence.integer_values.rows[2].payload,
        crate::decode::with_test_decode_ctx(|ctx| IntegerPayload::array(
            ctx,
            vec![1],
            vec![IntegerRun {
                count: 1,
                value: 42,
            }]
        ))
        .expect("numeric array work admission")
        .expect("complete numeric array")
    );
}

#[test]
fn type_1_integers_withhold_incomplete_arrays_and_overflow() {
    let data = b"@short 1 1\n0 1 [2]\n$0\n\
            @overflow 2 1\n0 2 2147483648\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert!(persistence.integer_values.rows.is_empty());
    assert_eq!(persistence.integer_values.unresolved_count, 2);
}

#[test]
fn remaining_numeric_types_decode_their_scalar_and_array_grammars() {
    let data = b"@root 1 0\n@five 2 5\n@five_array 3 5\n@six 4 6\n@six_array 5 6\n\
            @seven 6 7\n@seven_array 7 7\n@nine 8 9\n@eleven 9 11\n@eleven_single 10 11\n\
            0 1 ->\n1 2 2700\n1 3 [3]\n$0,2*144\n1 4 400\n1 5 [2][2]\n$3FF,3*0\n\
            1 6 7\n1 7 [4]\n$0,1,2,8\n1 8 [4]\n$0,67108864,2*1\n\
            1 9 [3]\n$3,4,5\n1 10 [1]\n2 10 14633\n";
    let root_offset = data
        .windows(b"0 1 ->".len())
        .position(|window| window == b"0 1 ->")
        .expect("root offset");
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.type_5_values.rows.len(), 2);
    assert_eq!(persistence.type_5_values.unresolved_count, 0);
    assert_eq!(
        persistence.type_5_values.rows[1].payload,
        crate::decode::with_test_decode_ctx(|ctx| UnsignedPayload::array(
            ctx,
            vec![3],
            vec![
                NumericRun { count: 1, value: 0 },
                NumericRun {
                    count: 2,
                    value: 144,
                },
            ]
        ))
        .expect("numeric array work admission")
        .expect("complete numeric array")
    );
    assert_eq!(persistence.type_6_values.rows.len(), 2);
    assert_eq!(persistence.type_6_values.unresolved_count, 0);
    assert_eq!(
        persistence.type_6_values.rows[0].payload,
        RealPayload::Scalar {
            value: Real(2.0f64.to_bits())
        }
    );
    assert_eq!(persistence.type_7_values.rows.len(), 2);
    assert_eq!(persistence.type_7_values.unresolved_count, 0);
    assert_eq!(persistence.type_9_values.rows.len(), 1);
    assert_eq!(persistence.type_9_values.unresolved_count, 0);
    assert_eq!(persistence.type_11_values.rows.len(), 2);
    assert_eq!(persistence.type_11_values.unresolved_count, 0);
    assert_eq!(
        persistence.type_11_values.rows[1].payload,
        crate::decode::with_test_decode_ctx(|ctx| UnsignedPayload::array(
            ctx,
            vec![1],
            vec![NumericRun {
                count: 1,
                value: 14633,
            }]
        ))
        .expect("numeric array work admission")
        .expect("complete numeric array")
    );
    assert_eq!(persistence.type_11_values.rows[1].parent, Some(root_offset));
}

#[test]
fn remaining_numeric_types_withhold_undefined_values() {
    let data = b"@negative 1 5\n0 1 -1\n@nonfinite 2 6\n0 2 7FF\n\
            @short 3 11\n0 3 [2]\n$1\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert!(persistence.type_5_values.rows.is_empty());
    assert_eq!(persistence.type_5_values.unresolved_count, 1);
    assert!(persistence.type_6_values.rows.is_empty());
    assert_eq!(persistence.type_6_values.unresolved_count, 1);
    assert!(persistence.type_11_values.rows.is_empty());
    assert_eq!(persistence.type_11_values.unresolved_count, 1);
}

#[test]
fn type_3_and_type_4_decode_exact_scalar_bytes() {
    let data = b"@root 1 0\n@three_null 2 3\n@three_text 3 3\n@three_bytes 4 3\n\
            @four_null_text 5 4\n@four_empty 6 4\n@continued 7 3\n0 1 ->\n1 2 NULL\n\
            1 3 texture-name\n1 4 \xff\n1 5 NULL\n1 6\n1 7 first\n$second\n";
    let root_offset = data
        .windows(b"0 1 ->".len())
        .position(|window| window == b"0 1 ->")
        .expect("root offset");
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.type_3_values.rows.len(), 3);
    assert_eq!(persistence.type_3_values.unresolved_count, 1);
    assert_eq!(persistence.type_3_values.rows[0].payload, StringValue::Null);
    assert_eq!(
        persistence.type_3_values.rows[1].payload,
        StringValue::Utf8 {
            text: "texture-name".to_string(),
        }
    );
    assert_eq!(
        persistence.type_3_values.rows[2].payload,
        StringValue::Bytes { bytes: vec![0xff] }
    );
    assert_eq!(persistence.type_3_values.rows[0].parent, Some(root_offset));

    assert_eq!(persistence.type_4_values.rows.len(), 2);
    assert_eq!(persistence.type_4_values.unresolved_count, 0);
    assert_eq!(
        persistence.type_4_values.rows[0].payload,
        StringValue::Utf8 {
            text: "NULL".to_string(),
        }
    );
    assert_eq!(
        persistence.type_4_values.rows[1].payload,
        StringValue::Utf8 {
            text: String::new(),
        }
    );
}

#[test]
fn type_10_strings_decode_null_bytes_and_direct_element_arrays() {
    let data = b"@root 1 0\n@label 2 10\n@empty 3 10\n@missing 4 10\n\
            @encoded 5 10\n@names 6 10\n0 1 ->\n1 2 alpha beta\n1 3 \n1 4 NULL\n\
            1 5 \xE9\n1 6 [2][81]\n2 6 first\n2 6 \n";
    let root_offset = data
        .windows(b"0 1 ->".len())
        .position(|window| window == b"0 1 ->")
        .expect("root offset");
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.string_values.len(), 5);
    assert_eq!(persistence.incomplete_string_array_count, 0);
    assert_eq!(persistence.unresolved_string_value_count, 0);
    assert_eq!(
        persistence.string_values[0].payload,
        StringPayload::Scalar {
            value: StringValue::Utf8 {
                text: "alpha beta".to_string()
            }
        }
    );
    assert_eq!(
        persistence.string_values[1].payload,
        StringPayload::Scalar {
            value: StringValue::Utf8 {
                text: String::new()
            }
        }
    );
    assert_eq!(
        persistence.string_values[2].payload,
        StringPayload::Scalar {
            value: StringValue::Null
        }
    );
    assert_eq!(
        persistence.string_values[3].payload,
        StringPayload::Scalar {
            value: StringValue::Bytes { bytes: vec![0xe9] }
        }
    );
    assert_eq!(
        persistence.string_values[4].payload,
        StringPayload::Array {
            dimensions: vec![2, 81],
            values: vec![
                Ok(StringValue::Utf8 {
                    text: "first".to_string()
                }),
                Ok(StringValue::Utf8 {
                    text: String::new()
                }),
            ],
            complete: true,
            accepted_value_indices: vec![0, 1],
        }
    );
    let array =
        serde_json::to_value(&persistence.string_values[4].payload).expect("string array wire");
    assert_eq!(array["complete"], true);
    assert_eq!(
        array["values"].as_array().expect("supported values").len(),
        2
    );
    let StringPayload::Scalar { value } = &persistence.string_values[3].payload else {
        panic!("byte string scalar");
    };
    assert_eq!(value.undecoded_encoding_count(), 1);
    assert_eq!(persistence.string_values[4].parent, Some(root_offset));
}

#[test]
fn type_10_strings_retain_incomplete_arrays_and_withhold_continuations() {
    let data = b"@names 1 10\n0 1 [2]\n1 1 only\n\
            @continued 2 10\n0 2 first\n$second\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.string_values.len(), 1);
    assert_eq!(persistence.incomplete_string_array_count, 1);
    assert_eq!(
        serde_json::to_value(&persistence.string_values[0].payload).expect("incomplete array wire")
            ["complete"],
        false
    );
    assert_eq!(persistence.unresolved_string_value_count, 1);
    assert_eq!(
        persistence.string_values[0].payload,
        StringPayload::Array {
            dimensions: vec![2],
            values: vec![Ok(StringValue::Utf8 {
                text: "only".to_string()
            })],
            complete: false,
            accepted_value_indices: vec![0],
        }
    );
}

#[test]
fn array_completeness_wire_retains_continuation_failures() {
    let data = b"@names 1 10\n0 1 [1]\n$header\n1 1 value\n\
        @other 2 10\n0 2 [1]\n1 2 value\n$child\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    assert_eq!(persistence.incomplete_string_array_count, 2);
    assert_eq!(persistence.unresolved_string_value_count, 2);
    assert_eq!(
        serde_json::to_value(&persistence.string_values[0].payload).unwrap(),
        serde_json::json!({"form": "array", "dimensions": [1],
            "values": [{"form": "utf8", "text": "value"}], "complete": false})
    );
    assert_eq!(
        serde_json::to_value(&persistence.string_values[1].payload).unwrap(),
        serde_json::json!({"form": "array", "dimensions": [1],
            "values": [], "complete": false})
    );
    let payload = ObjectPayload::Array {
        dimensions: vec![1, 2],
        elements: vec!["first".into(), "second".into()],
        complete: true,
    };
    assert_eq!(
        serde_json::to_value(payload).unwrap(),
        serde_json::json!({
            "form": "array", "dimensions": [1, 2], "elements": ["first", "second"],
            "complete": true
        })
    );
}

#[test]
fn type_10_array_wire_uses_admitted_supported_source_indices() {
    let data = b"@names 1 10\n0 1 [3]\n1 1 first\n1 1 skipped\n$continued\n1 1 last\n";
    let persistence = crate::test_support::assert_work_boundaries(
        &["creo legacy string child traversal"],
        |ctx| super::super::scan(ctx, data, std::iter::once(0..data.len())),
    );

    assert_eq!(persistence.incomplete_string_array_count, 1);
    assert_eq!(persistence.unresolved_string_value_count, 1);
    let StringPayload::Array {
        accepted_value_indices,
        complete,
        ..
    } = &persistence.string_values[0].payload
    else {
        panic!("three-row type-10 array is retained");
    };
    assert_eq!(accepted_value_indices, &[0, 2]);
    assert!(!*complete);
    assert_eq!(
        serde_json::to_value(&persistence.string_values[0].payload).unwrap(),
        serde_json::json!({"form": "array", "dimensions": [3],
            "values": [
                {"form": "utf8", "text": "first"},
                {"form": "utf8", "text": "last"}
            ], "complete": false})
    );
}

#[test]
fn type_0_objects_define_scoped_ownership_and_array_elements() {
    let data = b"@root 1 0\n@number 2 1\n@children 3 0\n@weight 4 2\n\
            0 1 ->\n1 2 7\n1 3 [2]\n2 3 ->\n3 4 3FF\n2 3 NULL\n";
    let root_offset = data
        .windows(b"0 1 ->".len())
        .position(|window| window == b"0 1 ->")
        .expect("root offset");
    let array_offset = data
        .windows(b"1 3 [2]".len())
        .position(|window| window == b"1 3 [2]")
        .expect("array offset");
    let first_child_offset = data
        .windows(b"2 3 ->".len())
        .position(|window| window == b"2 3 ->")
        .expect("first child offset");
    let second_child_offset = data
        .windows(b"2 3 NULL".len())
        .position(|window| window == b"2 3 NULL")
        .expect("second child offset");
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.objects.len(), 4);
    assert_eq!(persistence.incomplete_object_array_count, 0);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| persistence.objects[1].payload.is_complete(ctx))
            .expect("complete array admission")
    );
    assert!(
        serde_json::to_value(&persistence.objects[1].payload).unwrap()["complete"]
            .as_bool()
            .expect("serialized object completeness")
    );
    assert_eq!(persistence.unresolved_object_value_count, 0);
    assert_eq!(persistence.objects[1].parent, Some(root_offset));
    assert_eq!(
        persistence.objects[1].payload,
        ObjectPayload::Array {
            dimensions: vec![2],
            elements: vec![
                object_node_id(first_child_offset),
                object_node_id(second_child_offset),
            ],
            complete: true,
        }
    );
    assert_eq!(persistence.integer_values.rows.len(), 1);
    assert_eq!(persistence.integer_values.rows[0].parent, Some(root_offset));
    assert_eq!(persistence.real_values.rows.len(), 1);
    assert_eq!(
        persistence.real_values.rows[0].parent,
        Some(first_child_offset)
    );
    assert_eq!(persistence.objects[1].offset, array_offset);
}

#[test]
fn type_0_objects_retain_incomplete_and_opaque_forms() {
    let data = b"@array 1 0\n0 1 [2]\n@future 2 0\n0 2 token\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");

    assert_eq!(persistence.objects.len(), 2);
    assert_eq!(persistence.incomplete_object_array_count, 1);
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| persistence.objects[0].payload.is_complete(ctx))
            .expect("complete array admission")
    );
    assert!(
        !serde_json::to_value(&persistence.objects[0].payload).unwrap()["complete"]
            .as_bool()
            .expect("serialized object incompleteness")
    );
    assert_eq!(persistence.unresolved_object_value_count, 1);
    assert!(matches!(
        persistence.objects[0].payload,
        ObjectPayload::Array { .. }
    ));
    assert_eq!(
        persistence.objects[1].payload,
        ObjectPayload::Opaque {
            bytes: b"token".to_vec()
        }
    );
}

#[test]
fn rejected_array_dimensions_release_each_candidate() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for nested in [false, true] {
        let cap = crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            None,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                super::super::array_dimensions(&ctx, b"[1]x")
            },
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut storage = ctx
            .reserve_scoped(0, "rejected dimensions parent")
            .expect("storage");
        for _ in 0..256 {
            let candidate = if nested {
                storage.with_storage(|| super::super::array_dimensions(&ctx, b"[1]x"))
            } else {
                super::super::array_dimensions(&ctx, b"[1]x")
            };
            assert_eq!(
                candidate.expect("rejected dimension backing released"),
                None
            );
        }
        let resource = ctx
            .reserve_scoped_limit(u64::MAX, "rejected dimensions release")
            .expect_err("read live storage");
        assert_eq!(resource.used, 0);
    }
}

#[test]
fn rejected_numeric_continuations_release_each_candidate() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let bytes = b"$7,invalid";
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            super::super::continuation_numeric_runs(&ctx, bytes, super::super::signed_integer)
        },
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut storage = ctx
        .reserve_scoped(0, "rejected numeric parent")
        .expect("storage");
    for _ in 0..256 {
        assert_eq!(
            storage
                .with_storage(|| super::super::continuation_numeric_runs(
                    &ctx,
                    bytes,
                    super::super::signed_integer
                ))
                .expect("rejected runs release"),
            None
        );
    }
    let resource = ctx
        .reserve_scoped_limit(u64::MAX, "rejected continuation release")
        .expect_err("read live storage");
    assert_eq!(resource.used, 0);
}

#[test]
fn rejected_numeric_payloads_release_dimensions_and_runs() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for continuation in [false, true] {
        let row = if continuation {
            "0 1 [3]\n$7,8,\n"
        } else {
            "0 1 [1]\n1 1 2*7\n"
        };
        let bytes = format!("@numbers 1 1\n{}", row.repeat(256)).into_bytes();
        let scope = crate::decode::with_test_decode_ctx(|ctx| {
            super::super::scan_scope(ctx, &bytes, 0..bytes.len())
        })
        .expect("synthetic scope");
        let parents = std::collections::HashMap::new();
        let cap = crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            None,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                super::super::numeric_records(
                    &ctx,
                    &bytes,
                    std::slice::from_ref(&scope),
                    super::super::ValueKind::INTEGER,
                    super::super::signed_integer,
                    &parents,
                )
            },
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = super::super::numeric_records(
            &ctx,
            &bytes,
            std::slice::from_ref(&scope),
            super::super::ValueKind::INTEGER,
            super::super::signed_integer,
            &parents,
        )
        .expect("rejected payloads refund scratch");
        assert!(result.rows.is_empty());
        assert_eq!(
            result.unresolved_count,
            if continuation { 256 } else { 512 }
        );
        let resource = ctx
            .reserve_scoped_limit(u64::MAX, "numeric payload release")
            .expect_err("read live backing");
        assert_eq!(resource.used, 0);
    }
}
