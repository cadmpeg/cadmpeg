// SPDX-License-Identifier: Apache-2.0
use super::SymbolicThreadWitness;
use crate::om::operation_record::OperationPayload;

fn payload(sentinel: bool) -> (Vec<u8>, usize) {
    let mut bytes = vec![3, 1, 2];
    // Two zeros and shifted binary64 0.1, copied exactly; point=(0,0,25)mm.
    let tuple = [0, 0, 0x2f, 0xb9, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a];
    bytes.extend(tuple);
    bytes.extend(tuple);
    bytes.extend([1, 3, 0xf0, 0, 0xf0, 255, 0xf1, 1, 0]);
    if sentinel {
        bytes.push(255);
    }
    let cut = bytes.len();
    bytes.extend(b"\x03\x05Cut\0");
    (bytes, cut)
}

#[test]
fn symbolic_thread_witness_preserves_zero_widths_references_and_duplicate_bytes() {
    crate::test_support::with_decode_context(|ctx| {
        for sentinel in [false, true] {
            let (bytes, cut) = payload(sentinel);
            let record = OperationPayload::new(&bytes, 100, "SYMBOLIC_THREAD").unwrap();
            let (field, offset) = SymbolicThreadWitness::read(ctx, record, cut + 100)
                .unwrap()
                .unwrap();
            assert_eq!(offset, 103);
            assert_eq!(field.point_mm(), [0., 0., 25.]);
            assert_eq!(field.byte_len(), cut - 3);
            assert_eq!(
                field
                    .tail_references
                    .map(crate::om::reference_index::PayloadIndexToken::value),
                [0, 255, 256]
            );
            assert_eq!(field.tail_sentinel, sentinel);
            for end in 0..bytes.len() {
                let truncated =
                    OperationPayload::new(&bytes[..end], 100, "SYMBOLIC_THREAD").unwrap();
                assert!(SymbolicThreadWitness::read(ctx, truncated, cut + 100)
                    .unwrap()
                    .is_none());
            }
            let mut different = bytes.clone();
            different[12] = 1;
            assert!(SymbolicThreadWitness::read(
                ctx,
                OperationPayload::new(&different, 100, "SYMBOLIC_THREAD").unwrap(),
                cut + 100
            )
            .unwrap()
            .is_none());
            assert!(SymbolicThreadWitness::read(
                ctx,
                OperationPayload::new(&bytes, 100, "THREADS").unwrap(),
                cut + 100
            )
            .unwrap()
            .is_none());
        }
    });
}

#[test]
fn symbolic_thread_witness_abstains_when_zero_atoms_also_parse_inside_a_scalar() {
    let (mut bytes, cut) = payload(false);
    for start in [5, 15] {
        bytes[start..start + 8].copy_from_slice(&[0x2f, 0xf0, 0, 0, 0, 0, 0, 0]);
    }
    crate::test_support::with_decode_context(|ctx| {
        // The six terminal zero bytes also spell two zero triples. Neither
        // candidate has a demonstrated tuple-start boundary, so both abstain.
        assert!(SymbolicThreadWitness::read(
            ctx,
            OperationPayload::new(&bytes, 0, "SYMBOLIC_THREAD").unwrap(),
            cut
        )
        .unwrap()
        .is_none());
    });
}

#[test]
fn symbolic_thread_witness_refuses_work_limit() {
    let (bytes, cut) = payload(false);
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            let error = SymbolicThreadWitness::read(
                ctx,
                OperationPayload::new(&bytes, 0, "SYMBOLIC_THREAD").unwrap(),
                cut,
            )
            .unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
            );
        },
    );
}

#[test]
fn symbolic_thread_witness_accepts_complete_zero_tuple() {
    let (mut bytes, cut) = payload(false);
    bytes.splice(3..23, [0; 6]);
    crate::test_support::with_decode_context(|ctx| {
        let (field, start) = SymbolicThreadWitness::read(
            ctx,
            OperationPayload::new(&bytes, 100, "SYMBOLIC_THREAD").unwrap(),
            cut - 14 + 100,
        )
        .unwrap()
        .unwrap();
        assert_eq!(field.point_mm(), [0.; 3]);
        assert_eq!(start, 103);
        assert_eq!(field.byte_len(), 15);
    });
}
