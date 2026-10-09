// SPDX-License-Identifier: Apache-2.0

use super::super::{SketchPoint, SketchPointCompanion, SketchPointDraft};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use serde_json::json;

fn native_draft() -> SketchPointDraft {
    let point: SketchPoint = serde_json::from_value(super::native_point_wire(&json!({
        "kind": "version11", "padded_paired_reference": false,
        "companion_prefix_present_zero": false
    })))
    .unwrap();
    SketchPointDraft {
        id: point.id,
        record_index: point.record_index,
        owner_reference: point.owner_reference,
        class_tag: point.class_tag,
        byte_offset: point.byte_offset,
        coordinate_offset: point.coordinate_offset,
        record_form: point.record_form.into_raw(),
        companion: point.companion,
        paired_reference: point.paired_reference,
        coordinates: point.coordinates.get(),
    }
}

#[test]
fn point_companion_validation_keeps_native_fields_and_retained_children() {
    let draft = native_draft();
    let incident_backing = u64::try_from(
        draft.companion.incident_curves.capacity() * std::mem::size_of::<u32>(),
    )
    .unwrap();
    let expected = serde_json::to_vec(&SketchPoint::try_from(draft.clone()).unwrap()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = incident_backing;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    ctx.charge_retained(incident_backing, "owned point incidence backing").unwrap();
    let point = SketchPoint::try_from_charged(&ctx, draft).unwrap();
    assert_eq!(serde_json::to_vec(&point).unwrap(), expected);
    assert_eq!(point.companion().incident_curves, &[7, 2]);
    assert_eq!(point.persistent_id(), Some(4));
    assert_eq!(point.paired_reference, 2);
    assert_eq!(point.coordinates(), cadmpeg_ir::math::Point2::new(2.0, 3.0));
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "validation table released").unwrap());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_retained(1, "point child stays retained") else {
        panic!("surviving child backing must stay retained");
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((original.used, original.additional), (incident_backing, 1));
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert_eq!(point.companion().incident_curves, &[7, 2]);
}

#[test]
fn point_companion_validation_temporary_set_needs_no_retained_allowance() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let point = SketchPoint::try_from_charged(&ctx, native_draft()).unwrap();
    assert_eq!(point.companion().incident_curves, &[7, 2]);
    ctx.finish_session().unwrap();
}

#[test]
fn point_companion_validation_duplicate_refusal_releases_actual_table() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut draft = native_draft();
    draft.companion.incident_curves = vec![7, 7, 2];
    let Err(CodecError::Malformed(message)) = SketchPoint::try_from_charged(&ctx, draft) else {
        panic!("duplicate incidence must preserve semantic refusal");
    };
    assert_eq!(message, "sketch point companion.incident_curves must be distinct");
    assert!(ctx.resource_refusal().is_none());
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "duplicate table released").unwrap());
    let point = SketchPoint::try_from_charged(&ctx, native_draft()).unwrap();
    assert_eq!(point.companion().incident_curves, &[7, 2]);
}

#[test]
fn point_companion_validation_refuses_original_dimensions_before_mutation() {
    for (dimension, materialized, items) in [
        (ResourceDimension::MaterializedBytes, 0, 2),
        (ResourceDimension::CollectionItems, 1024, 0),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = materialized;
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = SketchPoint::try_from_charged(&ctx, native_draft()) else {
            panic!("actual validation table admission must refuse");
        };
        assert_eq!(original.dimension, dimension);
        assert_eq!(original.used, 0);
        assert_eq!(original.operation, "index F3D sketch point incident curves");
        assert_eq!(ctx.resource_refusal(), Some(original));
        let Err(CodecError::ResourceLimit(later)) = ctx.charge_work(0, "after validation refusal") else {
            panic!("the original refusal must remain sticky");
        };
        assert_eq!(later, original);
    }
}

#[test]
fn point_companion_validation_empty_set_observes_original_context_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let companion = SketchPointCompanion { incident_curves: Vec::new() };
    companion.validate_charged(&ctx).unwrap();
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original point validation fuse") else {
        panic!("zero work context must refuse");
    };
    let Err(CodecError::ResourceLimit(later)) = companion.validate_charged(&ctx) else {
        panic!("empty validation must observe its original session refusal");
    };
    assert_eq!(later, original);
}
