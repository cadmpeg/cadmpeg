// SPDX-License-Identifier: Apache-2.0
//! Kernel prologues of `Brep Data` records.

use cadmpeg_asm::kernel_header::BinaryHeader;
use cadmpeg_core::decode::{ParseError, View};
use cadmpeg_core::CodecError;
use cadmpeg_parasolid::OwnedSchemaToken;

/// First byte of a Parasolid kernel record.
const PARASOLID_LEAD: u8 = 0x41;

#[derive(Debug)]
pub(crate) struct PrologueError(CodecError);

impl From<ParseError> for PrologueError {
    fn from(error: ParseError) -> Self {
        Self(error.into())
    }
}

impl From<CodecError> for PrologueError {
    fn from(error: CodecError) -> Self {
        Self(error)
    }
}

impl std::fmt::Display for PrologueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

fn malformed(message: &str) -> PrologueError {
    PrologueError(CodecError::Malformed(message.into()))
}

/// Reads the schema token of a little-endian Parasolid transmit.
pub(crate) fn parasolid_schema(bytes: View<'_>) -> Result<OwnedSchemaToken, PrologueError> {
    let mut cursor = bytes;
    if cursor.req_u8()? != PARASOLID_LEAD {
        return Err(malformed("Parasolid lead byte is absent"));
    }
    let description = cursor.req_u16_le()?;
    cursor.req_take(usize::from(description))?;
    let schema = cursor.req_u32_le()?;
    let schema = cursor
        .counted(u64::from(schema), 1)
        .ok_or_else(|| malformed("Parasolid schema length exceeds the record"))?;
    let token = cursor.req_take(schema.get())?;
    cadmpeg_parasolid::find_schema_token(token)
        .filter(|found| found.value().len() == token.len())
        .map(Into::into)
        .ok_or_else(|| malformed("Parasolid schema declaration is invalid"))
}

/// Reads the header of an ACIS binary save stream.
pub(crate) fn acis_header(bytes: View<'_>) -> Result<BinaryHeader, PrologueError> {
    cadmpeg_asm::acis_header::parse(bytes.window())
        .ok_or_else(|| malformed("ACIS binary header is absent"))
}

#[cfg(test)]
mod tests;
