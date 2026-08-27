// SPDX-License-Identifier: Apache-2.0

use std::io::Write as _;

use flate2::write::ZlibEncoder;
use flate2::Compression;

const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const FREE_SECTOR: u32 = 0xffff_ffff;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const NO_STREAM: u32 = 0xffff_ffff;
const SECTOR_SIZE: usize = 512;

pub(crate) fn fixture(inventor: bool) -> Vec<u8> {
    let mut file = vec![0u8; SECTOR_SIZE * 3];
    file[..8].copy_from_slice(&MAGIC);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 44, 1);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, 4096);
    put_u32(&mut file, 60, END_OF_CHAIN);
    put_u32(&mut file, 68, END_OF_CHAIN);
    for index in 0..109 {
        put_u32(&mut file, 76 + index * 4, FREE_SECTOR);
    }
    put_u32(&mut file, 76, 1);
    let directory = sector_mut(&mut file, 0);
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(0xff);
    }
    directory_entry(directory, 0, "Root Entry", 5, 1, END_OF_CHAIN, 0);
    let storage_name = if inventor {
        "RSeStorage"
    } else {
        "OtherStorage"
    };
    directory_entry(directory, 1, storage_name, 1, 2, END_OF_CHAIN, 0);
    directory_entry(directory, 2, "RSeSegInfo", 2, NO_STREAM, END_OF_CHAIN, 0);
    let fat = sector_mut(&mut file, 1);
    fat.fill(0xff);
    put_u32(fat, 0, END_OF_CHAIN);
    put_u32(fat, 4, FAT_SECTOR);
    file
}

fn directory_entry(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    child: u32,
    start_sector: u32,
    size: u64,
) {
    let entry = &mut directory[index * 128..(index + 1) * 128];
    let encoded = name.encode_utf16().collect::<Vec<_>>();
    for (offset, unit) in encoded.iter().enumerate() {
        put_u16(entry, offset * 2, *unit);
    }
    put_u16(entry, 64, ((encoded.len() + 1) * 2) as u16);
    entry[66] = object_type;
    entry[67] = 1;
    put_u32(entry, 68, NO_STREAM);
    put_u32(entry, 72, NO_STREAM);
    put_u32(entry, 76, child);
    put_u32(entry, 116, start_sector);
    entry[120..128].copy_from_slice(&size.to_le_bytes());
}

fn sector_mut(file: &mut [u8], id: usize) -> &mut [u8] {
    let start = SECTOR_SIZE * (id + 1);
    &mut file[start..start + SECTOR_SIZE]
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// The `RSe` version declarations a synthetic primary envelope carries.
///
/// [`Default`] is the pair this codec implements. A test that needs an
/// unimplemented declaration changes one field, so the rest of the document
/// stays byte-identical and the classification is the only difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnvelopeDeclarations {
    /// The `RSeDb` schema word.
    pub(crate) schema: u32,
    /// The `RSe` metadata stream marker.
    pub(crate) meta_marker: &'static str,
    /// The `RSe` metadata stream version word.
    pub(crate) meta_version: u16,
}

impl Default for EnvelopeDeclarations {
    fn default() -> Self {
        Self {
            schema: 31,
            meta_marker: "RSe Meta Stream Version 8",
            meta_version: 8,
        }
    }
}

pub(crate) fn primary_envelope_fixture() -> Vec<u8> {
    primary_envelope_fixture_with(EnvelopeDeclarations::default())
}

