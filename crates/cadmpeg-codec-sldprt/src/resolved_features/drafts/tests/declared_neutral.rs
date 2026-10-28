//! Declared neutral-frame field and owner boundaries.

use super::super::unique_declared_draft_direction;

pub(super) fn frame(direction: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0; 153];
    b[..8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    b[12..16].copy_from_slice(&13u32.to_le_bytes());
    for (i, value) in [-1.0f64, 1.0, 1.0, -1.0, 2.0, 3.0, 4.0]
        .into_iter()
        .chain(direction)
        .enumerate()
    {
        b[24 + 8 * i..32 + 8 * i].copy_from_slice(&value.to_le_bytes());
    }
    b[129..137].copy_from_slice(&1.0f64.to_le_bytes());
    b
}

#[test]
fn signed_axes_use_the_declared_field_despite_auxiliary_vectors() {
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let mut expected = [0.0; 3];
            expected[axis] = sign;
            let b = frame(expected);
            let actual = unique_declared_draft_direction(&b, 0, b.len())
                .expect("complete declared frame")
                .get();
            assert_eq!([actual.x, actual.y, actual.z], expected);
        }
    }
}

#[test]
fn malformed_truncated_and_repeated_frames_withhold_the_direction() {
    let valid = frame([0.0, 0.0, -1.0]);
    for (at, value) in [(24, 0.0f64), (56, f64::NAN), (80, 2.0), (104, 1.0)] {
        let mut b = valid.clone();
        b[at..at + 8].copy_from_slice(&value.to_le_bytes());
        assert!(unique_declared_draft_direction(&b, 0, b.len()).is_none());
    }
    for at in [8, 16] {
        let mut b = valid.clone();
        b[at] = 1;
        assert!(unique_declared_draft_direction(&b, 0, b.len()).is_none());
    }
    let mut no_address = valid.clone();
    no_address[12..16].fill(0);
    assert!(unique_declared_draft_direction(&no_address, 0, no_address.len()).is_none());
    assert!(unique_declared_draft_direction(&valid, 0, 119).is_none());
    assert!(unique_declared_draft_direction(&valid, 1, valid.len()).is_none());
    for malformed in [false, true] {
        let mut repeated = valid.clone();
        let mut second = valid.clone();
        if malformed {
            second[24] ^= 1;
        }
        repeated.extend(second);
        assert!(unique_declared_draft_direction(&repeated, 0, repeated.len()).is_none());
    }
}
