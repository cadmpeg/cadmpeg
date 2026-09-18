// SPDX-License-Identifier: Apache-2.0
//! Siemens NX PRT seed builders.

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_core::CodecError;

pub const MAGIC: &[u8; 8] = b"SPLMSSTR";

fn be_f64(v: f64) -> [u8; 8] {
    v.to_be_bytes()
}

pub fn put_vec3(rec: &mut [u8], at: usize, xyz: [f64; 3]) {
    for (i, v) in xyz.iter().enumerate() {
        rec[at + 8 * i..at + 8 * i + 8].copy_from_slice(&be_f64(*v));
    }
}

pub fn put_f64(rec: &mut [u8], at: usize, v: f64) {
    rec[at..at + 8].copy_from_slice(&be_f64(v));
}

pub fn record(tag: u8, len: usize) -> Result<Vec<u8>, CodecError> {
    let mut r = alloc_filled(len, 0_u8, "NX seed record")?;
    r[0] = 0x00;
    r[1] = tag;
    Ok(r)
}

pub fn partition_stream() -> Result<Vec<u8>, CodecError> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(b"XX: TRANSMIT FILE (partition) created by modeller version 3400176\x00");
    s.extend_from_slice(b"SCH_TEST_1_9999\x00");

    let mut pt = record(0x1d, 40)?;
    put_vec3(&mut pt, 16, [0.0625, 0.0, 0.0127]);
    s.extend_from_slice(&pt);

    let mut pl = record(0x32, 91)?;
    put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]);
    put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&pl);

    let mut cy = record(0x33, 99)?;
    put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, 0.004_05);
    s.extend_from_slice(&cy);

    let mut ln = record(0x1e, 67)?;
    put_vec3(&mut ln, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut ln, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&ln);

    Ok(s)
}

pub fn just_magic() -> Vec<u8> {
    MAGIC.to_vec()
}

pub fn assembly_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0, 0, 0]);
    f.extend_from_slice(&[0, 0, 0, 0]);
    f.push(0x00);
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    f.extend_from_slice(&[0u8; 16]);
    f
}
