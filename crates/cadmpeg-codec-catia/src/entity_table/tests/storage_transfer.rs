// SPDX-License-Identifier: Apache-2.0
//! Actual entity candidate destinations and transfer refusals.

use cadmpeg_core::decode::{
    refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;

fn destination_precedes_candidate_promotion<T>(
    bytes: &[u8],
    destination: &'static str,
    promotion: &'static str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let cap = {
        let _probe = RefusalProbe::arm(ResourceDimension::CollectionItems, destination, None);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .expect("source fits the input limit");
        let CodecError::ResourceLimit(limit) = run(&ctx).err().expect("destination is reached")
        else {
            panic!("destination must refuse its collection slot");
        };
        assert_eq!(limit.operation, destination);
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        limit.used
    };
    let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, promotion, None);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = cap;
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("source fits the input limit");
    let CodecError::ResourceLimit(limit) = run(&ctx).err().expect("destination refuses first")
    else {
        panic!("destination must preserve its refusal");
    };
    assert_eq!(limit.operation, destination);
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    let Err(CodecError::ResourceLimit(sticky)) = ctx.charge_work(1, "after entity destination refusal")
    else {
        panic!("entity destination refusal must stay sticky");
    };
    assert_eq!(sticky, limit);
}

#[test]
fn materialized_run_reserves_its_destination_before_candidate_promotion() {
    let bytes = super::record(&[0x11], 37);
    destination_precedes_candidate_promotion(
        &bytes,
        "collect CATIA 7C05 materialized runs",
        "catia_entity_run_records",
        |ctx| super::super::parse_runs(ctx, &bytes),
    );
    let runs = super::parse_runs(&bytes);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].len(), 1);
    assert_eq!(runs[0][0].entity_id, 37);
    assert_eq!(super::nested_body(&runs[0][0]).1, [0x11]);
}

#[test]
fn numeric_packet_reserves_its_destination_before_actual_item_transfer() {
    let mut bytes = vec![0xaa, 0x81, 0x87, 0xe8, 0xf4, 0x1a, 0x37, 0x83, 0x84, 0xe6];
    bytes.extend_from_slice(&42.0_f64.to_bits().to_le_bytes());
    bytes.extend_from_slice(&[0xfe, 0xbb]);
    let fields = crate::value_block::tokenize(&bytes);
    destination_precedes_candidate_promotion(
        &bytes,
        "catia_numeric_value_packets",
        "catia_numeric_packet_candidate_output",
        |ctx| super::super::value_packets_charged(ctx, &bytes, &fields),
    );
    let error = crate::test_support::with_retained_refusal(
        &bytes,
        "catia_numeric_packet_candidate_output",
        |ctx| super::super::value_packets_charged(ctx, &bytes, &fields),
    ).expect_err("selected numeric items require retained transfer");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "catia_numeric_packet_candidate_output"));
    let packets = crate::test_support::with_service_context(|ctx| {
        super::super::value_packets_charged(ctx, &bytes, &fields)
    }).expect("selected numeric packet is admitted");
    assert!(matches!(packets.as_slice(), [super::EntityValuePacket::Numeric {
        offset: 1, prefix_atoms: [1, 7], type_selector: 0x1af4,
        layout_atom: 3, value_atom: 4, items, terminator_count: 1,
    }] if matches!(items.as_slice(), [super::NumericPacketItem::Binary64 {
        bits, offset: 9,
    }] if *bits == 42.0_f64.to_bits())));
}

#[test]
fn invalid_required_value_frame_rejects_before_identity_scan() {
    let mut bytes = super::record(&[0x11; 128], 37);
    let frame = bytes.len() - 8;
    bytes[frame + 1] = 0x08;
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan CATIA 7C05 identities", None);
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let runs = super::super::parse_runs(ctx, &bytes).expect("fixed tag rejects before identity work");
        assert!(runs.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn selected_reference_signature_preserves_actual_production_transfer_refusal() {
    let bytes = [
        0x32, 0xcf, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x8c, 0x81, b'2', b'(', b'E', b',',
        b'0', b'(', b'E', b',', b'4', b')', b')', 0xfe, 0x32, 0xd0, 0, 0, 0, 0x83, 0xe9, 0xe0,
        0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe,
    ];
    let error = crate::test_support::with_retained_refusal(
        &bytes,
        "catia_reference_signature_candidate",
        |ctx| super::super::parse_reference_signature(ctx, &bytes),
    ).expect_err("selected signature requires retained transfer");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "catia_reference_signature_candidate"));
    assert_eq!(
        super::parse_reference_signature(&bytes),
        Some(super::complete_reference_signature_wire().try_into().expect("valid fixed production")),
    );
}
