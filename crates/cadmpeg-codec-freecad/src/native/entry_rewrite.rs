// SPDX-License-Identifier: Apache-2.0
//! Typed entry identities.

use super::EntryRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

impl RewriteIdentities for EntryRecord {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for target in &self.referenced_by.0 {
            visitor(target)?;
        }
        Ok(())
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = self.id.rewrite_identities(ctx, map)?;
        for target in &mut self.referenced_by.0 {
            *target = map.identity(target)?;
        }
        Ok(self)
    }

    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let fields = value
            .as_object_mut()
            .ok_or_else(|| CodecError::malformed("entry native wire is not an object"))?;
        let identity = fields
            .get_mut("id")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| CodecError::malformed("entry native wire has no identity"))?;
        fields.insert("id".into(), map.identity(&identity)?.into());
        if let Some(references) = fields.get_mut("referenced_by") {
            let references = references
                .as_array_mut()
                .ok_or_else(|| CodecError::malformed("entry native references are not a list"))?;
            for target in references {
                let original = target.as_str().ok_or_else(|| {
                    CodecError::malformed("entry native reference is not an identity")
                })?;
                *target = map.identity(original)?.into();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_owners_are_lists_while_names_remain_literal_content() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let owner = "test:native:property#before";
        let entry = EntryRecord::new(
            "test:native:entry#before".into(),
            owner.into(),
            cadmpeg_core::container::ContainerRole::Auxiliary,
            vec![owner.into(), "test:native:property#kept".into()],
        )
        .unwrap();
        let mut map = IdentityMap::new(|id: &str| Ok(id.replace("#", "#changed-")));
        let rewritten = entry.rewrite_identities(&ctx, &mut map).unwrap();
        assert_eq!(rewritten.id(), "test:native:entry#changed-before");
        assert_eq!(
            rewritten.referenced_by(),
            [
                "test:native:property#changed-before",
                "test:native:property#changed-kept"
            ]
        );
        assert_eq!(rewritten.name(), owner);
    }
}
