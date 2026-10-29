// SPDX-License-Identifier: Apache-2.0
//! Original archive access for source processing, never semantic synthesis.

use cadmpeg_core::CodecError;
use cadmpeg_ir::SourceFidelity;

/// Original physical input; expanded member copies are not retained.
pub(crate) fn image(fidelity: &SourceFidelity) -> Result<&[u8], CodecError> {
    let record = fidelity
        .retained_record(crate::fidelity::SOURCE_IMAGE)
        .ok_or_else(|| CodecError::malformed("original FCStd archive is unavailable"))?;
    let bytes = record
        .data()
        .ok_or_else(|| CodecError::malformed("original FCStd archive has no bytes"))?;
    if record.byte_len() != cadmpeg_core::decode::u64_from_index(bytes.len())
        || record.sha256() != cadmpeg_ir::hash::sha256_hex(bytes)
    {
        return Err(CodecError::malformed(
            "original FCStd archive evidence disagrees",
        ));
    }
    Ok(bytes)
}
