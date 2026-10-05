// SPDX-License-Identifier: Apache-2.0
use super::super::same_native_occurrence;

#[test]
fn native_occurrence_scan_preserves_utf8_and_ascii_digit_rules() {
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(same_native_occurrence(
        &ctx,
        "f3d:café/occurrence-12/BulkStream.dat:record#1",
        "f3d:café/occurrence-12/design:record#1",
    )
    .expect("admitted comparison"));
    assert!(!same_native_occurrence(
        &ctx,
        "f3d:xref/root/occurrence-１２/design:record#1",
        "f3d:xref/root/occurrence-１２/design:other#1",
    )
    .expect("admitted comparison"));
}

#[test]
fn native_occurrence_scan_propagates_and_fuses_work_refusal() {
    const OPERATION: &str = "compare F3D native occurrence IDs";
    for (left, right, refusals) in [
        (
            "f3d:xref/root/occurrence-0/design:record#1",
            "f3d:xref/root/occurrence-0/design:other#1",
            8,
        ),
        (
            "f3d:root/Design1/BulkStream.dat:record#1",
            "f3d:design:persistent-subentity-tag#1",
            4,
        ),
    ] {
        for skip in 0..refusals {
            let error = crate::test_support::resource_refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                OPERATION,
                skip,
                |ctx| same_native_occurrence(ctx, left, right),
            );
            let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
                panic!("native occurrence scan returns its work refusal");
            };
            assert_eq!(refusal.operation, OPERATION);
        }
    }
}
