// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::records_by_operation;

fn grouped_records_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<(), cadmpeg_core::CodecError> {
    let records = [("first".to_owned(), 1), ("first".to_owned(), 2)];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let grouped = records_by_operation(&ctx, &records, |record| record.0.as_str())?;
    assert_eq!(grouped["first"].iter().map(|record| record.1).collect::<Vec<_>>(), [1, 2]);
    Ok(())
}

#[test]
fn operation_record_index_refuses_collection_limit() {
    let error = grouped_records_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn operation_record_index_refuses_scoped_limit() {
    let error = grouped_records_with_limit(|policy| policy.limits.max_materialized_bytes = 0).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn operation_record_index_refuses_work_limit() {
    let error = grouped_records_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