pub(crate) fn primary_envelope_fixture_with(declarations: EnvelopeDeclarations) -> Vec<u8> {
    const ROOT: usize = 0;
    const RSE_STORAGE: usize = 1;
    const V1: usize = 2;
    const SEGMENT_INFO: usize = 3;
    const DATABASE: usize = 4;
    const REVISION_INFO: usize = 5;
    const BULK: usize = 6;
    const META: usize = 7;
    const DIRECTORY_SECTORS: usize = 3;
    const MINI_SECTOR_SIZE: usize = 64;
    const SECTOR_SIZE: usize = 512;
    const END_OF_CHAIN: u32 = 0xffff_fffe;
    const FREE_SECTOR: u32 = 0xffff_ffff;
    const FAT_SECTOR: u32 = 0xffff_fffd;

    let carrier = kernel_carrier_fixture();
    let meta = meta_stream_fixture(carrier.len(), declarations);
    let bulk = bulk_stream_fixture(&carrier);
    let database = database_fixture(declarations.schema);
    let registry = registry_fixture();
    let revisions = revision_fixture();
    let streams = [
        (SEGMENT_INFO, "RSeSegInfo", registry),
        (DATABASE, "RSeDb", database),
        (REVISION_INFO, "RSeDbRevisionInfo", revisions),
        (BULK, "Bseg", bulk),
        (META, "Mseg", meta),
    ];

    let mut mini_stream = Vec::new();
    let mut mini_fat = vec![FREE_SECTOR; SECTOR_SIZE / 4];
    let mut allocations = Vec::new();
    for (_, _, bytes) in &streams {
        let start = u32::try_from(mini_stream.len() / MINI_SECTOR_SIZE)
            .expect("synthetic mini-sector index fits u32");
        let count = bytes.len().div_ceil(MINI_SECTOR_SIZE);
        for ordinal in 0..count {
            let begin = ordinal * MINI_SECTOR_SIZE;
            let end = (begin + MINI_SECTOR_SIZE).min(bytes.len());
            mini_stream.extend_from_slice(&bytes[begin..end]);
            mini_stream.resize(mini_stream.len() + MINI_SECTOR_SIZE - (end - begin), 0);
            let id = start + ordinal as u32;
            mini_fat[id as usize] = if ordinal + 1 == count {
                END_OF_CHAIN
            } else {
                id + 1
            };
        }
        allocations.push((start, bytes.len() as u64));
    }
    let root_mini_sectors = mini_stream.len().div_ceil(SECTOR_SIZE);
    mini_stream.resize(root_mini_sectors * SECTOR_SIZE, 0);

    let root_mini_start = DIRECTORY_SECTORS as u32;
    let mini_fat_sector = root_mini_start + root_mini_sectors as u32;
    let fat_sector = mini_fat_sector + 1;
    let sector_count = fat_sector as usize + 1;
    let mut file = vec![0_u8; (sector_count + 1) * SECTOR_SIZE];
    file[..8].copy_from_slice(&MAGIC);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 40, 0);
    put_u32(&mut file, 44, 1);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, 4096);
    put_u32(&mut file, 60, mini_fat_sector);
    put_u32(&mut file, 64, 1);
    put_u32(&mut file, 68, END_OF_CHAIN);
    put_u32(&mut file, 72, 0);
    for index in 0..109 {
        put_u32(&mut file, 76 + index * 4, FREE_SECTOR);
    }
    put_u32(&mut file, 76, fat_sector);

    let mut directory = vec![0_u8; DIRECTORY_SECTORS * SECTOR_SIZE];
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(0xff);
    }
    directory_node(
        &mut directory,
        ROOT,
        "Root Entry",
        5,
        NO_STREAM,
        NO_STREAM,
        RSE_STORAGE as u32,
        root_mini_start,
        mini_stream.len() as u64,
    );
    directory_node(
        &mut directory,
        RSE_STORAGE,
        "RSeStorage",
        1,
        NO_STREAM,
        NO_STREAM,
        V1 as u32,
        END_OF_CHAIN,
        0,
    );
    directory_node(
        &mut directory,
        V1,
        "V1",
        1,
        NO_STREAM,
        BULK as u32,
        DATABASE as u32,
        END_OF_CHAIN,
        0,
    );
    directory_node(
        &mut directory,
        SEGMENT_INFO,
        "RSeSegInfo",
        2,
        NO_STREAM,
        REVISION_INFO as u32,
        NO_STREAM,
        allocations[0].0,
        allocations[0].1,
    );
    directory_node(
        &mut directory,
        DATABASE,
        "RSeDb",
        2,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        allocations[1].0,
        allocations[1].1,
    );
    directory_node(
        &mut directory,
        REVISION_INFO,
        "RSeDbRevisionInfo",
        2,
        NO_STREAM,
        NO_STREAM,
        NO_STREAM,
        allocations[2].0,
        allocations[2].1,
    );
    directory_node(
        &mut directory,
        BULK,
        "Bseg",
        2,
        NO_STREAM,
        META as u32,
        NO_STREAM,
        allocations[3].0,
        allocations[3].1,
    );
    directory_node(
        &mut directory,
        META,
        "Mseg",
        2,
        NO_STREAM,
        SEGMENT_INFO as u32,
        NO_STREAM,
        allocations[4].0,
        allocations[4].1,
    );
    file[SECTOR_SIZE..SECTOR_SIZE + directory.len()].copy_from_slice(&directory);

    let mut fat = vec![FREE_SECTOR; SECTOR_SIZE / 4];
    for (sector, entry) in fat.iter_mut().enumerate().take(DIRECTORY_SECTORS) {
        *entry = if sector + 1 == DIRECTORY_SECTORS {
            END_OF_CHAIN
        } else {
            (sector + 1) as u32
        };
    }
    for sector in 0..root_mini_sectors {
        let id = root_mini_start as usize + sector;
        fat[id] = if sector + 1 == root_mini_sectors {
            END_OF_CHAIN
        } else {
            (id + 1) as u32
        };
    }
    fat[mini_fat_sector as usize] = END_OF_CHAIN;
    fat[fat_sector as usize] = FAT_SECTOR;
    for (ordinal, chunk) in mini_stream.chunks_exact(SECTOR_SIZE).enumerate() {
        let start = (root_mini_start as usize + ordinal + 1) * SECTOR_SIZE;
        file[start..start + SECTOR_SIZE].copy_from_slice(chunk);
    }
    let mini_fat_offset = (mini_fat_sector as usize + 1) * SECTOR_SIZE;
    for (index, value) in mini_fat.iter().enumerate() {
        put_u32(&mut file, mini_fat_offset + index * 4, *value);
    }
    let fat_offset = (fat_sector as usize + 1) * SECTOR_SIZE;
    for (index, value) in fat.iter().enumerate() {
        put_u32(&mut file, fat_offset + index * 4, *value);
    }
    file
}

