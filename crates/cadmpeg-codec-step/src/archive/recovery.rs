// SPDX-License-Identifier: Apache-2.0
//! Explicit default recovery of noncanonical ancillary ZIP declarations.

use crate::{ids, loss::StepLossCode};
use cadmpeg_container::{ArchiveSnapshot, EntryRecord, ZipCompression};
use cadmpeg_core::{
    decode::{DecodeContext, View},
    CodecError,
};
use cadmpeg_ir::unknown::UnknownRecord;

fn noncanonical(entry: &EntryRecord) -> bool {
    entry.uses_utf8_name_encoding()
        || entry.encrypted
        || entry.data_start.is_none()
        || !matches!(
            entry.compression,
            ZipCompression::Stored | ZipCompression::Deflate
        )
}

pub(crate) fn losses(
    archive: &ArchiveSnapshot<'_>,
) -> std::vec::Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for entry in archive.entries() {
        if noncanonical(entry) {
            let message = format!("ZIP member {:?} has noncanonical filename, compression, encryption or local framing; retained its declaration while decoding the required root", entry.name);
            losses.push(StepLossCode::ContainerMemberNoncanonical.note(message));
        }
    }
    losses
}

pub(crate) fn retain(
    ctx: &DecodeContext<'_>,
    archive: &ArchiveSnapshot<'_>,
    root: View<'_>,
    decoded: &mut cadmpeg_ir::codec::Decoded,
) -> Result<(), CodecError> {
    let mut records = Vec::new();
    for entry in archive.entries().iter().filter(|entry| noncanonical(entry)) {
        let range = entry.declaration_range();
        let mut view = root;
        view.seek(
            usize::try_from(range.start).map_err(|_| {
                CodecError::malformed("ZIP declaration offset exceeds address space")
            })?,
        )
        .ok_or_else(|| CodecError::malformed("ZIP declaration start is outside source"))?;
        let bytes = view
            .take(usize::try_from(range.end - range.start).map_err(|_| {
                CodecError::malformed("ZIP declaration length exceeds address space")
            })?)
            .ok_or_else(|| CodecError::malformed("ZIP declaration end is outside source"))?;
        let id = ids::zip_declaration(range.start);

        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "STEP ZIP retained declaration",
        )?;
        let bytes = bytes.to_vec();
        records.push(UnknownRecord::retained(id, range.start, bytes, Vec::new()));
    }
    decoded
        .source_fidelity
        .attach_native_unknown_records(&mut decoded.ir, "step", records, ctx)
}
