// SPDX-License-Identifier: Apache-2.0
use crate::test_support::{owned_deltas, test_pmarks, with_decode_context};

#[test]
fn owned_definitions_are_shared_and_instances_retain_exact_source_ranges() {
    let (declaration, ordinary, _) = owned_deltas::body(36);
    let mut bytes = owned_deltas::body_stream(36);
    let second = bytes.len();
    bytes.extend(&ordinary);
    let bytes = test_pmarks::complete(bytes);
    let streams = [owned_deltas::stream(bytes.clone())];
    let events = super::super::parasolid_deltas_events(&streams);
    assert_eq!(events.schemas.len(), 3);
    assert_eq!(events.instances.len(), 2);
    assert!(events.records.is_empty());
    assert_eq!(events.metadata.len(), 4);
    assert!(events.residual_spans.is_empty());
    let first = serde_json::to_value(&events.instances[0]).unwrap();
    let later = serde_json::to_value(&events.instances[1]).unwrap();
    let schema = serde_json::to_value(&events.schemas[2]).unwrap();
    assert_eq!(first["schema"], schema["id"]);
    assert_eq!(later["schema"], schema["id"]);
    assert!(first.get("definition").is_none());
    assert_eq!(
        first["identity_offset"],
        test_pmarks::prefix(1).len() + declaration.len()
    );
    assert_eq!(later["inflated_offset"], second);
    assert_eq!(later["byte_len"], ordinary.len());
    assert_eq!(first["fields"].as_array().unwrap().len(), 36);
    let owner = with_decode_context(|_ctx| crate::records::DeltasRecords::read(&bytes)).unwrap();
    assert_eq!(
        owner
            .events()
            .iter()
            .map(|event| event.range().len())
            .sum::<usize>(),
        bytes.len()
    );
    assert_eq!(owner.census().bytes_decoded(), bytes.len());
}

#[test]
fn unsupported_packets_and_every_later_marker_remain_one_exact_suffix() {
    for suffix in [
        test_pmarks::packet(),
        vec![0, 1, 0, 1],
        vec![0xde, 0xad, 0, 19, 255],
    ] {
        let mut bytes = test_pmarks::prefix(1);
        let at = bytes.len()
            + if suffix.starts_with(&test_pmarks::TERMINATOR) {
                4
            } else {
                0
            };
        bytes.extend(suffix);
        bytes.extend(owned_deltas::region(9, 52, b'S'));
        let streams = [owned_deltas::stream(bytes.clone())];
        let events = super::super::parasolid_deltas_events(&streams);
        assert!(events.instances.is_empty());
        assert_eq!(events.transmit_headers.len(), 1);
        assert_eq!(events.partition_prefixes.len(), 1);
        assert_eq!(events.residual_spans.len(), 1);
        let span = serde_json::to_value(&events.residual_spans[0]).unwrap();
        assert_eq!(span["inflated_offset"], at);
        assert_eq!(span["byte_len"], bytes.len() - at);
        assert_eq!(
            span["sha256"],
            serde_json::to_value(cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[at..]))
                .unwrap()
        );
    }
}

#[test]
fn region_projection_uses_the_same_owned_fields_as_generic_native_values() {
    for layout in [7, 8, 9] {
        for region_type in *b"SV" {
            let mut bytes = test_pmarks::prefix(1);
            bytes.extend(owned_deltas::region(layout, 52, region_type));
            let streams = [owned_deltas::stream(test_pmarks::complete(bytes))];
            let events = super::super::parasolid_deltas_events(&streams);
            assert_eq!(events.instances.len(), 1);
            let [record] = events.records.as_slice() else {
                panic!("REGION projection");
            };
            let crate::deltas::record_family::RecordFamily::Region { fields } = &record.family
            else {
                panic!("REGION");
            };
            assert_eq!(record.family.node_id(), u32::MAX);
            assert_eq!(u8::from(fields.region_type), region_type);
            assert_eq!(fields.references().count(), usize::from(layout - 2));
            let wire = serde_json::to_value(&events.instances[0]).unwrap();
            assert_eq!(wire["fields"][0]["values"][0]["value"], u32::MAX);
        }
    }
}

#[test]
fn two_streams_join_instances_to_their_own_aggregate_schema_indices() {
    let mut region = test_pmarks::prefix(1);
    region.extend(owned_deltas::region(7, 8, b'S'));
    let streams = [
        owned_deltas::stream(test_pmarks::complete(owned_deltas::body_stream(36))),
        owned_deltas::stream(test_pmarks::complete(region)),
    ];
    let events = super::super::parasolid_deltas_events(&streams);
    assert_eq!(events.schemas.len(), 6);
    assert_eq!(events.instances.len(), 2);
    assert_eq!(events.metadata.len(), 8);
    for (ordinal, expected_kind) in [(0, 12), (1, 19)] {
        let instance = serde_json::to_value(&events.instances[ordinal]).unwrap();
        assert_eq!(instance["stream_ordinal"], ordinal);
        let schema = events
            .schemas
            .iter()
            .map(|schema| serde_json::to_value(schema).unwrap())
            .find(|schema| schema["id"] == instance["schema"])
            .unwrap();
        assert_eq!(schema["stream_ordinal"], ordinal);
        assert_eq!(schema["definition"]["kind"], expected_kind);
    }
}
