use crate::container;
use crate::test_support::test_om::segment_om_record_area_payload;
use crate::test_support::test_prt::prt_with_named_payloads;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn record_area_limit_error(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]);

    crate::test_support::with_decode_context_over(
        &file,
        |_| {},
        |scan_ctx| {
            let container = container::scan_bytes(scan_ctx, file.as_slice()).unwrap();

            crate::test_support::with_decode_context_over(
                &[],
                |policy| {
                    configure(policy);
                },
                |ctx| super::super::om_record_areas(ctx, &container).unwrap_err(),
            )
        },
    )
}

#[test]
fn om_record_area_route_refuses_collection_limit() {
    let error = record_area_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn om_record_area_route_refuses_retained_limit() {
    let error = record_area_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn om_record_area_route_refuses_work_limit() {
    let error = record_area_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}
