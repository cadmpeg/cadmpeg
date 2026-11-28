// SPDX-License-Identifier: Apache-2.0
//! Encode source-less F3D archives and apply supported edits to retained source
//! archives.

pub(crate) mod generate;
pub(crate) mod patch;
pub(crate) mod primitives;
pub(crate) mod saved;
pub(crate) mod target;

/// Every text field this writer fits to its width (cadir-v11.md §3.4).
#[cfg(test)]
pub(crate) const CAPACITY_FIELDS: &[cadmpeg_ir::codec::write::capacity::TextCapacity] = &[
    generate::configuration_root::TABLE_ID,
    generate::configuration_root::ROW_ID,
];

#[cfg(test)]
mod tests;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, DecodeFailure, DecodeResult};
use cadmpeg_ir::report::strictness::Strictness;

pub(crate) struct RetainedArchive<'a> {
    pub(crate) scan: crate::container::ContainerScan<'a>,
    pub(crate) decoded: DecodeResult,
}

impl<'a> RetainedArchive<'a> {
    pub(crate) fn read(ctx: &DecodeContext<'a>, bytes: &'a [u8]) -> Result<Self, CodecError> {
        let root = ctx.open_input(bytes, "F3D retained source archive")?;
        let scan = crate::container::scan_for_write(ctx, root)?;
        let decoded = match ctx.run_part(|| crate::decode::decode_scanned(ctx, &scan))? {
            Ok(decoded) => decoded,
            Err(error @ CodecError::ResourceLimit(_)) => {
                return Err(CodecError::NotImplemented(format!(
                    "retained F3D member: {error}"
                )));
            }
            Err(error) => return Err(error),
        };
        let decoded = DecodeResult::from_decoded_without_digests(
            ctx,
            decoded,
            <crate::F3dCodec as CodecBackend>::FORMAT,
            &Strictness::default(),
        )
        .map_err(|failure| match failure {
            DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format!("the retained archive decode refused: {other}")),
        })?;
        Ok(Self { scan, decoded })
    }
}
