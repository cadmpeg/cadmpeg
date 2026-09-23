// SPDX-License-Identifier: Apache-2.0
//! Siemens NX PRT seed builders.

use std::io::Write;

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_core::CodecError;
use flate2::write::ZlibEncoder;
use flate2::Compression;

const MAGIC: &[u8; 8] = b"SPLMSSTR";

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

pub fn put_ref(rec: &mut [u8], at: usize, value: u16) {
    rec[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

pub fn record(tag: u8, len: usize) -> Result<Vec<u8>, CodecError> {
    if len < 2 {
        return Err(CodecError::InvalidInput(
            "NX seed record needs a two-byte header".into(),
        ));
    }
    let mut r = alloc_filled(len, 0_u8, "NX seed record")?;
    r[0] = 0x00;
    r[1] = tag;
    Ok(r)
}

/// A partition stream with one POINT, one PLANE, one CYLINDER and one LINE
/// record at their fixed lengths. Each record carries a node id at +2; the
/// surfaces and the line carry the `+` sense byte at +18.
fn partition_stream() -> Result<Vec<u8>, CodecError> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(b"XX: TRANSMIT FILE (partition) created by modeller version 3400176\x00");
    s.extend_from_slice(b"SCH_TEST_1_9999\x00");

    let mut pt = record(0x1d, 40)?;
    put_ref(&mut pt, 2, 2);
    put_vec3(&mut pt, 16, [0.0625, 0.0, 0.0127]);
    s.extend_from_slice(&pt);

    let mut pl = record(0x32, 91)?;
    put_ref(&mut pl, 2, 3);
    pl[18] = b'+';
    put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]);
    put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&pl);

    let mut cy = record(0x33, 99)?;
    put_ref(&mut cy, 2, 4);
    cy[18] = b'+';
    put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, 0.004_05);
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&cy);

    let mut ln = record(0x1e, 67)?;
    put_ref(&mut ln, 2, 5);
    ln[18] = b'+';
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

pub fn zlib_compress(raw: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(raw)?;
    e.finish()
}

pub fn single_part_prt() -> Result<Vec<u8>, CodecError> {
    single_part_prt_with_partition(&partition_stream()?)
}

/// Build an SPLMSSTR part whose HEADER directory holds one
/// `/Root/UG_PART/UG_PART` file entry spanning the zlib-compressed `stream`.
///
/// The HEADER region runs from the `HEADER` marker to the FOOTER region, whose
/// offset is the 48-bit little-endian value at byte 17. The FOOTER region holds
/// an empty counted directory followed by the four-byte fingerprint that ends
/// the file.
pub fn single_part_prt_with_partition(stream: &[u8]) -> Result<Vec<u8>, CodecError> {
    const FOOTER_OFFSET: usize = 17;
    const FOOTER_OFFSET_LEN: usize = 6;
    let offset_overflow = || CodecError::InvalidInput("NX seed directory offset overflows".into());

    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0x11, 0x22, 0x33]);
    f.extend_from_slice(&[0, 0, 0, 0]);
    f.push(0x00);
    f.extend_from_slice(&[0; FOOTER_OFFSET_LEN]);
    f.extend_from_slice(&[0, 0]);

    f.extend_from_slice(b"HEADER");
    f.extend_from_slice(&1_u32.to_le_bytes());
    let name = b"/Root/UG_PART/UG_PART";
    let name_len = u32::try_from(name.len())
        .map_err(|_| CodecError::InvalidInput("NX seed entry name length overflows".into()))?;
    f.extend_from_slice(&name_len.to_le_bytes());
    f.extend_from_slice(name);

    let blob = zlib_compress(stream)?;
    let dir_end = f.len().checked_add(16).ok_or_else(offset_overflow)?;
    let blob_off = u64::try_from(dir_end).map_err(|_| offset_overflow())?;
    let blob_len = u64::try_from(blob.len())
        .map_err(|_| CodecError::InvalidInput("NX seed partition length overflows".into()))?;
    f.extend_from_slice(&blob_off.to_le_bytes());
    f.extend_from_slice(&blob_len.to_le_bytes());
    f.extend_from_slice(&blob);

    let footer_offset = u64::try_from(f.len()).map_err(|_| offset_overflow())?;
    let footer_offset = footer_offset.to_le_bytes();
    let (footer_offset, high) = footer_offset.split_at(FOOTER_OFFSET_LEN);
    if high.iter().any(|&byte| byte != 0) {
        return Err(CodecError::InvalidInput(
            "NX seed footer offset exceeds 48 bits".into(),
        ));
    }
    f[FOOTER_OFFSET..FOOTER_OFFSET + FOOTER_OFFSET_LEN].copy_from_slice(footer_offset);
    f.extend_from_slice(b"FOOTER");
    f.extend_from_slice(&0_u32.to_le_bytes());
    f.extend_from_slice(&[0; 4]);
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::{record, single_part_prt_with_partition};
    use cadmpeg_container::compression::inflate_zlib_probe;

    #[test]
    fn long_partition_has_exact_directory_offset_and_size() {
        let mut state = 0x1234_5678u32;
        let stream = (0..10_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state >> 16) as u8
            })
            .collect::<Vec<_>>();
        let file = single_part_prt_with_partition(&stream).expect("seed partition");
        let name = b"/Root/UG_PART/UG_PART";
        assert_eq!(&file[25..31], b"HEADER");
        assert_eq!(&file[31..35], &1_u32.to_le_bytes());
        let directory = 8 + 1 + 3 + 4 + 1 + 6 + 2 + 6 + 4 + 4 + name.len();
        let offset =
            cadmpeg_core::decode::View::u64_le_at(&file, directory).expect("partition offset");
        let size =
            cadmpeg_core::decode::View::u64_le_at(&file, directory + 8).expect("partition length");
        let offset = usize::try_from(offset).expect("host partition offset");
        let size = usize::try_from(size).expect("host partition length");
        let mut footer_offset = [0; 8];
        footer_offset[..6].copy_from_slice(&file[17..23]);
        let footer_offset =
            usize::try_from(u64::from_le_bytes(footer_offset)).expect("host footer offset");
        assert_eq!(offset, directory + 16);
        assert_eq!(footer_offset, offset + size);
        assert_eq!(&file[footer_offset..], b"FOOTER\0\0\0\0\0\0\0\0");
        let recovered =
            inflate_zlib_probe(&file[offset..footer_offset], stream.len()).expect("zlib partition");
        assert_eq!(recovered, stream);
    }

    #[test]
    fn record_refuses_length_below_its_header() {
        assert!(record(1, 0).is_err());
        assert!(record(1, 1).is_err());
        assert_eq!(record(1, 2).expect("minimal record"), [0, 1]);
    }
}
