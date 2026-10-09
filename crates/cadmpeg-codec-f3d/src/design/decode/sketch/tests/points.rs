// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{
    decode_sketch_point_companion, decode_sketch_point_record, SKETCH_CONTAINER_TYPE_GUID,
    SKETCH_POINT_COMPANION_TYPE, SKETCH_POINT_TYPE_GUID,
};
use crate::records::sketch_geometry::{
    SketchPointClosure, SketchPointCompanion, SketchPointCompanionReferenceEncoding,
    SketchPointRecordForm,
};
use crate::test_support::lp_ascii;
use std::collections::HashMap;

const POINT: u32 = 41;
const COMPANION: u32 = 43;
const OWNER: u32 = 17;
const LINE: &str = "DCA267ED-D615-4934-B64F-AD805E8003E2";
const ARC: &str = "F0130424-8B7E-4092-93C9-1CA807482534";

fn push_header(out: &mut Vec<u8>, class_tag: &str, record_index: u32) {
    out.extend_from_slice(&u32::try_from(class_tag.len()).unwrap().to_le_bytes());
    out.extend_from_slice(class_tag.as_bytes());
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn push_reference(out: &mut Vec<u8>, target: u32, type_guid: Option<&str>) {
    out.push(1);
    out.extend_from_slice(&u64::from(target).to_le_bytes());
    if let Some(type_guid) = type_guid {
        lp_ascii(out, type_guid);
    }
    out.extend_from_slice(&[0; 2]);
}

fn tagged_point_payload(
    version: u32,
    inline_typed: bool,
    selector: u64,
    state: u8,
    padded_paired_reference: bool,
) -> Vec<u8> {
    let mut payload = Vec::new();
    push_header(&mut payload, "257", POINT);
    payload.extend_from_slice(&[0; 9]);
    payload.push(1);
    payload.extend_from_slice(&1u32.to_le_bytes());
    lp_ascii(&mut payload, "pt_tag");
    lp_ascii(&mut payload, "IntrinsicMetaTypeuint64");
    payload.extend_from_slice(&500u64.to_le_bytes());
    let paired_type = inline_typed.then_some(SKETCH_POINT_COMPANION_TYPE.0);
    push_reference(&mut payload, COMPANION, paired_type);
    payload.extend(std::iter::repeat_n(0, if version == 11 { 8 } else { 7 }));
    for value in [1.25f64, -2.5, 0.25] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&selector.to_le_bytes());
    payload.push(state);
    payload.extend(std::iter::repeat_n(0, if version == 8 { 8 } else { 12 }));
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[0, 1, 0, 0, 0]);
    push_reference(&mut payload, COMPANION, paired_type);
    if padded_paired_reference {
        payload.extend_from_slice(&[0; 4]);
    }
    push_reference(
        &mut payload,
        OWNER,
        inline_typed.then_some(SKETCH_CONTAINER_TYPE_GUID),
    );
    payload
}

