// SPDX-License-Identifier: Apache-2.0
//! Source-coordinate and retained-frame owner invariants.

use crate::test_support::test_b2::b2_adjacent_face_counted_owner_stream;
use crate::wire::records::{consolidated_records, consolidated_records_in_range_sources};

#[test]
fn compact_face_double_three_counted_owner_survives_native_wire_roundtrip() {
    let mut bytes = b2_adjacent_face_counted_owner_stream();
    bytes[10] = 0x03;
    let records = consolidated_records(&bytes);
    let packets = crate::test_support::with_service_context(|_ctx| {
        crate::native::projection::consolidated_owner_packets(&bytes, &records)
            .expect("valid owner records")
    });
    let [packet] = packets.as_slice() else {
        panic!("one owner packet")
    };
    let face = packet.face_node.as_ref().expect("face relation");
    assert_eq!(face.terminal, [0x03, 0x03]);
    assert_eq!(face.target.checked_add(1), packet.payload.final_reference());
    let wire = serde_json::to_value(packet).expect("owner wire");
    assert_eq!(
        serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(wire)
            .expect("valid owner wire"),
        *packet
    );
}

#[test]
fn owned_child_face_target_survives_native_projection_and_wire_roundtrip() {
    let mut bytes = [
        0xb2, 0x03, 0x5f, 0x04, 0x05, 0x82, 0x07, 0x0b, 0x03, 0xb2, 0x03, 0x30, 0x02, 0x05, 0x80,
        0x01, 0xb2, 0x03, 0x37, 0x02, 0x05, 0x81, 0x05, 0xb2, 0x03, 0x62, 0x14, 0x05, 0x8d, 0x03,
        0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x03, 0x05, 0x86, 0x41, 0x17,
        0xdc, 0x03, 0x01,
    ];
    let records = consolidated_records(&bytes);
    let packets = crate::test_support::with_service_context(|_ctx| {
        crate::native::projection::consolidated_owner_packets(&bytes, &records)
            .expect("valid owner records")
    });
    let [packet] = packets.as_slice() else {
        panic!("one owner packet")
    };
    let face = packet.face_node.as_ref().expect("face relation");
    assert_eq!(face.frame.pos, 0);
    assert_eq!(face.target, 1);
    assert_eq!(packet.frame.pos, 23);
    let wire = serde_json::to_value(packet).expect("owner wire");
    assert_eq!(wire["face_node"]["target_encoding"], "owned_child");
    assert_eq!(
        serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(wire)
            .expect("valid owner wire"),
        *packet
    );

    bytes[8] = 0x05;
    let records = consolidated_records(&bytes);
    let packets = crate::native::projection::consolidated_owner_packets(&bytes, &records)
        .expect("project owner with unsupported face link");
    let [packet] = packets.as_slice() else {
        panic!("retain the owner packet");
    };
    assert!(packet.face_node.is_none());
    let wire = serde_json::to_value(packet).expect("owner wire");
    assert_eq!(
        serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(wire)
            .expect("owner wire without an unsupported face link"),
        *packet
    );
}

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
    let packets = crate::test_support::with_service_context(|_ctx| {
        crate::native::projection::consolidated_owner_packets(&bytes, &records)
            .expect("valid source fixture")
    });
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
        let packets = crate::test_support::with_service_context(|_ctx| {
            crate::native::projection::consolidated_owner_packets(&bytes, &records)
                .expect("valid source fixture")
        });
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
        let packets = crate::test_support::with_service_context(|_ctx| {
            crate::native::projection::consolidated_owner_packets(&stream, &records)
                .expect("valid source fixture")
        });
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

#[test]
fn counted_numeric_bounds_round_trip_and_reject_changed_projection() {
    let mut bytes = b2_adjacent_face_counted_owner_stream();
    let owner_start = 5 + usize::from(bytes[3]);
    let old_tail_len = 5;
    bytes.truncate(bytes.len() - old_tail_len);
    let tail_start = bytes.len();
    bytes.extend_from_slice(&[0x83, 0x41, 0, 0, 0x0d]);
    for value in [0.0_f64, -1.0, 2.0, 3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    for value in [-2.0_f32, 2.0, -3.0, 3.0, -4.0, 4.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let new_tail_len = bytes.len() - tail_start;
    bytes[owner_start + 3] =
        u8::try_from(usize::from(bytes[owner_start + 3]) - old_tail_len + new_tail_len)
            .expect("small owner frame");
    let native = crate::native::CatiaNative::decode(&bytes);
    let packet = &native.consolidated_owner_packets[0];
    let wire = serde_json::to_value(packet).expect("owner wire");
    assert!(wire["payload"]["numeric_bounds"].is_object());
    assert_eq!(
        serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(wire.clone())
            .expect("valid wire"),
        *packet
    );
    let mut invalid = wire.clone();
    invalid["payload"]["numeric_bounds"]["upper"][0] = serde_json::json!(1.0);
    assert!(
        serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(invalid).is_err()
    );
    let mut absent = wire;
    absent["payload"]
        .as_object_mut()
        .expect("payload object")
        .remove("numeric_bounds");
    assert!(serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(absent).is_err());
}
