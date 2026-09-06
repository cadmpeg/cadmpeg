// SPDX-License-Identifier: Apache-2.0
//! Checked reference fixtures for native projection tests.

pub(crate) fn boolean_reference(
    value: u32,
    offset: u64,
) -> crate::om::PayloadObjectReference<crate::om::reference_index::ReferenceIndexToken, u64> {
    let value = u16::try_from(value).unwrap();
    let [high, low] = value.to_be_bytes();
    crate::om::PayloadObjectReference {
        token: crate::om::reference_index::ReferenceIndexToken::from_wire(
            u32::from(value), &[0x90, high, low],
        ).unwrap(),
        offset,
    }
}
