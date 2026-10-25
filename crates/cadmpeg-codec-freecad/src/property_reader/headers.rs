// SPDX-License-Identifier: Apache-2.0
//! Selected property headers can contribute without an admitted value row.

use super::PropertyReader;
use crate::native::PropertyRecord;
use cadmpeg_core::CodecError;

impl PropertyReader<'_, '_> {
    pub(crate) fn feature_header(
        &self,
        property: &PropertyRecord,
    ) -> Result<Option<(String, String)>, CodecError> {
        let Some(ranges) = self.property_sources else {
            return Ok(None);
        };
        let owner = self.feature_owner.borrow();
        let Some(owner) = owner.as_ref() else {
            return Ok(None);
        };
        let source = self
            .xml_source
            .ok_or_else(|| CodecError::malformed("operation has no original XML supplier"))?;
        let range = ranges
            .get(&property.id)
            .ok_or_else(|| CodecError::malformed("operation property has no original range"))?
            .record_range();
        let index = source
            .elements()
            .iter()
            .position(|element| {
                element.extent().start() == source.view().start() + range.start
                    && element.extent().end() == source.view().start() + range.end
            })
            .ok_or_else(|| CodecError::malformed("operation property has no original element"))?;
        self.context
            .record_fragment(owner, source.extent(index)?, || {
                let name = source
                    .attribute(self.context, index, "name")?
                    .ok_or_else(|| CodecError::malformed("operation property has no name"))?;
                let kind = source
                    .attribute(self.context, index, "type")?
                    .ok_or_else(|| CodecError::malformed("operation property has no runtime"))?;
                if name != property.name || kind != property.type_name {
                    return Err(CodecError::malformed("operation property header differs"));
                }
                Ok((name, kind))
            })
            .map(Some)
    }
}