#[test]
fn point_record_parser_closes_every_versioned_three_coordinate_form() {
    let cases = [
        (8, false, 0, 0, false),
        (10, false, 0, 1, false),
        (10, true, 0, 1, false),
        (10, true, 2, 1, false),
        (11, true, 0, 0, false),
    ];
    for (version, inline_typed, selector, state, padded) in cases {
        let decoded = decode_sketch_point_record(
            &cadmpeg_test_support::service_decode_context(),
            &tagged_point_payload(version, inline_typed, selector, state, padded),
            version,
        )
        .unwrap()
        .expect("synthetic point form");
        assert_eq!(decoded.record_form.class_version(), version);
        assert_eq!(
            decoded.record_form.uses_inline_typed_references(),
            inline_typed
        );
        assert_eq!(decoded.record_form.persistent_id(), Some(500));
        assert_eq!(decoded.paired_reference, COMPANION);
        assert_eq!(decoded.coordinates, [1.25, -2.5]);
        assert_eq!(decoded.record_form.depth(), 0.25 * 10.0);
        assert_eq!(
            decoded.record_form.closure(),
            SketchPointClosure::from_pair(selector, state)
        );
    }
    assert!(decode_sketch_point_record(
        &cadmpeg_test_support::service_decode_context(),
        &tagged_point_payload(10, false, 2, 1, false),
        10,
    )
    .unwrap()
    .is_none());
    for (selector, state) in [(0, 0), (0, 1), (1, 0), (2, 1), (4, 0)] {
        for padded_paired_reference in [false, true] {
            let decoded = decode_sketch_point_record(
                &cadmpeg_test_support::service_decode_context(),
                &tagged_point_payload(11, false, selector, state, padded_paired_reference),
                11,
            )
            .unwrap()
            .expect("synthetic version-11 point");
            assert_eq!(
                decoded.record_form,
                SketchPointRecordForm::Version11 {
                    depth: 0.25 * 10.0,
                    entity_genesis: None,
                    padded_paired_reference,
                    companion_prefix_present_zero: false,
                    persistent_id: std::num::NonZeroU64::new(500).unwrap(),
                    flags: [false; 8],
                    closure: SketchPointClosure::from_pair(selector, state).unwrap(),
                }
            );
            assert_eq!(
                decoded.record_form.closure(),
                SketchPointClosure::from_pair(selector, state)
            );
        }
    }
    for (selector, state) in [(1, 1), (2, 0), (4, 1), (3, 0), (0, 2)] {
        assert!(decode_sketch_point_record(
            &cadmpeg_test_support::service_decode_context(),
            &tagged_point_payload(11, false, selector, state, false),
            11,
        )
        .unwrap()
        .is_none());
    }
}

#[test]
fn version_zero_point_retains_its_one_flag_and_source_local_identity() {
    let mut payload = Vec::new();
    push_header(&mut payload, "257", POINT);
    payload.extend_from_slice(&[0; 10]);
    push_reference(&mut payload, COMPANION, None);
    payload.push(1);
    payload.extend_from_slice(&1.25f64.to_le_bytes());
    payload.extend_from_slice(&(-2.5f64).to_le_bytes());
    payload.extend_from_slice(&[0; 20]);
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[0; 12]);
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    push_reference(&mut payload, COMPANION, None);
    push_reference(&mut payload, OWNER, None);
    let decoded =
        decode_sketch_point_record(&cadmpeg_test_support::service_decode_context(), &payload, 0)
            .unwrap()
            .expect("version-0 point");
    assert_eq!(
        decoded.record_form,
        SketchPointRecordForm::Version0 { flag: true }
    );
    assert_eq!(decoded.record_form.persistent_id(), None);
    assert_eq!(decoded.record_form.flags(), [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(decoded.coordinates, [1.25, -2.5]);
    assert_eq!(decoded.record_form.depth(), 0.0);
    assert_eq!(decoded.record_form.closure(), None);
}

#[test]
fn point_companion_retains_both_prefixes_and_reference_encodings() {
    let types = HashMap::from([
        (POINT, (SKETCH_POINT_TYPE_GUID, 11, "Geometry")),
        (71, (LINE, 2, "Geometry")),
        (72, (ARC, 0, "Geometry")),
    ]);
    for prefix_present_zero in [false, true] {
        for record_form in [
            SketchPointRecordForm::version11(500, SketchPointClosure::Selector0State0, None, 0.0),
            SketchPointRecordForm::Version11InlineTyped {
                depth: 0.0,
                entity_genesis: None,
                trailing_reference: OWNER,
                companion_prefix_present_zero: false,
                persistent_id: std::num::NonZeroU64::new(500).unwrap(),
                flags: [false; 8],
                closure: SketchPointClosure::Selector0State0,
            },
        ] {
            let reference_encoding = SketchPointCompanionReferenceEncoding::for_form(&record_form);
            if prefix_present_zero
                && reference_encoding == SketchPointCompanionReferenceEncoding::InlineTyped
            {
                continue;
            }
            let mut payload = Vec::new();
            push_header(&mut payload, "258", COMPANION);
            if prefix_present_zero {
                payload.extend_from_slice(&[0; 9]);
                payload.extend_from_slice(&[1, 0, 0, 0, 0]);
            } else {
                payload.extend_from_slice(&[0; 10]);
            }
            payload.extend_from_slice(&2u32.to_le_bytes());
            let inline_typed =
                reference_encoding == SketchPointCompanionReferenceEncoding::InlineTyped;
            push_reference(&mut payload, 71, inline_typed.then_some(LINE));
            push_reference(&mut payload, 72, inline_typed.then_some(ARC));
            payload.push(0);
            push_reference(
                &mut payload,
                POINT,
                inline_typed.then_some(SKETCH_POINT_TYPE_GUID),
            );
            assert_eq!(
                crate::design::test_support::with_test_decode_context(|ctx| {
                    decode_sketch_point_companion(
                        ctx,
                        &payload,
                        POINT,
                        record_form.clone(),
                        |target| Ok(types.get(&target).map(|registered| registered.0)),
                    )
                })
                .expect("point companion admission"),
                Some((
                    record_form
                        .clone()
                        .with_companion_prefix_present_zero(prefix_present_zero)
                        .expect("version-11 form carries the companion prefix"),
                    SketchPointCompanion {
                        incident_curves: vec![71, 72],
                    }
                ))
            );
        }
    }
}

#[test]
fn sketch_point_incident_curves_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let types = HashMap::from([
        (POINT, (SKETCH_POINT_TYPE_GUID, 11, "Geometry")),
        (71, (LINE, 2, "Geometry")),
        (72, (ARC, 0, "Geometry")),
    ]);
    let mut payload = Vec::new();
    push_header(&mut payload, "258", COMPANION);
    payload.extend_from_slice(&[0; 10]);
    payload.extend_from_slice(&2u32.to_le_bytes());
    push_reference(&mut payload, 71, None);
    push_reference(&mut payload, 72, None);
    payload.push(0);
    push_reference(&mut payload, POINT, None);
    let record_form =
        SketchPointRecordForm::version11(500, SketchPointClosure::Selector0State0, None, 0.0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = decode_sketch_point_companion(&ctx, &payload, POINT, record_form, |target| {
        Ok(types.get(&target).map(|registered| registered.0))
    })
    .expect_err("collection limit must refuse incident curves");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d sketch point incident curves")
    );
}

