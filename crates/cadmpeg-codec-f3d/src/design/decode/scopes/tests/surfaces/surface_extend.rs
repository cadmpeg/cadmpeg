// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::surfaces::exact_surface_extend_operation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use crate::test_support::indexed_header;

const SCOPE: u32 = 40;
const OWNER: u32 = 41;
const CARRIER: u32 = 44;
const EDGE: u32 = 47;
const FLAG_RECORD: u32 = 60;

fn marked(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

fn compact_distance_owner(bytes: &mut Vec<u8>, distance: f64) -> usize {
    let start = bytes.len();
    indexed_header(bytes, *b"394", OWNER);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[1, 1, 0, 0, 0]);
    marked(bytes, SCOPE);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&distance.to_le_bytes());
    marked(bytes, OWNER + 1);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    marked(bytes, SCOPE);
    bytes.extend_from_slice(&[0; 2]);
    marked(bytes, OWNER + 2);
    bytes.push(0);
    marked(bytes, SCOPE);
    assert_eq!(bytes.len() - start, 103);
    indexed_header(bytes, *b"258", OWNER);
    start
}

struct Tail {
    trailing_count: u32,
    role: u32,
    ordinal: u32,
    ordinal_copy: u32,
    flags: &'static [u8],
}

const COMPACT_TAIL: Tail = Tail {
    trailing_count: 1,
    role: 8,
    ordinal: 125,
    ordinal_copy: 125,
    flags: &[0, 0],
};

fn boundary_carrier(bytes: &mut Vec<u8>, tail: &Tail) -> usize {
    let start = bytes.len();
    indexed_header(bytes, *b"269", CARRIER);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    marked(bytes, EDGE);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&tail.trailing_count.to_le_bytes());
    marked(bytes, FLAG_RECORD);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&tail.role.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&tail.ordinal.to_le_bytes());
    bytes.extend_from_slice(&4.85e-5f64.to_le_bytes());
    bytes.extend_from_slice(&tail.ordinal_copy.to_le_bytes());
    marked(bytes, CARRIER + 2);
    bytes.extend_from_slice(tail.flags);
    marked(bytes, CARRIER + 1);
    bytes.push(0);
    marked(bytes, SCOPE);
    indexed_header(bytes, *b"258", CARRIER);
    start
}

fn surface_extend_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:test:surface-extend#1",
        crate::records::feature::scope::DesignFeatureKind::SurfaceExtend,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![OWNER, CARRIER, EDGE]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn decode(bytes: &[u8]) -> Option<DesignSurfaceExtendOperation> {
    exact_surface_extend_operation(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        &surface_extend_scope(),
    )
}

fn fixture(distance: f64, tail: &Tail) -> (Vec<u8>, usize, usize) {
    let mut bytes = Vec::new();
    let owner_at = compact_distance_owner(&mut bytes, distance);
    let carrier_at = boundary_carrier(&mut bytes, tail);
    (bytes, owner_at, carrier_at)
}

#[test]
fn compact_owner_and_87_byte_boundary_tail_decode_the_extend_operation() {
    let (bytes, owner_at, carrier_at) = fixture(2.54, &COMPACT_TAIL);
    let tail = carrier_at + 36;
    assert_eq!(
        decode(&bytes),
        Some(DesignSurfaceExtendOperation {
            distance: crate::test_support::real(2.54),
            distance_offset: u64_from_index(owner_at + 40),
            distance_record_index: OWNER,
            boundary_record_index: CARRIER,
            boundary_reference_record_index: FLAG_RECORD,
            boundary_reference_offset: u64_from_index(tail + 6),
            edge_record_indices: vec![EDGE],
            tolerance: cadmpeg_ir::scalar::PositiveReal::new(4.85e-5)
                .expect("checked fixture value"),
            tolerance_offset: u64_from_index(tail + 39),
        })
    );
}

#[test]
fn compact_boundary_tail_admits_a_set_variant_and_closing_flag() {
    let (bytes, _, _) = fixture(
        2.54,
        &Tail {
            flags: &[1, 1],
            ..COMPACT_TAIL
        },
    );
    assert!(decode(&bytes).is_some());
}

#[test]
fn surface_extend_keeps_a_signed_distance() {
    let (bytes, _, _) = fixture(-0.02, &COMPACT_TAIL);
    assert_eq!(
        decode(&bytes).map(|operation| operation.distance.get()),
        Some(-0.02)
    );
}

#[test]
fn boundary_group_rejects_each_violated_tail_invariant() {
    for tail in [
        Tail {
            trailing_count: 2,
            ..COMPACT_TAIL
        },
        Tail {
            role: 0x41,
            ..COMPACT_TAIL
        },
        Tail {
            ordinal: 0,
            ordinal_copy: 0,
            ..COMPACT_TAIL
        },
        Tail {
            ordinal_copy: 126,
            ..COMPACT_TAIL
        },
        Tail {
            flags: &[2, 0],
            ..COMPACT_TAIL
        },
        Tail {
            flags: &[0, 0, 0],
            ..COMPACT_TAIL
        },
    ] {
        let (bytes, _, _) = fixture(2.54, &tail);
        assert_eq!(decode(&bytes), None);
    }
}
