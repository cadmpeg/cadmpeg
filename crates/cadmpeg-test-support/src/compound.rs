// SPDX-License-Identifier: Apache-2.0
//! A synthetic compound-file (CFB) byte fixture.
//!
//! The CLI package has no library target, so its unit tests and its
//! integration tests share no module path. This crate is the one home both
//! read the builder from.

use crate::bytes::{put_u16, put_u32};

const CFB_SECTOR: usize = 512;
const CFB_FREE: u32 = 0xffff_ffff;
const CFB_END: u32 = 0xffff_fffe;
const CFB_FAT: u32 = 0xffff_fffd;

/// One FAT sector, eight data sectors of 0x5a, and a directory with Root Entry
/// plus a 4096-byte Payload stream.
pub fn compound_fixture() -> Vec<u8> {
    // A compile-time length: header sector, directory, eight data sectors,
    // and the FAT sector.
    let mut file = [0_u8; CFB_SECTOR * 11].to_vec();
    file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 44, 1);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, 4096);
    put_u32(&mut file, 60, CFB_END);
    put_u32(&mut file, 68, CFB_END);
    for index in 0..109 {
        put_u32(&mut file, 76 + index * 4, CFB_FREE);
    }
    put_u32(&mut file, 76, 9);
    let directory = sector_mut(&mut file, 0);
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(0xff);
    }
    directory_entry(directory, 0, "Root Entry", 5, 1, CFB_END, 0);
    directory_entry(directory, 1, "Payload", 2, CFB_FREE, 1, 4096);
    for sector in 1..=8 {
        sector_mut(&mut file, sector).fill(0x5a);
    }
    let fat = sector_mut(&mut file, 9);
    fat.fill(0xff);
    put_u32(fat, 0, CFB_END);
    for sector in 1..8 {
        let next = u32::try_from(sector + 1).expect("the fixture spans eleven sectors");
        put_u32(fat, sector * 4, next);
    }
    put_u32(fat, 8 * 4, CFB_END);
    put_u32(fat, 9 * 4, CFB_FAT);
    file
}

/// Write one 128-byte directory entry.
fn directory_entry(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    child: u32,
    start: u32,
    size: u64,
) {
    let entry = &mut directory[index * 128..(index + 1) * 128];
    let units = name.encode_utf16().collect::<Vec<_>>();
    for (offset, unit) in units.iter().enumerate() {
        put_u16(entry, offset * 2, *unit);
    }
    let name_bytes = u16::try_from((units.len() + 1) * 2)
        .expect("a directory entry name field holds 32 UTF-16 units");
    put_u16(entry, 64, name_bytes);
    entry[66] = object_type;
    entry[67] = 1;
    put_u32(entry, 68, CFB_FREE);
    put_u32(entry, 72, CFB_FREE);
    put_u32(entry, 76, child);
    put_u32(entry, 116, start);
    entry[120..128].copy_from_slice(&size.to_le_bytes());
}

/// Borrow one sector body, counting the header as sector 0.
fn sector_mut(file: &mut [u8], sector: usize) -> &mut [u8] {
    let start = (sector + 1) * CFB_SECTOR;
    &mut file[start..start + CFB_SECTOR]
}
