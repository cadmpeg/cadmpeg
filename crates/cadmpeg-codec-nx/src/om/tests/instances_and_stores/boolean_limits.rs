// SPDX-License-Identifier: Apache-2.0
//! Resource refusals for Boolean operation references.

use super::{boolean_operations_with_labels_for_test, operation_labels};
use crate::om::BooleanOperationKind;

fn boolean_operation_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let labels = super::operation_labels(bytes, 100);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    configure(&mut policy);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    crate::om::boolean_operations_with_labels(&ctx, bytes, 100, &labels).unwrap_err()
}

#[test]
fn boolean_operations_refuse_collection_limit() {
    let error = boolean_operation_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn boolean_operations_refuse_retained_limit() {
    let error = boolean_operation_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn boolean_operations_refuse_work_limit() {
    let error = boolean_operation_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
#[test]
fn om_boolean_operations_decode_counted_target_and_tools() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let operations =
        boolean_operations_with_labels_for_test(bytes, 100, &operation_labels(bytes, 100));
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].kind, BooleanOperationKind::Subtract);
    assert_eq!(operations[0].target.token.value(), 6494);
    assert_eq!(
        operations[0].target.token.raw().to_vec(),
        [0x90, 0x19, 0x5e]
    );
    assert_eq!(
        operations[0].target.offset,
        100 + bytes
            .windows(3)
            .position(|window| window == [0x90, 0x19, 0x5e])
            .unwrap()
    );
    assert_eq!(
        operations[0]
            .tools
            .iter()
            .map(|token| token.token.value())
            .collect::<Vec<_>>(),
        [6495, 6468, 6467, 6496]
    );
    assert_eq!(
        operations[0]
            .tools
            .iter()
            .map(|token| token.token.raw().to_vec())
            .collect::<Vec<_>>(),
        [
            vec![0x90, 0x19, 0x5f],
            vec![0x90, 0x19, 0x44],
            vec![0x90, 0x19, 0x43],
            vec![0x90, 0x19, 0x60],
        ]
    );
    assert_eq!(
        operations[0]
            .tools
            .iter()
            .map(|token| token.offset)
            .collect::<Vec<_>>(),
        [0x5f, 0x44, 0x43, 0x60].map(|low| {
            100 + bytes
                .windows(3)
                .position(|window| window == [0x90, 0x19, low])
                .unwrap()
        })
    );

    let mut invalid = bytes.to_vec();
    *invalid.last_mut().unwrap() = 1;
    assert!(
        boolean_operations_with_labels_for_test(&invalid, 0, &operation_labels(&invalid, 0))
            .is_empty()
    );
}
