// SPDX-License-Identifier: Apache-2.0
//! Document identity is distinct from literal metadata.

use super::DocumentFacts;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

impl RewriteIdentities for DocumentFacts {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        Ok(())
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.id = map.identity(&self.id)?;
        Ok(self)
    }

    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let id = value
            .get_mut("id")
            .ok_or_else(|| CodecError::malformed("document native wire has no identity"))?;
        let original = id
            .as_str()
            .ok_or_else(|| CodecError::malformed("document native identity is not text"))?;
        *id = map.identity(original)?.into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_metadata_strings_are_literal_while_its_identity_is_rewritten() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let literal = "test:native:document#literal";
        let facts = DocumentFacts {
            id: "test:native:document#root".into(),
            file_version: literal.to_owned().into(),
            program_version: Some(literal.into()),
            root_name: literal.into(),
            object_count: 1,
            domains: vec![literal.into()],
        };
        let mut references = Vec::new();
        facts
            .visit_identity_references(&ctx, &mut |id| {
                references.push(id.to_owned());
                Ok(())
            })
            .unwrap();
        assert!(references.is_empty());
        let mut wire = serde_json::to_value(&facts).unwrap();
        let mut map = IdentityMap::new(|id: &str| Ok(id.replace('#', "#changed-")));
        let changed = facts.rewrite_identities(&ctx, &mut map).unwrap();
        DocumentFacts::rewrite_native_value(&ctx, &mut wire, &mut map).unwrap();
        assert_eq!(serde_json::to_value(&changed).unwrap(), wire);
        assert_eq!(changed.id, "test:native:document#changed-root");
        assert_eq!(changed.file_version.as_str(), literal);
        assert_eq!(changed.program_version.as_deref(), Some(literal));
        assert_eq!(changed.root_name, literal);
        assert_eq!(changed.domains, [literal]);
    }
}
