// SPDX-License-Identifier: Apache-2.0
//! Optional Directory scalar admission from original fixed-column fields.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput};

use super::super::{directory_integer, GlobalTable};

pub(super) fn admit(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    name: &'static str,
    number: u8,
    global: GlobalTable,
    output: &mut ParserPartOutput,
) -> Result<Option<i64>, CodecError> {
    ctx.decode_parser_part(
        output,
        source,
        super::part_spec("iges.directory_field", Vec::new())?,
        || {
            let bytes = source
                .read_slice(source.start()..source.end())
                .ok_or_else(|| CodecError::malformed("Directory field is unreadable"))?;
            let bytes = <[u8; 8]>::try_from(bytes).map_err(|_| {
                CodecError::malformed("Directory field width differs from eight columns")
            })?;
            directory_integer(bytes, name, number, global).map_err(CodecError::from)
        },
    )
}

#[cfg(test)]
mod tests;
