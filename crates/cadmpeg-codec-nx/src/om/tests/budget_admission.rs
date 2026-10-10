// SPDX-License-Identifier: Apache-2.0
//! Resource boundaries for bounded probes and early-exit searches.

#[test]
fn store_version_search_stops_before_the_unused_suffix() {
    let mut bytes = b"\x04\x01\x05NX \0".to_vec();
    bytes.extend_from_slice(&[0; 4096]);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            // One marker visit, three UTF-8 bytes, three characters, and the syntax end probe.
            policy.limits.max_work_units = 8;
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let version = crate::om::store_version(ctx, &bytes, 100).unwrap().unwrap();
            assert_eq!(version.offset, 100);
            assert_eq!(version.value.as_str(), "NX ");
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX store version marker search",
        |ctx| crate::om::store_version(ctx, &bytes, 100),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "NX store version marker search" && limit.additional == 1)
    );
}

#[test]
fn unique_body_reference_search_stops_on_the_second_reference() {
    let mut bytes = b"\x01\x02\x10\x42\xff\x01\x02\x10\x43\xff".to_vec();
    bytes.extend_from_slice(&[0; 4096]);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 6,
        |ctx| {
            let input =
                crate::om::operation_record::OperationBodyInput::new(&bytes, 100, 0, "BLOCK")
                    .unwrap();
            assert_eq!(
                crate::om::operation_body_reference(ctx, input).unwrap(),
                None
            );
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn scalar_run_initial_widths_refuse_at_the_variable_traversal() {
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX scalar run token widths",
        |ctx| {
            let atom = crate::om::fixed::Q155Atom {
                marker: crate::om::fixed::Q155Marker::M30,
                scalar: crate::om::fixed::Q155::from_raw([0; 7]).unwrap(),
            };
            let values =
                crate::om::nonempty::NonEmpty::from_admitted_vec(vec![(atom, ()), (atom, ())])
                    .unwrap();
            crate::om::scalar_run::FramedScalarRun::from_wire(
                ctx,
                crate::om::fixed::Q155LaneFrame,
                0,
                values,
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "NX scalar run token widths" && limit.additional == 1)
    );
}

#[test]
fn native_state_slot_widths_refuse_at_their_traversal() {
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX native state slot width traversal",
        |ctx| {
            let slots = crate::om::state_slots::StateSlots::new(vec![None, None]).unwrap();
            crate::om::state_slot_lane::StateSlotLane::from_wire(ctx, 100, slots)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "NX native state slot width traversal" && limit.additional == 1)
    );
}

#[test]
fn rejected_numeric_expression_does_not_retain_its_native_unit() {
    let text = b"(Number [custom]) p1: = 1; invalid";
    let mut bytes = vec![0, 4, u8::try_from(text.len() + 2).unwrap()];
    bytes.extend_from_slice(text);
    bytes.push(0);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            assert!(crate::om::numeric_expression_at(ctx, &bytes, 100, None)
                .unwrap()
                .is_none());
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn empty_counted_feature_references_need_no_traversal_work() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 0;
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let (values, end) = crate::om::counted_feature_object_indices(ctx, &[1, 1], 100, 0)
                .unwrap()
                .unwrap();
            assert!(values.is_empty());
            assert_eq!(end, 2);
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn control_word_traversal_pays_for_exactly_the_declared_words() {
    let bytes = [0, 7, 0, 0, 0, 8, 0, 0];
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 2,
        |ctx| {
            let values = crate::om::offset_store_control_values(ctx, &bytes)
                .map(|values| values.map(|(values, _storage)| values))
                .unwrap()
                .unwrap();
            assert_eq!(
                values
                    .into_iter()
                    .map(crate::om::control_word::ControlWord24::value)
                    .collect::<Vec<_>>(),
                [7, 8]
            );
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

fn class_ordinal_control() -> Vec<u8> {
    [2_u32, 0, 4, 8, 3].into_iter().flat_map(|value| {
        let bytes = value.to_le_bytes();
        [0, bytes[0], bytes[1], bytes[2]]
    }).collect()
}

fn class_ordinal_discovery_peak() -> u64 {
    // Five suffix minima and one B-tree node for the four visited identities.
    // The node has eleven u32 keys, sixteen pointer metadata/child slots,
    // and two pointer-alignment bounds. The final two ordinals are later.
    cadmpeg_core::decode::u64_from_index(
        5 * std::mem::size_of::<u32>() + 11 * std::mem::size_of::<u32>()
            + 16 * std::mem::size_of::<usize>() + 2 * std::mem::align_of::<usize>(),
    )
}

#[test]
fn class_ordinal_projection_releases_discovery_workspace() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let bytes = class_ordinal_control();
    let peak = class_ordinal_discovery_peak();
    for cap in [peak - 1, peak] {
        crate::test_support::with_decode_context_over(&[], |policy| {
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 2 * u64::from(u32::BITS / 8);
        }, |ctx| {
            // The parent captures only the final projected vector. Discovery
            // must end before that allocation can increase the parent's peak.
            let mut output_storage = ctx.reserve_scoped(0, "test class ordinal output").unwrap();
            let result = output_storage.with_storage(||
                crate::om::offset_store_control_class_ordinals(ctx, &bytes));
            if cap == peak {
                let values = result.unwrap().unwrap();
                assert_eq!(values, [2, 0]);
                let _values = output_storage.commit_value(values).expect("only two final u32 slots remain");
                assert!(ctx.resource_refusal().is_none());
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("one byte below the discovery peak must refuse");
                };
                assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(limit.operation, "NX offset-store class identity insertion");
                assert_eq!(limit.used, 5 * u64::from(u32::BITS / 8));
                assert_eq!(limit.used + limit.additional, peak);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            }
        });
    }
}

#[test]
fn class_ordinal_projection_retains_only_accepted_output() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let bytes = class_ordinal_control();
    let retained = cadmpeg_core::decode::u64_from_index(2 * std::mem::size_of::<u32>());
    for cap in [retained - 1, retained] {
        crate::test_support::with_decode_context_over(&[], |policy| {
            policy.limits.max_materialized_bytes = class_ordinal_discovery_peak();
            policy.limits.max_retained_bytes = cap;
        }, |ctx| {
            let mut output_storage = ctx.reserve_scoped(0, "test class ordinal output").unwrap();
            let values = output_storage.with_storage(||
                crate::om::offset_store_control_class_ordinals(ctx, &bytes))
                .expect("discovery and projection fit separately").unwrap();
            assert_eq!(values, [2, 0]);
            let result = output_storage.commit_value(values);
            if cap == retained {
                result.expect("exact retained output budget");
                assert!(ctx.resource_refusal().is_none());
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("one byte below final retention must refuse");
                };
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "test class ordinal output");
                assert_eq!(limit.used, 0);
                assert_eq!(limit.additional, retained);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            }
        });
    }
}
