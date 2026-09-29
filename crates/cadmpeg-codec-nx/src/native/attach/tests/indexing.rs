// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::insert_source_property;
use crate::native::attach::insert_source_property_reference;
use crate::native::attach::last_record_index;
use crate::native::attach::records_by_operation;
use crate::native::attach::segment_binding_body_indexes;

#[test]
fn source_property_reference_preserves_block_and_numeric_fallback() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut properties = std::collections::BTreeMap::new();
    insert_source_property_reference(
        &ctx,
        &mut properties,
        format_args!("reference.{}", 0),
        Some("source-block"),
        17u32,
    )
    .unwrap();
    insert_source_property_reference(
        &ctx,
        &mut properties,
        format_args!("reference.{}", 1),
        None,
        17u32,
    )
    .unwrap();
    assert_eq!(properties["reference.0"], "source-block");
    assert_eq!(properties["reference.1"], "17");
}

#[test]
fn operation_body_operand_source_keys_follow_reference_ordinal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut properties = std::collections::BTreeMap::new();
    for reference_ordinal in [0, 1] {
        insert_source_property_reference(
            &ctx,
            &mut properties,
            format_args!("operation_body_operand.{}.{}", reference_ordinal, 0),
            None,
            20u32,
        )
        .unwrap();
    }
    assert_eq!(properties["operation_body_operand.0.0"], "20");
    assert_eq!(properties["operation_body_operand.1.0"], "20");
}

fn source_property_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut properties = std::collections::BTreeMap::new();
    insert_source_property(
        &ctx,
        &mut properties,
        format_args!("body_write.{}", 7),
        format_args!("{}", "write"),
    )?;
    assert_eq!(properties["body_write.7"], "write");
    Ok(())
}

#[test]
fn source_property_formats_key_and_value() {
    source_property_with_limit(|_| {}).unwrap();
}

#[test]
fn source_property_refuses_collection_limit() {
    let error =
        source_property_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn source_property_refuses_retained_limit() {
    let error =
        source_property_with_limit(|policy| policy.limits.max_retained_bytes = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn source_property_refuses_work_limit() {
    let error = source_property_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn segment_body_index_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), cadmpeg_core::CodecError> {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let body = cadmpeg_ir::ids::BodyId::mint("nx:s2:body#3").unwrap();
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        id: body.clone(),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let binding = crate::native::segments::SegmentBodyBinding {
        id: "binding".into(),
        stream_link: "stream".into(),
        stream_ordinal: 2,
        stream_kind: crate::parasolid::StreamKind::Partition,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 0,
        source_offset: 0,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let bindings = [binding];
    let indexes = segment_binding_body_indexes(&ctx, &ir, &bindings)?;
    assert_eq!(
        indexes.by_object[&10].as_slice(),
        std::slice::from_ref(&body)
    );
    assert_eq!(
        indexes.by_object[&11].as_slice(),
        std::slice::from_ref(&body)
    );
    assert_eq!(indexes.by_binding["binding"], [body]);
    Ok(())
}

#[test]
fn segment_body_index_refuses_collection_limit() {
    let error =
        segment_body_index_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn segment_body_index_refuses_scoped_limit() {
    let error = segment_body_index_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn segment_body_index_refuses_work_limit() {
    let error =
        segment_body_index_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn last_record_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), cadmpeg_core::CodecError> {
    let records = [("operation", 1u32), ("operation", 2u32)];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let index = last_record_index(&ctx, records)?;
    assert_eq!(index["operation"], 2);
    Ok(())
}

#[test]
fn last_record_index_refuses_collection_limit() {
    let error =
        last_record_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn last_record_index_refuses_scoped_limit() {
    let error =
        last_record_with_limit(|policy| policy.limits.max_materialized_bytes = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn last_record_index_refuses_work_limit() {
    let error = last_record_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn manual_group_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut reservation = ctx.reserve_scoped(0, "NX feature operation group indexes")?;
    let mut grouped = std::collections::BTreeMap::new();
    ctx.push_scoped_btree_group(&mut reservation, &mut grouped, "operation", || 1u32, 0, "NX feature operation group index")?;
    ctx.push_scoped_btree_group(&mut reservation, &mut grouped, "operation", || 2u32, 0, "NX feature operation group index")?;
    assert_eq!(grouped["operation"], [1, 2]);
    Ok(())
}

#[test]
fn manual_operation_group_refuses_collection_limit() {
    let error =
        manual_group_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn manual_operation_group_refuses_scoped_limit() {
    let error =
        manual_group_with_limit(|policy| policy.limits.max_materialized_bytes = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn manual_operation_group_refuses_work_limit() {
    let error = manual_group_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn grouped_records_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), cadmpeg_core::CodecError> {
    let records = [("first".to_owned(), 1), ("first".to_owned(), 2)];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let grouped = records_by_operation(&ctx, &records, |record| record.0.as_str())?;
    assert_eq!(
        grouped["first"]
            .iter()
            .map(|record| record.1)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    Ok(())
}

#[test]
fn operation_record_index_refuses_collection_limit() {
    let error =
        grouped_records_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn operation_record_index_refuses_scoped_limit() {
    let error =
        grouped_records_with_limit(|policy| policy.limits.max_materialized_bytes = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn operation_record_index_refuses_work_limit() {
    let error = grouped_records_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
