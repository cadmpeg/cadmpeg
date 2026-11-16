// SPDX-License-Identifier: Apache-2.0
//! Synthetic thread-definition record bytes.

pub(crate) fn definition_bytes(
    owner: u32,
    label: u32,
    nominal: &str,
    designation: &str,
    standard: &str,
    form: u32,
    pitch: &str,
) -> Vec<u8> {
    fn text(bytes: &mut Vec<u8>, value: &str) {
        let units: Vec<_> = value.encode_utf16().collect();
        bytes.extend_from_slice(
            &u32::try_from(units.len())
                .expect("fixture text length")
                .to_le_bytes(),
        );
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for value in [0, 0, owner | 0x8000_0000, label | 0x8000_0000, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [nominal, designation, standard] {
        text(&mut bytes, value);
    }
    for value in [0, form, 0, 0, 0, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    text(&mut bytes, "synthetic class");
    for value in ["", "", "", "", "", "", "", pitch] {
        text(&mut bytes, value);
    }
    bytes
}
