// SPDX-License-Identifier: Apache-2.0
use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub fn charged_name_probe(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<bool, CodecError> {
    ArchiveSnapshot::probe_readable_names(ctx, bytes, |name| {
        ctx.ends_with(name, "schema.xml", "ZIP schema name")
    })
}

pub fn uncharged_name_probe(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    suffix: &str,
) -> Result<bool, CodecError> {
    ArchiveSnapshot::probe_readable_names(ctx, bytes, |name| {
        Ok(name.ends_with(suffix)) // finding: uncharged_decode_work
    })
}
