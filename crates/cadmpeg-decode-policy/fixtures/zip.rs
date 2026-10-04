// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;

pub fn access(
    _ctx: &DecodeContext,
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    index: usize,
) {
    let _name = archive.name_for_index(index);
    {
        let _raw = archive.by_index_raw(index);
    }
    let _decoded = archive.by_index(index); // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn index(_ctx: &DecodeContext, bytes: &[u8]) {
    let _archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)); // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn opaque_reader<R: std::io::Read + std::io::Seek>(
    _ctx: &DecodeContext,
    archive: &mut zip::ZipArchive<R>,
    index: usize,
) {
    let _name = archive.name_for_index(index);
    let _raw = archive.by_index_raw(index); // finding: unproven_decode_charge
}
