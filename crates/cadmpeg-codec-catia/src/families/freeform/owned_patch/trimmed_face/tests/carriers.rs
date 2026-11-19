// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::consolidated_records;

fn frame(bytes: &mut Vec<u8>, class: u8, payload: &[u8]) {
    bytes.extend([
        0xb2,
        0x03,
        class,
        u8::try_from(payload.len()).expect("short payload"),
        5,
    ]);
    bytes.extend_from_slice(payload);
}

fn chamfer_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut plane = vec![0x84, 0x00];
    for value in [
        1.0_f64, 2.0, 3.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -2.0, 2.0, -3.0, 3.0,
    ] {
        plane.extend(value.to_le_bytes());
    }
    frame(&mut bytes, 0x27, &plane);
    for reference in [0x05, 0x09] {
        let mut offset = vec![reference, reference];
        offset.extend(0.0_f64.to_le_bytes());
        offset.push(0x15);
        for value in [-2.0_f64, 2.0, -3.0, 3.0] {
            offset.extend(value.to_le_bytes());
        }
        frame(&mut bytes, 0x30, &offset);
    }
    for _ in 0..2 {
        let mut line = vec![0x05, 0x0d];
        line.extend(0.0_f64.to_le_bytes());
        frame(&mut bytes, 0x18, &line);
    }
    let mut chamfer = vec![
        0x85, 0x15, 0x11, 0x0d, 0x09, 0x05, 0x09, 0x01, 0x01, 0x05, 0x05, 0x0d,
    ];
    chamfer.extend(1.0_f64.to_le_bytes());
    chamfer.extend(1.0_f64.to_le_bytes());
    frame(&mut bytes, 0x3b, &chamfer);
    bytes
}

fn carrier_point(bytes: &[u8]) -> Option<Point3> {
    let records = consolidated_records(bytes);
    let source = Source {
        bytes,
        records: &records,
        resolve: |index: usize, reference: AllocationReference| {
            let value = usize::try_from(reference.value).ok()?;
            match reference.encoding {
                AllocationReferenceEncoding::OwnedChild => index.checked_add(1)?.checked_add(value),
                AllocationReferenceEncoding::BackwardDistance => index.checked_sub(value),
                _ => None,
            }
        },
        physical: Some,
        tensors: &BTreeMap::new(),
    };
    let ir = CadIr::empty();
    let carriers = BTreeMap::new();
    let mut refusal = LaneRefusals::new();
    source
        .carrier(&ir, 5, &carriers, &mut refusal)?
        .evaluate([0.5, 0.75])
}

#[test]
fn chamfer_retains_its_plane_result_with_offset_supports() {
    let bytes = chamfer_stream();
    assert_eq!(carrier_point(&bytes), Some(Point3::new(1.5, 2.75, 3.0)));
}

#[test]
fn chamfer_requires_matching_result_kind_and_resolved_support_types() {
    let bytes = chamfer_stream();
    let records = consolidated_records(&bytes);
    let start = records[5].payload().expect("chamfer payload").start;
    let mut wrong_result = bytes.clone();
    wrong_result[start + 6] = 0x05;
    assert_eq!(carrier_point(&wrong_result), None);
    let mut wrong_support = bytes.clone();
    wrong_support[records[1].range().expect("support frame").start + 2] = 0x18;
    assert_eq!(carrier_point(&wrong_support), None);
    let mut unresolved = bytes;
    unresolved[start + 2] = 0xfd;
    assert_eq!(carrier_point(&unresolved), None);
}
