// SPDX-License-Identifier: Apache-2.0
//! Shared `PmDc` record fixtures for the feature projection tests.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;

use crate::container::InventorContainer;
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::test_support::test_fixtures::primary_envelope_fixture;

pub(crate) fn inventory_with_record(
    type_id: [u8; 16],
    payload: &[u8],
    policy: DecodePolicy,
) -> Result<super::super::FeatureInventory, CodecError> {
    with_record(type_id, payload, policy, |ctx, rse| {
        let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
            cadmpeg_ir::CodecFormat::Inventor,
            ctx,
        );
        super::super::inventory(ctx, rse, &mut parts)
    })
}

/// Run `read` over a `PmDc` segment whose first record has `type_id` and
/// `payload`.
pub(crate) fn with_record<T>(
    type_id: [u8; 16],
    payload: &[u8],
    policy: DecodePolicy,
    read: impl FnOnce(&DecodeContext<'_>, &crate::rse::RseInventory<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
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
    read(&ctx, &container.rse)
}
