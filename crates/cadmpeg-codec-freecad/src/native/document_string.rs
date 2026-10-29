// SPDX-License-Identifier: Apache-2.0
//! Typed identity ownership for admitted document string properties.

use super::{PropertyBody, PropertyRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use serde::{Deserialize, Serialize};

pub(crate) struct DocumentString(PropertyRecord);

impl DocumentString {
    pub(crate) fn new(property: PropertyRecord) -> Result<Self, CodecError> {
        if property.type_name != "App::PropertyString"
            || !(matches!(&property.body, Some(PropertyBody::Persisted { links, side_entries, .. }) if links.is_empty() && side_entries.is_empty())
                || property.body.is_none())
        {
            return Err(CodecError::malformed(
                "document string has a different property grammar",
            ));
        }
        Ok(Self(property))
    }
}

impl Serialize for DocumentString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for DocumentString {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(PropertyRecord::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl RewriteIdentities for DocumentString {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.0.owner)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        self.0.id = map.identity(&self.0.id)?;
        self.0.owner = map.identity(&self.0.owner)?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        for field in ["id", "owner"] {
            let target = value
                .get_mut(field)
                .ok_or_else(|| CodecError::malformed("document property identity is absent"))?;
            let original = target
                .as_str()
                .ok_or_else(|| CodecError::malformed("document property identity is not text"))?;
            *target = map.identity(original)?.into();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_property_owner_is_required_and_value_strings_remain_literal() {
        let text = "<Document><Properties Count='1'><Property name='test:native:object#literal' type='App::PropertyString'><String value='test:native:object#literal'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
        let xml = roxmltree::Document::parse(text).unwrap();
        let graph =
            crate::persistence::parse_document(&xml, crate::persistence::Vocabulary::Objects)
                .unwrap();
        let original = graph.properties.into_iter().next().unwrap();
        let ctx = cadmpeg_test_support::service_decode_context();
        let property = DocumentString::new(original.clone()).unwrap();
        let mut map = IdentityMap::new(|id: &str| Ok(id.replace('#', "#changed-")));
        let rewritten = property.rewrite_identities(&ctx, &mut map).unwrap();
        assert_ne!(rewritten.0.id, original.id);
        assert_ne!(rewritten.0.owner, original.owner);
        assert_eq!(rewritten.0.name, original.name);
        assert_eq!(rewritten.0.values(), original.values());
        let mut wire = serde_json::to_value(&rewritten).unwrap();
        wire["type_name"] = serde_json::json!("App::PropertyLink");
        assert!(serde_json::from_value::<DocumentString>(wire).is_err());
    }
}
