// SPDX-License-Identifier: Apache-2.0
//! CATIA V5 CATPart container seed builders.

use std::io;

pub const OUTER_MAGIC: &[u8; 8] = b"V5_CFV2\0";
pub const DIR_MAGIC: &[u8; 16] = b"CATIA_V5 CB0001\0";

pub fn outer_magic() -> Vec<u8> {
    OUTER_MAGIC.to_vec()
}

pub fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

fn le_f32(v: f32) -> [u8; 4] {
    v.to_le_bytes()
}

fn be_f32(v: f32) -> [u8; 4] {
    v.to_be_bytes()
}

fn main_stream() -> Vec<u8> {
    let mut b = Vec::new();
    for _ in 0..2 {
        b.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    }
    b.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    for xyz in [[0.0f32, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]] {
        b.extend_from_slice(&[0x05, 0x08, 0x01]);
        for v in xyz {
            b.extend_from_slice(&le_f32(v));
        }
    }
    b
}

fn surf_stream() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
    b.push(0x00);
    b.push(0x1a);
    b.extend_from_slice(&[0x00, 0x33, 0x33]);
    for v in [0.0f32, 0.0, 0.0, 0.0, 0.0, 5.0] {
        b.extend_from_slice(&be_f32(v));
    }
    b
}

pub fn descriptor(name: &str, phys_off: u32, phys_len: u32) -> io::Result<Vec<u8>> {
    let units = name.encode_utf16().collect::<Vec<_>>();
    if units.len() > 32 {
        return Err(io::Error::other("descriptor name exceeds 32 UTF-16 units"));
    }
    let mut b = vec![0u8; 0x54];
    b[0x0c..0x10].copy_from_slice(&be32(phys_len));
    for (slot, unit) in b[0x10..0x50].chunks_exact_mut(2).zip(units) {
        slot.copy_from_slice(&unit.to_le_bytes());
    }
    b[0x50..0x54].copy_from_slice(&be32(1));
    b.extend_from_slice(&be32(phys_off));
    b.extend_from_slice(&be32(phys_len));
    b.extend_from_slice(&be32(phys_len));
    b.extend_from_slice(&be32(0));
    b.extend_from_slice(&be32(0));
    Ok(b)
}

pub fn standard_catpart() -> io::Result<Vec<u8>> {
    let main = main_stream();
    let surf = surf_stream();
    let main_off = 16u32;
    let surf_off = main_off + main.len() as u32;
    let dir_rel = surf_off + surf.len() as u32;

    let mut dir = Vec::new();
    dir.extend_from_slice(DIR_MAGIC);
    dir.extend_from_slice(&descriptor("MainDataStream", main_off, main.len() as u32)?);
    dir.extend_from_slice(&descriptor("SurfacicReps", surf_off, surf.len() as u32)?);
    dir.extend_from_slice(b"CB__END");
    let b_len = dir.len() as u32;

    let mut inner = Vec::new();
    inner.extend_from_slice(OUTER_MAGIC);
    inner.extend_from_slice(&be32(dir_rel));
    inner.extend_from_slice(&be32(b_len));
    inner.extend_from_slice(&main);
    inner.extend_from_slice(&surf);
    inner.extend_from_slice(&dir);

    let mut f = Vec::new();
    f.extend_from_slice(OUTER_MAGIC);
    let outer_dir_off = 16u32 + inner.len() as u32;
    f.extend_from_slice(&be32(outer_dir_off));
    f.extend_from_slice(&be32(0));
    f.extend_from_slice(&inner);
    Ok(f)
}

pub fn zero_entity_catpart() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(OUTER_MAGIC);
    f.extend_from_slice(&be32(0));
    f.extend_from_slice(&be32(0));
    for _ in 0..5 {
        f.extend_from_slice(&[0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
    f
}

#[cfg(test)]
mod tests {
    use super::descriptor;

    #[test]
    fn descriptor_bounds_and_encodes_utf16_name_field() {
        let bytes = descriptor("é😀", 0, 0).expect("short name");
        assert_eq!(&bytes[0x10..0x16], &[0xe9, 0x00, 0x3d, 0xd8, 0x00, 0xde]);
        assert!(descriptor(&"x".repeat(33), 0, 0).is_err());
    }
}