fn candidate_companion_payload(inverse: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    push_header(&mut payload, "258", COMPANION);
    payload.extend_from_slice(&[0; 10]);
    payload.extend_from_slice(&2u32.to_le_bytes());
    push_reference(&mut payload, 71, None);
    push_reference(&mut payload, 72, None);
    payload.push(0);
    push_reference(&mut payload, inverse, None);
    payload
}

#[test]
fn rejected_point_companion_releases_only_its_candidate_backing() {
    use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};
    use std::cell::Cell;

    let valid = candidate_companion_payload(POINT);
    let presence = valid.len() - 1 - std::mem::size_of::<u64>() - 2 - 1;
    let mut bad_presence = valid.clone();
    bad_presence[presence] = 1;
    let mut truncated = valid.clone();
    truncated.pop();
    let mut trailing = valid.clone();
    trailing.push(0);
    let candidates = [
        (bad_presence, None, 2),
        (truncated, None, 2),
        (trailing, None, 2),
        (candidate_companion_payload(POINT + 1), None, 2),
        (valid.clone(), Some(71), 1),
        (valid, Some(72), 2),
    ];
    // reserve_capacity uses the core's four-slot minimum for u32 backing.
    let backing = u64_from_index(4 * std::mem::size_of::<u32>());
    for (payload, missing, expected_queries) in candidates {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = backing;
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let queries = Cell::new(0);
        let form = SketchPointRecordForm::version11(
            500, SketchPointClosure::Selector0State0, None, 0.0,
        );
        assert!(decode_sketch_point_companion(&ctx, &payload, POINT, form, |target| {
            queries.set(queries.get() + 1);
            Ok(if missing == Some(target) { None } else { Some(LINE) })
        })
        .unwrap()
        .is_none());
        assert_eq!(queries.get(), expected_queries);
        let mut storage = ctx.reserve_scoped(0, "after rejected point companion").unwrap();
        let mut later = Vec::<u32>::new();
        storage.with_storage(|| {
            ctx.reserve_capacity(&mut later, 2, "after rejected point companion")
        })
        .unwrap();
        later.extend_from_slice(&[73, 74]);
        assert_eq!(later, [73, 74]);
        assert_eq!(later.capacity(), 4);
        let held = ctx.copy_retained_text("a", "after rejected point companion").unwrap();
        assert_eq!(held, "a");
        assert_eq!(held.capacity(), 1);
    }
}

