// SPDX-License-Identifier: Apache-2.0
//! A dependency-free declaration keeps its name-derived identity.

use super::ObjectRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use serde::{Deserialize, Serialize};

pub(crate) struct ObjectDeclaration(ObjectRecord);
impl ObjectDeclaration {
    pub(crate) fn new(record: ObjectRecord) -> Result<Self, CodecError> {
        if !record.dependencies.is_empty() || record.dependency_allow_partial.is_some() {
            return Err(CodecError::malformed(
                "dependency-free object declaration has dependencies",
            ));
        }
        Ok(Self(record))
    }
}
impl Serialize for ObjectDeclaration {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ObjectDeclaration {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(ObjectRecord::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl RewriteIdentities for ObjectDeclaration {
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
        self.0.identity = super::object_identity::ObjectIdentity::try_new(
            map.identity(self.0.id())?,
            self.0.name().clone(),
        )
        .map_err(CodecError::Malformed)?;
        Ok(self)
    }
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, CodecError>>(
        _ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<(), CodecError> {
        let id = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CodecError::malformed("object declaration identity is absent"))?;
        let name = value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| CodecError::malformed("object declaration name is absent"))?;
        let identity =
            super::object_identity::ObjectIdentity::try_new(map.identity(id)?, name.to_owned())
                .map_err(CodecError::Malformed)?;
        value["id"] = identity.id().clone().into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declaration_owner_preserves_name_identity_and_literal_type_without_reference_guesses() {
        let xml = roxmltree::Document::parse("<Document><Objects Count='1'><Object name='A' type='test:native:object#literal'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>").unwrap();
        let graph =
            crate::persistence::parse_document(&xml, crate::persistence::Vocabulary::Objects)
                .unwrap();
        let original = graph.objects[0].clone();
        let ctx = cadmpeg_test_support::service_decode_context();
        let value = ObjectDeclaration::new(original.clone()).unwrap();
        let mut refs = Vec::new();
        value
            .visit_identity_references(&ctx, &mut |id| {
                refs.push(id.to_owned());
                Ok(())
            })
            .unwrap();
        assert!(refs.is_empty());
        let mut unchanged = IdentityMap::new(|id: &str| Ok(id.to_owned()));
        let kept = value.rewrite_identities(&ctx, &mut unchanged).unwrap();
        assert_eq!(
            serde_json::to_value(kept).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        let mut changed = IdentityMap::new(|id: &str| Ok(id.replace("#", "#changed-")));
        assert!(ObjectDeclaration::new(original.clone())
            .unwrap()
            .rewrite_identities(&ctx, &mut changed)
            .is_err());
        let mut with_dependency = original;
        with_dependency
            .dependencies
            .push("test:native:object#dependency".into());
        assert!(ObjectDeclaration::new(with_dependency).is_err());
    }
}
