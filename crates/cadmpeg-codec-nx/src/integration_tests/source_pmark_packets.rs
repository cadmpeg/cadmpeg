// SPDX-License-Identifier: Apache-2.0
use super::decode;
use crate::native::attach_failure::{inject, Point};
use crate::test_support::{test_pmarks, test_prt, test_streams};

fn transmit_header() -> Vec<u8> {
    let mut bytes = crate::test_support::test_bytes::transmit_preamble(
        ": TRANSMIT FILE (deltas)",
        "SCH_3501171_35102_13006",
    );
    assert_eq!(bytes.pop(), Some(0));
    bytes.extend(239_u16.to_be_bytes());
    bytes.extend(0_u32.to_be_bytes());
    bytes
}

fn source(with_geometry: bool, identifier: u32) -> Vec<u8> {
    let prefix = test_pmarks::prefix(40_001);
    let mut stream = transmit_header();
    stream.extend_from_slice(&prefix[test_pmarks::header().len()..]);
    stream.extend([0, 1, 0, 1]);
    stream.extend(test_pmarks::node(40_000, 1, 40_001, 1));
    stream.extend(test_pmarks::node(40_001, 40_000, 49_948, identifier));
    stream.extend([0, 1, 0, 1, 0, 1, 0, 0, 0, 1]);
    if with_geometry {
        test_prt::prt_with_streams(&[&test_streams::topology_partition_stream(), &stream])
    } else {
        test_prt::prt_with_partition(&stream)
    }
}

#[test]
fn public_decode_keeps_real_compound_packet_values_and_exact_source_capture_in_both_routes() {
    for with_geometry in [false, true] {
        for identifier in [2_u32, 7] {
            let decoded = decode(source(with_geometry, identifier));
            let namespace = decoded.ir().native.namespace("nx").unwrap();
            let packets = &namespace.arenas()["parasolid_deltas_pmark_packets"];
            assert_eq!(packets.len(), 1);
            let record = &packets[0];
            let fields = record.fields().unwrap();
            assert_eq!(fields["nodes"].as_array().unwrap().len(), 2);
            assert_eq!(fields["nodes"][0]["identifier"], 1);
            assert_eq!(fields["nodes"][1]["identifier"], identifier);
            assert_eq!(fields["nodes"][1]["preceding"], 40_000);
            assert_eq!(fields["terminal"], true);
            let layout = &decoded.source_fidelity().record_layout()[record.id()];
            let prefix_body_len = test_pmarks::prefix(40_001).len() - test_pmarks::header().len();
            assert_eq!(prefix_body_len, 53);
            let start = u64::try_from(transmit_header().len() + prefix_body_len).unwrap();
            assert_eq!(layout.offset(), start);
            assert_eq!(layout.byte_len(), 90);
            assert_eq!(
                layout.reads()[layout.stream()],
                [start + 6..start + 41, start + 43..start + 80]
            );
            assert!(layout.reads().contains_key("file"));
            assert_eq!(decoded.ir().model.bodies.is_empty(), !with_geometry);
        }
    }
}

#[test]
fn native_attachment_recovery_drops_packet_capture_with_its_native_owner() {
    for with_geometry in [false, true] {
        let complete = decode(source(with_geometry, 2));
        let native = complete.ir().native.namespace("nx").unwrap();
        let owner = native.arenas()["parasolid_deltas_pmark_packets"][0].id();
        assert!(complete
            .source_fidelity()
            .record_layout()
            .contains_key(owner));
        let _injected = inject(Point::Exit, || {
            cadmpeg_core::CodecError::malformed("packet owner rollback control")
        });
        let recovered = decode(source(with_geometry, 2));
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "native.attachment-failed"));
        assert!(!recovered
            .source_fidelity()
            .record_layout()
            .contains_key(owner));
        assert_eq!(recovered.ir().model.bodies, complete.ir().model.bodies);
        assert_eq!(recovered.ir().model.points, complete.ir().model.points);
        assert_eq!(recovered.ir().model.surfaces, complete.ir().model.surfaces);
    }
}
