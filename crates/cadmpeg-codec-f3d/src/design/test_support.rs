// SPDX-License-Identifier: Apache-2.0
//! Shared helpers for design-owner unit tests.

use crate::design::decode::parameters::design_parameter_discriminator;

pub(crate) fn lp_utf16(out: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    out.extend_from_slice(&(units.len() as u32).to_le_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

pub(crate) fn parameter_record(
    owner: Option<u32>,
    expression: &str,
    source_kind: &str,
    unit: Option<&str>,
    name: &str,
    evaluated_value: f64,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"305");
    out.extend_from_slice(&71u32.to_le_bytes());
    out.extend_from_slice(&[0; 11]);
    out.extend_from_slice(&design_parameter_discriminator(source_kind).to_le_bytes());
    out.push(0);
    out.extend_from_slice(&9u32.to_le_bytes());
    match owner {
        Some(owner) => {
            out.push(1);
            out.extend_from_slice(&owner.to_le_bytes());
            out.extend_from_slice(&[0; 6]);
        }
        None => out.push(0),
    }
    lp_utf16(&mut out, expression);
    out.extend_from_slice(if owner.is_some() {
        &[0; 9]
    } else {
        &[0, 0, 0, 0, 0, 0, 0, 0, 1]
    });
    lp_utf16(&mut out, source_kind);
    out.extend_from_slice(&0u32.to_le_bytes());
    if let Some(unit) = unit {
        lp_utf16(&mut out, unit);
    }
    lp_utf16(&mut out, name);
    out.extend_from_slice(&evaluated_value.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

pub(crate) fn parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 104];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"292");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40..48].copy_from_slice(&6.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..53].copy_from_slice(&45u32.to_le_bytes());
    frame[59..63].copy_from_slice(&9u32.to_le_bytes());
    frame[67] = 1;
    frame[68..72].copy_from_slice(&12u32.to_le_bytes());
    frame[78] = 1;
    frame[79] = 1;
    frame[81] = 1;
    frame[82..86].copy_from_slice(&46u32.to_le_bytes());
    frame[93] = 1;
    frame[94..98].copy_from_slice(&12u32.to_le_bytes());
    frame
}

pub(crate) fn push_reference(out: &mut Vec<u8>, reference: u32) {
    out.push(1);
    out.extend_from_slice(&reference.to_le_bytes());
}

pub(crate) fn push_genesis_block(out: &mut Vec<u8>, genesis: u64) {
    out.push(1);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&13u32.to_le_bytes());
    out.extend_from_slice(b"EntityGenesis");
    out.extend_from_slice(&23u32.to_le_bytes());
    out.extend_from_slice(b"IntrinsicMetaTypeuint64");
    out.extend_from_slice(&genesis.to_le_bytes());
}

pub(crate) mod dump;
