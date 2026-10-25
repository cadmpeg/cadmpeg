// SPDX-License-Identifier: Apache-2.0
//! One original attribute supplies an independent derived producer.
use super::{check_allocation, decode_text, valid_name};
use cadmpeg_core::decode::{DecodeContext, SourceExtentProof, View};
use cadmpeg_core::CodecError;
pub(crate) struct XmlAttributeSource<'source> {
    pub(super) header: View<'source>,
    pub(super) name: View<'source>,
    pub(super) value: View<'source>,
    pub(super) proof: SourceExtentProof,
}
impl<'source> XmlAttributeSource<'source> {
    pub(crate) fn primary(&self) -> View<'source> {
        self.header
    }
    pub(crate) fn read(&self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        ctx.require_source_authority(&self.proof)?;
        ctx.require_source_ancestry(&self.proof, self.name)?;
        ctx.require_source_ancestry(&self.proof, self.value)?;
        let name = self
            .name
            .read_slice(self.name.start()..self.name.end())
            .ok_or_else(|| CodecError::malformed("attribute supplier name escapes source"))?;
        let name = std::str::from_utf8(name)
            .map_err(|_| CodecError::malformed("attribute supplier name is not UTF-8"))?;
        if !valid_name(name) {
            return Err(CodecError::malformed("attribute supplier name is invalid"));
        }
        let bytes = self
            .value
            .read_slice(self.value.start()..self.value.end())
            .ok_or_else(|| CodecError::malformed("attribute supplier value escapes source"))?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| CodecError::malformed("attribute supplier value is not UTF-8"))?;
        check_allocation(ctx, value.len())?;
        decode_text(value, true)
    }
}