fn database_fixture(schema: u32) -> Vec<u8> {
    let mut bytes = vec![0x21; 16];
    put_u32_vec(&mut bytes, schema);
    push_version(&mut bytes, 24);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    push_version(&mut bytes, 25);
    bytes.extend_from_slice(&18_u64.to_le_bytes());
    push_utf16_vec(&mut bytes, "synthetic primary document");
    bytes
}

fn registry_fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    put_u32_vec(&mut bytes, 1);
    push_utf16_vec(&mut bytes, "PmBRepSegment");
    bytes.extend_from_slice(&[0x5a; 16]);
    bytes.extend_from_slice(&[0x20; 16]);
    put_u32_vec(&mut bytes, 3);
    put_u32_vec(&mut bytes, 1);
    for value in 4..9 {
        put_u32_vec(&mut bytes, value);
    }
    put_u32_vec(&mut bytes, 9);
    push_utf16_vec(&mut bytes, "PmBrepSegmentType");
    put_u32_vec(&mut bytes, 10);
    put_u32_vec(&mut bytes, 11);
    push_version(&mut bytes, 18);
    put_u32_vec(&mut bytes, 12);
    bytes.extend_from_slice(&[0x20; 16]);
    bytes.extend_from_slice(&[0x30; 9]);
    bytes.extend_from_slice(&[0x5a; 16]);
    put_u32_vec(&mut bytes, 13);
    put_u32_vec(&mut bytes, 2);
    put_u32_vec(&mut bytes, 14);
    bytes.extend_from_slice(&(-1_i16).to_le_bytes());
    bytes.extend_from_slice(&2_i16.to_le_bytes());
    for value in 15_u16..21 {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&21_u16.to_le_bytes());
    bytes.extend_from_slice(&22_u16.to_le_bytes());
    bytes.extend_from_slice(&23_u16.to_le_bytes());
    put_u32_vec(&mut bytes, 1);
    bytes.extend_from_slice(&[0x61; 16]);
    put_u32_vec(&mut bytes, 0);
    bytes
}

fn revision_fixture() -> Vec<u8> {
    [3_u32, 0].into_iter().flat_map(u32::to_le_bytes).collect()
}

fn meta_stream_fixture(payload_len: usize, declarations: EnvelopeDeclarations) -> Vec<u8> {
    let body = meta_table_body(payload_len);
    let mut bytes = Vec::new();
    push_bytes_vec(&mut bytes, declarations.meta_marker.as_bytes());
    bytes.extend_from_slice(&declarations.meta_version.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0, 7, 0, 8, 0]);
    push_utf16_vec(&mut bytes, "PmBRepSegment");
    bytes.extend_from_slice(&[0x5a; 16]);
    bytes.extend_from_slice(
        &[5_u32, 6, 7]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    push_bytes_vec(&mut bytes, b"created");
    push_bytes_vec(&mut bytes, b"modified");
    bytes.push(1);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&body)
        .expect("write synthetic metadata body");
    bytes.extend_from_slice(&encoder.finish().expect("finish synthetic metadata body"));
    bytes
}