#[test]
fn accepted_point_companion_retains_exact_surviving_backing() {
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    use cadmpeg_core::CodecError;
    use std::cell::Cell;

    let payload = candidate_companion_payload(POINT);
    let backing = u64_from_index(4 * std::mem::size_of::<u32>());
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::RetainedBytes] {
        for ceiling in [backing, backing - 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = ceiling,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = ceiling,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let queries = Cell::new(0);
            let form = SketchPointRecordForm::version11(
                500, SketchPointClosure::Selector0State0, None, 0.0,
            );
            let result = decode_sketch_point_companion(&ctx, &payload, POINT, form.clone(), |_| {
                queries.set(queries.get() + 1);
                Ok(Some(LINE))
            });
            if ceiling == backing {
                let (decoded_form, held) = result.unwrap().unwrap();
                assert_eq!(queries.get(), 2);
                assert_eq!(decoded_form, form);
                assert_eq!(held.incident_curves, [71, 72]);
                assert_eq!(held.incident_curves.capacity(), 4);
                if dimension == ResourceDimension::MaterializedBytes {
                    let mut later_storage = ctx.reserve_scoped(0, "after companion transfer").unwrap();
                    let mut later = Vec::<u32>::new();
                    later_storage.with_storage(|| {
                        ctx.reserve_capacity(&mut later, 2, "after companion transfer")
                    })
                    .unwrap();
                    later.extend_from_slice(&[73, 74]);
                    assert_eq!(later, [73, 74]);
                    assert_eq!(held.incident_curves, [71, 72]);
                } else {
                    let CodecError::ResourceLimit(limit) = ctx
                        .copy_retained_text("a", "after companion transfer")
                        .unwrap_err()
                    else { panic!("surviving companion backing must remain retained"); };
                    assert_eq!(limit.dimension, dimension);
                    assert_eq!(limit.operation, "after companion transfer");
                    assert_eq!(limit.used, backing);
                    assert_eq!(limit.additional, 1);
                    assert_eq!(limit.limit, ceiling);
                    assert_eq!(held.incident_curves, [71, 72]);
                    let CodecError::ResourceLimit(later) = ctx
                        .charge_work(0, "after companion retained refusal").unwrap_err()
                    else { panic!("the original refusal must remain fused"); };
                    assert_eq!(later, limit);
                }
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("the actual companion backing must refuse");
                };
                assert_eq!(limit.dimension, dimension);
                assert_eq!(limit.operation, "f3d sketch point incident curves");
                assert_eq!(limit.used, 0);
                assert_eq!(limit.additional, backing);
                assert_eq!(limit.limit, ceiling);
                assert_eq!(queries.get(), if dimension == ResourceDimension::MaterializedBytes { 0 } else { 2 });
                let CodecError::ResourceLimit(later) = ctx
                    .charge_work(0, "after companion backing refusal").unwrap_err()
                else { panic!("the original refusal must remain fused"); };
                assert_eq!(later, limit);
            }
        }
    }
}

#[test]
fn point_companion_callback_diagnostic_escapes_candidate_storage() {
    use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::cell::Cell;

    let payload = candidate_companion_payload(POINT);
    let message = "registered curve is unavailable";
    let backing = u64_from_index(4 * std::mem::size_of::<u32>());
    for rejected_target in [71, 72] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = backing;
        policy.limits.max_retained_bytes = u64_from_index(message.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let queries = Cell::new(0);
        let form = SketchPointRecordForm::version11(
            500, SketchPointClosure::Selector0State0, None, 0.0,
        );
        let result = decode_sketch_point_companion(&ctx, &payload, POINT, form, |target| {
            queries.set(queries.get() + 1);
            if target == rejected_target {
                Err(CodecError::Malformed(ctx.copy_retained_text(message, "companion type diagnostic")?))
            } else {
                Ok(Some(LINE))
            }
        });
        let Err(CodecError::Malformed(held)) = result else {
            panic!("the callback's owned diagnostic must escape");
        };
        assert_eq!(queries.get(), if rejected_target == 71 { 1 } else { 2 });
        assert_eq!(held, message);
        assert_eq!(held.capacity(), message.len());
        let mut storage = ctx.reserve_scoped(0, "after companion callback").unwrap();
        let mut later = Vec::<u32>::new();
        storage.with_storage(|| ctx.reserve_capacity(&mut later, 2, "after companion callback"))
            .unwrap();
        later.extend_from_slice(&[73, 74]);
        assert_eq!(later, [73, 74]);
        let CodecError::ResourceLimit(limit) = ctx
            .copy_retained_text("a", "after companion callback diagnostic").unwrap_err()
        else { panic!("the callback diagnostic must remain retained"); };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "after companion callback diagnostic");
        assert_eq!(limit.used, u64_from_index(message.len()));
        assert_eq!(limit.additional, 1);
        assert_eq!(limit.limit, u64_from_index(message.len()));
        assert_eq!(held, message);
        let CodecError::ResourceLimit(later) = ctx
            .charge_work(0, "after companion callback refusal").unwrap_err()
        else { panic!("the original refusal must remain fused"); };
        assert_eq!(later, limit);
    }
}

