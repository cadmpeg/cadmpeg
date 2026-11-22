// SPDX-License-Identifier: Apache-2.0
//! Synthetic eDrawings packages for crate tests.
//!
//! [`emodel`] writes an `eModel` member from the layout in
//! docs/formats/edrawings.md section 2: the comment line, the `I` and `B`
//! wrapper records, the `Z` tag, a zlib stream, and the one-byte tail.
//! [`package`] wraps it in a ZIP archive beside the other members. The zlib
//! stream holds stored blocks only, so the bytes depend on no compressor.

pub(crate) mod hsf;
pub(crate) mod models;
pub(crate) mod solidworks;

use cadmpeg_test_support::zip_fixture::{archive, RawEntry, DEFLATE};

/// The stream version of the re-saved files.
pub(crate) const OBSERVED_VERSION: &str = "19.10";
/// The stream version of the files that `SolidWorks` wrote.
pub(crate) const SOLIDWORKS_VERSION: &str = "18.00";
/// Bounds of the synthetic model: minimum corner, then maximum corner.
pub(crate) const DEFAULT_BOUNDS: [f32; 6] = [-0.5, 0.0, -0.25, 0.5, 1.0, 0.25];
/// Largest payload of one stored DEFLATE block.
const STORED_BLOCK_MAX: usize = 65_535;
/// Modulus of the Adler-32 sums.
const ADLER_MODULUS: u32 = 65_521;

/// The zlib stream header the observed files carry, with the default
/// compression level flag.
const ZLIB_HEADER: [u8; 2] = [0x78, 0x01];

/// Adler-32 of `bytes` (RFC 1950 section 8.2).
fn adler32(bytes: &[u8]) -> u32 {
    let (mut low, mut high) = (1_u32, 0_u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % ADLER_MODULUS;
        high = (high + low) % ADLER_MODULUS;
    }
    (high << 16) | low
}

/// `plain` as one zlib stream made of stored DEFLATE blocks.
pub(crate) fn zlib_stored(plain: &[u8]) -> Vec<u8> {
    let mut stream = ZLIB_HEADER.to_vec();
    let mut blocks = plain.chunks(STORED_BLOCK_MAX).peekable();
    if blocks.peek().is_none() {
        stream.extend_from_slice(&[0x01, 0, 0, 0xff, 0xff]);
    }
    while let Some(block) = blocks.next() {
        let len = u16::try_from(block.len()).unwrap();
        stream.push(u8::from(blocks.peek().is_none()));
        stream.extend_from_slice(&len.to_le_bytes());
        stream.extend_from_slice(&(!len).to_le_bytes());
        stream.extend_from_slice(block);
    }
    stream.extend_from_slice(&adler32(plain).to_be_bytes());
    stream
}

/// The `I` record of the observed files: the tag and the `u32` 4.
pub(crate) fn info_record() -> Vec<u8> {
    let mut record = vec![b'I'];
    record.extend_from_slice(&4_u32.to_le_bytes());
    record
}

/// A `B` record: the tag, a zero flag byte, and `bounds` as six `f32`.
pub(crate) fn bounds_record(bounds: [f32; 6]) -> Vec<u8> {
    let mut record = vec![b'B', 0];
    for value in bounds {
        record.extend_from_slice(&value.to_le_bytes());
    }
    record
}

/// An `eModel` member from its parts: the comment line for `version`, the
/// wrapper records `records`, the `Z` tag, the zlib stream bytes `zlib`, and
/// the `tail`.
pub(crate) fn emodel_with(version: &str, records: &[u8], zlib: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut member = format!(";; HSF V{version} \n").into_bytes();
    member.extend_from_slice(records);
    member.push(b'Z');
    member.extend_from_slice(zlib);
    member.extend_from_slice(tail);
    member
}

/// An `eModel` member in the observed layout whose model stream is `body`.
pub(crate) fn emodel(body: &[u8]) -> Vec<u8> {
    emodel_of(OBSERVED_VERSION, body)
}

/// An `eModel` member that declares `version` and whose model stream is `body`.
pub(crate) fn emodel_of(version: &str, body: &[u8]) -> Vec<u8> {
    let mut records = info_record();
    records.extend(bounds_record(DEFAULT_BOUNDS));
    emodel_with(version, &records, &zlib_stored(body), &[0])
}

/// `plain` as one final stored DEFLATE block (RFC 1951 section 3.2.4).
pub(crate) fn deflate_stored_block(plain: &[u8]) -> Vec<u8> {
    let len = u16::try_from(plain.len()).unwrap();
    let mut block = vec![0x01];
    block.extend_from_slice(&len.to_le_bytes());
    block.extend_from_slice(&(!len).to_le_bytes());
    block.extend_from_slice(plain);
    block
}

/// A part package: `eModel` first, then the members of an `.eprt`.
pub(crate) fn part_package(body: &[u8]) -> Vec<u8> {
    package_of(&emodel(body), &[])
}

/// An assembly package: the part members plus `materials.xml` and
/// `FileProperties.xml`.
pub(crate) fn assembly_package(body: &[u8]) -> Vec<u8> {
    package_of(
        &emodel(body),
        &[
            ("materials.xml", b"<Materials/>"),
            ("FileProperties.xml", b"<FileProperties/>"),
        ],
    )
}

/// An assembly package whose `eModel` member declares `version`.
pub(crate) fn assembly_package_of(version: &str, body: &[u8]) -> Vec<u8> {
    package_of(
        &emodel_of(version, body),
        &[
            ("materials.xml", b"<Materials/>"),
            ("FileProperties.xml", b"<FileProperties/>"),
        ],
    )
}

/// A stored-member package: `eModel`, `preview.jpg`, `OLE`, `scene.xml`, then
/// the `extra` members.
pub(crate) fn package_of(emodel: &[u8], extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries = vec![
        RawEntry::stored("eModel", emodel),
        RawEntry::stored("preview.jpg", b"\xff\xd8synthetic preview"),
        RawEntry::stored("OLE", &[0_u8; 12]),
        RawEntry::stored("scene.xml", b"<SceneData SceneDataVersion=\"1.0\"/>"),
    ];
    entries.extend(
        extra
            .iter()
            .map(|(name, bytes)| RawEntry::stored(name, bytes)),
    );
    archive(&entries)
}

/// A package whose `eModel` member is DEFLATE-compressed (stored block) in
/// the ZIP, as in the observed files.
pub(crate) fn deflated_part_package(body: &[u8]) -> Vec<u8> {
    let member = emodel(body);
    archive(&[
        RawEntry::compressed("eModel", DEFLATE, &deflate_stored_block(&member), &member),
        RawEntry::stored("scene.xml", b"<SceneData/>"),
    ])
}

/// A ZIP archive of stored members, in the given order.
pub(crate) fn zip_of(members: &[(&str, &[u8])]) -> Vec<u8> {
    archive(
        &members
            .iter()
            .map(|(name, bytes)| RawEntry::stored(name, bytes))
            .collect::<Vec<_>>(),
    )
}
