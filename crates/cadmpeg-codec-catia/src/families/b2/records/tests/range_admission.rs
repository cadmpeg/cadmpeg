use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

use crate::families::b2::records::b2_counted_owners_from_records;
use crate::test_support::test_b2::b2_adjacent_face_counted_owner_stream;
use crate::wire::records::consolidated_records;

#[test]
fn b2_counted_owner_reference_range_refuses_work() {
    const OPERATION: &str = "catia_b2_counted_owner_reference_scan";
    let bytes = b2_adjacent_face_counted_owner_stream();
    let records = consolidated_records(&bytes);
    let owners = crate::test_support::with_service_context(|ctx| {
        b2_counted_owners_from_records(ctx, &bytes, &records)
    })
    .expect("service counted owners");
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].references, [911, 7, 263, 258, 281, 276, 917]);

    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        OPERATION,
        |cap| {
            crate::test_support::with_work_limit(cap, |ctx| {
                let result = b2_counted_owners_from_records(ctx, &bytes, &records);
                if let Err(CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == OPERATION));
}
