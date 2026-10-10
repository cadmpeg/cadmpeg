// SPDX-License-Identifier: Apache-2.0

use super::super::{local_reference_candidates, ReferencePadding};

#[test]
fn local_reference_readings_cover_both_forms_and_the_extra_zero() {
    let mut bytes = vec![0; 12];
    bytes[0] = 1;
    bytes[1..9].copy_from_slice(&257_u64.to_le_bytes());

    let candidates = local_reference_candidates(&bytes, 0, true).map(|candidate| {
        candidate.map(|candidate| {
            (
                candidate.target,
                candidate.end,
                candidate.inline_type_guid,
                candidate.padding,
            )
        })
    });
    assert_eq!(
        candidates,
        [
            Some((257, 11, None, ReferencePadding::TwoZeros)),
            Some((257, 12, None, ReferencePadding::TwoZeros)),
            Some((1, 10, None, ReferencePadding::None)),
            Some((1, 11, None, ReferencePadding::None)),
        ]
    );

    let without_extra_zero = local_reference_candidates(&bytes, 0, false);
    assert!(without_extra_zero[1].is_none());
    assert!(without_extra_zero[3].is_none());
    assert!(local_reference_candidates(&bytes, 12, true)
        .iter()
        .all(Option::is_none));
}
