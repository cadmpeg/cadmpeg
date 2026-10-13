// SPDX-License-Identifier: Apache-2.0
//! Source-coordinate and retained-frame owner invariants.

use crate::test_support::test_b2::b2_adjacent_face_counted_owner_stream;
use crate::wire::records::{consolidated_records, consolidated_records_in_range_sources};

#[test]
fn owner_incidence_preserves_reversed_physical_extents() {
    let stream = b2_adjacent_face_counted_owner_stream();
    let split = 5 + usize::from(stream[3]);
    let mut bytes = stream[split..].to_vec();
    let node_start = bytes.len();
    bytes.extend_from_slice(&stream[..split]);
    let records = consolidated_records_in_range_sources(
        &bytes,
        [vec![node_start..bytes.len(), 0..node_start]],
    );
    let packets = crate::test_support::with_service_context(|ctx| {
        crate::native::projection::consolidated_owner_packets(ctx, &bytes, &records)
    })
    .expect("owner frames fit service limits");
    let [packet] = packets.as_slice() else {
        panic!("one owner")
    };
    let node = packet
        .face_node
        .as_ref()
        .expect("logical predecessor survives physical reversal");
    assert_eq!(packet.frame.pos, 0);
    assert_eq!(
        node.frame.pos,
        u64::try_from(node_start).expect("small offset")
    );
    assert_eq!(
        packet.source_offset,
        u64::try_from(split).expect("small logical offset")
    );
    assert_eq!(node.source_offset, 0);
    assert_eq!(node.frame.payload.as_slice(), &stream[5..split]);
    assert_eq!(packet.frame.payload.as_slice(), &stream[split + 5..]);
    let wire = serde_json::to_value(packet).expect("owner wire");
    assert_eq!(
        serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(wire)
            .expect("source-aware wire"),
        *packet
    );
}

#[test]
fn owner_incidence_refuses_unframed_gaps_and_other_sources() {
    let stream = b2_adjacent_face_counted_owner_stream();
    let split = 5 + usize::from(stream[3]);
    let mut gap = stream.clone();
    gap.insert(split, 0);
    let cases = [
        (gap.clone(), consolidated_records(&gap)),
        (
            stream.clone(),
            consolidated_records_in_range_sources(
                &stream,
                [
                    std::iter::once(0..split),
                    std::iter::once(split..stream.len()),
                ],
            ),
        ),
    ];
    for (bytes, records) in cases {
        let packets = crate::test_support::with_service_context(|ctx| {
            crate::native::projection::consolidated_owner_packets(ctx, &bytes, &records)
        })
        .expect("owner frames fit service limits");
        assert_eq!(packets.len(), 1);
        assert!(packets[0].face_node.is_none());
    }
}

#[test]
fn owner_wire_refuses_frame_field_and_logical_incidence_mutations() {
    let bytes = b2_adjacent_face_counted_owner_stream();
    let native = crate::native::CatiaNative::decode(&bytes);
    let packet = &native.consolidated_owner_packets[0];
    let valid = serde_json::to_value(packet).expect("owner wire");
    for field in [
        "value",
        "encoding",
        "tail",
        "node-target",
        "node-offset",
        "owner-offset",
        "frame",
    ] {
        let mut invalid = valid.clone();
        match field {
            "value" => invalid["payload"]["references"][0] = serde_json::json!(0),
            "encoding" => {
                invalid["payload"]["reference_encodings"][0] = serde_json::json!("owned_child");
            }
            "tail" => invalid["payload"]["tail"] = serde_json::json!("01"),
            "node-target" => invalid["face_node"]["target"] = serde_json::json!(0),
            "node-offset" => invalid["face_node"]["source_offset"] = serde_json::json!(1),
            "owner-offset" => invalid["source_offset"] = serde_json::json!(0),
            "frame" => invalid["frame"]["payload"] = serde_json::json!(""),
            _ => unreachable!("known mutation"),
        }
        assert!(
            serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(invalid).is_err(),
            "accepted {field}"
        );
    }
}

#[test]
fn counted_incidence_requires_two_pairs_and_one_final_reference() {
    for count in 1u8..=5 {
        let mut stream = vec![0xb2, 0x83, 0x5f, 4, 5, 0x82, 5, 3, 5];
        let mut payload = vec![0x80 + count];
        payload.extend(std::iter::repeat_n(3, usize::from(count - 1)));
        payload.extend_from_slice(&[9, 1]);
        stream.extend_from_slice(&[
            0xb2,
            3,
            0x62,
            u8::try_from(payload.len()).expect("small payload"),
            5,
        ]);
        stream.extend_from_slice(&payload);
        let records = crate::wire::records::consolidated_records(&stream);
        let packets = crate::test_support::with_service_context(|ctx| {
            crate::native::projection::consolidated_owner_packets(ctx, &stream, &records)
        })
        .expect("counted owner frames fit service limits");
        assert_eq!(
            packets.len(),
            1,
            "generic packet is retained for count {count}"
        );
        assert_eq!(
            packets
                .iter()
                .filter(|packet| packet.face_node.is_some())
                .count(),
            usize::from(count == 5),
            "incidence count {count}"
        );
    }
}