fn meta_table_body(payload_len: usize) -> Vec<u8> {
    let type_id = [
        0x5c, 0x59, 0x45, 0xf6, 0xd5, 0x11, 0x33, 0x13, 0x10, 0x00, 0x60, 0xa6, 0xbb, 0xa6, 0x47,
        0xb5,
    ];
    let mut body = Vec::new();
    for value in [3_u16, 0, 2, 1, 0, 4, 0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    push_counted_section(&mut body, &[0x8000_0000 | payload_len as u32], 4);
    push_counted_section(&mut body, &[], 10);
    push_counted_section(&mut body, &[], 28);
    put_u32_vec(&mut body, 1);
    body.extend_from_slice(&type_id);
    body.extend_from_slice(&1_u16.to_le_bytes());
    put_u32_vec(&mut body, 2);
    body.extend_from_slice(&3_u16.to_le_bytes());
    put_u32_vec(&mut body, 4);
    put_u32_vec(&mut body, 32);
    let payloads = [0_usize, 0, 0, 0, 0, 0, 0x48];
    let discriminators = [u32::MAX, 0, 0, 0, 0, 0, 18];
    put_u32_vec(&mut body, discriminators[0]);
    body.resize(body.len() + payloads[0], 0);
    for index in 1..payloads.len() {
        put_u32_vec(&mut body, payloads[index - 1] as u32 + 4);
        put_u32_vec(&mut body, discriminators[index]);
        body.resize(body.len() + payloads[index], 0);
    }
    body.extend_from_slice(&[0x77; 16]);
    body
}

fn bulk_stream_fixture(carrier: &[u8]) -> Vec<u8> {
    let mut expanded = Vec::new();
    expanded.extend_from_slice(&0_u32.to_le_bytes());
    expanded.extend_from_slice(carrier);
    expanded.extend_from_slice(&(carrier.len() as u32).to_le_bytes());
    expanded.extend_from_slice(&u32::MAX.to_le_bytes());
    let mut bytes = vec![0x3c; 16];
    bytes.extend_from_slice(&0x0104_u16.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&expanded)
        .expect("write synthetic bulk body");
    bytes.extend_from_slice(&encoder.finish().expect("finish synthetic bulk body"));
    bytes
}

fn kernel_carrier_fixture() -> Vec<u8> {
    let mut kernel = b"ASM BinaryFile4".to_vec();
    kernel.extend_from_slice(&700_u32.to_le_bytes());
    kernel.extend_from_slice(&[0_u8; 12]);
    for value in ["Inventor", "synthetic ASM", "2000-01-01"] {
        kernel.push(0x07);
        kernel.push(value.len() as u8);
        kernel.extend_from_slice(value.as_bytes());
    }
    for value in [1.0_f64, 1.0e-6, 1.0e-10] {
        kernel.push(0x06);
        kernel.extend_from_slice(&value.to_le_bytes());
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&kernel);
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&(-1_i32).to_le_bytes());
    bytes.extend_from_slice(&6_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes
}

#[allow(clippy::too_many_arguments)]
fn directory_node(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    left: u32,
    right: u32,
    child: u32,
    start_sector: u32,
    size: u64,
) {
    let entry = &mut directory[index * 128..(index + 1) * 128];
    let encoded = name.encode_utf16().collect::<Vec<_>>();
    for (offset, unit) in encoded.iter().enumerate() {
        put_u16(entry, offset * 2, *unit);
    }
    put_u16(entry, 64, ((encoded.len() + 1) * 2) as u16);
    entry[66] = object_type;
    entry[67] = 1;
    put_u32(entry, 68, left);
    put_u32(entry, 72, right);
    put_u32(entry, 76, child);
    put_u32(entry, 116, start_sector);
    entry[120..128].copy_from_slice(&size.to_le_bytes());
}

fn push_counted_section(bytes: &mut Vec<u8>, values: &[u32], item_size: usize) {
    put_u32_vec(bytes, values.len() as u32);
    for value in values {
        put_u32_vec(bytes, *value);
        bytes.resize(bytes.len() + item_size - 4, 0);
    }
    put_u32_vec(bytes, (4 + values.len() * item_size) as u32);
}

fn push_bytes_vec(bytes: &mut Vec<u8>, value: &[u8]) {
    put_u32_vec(bytes, value.len() as u32);
    bytes.extend_from_slice(value);
}

fn push_utf16_vec(bytes: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    put_u32_vec(bytes, units.len() as u32);
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

fn push_version(bytes: &mut Vec<u8>, major: u8) {
    bytes.extend_from_slice(&[1, 2, major, 4, 5, 6, 7, 8]);
}

fn put_u32_vec(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