#[test]
fn point_companion_preserves_member_callback_and_collection_refusal_order() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::cell::RefCell;

    let payload = candidate_companion_payload(POINT);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let visited = RefCell::new(Vec::new());
    let form = SketchPointRecordForm::version11(
        500, SketchPointClosure::Selector0State0, None, 0.0,
    );
    let result = decode_sketch_point_companion(&ctx, &payload, POINT, form, |target| {
        visited.borrow_mut().push(target);
        Ok(Some(LINE))
    });
    let Err(CodecError::ResourceLimit(limit)) = result else {
        panic!("the second original collection admission must refuse");
    };
    assert_eq!(*visited.borrow(), [71, 72]);
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "f3d sketch point incident curves");
    assert_eq!(limit.used, 1);
    assert_eq!(limit.additional, 1);
    assert_eq!(limit.limit, 1);
    let CodecError::ResourceLimit(later) = ctx
        .charge_work(0, "after companion member refusal").unwrap_err()
    else { panic!("the original refusal must remain fused"); };
    assert_eq!(later, limit);
}

#[test]
fn point_companion_callback_refusal_keeps_original_resource_failure() {
    use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::cell::RefCell;

    let payload = candidate_companion_payload(POINT);
    let message = "registered curve is unavailable";
    for rejected_target in [71, 72] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64_from_index(message.len() - 1);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let visited = RefCell::new(Vec::new());
        let form = SketchPointRecordForm::version11(
            500, SketchPointClosure::Selector0State0, None, 0.0,
        );
        let result = decode_sketch_point_companion(&ctx, &payload, POINT, form, |target| {
            visited.borrow_mut().push(target);
            if target == rejected_target {
                Err(CodecError::Malformed(ctx.copy_retained_text(message, "companion type diagnostic")?))
            } else {
                Ok(Some(LINE))
            }
        });
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("the original callback diagnostic admission must refuse");
        };
        assert_eq!(*visited.borrow(), if rejected_target == 71 { vec![71] } else { vec![71, 72] });
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "companion type diagnostic");
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, u64_from_index(message.len()));
        assert_eq!(limit.limit, u64_from_index(message.len() - 1));
        let CodecError::ResourceLimit(later) = ctx
            .charge_work(0, "after companion callback copy refusal").unwrap_err()
        else { panic!("the original refusal must remain fused"); };
        assert_eq!(later, limit);
    }
}

#[test]
fn point_companion_original_fuse_stops_before_registered_callback() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use std::cell::Cell;

    let payload = candidate_companion_payload(POINT);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx
        .copy_retained_text("a", "original companion refusal").unwrap_err()
    else { panic!("the original copy must refuse"); };
    let queried = Cell::new(false);
    let form = SketchPointRecordForm::version11(
        500, SketchPointClosure::Selector0State0, None, 0.0,
    );
    let result = decode_sketch_point_companion(&ctx, &payload, POINT, form, |_| {
        queried.set(true);
        Ok(Some(LINE))
    });
    let Err(CodecError::ResourceLimit(later)) = result else {
        panic!("the original refusal must escape the companion parser");
    };
    assert_eq!(later, original);
    assert!(!queried.get());
}
