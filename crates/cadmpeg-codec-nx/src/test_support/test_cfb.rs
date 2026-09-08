// SPDX-License-Identifier: Apache-2.0
//! Legacy CFB container byte builders for the crate's `#[cfg(test)]` suites.
//!
//! An NX legacy part is a Compound File Binary image whose `UG_PART` storage
//! holds the `UGII` payload. These builders write that image directly: sector
//! table, directory entries, and an uncompressed payload. Nothing here goes
//! through `zlib_compress`, so a golden built from one of these pins no
//! compressor output.
#![allow(clippy::unwrap_used)]

pub(crate) fn legacy_cfb_with_ug_part() -> Vec<u8> {
    const SECTOR: usize = 512;
    const END: u32 = 0xffff_fffe;
    const FREE: u32 = 0xffff_ffff;
    const FAT: u32 = 0xffff_fffd;
    const STREAM_SECTORS: usize = 10;
    const FAT_SECTOR: usize = 11;
    let mut file = vec![0; SECTOR * (1 + FAT_SECTOR + 1)];
    file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 40, 0);
    put_u32(&mut file, 44, 1);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, 4096);
    put_u32(&mut file, 60, END);
    put_u32(&mut file, 64, 0);
    put_u32(&mut file, 68, END);
    put_u32(&mut file, 72, 0);
    for index in 0..109 {
        put_u32(&mut file, 76 + index * 4, FREE);
    }
    put_u32(&mut file, 76, FAT_SECTOR as u32);

    let directory = sector_mut(&mut file, 0);
    for index in 0..4 {
        put_u32(directory, index * 128 + 68, FREE);
        put_u32(directory, index * 128 + 72, FREE);
        put_u32(directory, index * 128 + 76, FREE);
    }
    cfb_directory_entry(directory, 0, "Root Entry", 5, END, 1, 0);
    cfb_directory_entry(directory, 1, "UG_PART", 1, END, 2, 0);
    cfb_directory_entry(
        directory,
        2,
        "UG_PART",
        2,
        1,
        END,
        (STREAM_SECTORS * SECTOR) as u64,
    );

    let payload = sector_mut(&mut file, 1);
    payload[..8].copy_from_slice(b"\x0d\x01UGII  ");
    payload[8] = 0x32;
    let description = b": TRANSMIT FILE (partition) created by test";
    let mut at = 9;
    payload[at..at + 2].copy_from_slice(b"PS");
    at += 2;
    payload[at..at + 4].copy_from_slice(&(description.len() as u32).to_be_bytes());
    at += 4;
    payload[at..at + description.len()].copy_from_slice(description);

    let fat = sector_mut(&mut file, FAT_SECTOR);
    fat.fill(0xff);
    put_u32(fat, 0, END);
    for sector in 1..=STREAM_SECTORS {
        put_u32(
            fat,
            sector * 4,
            if sector == STREAM_SECTORS {
                END
            } else {
                (sector + 1) as u32
            },
        );
    }
    put_u32(fat, FAT_SECTOR * 4, FAT);
    file
}

pub(crate) fn legacy_cfb_with_partial_ug_part() -> Vec<u8> {
    const SECTOR: usize = 512;
    const END: u32 = 0xffff_fffe;
    const STREAM_LAST: usize = 10;
    const FAT_SECTOR: usize = 11;
    let mut file = legacy_cfb_with_ug_part();
    file.resize(file.len() + SECTOR, 0);
    let directory_entry = &mut file[SECTOR + 2 * 128..SECTOR + 3 * 128];
    directory_entry[120..128].copy_from_slice(&5133_u64.to_le_bytes());
    let unallocated_entry = &mut file[SECTOR + 3 * 128..SECTOR + 4 * 128];
    unallocated_entry[..64].fill(0xa5);
    unallocated_entry[68..80].fill(0xa5);
    unallocated_entry[120..128].fill(0xa5);
    let fat = sector_mut(&mut file, FAT_SECTOR);
    put_u32(fat, STREAM_LAST * 4, 12);
    put_u32(fat, 12 * 4, END);
    file.truncate(SECTOR * 13 + 13);
    file
}

pub(crate) fn legacy_cfb_with_two_streams() -> Vec<u8> {
    const SECTOR: usize = 512;
    const END: u32 = 0xffff_fffe;
    const FAT: u32 = 0xffff_fffd;
    const EXTRA_FIRST_SECTOR: usize = 12;
    const EXTRA_SECTORS: usize = 8;
    const FAT_SECTOR: usize = 20;
    let mut file = legacy_cfb_with_ug_part();
    file.resize(SECTOR * (1 + FAT_SECTOR + 1), 0);
    put_u32(&mut file, 76, FAT_SECTOR as u32);
    sector_mut(&mut file, 11).fill(0xff);

    let directory = sector_mut(&mut file, 0);
    cfb_directory_entry(
        directory,
        3,
        "Extra",
        2,
        EXTRA_FIRST_SECTOR as u32,
        END,
        (EXTRA_SECTORS * SECTOR) as u64,
    );
    put_u32(directory, 128 + 76, 3);
    put_u32(directory, 3 * 128 + 72, 2);

    let extra = sector_mut(&mut file, EXTRA_FIRST_SECTOR);
    extra.fill(0xa5);
    let marker = b"SECOND STREAM PAYLOAD";
    extra[..marker.len()].copy_from_slice(marker);

    let fat = sector_mut(&mut file, FAT_SECTOR);
    fat.fill(0xff);
    put_u32(fat, 0, END);
    for sector in 1..=10 {
        put_u32(
            fat,
            sector * 4,
            if sector == 10 {
                END
            } else {
                (sector + 1) as u32
            },
        );
    }
    for sector in EXTRA_FIRST_SECTOR..EXTRA_FIRST_SECTOR + EXTRA_SECTORS {
        put_u32(
            fat,
            sector * 4,
            if sector + 1 == EXTRA_FIRST_SECTOR + EXTRA_SECTORS {
                END
            } else {
                (sector + 1) as u32
            },
        );
    }
    put_u32(fat, FAT_SECTOR * 4, FAT);
    file
}

pub(crate) fn cfb_directory_entry(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    start_sector: u32,
    child: u32,
    size: u64,
) {
    let entry = &mut directory[index * 128..(index + 1) * 128];
    for (offset, unit) in name.encode_utf16().enumerate() {
        put_u16(entry, offset * 2, unit);
    }
    put_u16(entry, 64, ((name.encode_utf16().count() + 1) * 2) as u16);
    entry[66] = object_type;
    entry[67] = 1;
    put_u32(entry, 68, 0xffff_ffff);
    put_u32(entry, 72, 0xffff_ffff);
    put_u32(entry, 76, child);
    put_u32(entry, 116, start_sector);
    entry[120..128].copy_from_slice(&size.to_le_bytes());
}

pub(crate) fn sector_mut(file: &mut [u8], sector: usize) -> &mut [u8] {
    let start = (sector + 1) * 512;
    &mut file[start..start + 512]
}

pub(crate) fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
