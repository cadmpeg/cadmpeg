// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::tests::segment;
use crate::feature::{PmDcWorkPlanePayload, WORK_PLANE_TYPE};
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::test_fixtures::{content_at, parse as with_context};
use cadmpeg_ir::scalar::FiniteReal;

const OWNER: u32 = 0x8000_0011;
const TARGET: u32 = 0x8000_0012;
const ROLE: u32 = 7;
const TAIL_START: usize = 22;

fn fixture() -> Vec<u8> {
    let mut bytes = content_at(1, 19, &[]);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&OWNER.to_le_bytes());
    bytes.extend_from_slice(&[0x01, 0x05]);
    bytes.extend_from_slice(&TARGET.to_le_bytes());
    bytes.extend_from_slice(&ROLE.to_le_bytes());
    bytes
}

fn run(bytes: &[u8], version: u8) -> Result<Option<PmDcFeaturePropertyPayload>, CodecError> {
    with_context(bytes, |ctx, source| parse(ctx, source, version))
}

fn plane(origin: [f64; 3], u: [f64; 3], v: [f64; 3]) -> PmDcWorkPlane {
    let real = |values: [f64; 3]| values.map(|value| FiniteReal::new(value).expect("finite"));
    Located::new(
        PmDcWorkPlanePayload {
            save_version_major: 19,
            header: crate::feature::tests::test_header(),
            entity_flags: 0,
            values: vec![0; 3],
            marker: false,
            origin: real(origin),
            u_axis: real(u),
            v_axis: real(v),
        },
        RecordTypeId::from_bytes(WORK_PLANE_TYPE),
        segment(),
        1,
    )
}

#[test]
fn plane_reference_keeps_the_owner_target_and_role() {
    let parsed = run(&fixture(), 19)
        .expect("record envelope")
        .expect("plane reference");
    assert_eq!(
        parsed.kind,
        PmDcFeaturePropertyKind::Mirror {
            definition: super::super::Property::PlaneReference {
                owner: PmDcReference::from_packed(OWNER),
                target: PmDcReference::from_packed(TARGET),
                role: ROLE,
            }
        }
    );
}

#[test]
fn plane_reference_checks_the_state_and_marker_bytes() {
    use crate::layout::pm_dc_mirror_plane_reference_tail_v19 as layout;

    assert_eq!(layout::LEN, fixture().len() - TAIL_START);
    for offset in [0, 8, 9] {
        let mut changed = fixture();
        changed[TAIL_START + offset] ^= 1;
        assert!(run(&changed, 19).expect("record envelope").is_none());
    }
}

#[test]
fn plane_reference_requires_distinct_nonnull_qualified_references() {
    for (offset, value) in [
        (4, 0_u32),
        (4, 0x8000_0000),
        (4, 0x11),
        (10, 0),
        (10, 0x8000_0000),
        (10, 0x12),
        (10, OWNER),
    ] {
        let mut changed = fixture();
        changed[TAIL_START + offset..TAIL_START + offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(run(&changed, 19).expect("record envelope").is_none());
    }
}

#[test]
fn plane_reference_requires_the_stored_version_and_exact_length() {
    let original = fixture();
    for len in 0..original.len() {
        assert!(!matches!(run(&original[..len], 19), Ok(Some(_))));
    }
    let mut extended = original.clone();
    extended.push(0);
    assert!(run(&extended, 19).expect("record envelope").is_none());
    for version in [15, 18, 20, 21, 23, 25, 28, 29, 30, 31] {
        assert!(run(&original, version).expect("version gate").is_none());
    }
}

#[test]
fn work_planes_with_one_plane_match_whatever_their_axes_and_normal_sign() {
    let base = plane([2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let turned = plane([2.0, 5.0, -3.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]);
    let flipped = plane([2.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    assert!(same_plane(&base, &turned));
    assert!(same_plane(&turned, &base));
    assert!(same_plane(&base, &flipped));
}

#[test]
fn work_planes_with_another_offset_or_direction_do_not_match() {
    let base = plane([2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let offset = plane([2.5, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let tilted = plane([2.0, 0.0, 0.0], [1.0e-6, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let rotated = plane([2.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let degenerate = plane([2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]);
    for other in [&offset, &tilted, &rotated, &degenerate] {
        assert!(!same_plane(&base, other));
        assert!(!same_plane(other, &base));
    }
}
