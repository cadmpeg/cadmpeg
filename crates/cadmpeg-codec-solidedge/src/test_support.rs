// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::bytes::{put_u16, put_u32};

pub(crate) fn compound() -> Vec<u8> {
    let mut file = cadmpeg_test_support::compound::compound_fixture();
    let directory = &mut file[512..1024];
    directory[128..192].fill(0);
    name(&mut directory[128..256], "JGlobal");
    put_u32(directory, 128 + 72, 2);
    let entry = &mut directory[256..384];
    name(entry, "JSitesList");
    entry[66] = 2;
    entry[67] = 0;
    entry[68..80].fill(0xff);
    put_u32(entry, 116, 0xffff_fffe);
    file
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
