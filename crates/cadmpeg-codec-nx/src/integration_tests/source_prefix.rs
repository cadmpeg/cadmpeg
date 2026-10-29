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

fn source(with_geometry: bool) -> Vec<u8> {
    let unit_prefix = test_pmarks::prefix(1);
    let mut prefix = transmit_header();
    prefix.extend_from_slice(&unit_prefix[test_pmarks::header().len()..]);
    if with_geometry {
        test_prt::prt_with_streams(&[&test_streams::topology_partition_stream(), &prefix])
    } else {
        test_prt::prt_with_partition(&prefix)
    }
}

#[test]
fn public_decode_installs_only_actual_partition_prefix_reads_with_the_retained_native_owner() {
    for with_geometry in [false, true] {
        let decoded = decode(source(with_geometry));
        let native = decoded.ir().native.namespace("nx").unwrap();
        let records = &native.arenas()["parasolid_deltas_partition_prefixes"];
        assert_eq!(records.len(), 1);
        let record = &records[0];
        let fields = record.fields().unwrap();
        assert_eq!(fields["partition"]["highest_id"], u32::MAX);
        assert_eq!(fields["current_pmark"]["identifier"], 0x1234_5678_u32);
        assert_eq!(decoded.ir().model.bodies.is_empty(), !with_geometry);
    }
}

#[test]
fn native_attachment_recovery_removes_prefix_reads_in_both_decode_routes() {
    for with_geometry in [false, true] {
        let complete = decode(source(with_geometry));
        assert_eq!(
            complete.ir().native.namespace("nx").unwrap().arenas()
                ["parasolid_deltas_partition_prefixes"]
                .len(),
            1
        );
        for point in [Point::Entry, Point::Exit] {
            let _injected = inject(point, || {
                cadmpeg_core::CodecError::malformed("source rollback control")
            });
            let recovered = decode(source(with_geometry));
            assert!(recovered
                .report()
                .losses
                .iter()
                .any(|loss| { loss.code.local_code() == "native.attachment-failed" }));
            assert!(recovered
                .ir()
                .native
                .namespace("nx")
                .is_none_or(|namespace| {
                    namespace
                        .arenas()
                        .get("parasolid_deltas_partition_prefixes")
                        .is_none_or(Vec::is_empty)
                }));
            assert_eq!(recovered.ir().model.bodies, complete.ir().model.bodies);
            assert_eq!(recovered.ir().model.points, complete.ir().model.points);
            assert_eq!(recovered.ir().model.surfaces, complete.ir().model.surfaces);
        }
    }
}
