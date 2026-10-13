// SPDX-License-Identifier: Apache-2.0
//! Inventor sketch inventory admission tests.

use super::*;

#[test]
fn sketch_inventory_refuses_before_first_segment_step() {
    let bytes = primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("sketch fixture context");
    let container = InventorContainer::open(&setup, view).expect("sketch fixture container");
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("sketch scan context");
    let Err(error) = inventory(&ctx, &container.rse) else {
        panic!("first segment step refuses");
    };
    assert!(matches!(&error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "visit Inventor sketch items"
            && limit.used == 0
            && limit.additional == 1));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
        if matches!(&error, CodecError::ResourceLimit(original) if original == &limit))
    );
}

fn inventory_with_record(
    type_id: [u8; 16],
    payload: &[u8],
    policy: DecodePolicy,
) -> Result<SketchInventory, CodecError> {
    let bytes = primary_envelope_fixture();
    let payload = payload.to_vec();
    let arena = DecodeArena::new();
    let (setup_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("envelope view");
    let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
    let segment = &mut container.rse.segments[0];
    segment.kind = SegmentKind::PmDc;
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("framed bulk fixture");
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("framed record fixture");
    };
    table.records[0].type_id = type_id;
    table.records[0].payload = View::over_retained(&payload);
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
    inventory(&ctx, &container.rse)
}

#[test]
fn sketch_transform_record_refuses_collection_limit_before_push() {
    let mut payload = content(0);
    payload.extend_from_slice(&0x8421u16.to_le_bytes());
    payload.extend_from_slice(&0x7bdeu16.to_le_bytes());
    assert_eq!(
        inventory_with_record(TRANSFORM_TYPE, &payload, DecodePolicy::service())
            .expect("transform is admitted")
            .transforms
            .len(),
        1
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        inventory_with_record(TRANSFORM_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Inventor PmDc transform record"
                && limit.used == 0
    ));
}

#[test]
fn sketch_transform_record_refuses_retained_limit_before_identity_copy() {
    let mut payload = content(0);
    payload.extend_from_slice(&0x8421u16.to_le_bytes());
    payload.extend_from_slice(&0x7bdeu16.to_le_bytes());
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 31;
    assert!(matches!(
        inventory_with_record(TRANSFORM_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor PmDc record type id"
                && limit.used == 0
    ));
    policy.limits.max_retained_bytes = 32;
    assert!(matches!(
        inventory_with_record(TRANSFORM_TYPE, &payload, policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor PmDc record segment token"
                && limit.used == 32
    ));
}

#[test]
fn sketch_parse_issue_refuses_collection_limit_before_push() {
    let admitted = inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
        .expect("truncated transform becomes an issue");
    assert_eq!(admitted.issues.len(), 1);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        inventory_with_record(TRANSFORM_TYPE, &[], policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Inventor PmDc sketch issue"
                && limit.used == 0
    ));
}

#[test]
fn sketch_parse_issue_refuses_entity_limit_before_push() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    assert!(matches!(
        inventory_with_record(TRANSFORM_TYPE, &[], policy),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Inventor PmDc sketch issue"
    ));
    assert_eq!(
        inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
            .expect("service issue")
            .issues
            .len(),
        1
    );
}

#[test]
fn sketch_parse_issue_copies_refuse_retained_limits_before_creation() {
    let issue = inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
        .expect("truncated transform becomes an issue")
        .issues
        .remove(0);
    let detail_len = issue.detail.len();
    let token_len = issue.segment_token.as_str().len();
    let type_id_len = 32;
    // Retained bytes count the owned type id, segment token, detail, and four issue slots.
    for (limit_bytes, operation, used) in [
        (
            type_id_len - 1,
            "retain Inventor PmDc sketch issue type id",
            0,
        ),
        (
            type_id_len + token_len - 1,
            "retain Inventor PmDc sketch issue segment token",
            type_id_len,
        ),
        (
            type_id_len + token_len + detail_len - 1,
            "retain Inventor PmDc sketch issue detail",
            type_id_len + token_len,
        ),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(limit_bytes);
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == operation
                    && limit.used == cadmpeg_core::decode::u64_from_index(used)
        ));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(
        type_id_len
            + token_len
            + detail_len
            + 4 * std::mem::size_of::<crate::record_issue::RecordIssue>(),
    );
    assert_eq!(
        inventory_with_record(TRANSFORM_TYPE, &[], policy)
            .expect("type id, token, and detail fit their retained allowance")
            .issues
            .len(),
        1
    );
}

#[test]
fn sketch_record_forms_refuse_collection_limit_before_push() {
    let mut sketch = content(0);
    sketch.extend_from_slice(&0u32.to_le_bytes());
    sketch.extend_from_slice(&0u32.to_le_bytes());
    sketch.extend(list(8, &[]));
    sketch.extend_from_slice(&[0; 16]);
    let mut direction = content(0);
    direction.extend_from_slice(&[0; 36]);
    let mut constraint = constraint_header(0, 0);
    constraint.extend_from_slice(&0u32.to_le_bytes());
    constraint.push(1);
    for (type_id, payload, operation) in [
        (SKETCH_TYPE, sketch, "admit Inventor PmDc sketch record"),
        (
            POINT_TYPE,
            point_bytes(0, 0, [0.0, 0.0]),
            "admit Inventor PmDc sketch entity record",
        ),
        (
            DIRECTION_TYPE,
            direction,
            "admit Inventor PmDc direction record",
        ),
        (
            HORIZONTAL_TYPE,
            constraint,
            "admit Inventor PmDc sketch constraint record",
        ),
    ] {
        let admitted = inventory_with_record(type_id, &payload, DecodePolicy::service())
            .expect("sketch record is admitted");
        assert_eq!(
            admitted.sketches.len()
                + admitted.entities.len()
                + admitted.transforms.len()
                + admitted.directions.len()
                + admitted.constraints.len(),
            1
        );
        assert!(admitted.issues.is_empty());
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(type_id, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
                    && limit.used == 0
        ));
    }
}
