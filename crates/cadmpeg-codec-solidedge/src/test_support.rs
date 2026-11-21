// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_container::compound::CompoundClassId;
use cadmpeg_test_support::bytes::{put_u16, put_u32};

/// Root class of a part document.
pub(crate) const PART: CompoundClassId = CompoundClassId::from_guid(
    0x23c5_2e80,
    0x4698,
    0x11ce,
    [0xb3, 0x07, 0x08, 0x00, 0x36, 0x3a, 0x1e, 0x02],
);
/// Root class of a sheet-metal document.
pub(crate) const SHEET_METAL: CompoundClassId = CompoundClassId::from_guid(
    0xdd85_22e0,
    0x2375,
    0x11d0,
    [0xac, 0x05, 0x08, 0x00, 0x36, 0xfd, 0x18, 0x02],
);
/// Root class of an assembly document.
pub(crate) const ASSEMBLY: CompoundClassId = CompoundClassId::from_guid(
    0x00c6_bf00,
    0x483b,
    0x11ce,
    [0x95, 0x1a, 0x08, 0x00, 0x36, 0x01, 0xbe, 0x52],
);
/// Root class of a family-of-assemblies document.
pub(crate) const FAMILY: CompoundClassId = CompoundClassId::from_guid(
    0x04d6_13a0,
    0xa322,
    0x40b5,
    [0xa2, 0xa4, 0x36, 0xca, 0x0d, 0xe6, 0xf5, 0xd9],
);
/// A root class that no Solid Edge document kind uses.
pub(crate) const OTHER: CompoundClassId =
    CompoundClassId::from_guid(0x0000_0001, 0x0002, 0x0003, [4, 5, 6, 7, 8, 9, 10, 11]);

/// Compound document whose root holds the two named stream entries and the given class.
pub(crate) fn compound_with(first: &str, second: &str, class: CompoundClassId) -> Vec<u8> {
    let mut file = cadmpeg_test_support::compound::compound_fixture();
    let directory = &mut file[512..1024];
    directory[80..96].copy_from_slice(&class.to_bytes());
    directory[128..192].fill(0);
    name(&mut directory[128..256], first);
    put_u32(directory, 128 + 72, 2);
    let entry = &mut directory[256..384];
    name(entry, second);
    entry[66] = 2;
    entry[67] = 0;
    entry[68..80].fill(0xff);
    put_u32(entry, 116, 0xffff_fffe);
    file
}

pub(crate) fn compound() -> Vec<u8> {
    compound_with("PSMroots", "JSitesList", PART)
}

fn name(entry: &mut [u8], value: &str) {
    for (i, unit) in value.encode_utf16().enumerate() {
        put_u16(entry, i * 2, unit);
    }
    put_u16(entry, 64, u16::try_from((value.len() + 1) * 2).unwrap());
}

/// A Solid Edge directory whose `PSMcluster0` stream holds `cluster`,
/// zero-padded to the 4096-byte regular-stream cutoff.
pub(crate) fn with_cluster(cluster: &[u8]) -> Vec<u8> {
    assert!(cluster.len() <= 4096);
    let mut file = compound();
    let directory = &mut file[512..1024];
    // JGlobal becomes empty so the cluster owns the data chain.
    put_u32(directory, 128 + 116, 0xffff_fffe);
    put_u32(directory, 128 + 120, 0);
    put_u32(directory, 256 + 72, 3);
    let entry = &mut directory[384..512];
    entry.fill(0);
    name(entry, "PSMcluster0");
    entry[66] = 2;
    entry[67] = 1;
    entry[68..80].fill(0xff);
    put_u32(entry, 116, 1);
    put_u32(entry, 120, 4096);
    let data = &mut file[1024..1024 + 4096];
    data.fill(0);
    data[..cluster.len()].copy_from_slice(cluster);
    file
}

/// One property record value.
pub(crate) enum RecordValue {
    Word(u32, u32),
    Real(f64),
}

/// A property set with index `index` holding `records` in order.
pub(crate) fn property_set(index: u32, records: &[(&str, RecordValue)]) -> Vec<u8> {
    property_set_of_form(1, index, records)
}

/// A property set with set form `form`.
pub(crate) fn property_set_of_form(
    form: u16,
    index: u32,
    records: &[(&str, RecordValue)],
) -> Vec<u8> {
    let mut out = vec![0, 0, 0, 0, 0x51, 0x51];
    out.extend_from_slice(&form.to_le_bytes());
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
    for (record, (name, value)) in records.iter().enumerate() {
        let units = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        out.extend_from_slice(&u32::try_from(units.len() * 2).unwrap().to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&u32::try_from(record).unwrap().to_le_bytes());
        out.extend_from_slice(&1_u32.to_le_bytes());
        match value {
            RecordValue::Word(kind, word) => {
                out.extend_from_slice(&kind.to_le_bytes());
                out.extend_from_slice(&word.to_le_bytes());
            }
            RecordValue::Real(real) => {
                out.extend_from_slice(&4_u32.to_le_bytes());
                out.extend_from_slice(&real.to_le_bytes());
            }
        }
    }
    out
}
